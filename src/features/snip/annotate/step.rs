//! Numbered step: each click drops the next number. The number is the badge's
//! place among the steps, so deleting or undoing one renumbers the rest.
use iced::{Point, Vector};
use tiny_skia::PathBuilder;

use super::document::{Layer, halo, on};
use super::text;
use crate::shared::picture::Crop;

pub fn paint(layer: &mut Layer, at: Point, radius: f32, number: u32, colour: [u8; 4]) {
    let Some(disc) = PathBuilder::from_circle(at.x, at.y, radius) else {
        return;
    };
    layer.stroke(&disc, halo(colour), (radius * 0.15).max(2.0));
    layer.fill(&disc, colour);
    let label = number.to_string();
    let size = radius * 1.1;
    let box_ = text::measure(&label, size);
    let top_left = at - Vector::new(box_.width / 2.0, box_.height / 2.0);
    text::paint(layer, top_left, &label, size, on(colour), false);
}

/// `crop` with `number` in a badge at its top-left corner (the walkthrough's numbered frames).
// ponytail: the frame is read as opaque (screenshots are); a see-through one gets a wrong edge only.
pub fn badge(crop: Crop, number: u32) -> Crop {
    let Some(mut pm) = tiny_skia::Pixmap::new(crop.width, crop.height) else {
        return crop;
    };
    pm.data_mut().copy_from_slice(&crop.rgba);
    let radius = (crop.width.min(crop.height) as f32 / 18.0).clamp(10.0, 36.0);
    let mut layer = Layer {
        pm,
        origin: Point::ORIGIN,
    };
    paint(
        &mut layer,
        Point::new(radius * 1.6, radius * 1.6),
        radius,
        number,
        [10, 132, 255, 255],
    );
    Crop {
        rgba: layer.pm.data().to_vec(),
        ..crop
    }
}
