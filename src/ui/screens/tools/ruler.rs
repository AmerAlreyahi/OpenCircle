//! The screen ruler (N-76): measures in physical px (and, off 100 %, the
//! scaled value too). Modes: a line (any angle; Shift = 0 / 45 / 90°) with
//! its length and Δx / Δy; across and up-down, from the pointer to the
//! nearest strong colour change each way along that axis (a gap or a box's
//! width, live under the pointer); a box (W × H; its corners snap to window
//! and control edges as a snip's do). Ctrl+C copies the measure as text.
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Geometry};
use iced::widget::{Space, canvas as draw, pin, stack};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme};

use super::{CLOSE, Msg};
use crate::ui::kit::area;
use crate::ui::kit::chip;
use crate::ui::kit::divider;
use crate::ui::kit::icon_button::{self, State};
use crate::ui::kit::key_hint::Group;
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::sub;
use crate::ui::kit::tokens::{Role, radius, space};
use crate::ui::kit::tooltip::{Why, hint};
use crate::ui::screens::bar::geometry;

/// lucide "ruler" (the Snip toolbar's button), and the modes' glyphs:
/// "move-diagonal-2", "move-horizontal", "move-vertical", "square-dashed".
pub const ICON: char = '\u{e14e}';
pub const LINE: char = '\u{e1c4}';
pub const ACROSS: char = '\u{e1c5}';
pub const DOWN: char = '\u{e1c6}';
pub const BOX: char = '\u{e1ca}';
const DIVIDER_H: f32 = 24.0;
/// A neighbour pixel with a channel further than this from the pointer's
/// is an edge (0..255).
pub const TOLERANCE: u8 = 30;
/// The readout's distance from the pointer.
const READOUT_GAP: f32 = 16.0;
/// Room kept for the readout at the screen's right edge.
const READOUT_ROOM: f32 = 240.0;

/// The freeze hint while the ruler starts.
pub const HINT: [Group<'static>; 4] = [
    (None, "Drag to measure"),
    (Some("Shift"), "0° · 45° · 90°"),
    (Some("Ctrl+C"), "Copy"),
    (Some("Esc"), "Close"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Line,
    Across,
    Down,
    Box,
}

/// An axis an edge is looked for along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Across,
    Down,
}

/// The ruler's state: its mode and the span dragged (Line, Box), the
/// starting screen's logical px.
#[derive(Debug, Clone, Default)]
pub struct Ruler {
    pub mode: Mode,
    pub span: Option<(Point, Point)>,
    pub held: bool,
}

impl Ruler {
    pub fn set_mode(&mut self, mode: Mode) {
        *self = Ruler {
            mode,
            ..Ruler::default()
        };
    }

    /// Does this mode measure a drag (else: the pointer)?
    pub fn drags(&self) -> bool {
        matches!(self.mode, Mode::Line | Mode::Box)
    }

    /// The button went down at `p`: a new span starts there.
    pub fn press(&mut self, p: Point) {
        if self.drags() {
            (self.span, self.held) = (Some((p, p)), true);
        }
    }

    /// The pointer moved to `p` while held; `shift`: a line keeps to 0 / 45 / 90°.
    pub fn drag(&mut self, p: Point, shift: bool) {
        if let (true, Some((a, _))) = (self.held, self.span) {
            let b = match shift && self.mode == Mode::Line {
                true => constrain(a, p),
                false => p,
            };
            self.span = Some((a, b));
        }
    }

    pub fn release(&mut self) {
        self.held = false;
    }

    /// The measure: of the span (`px` puts a point in frame px), or at the
    /// pointer `at` (frame px) the run of like colour along the axis
    /// (`run`, as `ruler::run` over the frame).
    pub fn measure(
        &self,
        at: Option<(i32, i32)>,
        px: impl Fn(Point) -> (i32, i32),
        run: impl Fn((i32, i32), Axis) -> Option<(i32, i32)>,
    ) -> Option<Measure> {
        let (a, b) = match self.mode {
            Mode::Line | Mode::Box => {
                let (a, b) = self.span?;
                (px(a), px(b))
            }
            Mode::Across => {
                let at = at?;
                let (from, to) = run(at, Axis::Across)?;
                ((from, at.1), (to, at.1))
            }
            Mode::Down => {
                let at = at?;
                let (from, to) = run(at, Axis::Down)?;
                ((at.0, from), (at.0, to))
            }
        };
        Some(Measure {
            mode: self.mode,
            a,
            b,
            scale: 1.0,
            live: self.held,
        })
    }
}

/// A measure in frame px of the screen it is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    pub mode: Mode,
    pub a: (i32, i32),
    pub b: (i32, i32),
    /// That screen's physical px per logical px.
    pub scale: f32,
    /// Still being dragged.
    pub live: bool,
}

impl Measure {
    /// On a screen at `scale`.
    pub fn at(self, scale: f32) -> Measure {
        Measure { scale, ..self }
    }

    /// Δx, Δy (physical px, unsigned).
    pub fn deltas(&self) -> (i32, i32) {
        ((self.b.0 - self.a.0).abs(), (self.b.1 - self.a.1).abs())
    }

    /// The measure as the readout shows it and Ctrl+C copies it.
    pub fn text(&self) -> String {
        let (dx, dy) = self.deltas();
        let per = |v: i32| scaled(v, self.scale);
        let at = percent(self.scale);
        match self.mode {
            Mode::Line => {
                let len = (f64::from(dx).hypot(f64::from(dy))).round() as i32;
                let mut s = format!("{len} px · Δx {dx} · Δy {dy}");
                if let Some(at) = at {
                    s += &format!(" · {} at {at} %", per(len));
                }
                s
            }
            Mode::Across | Mode::Down => {
                let n = dx.max(dy);
                match at {
                    Some(at) => format!("{n} px · {} at {at} %", per(n)),
                    None => format!("{n} px"),
                }
            }
            Mode::Box => match at {
                Some(at) => format!("{dx} × {dy} px · {} × {} at {at} %", per(dx), per(dy)),
                None => format!("{dx} × {dy} px"),
            },
        }
    }
}

/// `px` physical px in the screen's scaled (logical) px.
pub fn scaled(px: i32, scale: f32) -> i32 {
    (px as f32 / scale).round() as i32
}

/// The scale in % when it isn't 100 %.
pub fn percent(scale: f32) -> Option<u32> {
    let at = (scale * 100.0).round() as u32;
    (at != 100).then_some(at)
}

/// `b` moved onto the nearest of the 8 directions from `a` (0 / 45 / 90°
/// and their turns): straight lines along an axis, or equal Δx and Δy.
pub fn constrain(a: Point, b: Point) -> Point {
    let d = b - a;
    if d.x == 0.0 && d.y == 0.0 {
        return b;
    }
    let turn = (d.y.atan2(d.x) / std::f32::consts::FRAC_PI_4).round() as i32;
    match turn.rem_euclid(4) {
        0 => Point::new(b.x, a.y),
        2 => Point::new(a.x, b.y),
        // a diagonal: as far along it as the drag goes
        k => {
            let (ux, uy) = (
                d.x.signum(),
                if k == 1 { d.x.signum() } else { -d.x.signum() },
            );
            let along = (d.x * ux + d.y * uy) / 2.0;
            Point::new(a.x + ux * along, a.y + uy * along)
        }
    }
}

/// From `at` (frame px) along `axis`: the run of pixels like the one there
/// (no channel more than `TOLERANCE` off), `[from, to)`; none off the frame.
pub fn run(rgba: &[u8], width: u32, height: u32, at: (i32, i32), axis: Axis) -> Option<(i32, i32)> {
    let colour = |x: i32, y: i32| super::picker::colour_at(rgba, width, height, (x, y));
    let start = colour(at.0, at.1)?;
    let like = |c: [u8; 3]| (0..3).all(|i| c[i].abs_diff(start[i]) <= TOLERANCE);
    let step = match axis {
        Axis::Across => (1, 0),
        Axis::Down => (0, 1),
    };
    let same = |n: i32| colour(at.0 + step.0 * n, at.1 + step.1 * n).is_some_and(like);
    let (mut back, mut on) = (0, 1);
    while same(back - 1) {
        back -= 1;
    }
    while same(on) {
        on += 1;
    }
    let base = match axis {
        Axis::Across => at.0,
        Axis::Down => at.1,
    };
    Some((base + back, base + on))
}

/// The measure drawn over one screen (`measure` in its frame px, `scale`),
/// its readout by the pointer `at` (logical px) on a `screen`-big window.
pub fn marks<'a>(
    measure: Option<Measure>,
    scale: f32,
    at: Point,
    screen: Size,
    look: (Color, Color),
) -> Element<'a, Msg> {
    let Some(m) = measure else {
        return Space::new().into();
    };
    let label = m.text();
    let readout = chip::readout(label, m.live, None);
    let x = match at.x + READOUT_ROOM > screen.width {
        true => (at.x - READOUT_ROOM).max(0.0),
        false => at.x + READOUT_GAP,
    };
    let y = match at.y + READOUT_GAP + chip::READOUT_H > screen.height {
        true => at.y - READOUT_GAP - chip::READOUT_H,
        false => at.y + READOUT_GAP,
    };
    stack![
        draw(Marks {
            measure: m,
            scale,
            look
        })
        .width(Fill)
        .height(Fill),
        pin(readout).position(Point::new(x, y)),
    ]
    .into()
}

/// The measure's lines (or box) on a canvas.
struct Marks {
    measure: Measure,
    scale: f32,
    look: (Color, Color),
}

impl canvas::Program<Msg> for Marks {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let Measure { mode, a, b, .. } = self.measure;
        let s = self.scale;
        // a pixel's middle, and a pixel's edge, in logical px
        let mid = |v: i32| (v as f32 + 0.5) / s;
        let edge = |v: i32| v as f32 / s;
        match mode {
            Mode::Line => {
                let (p, q) = (
                    Point::new(mid(a.0), mid(a.1)),
                    Point::new(mid(b.0), mid(b.1)),
                );
                area::span(&mut frame, p, q, self.look);
            }
            Mode::Across => {
                let y = mid(a.1);
                let (p, q) = (Point::new(edge(a.0), y), Point::new(edge(b.0), y));
                area::span(&mut frame, p, q, self.look);
            }
            Mode::Down => {
                let x = mid(a.0);
                let (p, q) = (Point::new(x, edge(a.1)), Point::new(x, edge(b.1)));
                area::span(&mut frame, p, q, self.look);
            }
            Mode::Box => {
                let lo = Point::new(edge(a.0.min(b.0)), edge(a.1.min(b.1)));
                let hi = Point::new(edge(a.0.max(b.0)), edge(a.1.max(b.1)));
                let size = Size::new(hi.x - lo.x, hi.y - lo.y);
                area::outline(&mut frame, Rectangle::new(lo, size), self.look);
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The toolbar: the four modes (`mode` chosen), the measure (`measure`), ×.
/// `rtl`: mirrored.
pub fn toolbar<'a>(mode: Mode, measure: Option<&Measure>, rtl: bool) -> Element<'a, Msg> {
    let ar = |latin: &'a str, arabic: &'a str| if rtl { arabic } else { latin };
    let side = iced::widget::tooltip::Position::Bottom;
    let modes = [
        (
            Mode::Line,
            LINE,
            ar("Line: any angle (Shift: 45°)", "خط: أي زاوية (Shift: 45°)"),
        ),
        (
            Mode::Across,
            ACROSS,
            ar("Across: to the nearest edges", "أفقي: إلى أقرب حافتين"),
        ),
        (
            Mode::Down,
            DOWN,
            ar("Up and down: to the nearest edges", "رأسي: إلى أقرب حافتين"),
        ),
        (
            Mode::Box,
            BOX,
            ar("Box: width × height", "مربع: العرض × الارتفاع"),
        ),
    ];
    let mut parts: Vec<Element<'a, Msg>> = (modes.into_iter())
        .map(|(m, glyph, tip)| {
            let state = State {
                active: m == mode,
                focused: false,
            };
            hint(
                icon_button::square(glyph, state, Some(Msg::Mode(m))),
                tip,
                Why::Icon,
                side,
            )
        })
        .collect();
    parts.push(divider::vertical(DIVIDER_H));
    parts.push(match measure {
        Some(m) => chip::readout(m.text(), m.live, None),
        None => {
            let what = match mode {
                Mode::Line | Mode::Box => ar("Drag to measure", "اسحب للقياس"),
                Mode::Across | Mode::Down => ar("Point at a gap", "أشر إلى فراغ"),
            };
            sub(Role::META, what).into()
        }
    });
    let close = icon_button::square(CLOSE, State::default(), Some(Msg::Close));
    parts.extend([
        divider::vertical(DIVIDER_H),
        hint(close, ar("Close", "إغلاق"), Why::Keys("Esc"), side),
    ]);
    if rtl {
        parts.reverse();
    }
    let line = iced::widget::Row::with_children(parts)
        .spacing(space::GAP)
        .padding([0.0, space::GAP])
        .align_y(iced::Center);
    surface(Material::Widget, radius::WINDOW, line)
        .padding(space::BAR_PAD)
        .height(geometry::HEIGHT)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` x `h` white frame with a `[x, y, w, h]` box of `c`.
    fn frame(w: u32, h: u32, b: [u32; 4], c: [u8; 3]) -> Vec<u8> {
        let mut px = vec![255u8; (w * h * 4) as usize];
        for y in b[1]..b[1] + b[3] {
            for x in b[0]..b[0] + b[2] {
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 3].copy_from_slice(&c);
            }
        }
        px
    }

    #[test]
    fn a_run_reaches_the_nearest_edges_each_way() {
        // a 200 x 120 box at (50, 40) on a 400 x 300 page
        let px = frame(400, 300, [50, 40, 200, 120], [30, 144, 255]);
        let inside = (120, 90);
        assert_eq!(run(&px, 400, 300, inside, Axis::Across), Some((50, 250)));
        assert_eq!(run(&px, 400, 300, inside, Axis::Down), Some((40, 160)));
        // the white gap left of the box: from the page's edge to the box
        assert_eq!(run(&px, 400, 300, (10, 90), Axis::Across), Some((0, 50)));
        assert_eq!(run(&px, 400, 300, (400, 90), Axis::Across), None);
        // a faint shade (anti-aliasing, a gradient step) is not an edge; a strong one is
        let mut soft = px.clone();
        let i = ((90 * 400 + 200) * 4) as usize;
        soft[i..i + 3].copy_from_slice(&[40, 150, 250]);
        assert_eq!(run(&soft, 400, 300, inside, Axis::Across), Some((50, 250)));
        soft[i..i + 3].copy_from_slice(&[30, 144, 160]);
        assert_eq!(run(&soft, 400, 300, inside, Axis::Across), Some((50, 200)));
    }

    #[test]
    fn shift_keeps_a_line_to_0_45_or_90_degrees() {
        let a = Point::new(100.0, 100.0);
        assert_eq!(
            constrain(a, Point::new(300.0, 120.0)),
            Point::new(300.0, 100.0)
        );
        assert_eq!(
            constrain(a, Point::new(90.0, -100.0)),
            Point::new(100.0, -100.0)
        );
        assert_eq!(
            constrain(a, Point::new(-50.0, 95.0)),
            Point::new(-50.0, 100.0)
        );
        // near a diagonal: equal Δx and Δy, as far as the drag went along it
        assert_eq!(
            constrain(a, Point::new(200.0, 190.0)),
            Point::new(195.0, 195.0)
        );
        assert_eq!(
            constrain(a, Point::new(0.0, 210.0)),
            Point::new(-5.0, 205.0)
        );
        assert_eq!(constrain(a, Point::new(10.0, 0.0)), Point::new(5.0, 5.0));
        assert_eq!(constrain(a, a), a);
        // every direction lands on a multiple of 45°
        for deg in (0..360).step_by(7) {
            let r = (deg as f32).to_radians();
            let b = constrain(
                a,
                Point::new(100.0 + 80.0 * r.cos(), 100.0 + 80.0 * r.sin()),
            );
            let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
            assert!(
                dx < 1e-3 || dy < 1e-3 || (dx - dy).abs() < 1e-3,
                "{deg}: {dx} {dy}"
            );
        }
    }

    #[test]
    fn measures_are_physical_px_with_the_scaled_value_off_100_percent() {
        let m = |mode, a, b, scale| Measure {
            mode,
            a,
            b,
            scale,
            live: false,
        };
        assert_eq!(m(Mode::Across, (50, 9), (250, 9), 1.0).text(), "200 px");
        assert_eq!(
            m(Mode::Down, (9, 40), (9, 340), 1.5).text(),
            "300 px · 200 at 150 %"
        );
        assert_eq!(
            m(Mode::Line, (0, 0), (300, 400), 1.0).text(),
            "500 px · Δx 300 · Δy 400"
        );
        assert_eq!(
            m(Mode::Line, (10, 10), (310, 10), 1.25).text(),
            "300 px · Δx 300 · Δy 0 · 240 at 125 %"
        );
        assert_eq!(
            m(Mode::Box, (250, 160), (50, 40), 1.0).text(),
            "200 × 120 px"
        );
        assert_eq!(
            m(Mode::Box, (0, 0), (300, 180), 1.5).text(),
            "300 × 180 px · 200 × 120 at 150 %"
        );
        assert_eq!(scaled(301, 1.5), 201);
        assert_eq!(percent(1.0), None);
        assert_eq!(percent(1.75), Some(175));
    }

    #[test]
    fn a_span_follows_the_drag_and_the_axis_modes_follow_the_pointer() {
        let id = |p: Point| (p.x as i32, p.y as i32);
        let none = |_: (i32, i32), _: Axis| None;
        let mut r = Ruler::default();
        r.press(Point::new(10.0, 10.0));
        r.drag(Point::new(110.0, 14.0), true); // Shift: level
        let m = r.measure(None, id, none).unwrap();
        assert_eq!((m.a, m.b, m.live), ((10, 10), (110, 10), true));
        r.release();
        assert!(!r.measure(None, id, none).unwrap().live);
        r.drag(Point::new(500.0, 500.0), false); // not held: kept
        assert_eq!(r.measure(None, id, none).unwrap().b, (110, 10));
        // a mode switch starts afresh; across measures at the pointer
        r.set_mode(Mode::Across);
        r.press(Point::new(0.0, 0.0));
        assert!(r.span.is_none());
        let runs = |at: (i32, i32), axis| (axis == Axis::Across).then_some((at.0 - 5, at.0 + 7));
        let m = r.measure(Some((40, 3)), id, runs).unwrap();
        assert_eq!((m.a, m.b), ((35, 3), (47, 3)));
        assert!(r.measure(None, id, runs).is_none());
    }
}
