//! Overlay canvas (drawn over the frozen frame): dim, then the selections (freehand
//! loops, dragged rectangles, tapped words) with a glowing gradient stroke, the
//! resize handles and the count badge. Also `View`: the zoomed / panned part of
//! the frame on screen. Selections live in image coordinates (the overlay's
//! logical px at 1x); events arrive in screen px and the App maps them.
use std::cell::Cell;
use std::time::{Duration, Instant};

use iced::alignment::Vertical;
use iced::keyboard::{self, key::Named};
use iced::mouse::{self, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Action, Fill, Geometry, Path, Stroke, fill, gradient, stroke};
use iced::widget::text::Alignment;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector};

use super::theme;

/// A selection smaller than this (screen px, either side) is a tap, not a circle.
pub const MIN_SIDE: f32 = 12.0;

/// A press-drag is a rectangle if no point strays further than this fraction of
/// the start→end distance from the start→end segment (a loop strays much further).
const DRAG_STRAY: f32 = 0.2;

/// A press that stays within this (screen px, either side) is a tap.
const TAP: f32 = 4.0;

/// Corner handles: drawn radius, and how near (screen px) a press grabs one.
const HANDLE: f32 = 5.0;
const HANDLE_HIT: f32 = 12.0;

/// Zoom per wheel notch, and the most it zooms in.
const ZOOM_STEP: f32 = 1.25;
const MAX_ZOOM: f32 = 4.0;
/// Wheel pixels per notch (touchpads report pixels).
const WHEEL_PX: f32 = 50.0;
/// Two right presses this close are a double right-click (back to 1x).
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// Left button down; `shift` = add a selection instead of replacing it.
    Press {
        at: Point,
        shift: bool,
    },
    Move(Point),
    /// The pointer moved with no button down (only when `Overlay::hover`).
    Hover(Point),
    Release,
    /// Enter: the default action.
    Enter,
    /// A letter key (Latin, lower-case, keyboard-layout independent), with Ctrl or not.
    Key {
        letter: char,
        ctrl: bool,
    },
    /// The wheel turned `notches` (away from the user = positive) at `at`.
    Zoom {
        at: Point,
        notches: f32,
    },
    /// Right or middle button down / the pointer moved while it is down: pan.
    PanStart(Point),
    PanMove(Point),
    /// Double right-click: back to 1x.
    ZoomReset,
}

/// How a selection was made (and so how its clear area is drawn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A freehand loop: the clear area is the drawn shape.
    Loop,
    /// A dragged (or resized) rectangle.
    Drag,
    /// A tapped word or range on a text line: a rounded highlight.
    Word,
}

/// What a finished stroke selects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    /// Image px. For a loop: its bounding box (the crop).
    pub area: Rectangle,
    pub kind: Kind,
}

/// The selection a stroke (image px, seen at `zoom`) means, or `None` for a tap
/// or a thin line.
pub fn select(stroke: &[Point], zoom: f32) -> Option<Selection> {
    let drag = is_drag(stroke);
    let area = if drag {
        drag_rect(stroke)
    } else {
        bounds(stroke)?
    };
    let min = MIN_SIDE / zoom;
    let kind = if drag { Kind::Drag } else { Kind::Loop };
    (area.width >= min && area.height >= min).then_some(Selection { area, kind })
}

/// A press that hardly moved (image px, seen at `zoom`).
pub fn is_tap(stroke: &[Point], zoom: f32) -> bool {
    bounds(stroke).is_some_and(|b| b.width.max(b.height) <= TAP / zoom)
}

/// A mostly straight press-drag that doesn't loop back: every point stays near the
/// segment from the first point to the last.
// ponytail: one threshold, no timing/speed; tune DRAG_STRAY if real strokes misfire.
fn is_drag(stroke: &[Point]) -> bool {
    let (Some(&a), Some(&b)) = (stroke.first(), stroke.last()) else {
        return false;
    };
    let chord = a.distance(b);
    chord > 0.0
        && stroke
            .iter()
            .all(|&p| to_segment(p, a, b) <= DRAG_STRAY * chord)
}

/// Distance from `p` to the segment `a`–`b` (`a` ≠ `b`).
fn to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (ab, ap) = (b - a, p - a);
    let t = ((ap.x * ab.x + ap.y * ab.y) / (ab.x * ab.x + ab.y * ab.y)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The rectangle a drag spans: first and last points are opposite corners.
fn drag_rect(stroke: &[Point]) -> Rectangle {
    let (Some(&a), Some(&b)) = (stroke.first(), stroke.last()) else {
        return Rectangle::default();
    };
    let min = Point::new(a.x.min(b.x), a.y.min(b.y));
    Rectangle::new(min, Size::new((a.x - b.x).abs(), (a.y - b.y).abs()))
}

/// The stroke's bounding box (`None` for no points).
fn bounds(stroke: &[Point]) -> Option<Rectangle> {
    let first = stroke.first()?;
    let (mut min, mut max) = (*first, *first);
    for p in stroke {
        min = Point::new(min.x.min(p.x), min.y.min(p.y));
        max = Point::new(max.x.max(p.x), max.y.max(p.y));
    }
    Some(Rectangle::new(min, Size::new(max.x - min.x, max.y - min.y)))
}

/// The stroke as quadratic curves `(control, end)` from `stroke[0]`, through the
/// segment midpoints: smooth, and never outside the stroke's bounding box.
fn smooth(stroke: &[Point]) -> Vec<(Point, Point)> {
    let mid = |a: Point, b: Point| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let mut curves: Vec<_> = stroke.windows(2).map(|w| (w[0], mid(w[0], w[1]))).collect();
    curves.extend(stroke.last().map(|&p| (p, p)));
    curves
}

/// Adds the smoothed stroke to `p`; `closed` joins its end back to its start.
fn trace(p: &mut canvas::path::Builder, stroke: &[Point], closed: bool) {
    let Some(&first) = stroke.first() else { return };
    p.move_to(first);
    smooth(stroke)
        .into_iter()
        .for_each(|(control, end)| p.quadratic_curve_to(control, end));
    if closed {
        p.close();
    }
}

/// Adds a selection's shape (its stroke in screen px) to `p`.
fn shape(p: &mut canvas::path::Builder, kind: Kind, stroke: &[Point], closed: bool) {
    let r = drag_rect(stroke);
    match kind {
        Kind::Drag => p.rectangle(r.position(), r.size()),
        Kind::Word => p.rounded_rectangle(r.position(), r.size(), (r.height / 4.0).min(8.0).into()),
        Kind::Loop => trace(p, stroke, closed),
    }
}

/// The corners of `r`: top-left, top-right, bottom-right, bottom-left.
fn corners(r: Rectangle) -> [Point; 4] {
    let (x1, y1) = (r.x + r.width, r.y + r.height);
    [
        r.position(),
        Point::new(x1, r.y),
        Point::new(x1, y1),
        Point::new(r.x, y1),
    ]
}

/// The corner handle of `area` (screen px) under `p`, as an index into `corners`.
pub fn handle_at(area: Rectangle, p: Point) -> Option<usize> {
    corners(area)
        .iter()
        .position(|c| c.distance(p) <= HANDLE_HIT)
}

/// `area` with its corner `corner` dragged to `to`. The opposite corner stays;
/// the dragged one keeps its side of it and at least `min` away (no flipping).
pub fn resize(area: Rectangle, corner: usize, to: Point, min: f32) -> Rectangle {
    let all = corners(area);
    let (moving, fixed) = (all[corner % 4], all[(corner + 2) % 4]);
    let side = |fixed: f32, was: f32, to: f32| {
        if was >= fixed {
            to.max(fixed + min)
        } else {
            to.min(fixed - min)
        }
    };
    let (x, y) = (side(fixed.x, moving.x, to.x), side(fixed.y, moving.y, to.y));
    Rectangle::new(
        Point::new(fixed.x.min(x), fixed.y.min(y)),
        Size::new((x - fixed.x).abs(), (y - fixed.y).abs()),
    )
}

/// `stroke` moved and scaled from the box `from` to the box `to`.
pub fn refit(stroke: &[Point], from: Rectangle, to: Rectangle) -> Vec<Point> {
    let ratio = |old: f32, new: f32| if old > 0.0 { new / old } else { 1.0 };
    let (sx, sy) = (ratio(from.width, to.width), ratio(from.height, to.height));
    let map = |p: &Point| Point::new(to.x + (p.x - from.x) * sx, to.y + (p.y - from.y) * sy);
    stroke.iter().map(map).collect()
}

/// What part of the frozen frame the overlay shows: all of it at 1x, a part of
/// it zoomed in. The shown part is whole frame pixels (what the image widget
/// crops), and the mapping uses exactly that, so a selection drawn at any zoom
/// crops what was under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The frame (physical px) and the overlay (logical px).
    frame: (u32, u32),
    screen: Size,
    /// The frame pixels shown.
    shown: Rectangle<u32>,
}

impl View {
    pub fn new(width: u32, height: u32, screen: Size) -> View {
        View {
            frame: (width, height),
            screen,
            shown: Rectangle {
                x: 0,
                y: 0,
                width: width.max(1),
                height: height.max(1),
            },
        }
    }

    /// Back to 1x.
    pub fn reset(self) -> View {
        View::new(self.frame.0, self.frame.1, self.screen)
    }

    pub fn zoom(&self) -> f32 {
        self.frame.0 as f32 / self.shown.width as f32
    }

    /// Frame px per image px, and screen px per shown frame px (per axis).
    fn ratios(&self) -> (Vector, Vector) {
        let (w, h) = (self.frame.0 as f32, self.frame.1 as f32);
        let px = Vector::new(w / self.screen.width, h / self.screen.height);
        let per = Vector::new(
            self.screen.width / self.shown.width as f32,
            self.screen.height / self.shown.height as f32,
        );
        (px, per)
    }

    /// A screen point in image px.
    pub fn to_image(self, p: Point) -> Point {
        let (px, per) = self.ratios();
        Point::new(
            (self.shown.x as f32 + p.x / per.x) / px.x,
            (self.shown.y as f32 + p.y / per.y) / px.y,
        )
    }

    /// An image point on screen.
    pub fn to_screen(self, q: Point) -> Point {
        let (px, per) = self.ratios();
        Point::new(
            (q.x * px.x - self.shown.x as f32) * per.x,
            (q.y * px.y - self.shown.y as f32) * per.y,
        )
    }

    pub fn rect_to_screen(&self, r: Rectangle) -> Rectangle {
        let a = self.to_screen(r.position());
        let b = self.to_screen(r.position() + Vector::new(r.width, r.height));
        Rectangle::new(a, Size::new(b.x - a.x, b.y - a.y))
    }

    /// The frame crop the image widget shows; `None` at 1x (all of it).
    pub fn crop(&self) -> Option<Rectangle<u32>> {
        (self.shown.width < self.frame.0 || self.shown.height < self.frame.1).then_some(self.shown)
    }

    /// Zoomed by `notches` wheel notches (1x–4x) around `at` (screen px): the
    /// frame pixel under the pointer stays under it.
    pub fn zoomed(&self, at: Point, notches: f32) -> View {
        let zoom = (self.zoom() * ZOOM_STEP.powf(notches)).clamp(1.0, MAX_ZOOM);
        let (fw, fh) = self.frame;
        let w = ((fw as f32 / zoom).round() as u32).clamp(1, fw);
        let h = ((fh as f32 / zoom).round() as u32).clamp(1, fh);
        let (_, per) = self.ratios();
        let under = |shown: u32, at: f32, per: f32, screen: f32, new: u32| {
            shown as f32 + at / per - at / screen * new as f32
        };
        let x = under(self.shown.x, at.x, per.x, self.screen.width, w);
        let y = under(self.shown.y, at.y, per.y, self.screen.height, h);
        self.place(x, y, w, h)
    }

    /// Moved by `by` screen px (the frame follows the pointer).
    pub fn panned(&self, by: Vector) -> View {
        let (_, per) = self.ratios();
        let (x, y) = (
            self.shown.x as f32 - by.x / per.x,
            self.shown.y as f32 - by.y / per.y,
        );
        self.place(x, y, self.shown.width, self.shown.height)
    }

    /// `w` x `h` frame px at (`x`, `y`), rounded, kept inside the frame.
    fn place(&self, x: f32, y: f32, width: u32, height: u32) -> View {
        let clamp = |v: f32, max: u32| (v.round().max(0.0) as u32).min(max);
        let shown = Rectangle {
            x: clamp(x, self.frame.0 - width),
            y: clamp(y, self.frame.1 - height),
            width,
            height,
        };
        View { shown, ..*self }
    }
}

pub struct Overlay<'a> {
    /// The stroke being drawn, or the current selection's (image px).
    pub stroke: &'a [Point],
    pub selection: Option<Selection>,
    /// Selections added before this one (Shift), with their strokes.
    pub others: &'a [(Selection, Vec<Point>)],
    pub view: View,
    /// Corner handles on `selection` (resize; drag inside moves it).
    pub handles: bool,
    /// Counts overlay openings: pointer state from an earlier opening is stale.
    pub opening: u32,
    pub first_draw: &'a Cell<Option<(Instant, Size)>>,
    /// Report moves without a button down too (snip's Window mode).
    pub hover: bool,
    /// Dim the frame (not over a full-screen translation).
    pub dim: bool,
}

/// The pointer as the canvas saw it, event by event. Events can arrive in one batch
/// (behind a slow frame): `cursor` and the app state are then already stale.
#[derive(Default)]
pub struct Pointer {
    opening: u32,
    at: Option<Point>,
    down: bool,
    /// Right or middle button held.
    panning: bool,
    /// The last right press that did not reset the zoom.
    right: Option<Instant>,
    shift: bool,
}

impl canvas::Program<Event> for Overlay<'_> {
    type State = Pointer;

    fn update(
        &self,
        pointer: &mut Pointer,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Event>> {
        if pointer.opening != self.opening {
            *pointer = Pointer {
                opening: self.opening,
                ..Pointer::default()
            };
        }
        let event = match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                pointer.shift = m.shift();
                return None;
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) => match key {
                keyboard::Key::Named(Named::Enter) => Event::Enter,
                _ => Event::Key {
                    letter: key.to_latin(*physical_key)?.to_ascii_lowercase(),
                    ctrl: modifiers.command(),
                },
            },
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let now = cursor.position_in(bounds)?; // None = over the action bar
                pointer.down = true;
                Event::Press {
                    at: pointer.at.unwrap_or(now),
                    shift: pointer.shift,
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(
                button @ (mouse::Button::Right | mouse::Button::Middle),
            )) => {
                let at = pointer.at.unwrap_or(cursor.position_in(bounds)?);
                let right = *button == mouse::Button::Right;
                let double = right && pointer.right.is_some_and(|t| t.elapsed() < DOUBLE_CLICK);
                pointer.right = (right && !double).then(Instant::now);
                if double {
                    Event::ZoomReset
                } else {
                    pointer.panning = true;
                    Event::PanStart(at)
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Right | mouse::Button::Middle,
            )) => {
                pointer.panning = false;
                return None;
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let at = cursor.position_in(bounds)?;
                let notches = match delta {
                    ScrollDelta::Lines { y, .. } => *y,
                    ScrollDelta::Pixels { y, .. } => *y / WHEEL_PX,
                };
                Event::Zoom { at, notches }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let at = *position - (bounds.position() - Point::ORIGIN);
                pointer.at = Some(at);
                match (pointer.down, pointer.panning, self.hover) {
                    (true, _, _) => Event::Move(at),
                    (false, true, _) => Event::PanMove(at),
                    (false, false, true) => Event::Hover(at),
                    (false, false, false) => return None,
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if pointer.down =>
            {
                pointer.down = false;
                Event::Release
            }
            _ => return None,
        };
        Some(Action::publish(event).and_capture())
    }

    fn draw(
        &self,
        _pointer: &Pointer,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let screen = Rectangle::with_size(bounds.size());
        let on_screen =
            |s: &[Point]| -> Vec<Point> { s.iter().map(|&p| self.view.to_screen(p)).collect() };
        let current = on_screen(self.stroke);
        // While drawing, a straight drag already shows as the rectangle it will select.
        let kind = (self.selection).map_or(
            if is_drag(&current) {
                Kind::Drag
            } else {
                Kind::Loop
            },
            |s| s.kind,
        );
        let others: Vec<(Kind, Vec<Point>)> = (self.others.iter())
            .map(|(s, stroke)| (s.kind, on_screen(stroke)))
            .collect();

        // After release, the clear region is the drawn shape itself (closed), not its
        // bounding box.
        // ponytail: even-odd re-dims areas circled twice (or two overlapping
        // selections); trim the stroke at its first self-crossing if users mind.
        let closed = self.selection.is_some();
        if closed || !others.is_empty() {
            let outside = Path::new(|p| {
                p.rectangle(Point::ORIGIN, screen.size());
                for (kind, s) in &others {
                    shape(p, *kind, s, true);
                }
                if closed {
                    shape(p, kind, &current, true);
                }
            });
            frame.fill(
                &outside,
                Fill {
                    style: canvas::Style::Solid(theme::DIM_OUTSIDE),
                    rule: fill::Rule::EvenOdd,
                },
            );
        } else if self.dim {
            frame.fill_rectangle(Point::ORIGIN, screen.size(), theme::DIM_IDLE);
        }

        for (kind, s) in &others {
            glow(&mut frame, *kind, s, true, screen);
        }
        glow(&mut frame, kind, &current, closed, screen);

        if let Some(sel) = self.selection {
            let r = self.view.rect_to_screen(sel.area);
            if self.handles {
                for c in corners(r) {
                    let dot = Path::circle(c, HANDLE);
                    frame.fill(&dot, Color::WHITE);
                    let ring = Stroke::default().with_color(theme::ACCENT_START);
                    frame.stroke(&dot, ring.with_width(2.0));
                }
            }
            // How many selections the actions get.
            if !self.others.is_empty() {
                let at = Point::new(r.x + r.width + 14.0, r.y - 14.0);
                frame.fill(&Path::circle(at, 11.0), theme::ACCENT_START);
                frame.fill_text(canvas::Text {
                    content: (self.others.len() + 1).to_string(),
                    position: at,
                    color: theme::ON_ACCENT,
                    size: 13.0.into(),
                    align_x: Alignment::Center,
                    align_y: Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        }

        // Logged once the window is really full-screen (WSLg maps it windowed first).
        if let Some((at, size)) = self.first_draw.get()
            && (bounds.width - size.width).abs() < 1.0
            && (bounds.height - size.height).abs() < 1.0
        {
            self.first_draw.set(None);
            eprintln!(
                "[opencircle] trigger -> first full-screen overlay frame drawn: {} ms",
                at.elapsed().as_millis()
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        pointer: &Pointer,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if pointer.panning {
            return mouse::Interaction::Grabbing;
        }
        if let (true, Some(sel), Some(p)) =
            (self.handles, self.selection, cursor.position_in(bounds))
        {
            let r = self.view.rect_to_screen(sel.area);
            match handle_at(r, p) {
                Some(0 | 2) => return mouse::Interaction::ResizingDiagonallyDown,
                Some(_) => return mouse::Interaction::ResizingDiagonallyUp,
                None if r.contains(p) => return mouse::Interaction::Grab,
                None => {}
            }
        }
        mouse::Interaction::Crosshair
    }
}

/// A selection's stroke (screen px): the glowing gradient line; a word is a
/// tinted highlight with a thin line.
fn glow(frame: &mut canvas::Frame, kind: Kind, stroke: &[Point], closed: bool, screen: Rectangle) {
    if stroke.len() < 2 {
        return;
    }
    let path = Path::new(|p| shape(p, kind, stroke, closed));
    let span = bounds(stroke).unwrap_or(screen);
    let line = |width, alpha| Stroke {
        style: gradient_style(span, alpha),
        width,
        line_cap: stroke::LineCap::Round,
        line_join: stroke::LineJoin::Round,
        ..Stroke::default()
    };
    if kind == Kind::Word {
        frame.fill(&path, theme::SELECTION);
        frame.stroke(&path, line(2.0, 1.0));
        return;
    }
    // Soft glow = the same stroke, wider and fainter, under the core line.
    for (width, alpha) in [(18.0, 0.10), (10.0, 0.22), (3.5, 1.0)] {
        frame.stroke(&path, line(width, alpha));
    }
}

fn gradient_style(span: Rectangle, alpha: f32) -> canvas::Style {
    let tint = |c: Color| c.scale_alpha(alpha);
    let linear = gradient::Linear::new(span.position(), span.position() + span.size().into())
        .add_stop(0.0, tint(theme::STROKE_START))
        .add_stop(1.0, tint(theme::STROKE_END));
    canvas::Style::Gradient(linear.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_fits_to_its_bounding_box() {
        let circle: Vec<Point> = (0..64)
            .map(|i| {
                let a = i as f32 / 64.0 * std::f32::consts::TAU;
                Point::new(200.0 + 50.0 * a.cos(), 100.0 + 30.0 * a.sin())
            })
            .collect();
        let sel = bounds(&circle).unwrap();
        assert!((sel.x - 150.0).abs() < 0.5 && (sel.y - 70.0).abs() < 0.5);
        assert!((sel.width - 100.0).abs() < 0.5 && (sel.height - 60.0).abs() < 0.5);
    }

    #[test]
    fn smoothed_stroke_stays_in_its_box_and_survives_odd_input() {
        // A figure eight: self-intersecting.
        let eight: Vec<Point> = (0..=64)
            .map(|i| {
                let a = i as f32 / 64.0 * std::f32::consts::TAU;
                Point::new(100.0 + 80.0 * a.sin(), 100.0 + 40.0 * (2.0 * a).sin())
            })
            .collect();
        let sel = bounds(&eight).unwrap();
        let curves = smooth(&eight);
        assert_eq!(curves.len(), eight.len());
        for (control, end) in &curves {
            for p in [control, end] {
                assert!(
                    sel.x - 0.01 <= p.x && p.x <= sel.x + sel.width + 0.01,
                    "{p:?}"
                );
                assert!(
                    sel.y - 0.01 <= p.y && p.y <= sel.y + sel.height + 0.01,
                    "{p:?}"
                );
            }
        }
        // Curves end at segment midpoints (not the raw points) and finish on the last point.
        let mid = Point::new(
            (eight[1].x + eight[2].x) / 2.0,
            (eight[1].y + eight[2].y) / 2.0,
        );
        assert_eq!(curves[1], (eight[1], mid));
        assert_eq!(curves.last(), Some(&(eight[64], eight[64])));

        // Degenerate strokes build a path without panicking.
        assert!(smooth(&[]).is_empty());
        let dot = [Point::new(5.0, 5.0)];
        for stroke in [&eight[..], &[], &dot, &[dot[0], dot[0]]] {
            let _ = Path::new(|p| trace(p, stroke, true));
        }
    }

    fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, turn: f32) -> Vec<Point> {
        (0..=64)
            .map(|i| {
                let a = i as f32 / 64.0 * turn * std::f32::consts::TAU;
                Point::new(cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect()
    }

    #[test]
    fn a_straight_drag_selects_the_rectangle_it_spans() {
        // Slow 45° mouse drag: a pixel staircase, up-left (end before start).
        let mut stairs = vec![Point::new(300.0, 200.0)];
        for i in 0..150 {
            let last = *stairs.last().unwrap();
            stairs.push(if i % 2 == 0 {
                Point::new(last.x - 1.0, last.y)
            } else {
                Point::new(last.x, last.y - 1.0)
            });
        }
        let sel = select(&stairs, 1.0).unwrap();
        assert_eq!(sel.kind, Kind::Drag);
        assert_eq!(
            sel.area,
            Rectangle::new(Point::new(225.0, 125.0), Size::new(75.0, 75.0))
        );

        // A wobbly, slightly bowed drag is still a drag.
        let wobbly: Vec<Point> = (0..=50)
            .map(|i| {
                let t = i as f32 / 50.0;
                let bow = 25.0 * (t * std::f32::consts::PI).sin();
                Point::new(100.0 + 300.0 * t + bow, 100.0 + 120.0 * t + (i % 3) as f32)
            })
            .collect();
        assert_eq!(select(&wobbly, 1.0).unwrap().kind, Kind::Drag);
    }

    #[test]
    fn loops_and_arcs_select_their_bounding_box() {
        for stroke in [
            ellipse(200.0, 100.0, 50.0, 30.0, 1.0),  // closed loop
            ellipse(200.0, 100.0, 50.0, 30.0, 0.85), // loop not quite closed
            ellipse(200.0, 100.0, 50.0, 50.0, 0.5),  // half circle (a "C")
        ] {
            let sel = select(&stroke, 1.0).unwrap();
            assert_eq!(sel.kind, Kind::Loop, "{:?}", stroke.last());
            assert_eq!(Some(sel.area), bounds(&stroke));
        }
    }

    #[test]
    fn tiny_strokes_are_ignored() {
        assert_eq!(bounds(&[]), None);
        assert_eq!(select(&[], 1.0), None);
        assert_eq!(select(&[Point::new(5.0, 5.0)], 1.0), None);
        // Long but flat: a line, not a circle.
        let (a, b) = (Point::new(0.0, 0.0), Point::new(300.0, 4.0));
        assert_eq!(
            select(&[a, Point::new(150.0, 2.0), Point::new(0.0, 3.0)], 1.0),
            None
        );
        // A flat drag is a line, not a rectangle.
        assert_eq!(select(&[a, b], 1.0), None);
        assert!(select(&[a, Point::new(12.0, 12.0)], 1.0).is_some());
        // Zoomed in 4x, a 12 px square on screen is 3 image px.
        let small = [a, Point::new(3.0, 3.0)];
        assert!(select(&small, 1.0).is_none() && select(&small, 4.0).is_some());
        // Taps: a press that hardly moved (on screen).
        assert!(is_tap(&[Point::new(5.0, 5.0)], 1.0));
        assert!(is_tap(&[Point::new(5.0, 5.0), Point::new(8.0, 7.0)], 1.0));
        assert!(!is_tap(&[Point::new(5.0, 5.0), Point::new(8.0, 7.0)], 4.0));
        assert!(!is_tap(&[], 1.0));
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn handles_are_the_four_corners() {
        let r = rect(100.0, 100.0, 200.0, 50.0);
        assert_eq!(handle_at(r, Point::new(101.0, 99.0)), Some(0));
        assert_eq!(handle_at(r, Point::new(308.0, 104.0)), Some(1));
        assert_eq!(handle_at(r, Point::new(300.0, 150.0)), Some(2));
        assert_eq!(handle_at(r, Point::new(95.0, 155.0)), Some(3));
        assert_eq!(
            handle_at(r, Point::new(200.0, 125.0)),
            None,
            "inside = move"
        );
        assert_eq!(
            handle_at(r, Point::new(130.0, 100.0)),
            None,
            "an edge, not a corner"
        );
    }

    #[test]
    fn resizing_keeps_the_opposite_corner_and_never_flips() {
        let r = rect(100.0, 100.0, 200.0, 50.0);
        // bottom-right out: top-left stays
        let to = Point::new(350.0, 180.0);
        assert_eq!(resize(r, 2, to, 12.0), rect(100.0, 100.0, 250.0, 80.0));
        // top-left in: bottom-right stays
        let to = Point::new(150.0, 110.0);
        assert_eq!(resize(r, 0, to, 12.0), rect(150.0, 110.0, 150.0, 40.0));
        // top-right dragged past the left edge: stops `min` short of it
        let to = Point::new(0.0, 0.0);
        assert_eq!(resize(r, 1, to, 12.0), rect(100.0, 0.0, 12.0, 150.0));
        // bottom-left dragged past everything
        let to = Point::new(900.0, 0.0);
        assert_eq!(resize(r, 3, to, 12.0), rect(288.0, 100.0, 12.0, 12.0));
    }

    #[test]
    fn a_resized_or_moved_loop_scales_with_its_box() {
        let lp = ellipse(200.0, 100.0, 50.0, 30.0, 1.0);
        let from = bounds(&lp).unwrap();
        // twice as wide, moved right: the loop's box is the new box
        let to = rect(from.x + 40.0, from.y, from.width * 2.0, from.height);
        let got = bounds(&refit(&lp, from, to)).unwrap();
        for (a, b) in [
            (got.x, to.x),
            (got.y, to.y),
            (got.width, to.width),
            (got.height, to.height),
        ] {
            assert!((a - b).abs() < 1e-3, "{got:?} vs {to:?}");
        }
        // a drag's two corners stay its corners
        let corners = [Point::new(10.0, 10.0), Point::new(60.0, 40.0)];
        let to = rect(0.0, 0.0, 100.0, 60.0);
        let fitted = refit(&corners, rect(10.0, 10.0, 50.0, 30.0), to);
        assert_eq!(fitted, [Point::ORIGIN, Point::new(100.0, 60.0)]);
        // a degenerate box only moves
        let dot = [Point::new(5.0, 5.0)];
        let moved = refit(&dot, rect(5.0, 5.0, 0.0, 0.0), rect(7.0, 9.0, 0.0, 0.0));
        assert_eq!(moved, [Point::new(7.0, 9.0)]);
    }

    /// 2x scale: the frame is 3840x2160, the overlay 1920x1080.
    fn view() -> View {
        View::new(3840, 2160, Size::new(1920.0, 1080.0))
    }

    fn near(a: Point, b: Point) -> bool {
        a.distance(b) < 0.01
    }

    #[test]
    fn at_1x_image_and_screen_are_the_same() {
        let v = view();
        assert_eq!(v.zoom(), 1.0);
        assert_eq!(v.crop(), None);
        let p = Point::new(123.5, 456.25);
        assert!(near(v.to_image(p), p) && near(v.to_screen(p), p));
        // zooming out below 1x, or panning at 1x, changes nothing
        assert_eq!(v.zoomed(p, -3.0), v);
        assert_eq!(v.panned(Vector::new(50.0, -20.0)), v);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer_and_stops_at_4x() {
        let v = view();
        let at = Point::new(600.0, 300.0);
        let under = v.to_image(at);
        let z = v.zoomed(at, 2.0);
        assert!((z.zoom() - 1.5625).abs() < 0.01, "{}", z.zoom());
        assert!(z.to_image(at).distance(under) < 0.5, "{:?}", z.to_image(at));
        // screen and image mappings are inverses at any zoom
        let q = Point::new(700.0, 333.0);
        assert!(near(z.to_image(z.to_screen(q)), q));
        let max = z.zoomed(at, 50.0);
        assert!((max.zoom() - 4.0).abs() < 1e-3);
        assert_eq!(max.reset(), v);
        // near the edge the view stays on the frame
        let corner = v.zoomed(Point::new(1919.0, 1079.0), 10.0);
        let c = corner.crop().unwrap();
        assert!(c.x + c.width <= 3840 && c.y + c.height <= 2160);
    }

    #[test]
    fn pan_follows_the_pointer_and_stays_on_the_frame() {
        let z = view().zoomed(Point::new(960.0, 540.0), 50.0); // 4x, centred
        let q = z.to_image(Point::new(500.0, 500.0));
        // dragging right by 100 screen px moves that image point right by 100
        let p = z.panned(Vector::new(100.0, 0.0));
        assert!(p.to_screen(q).distance(Point::new(600.0, 500.0)) < 0.5);
        // far past the edge: clamped to the frame's left/top
        let far = z.panned(Vector::new(10_000.0, 10_000.0));
        let c = far.crop().unwrap();
        assert_eq!((c.x, c.y), (0, 0));
        assert!(near(far.to_image(Point::ORIGIN), Point::ORIGIN));
    }

    /// 3840x2160 frame, pixel (x, y) = [x % 256, y % 256, x / 256, y / 256].
    fn coded_frame() -> Vec<u8> {
        let mut out = Vec::with_capacity(3840 * 2160 * 4);
        for y in 0..2160u32 {
            for x in 0..3840u32 {
                let [x0, x1, ..] = x.to_le_bytes();
                let [y0, y1, ..] = y.to_le_bytes();
                out.extend([x0, y0, x1, y1]);
            }
        }
        out
    }

    fn pixel_xy(px: &[u8]) -> (u32, u32) {
        (
            px[0] as u32 + px[2] as u32 * 256,
            px[1] as u32 + px[3] as u32 * 256,
        )
    }

    #[test]
    fn a_selection_drawn_zoomed_crops_what_was_under_it() {
        let frame = coded_frame();
        let v = view().zoomed(Point::new(800.0, 400.0), 3.0);
        let v = v.panned(Vector::new(-37.0, 21.0));
        let shown = v.crop().unwrap();
        // what the image widget draws at a screen point: the shown frame px, Fill-ed
        let drawn_at = |s: Point| {
            (
                shown.x + (s.x * shown.width as f32 / 1920.0) as u32,
                shown.y + (s.y * shown.height as f32 / 1080.0) as u32,
            )
        };
        // a drag from (400, 300) to (700, 520) on screen
        let (a, b) = (Point::new(400.0, 300.0), Point::new(700.0, 520.0));
        let sel = select(&[v.to_image(a), v.to_image(b)], v.zoom()).unwrap();
        let crop = crate::actions::crop(&frame, 3840, 2160, 2.0, sel.area).unwrap();
        let first = pixel_xy(&crop.rgba[..4]);
        let want = drawn_at(a);
        assert!(
            first.0.abs_diff(want.0) <= 1 && first.1.abs_diff(want.1) <= 1,
            "{first:?} vs {want:?}"
        );
        let last = pixel_xy(&crop.rgba[crop.rgba.len() - 4..]);
        let want = drawn_at(Point::new(b.x - 0.01, b.y - 0.01));
        assert!(
            last.0.abs_diff(want.0) <= 1 && last.1.abs_diff(want.1) <= 1,
            "{last:?} vs {want:?}"
        );
        // and it is the right size: the screen span over the zoom
        let z = v.zoom();
        let want = 300.0 * 2.0 / z;
        assert!(
            (crop.width as f32 - want).abs() <= 2.0,
            "{} at {z}",
            crop.width
        );
        // back on screen, the selection sits where it was drawn
        let r = v.rect_to_screen(sel.area);
        assert!(near(r.position(), a) && (r.width - 300.0).abs() < 0.01);
    }
}
