//! Snip's live area (N-73 points 3-4): after the pick the captured area stays
//! movable and resizable on the frozen frame, before and while it is annotated.
//! The document then holds the whole frozen monitor and the area is its crop:
//! resizing only changes the crop (the capture is cut or extended from the
//! frame, the drawing is never scaled); moving shifts the crop and every item
//! with it. Pure geometry here; `Editor` applies it as one undoable edit.
use iced::{Point, Rectangle, Size, Vector};

/// Grips react this close to the area's outline (display px).
pub const REACH: f32 = 8.0;
/// The area is never smaller than this (image px, either side).
pub const MIN_SIDE: f32 = 8.0;

/// What a press on the area takes: one of its 8 handles, or the whole area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Move,
    /// Which edges follow the pointer: -1 = left / top, 1 = right / bottom, 0 = neither.
    Edge(i8, i8),
}

impl Grip {
    /// The 8 handles, clockwise from the top-left corner.
    pub const HANDLES: [Grip; 8] = [
        Grip::Edge(-1, -1),
        Grip::Edge(0, -1),
        Grip::Edge(1, -1),
        Grip::Edge(1, 0),
        Grip::Edge(1, 1),
        Grip::Edge(0, 1),
        Grip::Edge(-1, 1),
        Grip::Edge(-1, 0),
    ];

    /// Where this handle sits on `r`.
    pub fn at(self, r: Rectangle) -> Point {
        let (sx, sy) = match self {
            Grip::Move => (0, 0),
            Grip::Edge(x, y) => (x, y),
        };
        let along = |s: i8, from: f32, len: f32| from + len * (s as f32 + 1.0) / 2.0;
        Point::new(along(sx, r.x, r.width), along(sy, r.y, r.height))
    }
}

/// The grip under `p` on area `r` (same units, `reach` in them): a handle, else
/// an edge (the whole edge resizes, as in most apps), else `None`. Inside is
/// not a grip here: the host decides whether a press inside draws or moves.
pub fn grip_at(r: Rectangle, p: Point, reach: f32) -> Option<Grip> {
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    let near = |v: f32, edge: f32| (v - edge).abs() <= reach;
    let within = |v: f32, a: f32, b: f32| v >= a - reach && v <= b + reach;
    if !within(p.x, x0, x1) || !within(p.y, y0, y1) {
        return None;
    }
    // A small area: the nearer edge wins on each axis.
    let side = |v: f32, a: f32, b: f32| match (near(v, a), near(v, b)) {
        (true, true) if (v - a).abs() <= (v - b).abs() => -1,
        (true, true) => 1,
        (true, false) => -1,
        (false, true) => 1,
        (false, false) => 0,
    };
    match (side(p.x, x0, x1), side(p.y, y0, y1)) {
        (0, 0) => None,
        (x, y) => Some(Grip::Edge(x, y)),
    }
}

/// Area `start` after `grip` was dragged by `by`: inside `bounds` (the frozen
/// frame), at least `min` a side. A move keeps its size (stops at the edge); a
/// resize stops its edge at the frame's edge and at `min` from the other edge.
pub fn dragged(grip: Grip, start: Rectangle, by: Vector, bounds: Rectangle, min: f32) -> Rectangle {
    let (bx0, by0) = (bounds.x, bounds.y);
    let (bx1, by1) = (bounds.x + bounds.width, bounds.y + bounds.height);
    match grip {
        Grip::Move => {
            let x = (start.x + by.x).min(bx1 - start.width).max(bx0);
            let y = (start.y + by.y).min(by1 - start.height).max(by0);
            Rectangle::new(Point::new(x, y), start.size())
        }
        Grip::Edge(sx, sy) => {
            let axis = |s: i8, a: f32, b: f32, d: f32, lo: f32, hi: f32| match s {
                -1 => ((a + d).max(lo).min(b - min), b),
                1 => (a, (b + d).min(hi).max(a + min)),
                _ => (a, b),
            };
            let (x0, x1) = axis(sx, start.x, start.x + start.width, by.x, bx0, bx1);
            let (y0, y1) = axis(sy, start.y, start.y + start.height, by.y, by0, by1);
            Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
        }
    }
}

/// An arrow key on the area: what it does (the overlay's own convention, so
/// both modes feel the same): moves 1 px; Shift = 10 px; Ctrl resizes from the
/// right / bottom edge, Ctrl+Alt from the left / top edge.
/// `(dx, dy)` = -1, 0 or 1 per axis; image px per screen px = `px`.
pub fn nudge(
    start: Rectangle,
    (dx, dy): (i8, i8),
    (ctrl, shift, alt): (bool, bool, bool),
    px: f32,
    bounds: Rectangle,
    min: f32,
) -> Rectangle {
    let step = px * if shift { 10.0 } else { 1.0 };
    let by = Vector::new(dx as f32 * step, dy as f32 * step);
    let grip = match (ctrl, alt) {
        (false, _) => Grip::Move,
        // The moved edge is the one the arrow points along: Ctrl+Right grows
        // the right edge, Ctrl+Alt+Left grows the left one.
        (true, false) => Grip::Edge(dx.abs(), dy.abs()),
        (true, true) => Grip::Edge(-dx.abs(), -dy.abs()),
    };
    dragged(grip, start, by, bounds, min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    const FRAME: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
    };

    #[test]
    fn grips_are_the_8_handles_and_the_edges_and_never_inside() {
        let r = rect(100.0, 100.0, 400.0, 200.0);
        for g in Grip::HANDLES {
            assert_eq!(grip_at(r, g.at(r), REACH), Some(g), "{g:?}");
        }
        // Along an edge, away from its handle: that edge.
        assert_eq!(
            grip_at(r, Point::new(180.0, 103.0), REACH),
            Some(Grip::Edge(0, -1))
        );
        assert_eq!(
            grip_at(r, Point::new(507.0, 250.0), REACH),
            Some(Grip::Edge(1, 0))
        );
        // Inside and outside: no grip.
        assert_eq!(grip_at(r, Point::new(300.0, 200.0), REACH), None);
        assert_eq!(grip_at(r, Point::new(50.0, 50.0), REACH), None);
        assert_eq!(grip_at(r, Point::new(300.0, 320.0), REACH), None);
        // A tiny area: the nearer edge wins, so both sides stay reachable.
        let tiny = rect(100.0, 100.0, 10.0, 20.0);
        assert_eq!(
            grip_at(tiny, Point::new(102.0, 110.0), REACH),
            Some(Grip::Edge(-1, 0))
        );
        assert_eq!(
            grip_at(tiny, Point::new(109.0, 110.0), REACH),
            Some(Grip::Edge(1, 0))
        );
    }

    #[test]
    fn moving_keeps_the_size_and_stops_at_the_frame() {
        let r = rect(100.0, 100.0, 400.0, 200.0);
        let moved = dragged(Grip::Move, r, Vector::new(30.0, -20.0), FRAME, MIN_SIDE);
        assert_eq!(moved, rect(130.0, 80.0, 400.0, 200.0));
        // Pushed past the corner: it stops there, same size.
        let far = dragged(Grip::Move, r, Vector::new(5000.0, 5000.0), FRAME, MIN_SIDE);
        assert_eq!(far, rect(880.0, 520.0, 400.0, 200.0));
        let back = dragged(Grip::Move, r, Vector::new(-500.0, -500.0), FRAME, MIN_SIDE);
        assert_eq!(back, rect(0.0, 0.0, 400.0, 200.0));
    }

    #[test]
    fn resizing_moves_only_its_edges_with_a_minimum_and_inside_the_frame() {
        let r = rect(100.0, 100.0, 400.0, 200.0);
        // Bottom-right corner out by (50, 20).
        let grown = dragged(
            Grip::Edge(1, 1),
            r,
            Vector::new(50.0, 20.0),
            FRAME,
            MIN_SIDE,
        );
        assert_eq!(grown, rect(100.0, 100.0, 450.0, 220.0));
        // Top edge only: x and width never change.
        let top = dragged(
            Grip::Edge(0, -1),
            r,
            Vector::new(77.0, -30.0),
            FRAME,
            MIN_SIDE,
        );
        assert_eq!(top, rect(100.0, 70.0, 400.0, 230.0));
        // Left edge dragged past the right one: stops MIN_SIDE before it (no flip).
        let thin = dragged(
            Grip::Edge(-1, 0),
            r,
            Vector::new(900.0, 0.0),
            FRAME,
            MIN_SIDE,
        );
        assert_eq!(thin, rect(500.0 - MIN_SIDE, 100.0, MIN_SIDE, 200.0));
        // Out past the frame: clamped to it.
        let out = dragged(
            Grip::Edge(-1, -1),
            r,
            Vector::new(-900.0, -900.0),
            FRAME,
            MIN_SIDE,
        );
        assert_eq!(out, rect(0.0, 0.0, 500.0, 300.0));
        let out = dragged(
            Grip::Edge(1, 1),
            r,
            Vector::new(9000.0, 9000.0),
            FRAME,
            MIN_SIDE,
        );
        assert_eq!(out, rect(100.0, 100.0, 1180.0, 620.0));
    }

    #[test]
    fn arrows_move_shift_is_ten_ctrl_resizes_ctrl_alt_from_the_other_edge() {
        let r = rect(100.0, 100.0, 400.0, 200.0);
        let n = |d, m| nudge(r, d, m, 1.0, FRAME, MIN_SIDE);
        let plain = (false, false, false);
        assert_eq!(n((1, 0), plain), rect(101.0, 100.0, 400.0, 200.0));
        assert_eq!(
            n((0, -1), (false, true, false)),
            rect(100.0, 90.0, 400.0, 200.0)
        );
        // Ctrl: the right / bottom edge.
        assert_eq!(
            n((1, 0), (true, false, false)),
            rect(100.0, 100.0, 401.0, 200.0)
        );
        assert_eq!(
            n((-1, 0), (true, false, false)),
            rect(100.0, 100.0, 399.0, 200.0)
        );
        assert_eq!(
            n((0, 1), (true, true, false)),
            rect(100.0, 100.0, 400.0, 210.0)
        );
        // Ctrl+Alt: the left / top edge.
        assert_eq!(
            n((-1, 0), (true, false, true)),
            rect(99.0, 100.0, 401.0, 200.0)
        );
        assert_eq!(
            n((0, 1), (true, false, true)),
            rect(100.0, 101.0, 400.0, 199.0)
        );
        // A step is one SCREEN px: 1.5 image px on a 150 % monitor.
        let hi = nudge(r, (1, 0), plain, 1.5, FRAME, MIN_SIDE);
        assert_eq!(hi.x, 101.5);
        // Never off the frame.
        let edge = rect(0.0, 0.0, 400.0, 200.0);
        let left = nudge(edge, (-1, 0), (false, true, false), 1.0, FRAME, MIN_SIDE);
        assert_eq!(left, edge);
    }
}
