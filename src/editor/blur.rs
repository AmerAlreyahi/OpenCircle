//! Blur: hides its region (passwords, codes, faces) behind a mosaic that can't be
//! reversed. A plain mean mosaic is NOT safe: its cells are a fixed function of
//! the text, so an attacker renders candidate strings and matches the cells
//! (Depix). So: a strong blur first, cells big enough that a text line is at most
//! ~3 cells tall, then per-cell random noise (secret, OS-random seed per item)
//! and coarse quantisation, so cell values carry less text than noise.
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use iced::Rectangle;

use super::document::Layer;

/// Smallest cell, image px.
pub const MIN_BLOCK: u32 = 12;
/// Per-cell noise, ± per channel.
const NOISE: u64 = 24;
/// Quantisation step; cell values are `STEP / 2 + k * STEP` (never 0 or 255).
const STEP: f32 = 16.0;

/// A fresh secret seed (std's per-process OS-random hash keys; no new dependency).
pub fn seed() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u64(0x6f70_656e_6369_7263);
    h.finish()
}

/// Cell size for `area`: a text line filling it is at most ~3 cells tall.
pub fn block(area: Rectangle) -> u32 {
    MIN_BLOCK.max((area.height / 3.0).ceil() as u32)
}

/// Replaces every pixel of `area` (image px) with its cell's noisy, quantised
/// colour. The grid is anchored to the area and the noise to (seed, cell), so a
/// repaint of the whole area gives the same cells.
// ponytail: a live layer that clips the area (another item dragged over half of
// it) blurs only the clipped part, so cells at that edge differ until the drag
// ends and the base repaints.
pub fn paint(layer: &mut Layer, area: Rectangle, seed: u64) {
    let (w, h) = (layer.pm.width() as i64, layer.pm.height() as i64);
    let (ox, oy) = (layer.origin.x as i64, layer.origin.y as i64);
    let (x0, y0) = (area.x.round() as i64, area.y.round() as i64);
    let (x1, y1) = (
        (area.x + area.width).round() as i64,
        (area.y + area.height).round() as i64,
    );
    // The area in pixmap px, clipped to the pixmap.
    let (px0, py0) = ((x0 - ox).max(0), (y0 - oy).max(0));
    let (px1, py1) = ((x1 - ox).min(w), (y1 - oy).min(h));
    if px0 >= px1 || py0 >= py1 {
        return;
    }
    let (rw, rh) = ((px1 - px0) as usize, (py1 - py0) as usize);
    let block = block(area) as i64;
    let data = layer.pm.data_mut();
    let index = |x: i64, y: i64| ((y * w + x) * 4) as usize;

    // Strong blur over the region (two box passes each way ≈ gaussian, radius
    // half a cell), reading only the region itself.
    let mut buf: Vec<[f32; 3]> = (py0..py1)
        .flat_map(|y| (px0..px1).map(move |x| (x, y)))
        .map(|(x, y)| {
            let i = index(x, y);
            [data[i] as f32, data[i + 1] as f32, data[i + 2] as f32]
        })
        .collect();
    let r = (block / 2) as usize;
    let mut line = Vec::new();
    for _ in 0..2 {
        for row in buf.chunks_mut(rw) {
            box_blur(row, r, &mut line);
        }
        let mut column = Vec::with_capacity(rh);
        for x in 0..rw {
            column.clear();
            column.extend((0..rh).map(|y| buf[y * rw + x]));
            box_blur(&mut column, r, &mut line);
            for (y, v) in column.iter().enumerate() {
                buf[y * rw + x] = *v;
            }
        }
    }

    for cy in (y0..y1).step_by(block as usize) {
        for cx in (x0..x1).step_by(block as usize) {
            // This cell in region px, clipped.
            let (qx0, qy0) = ((cx - ox).max(px0) - px0, (cy - oy).max(py0) - py0);
            let qx1 = ((cx + block).min(x1) - ox).min(px1) - px0;
            let qy1 = ((cy + block).min(y1) - oy).min(py1) - py0;
            if qx0 >= qx1 || qy0 >= qy1 {
                continue;
            }
            let cells = || (qy0..qy1).flat_map(move |y| (qx0..qx1).map(move |x| (x, y)));
            let mut sum = [0.0f32; 3];
            for (x, y) in cells() {
                let v = buf[y as usize * rw + x as usize];
                (0..3).for_each(|c| sum[c] += v[c]);
            }
            let n = ((qx1 - qx0) * (qy1 - qy0)) as f32;
            let cell = ((cx - x0) / block) as u64 | (((cy - y0) / block) as u64) << 32;
            let colour: [u8; 3] = std::array::from_fn(|c| {
                let noise = (mix(seed ^ mix(cell) ^ c as u64) % (2 * NOISE + 1)) as f32;
                let v = sum[c] / n + noise - NOISE as f32;
                let k = (v / STEP).floor().clamp(0.0, 256.0 / STEP - 1.0);
                (k * STEP + STEP / 2.0) as u8
            });
            for (x, y) in cells() {
                let i = index(x + px0, y + py0);
                data[i..i + 4].copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
            }
        }
    }
}

/// Box blur of radius `r` along `values`, edges clamped.
fn box_blur(values: &mut [[f32; 3]], r: usize, tmp: &mut Vec<[f32; 3]>) {
    let n = values.len() as isize;
    tmp.clear();
    tmp.extend_from_slice(values);
    let at = |i: isize| tmp[i.clamp(0, n - 1) as usize];
    let r = r as isize;
    let mut sum = [0.0f32; 3];
    for i in -r..=r {
        (0..3).for_each(|c| sum[c] += at(i)[c]);
    }
    let k = (2 * r + 1) as f32;
    for (i, v) in values.iter_mut().enumerate() {
        let i = i as isize;
        *v = sum.map(|s| s / k);
        let (add, sub) = (at(i + r + 1), at(i - r));
        (0..3).for_each(|c| sum[c] += add[c] - sub[c]);
    }
}

/// splitmix64 finaliser: a well-mixed hash of `x`.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}
