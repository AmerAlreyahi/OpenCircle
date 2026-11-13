//! Highlighter: a wide, translucent stroke (one path, so crossings don't darken).
use iced::Point;

use super::{document::Layer, pen};

/// Opacity of the highlight.
const ALPHA: u8 = 100;

pub fn paint(layer: &mut Layer, points: &[Point], colour: [u8; 4], width: f32) {
    if let Some(path) = pen::smooth(points) {
        layer.stroke(&path, [colour[0], colour[1], colour[2], ALPHA], width);
    }
}
