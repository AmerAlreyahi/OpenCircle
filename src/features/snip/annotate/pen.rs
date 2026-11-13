//! Pen: a smoothed freehand stroke. On release the editor offers it to the smart
//! shape (`shape::snap`) first.
use iced::Point;
use tiny_skia::{Path, PathBuilder};

use super::document::Layer;

pub fn paint(layer: &mut Layer, points: &[Point], colour: [u8; 4], width: f32) {
    if let Some(path) = smooth(points) {
        layer.stroke(&path, colour, width);
    }
}

/// The stroke as quadratic curves through the segment midpoints; a single point
/// becomes a dot.
pub fn smooth(points: &[Point]) -> Option<Path> {
    let (&first, &last) = (points.first()?, points.last()?);
    let mut p = PathBuilder::new();
    p.move_to(first.x, first.y);
    if points.len() == 1 {
        p.line_to(first.x + 0.01, first.y);
    }
    for w in points.windows(2) {
        let mid = Point::new((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
        p.quad_to(w[0].x, w[0].y, mid.x, mid.y);
    }
    p.line_to(last.x, last.y);
    p.finish()
}
