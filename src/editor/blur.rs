//! Blur: pixelates its region (passwords, codes, faces). Only the region's pixels
//! are read and written, once per repaint of that region.
use iced::Rectangle;

use super::document::Layer;

/// Pixel block size, image px. Big enough that text can't be read back.
pub const BLOCK: u32 = 12;

/// Averages `BLOCK`-sized cells of `area` (image px). The grid is anchored to the
/// area itself, so a partial repaint gives the same cells as a full one.
pub fn paint(layer: &mut Layer, area: Rectangle) {
    let (w, h) = (layer.pm.width() as i64, layer.pm.height() as i64);
    let (ox, oy) = (layer.origin.x as i64, layer.origin.y as i64);
    let (x0, y0) = (area.x.round() as i64, area.y.round() as i64);
    let (x1, y1) = (
        (area.x + area.width).round() as i64,
        (area.y + area.height).round() as i64,
    );
    let data = layer.pm.data_mut();
    let block = BLOCK as i64;
    for cy in (y0..y1).step_by(BLOCK as usize) {
        for cx in (x0..x1).step_by(BLOCK as usize) {
            // This cell in pixmap px, clipped to the pixmap.
            let (px0, py0) = ((cx - ox).max(0), (cy - oy).max(0));
            let (px1, py1) = (
                ((cx + block).min(x1) - ox).min(w),
                ((cy + block).min(y1) - oy).min(h),
            );
            if px0 >= px1 || py0 >= py1 {
                continue;
            }
            let pixels = || {
                (py0..py1).flat_map(move |y| (px0..px1).map(move |x| ((y * w + x) * 4) as usize))
            };
            let mut sum = [0u64; 4];
            for i in pixels() {
                for (s, v) in sum.iter_mut().zip(&data[i..i + 4]) {
                    *s += *v as u64;
                }
            }
            let n = ((px1 - px0) * (py1 - py0)) as u64;
            let mean = sum.map(|s| (s / n) as u8);
            for i in pixels() {
                data[i..i + 4].copy_from_slice(&mean);
            }
        }
    }
}
