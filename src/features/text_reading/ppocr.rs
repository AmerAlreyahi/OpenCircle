//! PP-OCRv5 on ONNX Runtime, directly (#61): the detector (DB: a text
//! probability map -> line boxes) and a recognizer (CRNN + CTC: a line
//! crop -> text). The same maths as PaddleOCR's own pre- and
//! post-processing, step for step as oar-ocr 0.10 did it (its output is
//! the reference: the same boxes and texts), with none of its layers.
//! Helper process only. Line boxes are axis-aligned: `Engine` crops them
//! straight, so no rotation is needed.

use std::path::Path;

use clipper2_rust::clipper::inflate_paths_d;
use clipper2_rust::core::{PathD, PointD, area};
use clipper2_rust::offset::{EndType, JoinType};
use image::RgbImage;
use image::imageops::FilterType::Triangle;
use ort::session::Session;
use ort::session::builder::SessionBuilder;
use ort::value::Tensor;

use super::OcrError;

/// Pixels whose text probability is above this are text (PaddleOCR's
/// `thresh`).
const THRESH: f32 = 0.3;
/// A box is text when its mean probability reaches this (`box_thresh`).
const BOX_THRESH: f32 = 0.6;
/// How far a box grows past its text core (`unclip_ratio`).
const UNCLIP: f32 = 1.5;
const MAX_CANDIDATES: usize = 1000;
/// Boxes with a shorter side under this are dropped (before unclip; under
/// this + 2 after).
const MIN_SIZE: f32 = 3.0;
/// The recognizer's input height and its narrowest width.
const REC_H: usize = 48;
const REC_W: usize = 320;
/// The widest recognizer input (a longer line is squeezed into it).
const REC_MAX_W: usize = 3200;

fn engine_err(e: impl std::fmt::Display) -> OcrError {
    OcrError::Engine(e.to_string())
}

/// One model's session, as OCR runs them: no memory patterns (every
/// capture has a new shape: they only add RAM, ~100 MB measured), no
/// spin-waiting threads after a run (in a warm helper they kept the CPU
/// busy while the app rendered: a live step 0.6 s -> 0.2 s), the CPU EP
/// registered as ours = no ORT memory arena (a warm helper kept every
/// capture's peak, ~210 MB more after one 1080p read, for the same speed).
fn session(path: &Path) -> Result<Session, OcrError> {
    let build = |b: SessionBuilder| -> ort::Result<Session> {
        b.with_memory_pattern(false)?
            .with_config_entry("session.intra_op.allow_spinning", "0")?
            .with_config_entry("session.inter_op.allow_spinning", "0")?
            .with_execution_providers([ort::ep::CPU::default().build()])?
            .commit_from_file(path)
    };
    Session::builder().and_then(build).map_err(engine_err)
}

/// Runs `session` on one NCHW f32 tensor; `read` gets the first output's
/// shape and data.
fn run<T>(
    session: &mut Session,
    shape: [usize; 4],
    data: Vec<f32>,
    read: impl FnOnce(&[i64], &[f32]) -> T,
) -> Result<T, OcrError> {
    let input = Tensor::from_array((shape, data)).map_err(engine_err)?;
    let out = session
        .run(ort::inputs!["x" => input])
        .map_err(engine_err)?;
    let (shape, data) = out[0].try_extract_tensor::<f32>().map_err(engine_err)?;
    Ok(read(shape, data))
}

/// The text detector.
pub(crate) struct Detector {
    session: Session,
}

impl Detector {
    pub(crate) fn load(path: &Path) -> Result<Detector, OcrError> {
        Ok(Detector {
            session: session(path)?,
        })
    }

    /// The text lines in `img` as (x0, y0, x1, y1) in its px, whole pixels.
    /// `img` is read at its own size (rounded to the model's multiple of
    /// 32: the caller sizes it).
    pub(crate) fn boxes(&mut self, img: &RgbImage) -> Result<Vec<[f32; 4]>, OcrError> {
        let (src_w, src_h) = img.dimensions();
        // tiny images are padded to 32 px (black), then sized to 32 px steps
        let padded;
        let img = if src_w + src_h < 64 {
            let mut p = RgbImage::new(src_w.max(32), src_h.max(32));
            image::imageops::overlay(&mut p, img, 0, 0);
            padded = p;
            &padded
        } else {
            img
        };
        let (w, h) = img.dimensions();
        let side = |v: u32| (v + 16) / 32 * 32;
        let (rw, rh) = (side(w).max(32), side(h).max(32));
        let resized;
        let img = if (rw, rh) == (w, h) {
            img
        } else {
            resized = image::imageops::resize(img, rw, rh, Triangle);
            &resized
        };
        let (pw, ph) = (rw as usize, rh as usize);
        let data = det_tensor(img);
        let found = run(&mut self.session, [1, 3, ph, pw], data, |shape, pred| {
            let ok = shape.len() == 4 && shape[2] as usize == ph && shape[3] as usize == pw;
            ok.then(|| boxes_from_map(&pred[..pw * ph], pw, ph, src_w, src_h))
        })?;
        found.ok_or_else(|| engine_err("the text detector gave an unexpected output"))
    }
}

/// `img` as the detector's input: BGR planes, ImageNet-normalised (the
/// mean / std pairs in PaddleOCR's order: B with 0.485 / 0.229, ...).
fn det_tensor(img: &RgbImage) -> Vec<f32> {
    const SCALE: f32 = 1.0 / 255.0;
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let px = img.as_raw();
    let plane = px.len() / 3;
    let mut out = vec![0.0f32; plane * 3];
    for (c, dst) in out.chunks_exact_mut(plane).enumerate() {
        // plain multiply-add (no FMA): the reference's exact values
        let (a, b, src) = (SCALE / STD[c], -MEAN[c] / STD[c], 2 - c);
        for (d, p) in dst.iter_mut().zip(px.as_chunks::<3>().0) {
            *d = p[src] as f32 * a + b;
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pt {
    x: f32,
    y: f32,
}

fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// DB post-processing: the probability map (`w` x `h`) -> line boxes in
/// the source's px (`src_w` x `src_h`), each the axis-aligned bounds of
/// its grown minimum-area rectangle.
fn boxes_from_map(pred: &[f32], w: usize, h: usize, src_w: u32, src_h: u32) -> Vec<[f32; 4]> {
    let mask: Vec<bool> = pred.iter().map(|&v| v > THRESH).collect();
    let (sx, sy) = (src_w as f32 / w as f32, src_h as f32 / h as f32);
    let (dw, dh) = (src_w as f32, src_h as f32);
    let mut boxes = Vec::new();
    for contour in contours(&mask, w, h).into_iter().take(MAX_CANDIDATES) {
        let simple = simplify(&contour);
        let points = if simple.len() >= 3 { &simple } else { &contour };
        let Some((mini, side)) = mini_box(points) else {
            continue;
        };
        if side < MIN_SIZE || score(pred, w, h, &mini) < BOX_THRESH {
            continue;
        }
        let grown = unclip(&mini, UNCLIP);
        if grown.is_empty() {
            continue;
        }
        let Some((corners, side)) = mini_box(&grown) else {
            continue;
        };
        if side < MIN_SIZE + 2.0 {
            continue;
        }
        let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
        let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in corners {
            let x = (p.x * sx).round().clamp(0.0, dw);
            let y = (p.y * sy).round().clamp(0.0, dh);
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        boxes.push([x0, y0, x1, y1]);
    }
    boxes
}

/// The 8 neighbours from west, clockwise on screen (y down).
const AROUND: [(i32, i32); 8] = [
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
];

/// Every border (outer and hole) of the `mask`'s 8-connected regions, in
/// scan order: Suzuki and Abe's border following, as imageproc 0.27's
/// `find_contours` (MIT) runs it, so the candidates and their order match.
/// (Like it, a region whose left edge is column 0 starts no outer border
/// there.)
fn contours(mask: &[bool], w: usize, h: usize) -> Vec<Vec<Pt>> {
    let mut v: Vec<i32> = mask.iter().map(|&m| i32::from(m)).collect();
    let at = |x: usize, y: usize| x + w * y;
    let set = |v: &[i32], x: i32, y: i32| {
        (x > -1 && x < w as i32 && y > -1 && y < h as i32 && v[x as usize + w * y as usize] != 0)
            .then_some((x as usize, y as usize))
    };
    // `AROUND` turned so that this step comes first
    let start = |d: (i32, i32)| {
        AROUND
            .iter()
            .position(|a| *a == d)
            .expect("a neighbour step")
    };
    let mut all = Vec::new();
    let mut num = 1;
    for y in 0..h {
        for x in 0..w {
            let here = v[at(x, y)];
            if here == 0 {
                continue;
            }
            let adj = if here == 1 && x > 0 && v[at(x - 1, y)] == 0 {
                (x as i32 - 1, y as i32)
            } else if here > 0 && x + 1 < w && v[at(x + 1, y)] == 0 {
                (x as i32 + 1, y as i32)
            } else {
                continue;
            };
            num += 1;
            let mut points = Vec::new();
            let (cx, cy) = (x as i32, y as i32);
            let s = start((adj.0 - cx, adj.1 - cy));
            let first = (0..8)
                .map(|k| AROUND[(s + k) % 8])
                .find_map(|d| set(&v, cx + d.0, cy + d.1));
            if let Some(p1) = first {
                let (mut p2, mut p3) = (p1, (x, y));
                loop {
                    points.push(pt(p3.0 as f32, p3.1 as f32));
                    let s = start((p2.0 as i32 - p3.0 as i32, p2.1 as i32 - p3.1 as i32));
                    let (x3, y3) = (p3.0 as i32, p3.1 as i32);
                    let p4 = (0..8)
                        .rev()
                        .map(|k| AROUND[(s + k) % 8])
                        .find_map(|d| set(&v, x3 + d.0, y3 + d.1))
                        .expect("the pixel before is set");
                    let step = (p4.0 as i32 - x3, p4.1 as i32 - y3);
                    let mut right_edge = false;
                    for k in (0..8).rev() {
                        let d = AROUND[(s + k) % 8];
                        if d == step {
                            break;
                        }
                        if d == (1, 0) {
                            right_edge = true;
                            break;
                        }
                    }
                    let i = at(p3.0, p3.1);
                    if p3.0 + 1 == w || right_edge {
                        v[i] = -num;
                    } else if v[i] == 1 {
                        v[i] = num;
                    }
                    if p4 == (x, y) && p3 == p1 {
                        break;
                    }
                    (p2, p3) = (p3, p4);
                }
            } else {
                points.push(pt(x as f32, y as f32));
                v[at(x, y)] = -num;
            }
            all.push(points);
        }
    }
    all
}

/// A pixel chain without the points in the middle of a straight run.
fn simplify(points: &[Pt]) -> Vec<Pt> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let sign = |v: f32| (v > 0.0) as i8 - (v < 0.0) as i8;
    let dir = |a: Pt, b: Pt| (sign(b.x - a.x), sign(b.y - a.y));
    let kept: Vec<Pt> = (0..n)
        .filter(|&i| {
            let (prev, cur, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            dir(prev, cur) != dir(cur, next)
        })
        .map(|i| points[i])
        .collect();
    if kept.len() < 3 {
        points.to_vec()
    } else {
        kept
    }
}

/// The minimum-area rectangle around `points`: its corners (PaddleOCR's
/// order: top-left, top-right, bottom-right, bottom-left) and its shorter
/// side.
fn mini_box(points: &[Pt]) -> Option<([Pt; 4], f32)> {
    if points.len() < 3 {
        return None;
    }
    let (center, w, h, angle) = min_area_rect(points);
    let side = w.min(h);
    if !side.is_finite() || side <= 0.0 {
        return None;
    }
    // (separate sin and cos, in degrees through f32 PI: the reference's values)
    let rad = angle * std::f32::consts::PI / 180.0;
    let (cos, sin) = (rad.cos(), rad.sin());
    let (w2, h2) = (w / 2.0, h / 2.0);
    let mut c = [(-w2, -h2), (w2, -h2), (w2, h2), (-w2, h2)]
        .map(|(x, y)| pt(x * cos - y * sin + center.x, x * sin + y * cos + center.y));
    c.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    let (i1, i4) = if c[1].y > c[0].y { (0, 1) } else { (1, 0) };
    let (i2, i3) = if c[3].y > c[2].y { (2, 3) } else { (3, 2) };
    Some(([c[i1], c[i2], c[i3], c[i4]], side))
}

fn cross(a: Pt, b: Pt, c: Pt) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Graham scan from the lowest (then leftmost) point.
fn hull(src: &[Pt]) -> Vec<Pt> {
    let mut p = src.to_vec();
    let mut first = 0;
    for i in 1..p.len() {
        if p[i].y < p[first].y || (p[i].y == p[first].y && p[i].x < p[first].x) {
            first = i;
        }
    }
    p.swap(0, first);
    let o = p[0];
    p[1..].sort_by(|a, b| {
        let (ta, tb) = ((a.y - o.y).atan2(a.x - o.x), (b.y - o.y).atan2(b.x - o.x));
        ta.total_cmp(&tb).then_with(|| {
            let da = (a.x - o.x).powi(2) + (a.y - o.y).powi(2);
            let db = (b.x - o.x).powi(2) + (b.y - o.y).powi(2);
            da.total_cmp(&db)
        })
    });
    let mut out: Vec<Pt> = Vec::with_capacity(p.len());
    for q in p {
        while out.len() > 1 && cross(out[out.len() - 2], out[out.len() - 1], q) <= 0.0 {
            out.pop();
        }
        out.push(q);
    }
    out
}

/// Rotating calipers over the hull: (center, width, height, angle in
/// degrees) of the smallest rectangle (the first of equal ones).
fn min_area_rect(src: &[Pt]) -> (Pt, f32, f32, f32) {
    let hull = hull(src);
    if hull.len() < 3 {
        let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
        let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in src {
            (x0, x1, y0, y1) = (x0.min(p.x), x1.max(p.x), y0.min(p.y), y1.max(p.y));
        }
        if !x0.is_finite() {
            return (pt(0.0, 0.0), 0.0, 0.0, 0.0);
        }
        return (pt((x0 + x1) * 0.5, (y0 + y1) * 0.5), x1 - x0, y1 - y0, 0.0);
    }
    let mut best = (f32::MAX, (pt(0.0, 0.0), 0.0, 0.0, 0.0));
    let n = hull.len();
    for i in 0..n {
        let (a, b) = (hull[i], hull[(i + 1) % n]);
        let (ex, ey) = (b.x - a.x, b.y - a.y);
        let len2 = ex * ex + ey * ey;
        if len2 < f32::EPSILON {
            continue;
        }
        let inv = 1.0 / len2.sqrt();
        let (nx, ny) = (ex * inv, ey * inv);
        let (px, py) = (-ny, nx);
        let (mut n0, mut n1, mut p0, mut p1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for q in &hull {
            let (dx, dy) = (q.x - a.x, q.y - a.y);
            let (pn, pp) = (nx * dx + ny * dy, px * dx + py * dy);
            (n0, n1, p0, p1) = (n0.min(pn), n1.max(pn), p0.min(pp), p1.max(pp));
        }
        let (w, h) = (n1 - n0, p1 - p0);
        if w * h < best.0 {
            let (cn, cp) = ((n0 + n1) * 0.5, (p0 + p1) * 0.5);
            let center = pt(a.x + cn * nx + cp * px, a.y + cn * ny + cp * py);
            let angle = f32::atan2(ny, nx) * 180.0 / std::f32::consts::PI;
            best = (w * h, (center, w, h, angle));
        }
    }
    best.1
}

/// The mean probability inside the polygon `poly` (pixel centres on each
/// row, left to right).
fn score(pred: &[f32], w: usize, h: usize, poly: &[Pt; 4]) -> f32 {
    let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
    let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in poly {
        (x0, x1, y0, y1) = (x0.min(p.x), x1.max(p.x), y0.min(p.y), y1.max(p.y));
    }
    let clamp = |v: f32, most: usize| v.max(0.0).min(most as f32 - 1.0) as usize;
    let (sx, ex) = (clamp(x0.floor(), w), clamp(x1.ceil(), w) + 1);
    let (sy, ey) = (clamp(y0.floor(), h), clamp(y1.ceil(), h) + 1);
    let (mut total, mut pixels) = (0.0f32, 0usize);
    let mut cuts: Vec<f32> = Vec::with_capacity(4);
    for row in sy..ey {
        let y = row as f32 + 0.5;
        cuts.clear();
        for i in 0..4 {
            let (a, b) = (poly[i], poly[(i + 1) % 4]);
            if ((a.y <= y && y < b.y) || (b.y <= y && y < a.y)) && (b.y - a.y).abs() > f32::EPSILON
            {
                cuts.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
            }
        }
        cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let yi = y as usize;
        if yi >= h {
            continue;
        }
        let line = &pred[yi * w..yi * w + w];
        let mut sum = 0.0f32;
        for pair in cuts.as_chunks::<2>().0 {
            let a = pair[0].max(sx as f32) as usize;
            let b = pair[1].min(ex as f32) as usize;
            if a < b && a >= sx && b <= ex {
                let end = b.min(w);
                if a < end {
                    for v in &line[a..end] {
                        sum += v;
                    }
                    pixels += end - a;
                }
            }
        }
        total += sum;
    }
    if pixels > 0 {
        total / pixels as f32
    } else {
        0.0
    }
}

/// `poly` grown by area x `ratio` / perimeter, round corners (Clipper2,
/// as PaddleOCR's pyclipper). Empty when it can't grow.
fn unclip(poly: &[Pt; 4], ratio: f32) -> Vec<Pt> {
    let path: PathD = poly
        .iter()
        .map(|p| PointD {
            x: p.x as f64,
            y: p.y as f64,
        })
        .collect();
    let a = area(&path).abs();
    if a <= f64::EPSILON {
        return Vec::new();
    }
    let mut perimeter = 0.0f64;
    for i in 0..path.len() {
        let (p, q) = (&path[i], &path[(i + 1) % path.len()]);
        perimeter += (q.x - p.x).hypot(q.y - p.y);
    }
    if perimeter <= f64::EPSILON {
        return Vec::new();
    }
    let delta = a * ratio as f64 / perimeter;
    if delta.abs() <= f64::EPSILON {
        return Vec::new();
    }
    let mut out = inflate_paths_d(
        &vec![path],
        delta,
        JoinType::Round,
        EndType::Polygon,
        2.0,
        2,
        0.0,
    );
    if out.len() != 1 {
        return Vec::new();
    }
    let mut points: Vec<Pt> = (out.remove(0).iter())
        .map(|q| pt(q.x as f32, q.y as f32))
        .collect();
    if let [first, .., last] = points[..]
        && (first.x - last.x).abs() < f32::EPSILON
        && (first.y - last.y).abs() < f32::EPSILON
    {
        points.pop();
    }
    if points.len() < 3 { Vec::new() } else { points }
}

/// A text recognizer and its alphabet.
pub(crate) struct Recognizer {
    session: Session,
    /// Index 0 is CTC's blank.
    chars: Vec<char>,
}

impl Recognizer {
    pub(crate) fn load(model: &Path, dict: &Path) -> Result<Recognizer, OcrError> {
        let dict = std::fs::read_to_string(dict)?;
        // each line's first character (an empty line is skipped), then a space
        let chars = std::iter::once('\0')
            .chain(dict.lines().filter_map(|l| l.chars().next()))
            .chain(std::iter::once(' '))
            .collect();
        Ok(Recognizer {
            session: session(model)?,
            chars,
        })
    }

    /// Reads each crop (one line of text each): its text and its mean
    /// character confidence (0 when nothing is read). One batch: the crops
    /// are padded to the widest one's width (at least 320 / 48).
    pub(crate) fn read(&mut self, crops: &[RgbImage]) -> Result<Vec<(String, f32)>, OcrError> {
        if crops.is_empty() {
            return Ok(Vec::new());
        }
        let ratio = (crops.iter())
            .map(|c| c.width() as f32 / c.height().max(1) as f32)
            .fold(REC_W as f32 / REC_H as f32, f32::max);
        let width = ((REC_H as f32 * ratio) as usize).min(REC_MAX_W);
        let plane = REC_H * width;
        let mut data = vec![0.0f32; crops.len() * 3 * plane];
        for (crop, dst) in crops.iter().zip(data.chunks_exact_mut(3 * plane)) {
            let r = crop.width() as f32 / crop.height() as f32;
            let w = ((REC_H as f32 * r).ceil() as usize).min(width);
            let line = image::imageops::resize(crop, w as u32, REC_H as u32, Triangle);
            let px = line.as_raw();
            // BGR planes, each value to -1..1
            for (c, dst) in dst.chunks_exact_mut(plane).enumerate() {
                for y in 0..REC_H {
                    let src = &px[y * w * 3..(y + 1) * w * 3];
                    for (x, d) in dst[y * width..y * width + w].iter_mut().enumerate() {
                        *d = (src[x * 3 + 2 - c] as f32 / 255.0 - 0.5) / 0.5;
                    }
                }
            }
        }
        let n = crops.len();
        let chars = &self.chars;
        let read = run(
            &mut self.session,
            [n, 3, REC_H, width],
            data,
            |shape, out| {
                let [b, t, k] = shape[..] else {
                    return None;
                };
                let (t, k) = (t as usize, k as usize);
                (b as usize == n).then(|| {
                    out.chunks_exact(t * k)
                        .map(|seq| ctc(seq, k, chars))
                        .collect()
                })
            },
        )?;
        read.ok_or_else(|| engine_err("the text recognizer gave an unexpected output"))
    }
}

/// Greedy CTC: each step's best class (the last of equal ones), repeats
/// and blanks dropped; the confidence is the kept steps' mean.
fn ctc(seq: &[f32], classes: usize, chars: &[char]) -> (String, f32) {
    let (mut text, mut probs) = (String::new(), Vec::new());
    let mut prev = 0;
    for step in seq.chunks_exact(classes) {
        let (i, p) = (step.iter().copied().enumerate())
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, 0.0));
        if i != 0
            && i != prev
            && let Some(&c) = chars.get(i)
        {
            text.push(c);
            probs.push(p);
        }
        prev = i;
    }
    let conf = if probs.is_empty() {
        0.0
    } else {
        probs.iter().sum::<f32>() / probs.len() as f32
    };
    (text, conf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctc_drops_blanks_and_repeats() {
        let chars = ['\0', 'a', 'b', ' '];
        // steps: a a blank a b b space (one-hot; 0.9 on even steps, 0.5 on odd)
        let steps = [1, 1, 0, 1, 2, 2, 3];
        let seq: Vec<f32> = (steps.iter().enumerate())
            .flat_map(|(t, &k)| {
                let mut row = [0.0; 4];
                row[k] = if t % 2 == 0 { 0.9 } else { 0.5 };
                row
            })
            .collect();
        let (text, conf) = ctc(&seq, 4, &chars);
        assert_eq!(text, "aab ");
        assert!((conf - (0.9 + 0.5 + 0.9 + 0.9) / 4.0).abs() < 1e-6);
        assert_eq!(ctc(&[0.9, 0.1, 0.0, 0.0], 4, &chars), (String::new(), 0.0));
    }

    /// A solid text core gives one box: the core grown by area x 1.5 /
    /// perimeter on every side (PaddleOCR's unclip), in whole pixels.
    #[test]
    fn a_text_core_becomes_one_grown_box() {
        let (w, h) = (64, 32);
        let mut pred = vec![0.0f32; w * h];
        for y in 10..20 {
            for x in 8..48 {
                pred[y * w + x] = 0.9;
            }
        }
        let boxes = boxes_from_map(&pred, w, h, w as u32, h as u32);
        // the border pixels' centres span 8..47 x 10..19 (39 x 9), grown by
        // 39*9*1.5/96 = 5.48 on each side
        assert_eq!(boxes, [[3.0, 5.0, 52.0, 24.0]]);
        // a core under 3 px tall is no text; a faint one neither
        for v in pred.iter_mut().skip(12 * w).take(8 * w) {
            *v = 0.0;
        }
        assert!(boxes_from_map(&pred, w, h, w as u32, h as u32).is_empty());
        let faint = vec![0.5f32; w * h];
        assert!(boxes_from_map(&faint, w, h, w as u32, h as u32).is_empty());
    }

    #[test]
    fn contours_follow_outer_borders_and_holes() {
        // a 5x5 square with a 1-px hole: its outer border, then the hole's
        let (w, h) = (8, 8);
        let mask: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                (1..6).contains(&x) && (1..6).contains(&y) && (x, y) != (3, 3)
            })
            .collect();
        let found = contours(&mask, w, h);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].len(), 16);
        assert_eq!(found[0][0], pt(1.0, 1.0));
        assert_eq!(found[1].len(), 4); // its 4 neighbours (8-connected walk)
        let (_, rw, rh, _) = min_area_rect(&found[0]);
        assert_eq!((rw, rh), (4.0, 4.0));
    }

    /// Where the detector's time goes on one screen (`OC_IMG`, a PNG, read at
    /// its own size rounded to 32 px): input tensor, the model, post-processing.
    /// `OC_IMG=<png> cargo test --release ppocr::tests::detector_stages -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn detector_stages() {
        use std::time::Instant;
        let dir = super::super::ensure_models(|_, _| {}).unwrap();
        super::super::init_ort(&dir).unwrap();
        let mut det = Detector::load(&dir.join(super::super::models::DET.name)).unwrap();
        let file = std::fs::File::open(std::env::var("OC_IMG").expect("OC_IMG")).unwrap();
        let png = crate::shared::platform::capture::decode_png(std::io::BufReader::new(file));
        let png = png.unwrap();
        let img = super::super::to_rgb(&png.rgba, png.width, png.height).unwrap();
        let (w, h) = img.dimensions();
        let side = |v: u32| (v + 16) / 32 * 32;
        let img = image::imageops::resize(&img, side(w), side(h), Triangle);
        let (pw, ph) = (img.width() as usize, img.height() as usize);
        for _ in 0..3 {
            let t = Instant::now();
            let data = det_tensor(&img);
            let t1 = t.elapsed();
            let map = run(&mut det.session, [1, 3, ph, pw], data, |_, p| p.to_vec()).unwrap();
            let t2 = t.elapsed();
            let boxes = boxes_from_map(&map, pw, ph, pw as u32, ph as u32);
            let t3 = t.elapsed();
            println!(
                "{pw}x{ph}: tensor {:?}, model {:?}, post {:?} ({} boxes)",
                t1,
                t2 - t1,
                t3 - t2,
                boxes.len()
            );
        }
    }
}
