//! Callout: a note bubble with a tail pointing at something; its text is editable.
use iced::{Point, Rectangle, Size, Vector};
use tiny_skia::PathBuilder;

use super::document::{Layer, halo, on, rounded};
use super::text;

fn padding(size: f32) -> Vector {
    Vector::new(size * 0.6, size * 0.45)
}

/// The bubble, centred on `at`, sized to the text.
pub fn bubble(at: Point, text: &str, size: f32) -> Rectangle {
    let inner = text::measure(text, size).max(Size::new(size * 3.0, 0.0));
    let pad = padding(size);
    let outer = Size::new(inner.width + 2.0 * pad.x, inner.height + 2.0 * pad.y);
    Rectangle::new(
        at - Vector::new(outer.width / 2.0, outer.height / 2.0),
        outer,
    )
}

/// Top-left of the text inside the bubble.
pub fn text_origin(at: Point, text: &str, size: f32) -> Point {
    bubble(at, text, size).position() + padding(size)
}

pub fn paint(layer: &mut Layer, tip: Point, at: Point, text: &str, size: f32, colour: [u8; 4]) {
    let body = bubble(at, text, size);
    let Some(shape) = rounded(body, size * 0.6) else {
        return;
    };
    // Tail: a triangle from the bubble's middle to the tip (hidden under the
    // bubble when the tip is inside it).
    let to_tip = tip - at;
    let len = (to_tip.x * to_tip.x + to_tip.y * to_tip.y).sqrt().max(1.0);
    let side = Vector::new(-to_tip.y / len, to_tip.x / len) * (body.height.min(body.width) * 0.22);
    let mut p = PathBuilder::new();
    p.move_to(at.x + side.x, at.y + side.y);
    p.line_to(tip.x, tip.y);
    p.line_to(at.x - side.x, at.y - side.y);
    p.close();
    let tail = p.finish();

    let ring = halo(colour);
    layer.stroke(&shape, ring, 3.0);
    if let Some(tail) = &tail {
        layer.stroke(tail, ring, 3.0);
        layer.fill(tail, colour);
    }
    layer.fill(&shape, colour);
    text::paint(
        layer,
        text_origin(at, text, size),
        text,
        size,
        on(colour),
        false,
    );
}
