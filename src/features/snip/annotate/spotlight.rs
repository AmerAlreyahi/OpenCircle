//! Spotlight: dims everything except the chosen areas (all spotlights together).
use iced::Rectangle;
use tiny_skia::{FillRule, Mask, Rect, Transform};

use super::document::{Layer, rounded, solid};

/// The dim outside the spotlights.
pub const DIM: [u8; 4] = [0, 0, 0, 140];
/// The corners of a hole, image px (the live dim round a hole uses it too).
pub const RADIUS: f32 = 10.0;

pub fn paint(layer: &mut Layer, holes: &[Rectangle]) {
    let (w, h) = (layer.pm.width(), layer.pm.height());
    let Some(mut mask) = Mask::new(w, h) else {
        return;
    };
    let t = layer.t();
    for hole in holes {
        if let Some(path) = rounded(*hole, RADIUS) {
            mask.fill_path(&path, FillRule::Winding, true, t);
        }
    }
    mask.invert();
    if let Some(all) = Rect::from_xywh(0.0, 0.0, w as f32, h as f32) {
        layer
            .pm
            .fill_rect(all, &solid(DIM), Transform::identity(), Some(&mask));
    }
}
