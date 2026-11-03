//! Blur brush: a fine pixel mosaic along a text line (owner, 2026-10-31: like
//! iTop's pixelate, softer and with smaller cells); line help keeps a stroke on
//! its line.
//!
//! It must not be reversible: a mosaic of fine cell means is a function of the
//! glyph shapes, and an attacker renders candidate strings and scores them
//! (Depix, or a likelihood over all cells). Even coarse block means (two or
//! three glyphs each) left the true text ranked first more often than chance
//! on Windows. So the source is read ONCE, for the whole stroke: its mean tone
//! and one tint (`tint`: the chroma of its ink / background pair, snapped
//! coarsely, none for grey content, plus a secret per-stroke jitter).
//! The stroke's tone gets a secret offset, is snapped to `TONE_STEP` with a
//! secret dither (its last levels are gone) and is kept so far inside 0..=255
//! that no noise below ever clips or folds (a fold would let a fitted offset
//! tell tones apart). The mosaic is then drawn from secrets only: coarse
//! blocks one brush tall and 2.5 brushes wide, each that tone plus a secret
//! offset; each fine cell's tone interpolated between them, plus a secret
//! jitter per cell, all from an OS-random seed (never from the content);
//! its colour = that tone in grey + the stroke's tint, the same on every cell
//! (no colour noise, no cast: grey stays grey, a blue link stays blue). The
//! cell grid is softened a little (two box passes of a quarter cell). The
//! footprint is a flat band `size` tall along the stroke (square ends, no
//! round blob), sharp at its edges. A repaint of any region holding the
//! stroke gives the same pixels (blocks, cells and jitter belong to the
//! image's coordinates and the stroke's seed).
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use iced::{Point, Rectangle};
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Stroke, Transform};

use super::document::{Layer, span};
use super::pen;

/// A fresh secret seed from the OS CSPRNG (std's OS-random hash keys if that
/// fails, still never from the content).
pub fn seed() -> u64 {
    getrandom::u64().unwrap_or_else(|_| {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(0x6f70_656e_6369_7263);
        h.finish()
    })
}

/// Secret tone offset per block, ± levels.
pub const BLOCK_NOISE: f32 = 20.0;
/// Secret tone jitter per cell, ± levels.
pub const JITTER: f32 = 14.0;
/// Secret tone offset for the whole stroke, ± levels: how much ink the text
/// has (its mean tone) is hidden under it, also where black and white fold
/// the tone back (a fitted offset can't undo a fold).
pub const STROKE_NOISE: f32 = 32.0;
/// The stroke's tone is snapped to steps of this many levels.
pub const TONE_STEP: f32 = 16.0;
/// A block is this many brushes wide (about two or three glyphs).
pub const BLOCK_WIDTH: f32 = 2.5;

/// Side of a mosaic cell, image px (at least 2).
pub fn cell(cell: f32) -> i64 {
    cell.round().max(2.0) as i64
}

/// A block's (width, height), image px, for a brush `size` tall.
pub fn block(size: f32) -> (i64, i64) {
    let h = size.round().max(4.0) as i64;
    ((BLOCK_WIDTH * h as f32).round() as i64, h)
}

/// Box radius of each of the two softening passes: a quarter cell.
pub fn soften(cell_px: i64) -> usize {
    (cell_px as usize / 4).max(1)
}

/// What the stroke covers (its points, `size` tall, square ends; a diagonal
/// stroke's corners reach a little further).
pub fn frame(points: &[Point], size: f32) -> Rectangle {
    span(points).expand(size * std::f32::consts::FRAC_1_SQRT_2 + 1.0)
}

/// Beyond `frame`: what the mosaic reads (the softening, whole cells, and the
/// blocks the cells are interpolated between: up to two blocks each way).
pub fn reach(size: f32, cell_px: f32) -> f32 {
    let c = cell(cell_px);
    let (bw, bh) = block(size);
    (2 * soften(c)) as f32 + c as f32 + 2.0 * bw.max(bh) as f32 + 1.0
}

/// One coarse block: the stroke's tone with its secret offset, snapped.
#[derive(Clone, Copy)]
struct Block {
    tone: f32,
}

/// Pixelates the stroke `points` (band `size` tall, cells `cell_px` wide,
/// image px; `seed`: the stroke's secret) in `layer`.
pub fn paint(layer: &mut Layer, points: &[Point], size: f32, cell_px: f32, seed: u64) {
    let (w, h) = (layer.pm.width() as i64, layer.pm.height() as i64);
    let (ox, oy) = (layer.origin.x, layer.origin.y);
    let clip = |r: Rectangle| {
        let x0 = ((r.x - ox).floor() as i64).clamp(0, w);
        let y0 = ((r.y - oy).floor() as i64).clamp(0, h);
        let x1 = ((r.x + r.width - ox).ceil() as i64).clamp(0, w);
        let y1 = ((r.y + r.height - oy).ceil() as i64).clamp(0, h);
        (x0, y0, x1, y1)
    };
    let area = frame(points, size);
    let (ax0, ay0, ax1, ay1) = clip(area);
    if ax0 >= ax1 || ay0 >= ay1 {
        return;
    }
    let c = cell(cell_px);
    let r = soften(c);
    let (oxi, oyi) = (ox.round() as i64, oy.round() as i64);
    // Grid snapping (image-anchored): `n` px cells or blocks, layer px `v`.
    let down = |v: i64, o: i64, n: i64| (v + o).div_euclid(n) * n - o;
    let up = |v: i64, o: i64, n: i64| -(-(v + o)).div_euclid(n) * n - o;
    // The cells drawn: the area plus the softening margin, whole cells.
    let (sx0, sy0, sx1, sy1) = clip(area.expand((2 * r) as f32));
    let (sx0, sx1) = (down(sx0, oxi, c).clamp(0, w), up(sx1, oxi, c).clamp(0, w));
    let (sy0, sy1) = (down(sy0, oyi, c).clamp(0, h), up(sy1, oyi, c).clamp(0, h));
    let (aw, ah) = ((ax1 - ax0) as usize, (ay1 - ay0) as usize);
    let (sw, sh) = ((sx1 - sx0) as usize, (sy1 - sy0) as usize);

    // The footprint: the stroke `size` wide with square ends (hard-edged).
    let Some(mut mask) = Mask::new(aw as u32, ah as u32) else {
        return;
    };
    let Some(path) = pen::smooth(points) else {
        return;
    };
    let stroke = Stroke {
        width: size,
        line_cap: LineCap::Square,
        line_join: LineJoin::Bevel,
        ..Stroke::default()
    };
    let Some(outline) = path.stroke(&stroke, 1.0) else {
        return;
    };
    let to_mask = Transform::from_translate(-(ox + ax0 as f32), -(oy + ay0 as f32));
    // Not anti-aliased: a half-covered rim pixel would show half of its source
    // (a glyph's edge) through the mosaic.
    mask.fill_path(&outline, FillRule::Winding, false, to_mask);

    // 1. The ONLY reading of the source: what is under the footprint gives
    // one tone and one tint for the whole stroke.
    let (tone, tint) = {
        let data = layer.pm.data();
        let under: Vec<[u8; 3]> = (ay0..ay1)
            .flat_map(|y| (ax0..ax1).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                let m = mask.data()[(y - ay0) as usize * aw + (x - ax0) as usize];
                let i = ((y * w + x) * 4) as usize;
                (m > 0).then(|| [data[i], data[i + 1], data[i + 2]])
            })
            .collect();
        let n = under.len().max(1) as f32;
        let tone = under.iter().map(|p| luma(p.map(f32::from))).sum::<f32>() / n;
        (tone, tint(colours(under.into_iter()), seed))
    };
    // The stroke's tone + its secret offset, kept so far inside 0..=255 (and
    // so that its tint fits) that no cell's noise ever clips or folds: the
    // noise is then purely additive, and a fitted offset tells nothing apart.
    let (lo, hi) = (
        tint.iter().fold(0.0f32, |m, &t| m.min(t)),
        tint.iter().fold(0.0f32, |m, &t| m.max(t)),
    );
    let noise = BLOCK_NOISE + JITTER;
    let (floor, ceiling) = (noise - lo, 255.0 - noise - hi);
    // Snapped to `TONE_STEP` with a secret dither: what is left of the mean
    // after the offset (its last few levels) is gone, not just hidden.
    let dither = TONE_STEP / 2.0 * unit(mix(seed ^ 0x2545_f491));
    let shifted = tone + STROKE_NOISE * unit(mix(seed ^ 0x5bd1_e995)) + dither;
    let stroke_tone = (shifted / TONE_STEP).round() * TONE_STEP - dither;
    let tone = match floor <= ceiling {
        true => stroke_tone.max(floor).min(ceiling),
        false => (floor + ceiling) / 2.0,
    };
    // 2. The coarse blocks (columns on the image's grid; rows on the stroke's
    // band, its start's middle): the stroke's tone, each with a secret
    // offset. They give the mosaic its patches; no content.
    let (bw, bh) = block(size);
    let band = (points[0].y - size / 2.0).round() as i64;
    let oyb = oyi - band;
    let (gx0, gx1) = (
        (sx0 + oxi).div_euclid(bw) - 1,
        (sx1 + oxi).div_euclid(bw) + 1,
    );
    let (gy0, gy1) = (
        (sy0 + oyb).div_euclid(bh) - 1,
        (sy1 + oyb).div_euclid(bh) + 1,
    );
    let gw = (gx1 - gx0 + 1) as usize;
    let mut blocks = Vec::with_capacity(gw * (gy1 - gy0 + 1) as usize);
    for gy in gy0..=gy1 {
        for gx in gx0..=gx1 {
            let key = mix(!seed ^ mix(gx as u64 ^ (gy as u64) << 32));
            blocks.push(Block {
                tone: tone + BLOCK_NOISE * unit(key),
            });
        }
    }
    let at = |gx: i64, gy: i64| {
        blocks[(gy.clamp(gy0, gy1) - gy0) as usize * gw + (gx.clamp(gx0, gx1) - gx0) as usize]
    };

    // 3. The fine cells, from the blocks only.
    let data = layer.pm.data_mut();
    let index = |x: i64, y: i64| ((y * w + x) * 4) as usize;
    let mut buf = vec![[0.0f32; 3]; sw * sh];
    let mut cy = sy0;
    while cy < sy1 {
        let y1 = up(cy + 1, oyi, c).min(sy1);
        let mut cx = sx0;
        while cx < sx1 {
            let x1 = up(cx + 1, oxi, c).min(sx1);
            // The cell's middle in block units: across, between the blocks'
            // middles; down, its own row (the line's, inside the band).
            let fx = ((cx + x1) as f32 / 2.0 + oxi as f32) / bw as f32 - 0.5;
            let iy = ((cy + y1) / 2 + oyb).div_euclid(bh);
            let ix = fx.floor() as i64;
            let tx = fx - ix as f32;
            let tone = (1.0 - tx) * at(ix, iy).tone + tx * at(ix + 1, iy).tone;
            let (gx, gy) = ((cx + oxi).div_euclid(c), (cy + oyi).div_euclid(c));
            let jitter = JITTER * unit(mix(seed ^ mix(gx as u64 ^ (gy as u64) << 32)));
            // (In range by construction; the clamp only guards rounding.)
            let value = tint.map(|t| (tone + jitter + t).clamp(0.0, 255.0));
            for y in cy..y1 {
                buf[(y - sy0) as usize * sw + (cx - sx0) as usize..][..(x1 - cx) as usize]
                    .fill(value);
            }
            cx = x1;
        }
        cy = y1;
    }
    blur2d(&mut buf, sw, sh, r, 2);

    for y in ay0..ay1 {
        for x in ax0..ax1 {
            let m = mask.data()[(y - ay0) as usize * aw + (x - ax0) as usize] as f32 / 255.0;
            if m <= 0.0 {
                continue;
            }
            let cell = buf[(y - sy0) as usize * sw + (x - sx0) as usize];
            let i = index(x, y);
            for k in 0..3 {
                data[i + k] = (cell[k] * m + data[i + k] as f32 * (1.0 - m)).round() as u8;
            }
            data[i + 3] = 255;
        }
    }
}

/// The stroke's ONE colour pair (owner's look, review: never per block, or a
/// coloured word would show in its block): (ink, background). The background
/// = the most common exact colour under the footprint (a field is one colour;
/// text never covers most of a line). The ink = per channel, the most extreme
/// value away from the background among the 2 % of pixels furthest from it in
/// tone (each channel is whole somewhere on a stem, however little text there
/// is and even with ClearType's per-channel fringes). Neither tracks how much
/// ink the text has; what is left of that is snapped away in `snapped_tint`.
pub(crate) fn colours(pixels: impl Iterator<Item = [u8; 3]>) -> ([f32; 3], [f32; 3]) {
    let mut all: Vec<(f32, [u8; 3])> = pixels.map(|p| (luma(p.map(f32::from)), p)).collect();
    if all.is_empty() {
        return ([128.0; 3], [128.0; 3]);
    }
    let mut count = std::collections::HashMap::new();
    for (_, p) in &all {
        *count.entry(*p).or_insert(0u32) += 1;
    }
    // Ties: the lower value (deterministic).
    let bg = (count.into_iter())
        .max_by_key(|&(p, n)| (n, std::cmp::Reverse(p)))
        .map_or([128; 3], |(p, _)| p);
    let bg_luma = luma(bg.map(f32::from));
    all.sort_by(|a, b| (b.0 - bg_luma).abs().total_cmp(&(a.0 - bg_luma).abs()));
    let far = &all[..(all.len() / 50).max(1)];
    let darker = far[0].0 < bg_luma;
    let ink = [0, 1, 2].map(|k| {
        let v = far.iter().map(|(_, p)| p[k]);
        (if darker { v.min() } else { v.max() }).unwrap_or(bg[k]) as f32
    });
    (ink, bg.map(f32::from))
}

/// The chroma of the middle of the stroke's pair, snapped to `TINT_STEP`
/// (coarser than the jitter; under half a step on every channel: none, so
/// grey text and ClearType fringes on it stay grey).
pub(crate) fn snapped_tint((ink, bg): ([f32; 3], [f32; 3])) -> [f32; 3] {
    let mid = [0, 1, 2].map(|k| (ink[k] + bg[k]) / 2.0);
    let y = luma(mid);
    mid.map(|v| ((v - y) / TINT_STEP).round() * TINT_STEP)
}

/// The stroke's ONE tint, the same on every cell whatever its tone, so no
/// cell's colour carries anything of its own: a cell = its tone in grey + this
/// tint. `snapped_tint`, plus (when there is one) a secret per-stroke jitter
/// added after the snap; its spread is kept within 255 levels so a grey tone
/// always fits.
fn tint(pair: ([f32; 3], [f32; 3]), seed: u64) -> [f32; 3] {
    let snapped = snapped_tint(pair);
    if snapped == [0.0; 3] {
        return snapped;
    }
    let jitter = [0u64, 1, 2].map(|k| TINT_JITTER * unit(mix(seed ^ (0x7f4a_7c15 + k))));
    let jy = luma(jitter);
    let t = [0, 1, 2].map(|k| snapped[k] + jitter[k] - jy);
    let spread =
        t.iter().fold(f32::MIN, |m, &v| m.max(v)) - t.iter().fold(f32::MAX, |m, &v| m.min(v));
    match spread > 255.0 {
        true => t.map(|v| v * 255.0 / spread),
        false => t,
    }
}

/// The stroke's tint is snapped to steps of this many levels.
pub const TINT_STEP: f32 = 48.0;
/// Secret per-stroke tint jitter, ± levels per channel (after the snap).
pub const TINT_JITTER: f32 = 8.0;

fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
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

/// Line help: where a stroke held at height `held` goes for pointer `p`
/// (brush `size`, image px). It stays on its line through any wobble; only a
/// clear move up or down (more than one and a half brush heights) takes it
/// to the pointer's height, held there from then on.
pub fn follow(held: &mut f32, p: Point, size: f32) -> Point {
    if (p.y - *held).abs() > 1.5 * size {
        *held = p.y;
    }
    Point::new(p.x, *held)
}
