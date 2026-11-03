//! Label: text on a rounded fill of its colour; the text is dark or white,
//! whichever reads on that fill (`document::on`). Also an arrow's label and the
//! board's capture numbers.
use iced::{Point, Rectangle, Size, Vector};

use super::document::{Layer, halo, on, rounded};
use super::text;

fn padding(size: f32) -> Vector {
    Vector::new(size * 0.45, size * 0.2)
}

/// The label's box, top-left at `at`, sized to the text.
pub fn bubble(at: Point, text: &str, size: f32) -> Rectangle {
    let inner = text::measure(text, size);
    let pad = padding(size);
    Rectangle::new(
        at,
        Size::new(inner.width + 2.0 * pad.x, inner.height + 2.0 * pad.y),
    )
}

/// Top-left of the text inside the box.
pub fn text_origin(at: Point, size: f32) -> Point {
    at + padding(size)
}

/// The top-left that centres the label on `centre`.
pub fn centred(centre: Point, text: &str, size: f32) -> Point {
    let b = bubble(centre, text, size);
    centre - Vector::new(b.width / 2.0, b.height / 2.0)
}

/// An arrow label's font size, from the arrow's stroke width.
pub fn arrow_size(width: f32) -> f32 {
    (width * 4.0).max(10.0)
}

/// `ring`: with its halo (an arrow's label follows its arrow's outline).
pub fn paint(layer: &mut Layer, at: Point, text: &str, size: f32, colour: [u8; 4], ring: bool) {
    let body = bubble(at, text, size);
    let Some(shape) = rounded(body, size * 0.4) else {
        return;
    };
    if ring {
        layer.stroke(&shape, halo(colour), 2.5);
    }
    layer.fill(&shape, colour);
    text::paint(layer, text_origin(at, size), text, size, on(colour), false);
}
