//! Snip: the capture flow editor (annotations on the picked area).
pub mod annotate;

use iced::{Point, Rectangle};

/// An edge snaps to a window / control edge this near (logical px).
pub const SNAP: f32 = 8.0;

/// `p` moved onto the nearest edge of the rectangles it is in or near, per axis,
/// if one is within `reach`.
pub fn snap<'a>(p: Point, rects: impl IntoIterator<Item = &'a Rectangle>, reach: f32) -> Point {
    let (mut x, mut y) = ((reach, p.x), (reach, p.y));
    for r in rects {
        let near = p.x >= r.x - reach
            && p.x <= r.x + r.width + reach
            && p.y >= r.y - reach
            && p.y <= r.y + r.height + reach;
        if !near {
            continue;
        }
        for edge in [r.x, r.x + r.width] {
            let d = (p.x - edge).abs();
            if d < x.0 {
                x = (d, edge);
            }
        }
        for edge in [r.y, r.y + r.height] {
            let d = (p.y - edge).abs();
            if d < y.0 {
                y = (d, edge);
            }
        }
    }
    Point::new(x.1, y.1)
}
