//! Pointer: a ring with a short arrow into it, "look here" without a number.
use iced::{Point, Rectangle, Vector};
use tiny_skia::PathBuilder;

use super::arrow;
use super::document::{Layer, halo, span};

/// Where the arrow comes from (up-left of the ring) and where it stops.
fn arrow_ends(at: Point, radius: f32) -> (Point, Point) {
    let dir = Vector::new(-0.6, -0.8);
    (at + dir * (radius * 3.6), at + dir * (radius * 1.3))
}

pub fn frame(at: Point, radius: f32) -> Rectangle {
    let (tail, _) = arrow_ends(at, radius);
    let r = Vector::new(radius, radius);
    span(&[tail, at - r, at + r])
}

pub fn paint(layer: &mut Layer, at: Point, radius: f32, colour: [u8; 4], width: f32) {
    let (tail, tip) = arrow_ends(at, radius);
    let mid = Point::new((tail.x + tip.x) / 2.0, (tail.y + tip.y) / 2.0);
    arrow::paint(layer, (tail, tip, mid), true, colour, width, true);
    let Some(ring) = PathBuilder::from_circle(at.x, at.y, radius) else {
        return;
    };
    layer.stroke(&ring, halo(colour), width + 3.0);
    layer.stroke(&ring, colour, width);
    if let Some(dot) = PathBuilder::from_circle(at.x, at.y, (radius * 0.28).max(1.5)) {
        layer.fill(&dot, colour);
    }
}
