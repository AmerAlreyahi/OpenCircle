//! Overlay canvas (drawn over the frozen frame): dim, then the selections (freehand
//! loops, dragged rectangles, tapped words) with a glowing gradient stroke, the
//! resize handles and the count badge. A snapped selection (`[selection] style =
//! "snap"`) morphs from the drawn stroke into a rounded box with corner brackets
//! (they resize it); in select-text mode every word in it is highlighted. Also `View`: the zoomed / panned part of
//! the frame on screen. Selections live in image coordinates (the overlay's
//! logical px at 1x); events arrive in screen px and the App maps them.
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use iced::alignment::Vertical;
use iced::keyboard::{self, key::Named};
use iced::mouse::{self, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Action, Fill, Geometry, Path, Stroke, fill, gradient, stroke};
use iced::widget::image::{FilterMethod, Handle};
use iced::widget::text::Alignment;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector};

use super::kit::{motion, tokens::fixed};
use super::theme;
use crate::config::{BracketStyle, Look};

/// A selection smaller than this (screen px, either side) is a tap, not a circle.
pub const MIN_SIDE: f32 = 12.0;

/// A press-drag is a rectangle if no point strays further than this fraction of
/// the start→end distance from the start→end segment (a loop strays much further).
const DRAG_STRAY: f32 = 0.2;

/// A press that stays within this (screen px, either side) is a tap.
pub const TAP: f32 = 4.0;

/// Scribble: the path is sampled every `TURN_STEP` px (screen), a corner is a
/// heading change over two samples of more than `TURN_ANGLE` degrees, and a
/// stroke with `SCRIBBLE_TURNS` of them zig-zags (a loop turns smoothly).
const TURN_STEP: f32 = 8.0;
const TURN_ANGLE: f32 = 100.0;
const SCRIBBLE_TURNS: usize = 3;
/// A press that began on text is a circle after all once it turns this far.
const LOOP_TURN: f32 = 300.0;
/// How far (screen px) a scribble's clear area reaches past the stroke's hull.
const SCRIBBLE_PAD: f32 = 14.0;

/// Corner handles: drawn radius, and how near (screen px) a press grabs one.
const HANDLE: f32 = 5.0;
const HANDLE_HIT: f32 = 12.0;

/// A snapped box: its corner radius (at most), how long the stroke takes to
/// become it, and its corner brackets (gap outside the box, arm length range,
/// line width; screen px).
const SNAP_RADIUS: f32 = 16.0;
pub const MORPH: Duration = motion::GLIDE;
/// The overlay's opening: the window fades in, the edge glow draws back to its
/// rest and the bars slide in, as one eased animation from the first frame.
pub const APPEAR: Duration = motion::OPEN;
const BRACKET_GAP: f32 = 3.0;
const BRACKET_ARM: (f32, f32) = (14.0, 36.0);
const BRACKET_WIDTH: f32 = 4.0;
/// Points round a morphing outline.
const MORPH_POINTS: usize = 96;

/// Zoom per wheel notch, and the most it zooms in.
const ZOOM_STEP: f32 = 1.25;
const MAX_ZOOM: f32 = 4.0;
/// Wheel pixels per notch (touchpads report pixels).
const WHEEL_PX: f32 = 50.0;
/// Two right presses this close are a double right-click (back to 1x).
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Frame times kept for one log line at most (`Overlay::frame_times`).
pub const FRAME_LOG: usize = 4096;

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
    /// Space: activates the bar's focused control (nothing else).
    Space,
    /// Tab / Shift+Tab (`back`): moves focus through the bar.
    Tab {
        back: bool,
    },
    /// A letter key (Latin, lower-case, keyboard-layout independent), with Ctrl or not.
    Key {
        letter: char,
        ctrl: bool,
    },
    /// An arrow key: `(dx, dy)` = -1, 0 or 1 per axis. The box moves; with Ctrl
    /// it grows / shrinks from its bottom-right edge; Shift = 10 px, else 1.
    Arrow {
        dx: i8,
        dy: i8,
        ctrl: bool,
        shift: bool,
    },
    /// Alt went down / up: with it a press inside the box moves it, even over text.
    Alt(bool),
    /// The wheel turned `notches` (away from the user = positive) at `at`.
    Zoom {
        at: Point,
        notches: f32,
    },
    /// Right or middle button down / the pointer moved while it is down: pan.
    PanStart(Point),
    PanMove(Point),
    /// That button let go.
    PanEnd,
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
    /// Tapped or dragged words, one rounded highlight per text line.
    Word,
    /// A zig-zag over an area: the clear area is its padded convex hull.
    Scribble,
    /// Snapped to a rounded box with corner brackets; the stroke is its
    /// top-left and bottom-right corners.
    Snap,
}

/// What a press-drag-release means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Hardly moved.
    Tap,
    /// Mostly straight: the rectangle it spans.
    Drag,
    /// Open, zig-zagging: the area it scribbles over.
    Scribble,
    /// Anything else: the loop's shape.
    Loop,
}

/// What a finished stroke selects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    /// Image px. For a loop: its bounding box (the crop).
    pub area: Rectangle,
    pub kind: Kind,
}

/// What a stroke (image px, seen at `zoom`) is, checked in this order:
/// - tap: it stays within `TAP`;
/// - scribble: at least `SCRIBBLE_TURNS` sharp corners (see `sharp_turns`). Before
///   the drag test, because a back-and-forth over a line ends near a straight chord;
/// - drag: every point stays near the segment from the first point to the last;
/// - loop: anything else (a hook or a Z has too few corners to scribble).
pub fn classify(stroke: &[Point], zoom: f32) -> Gesture {
    if is_tap(stroke, zoom) {
        Gesture::Tap
    } else if sharp_turns(stroke, zoom) >= SCRIBBLE_TURNS {
        Gesture::Scribble
    } else if is_drag(stroke) {
        Gesture::Drag
    } else {
        Gesture::Loop
    }
}

/// The selection a stroke (image px, seen at `zoom`) means, or `None` for a tap
/// or a thin line.
pub fn select(stroke: &[Point], zoom: f32) -> Option<Selection> {
    settle(stroke, zoom, Size::new(f32::MAX, f32::MAX)).map(|(sel, _)| sel)
}

/// `select`, and the stroke the selection keeps: the stroke itself, or for a
/// scribble its padded hull (so the clear area, handles and resizing all follow
/// that shape, kept inside the `screen`). A scribble's area is the hull's box, so
/// it holds the whole clear area; it may be thin (a scribble over one line of text).
pub fn settle(stroke: &[Point], zoom: f32, screen: Size) -> Option<(Selection, Vec<Point>)> {
    let min = MIN_SIDE / zoom;
    let (area, kind, kept) = match classify(stroke, zoom) {
        Gesture::Tap => return None,
        Gesture::Drag => (drag_rect(stroke), Kind::Drag, stroke.to_vec()),
        Gesture::Loop => (bounds(stroke)?, Kind::Loop, stroke.to_vec()),
        Gesture::Scribble => {
            let mut hull = padded_hull(stroke, SCRIBBLE_PAD / zoom);
            for p in &mut hull {
                *p = Point::new(p.x.clamp(0.0, screen.width), p.y.clamp(0.0, screen.height));
            }
            let area = bounds(&hull)?;
            let thin = bounds(stroke)?;
            return (thin.width.max(thin.height) >= min).then_some((
                Selection {
                    area,
                    kind: Kind::Scribble,
                },
                hull,
            ));
        }
    };
    (area.width >= min && area.height >= min).then_some((Selection { area, kind }, kept))
}

/// A press that hardly moved (image px, seen at `zoom`).
pub fn is_tap(stroke: &[Point], zoom: f32) -> bool {
    bounds(stroke).is_some_and(|b| b.width.max(b.height) <= TAP / zoom)
}

/// How many sharp corners the path has (see `TURN_STEP`). Samples within the
/// stroke's own points, so a coarse stroke (fast mouse) still counts its turns.
fn sharp_turns(stroke: &[Point], zoom: f32) -> usize {
    let step = TURN_STEP / zoom;
    let mut pts: Vec<Point> = stroke.first().copied().into_iter().collect();
    for &p in stroke {
        if pts.last().is_some_and(|q| q.distance(p) >= step) {
            pts.push(p);
        }
    }
    let cos_limit = TURN_ANGLE.to_radians().cos();
    let (mut turns, mut i) = (0, 2);
    while i + 2 < pts.len() {
        let (a, b) = (pts[i] - pts[i - 2], pts[i + 2] - pts[i]);
        let cos = (a.x * b.x + a.y * b.y) / (a.x.hypot(a.y) * b.x.hypot(b.y));
        if cos < cos_limit {
            turns += 1;
            i += 3; // the rest of this corner
        } else {
            i += 1;
        }
    }
    turns
}

/// The convex hull of `points` grown by `pad` all round (corners rounded), as a
/// polygon.
fn padded_hull(points: &[Point], pad: f32) -> Vec<Point> {
    let rounded: Vec<Point> = hull(points)
        .iter()
        .flat_map(|c| {
            (0..16).map(move |i| {
                let a = i as f32 / 16.0 * std::f32::consts::TAU;
                Point::new(c.x + pad * a.cos(), c.y + pad * a.sin())
            })
        })
        .collect();
    hull(&rounded)
}

/// Convex hull (Andrew's monotone chain), counter-clockwise on screen.
fn hull(points: &[Point]) -> Vec<Point> {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross =
        |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut out: Vec<Point> = Vec::new();
    for pass in 0..2 {
        let from = out.len();
        for &p in &pts {
            while out.len() >= from + 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0 {
                out.pop();
            }
            out.push(p);
        }
        out.pop(); // the end is the next pass's start
        if pass == 0 {
            pts.reverse();
        }
    }
    out
}

/// A loop that is clearly a circle: the path's heading turns at least
/// `LOOP_TURN` degrees one way (sampled every `TURN_STEP` px on screen). A drag
/// along text that runs to a line's end and back to the next line's start
/// turns about half that; a zig-zag's turns cancel out.
pub fn winds(stroke: &[Point], zoom: f32) -> bool {
    let step = TURN_STEP / zoom;
    let mut pts: Vec<Point> = stroke.first().copied().into_iter().collect();
    for &p in stroke {
        if pts.last().is_some_and(|q| q.distance(p) >= step) {
            pts.push(p);
        }
    }
    let heading = |w: &[Point]| (w[1].y - w[0].y).atan2(w[1].x - w[0].x);
    let headings: Vec<f32> = pts.windows(2).map(heading).collect();
    let turned: f32 = (headings.windows(2))
        .map(|h| {
            let d = h[1] - h[0];
            (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
        })
        .sum();
    turned.abs().to_degrees() >= LOOP_TURN
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

/// How the stroke (screen px) shows: a selection as its kind; while drawing
/// (no selection) only the open stroke, since the first points of a circle look
/// like a straight drag. With `live_drag`, a straight drag at least `MIN_SIDE`
/// both ways shows as its rectangle.
fn shown_kind(stroke: &[Point], selection: Option<Selection>, live_drag: bool) -> Kind {
    match selection {
        Some(s) => s.kind,
        None if live_drag && is_drag(stroke) => {
            let r = drag_rect(stroke);
            match r.width.min(r.height) >= MIN_SIDE {
                true => Kind::Drag,
                false => Kind::Loop,
            }
        }
        None => Kind::Loop,
    }
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

/// Adds a selection's shape (its stroke in screen px) to `p`. A word selection's
/// stroke is a (top-left, bottom-right) pair per text line.
fn shape(p: &mut canvas::path::Builder, kind: Kind, stroke: &[Point], closed: bool) {
    match kind {
        Kind::Drag => {
            let r = drag_rect(stroke);
            p.rectangle(r.position(), r.size());
        }
        Kind::Snap => {
            let r = drag_rect(stroke);
            p.rounded_rectangle(r.position(), r.size(), snap_radius(r).into());
        }
        Kind::Word => {
            for pair in stroke.as_chunks::<2>().0 {
                let r = drag_rect(&pair[..]);
                p.rectangle(r.position(), r.size());
            }
        }
        Kind::Loop => trace(p, stroke, closed),
        Kind::Scribble => {
            if let Some((&first, rest)) = stroke.split_first() {
                p.move_to(first);
                rest.iter().for_each(|&q| p.line_to(q));
                p.close();
            }
        }
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

/// Corner `i` of `r` (top-left, top-right, bottom-right, bottom-left).
pub fn corner(r: Rectangle, i: usize) -> Point {
    corners(r)[i % 4]
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

/// A stroke's selection snapped: the box `area` (image px) as a snapped
/// selection and its stroke (two corners).
pub fn snap(area: Rectangle) -> (Selection, Vec<Point>) {
    let sel = Selection {
        area,
        kind: Kind::Snap,
    };
    (
        sel,
        vec![area.position(), area.position() + Vector::from(area.size())],
    )
}

/// A snapped box's corner radius (screen px).
pub fn snap_radius(r: Rectangle) -> f32 {
    SNAP_RADIUS.min(r.width / 4.0).min(r.height / 4.0).max(0.0)
}

/// How far a morph started at `start` is at `now` (0..1, eased out).
pub fn morph_progress(start: Instant, now: Instant) -> f32 {
    motion::progress(start, now, MORPH, motion::Curve::Out)
}

/// `n` points round the outline of `r` with corners rounded by `radius`,
/// clockwise on screen from the top edge's left end.
pub fn rounded_outline(r: Rectangle, radius: f32, n: usize) -> Vec<Point> {
    let rad = radius.min(r.width / 2.0).min(r.height / 2.0).max(0.0);
    let (w, h) = (r.width - 2.0 * rad, r.height - 2.0 * rad);
    let arc = std::f32::consts::FRAC_PI_2 * rad;
    let total = 2.0 * (w + h) + 4.0 * arc;
    // the four sides and four arcs in order: (length, start, direction / arc centre)
    let at = |mut s: f32| -> Point {
        let corner = |cx: f32, cy: f32, from: f32, s: f32| {
            let a = from + s / rad.max(1e-6);
            Point::new(cx + rad * a.cos(), cy + rad * a.sin())
        };
        let (x0, y0, x1, y1) = (r.x + rad, r.y + rad, r.x + rad + w, r.y + rad + h);
        use std::f32::consts::{FRAC_PI_2, PI};
        if s < w {
            return Point::new(x0 + s, r.y);
        }
        s -= w;
        if s < arc {
            return corner(x1, y0, -FRAC_PI_2, s);
        }
        s -= arc;
        if s < h {
            return Point::new(r.x + r.width, y0 + s);
        }
        s -= h;
        if s < arc {
            return corner(x1, y1, 0.0, s);
        }
        s -= arc;
        if s < w {
            return Point::new(x1 - s, r.y + r.height);
        }
        s -= w;
        if s < arc {
            return corner(x0, y1, FRAC_PI_2, s);
        }
        s -= arc;
        if s < h {
            return Point::new(r.x, y1 - s);
        }
        s -= h;
        corner(x0, y0, PI, s.min(arc))
    };
    (0..n).map(|i| at(total * i as f32 / n as f32)).collect()
}

/// `stroke` as a closed outline of `n` points evenly spaced along it, clockwise
/// on screen, starting nearest to `start`.
fn resample(stroke: &[Point], n: usize, start: Point) -> Vec<Point> {
    let Some(&first) = stroke.first() else {
        return vec![start; n];
    };
    let mut pts = stroke.to_vec();
    pts.push(first); // closed
    // anticlockwise on screen (negative area in y-down coordinates): reverse
    let area: f32 = pts
        .windows(2)
        .map(|w| w[0].x * w[1].y - w[1].x * w[0].y)
        .sum();
    if area < 0.0 {
        pts.reverse();
    }
    let lengths: Vec<f32> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 {
        return vec![first; n];
    }
    let mut out = Vec::with_capacity(n);
    let (mut seg, mut done) = (0, 0.0);
    for i in 0..n {
        let want = total * i as f32 / n as f32;
        while seg + 1 < lengths.len() && done + lengths[seg] < want {
            done += lengths[seg];
            seg += 1;
        }
        let k = if lengths[seg] > 0.0 {
            ((want - done) / lengths[seg]).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(pts[seg] + (pts[seg + 1] - pts[seg]) * k);
    }
    let nearest = (0..n)
        .min_by(|&a, &b| out[a].distance(start).total_cmp(&out[b].distance(start)))
        .unwrap_or(0);
    out.rotate_left(nearest);
    out
}

/// The outline `t` (0..1) of the way from the drawn `stroke` to the snapped
/// box `to` (both screen px).
pub fn morph(stroke: &[Point], to: Rectangle, t: f32) -> Vec<Point> {
    let target = rounded_outline(to, snap_radius(to), MORPH_POINTS);
    let from = resample(stroke, MORPH_POINTS, target[0]);
    (from.iter().zip(&target))
        .map(|(&a, &b)| a + (b - a) * t.clamp(0.0, 1.0))
        .collect()
}

/// A bracket's arm length on the box `r` (screen px).
fn bracket_arm(r: Rectangle) -> f32 {
    (r.width.min(r.height) / 4.0).clamp(BRACKET_ARM.0, BRACKET_ARM.1)
}

/// The corner bracket of the snapped box `area` (screen px) under `p`, as an
/// index into `corners`: on (or near) either arm of the L.
pub fn bracket_at(area: Rectangle, p: Point) -> Option<usize> {
    let r = area.expand(BRACKET_GAP);
    let arm = bracket_arm(area);
    corners(r).iter().position(|&c| {
        let (sx, sy) = (
            if c.x > r.center_x() { -1.0 } else { 1.0 },
            if c.y > r.center_y() { -1.0 } else { 1.0 },
        );
        let along_x = to_segment(p, c, c + Vector::new(sx * arm, 0.0));
        let along_y = to_segment(p, c, c + Vector::new(0.0, sy * arm));
        along_x.min(along_y) <= HANDLE_HIT
    })
}

/// What a press on a selection's border grabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// A corner (index into `corners`).
    Corner(usize),
    /// An edge: 0 top, 1 right, 2 bottom, 3 left.
    Edge(usize),
}

/// The edge of `area` (screen px) under `p`: within `HANDLE_HIT` of it, but
/// never more than a third of the side in, so a small box keeps a middle to move.
pub fn edge_at(area: Rectangle, p: Point) -> Option<usize> {
    let [a, b, c, d] = corners(area);
    let reach = |side: f32| HANDLE_HIT.min(side / 3.0);
    let (rx, ry) = (reach(area.width), reach(area.height));
    [(a, b, ry), (b, c, rx), (d, c, ry), (a, d, rx)]
        .iter()
        .position(|&(from, to, reach)| to_segment(p, from, to) <= reach)
}

/// `area` with its edge `edge` dragged to `to` (the other three stay), at
/// least `min` thick.
pub fn resize_edge(area: Rectangle, edge: usize, to: Point, min: f32) -> Rectangle {
    let (mut x0, mut y0) = (area.x, area.y);
    let (mut x1, mut y1) = (area.x + area.width, area.y + area.height);
    match edge {
        0 => y0 = to.y.min(y1 - min),
        1 => x1 = to.x.max(x0 + min),
        2 => y1 = to.y.max(y0 + min),
        _ => x0 = to.x.min(x1 - min),
    }
    Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
}

/// `area` slid back inside the `screen` box (a bigger one just fits its origin).
pub fn keep_inside(area: Rectangle, screen: Size) -> Rectangle {
    let slide = |at: f32, len: f32, max: f32| at.min(max - len).max(0.0);
    Rectangle::new(
        Point::new(
            slide(area.x, area.width, screen.width),
            slide(area.y, area.height, screen.height),
        ),
        area.size(),
    )
}

/// The grip of a selection of `kind` at `area` (screen px) under `p`: a snapped
/// box's brackets, otherwise its corner handles, then the edges.
pub fn grip_at(kind: Kind, area: Rectangle, p: Point) -> Option<Grip> {
    let corner = match kind {
        Kind::Snap => bracket_at(area, p),
        _ => handle_at(area, p),
    };
    corner
        .map(Grip::Corner)
        .or_else(|| edge_at(area, p).map(Grip::Edge))
}

/// The four corner brackets of the snapped box `r` (screen px), `alpha` shown.
fn brackets(frame: &mut canvas::Frame, r: Rectangle, look: &Look, alpha: f32) {
    let colour = theme::bracket(look).scale_alpha(alpha);
    let shadow = fixed::BRACKET_SHADOW.scale_alpha(alpha);
    let line = |colour, width| Stroke {
        style: canvas::Style::Solid(colour),
        width,
        line_cap: stroke::LineCap::Round,
        line_join: stroke::LineJoin::Round,
        ..Stroke::default()
    };
    let out = r.expand(BRACKET_GAP);
    let rad = snap_radius(r) + BRACKET_GAP;
    let path = match look.bracket_style {
        BracketStyle::Frame => Path::rounded_rectangle(out.position(), out.size(), rad.into()),
        BracketStyle::Corners => {
            let arm = bracket_arm(r).max(rad + 2.0);
            Path::new(|p| {
                for c in corners(out) {
                    let sx = if c.x > out.center_x() { -1.0 } else { 1.0 };
                    let sy = if c.y > out.center_y() { -1.0 } else { 1.0 };
                    p.move_to(c + Vector::new(0.0, sy * arm));
                    p.line_to(c + Vector::new(0.0, sy * rad));
                    p.quadratic_curve_to(c, c + Vector::new(sx * rad, 0.0));
                    p.line_to(c + Vector::new(sx * arm, 0.0));
                }
            })
        }
    };
    let width = match look.bracket_style {
        BracketStyle::Frame => 2.0,
        BracketStyle::Corners => BRACKET_WIDTH,
    };
    frame.stroke(&path, line(shadow, width + 3.0));
    frame.stroke(&path, line(colour, width));
}

/// What part of the frozen frame the overlay shows: all of it at 1x, a part of
/// it zoomed in. At rest the shown part is whole frame pixels (what the image
/// widget crops), and the mapping uses exactly that, so a selection drawn at
/// any zoom crops what was under it; while the wheel zoom eases (`lerp`) it
/// is fractional (drawn by `Zoomed`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The frame (physical px) and the overlay (logical px).
    frame: (u32, u32),
    screen: Size,
    /// The frame pixels shown.
    shown: Rectangle,
}

impl View {
    pub fn new(width: u32, height: u32, screen: Size) -> View {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        View {
            frame: (width, height),
            screen,
            shown: Rectangle::new(Point::ORIGIN, Size::new(w, h)),
        }
    }

    /// Another screen's window (`app::screens`), `screen` logical px: image px
    /// `u` show at `u * k + at`. (A 1x1 "frame" whose shown part is placed so
    /// the usual mapping is exactly that; never zoomed.)
    pub fn mapped(screen: Size, k: f32, at: Vector) -> View {
        View {
            frame: (1, 1),
            screen,
            shown: Rectangle {
                x: -at.x / (screen.width * k),
                y: -at.y / (screen.height * k),
                width: 1.0 / k,
                height: 1.0 / k,
            },
        }
    }

    /// Back to 1x.
    pub fn reset(self) -> View {
        View::new(self.frame.0, self.frame.1, self.screen)
    }

    pub fn zoom(&self) -> f32 {
        self.frame.0 as f32 / self.shown.width
    }

    /// Frame px per image px, and screen px per shown frame px (per axis).
    fn ratios(&self) -> (Vector, Vector) {
        let (w, h) = (self.frame.0 as f32, self.frame.1 as f32);
        let px = Vector::new(w / self.screen.width, h / self.screen.height);
        let per = Vector::new(
            self.screen.width / self.shown.width,
            self.screen.height / self.shown.height,
        );
        (px, per)
    }

    /// A screen point in image px.
    pub fn to_image(self, p: Point) -> Point {
        let (px, per) = self.ratios();
        Point::new(
            (self.shown.x + p.x / per.x) / px.x,
            (self.shown.y + p.y / per.y) / px.y,
        )
    }

    /// An image point on screen.
    pub fn to_screen(self, q: Point) -> Point {
        let (px, per) = self.ratios();
        Point::new(
            (q.x * px.x - self.shown.x) * per.x,
            (q.y * px.y - self.shown.y) * per.y,
        )
    }

    pub fn rect_to_screen(&self, r: Rectangle) -> Rectangle {
        let a = self.to_screen(r.position());
        let b = self.to_screen(r.position() + Vector::new(r.width, r.height));
        Rectangle::new(a, Size::new(b.x - a.x, b.y - a.y))
    }

    /// The frame crop the image widget shows; `None` at 1x (all of it).
    pub fn crop(&self) -> Option<Rectangle<u32>> {
        let s = self.shown;
        let whole = Rectangle {
            x: s.x.round() as u32,
            y: s.y.round() as u32,
            width: s.width.round() as u32,
            height: s.height.round() as u32,
        };
        (whole.width < self.frame.0 || whole.height < self.frame.1).then_some(whole)
    }

    /// The whole frame as drawn on screen (logical px; past the screen's
    /// edges when zoomed in): `Zoomed` draws it there.
    pub fn frame_on_screen(&self) -> Rectangle {
        self.rect_to_screen(Rectangle::with_size(self.screen))
    }

    /// The view `t` (0..1) of the way from `self` to `to`: the shown part's
    /// edges move linearly, so a frame pixel both have under the same screen
    /// point stays under it all the way (the wheel zoom's ease).
    pub fn lerp(&self, to: &View, t: f32) -> View {
        let (a, b) = (self.shown, to.shown);
        let at = |a: f32, b: f32| a + (b - a) * t;
        let shown = Rectangle {
            x: at(a.x, b.x),
            y: at(a.y, b.y),
            width: at(a.width, b.width),
            height: at(a.height, b.height),
        };
        View { shown, ..*to }
    }

    /// A picture of just `area` of the frame (x0, y0, x1, y1 in frame px), as the
    /// image widget must draw it over the frame: the part of it inside the view (a
    /// crop of the picture, in its px) and where that goes on screen. `None`: out of view.
    pub fn patch_in_view(&self, area: [u32; 4]) -> Option<(Rectangle<u32>, Rectangle)> {
        let [x0, y0, x1, y1] = area;
        let s = self.shown;
        let (left, top) = (x0.max(s.x.floor() as u32), y0.max(s.y.floor() as u32));
        let right = x1.min((s.x + s.width).ceil() as u32);
        let bottom = y1.min((s.y + s.height).ceil() as u32);
        if right <= left || bottom <= top {
            return None;
        }
        let (_, per) = self.ratios();
        let crop = Rectangle {
            x: left - x0,
            y: top - y0,
            width: right - left,
            height: bottom - top,
        };
        let on_screen = Rectangle::new(
            Point::new((left as f32 - s.x) * per.x, (top as f32 - s.y) * per.y),
            Size::new((right - left) as f32 * per.x, (bottom - top) as f32 * per.y),
        );
        Some((crop, on_screen))
    }

    /// Zoomed by `notches` wheel notches (1x–4x) around `at` (screen px): the
    /// frame pixel under the pointer stays under it.
    pub fn zoomed(&self, at: Point, notches: f32) -> View {
        self.zoomed_to(at, self.zoom() * ZOOM_STEP.powf(notches))
    }

    /// At `zoom` (kept to 1x–4x) around `at` (screen px), whole frame px.
    pub fn zoomed_to(&self, at: Point, zoom: f32) -> View {
        let zoom = zoom.clamp(1.0, MAX_ZOOM);
        let (fw, fh) = self.frame;
        let w = ((fw as f32 / zoom).round() as u32).clamp(1, fw);
        let h = ((fh as f32 / zoom).round() as u32).clamp(1, fh);
        let (_, per) = self.ratios();
        let under = |shown: f32, at: f32, per: f32, screen: f32, new: u32| {
            shown + at / per - at / screen * new as f32
        };
        let x = under(self.shown.x, at.x, per.x, self.screen.width, w);
        let y = under(self.shown.y, at.y, per.y, self.screen.height, h);
        self.place(x, y, w, h)
    }

    /// Moved by `by` screen px (the frame follows the pointer).
    pub fn panned(&self, by: Vector) -> View {
        let (_, per) = self.ratios();
        let (x, y) = (self.shown.x - by.x / per.x, self.shown.y - by.y / per.y);
        let whole = |v: f32| (v.round() as u32).max(1);
        self.place(x, y, whole(self.shown.width), whole(self.shown.height))
    }

    /// `w` x `h` frame px at (`x`, `y`), rounded, kept inside the frame.
    fn place(&self, x: f32, y: f32, width: u32, height: u32) -> View {
        let clamp = |v: f32, max: u32| (v.round().max(0.0) as u32).min(max) as f32;
        let shown = Rectangle {
            x: clamp(x, self.frame.0.saturating_sub(width)),
            y: clamp(y, self.frame.1.saturating_sub(height)),
            width: width as f32,
            height: height as f32,
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
    /// The wheel zoom is easing `view` (every frame redraws the whole screen).
    pub zooming: bool,
    /// Corner handles on `selection` (resize; drag inside moves it).
    pub handles: bool,
    /// Counts overlay openings: pointer state from an earlier opening is stale.
    pub opening: u32,
    pub first_draw: &'a Cell<Option<(Instant, Size)>>,
    /// When each frame was drawn (`App::log_frames`; at most `FRAME_LOG`).
    pub frame_times: &'a RefCell<Vec<Instant>>,
    /// Report moves without a button down too (snip's Window mode).
    pub hover: bool,
    /// Dim the frame (not over a full-screen translation).
    pub dim: bool,
    /// While drawing, a clear straight drag already shows as its rectangle
    /// (snip). Otherwise only the stroke shows until release decides.
    pub live_drag: bool,
    /// The active look: dims, stroke colours, brackets.
    pub look: Look,
    /// A snapped selection's drawn stroke (image px) and when it began to
    /// morph into the box.
    pub morph: Option<(&'a [Point], Instant)>,
    /// Select-text mode: every word read in the selection, and the highlight
    /// of the picked ones (image px).
    pub words: &'a [Rectangle],
    pub picked: Vec<Rectangle>,
    /// The text lines on the frame (image px): the pointer is an I-beam over them.
    pub text: Vec<Rectangle>,
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
    alt: bool,
    /// The last drawing and what it shows: drawn again only when that
    /// changes, so a frame where only a widget above it moves redraws only
    /// round that widget (the window redraws what changed).
    drawn: canvas::Cache,
    shown: RefCell<Option<Shown>>,
}

/// Everything the overlay's drawing depends on (`Pointer::drawn`).
#[derive(PartialEq)]
struct Shown {
    size: Size,
    stroke: Vec<Point>,
    selection: Option<Selection>,
    others: Vec<(Selection, Vec<Point>)>,
    view: View,
    zooming: bool,
    handles: bool,
    dim: bool,
    live_drag: bool,
    look: Look,
    morph: Option<f32>,
    words: Vec<Rectangle>,
    picked: Vec<Rectangle>,
}

impl Overlay<'_> {
    /// `p` (screen px) is over a line of the screen's text.
    fn over_text(&self, p: Point) -> bool {
        (self.text.iter()).any(|l| self.view.rect_to_screen(*l).contains(p))
    }
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
            // A morph in progress draws its next frame.
            canvas::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                let (_, start) = self.morph?;
                return (morph_progress(start, *now) < 1.0).then(Action::request_redraw);
            }
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                pointer.shift = m.shift();
                // (only a change is sent: nothing redraws for Shift or Ctrl)
                return (std::mem::replace(&mut pointer.alt, m.alt()) != m.alt())
                    .then(|| Action::publish(Event::Alt(m.alt())));
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) => match key {
                keyboard::Key::Named(Named::Enter) => Event::Enter,
                keyboard::Key::Named(Named::Space) => Event::Space,
                keyboard::Key::Named(Named::Tab) => Event::Tab {
                    back: modifiers.shift(),
                },
                keyboard::Key::Named(arrow @ (Named::ArrowLeft | Named::ArrowRight)) => {
                    Event::Arrow {
                        dx: if *arrow == Named::ArrowLeft { -1 } else { 1 },
                        dy: 0,
                        ctrl: modifiers.command(),
                        shift: modifiers.shift(),
                    }
                }
                keyboard::Key::Named(arrow @ (Named::ArrowUp | Named::ArrowDown)) => Event::Arrow {
                    dx: 0,
                    dy: if *arrow == Named::ArrowUp { -1 } else { 1 },
                    ctrl: modifiers.command(),
                    shift: modifiers.shift(),
                },
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
                if !std::mem::take(&mut pointer.panning) {
                    return None;
                }
                Event::PanEnd
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
        pointer: &Pointer,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut drawn = self.frame_times.borrow_mut();
        if drawn.len() >= FRAME_LOG {
            drawn.clear(); // never logged (a long hover): no growth
        }
        drawn.push(Instant::now());
        drop(drawn);
        // Logged once the window is really full-screen (WSLg maps it windowed first).
        if let Some((at, size)) = self.first_draw.get()
            && (bounds.width - size.width).abs() < 1.0
            && (bounds.height - size.height).abs() < 1.0
        {
            self.first_draw.set(None);
            crate::log!(
                "[opencircle] trigger -> first full-screen overlay frame drawn: {} ms",
                at.elapsed().as_millis()
            );
        }
        let morph_t = (self.morph)
            .map(|(_, start)| morph_progress(start, Instant::now()))
            .filter(|t| *t < 1.0);
        let shown = Shown {
            size: bounds.size(),
            stroke: self.stroke.to_vec(),
            selection: self.selection,
            others: self.others.to_vec(),
            view: self.view,
            zooming: self.zooming,
            handles: self.handles,
            dim: self.dim,
            live_drag: self.live_drag,
            look: self.look,
            morph: morph_t,
            words: self.words.to_vec(),
            picked: self.picked.clone(),
        };
        if pointer.shown.borrow().as_ref() != Some(&shown) {
            pointer.drawn.clear();
            *pointer.shown.borrow_mut() = Some(shown);
        }
        vec![pointer.drawn.draw(renderer, bounds.size(), |frame| {
            let screen = Rectangle::with_size(bounds.size());
            let on_screen =
                |s: &[Point]| -> Vec<Point> { s.iter().map(|&p| self.view.to_screen(p)).collect() };
            let current = on_screen(self.stroke);
            let kind = shown_kind(&current, self.selection, self.live_drag);
            let others: Vec<(Kind, Vec<Point>)> = (self.others.iter())
                .map(|(s, stroke)| (s.kind, on_screen(stroke)))
                .collect();

            // A snapped box on its way from the drawn stroke: the outline between.
            // At full from the first frame: the window fades in as a whole
            // (`windows::opacity`), never the dim over the whole screen.
            let (dim_idle, dim_outside) = theme::dims(&self.look);
            let snapped = (self.selection).filter(|s| s.kind == Kind::Snap);
            let box_on_screen = snapped.map(|s| self.view.rect_to_screen(s.area));
            let morph = match (box_on_screen, self.morph, morph_t) {
                (Some(r), Some((from, _)), Some(t)) => Some((t, morph(&on_screen(from), r, t))),
                _ => None,
            };

            // After release, the clear region is the drawn shape itself (closed), not its
            // bounding box.
            // ponytail: even-odd re-dims areas circled twice (or two overlapping
            // selections); trim the stroke at its first self-crossing if users mind.
            let closed = self.selection.is_some();
            if closed || !others.is_empty() {
                let mut holes: Vec<&[Point]> = others.iter().map(|(_, s)| &s[..]).collect();
                match &morph {
                    Some((_, outline)) => holes.push(outline),
                    None if closed => holes.push(&current),
                    None => {}
                }
                let holes: Vec<Rectangle> = holes.into_iter().filter_map(self::bounds).collect();
                dim(frame, screen, &holes, dim_outside, self.zooming, |p| {
                    for (kind, s) in &others {
                        shape(p, *kind, s, true);
                    }
                    match &morph {
                        Some((_, outline)) => shape(p, Kind::Scribble, outline, true),
                        None if closed => shape(p, kind, &current, true),
                        None => {}
                    }
                });
            } else if self.dim {
                dim(frame, screen, &[], dim_idle, self.zooming, |_| {});
            }

            let accent = theme::accent_of(&self.look);
            for (kind, s) in &others {
                glow(frame, *kind, s, true, screen, accent, 1.0);
            }
            match (&morph, box_on_screen) {
                (Some((t, outline)), Some(r)) => {
                    glow(
                        frame,
                        Kind::Scribble,
                        outline,
                        true,
                        screen,
                        accent,
                        1.0 - t,
                    );
                    brackets(frame, r, &self.look, *t);
                }
                (None, Some(r)) => brackets(frame, r, &self.look, 1.0),
                // A press that hasn't moved yet may be a tap: nothing until it moves.
                _ if !closed && is_tap(&current, 1.0) => {}
                _ => glow(frame, kind, &current, closed, screen, accent, 1.0),
            }

            // Select-text mode: every word, the picked ones stronger.
            for (rects, colour) in [
                (self.words, fixed::WORD),
                (&self.picked[..], fixed::WORD_PICKED),
            ] {
                for r in rects {
                    let r = self.view.rect_to_screen(*r);
                    let radius = (r.height / 4.0).min(6.0);
                    let path = Path::rounded_rectangle(r.position(), r.size(), radius.into());
                    frame.fill(&path, colour);
                }
            }

            if let Some(sel) = self.selection {
                let r = self.view.rect_to_screen(sel.area);
                if self.handles && !matches!(sel.kind, Kind::Snap | Kind::Word) {
                    for c in corners(r) {
                        let dot = Path::circle(c, HANDLE);
                        frame.fill(&dot, fixed::HANDLE);
                        let ring = Stroke::default().with_color(accent.0);
                        frame.stroke(&dot, ring.with_width(2.0));
                    }
                }
                // How many selections the actions get.
                if !self.others.is_empty() {
                    let at = Point::new(r.x + r.width + 14.0, r.y - 14.0);
                    frame.fill(&Path::circle(at, 11.0), accent.0);
                    frame.fill_text(canvas::Text {
                        content: (self.others.len() + 1).to_string(),
                        position: at,
                        color: fixed::ON_ACCENT,
                        size: 13.0.into(),
                        align_x: Alignment::Center,
                        align_y: Vertical::Center,
                        ..canvas::Text::default()
                    });
                }
            }
        })]
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
            let over_text = self.over_text(p) && !pointer.alt; // (Alt moves even over text)
            match grip_at(sel.kind, r, p).filter(|_| sel.kind != Kind::Word) {
                Some(Grip::Corner(0 | 2)) => return mouse::Interaction::ResizingDiagonallyDown,
                Some(Grip::Corner(_)) => return mouse::Interaction::ResizingDiagonallyUp,
                Some(Grip::Edge(0 | 2)) => return mouse::Interaction::ResizingVertically,
                Some(Grip::Edge(_)) => return mouse::Interaction::ResizingHorizontally,
                // inside, off the text (on it the press selects text)
                None if r.contains(p) && sel.kind != Kind::Word && !over_text => {
                    return mouse::Interaction::Move;
                }
                None => {}
            }
        }
        // Over text: it is selected like in a document.
        let over = |p: Point| self.over_text(p);
        match cursor.position_in(bounds) {
            Some(p) if over(p) => mouse::Interaction::Text,
            _ => mouse::Interaction::Crosshair,
        }
    }
}

/// A selection's stroke (screen px): the glowing `accent` gradient line, `fade`
/// of its strength; selected text is a flat highlight.
fn glow(
    frame: &mut canvas::Frame,
    kind: Kind,
    stroke: &[Point],
    closed: bool,
    screen: Rectangle,
    accent: (Color, Color),
    fade: f32,
) {
    if stroke.len() < 2 || fade <= 0.0 {
        return;
    }
    let path = Path::new(|p| shape(p, kind, stroke, closed));
    let span = bounds(stroke).unwrap_or(screen);
    let line = |width, alpha: f32| Stroke {
        style: gradient_style(span, alpha * fade, accent),
        width,
        line_cap: stroke::LineCap::Round,
        line_join: stroke::LineJoin::Round,
        ..Stroke::default()
    };
    if kind == Kind::Word {
        frame.fill(&path, fixed::TEXT_SELECTION); // flat, like a document's selection
        return;
    }
    // Soft glow = the same stroke, wider and fainter, under the core line.
    for (width, alpha) in [(18.0, 0.10), (10.0, 0.22), (3.5, 1.0)] {
        frame.stroke(&path, line(width, alpha));
    }
}

fn gradient_style(span: Rectangle, alpha: f32, (start, end): (Color, Color)) -> canvas::Style {
    let tint = |c: Color| c.scale_alpha(alpha);
    let linear = gradient::Linear::new(span.position(), span.position() + span.size().into())
        .add_stop(0.0, tint(start))
        .add_stop(1.0, tint(end));
    canvas::Style::Gradient(linear.into())
}

/// The dim's tile side (logical px).
const TILE: u32 = 256;

/// Dims `screen` in `colour` but for the `cut` shapes (even-odd), which lie
/// within `holes` (their bounds). As one rectangle per `TILE`, and one path
/// for the tiles the holes touch: a small redraw then fills only the dim
/// under it (tiny-skia fills a clipped path over its whole size). `whole` (the wheel zoom easing: every frame redraws all of
/// it): a row's untouched tiles side by side as one rectangle (the renderer
/// blends a big one on all its cores).
fn dim(
    frame: &mut canvas::Frame,
    screen: Rectangle,
    holes: &[Rectangle],
    colour: Color,
    whole: bool,
    cut: impl Fn(&mut canvas::path::Builder),
) {
    // a smoothed loop may bulge a little past its points
    let holes: Vec<Rectangle> = holes.iter().map(|h| h.expand(16.0)).collect();
    let side = TILE as f32;
    let mut touched = Vec::new();
    let mut y = 0.0;
    while y < screen.height {
        let mut x = 0.0;
        let mut run: Option<Rectangle> = None;
        while x < screen.width {
            let size = Size::new(side.min(screen.width - x), side.min(screen.height - y));
            let tile = Rectangle::new(Point::new(x, y), size);
            match holes.iter().any(|h| h.intersects(&tile)) {
                true => touched.push(tile),
                false if whole => {
                    run = Some(run.map_or(tile, |r| r.union(&tile)));
                    x += side;
                    continue;
                }
                false => frame.fill_rectangle(tile.position(), size, colour),
            }
            if let Some(r) = run.take() {
                frame.fill_rectangle(r.position(), r.size(), colour);
            }
            x += side;
        }
        if let Some(r) = run {
            frame.fill_rectangle(r.position(), r.size(), colour);
        }
        y += side;
    }
    if !touched.is_empty() {
        let path = Path::new(|p| {
            touched
                .iter()
                .for_each(|t| p.rectangle(t.position(), t.size()));
            cut(p);
        });
        let style = canvas::Style::Solid(colour);
        let rule = fill::Rule::EvenOdd;
        frame.fill(&path, Fill { style, rule });
    }
}

/// The frozen frame filling the canvas pixel for pixel (`scale` device px
/// per logical px), a quarter pixel in: tiny-skia truncates image bounds to
/// whole device pixels, and float error would shift it one. The renderer
/// copies just the damaged part of it (`Pipeline::blit`), with nothing
/// cleared under it, and the zoom draws the same image: its pixels are
/// prepared once, by the opening's first frame (#89: drawn as tiles, the
/// first zoom converted the whole frame, 25 ms at 4K).
pub struct Frozen<'a> {
    pub frame: &'a Handle,
    pub scale: f32,
}

impl<M> canvas::Program<M> for Frozen<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Handle::Rgba { width, height, .. } = self.frame {
            let k = 1.0 / self.scale;
            let at = Point::new(0.25 * k, 0.25 * k);
            let size = Size::new(*width as f32 * k, *height as f32 * k);
            let image = canvas::Image::new(self.frame.clone()).filter_method(FilterMethod::Nearest);
            frame.draw_image(Rectangle::new(at, size), image);
        }
        vec![frame.into_geometry()]
    }
}

/// The frozen frame (or its translation) while the wheel zoom eases: the
/// whole picture where `view` puts it, to a fraction of a pixel, Nearest (the
/// renderer scales it straight into the window: a whole 4K screen redrawn
/// each frame of the ease). At rest the image widget crops it (`View::crop`)
/// with its usual filter. `dim`: the overlay's plain dim (its alpha), drawn
/// here as the picture faded over black, which the renderer does in the same
/// pass as the picture (`iced_tiny_skia`'s `Renderer::base`).
pub struct Zoomed<'a> {
    pub picture: &'a Handle,
    pub view: View,
    pub dim: f32,
}

impl<M> canvas::Program<M> for Zoomed<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), fixed::BACKDROP);
        let image = canvas::Image::new(self.picture.clone())
            .filter_method(FilterMethod::Nearest)
            .opacity(1.0 - self.dim);
        frame.draw_image(self.view.frame_on_screen(), image);
        vec![frame.into_geometry()]
    }
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

    /// A path through `corners`, a point about every 3 px (a mouse's samples).
    fn through(corners: &[(f32, f32)]) -> Vec<Point> {
        let mut out = vec![Point::new(corners[0].0, corners[0].1)];
        for w in corners.windows(2) {
            let (a, b) = (Point::new(w[0].0, w[0].1), Point::new(w[1].0, w[1].1));
            let n = (a.distance(b) / 3.0).ceil().max(1.0) as usize;
            out.extend((1..=n).map(|i| a + (b - a) * (i as f32 / n as f32)));
        }
        out
    }

    fn zigzag() -> Vec<Point> {
        through(&[
            (100.0, 100.0),
            (220.0, 112.0),
            (100.0, 124.0),
            (220.0, 136.0),
            (100.0, 148.0),
        ])
    }

    #[test]
    fn only_a_real_loop_winds() {
        // circles, either way, closed or nearly
        assert!(winds(&ellipse(200.0, 100.0, 50.0, 30.0, 1.0), 1.0));
        assert!(winds(&ellipse(200.0, 100.0, 50.0, 30.0, -1.0), 1.0));
        assert!(winds(&ellipse(200.0, 100.0, 60.0, 60.0, 0.9), 1.0));
        assert!(winds(&ellipse(200.0, 100.0, 40.0, 40.0, 2.5), 1.0));
        // a text drag: along a line, back to the next line's start, along it
        let text = through(&[
            (100.0, 100.0),
            (400.0, 100.0),
            (60.0, 130.0),
            (200.0, 130.0),
        ]);
        assert!(!winds(&text, 1.0));
        // up a column of lines and over; a C; a zig-zag; a straight drag
        let up = through(&[(300.0, 300.0), (60.0, 270.0), (400.0, 240.0), (60.0, 210.0)]);
        assert!(!winds(&up, 1.0));
        assert!(!winds(&ellipse(200.0, 100.0, 50.0, 50.0, 0.5), 1.0));
        assert!(!winds(&zigzag(), 1.0));
        assert!(!winds(&through(&[(0.0, 0.0), (300.0, 40.0)]), 1.0));
        assert!(!winds(&[], 1.0));
    }

    #[test]
    fn strokes_are_a_tap_a_drag_a_loop_or_a_scribble() {
        let kind = |s: &[Point]| classify(s, 1.0);
        // tap: stays within 4 px; a bigger wiggle is not
        assert_eq!(
            kind(&[Point::new(9.0, 9.0), Point::new(11.0, 12.0)]),
            Gesture::Tap
        );
        assert_ne!(kind(&through(&[(0.0, 0.0), (30.0, 0.0)])), Gesture::Tap);
        // drag: straight, either direction, with a little hand wobble
        assert_eq!(
            kind(&through(&[(10.0, 10.0), (210.0, 90.0)])),
            Gesture::Drag
        );
        assert_eq!(
            kind(&through(&[(210.0, 90.0), (10.0, 10.0)])),
            Gesture::Drag
        );
        let wobble: Vec<Point> = through(&[(0.0, 0.0), (240.0, 0.0)])
            .iter()
            .enumerate()
            .map(|(i, p)| Point::new(p.x, p.y + [2.0, -2.0, 1.0][i % 3]))
            .collect();
        assert_eq!(kind(&wobble), Gesture::Drag);
        // loops: closed, open (a C), several turns, a hook or a Z (too few corners)
        for lp in [
            ellipse(200.0, 100.0, 50.0, 30.0, 1.0),
            ellipse(200.0, 100.0, 50.0, 30.0, 0.85),
            ellipse(200.0, 100.0, 50.0, 50.0, 0.5),
            ellipse(200.0, 100.0, 40.0, 40.0, 2.5),
            ellipse(200.0, 100.0, 10.0, 10.0, 1.0),
            through(&[(0.0, 0.0), (120.0, 0.0), (120.0, 90.0)]), // an L: 1 corner
            through(&[(0.0, 0.0), (100.0, 0.0), (0.0, 80.0), (100.0, 80.0)]), // a Z: 2
            through(&[(0.0, 0.0), (60.0, 80.0), (140.0, -20.0)]), // a tick
        ] {
            assert_eq!(kind(&lp), Gesture::Loop, "{:?}", lp.last());
        }
        // scribbles: 3+ sharp corners, any direction, any amplitude
        assert_eq!(kind(&zigzag()), Gesture::Scribble);
        let down = through(&[
            (100.0, 100.0),
            (112.0, 220.0),
            (124.0, 100.0),
            (136.0, 220.0),
            (148.0, 100.0),
        ]);
        assert_eq!(kind(&down), Gesture::Scribble);
        let diagonal = through(&[
            (0.0, 0.0),
            (90.0, 90.0),
            (20.0, 20.0),
            (110.0, 110.0),
            (40.0, 40.0),
            (130.0, 130.0),
        ]);
        assert_eq!(kind(&diagonal), Gesture::Scribble);
        // back and forth over one line (5 passes end on the far side: a straight chord, but a scribble)
        let sweep = through(&[
            (100.0, 100.0),
            (300.0, 103.0),
            (100.0, 106.0),
            (300.0, 109.0),
            (100.0, 112.0),
            (300.0, 115.0),
        ]);
        assert!(is_drag(&sweep));
        assert_eq!(kind(&sweep), Gesture::Scribble);
        // a star has five sharp corners
        let star: Vec<(f32, f32)> = [0, 2, 4, 1, 3, 0]
            .iter()
            .map(|k| {
                let a = *k as f32 / 5.0 * std::f32::consts::TAU;
                (200.0 + 60.0 * a.sin(), 200.0 - 60.0 * a.cos())
            })
            .collect();
        assert_eq!(kind(&through(&star)), Gesture::Scribble);
        // the same scribble seen at 2x zoom (half the image px) is still one
        let half: Vec<Point> = zigzag()
            .iter()
            .map(|p| Point::new(p.x * 0.5, p.y * 0.5))
            .collect();
        assert_eq!(classify(&half, 2.0), Gesture::Scribble);
        // no points, one point: nothing to select
        assert_eq!(select(&[], 1.0), None);
        assert_eq!(kind(&[Point::new(1.0, 1.0)]), Gesture::Tap);
    }

    #[test]
    fn a_scribble_selects_its_box_and_clears_a_padded_hull() {
        let zig = zigzag();
        let screen = Size::new(1920.0, 1080.0);
        let (sel, hull) = settle(&zig, 1.0, screen).unwrap();
        assert_eq!(sel.kind, Kind::Scribble);
        let pad = SCRIBBLE_PAD;
        // the area is the stroke's box grown by the pad: it holds the whole clear area
        let want = rect(
            100.0 - pad,
            100.0 - pad,
            120.0 + 2.0 * pad,
            48.0 + 2.0 * pad,
        );
        for (got, want) in [
            (sel.area.x, want.x),
            (sel.area.y, want.y),
            (sel.area.width, want.width),
            (sel.area.height, want.height),
        ] {
            assert!((got - want).abs() < 0.5, "{:?} vs {want:?}", sel.area);
        }
        assert_eq!(Some(sel.area), bounds(&hull));
        // the clear area is a convex shape with rounded corners, not the box
        assert!(hull.len() > 8 && hull.len() < 200);
        let corner = Point::new(sel.area.x, sel.area.y);
        assert!(hull.iter().all(|p| p.distance(corner) > 2.0), "corner cut");
        let turn =
            |a: Point, b: Point, c: Point| (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        let side = turn(hull[0], hull[1], hull[2]).signum();
        for i in 0..hull.len() {
            let (a, b, c) = (
                hull[i],
                hull[(i + 1) % hull.len()],
                hull[(i + 2) % hull.len()],
            );
            assert!(turn(a, b, c) * side >= -1e-3, "convex at {i}");
        }
        // every point of the stroke is inside it
        for p in &zig {
            assert!(
                (0..hull.len())
                    .all(|i| turn(hull[i], hull[(i + 1) % hull.len()], *p) * side >= -1e-3),
                "{p:?} outside"
            );
        }
        // the pad is on screen: at 2x zoom it is half as many image px
        let half: Vec<Point> = zig
            .iter()
            .map(|p| Point::new(p.x * 0.5, p.y * 0.5))
            .collect();
        let (zoomed, _) = settle(&half, 2.0, screen).unwrap();
        assert!(
            (zoomed.area.width - (60.0 + pad)).abs() < 0.5,
            "{:?}",
            zoomed.area
        );
        // kept inside the screen
        let near_edge = through(&[
            (2.0, 2.0),
            (120.0, 14.0),
            (2.0, 26.0),
            (120.0, 38.0),
            (2.0, 50.0),
        ]);
        let (edge, hull) = settle(&near_edge, 1.0, screen).unwrap();
        assert_eq!((edge.area.x, edge.area.y), (0.0, 0.0));
        assert!(hull.iter().all(|p| p.x >= 0.0 && p.y >= 0.0));
        // thin: a scribble over one line of text is still a selection
        let thin = through(&[
            (100.0, 100.0),
            (300.0, 102.0),
            (100.0, 104.0),
            (300.0, 106.0),
            (100.0, 108.0),
            (300.0, 110.0),
        ]);
        let (sel, _) = settle(&thin, 1.0, screen).unwrap();
        assert!(sel.area.height > 2.0 * pad && sel.kind == Kind::Scribble);
        // but a tiny wiggle (under MIN_SIDE) is nothing
        let tiny = through(&[
            (100.0, 100.0),
            (108.0, 101.0),
            (100.0, 102.0),
            (108.0, 103.0),
            (100.0, 104.0),
        ]);
        assert!(settle(&tiny, 1.0, screen).is_none());
        // a loop keeps its own stroke
        let lp = ellipse(200.0, 100.0, 50.0, 30.0, 1.0);
        let (sel, kept) = settle(&lp, 1.0, screen).unwrap();
        assert_eq!((sel.kind, kept), (Kind::Loop, lp));
    }

    #[test]
    fn the_hull_of_points() {
        let square = [
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (5.0, 5.0),
            (5.0, 0.0),
        ]
        .map(|(x, y)| Point::new(x, y));
        let h = hull(&square);
        assert_eq!(h.len(), 4, "the middle and edge points are inside: {h:?}");
        assert_eq!(hull(&[Point::ORIGIN]).len(), 1);
        let line = [0.0, 1.0, 2.0, 3.0].map(|x| Point::new(x, x));
        assert_eq!(hull(&line).len(), 2, "collinear: its two ends");
        assert!(hull(&[]).is_empty());
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

    /// Distance from `p` to the outline of `r` rounded by `rad`.
    fn off_outline(r: Rectangle, rad: f32, p: Point) -> f32 {
        let (cx, cy) = (r.center_x(), r.center_y());
        let qx = (p.x - cx).abs() - (r.width / 2.0 - rad);
        let qy = (p.y - cy).abs() - (r.height / 2.0 - rad);
        (qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad).abs()
    }

    fn signed_area(pts: &[Point]) -> f32 {
        (0..pts.len())
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum()
    }

    #[test]
    fn a_snapped_box_is_the_strokes_box_rounded_and_the_stroke_morphs_into_it() {
        let lp = ellipse(400.0, 300.0, 120.0, 70.0, 1.0);
        let (sel, _) = settle(&lp, 1.0, Size::new(1920.0, 1080.0)).unwrap();
        let (snapped, corners) = snap(sel.area);
        assert_eq!(snapped.kind, Kind::Snap);
        assert_eq!(snapped.area, sel.area);
        assert_eq!(
            drag_rect(&corners),
            sel.area,
            "the stroke is its two corners"
        );
        let r = sel.area;
        assert_eq!(snap_radius(r), SNAP_RADIUS);
        assert_eq!(
            snap_radius(rect(0.0, 0.0, 40.0, 20.0)),
            5.0,
            "small box: smaller corners"
        );
        // the outline: on the rounded box, clockwise, from the top edge's left end
        let outline = rounded_outline(r, SNAP_RADIUS, 96);
        assert_eq!(outline.len(), 96);
        assert!(
            outline
                .iter()
                .all(|&p| off_outline(r, SNAP_RADIUS, p) < 0.05)
        );
        assert!(outline[0].distance(Point::new(r.x + SNAP_RADIUS, r.y)) < 0.01);
        assert!(signed_area(&outline) > 0.0, "clockwise on screen");
        // the morph: the stroke at 0, the box at 1, halfway between them at 0.5
        let start = morph(&lp, r, 0.0);
        let end = morph(&lp, r, 1.0);
        let mid = morph(&lp, r, 0.5);
        assert_eq!(end, outline);
        for p in &start {
            let on_stroke = lp.windows(2).any(|w| to_segment(*p, w[0], w[1]) < 0.01);
            assert!(on_stroke, "{p:?} is not on the stroke");
        }
        for ((a, b), m) in start.iter().zip(&end).zip(&mid) {
            assert!(m.distance(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)) < 0.01);
        }
        // drawn the other way round: the same morph (no twist through the middle)
        let mut back = lp.clone();
        back.reverse();
        let other = morph(&back, r, 0.0);
        assert!(signed_area(&other) > 0.0);
        assert!(
            other[0].distance(start[0]) < 8.0,
            "{:?} vs {:?}",
            other[0],
            start[0]
        );
        // the start point is the stroke's point nearest the outline's start
        assert!(start[0].distance(outline[0]) < 60.0);
        // odd strokes don't panic
        assert_eq!(morph(&[], r, 0.5).len(), MORPH_POINTS);
        assert_eq!(morph(&[Point::ORIGIN; 3], r, 1.0), outline);
        // progress: eased, 0 at the start, 1 from MORPH on
        let t0 = Instant::now();
        assert_eq!(morph_progress(t0, t0), 0.0);
        let half = morph_progress(t0, t0 + MORPH / 2);
        assert!(half > 0.5 && half < 1.0, "eased out: {half}");
        assert_eq!(morph_progress(t0, t0 + MORPH * 2), 1.0);
        assert!(MORPH <= Duration::from_millis(220));
    }

    #[test]
    fn the_brackets_grab_their_corner_and_nothing_else() {
        let r = rect(100.0, 100.0, 200.0, 100.0); // arms: 25 px
        // the corner itself, just outside the box
        assert_eq!(bracket_at(r, Point::new(97.0, 97.0)), Some(0));
        // along the top-left arm, either way
        assert_eq!(bracket_at(r, Point::new(118.0, 99.0)), Some(0));
        assert_eq!(bracket_at(r, Point::new(96.0, 120.0)), Some(0));
        // every corner
        assert_eq!(bracket_at(r, Point::new(300.0, 98.0)), Some(1));
        assert_eq!(bracket_at(r, Point::new(303.0, 185.0)), Some(2));
        assert_eq!(bracket_at(r, Point::new(110.0, 204.0)), Some(3));
        // the middle of an edge, the inside and far away: not a bracket
        assert_eq!(bracket_at(r, Point::new(200.0, 97.0)), None);
        assert_eq!(bracket_at(r, Point::new(303.0, 150.0)), None);
        assert_eq!(bracket_at(r, Point::new(200.0, 150.0)), None);
        assert_eq!(bracket_at(r, Point::new(60.0, 60.0)), None);
        // a snapped box's grip is its brackets (an arm), others their corner dots
        let arm = Point::new(125.0, 97.0);
        assert_eq!(grip_at(Kind::Snap, r, arm), Some(Grip::Corner(0)));
        assert_eq!(grip_at(Kind::Drag, r, arm), Some(Grip::Edge(0)));
        let dot = Point::new(101.0, 99.0);
        assert_eq!(grip_at(Kind::Drag, r, dot), Some(Grip::Corner(0)));
        // a tiny box: the arms keep their minimum length
        let tiny = rect(100.0, 100.0, 20.0, 20.0);
        assert_eq!(bracket_at(tiny, Point::new(97.0 + 14.0, 97.0)), Some(0));
    }

    #[test]
    fn edges_grab_within_reach_and_keep_a_middle_to_move() {
        let r = rect(100.0, 100.0, 200.0, 100.0);
        assert_eq!(edge_at(r, Point::new(200.0, 92.0)), Some(0));
        assert_eq!(edge_at(r, Point::new(308.0, 150.0)), Some(1));
        assert_eq!(edge_at(r, Point::new(200.0, 208.0)), Some(2));
        assert_eq!(edge_at(r, Point::new(90.0, 150.0)), Some(3));
        assert_eq!(edge_at(r, Point::new(200.0, 150.0)), None, "inside = move");
        assert_eq!(edge_at(r, Point::new(200.0, 80.0)), None, "too far out");
        // a corner wins over the edges that meet there
        assert_eq!(
            grip_at(Kind::Drag, r, Point::new(101.0, 101.0)),
            Some(Grip::Corner(0))
        );
        // a small box: the bands stop at a third of its side
        let small = rect(100.0, 100.0, 30.0, 30.0);
        assert_eq!(edge_at(small, Point::new(115.0, 115.0)), None);
    }

    #[test]
    fn an_edge_moves_alone_and_never_flips() {
        let r = rect(100.0, 100.0, 200.0, 100.0);
        assert_eq!(
            resize_edge(r, 1, Point::new(350.0, 999.0), 12.0),
            rect(100.0, 100.0, 250.0, 100.0)
        );
        assert_eq!(
            resize_edge(r, 3, Point::new(150.0, 0.0), 12.0),
            rect(150.0, 100.0, 150.0, 100.0)
        );
        assert_eq!(
            resize_edge(r, 0, Point::new(0.0, 500.0), 12.0),
            rect(100.0, 188.0, 200.0, 12.0)
        );
        assert_eq!(
            resize_edge(r, 2, Point::new(0.0, 0.0), 12.0),
            rect(100.0, 100.0, 200.0, 12.0)
        );
    }

    #[test]
    fn a_moved_box_stays_on_the_screen() {
        let screen = Size::new(800.0, 600.0);
        let r = rect(100.0, 100.0, 200.0, 100.0);
        assert_eq!(keep_inside(r, screen), r);
        assert_eq!(
            keep_inside(rect(-50.0, 590.0, 200.0, 100.0), screen),
            rect(0.0, 500.0, 200.0, 100.0)
        );
        assert_eq!(
            keep_inside(rect(700.0, -9.0, 200.0, 100.0), screen),
            rect(600.0, 0.0, 200.0, 100.0)
        );
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

    /// The zoom's ease (#89): between two views with the same frame pixel
    /// under the pointer, every step keeps it there; the ends are the views.
    #[test]
    fn lerp_keeps_the_pointers_pixel_under_it_at_every_t() {
        let v = view().zoomed(Point::new(1500.0, 900.0), 2.0);
        for at in [Point::new(600.0, 300.0), Point::new(1900.0, 20.0)] {
            let to = v.zoomed_to(at, 3.7);
            let under = v.to_image(at);
            // (the target is on whole frame px: off by a little, never more)
            let end = to.to_image(at).distance(under);
            assert!(end < 0.5);
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let step = v.lerp(&to, t);
                let off = step.to_image(at).distance(under);
                assert!(off <= end * t + 1e-3, "{at:?} at t {t}: {off} px off");
                let z = step.zoom();
                assert!(z >= v.zoom() - 1e-3 && z <= to.zoom() + 1e-3, "{z}");
            }
            assert_eq!((v.lerp(&to, 0.0), v.lerp(&to, 1.0)), (v, to));
        }
    }

    #[test]
    fn a_patch_goes_where_its_area_is_in_the_view() {
        // 3840x2160 frame on a 1920x1080 screen: 2 frame px per screen px
        let v = view();
        let (crop, at) = v.patch_in_view([200, 100, 1000, 500]).unwrap();
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (0, 0, 800, 400));
        assert_eq!(
            (at.x, at.y, at.width, at.height),
            (100.0, 50.0, 400.0, 200.0)
        );
        // zoomed to the bottom right: only the part in view, cut from the patch
        let z = v.zoomed(Point::new(1919.0, 1079.0), 50.0); // 4x
        let shown = z.crop().unwrap();
        let area = [shown.x - 100, shown.y + 100, shown.x + 100, shown.y + 300];
        let (crop, at) = z.patch_in_view(area).unwrap();
        assert_eq!((crop.x, crop.y, crop.width), (100, 0, 100));
        assert!(at.x.abs() < 0.01 && (at.y - 100.0 * 1920.0 / shown.width as f32).abs() < 0.01);
        // a patch outside the view is not drawn
        assert!(z.patch_in_view([0, 0, 100, 100]).is_none());
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

    /// The overlay drawn headless (960x540, image = screen px) with `stroke` and
    /// `selection`, as RGBA.
    fn drawn(stroke: &[Point], selection: Option<Selection>, live_drag: bool) -> Vec<u8> {
        let first_draw = Cell::new(None);
        let overlay = Overlay {
            stroke,
            selection,
            others: &[],
            view: View::new(960, 540, Size::new(960.0, 540.0)),
            zooming: false,
            handles: false,
            opening: 0,
            first_draw: &first_draw,
            frame_times: &RefCell::default(),
            hover: false,
            dim: true,
            live_drag,
            look: crate::config::Config::default().look.active(),
            morph: None,
            words: &[],
            picked: Vec::new(),
            text: Vec::new(),
        };
        let canvas = iced::widget::canvas(overlay)
            .width(iced::Fill)
            .height(iced::Fill);
        crate::ui::kit::testing::shot::render(canvas.into(), 960, 540)
    }

    /// Selected text is drawn like a document's selection: flat boxes with
    /// square corners, no handles.
    #[test]
    fn selected_text_is_flat_and_square_without_handles() {
        let first_draw = Cell::new(None);
        // two lines: (100, 100)-(300, 124) and (100, 124)-(220, 148)
        let stroke = [
            Point::new(100.0, 100.0),
            Point::new(300.0, 124.0),
            Point::new(100.0, 124.0),
            Point::new(220.0, 148.0),
        ];
        let area = Rectangle::new(Point::new(100.0, 100.0), Size::new(200.0, 48.0));
        let overlay = Overlay {
            stroke: &stroke,
            selection: Some(Selection {
                area,
                kind: Kind::Word,
            }),
            others: &[],
            view: View::new(960, 540, Size::new(960.0, 540.0)),
            zooming: false,
            handles: true,
            opening: 0,
            first_draw: &first_draw,
            frame_times: &RefCell::default(),
            hover: false,
            dim: true,
            live_drag: false,
            look: crate::config::Config::default().look.active(),
            morph: None,
            words: &[],
            picked: Vec::new(),
            text: Vec::new(),
        };
        let canvas = iced::widget::canvas(overlay)
            .width(iced::Fill)
            .height(iced::Fill);
        let rgba = crate::ui::kit::testing::shot::render(canvas.into(), 960, 540);
        let fill = px(&rgba, Point::new(200.0, 112.0));
        // the very corners are the same flat colour as the middle
        for corner in [
            (100.0, 100.0),
            (299.0, 100.0),
            (100.0, 147.0),
            (219.0, 147.0),
        ] {
            assert_eq!(
                px(&rgba, Point::new(corner.0, corner.1)),
                fill,
                "{corner:?}"
            );
        }
        assert_ne!(
            px(&rgba, Point::new(250.0, 140.0)),
            fill,
            "the second line ends at 220"
        );
        // no handle (white dot) anywhere
        let white = rgba
            .chunks(4)
            .filter(|p| p[0] > 240 && p[1] > 240 && p[2] > 240)
            .count();
        assert_eq!(white, 0);
    }

    fn px(rgba: &[u8], p: Point) -> [u8; 4] {
        let (x, y) = (p.x.round() as usize, p.y.round() as usize);
        rgba[(y * 960 + x) * 4..][..4].try_into().unwrap()
    }

    /// The start of a circle (centre (480, 300), radius 150) up to `deg` degrees.
    fn arc(points: usize, deg: f32) -> Vec<Point> {
        (0..points)
            .map(|i| {
                let a = (deg * i as f32 / (points - 1).max(1) as f32).to_radians();
                Point::new(480.0 + 150.0 * a.cos(), 300.0 + 150.0 * a.sin())
            })
            .collect()
    }

    #[test]
    fn while_drawing_only_the_stroke_shows_never_a_box() {
        let idle = px(&drawn(&[], None, false), Point::new(10.0, 10.0));
        let short_drag = vec![Point::new(100.0, 100.0), Point::new(300.0, 200.0)];
        for stroke in [short_drag.clone(), arc(5, 40.0), arc(2, 25.0)] {
            assert!(is_drag(&stroke), "the first points look like a drag");
            let rgba = drawn(&stroke, None, false);
            // the rectangle it would select (first to last point): its other two
            // corners, off the stroke, stay idle
            let (a, b) = (stroke[0], stroke[stroke.len() - 1]);
            for c in [Point::new(a.x, b.y), Point::new(b.x, a.y)] {
                assert_eq!(px(&rgba, c), idle, "a box at {c:?}");
            }
            // and the stroke itself shows
            let on = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let on = if stroke.len() == 2 { on } else { stroke[2] };
            assert_ne!(px(&rgba, on), idle, "the stroke");
        }
        // a press with no or one point, or a tap's jitter: nothing at all
        let p = Point::new(400.0, 300.0);
        for stroke in [vec![], vec![p], vec![p, p + Vector::new(2.0, 1.0)]] {
            assert_eq!(px(&drawn(&stroke, None, false), p), idle);
        }
        // snip (live_drag): a clear drag shows its rectangle; a thin one doesn't
        let corner = Point::new(300.0, 100.0);
        assert_ne!(px(&drawn(&short_drag, None, true), corner), idle);
        let thin = [Point::new(5.0, 5.0), Point::new(9.0, 30.0)];
        assert_eq!(shown_kind(&thin, None, true), Kind::Loop);
        // released and snapped: the brackets at the box's corner
        let (sel, kept) = snap(Rectangle::new(
            Point::new(330.0, 150.0),
            Size::new(300.0, 300.0),
        ));
        let rgba = drawn(&kept, Some(sel), false);
        assert_ne!(px(&rgba, Point::new(326.0, 160.0)), idle, "a bracket");
    }

    /// `OPENCIRCLE_DRAW_PNG=<dir> cargo test circle_frames -- --ignored` writes
    /// the frames of a circle being drawn (stroke only), then released (brackets).
    #[test]
    #[ignore = "writes PNGs to look at"]
    fn circle_frames() {
        let dir = std::path::PathBuf::from(
            std::env::var("OPENCIRCLE_DRAW_PNG").expect("OPENCIRCLE_DRAW_PNG=<dir>"),
        );
        let write = |name: String, rgba: &[u8]| {
            crate::actions::write_png(&dir.join(name), 960, 540, rgba).unwrap();
        };
        let ring = arc(64, 360.0);
        for n in [1, 2, 3, 5, 10, 20, 40, 64] {
            write(format!("frame-{n:02}.png"), &drawn(&ring[..n], None, false));
        }
        let (sel, kept) = snap(bounds(&ring).unwrap());
        write("frame-released.png".into(), &drawn(&kept, Some(sel), false));
    }
}
