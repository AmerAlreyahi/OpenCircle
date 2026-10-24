//! Blur brush: a freehand stroke that softly blurs what is under it (feathered
//! edges, like a photo editor's blur brush), sized to the text line it covers.
//!
//! It must not be reversible. A plain blur or mosaic is a fixed function of the
//! text, so an attacker renders candidate strings and matches them (Depix). So:
//! a strong Gaussian (sigma >= 0.6 x the brush height, i.e. the line height:
//! glyph shapes are gone, only a smear of the line's average remains), then
//! per-pixel noise from a secret OS-random seed and a coarse quantisation, so
//! what is left of the text is smaller than the noise.
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use iced::{Point, Rectangle};
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Stroke, Transform};

use super::document::{Layer, span};
use super::pen;

/// Per-pixel noise, ± per channel.
const NOISE: u64 = 14;
/// Quantisation step; values are `STEP / 2 + k * STEP`.
const STEP: f32 = 6.0;

/// A fresh secret seed (std's per-process OS-random hash keys; no new dependency).
pub fn seed() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u64(0x6f70_656e_6369_7263);
    h.finish()
}

/// Box radius of each of the three blur passes for a brush `size` (three box
/// passes of radius r ≈ a Gaussian with sigma = sqrt(r (r + 1)) >= 0.6 x size).
pub fn radius(size: f32) -> usize {
    (0.6 * size).ceil().max(4.0) as usize
}

/// The soft edge, image px: the mask fades over this distance.
pub fn feather(size: f32) -> f32 {
    (size * 0.2).max(2.0)
}

/// What the stroke covers at full strength (its points ± size / 2).
pub fn frame(points: &[Point], size: f32) -> Rectangle {
    span(points).expand(size / 2.0)
}

/// Beyond `frame`: the soft edge plus what the blur reads.
pub fn reach(size: f32) -> f32 {
    2.0 * feather(size) + 3.0 * radius(size) as f32 + 1.0
}

/// Blurs the stroke `points` (brush `size`, image px) in `layer`. Deterministic
/// for (seed, pixel): a repaint of any region gives the same pixels.
pub fn paint(layer: &mut Layer, points: &[Point], size: f32, seed: u64) {
    let (w, h) = (layer.pm.width() as i64, layer.pm.height() as i64);
    let (ox, oy) = (layer.origin.x, layer.origin.y);
    let f = feather(size);
    // The painted area and the area read by the blur, in pixmap px, clipped.
    let clip = |r: Rectangle| {
        let x0 = ((r.x - ox).floor() as i64).clamp(0, w);
        let y0 = ((r.y - oy).floor() as i64).clamp(0, h);
        let x1 = ((r.x + r.width - ox).ceil() as i64).clamp(0, w);
        let y1 = ((r.y + r.height - oy).ceil() as i64).clamp(0, h);
        (x0, y0, x1, y1)
    };
    let area = frame(points, size).expand(2.0 * f);
    let (ax0, ay0, ax1, ay1) = clip(area);
    if ax0 >= ax1 || ay0 >= ay1 {
        return;
    }
    let r = radius(size);
    let (sx0, sy0, sx1, sy1) = clip(area.expand(3.0 * r as f32));
    let (aw, ah) = ((ax1 - ax0) as usize, (ay1 - ay0) as usize);
    let (sw, sh) = ((sx1 - sx0) as usize, (sy1 - sy0) as usize);

    // The mask: the stroke widened by 2f, softened by two box passes of f / 2:
    // full strength `size` wide, fading to 0 over 2f outside that.
    let Some(mut mask) = Mask::new(aw as u32, ah as u32) else {
        return;
    };
    let Some(path) = pen::smooth(points) else {
        return;
    };
    let stroke = Stroke {
        width: size + 2.0 * f,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let Some(outline) = path.stroke(&stroke, 1.0) else {
        return;
    };
    let to_mask = Transform::from_translate(-(ox + ax0 as f32), -(oy + ay0 as f32));
    mask.fill_path(&outline, FillRule::Winding, true, to_mask);
    let mut alpha: Vec<[f32; 1]> = mask.data().iter().map(|&m| [m as f32 / 255.0]).collect();
    let soft = (f / 2.0).round() as usize;
    blur2d(&mut alpha, aw, ah, soft, 2);

    let data = layer.pm.data_mut();
    let index = |x: i64, y: i64| ((y * w + x) * 4) as usize;
    let mut buf: Vec<[f32; 3]> = (sy0..sy1)
        .flat_map(|y| (sx0..sx1).map(move |x| (x, y)))
        .map(|(x, y)| {
            let i = index(x, y);
            [data[i] as f32, data[i + 1] as f32, data[i + 2] as f32]
        })
        .collect();
    blur2d(&mut buf, sw, sh, r, 3);

    for y in ay0..ay1 {
        for x in ax0..ax1 {
            let m = alpha[(y - ay0) as usize * aw + (x - ax0) as usize][0];
            if m <= 0.0 {
                continue;
            }
            let blurred = buf[(y - sy0) as usize * sw + (x - sx0) as usize];
            // The noise is keyed to the image pixel, not the pixmap one.
            let (ix, iy) = ((x as f32 + ox) as i64 as u64, (y as f32 + oy) as i64 as u64);
            // One noise value per pixel (grey grain, no colour speckles).
            let noise = (mix(seed ^ mix(ix | iy << 32)) % (2 * NOISE + 1)) as f32 - NOISE as f32;
            let i = index(x, y);
            for c in 0..3 {
                let k = ((blurred[c] + noise) / STEP)
                    .floor()
                    .clamp(0.0, 255.0 / STEP - 1.0);
                let v = k * STEP + STEP / 2.0;
                data[i + c] = (v * m + data[i + c] as f32 * (1.0 - m)).round() as u8;
            }
            data[i + 3] = 255;
        }
    }
}

/// `passes` box blurs of radius `r` along rows then columns (edges clamped).
fn blur2d<const N: usize>(buf: &mut [[f32; N]], w: usize, h: usize, r: usize, passes: usize) {
    if r == 0 || w == 0 || h == 0 {
        return;
    }
    let mut line = Vec::new();
    let mut column = Vec::with_capacity(h);
    for _ in 0..passes {
        for row in buf.chunks_mut(w) {
            box_blur(row, r, &mut line);
        }
        for x in 0..w {
            column.clear();
            column.extend((0..h).map(|y| buf[y * w + x]));
            box_blur(&mut column, r, &mut line);
            for (y, v) in column.iter().enumerate() {
                buf[y * w + x] = *v;
            }
        }
    }
}

/// Box blur of radius `r` along `values`, edges clamped.
fn box_blur<const N: usize>(values: &mut [[f32; N]], r: usize, tmp: &mut Vec<[f32; N]>) {
    let n = values.len() as isize;
    tmp.clear();
    tmp.extend_from_slice(values);
    let at = |i: isize| tmp[i.clamp(0, n - 1) as usize];
    let r = r as isize;
    let mut sum = [0.0f32; N];
    for i in -r..=r {
        (0..N).for_each(|c| sum[c] += at(i)[c]);
    }
    let k = (2 * r + 1) as f32;
    for (i, v) in values.iter_mut().enumerate() {
        let i = i as isize;
        *v = sum.map(|s| s / k);
        let (add, sub) = (at(i + r + 1), at(i - r));
        (0..N).for_each(|c| sum[c] += add[c] - sub[c]);
    }
}

/// splitmix64 finaliser: a well-mixed hash of `x`.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}
