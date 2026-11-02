//! Blur brush: a fine pixel mosaic along a text line (owner, 2026-10-31: like
//! iTop's pixelate, softer and with smaller cells); line help keeps a stroke on
//! its line.
//!
//! It must not be reversible: a mosaic of fine cell means is a function of the
//! glyph shapes, and an attacker renders candidate strings and scores them
//! (Depix, or a likelihood over all cells). So the stroke is FIRST reduced to
//! coarse information only: blocks one brush tall and 2.5 brushes wide (two or
//! three glyphs), each its mean tone plus a secret offset, snapped to `LEVELS`
//! tones with a secret dither; and ONE dark / light colour pair for the whole
//! stroke (`colours`), never per block (a block's own colours would show a
//! coloured word in it). No fine cell ever reads the source pixels. THEN the
//! fine mosaic is drawn from those blocks: each cell's tone is interpolated
//! between the neighbouring blocks, plus a secret jitter per cell and one per
//! stroke from an OS-random seed (never from the content), folded back at
//! black and white; its colour is that tone on the stroke's dark-light line
//! (text, background and every
//! mix lie on it): no chroma of its own, no tint (grey stays grey, a blue link
//! stays blue). The cell grid is softened a little (two box passes of a
//! quarter cell). The footprint is a flat band `size` tall along the stroke
//! (square ends, no round blob), sharp at its edges. A repaint of any region
//! gives the same pixels (blocks, cells and jitter belong to the image's
//! coordinates and the stroke's seed).
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

/// Tones a block's mean is snapped to.
pub const LEVELS: f32 = 8.0;
/// Secret tone offset per block, ± levels (before the snap).
pub const BLOCK_NOISE: f32 = 54.0;
/// Secret tone jitter, ± levels: per cell, and once per stroke.
pub const JITTER: f32 = 18.0;
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

/// One coarse block: its snapped tone (its colour is the stroke's pair).
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

    // The footprint: the stroke `size` wide with square ends (anti-aliased).
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
    mask.fill_path(&outline, FillRule::Winding, true, to_mask);

    // 1. The coarse blocks, one beyond the cells each way: the ONLY reading of
    // the source. Columns on the image's grid; rows on the stroke's band (its
    // start's middle), so one row of blocks is the line itself.
    let (bw, bh) = block(size);
    let (dark, light) = {
        let data = layer.pm.data();
        colours(
            (ay0..ay1)
                .flat_map(|y| (ax0..ax1).map(move |x| (x, y)))
                .filter_map(|(x, y)| {
                    let m = mask.data()[(y - ay0) as usize * aw + (x - ax0) as usize];
                    let i = ((y * w + x) * 4) as usize;
                    (m > 0).then(|| [data[i], data[i + 1], data[i + 2]])
                }),
        )
    };
    let band = (points[0].y - size / 2.0).round() as i64;
    let oyb = oyi - band;
    let data = layer.pm.data_mut();
    let index = |x: i64, y: i64| ((y * w + x) * 4) as usize;
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
            let (x0, x1) = (
                (gx * bw - oxi).clamp(0, w),
                ((gx + 1) * bw - oxi).clamp(0, w),
            );
            let (y0, y1) = (
                (gy * bh - oyb).clamp(0, h),
                ((gy + 1) * bh - oyb).clamp(0, h),
            );
            let mut b = Block {
                tone: block_tone(data, index, (x0, x1), (y0, y1)),
            };
            // A secret offset per block, then snapped with a secret dither (an
            // error of up to half a step, independent of the text): what the
            // mean tone still tells apart is drowned.
            let key = mix(!seed ^ mix(gx as u64 ^ (gy as u64) << 32));
            let step = 255.0 / (LEVELS - 1.0);
            let d = step / 2.0 * unit(mix(key));
            b.tone = quantise(b.tone + BLOCK_NOISE * unit(key) + d) - d;
            blocks.push(b);
        }
    }
    let at = |gx: i64, gy: i64| {
        blocks[(gy.clamp(gy0, gy1) - gy0) as usize * gw + (gx.clamp(gx0, gx1) - gx0) as usize]
    };

    // 2. The fine cells, from the blocks only.
    let stroke_tone = JITTER * unit(mix(seed ^ 0x5bd1_e995));
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
            let value = on_line(dark, light, reflect(tone + jitter + stroke_tone));
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

/// A block's mean tone (`xs` x `ys`, clipped to the layer; off the layer: mid
/// grey, never painted).
fn block_tone(
    data: &[u8],
    index: impl Fn(i64, i64) -> usize,
    xs: (i64, i64),
    ys: (i64, i64),
) -> f32 {
    let n = (xs.1 - xs.0) * (ys.1 - ys.0);
    if n <= 0 {
        return 128.0;
    }
    let mut sum = 0.0f32;
    for y in ys.0..ys.1 {
        for x in xs.0..xs.1 {
            let i = index(x, y);
            sum += luma([0, 1, 2].map(|k| data[i + k] as f32));
        }
    }
    sum / n as f32
}

/// The stroke's ONE colour pair (owner's look, review: never per block, or a
/// coloured word would show in its block): per channel, the median of the
/// darkest 2 % of the pixels under the footprint (the ink's own colour: whole
/// stems, not anti-aliased or ClearType-fringed edges) and of the lightest half
/// (the background's), snapped to `COLOUR_STEP`. Medians of the ink and of the
/// background, so another text in the same colours gives the same pair, and
/// moving the text around under the stroke changes nothing. No contrast: both
/// are the footprint's median.
pub(crate) fn colours(pixels: impl Iterator<Item = [u8; 3]>) -> ([f32; 3], [f32; 3]) {
    let mut all: Vec<(f32, [u8; 3])> = pixels.map(|p| (luma(p.map(f32::from)), p)).collect();
    if all.is_empty() {
        return ([128.0; 3], [128.0; 3]);
    }
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let median = |part: &[(f32, [u8; 3])]| {
        [0, 1, 2].map(|k| {
            let mut v: Vec<u8> = part.iter().map(|(_, p)| p[k]).collect();
            v.sort_unstable();
            let m = v[v.len() / 2] as f32;
            ((m / COLOUR_STEP).round() * COLOUR_STEP).clamp(0.0, 255.0)
        })
    };
    let ink = (all.len() / 50).max(1);
    let (dark, light) = (median(&all[..ink]), median(&all[all.len() / 2..]));
    match luma(light) - luma(dark) > 8.0 {
        true => (dark, light),
        false => {
            let m = median(&all);
            (m, m)
        }
    }
}

/// The stroke's colour pair is snapped to steps of this many levels.
pub const COLOUR_STEP: f32 = 16.0;

fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// The colour of tone `y` on the line from `dark` to `light` (text and its
/// background, with every anti-aliased mix between them, lie on it), so the
/// colour says no more than the tone (and the tone is under the secret
/// jitter): no chroma of its own to leak the text, no tint. Beyond the ends,
/// or with no contrast (a plain area: both are its colour), it moves in grey.
fn on_line(dark: [f32; 3], light: [f32; 3], y: f32) -> [f32; 3] {
    let (yd, yl) = (luma(dark), luma(light));
    let (base, at) = match yl - yd > 8.0 {
        true => {
            let t = (y.clamp(yd, yl) - yd) / (yl - yd);
            (
                [0, 1, 2].map(|k| dark[k] + t * (light[k] - dark[k])),
                y.clamp(yd, yl),
            )
        }
        false => (dark, luma(dark)),
    };
    base.map(|v| (v + y - at).clamp(0.0, 255.0))
}

/// A level folded back into 0..=255 at the ends (clipping would cut the
/// jitter near black and white).
fn reflect(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else if v > 255.0 {
        510.0 - v
    } else {
        v
    }
}

/// A level snapped to `LEVELS` steps (0 and 255 stay exact).
fn quantise(v: f32) -> f32 {
    let step = 255.0 / (LEVELS - 1.0);
    (v / step).round() * step
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
