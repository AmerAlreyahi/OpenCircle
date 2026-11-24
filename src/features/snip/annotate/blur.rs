//! Hide (the blur brush): a content-free fill over what the user wants gone.
//!
//! It must not be reversible: anything that reads the covered pixels (a
//! Gaussian of any strength, a pixelate up to 16 px cells) was recovered 100 %
//! by candidate matching in the W100 lab (Depix / Unredacter); only a fill
//! that never reads the content was at chance. So the source is read ONCE per
//! stroke for two colours only: its background (the most common colour under
//! the footprint, which says nothing about the text) and its ink (the
//! extreme colour away from it, whole on any stem however little text there
//! is). The fill is the background pulled a FIXED 12 % toward the ink
//! (`DARKEN`), plus a texture that belongs to the PICTURE: a smooth noise
//! (Frost, the default) or a fine mosaic of cells (Pixelate, today's look), from
//! one secret seed per document, anchored to image coordinates and a scale
//! in image px (the capture's DPI scale, never the zoom). So two strokes on
//! one background match within a level or two, a repaint of any crop gives
//! the same pixels, and strokes that overlap merge. The footprint is a flat
//! band `size` tall along the stroke (square ends).
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use iced::{Point, Rectangle};
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Path, PathBuilder, Stroke, Transform};

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

/// The fill is the background pulled this much toward the ink: fixed, so it
/// never tracks how much text there is.
pub const DARKEN: f32 = 0.12;
/// Frost: the texture's reach, ± levels.
pub const FROST: f32 = 10.0;
/// Pixelate: secret tone offset per block, ± levels.
pub const BLOCK_NOISE: f32 = 8.0;
/// Pixelate: secret tone jitter per cell, ± levels.
pub const JITTER: f32 = 6.0;

/// How a hidden area looks. Both never read the covered text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// A smooth, secure frost (the default).
    #[default]
    Frost,
    /// A fine pixel mosaic (the look of the first secure blur).
    Pixelate,
}

/// The shape of a Hide (W102 step 2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A free brush: the stroke `size` wide with soft round ends.
    #[default]
    Brush,
    /// The rectangle between the two points.
    Rect,
    /// The ellipse in the rectangle between the two points.
    Ellipse,
}

/// What a Hide covers: a brush stroke (`points`, `size` wide), or a rectangle
/// or ellipse between `points`' first and last, filled or, when `outline` is
/// over 0, as a ring that wide along its edge.
#[derive(Clone, Copy)]
pub struct Footprint<'a> {
    pub points: &'a [Point],
    pub size: f32,
    pub kind: Kind,
    pub outline: f32,
}

impl<'a> Footprint<'a> {
    /// A free brush stroke (the tests').
    #[cfg(test)]
    pub fn brush(points: &'a [Point], size: f32) -> Self {
        Footprint {
            points,
            size,
            kind: Kind::Brush,
            outline: 0.0,
        }
    }

    /// The outline of the covered area.
    fn path(&self) -> Option<Path> {
        let round = |width: f32| Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        if self.kind == Kind::Brush {
            return pen::smooth(self.points)?.stroke(&round(self.size), 1.0);
        }
        let b = span(&[*self.points.first()?, *self.points.last()?]);
        let rect = tiny_skia::Rect::from_xywh(b.x, b.y, b.width.max(0.01), b.height.max(0.01))?;
        let path = match self.kind {
            Kind::Ellipse => PathBuilder::from_oval(rect)?,
            _ => PathBuilder::from_rect(rect),
        };
        match self.outline > 0.0 {
            true => path.stroke(&round(self.outline), 1.0),
            false => Some(path),
        }
    }
}

/// Side of a mosaic cell, image px (at least 2).
pub fn cell(cell: f32) -> i64 {
    cell.round().max(2.0) as i64
}

/// A mosaic block's (width, height), image px: from the cell, so the pattern
/// is the same on every line.
pub fn block(cell_px: f32) -> (i64, i64) {
    let c = cell(cell_px);
    (8 * c, 3 * c)
}

/// Box radius of each of the two softening passes: a quarter cell.
pub fn soften(cell_px: i64) -> usize {
    (cell_px as usize / 4).max(1)
}

/// What a Hide covers fully (a brush's round ends reach half its size).
pub fn frame(fp: &Footprint) -> Rectangle {
    let pad = match fp.kind {
        Kind::Brush => fp.size / 2.0,
        _ => fp.outline / 2.0,
    };
    span(fp.points).expand(pad + 1.0)
}

/// The edge fades OUTWARD this far (image px): the inside is always whole.
pub fn feather(cell_px: f32) -> f32 {
    (cell_px * 0.75).round().max(2.0)
}

/// Beyond `frame`: what a repaint must hold (the feather, the softening and
/// whole cells).
pub fn reach(cell_px: f32) -> f32 {
    let c = cell(cell_px);
    feather(cell_px) + (2 * soften(c)) as f32 + 2.0 * c as f32 + 1.0
}

/// How many pixels of a footprint the two colours are read from, at most.
const SAMPLES: usize = 16_384;

/// Hides `fp` in `layer` (texture scale `cell_px`, image px; `seed`: the
/// document's secret).
pub fn paint(layer: &mut Layer, fp: &Footprint, cell_px: f32, style: Style, seed: u64) {
    let (w, h) = (layer.pm.width() as i64, layer.pm.height() as i64);
    let (ox, oy) = (layer.origin.x, layer.origin.y);
    let clip = |r: Rectangle| {
        let x0 = ((r.x - ox).floor() as i64).clamp(0, w);
        let y0 = ((r.y - oy).floor() as i64).clamp(0, h);
        let x1 = ((r.x + r.width - ox).ceil() as i64).clamp(0, w);
        let y1 = ((r.y + r.height - oy).ceil() as i64).clamp(0, h);
        (x0, y0, x1, y1)
    };
    let feather_px = feather(cell_px);
    let area = frame(fp).expand(feather_px);
    let (ax0, ay0, ax1, ay1) = clip(area);
    if ax0 >= ax1 || ay0 >= ay1 {
        return;
    }
    let (aw, ah) = ((ax1 - ax0) as usize, (ay1 - ay0) as usize);

    // The footprint, hard-edged: not anti-aliased, a half-covered rim pixel
    // would show half of its source (a glyph's edge) through the fill.
    let Some(mut hard) = Mask::new(aw as u32, ah as u32) else {
        return;
    };
    let Some(outline) = fp.path() else {
        return;
    };
    let to_mask = Transform::from_translate(-(ox + ax0 as f32), -(oy + ay0 as f32));
    hard.fill_path(&outline, FillRule::Winding, false, to_mask);
    // The coverage: whole inside, fading OUTWARD over `feather` (what the
    // shape covers is never softer than whole, so nothing of it shows).
    let mut cover: Vec<[f32; 1]> = hard
        .data()
        .iter()
        .map(|&m| [f32::from(m) / 255.0])
        .collect();
    blur2d(&mut cover, aw, ah, (feather_px as usize / 2).max(1), 2);
    let cover: Vec<f32> = cover
        .iter()
        .zip(hard.data())
        .map(|(&[soft], &m)| (soft * 2.0).min(1.0).max(f32::from(m) / 255.0))
        .collect();

    // 1. The ONLY reading of the source: the background and the ink colour.
    let fill = {
        let data = layer.pm.data();
        // (A big footprint is sampled: a drag repaints it every frame, and two
        // colours need no more than a few thousand pixels.)
        let step = (aw * ah / SAMPLES).max(1);
        let under = (ay0..ay1)
            .flat_map(|y| (ax0..ax1).map(move |x| (x, y)))
            .enumerate()
            .filter(|(n, _)| n % step == 0)
            .filter_map(|(_, (x, y))| {
                let m = hard.data()[(y - ay0) as usize * aw + (x - ax0) as usize];
                let i = ((y * w + x) * 4) as usize;
                (m > 0).then(|| [data[i], data[i + 1], data[i + 2]])
            });
        let (ink, bg) = colours(under);
        [0, 1, 2].map(|k| bg[k] * (1.0 - DARKEN) + ink[k] * DARKEN)
    };
    let (oxi, oyi) = (ox.round() as i64, oy.round() as i64);

    // 2. The texture, from the seed and image coordinates only.
    let tone: Box<dyn Fn(i64, i64) -> f32> = match style {
        Style::Frost => {
            let lattice = Lattice::new(
                seed,
                cell(cell_px) * 2,
                (ax0 + oxi, ay0 + oyi, ax1 + oxi, ay1 + oyi),
            );
            Box::new(move |x, y| FROST * lattice.at(x + oxi, y + oyi))
        }
        Style::Pixelate => {
            let buf = mosaic(
                cell_px,
                seed,
                clip(area.expand(2.0 * soften(cell(cell_px)) as f32)),
                (oxi, oyi),
                (w, h),
            );
            let (sx0, sy0, sw) = (buf.0, buf.1, buf.2);
            let data = buf.3;
            Box::new(move |x, y| data[(y - sy0) as usize * sw + (x - sx0) as usize])
        }
    };

    let data = layer.pm.data_mut();
    for y in ay0..ay1 {
        for x in ax0..ax1 {
            let m = cover[(y - ay0) as usize * aw + (x - ax0) as usize];
            if m <= 0.001 {
                continue;
            }
            let t = tone(x, y);
            let i = ((y * w + x) * 4) as usize;
            for k in 0..3 {
                let v = (fill[k] + t).clamp(0.0, 255.0);
                data[i + k] = (v * m + data[i + k] as f32 * (1.0 - m)).round() as u8;
            }
            data[i + 3] = 255;
        }
    }
}

/// Smooth value noise in -1..=1 on a lattice of `l` px anchored to image
/// coordinates (smoothstep between hashed corners): the Frost texture. The
/// corners over a region are hashed once.
struct Lattice {
    l: i64,
    /// The first corner column and row, and the columns per row.
    gx0: i64,
    gy0: i64,
    gw: usize,
    corners: Vec<f32>,
}

impl Lattice {
    /// Covers image px `(x0, y0)..(x1, y1)`.
    fn new(seed: u64, l: i64, (x0, y0, x1, y1): (i64, i64, i64, i64)) -> Self {
        let (gx0, gy0) = (x0.div_euclid(l), y0.div_euclid(l));
        let (gx1, gy1) = (x1.div_euclid(l) + 1, y1.div_euclid(l) + 1);
        let corners = (gy0..=gy1)
            .flat_map(|gy| (gx0..=gx1).map(move |gx| (gx, gy)))
            .map(|(gx, gy)| unit(mix(seed ^ mix(gx as u64 ^ (gy as u64) << 32))))
            .collect();
        Lattice {
            l,
            gx0,
            gy0,
            gw: (gx1 - gx0 + 1) as usize,
            corners,
        }
    }

    fn at(&self, x: i64, y: i64) -> f32 {
        let l = self.l;
        let smooth = |v: i64| {
            let t = v.rem_euclid(l) as f32 / l as f32;
            t * t * (3.0 - 2.0 * t)
        };
        let (tx, ty) = (smooth(x), smooth(y));
        let at = ((y.div_euclid(l) - self.gy0) as usize * self.gw)
            + (x.div_euclid(l) - self.gx0) as usize;
        let c = &self.corners;
        let top = c[at] * (1.0 - tx) + c[at + 1] * tx;
        let bottom = c[at + self.gw] * (1.0 - tx) + c[at + self.gw + 1] * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// The Pixelate texture over the layer region `(x0, y0, x1, y1)`: tone offsets
/// per fine cell (a secret jitter) on coarse blocks (a secret offset each,
/// blended across), softened a little. Returns (x0, y0, width, offsets).
fn mosaic(
    cell_px: f32,
    seed: u64,
    (x0, y0, x1, y1): (i64, i64, i64, i64),
    (oxi, oyi): (i64, i64),
    (w, h): (i64, i64),
) -> (i64, i64, usize, Vec<f32>) {
    let c = cell(cell_px);
    let r = soften(c);
    // Grid snapping (image-anchored): `n` px cells, layer px `v`.
    let down = |v: i64, o: i64, n: i64| (v + o).div_euclid(n) * n - o;
    let up = |v: i64, o: i64, n: i64| -(-(v + o)).div_euclid(n) * n - o;
    let (sx0, sx1) = (down(x0, oxi, c).clamp(0, w), up(x1, oxi, c).clamp(0, w));
    let (sy0, sy1) = (down(y0, oyi, c).clamp(0, h), up(y1, oyi, c).clamp(0, h));
    let (sw, sh) = ((sx1 - sx0) as usize, (sy1 - sy0) as usize);
    let (bw, bh) = block(cell_px);
    let block_tone =
        |gx: i64, gy: i64| BLOCK_NOISE * unit(mix(!seed ^ mix(gx as u64 ^ (gy as u64) << 32)));
    let mut buf = vec![[0.0f32; 1]; sw * sh];
    let mut cy = sy0;
    while cy < sy1 {
        let y1 = up(cy + 1, oyi, c).min(sy1);
        let mut cx = sx0;
        while cx < sx1 {
            let x1 = up(cx + 1, oxi, c).min(sx1);
            // The cell's middle in block units: across, between the blocks'
            // middles; down, its own block row.
            let fx = ((cx + x1) as f32 / 2.0 + oxi as f32) / bw as f32 - 0.5;
            let iy = ((cy + y1) / 2 + oyi).div_euclid(bh);
            let ix = fx.floor() as i64;
            let tx = fx - ix as f32;
            let blocks = (1.0 - tx) * block_tone(ix, iy) + tx * block_tone(ix + 1, iy);
            let (gx, gy) = ((cx + oxi).div_euclid(c), (cy + oyi).div_euclid(c));
            let jitter = JITTER * unit(mix(seed ^ mix(gx as u64 ^ (gy as u64) << 32)));
            for y in cy..y1 {
                buf[(y - sy0) as usize * sw + (cx - sx0) as usize..][..(x1 - cx) as usize]
                    .fill([blocks + jitter]);
            }
            cx = x1;
        }
        cy = y1;
    }
    blur2d(&mut buf, sw, sh, r, 2);
    (sx0, sy0, sw, buf.into_iter().map(|[v]| v).collect())
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
