//! Spotlight: dims everything except the chosen areas (all spotlights together).
use iced::Rectangle;
use tiny_skia::{FillRule, Mask, Rect, Transform};

use super::document::{Layer, rounded, solid};

/// The dim outside the spotlights.
pub const DIM: [u8; 4] = [0, 0, 0, 140];

pub fn paint(layer: &mut Layer, holes: &[Rectangle]) {
    let (w, h) = (layer.pm.width(), layer.pm.height());
    let Some(mut mask) = Mask::new(w, h) else {
        return;
    };
    let t = layer.t();
    for hole in holes {
        if let Some(path) = rounded(*hole, 10.0) {
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
