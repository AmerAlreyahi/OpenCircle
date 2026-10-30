//! Snip's area chrome on a canvas: the outline round a picked or hovered area
//! and its 8 handles. Soft (owner, 2026-10-30): a thin line in the active
//! look's pair (`look`, the screen edge's and main mode's selection colours,
//! passed in: the kit reads no config) over a faint dark halo, so it reads on
//! white and on dark pages; small round handles in the look's first colour,
//! no white squares. `grown` (0..1) scales the handles in (the pick's 120 ms).
//! `span`: the screen ruler's measured line, in the same line and halo.
use iced::widget::canvas::{Frame, LineDash, Path, Stroke, Style, gradient};
use iced::{Color, Point, Rectangle, Size};

/// The line, logical px, and its halo's.
pub const LINE: f32 = 1.25;
const HALO: f32 = 3.25;
/// The halo's darkness.
const HALO_ALPHA: f32 = 0.22;
/// A corner handle's side, an edge handle's long and short side, logical px.
pub const CORNER: f32 = 8.0;
pub const EDGE: (f32, f32) = (14.0, 4.5);
/// A handle under the pointer or held grows by this.
const HOT: f32 = 2.0;
/// The soft ring round a handle under the pointer.
const RING: f32 = 3.5;

/// How a handle shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Rest,
    /// Under the pointer: a little bigger, a soft ring.
    Hot,
    /// Held (resizing): bigger, no ring.
    Held,
}

/// The outline of `r` (logical px; drawn on its edge, half a line each side),
/// in the look's pair `(start, end)` along its diagonal.
pub fn outline(frame: &mut Frame, r: Rectangle, look: (Color, Color)) {
    stroke(frame, r, look, &[]);
}

/// The dash and gap of `dashed`, logical px.
pub const DASH: [f32; 2] = [7.0, 5.0];

/// `outline`, dashed (Snip's long capture while it runs), drawn just OUTSIDE
/// `r`: the area itself is the live screen there.
pub fn dashed(frame: &mut Frame, r: Rectangle, look: (Color, Color)) {
    stroke(frame, r.expand(HALO / 2.0), look, &DASH);
}

fn stroke(frame: &mut Frame, r: Rectangle, look: (Color, Color), dash: &[f32]) {
    if r.width <= 0.0 || r.height <= 0.0 {
        return;
    }
    let path = Path::rectangle(r.position(), r.size());
    let halo = Color::from_rgba(0.008, 0.024, 0.09, HALO_ALPHA);
    frame.stroke(&path, Stroke::default().with_color(halo).with_width(HALO));
    let linear = gradient::Linear::new(r.position(), r.position() + r.size().into())
        .add_stop(0.0, look.0)
        .add_stop(1.0, look.1);
    frame.stroke(
        &path,
        Stroke {
            style: Style::Gradient(linear.into()),
            width: LINE,
            line_dash: LineDash {
                segments: dash,
                offset: 0,
            },
            ..Stroke::default()
        },
    );
}

/// The end ticks' half length (the screen ruler), logical px.
pub const TICK: f32 = 5.0;

/// The screen ruler's measure from `a` to `b` (logical px): the outline's
/// line and halo, with a tick across each end.
pub fn span(frame: &mut Frame, a: Point, b: Point, look: (Color, Color)) {
    let d = b - a;
    let len = (d.x * d.x + d.y * d.y).sqrt();
    // across the line (a dot measures nothing: a plain cross-free tick)
    let n = match len > 0.0 {
        true => iced::Vector::new(-d.y / len, d.x / len) * TICK,
        false => iced::Vector::new(0.0, TICK),
    };
    let path = Path::new(|p| {
        p.move_to(a);
        p.line_to(b);
        for end in [a, b] {
            p.move_to(end - n);
            p.line_to(end + n);
        }
    });
    let halo = Color::from_rgba(0.008, 0.024, 0.09, HALO_ALPHA);
    frame.stroke(&path, Stroke::default().with_color(halo).with_width(HALO));
    let linear = gradient::Linear::new(a, b)
        .add_stop(0.0, look.0)
        .add_stop(1.0, look.1);
    frame.stroke(
        &path,
        Stroke {
            style: Style::Gradient(linear.into()),
            width: LINE,
            ..Stroke::default()
        },
    );
}

/// The handle at `at` on an edge (`(sx, sy)`: -1, 0, 1 per axis as
/// `editor::area::Grip::Edge`; both non-zero = a corner).
pub fn handle(frame: &mut Frame, at: Point, (sx, sy): (i8, i8), shown: Handle, colour: Color) {
    let grow = if shown == Handle::Rest { 0.0 } else { HOT };
    let size = match (sx, sy) {
        (0, _) => Size::new(EDGE.0 + grow, EDGE.1 + grow / 2.0),
        (_, 0) => Size::new(EDGE.1 + grow / 2.0, EDGE.0 + grow),
        _ => Size::new(CORNER + grow, CORNER + grow),
    };
    knob(frame, at, size, shown, colour);
}

fn knob(frame: &mut Frame, at: Point, size: Size, shown: Handle, colour: Color) {
    let rect = |s: Size| Rectangle::new(Point::new(at.x - s.width / 2.0, at.y - s.height / 2.0), s);
    let round = |r: Rectangle| {
        Path::rounded_rectangle(r.position(), r.size(), (r.width.min(r.height) / 2.0).into())
    };
    if shown == Handle::Hot {
        let ring = rect(Size::new(size.width + 2.0 * RING, size.height + 2.0 * RING));
        frame.fill(&round(ring), colour.scale_alpha(0.22));
    }
    // a faint dark rim under it: visible on a page of the same colour
    let rim = rect(Size::new(size.width + 2.0, size.height + 2.0));
    frame.fill(&round(rim), Color::from_rgba(0.008, 0.024, 0.09, 0.28));
    frame.fill(&round(rect(size)), colour);
}

/// The 8 handles of `r` (`hot`: the one under the pointer or held, as `at`'s
/// edges), scaled in by `grown`. Edge handles hide on a side under 40 px.
pub fn handles(
    frame: &mut Frame,
    r: Rectangle,
    look: (Color, Color),
    hot: Option<((i8, i8), Handle)>,
    grown: f32,
) {
    if grown <= 0.0 {
        return;
    }
    for (sx, sy) in [
        (-1, -1),
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
    ] {
        let short = |s: i8, len: f32| s == 0 && len < 40.0;
        if short(sx, r.width) || short(sy, r.height) {
            continue;
        }
        let along = |s: i8, from: f32, len: f32| from + len * (f32::from(s) + 1.0) / 2.0;
        let at = Point::new(along(sx, r.x, r.width), along(sy, r.y, r.height));
        let shown = match hot {
            Some((edge, shown)) if edge == (sx, sy) => shown,
            _ => Handle::Rest,
        };
        let colour = look.0;
        if grown < 1.0 {
            let k = 0.6 + 0.4 * grown;
            let size = match (sx, sy) {
                (0, _) => Size::new(EDGE.0, EDGE.1),
                (_, 0) => Size::new(EDGE.1, EDGE.0),
                _ => Size::new(CORNER, CORNER),
            } * k;
            knob(frame, at, size, Handle::Rest, colour.scale_alpha(grown));
        } else {
            handle(frame, at, (sx, sy), shown, colour);
        }
    }
}
