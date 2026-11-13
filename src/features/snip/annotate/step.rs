//! Numbered step: each click drops the next number. The number is the badge's
//! place among the steps, so deleting or undoing one renumbers the rest.
use iced::{Point, Vector};
use tiny_skia::PathBuilder;

use super::document::{Layer, halo, on};
use super::text;

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
