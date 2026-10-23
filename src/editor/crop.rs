//! Crop: drag the part to keep. Non-destructive (the document keeps the whole
//! image and every item), so it is one cheap undo step.
use iced::{Point, Rectangle, Size};

use super::document::span;

/// Smaller crops (image px, either side) are a slip, not a crop.
const MIN_SIDE: f32 = 8.0;

/// The crop a drag from `a` to `b` asks for: whole pixels, inside `view`.
pub fn area(a: Point, b: Point, view: Rectangle) -> Option<Rectangle> {
    let r = span(&[a, b]).intersection(&view)?;
    let (x0, y0) = (r.x.round(), r.y.round());
    let (x1, y1) = ((r.x + r.width).round(), (r.y + r.height).round());
    (x1 - x0 >= MIN_SIDE && y1 - y0 >= MIN_SIDE)
        .then(|| Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0)))
}
