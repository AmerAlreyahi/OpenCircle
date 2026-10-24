//! Arrow (and plain line, and an arrow with a label at its tail): straight, or curved by dragging its middle handle.
//! Rounded head, white halo so it reads on any background.
use iced::{Point, Vector};
use tiny_skia::PathBuilder;

use super::document::{Layer, halo};

/// Head length for a stroke `width`.
pub fn head_length(width: f32) -> f32 {
    width * 3.0 + 8.0
}

/// The curve's point halfway (t = 0.5): where the bend handle sits.
pub fn middle(a: Point, b: Point, bend: Point) -> Point {
    Point::new(
        0.25 * a.x + 0.5 * bend.x + 0.25 * b.x,
        0.25 * a.y + 0.5 * bend.y + 0.25 * b.y,
    )
}

/// `b` moved onto the nearest 0/45/90° ray from `a`, same length (Shift).
pub fn snap_angle(a: Point, b: Point) -> Point {
    let d = b - a;
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (d.y.atan2(d.x) / step).round() * step;
    let len = d.x.hypot(d.y);
    Point::new(a.x + len * angle.cos(), a.y + len * angle.sin())
}

/// The control point that makes the curve pass through `handle` halfway.
pub fn bend_through(a: Point, b: Point, handle: Point) -> Point {
    Point::new(
        2.0 * handle.x - (a.x + b.x) / 2.0,
        2.0 * handle.y - (a.y + b.y) / 2.0,
    )
}

pub fn paint(
    layer: &mut Layer,
    a: Point,
    b: Point,
    bend: Point,
    head: bool,
    colour: [u8; 4],
    width: f32,
) {
    // Direction the curve arrives at `b`.
    let along = if bend.distance(b) > 0.5 {
        b - bend
    } else {
        b - a
    };
    let len = (along.x * along.x + along.y * along.y).sqrt();
    if len == 0.0 {
        return;
    }
    let dir = Vector::new(along.x / len, along.y / len);
    let head_len = if head {
        head_length(width).min(a.distance(b) * 0.6)
    } else {
        0.0
    };
    // The shaft stops inside the head so its round cap doesn't poke out of the tip.
    let end = b - dir * (head_len * 0.7);

    let mut shaft = PathBuilder::new();
    shaft.move_to(a.x, a.y);
    shaft.quad_to(bend.x, bend.y, end.x, end.y);
    let shaft = shaft.finish();

    let tip = head.then(|| {
        let base = b - dir * head_len;
        let side = Vector::new(-dir.y, dir.x) * (head_len * 0.6);
        let mut p = PathBuilder::new();
        p.move_to(b.x, b.y);
        p.line_to(base.x + side.x, base.y + side.y);
        p.line_to(base.x - side.x, base.y - side.y);
        p.close();
        p.finish()
    });

    let ring = halo(colour);
    let joint = width * 0.6;
    if let Some(shaft) = &shaft {
        layer.stroke(shaft, ring, width + 3.0);
    }
    if let Some(Some(tip)) = &tip {
        layer.stroke(tip, ring, joint + 3.0);
    }
    if let Some(shaft) = &shaft {
        layer.stroke(shaft, colour, width);
    }
    if let Some(Some(tip)) = &tip {
        layer.fill(tip, colour);
        layer.stroke(tip, colour, joint); // rounds the corners
    }
}
