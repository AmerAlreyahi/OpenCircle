//! Blur brush: a freehand stroke that softly blurs what is under it (feathered
//! edges, like a photo editor's blur brush), sized to the text line it covers.
//!
//! It must not be reversible. A plain blur (or mosaic) is a fixed, nearly
//! invertible function of the text: an attacker deconvolves it or renders
//! candidate strings and matches them (Depix). So the result is built from
//! information at a scale larger than the text, not from its pixels: the area is
//! averaged into coarse cells (one brush height, so each cell mixes a few
//! glyphs), the cell means are quantised to 4 bits per channel, and only then
//! smoothed (a Gaussian-like fade over the cell grid), with a small secret
//! per-cell tone offset (mostly luminance, a smaller independent part per channel) from the OS-random per-stroke seed (never from the
//! content) that drowns what the means still tell apart. Different texts give
//! the same result up to that. It looks like the content simply faded out: no grain,
//! no squares. The cell grid is anchored in image coordinates, so a repaint of
//! any region gives the same pixels. Full strength over the whole covered line;
//! only the edge outside it is feathered.
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use iced::{Point, Rectangle};
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Stroke, Transform};

use super::document::{Layer, span};
use super::pen;

/// A fresh secret seed (std's per-process OS-random hash keys; no new dependency).
pub fn seed() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u64(0x6f70_656e_6369_7263);
    h.finish()
}

/// Side of a mean cell, image px: one brush height (the text line height).
pub fn cell(size: f32) -> i64 {
    size.round().max(8.0) as i64
}

/// Box radius of each of the three smoothing passes (~ a Gaussian with sigma
/// ~ 0.6 x a cell): wide enough that the cell edges fade out completely.
pub fn radius(size: f32) -> usize {
    (0.5 * cell(size) as f32).ceil() as usize
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
    2.0 * feather(size) + 3.0 * radius(size) as f32 + cell(size) as f32 + 1.0
}

/// Blurs the stroke `points` (brush `size`, image px) in `layer`. Deterministic:
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
    let c = cell(size);
    // What is read: the area plus the smoothing margin, grown to whole cells of
    // the image-anchored grid (clipped to the layer), so the means don't depend
    // on where this paint starts.
    let (sx0, sy0, sx1, sy1) = clip(area.expand(3.0 * r as f32));
    let (oxi, oyi) = (ox.round() as i64, oy.round() as i64);
    let down = |v: i64, o: i64| (v + o).div_euclid(c) * c - o;
    let up = |v: i64, o: i64| -(-(v + o)).div_euclid(c) * c - o;
    let (sx0, sx1) = (down(sx0, oxi).clamp(0, w), up(sx1, oxi).clamp(0, w));
    let (sy0, sy1) = (down(sy0, oyi).clamp(0, h), up(sy1, oyi).clamp(0, h));
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
    let mut buf = vec![[0.0f32; 3]; sw * sh];
    // Quantised mean colour of each cell, spread over the cell.
    let mut cy = sy0;
    while cy < sy1 {
        let y1 = up(cy + 1, oyi).min(sy1);
        let mut cx = sx0;
        while cx < sx1 {
            let x1 = up(cx + 1, oxi).min(sx1);
            let mut sum = [0.0f32; 3];
            for y in cy..y1 {
                for x in cx..x1 {
                    let i = index(x, y);
                    (0..3).for_each(|k| sum[k] += data[i + k] as f32);
                }
            }
            let n = ((x1 - cx) * (y1 - cy)) as f32;
            // A secret per-cell tonal offset (grey, same on all channels): the
            // few levels the means still tell apart are drowned by it, and it is
            // smoothed with the rest, so it reads as a soft tone change.
            let (gx, gy) = ((cx + oxi).div_euclid(c), (cy + oyi).div_euclid(c));
            let cell_key = seed ^ mix(gx as u64 ^ (gy as u64) << 32);
            let shared = TONE * unit(mix(cell_key));
            let mut mean = [0.0f32; 3];
            for k in 0..3 {
                let own = 0.5 * TONE * unit(mix(cell_key ^ ((k as u64 + 1) * 0x9e37_79b9)));
                mean[k] = reflect(quantise(sum[k] / n) + shared + own);
            }
            for y in cy..y1 {
                buf[(y - sy0) as usize * sw + (cx - sx0) as usize..][..(x1 - cx) as usize]
                    .fill(mean);
            }
            cx = x1;
        }
        cy = y1;
    }
    blur2d(&mut buf, sw, sh, r, 3);

    for y in ay0..ay1 {
        for x in ax0..ax1 {
            let m = alpha[(y - ay0) as usize * aw + (x - ax0) as usize][0];
            if m <= 0.0 {
                continue;
            }
            let blurred = buf[(y - sy0) as usize * sw + (x - sx0) as usize];
            let i = index(x, y);
            for k in 0..3 {
                data[i + k] = (blurred[k] * m + data[i + k] as f32 * (1.0 - m)).round() as u8;
            }
            data[i + 3] = 255;
        }
    }
}

/// Secret tonal offset amplitude per cell, ± levels: clearly above what is left of
/// the text after the 4-bit cell means (about 1 level between two texts).
pub const TONE: f32 = 24.0;

/// A level folded back into 0..=255 at the ends (clipping would cut the offset
/// on a white or black background).
fn reflect(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else if v > 255.0 {
        510.0 - v
    } else {
        v
    }
}

/// A hash as a value in -1..=1.
fn unit(hash: u64) -> f32 {
    (hash >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0
}

/// splitmix64 finaliser: a well-mixed hash of `x`.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A level snapped to 4 bits (0 and 255 stay exact: white and black are kept).
fn quantise(v: f32) -> f32 {
    const STEP: f32 = 255.0 / 15.0;
    (v / STEP).round() * STEP
}

/// `passes` box blurs of radius `r` along rows then columns (edges clamped).
pub(crate) fn blur2d<const N: usize>(
    buf: &mut [[f32; N]],
    w: usize,
    h: usize,
    r: usize,
    passes: usize,
) {
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
