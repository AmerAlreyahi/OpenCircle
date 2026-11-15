//! Cheap pixel work: grayscale cells (a `scale` x `scale` pixel average), a region's text-area and ring
//! signatures, their differences, and the discover step's tile hashes and change clusters.

use std::hash::{DefaultHasher, Hasher};

use super::{Frame, Rect};

/// Discover tiles, in cells (8 cells = 32 px at scale 4: today's live tile).
pub const TILE: u32 = 8;

/// The gray of one cell (cx, cy): the mean of its pixels inside the frame; from scale 4 on, of every
/// second pixel in each direction (a quarter of the reads: a letter's strokes still cover several of
/// them; measured 50 -> 19 us per 600x60 line).
pub fn cell(f: &Frame, s: u32, cx: u32, cy: u32) -> u8 {
    let step = if s >= 4 { 2 } else { 1 };
    let (x0, y0) = (cx * s, cy * s);
    let (x1, y1) = ((x0 + s).min(f.width), (y0 + s).min(f.height));
    let (mut sum, mut n) = (0u32, 0u32);
    for y in (y0..y1).step_by(step) {
        let row = &f.rgba[((y * f.width + x0) * 4) as usize..((y * f.width + x1) * 4) as usize];
        for p in row.as_chunks::<4>().0.iter().step_by(step) {
            sum += 77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32;
            n += 1;
        }
    }
    ((sum / n.max(1)) >> 8) as u8
}

/// The cells of a region: its text area (row-major), then its ring (`ring` cells around it, row-major,
/// clamped to the frame). `f(cx, cy, in_ring)` is called once per cell, always in this order.
pub fn walk(rect: Rect, s: u32, ring: u32, (w, h): (u32, u32), mut f: impl FnMut(u32, u32, bool)) {
    let (cw, ch) = (w.div_ceil(s), h.div_ceil(s));
    let (x0, y0) = ((rect.x / s).min(cw), (rect.y / s).min(ch));
    let (x1, y1) = (
        rect.right().div_ceil(s).min(cw),
        rect.bottom().div_ceil(s).min(ch),
    );
    for cy in y0..y1 {
        for cx in x0..x1 {
            f(cx, cy, false);
        }
    }
    let (rx0, ry0) = (x0.saturating_sub(ring), y0.saturating_sub(ring));
    let (rx1, ry1) = ((x1 + ring).min(cw), (y1 + ring).min(ch));
    for cy in ry0..ry1 {
        for cx in rx0..rx1 {
            if !(x0..x1).contains(&cx) || !(y0..y1).contains(&cy) {
                f(cx, cy, true);
            }
        }
    }
}

/// A region's text-area and ring cells on `frame`.
pub fn sample(f: &Frame, rect: Rect, s: u32, ring: u32) -> (Vec<u8>, Vec<u8>) {
    let (mut text, mut around) = (Vec::new(), Vec::new());
    walk(rect, s, ring, (f.width, f.height), |cx, cy, r| {
        if r { &mut around } else { &mut text }.push(cell(f, s, cx, cy));
    });
    (text, around)
}

/// `now` changed from `a` blended toward `b` by `t` (b = None: `a`): the mean absolute gray difference is
/// over `change`, or two cells or more moved over twice that (one new letter in a long line moves few cells
/// a lot: the mean alone would miss it).
pub fn changed(now: &[u8], a: &[u8], b: Option<(&[u8], f32)>, change: f32) -> bool {
    if now.is_empty() {
        return false;
    }
    let (mut sum, mut strong) = (0.0, 0);
    for i in 0..now.len() {
        let want = match b {
            None => a[i] as f32,
            Some((b, t)) => a[i] as f32 + (b[i] as f32 - a[i] as f32) * t,
        };
        let d = (now[i] as f32 - want).abs();
        sum += d;
        strong += (d > 2.0 * change) as u32;
    }
    sum / now.len() as f32 > change || strong >= 2
}

/// The share (0..1) of `now`'s cells that moved more than `change` from `a` blended toward `b` by `t`.
pub fn moved(now: &[u8], a: &[u8], b: Option<(&[u8], f32)>, change: f32) -> f32 {
    if now.is_empty() {
        return 0.0;
    }
    let n = (0..now.len())
        .filter(|&i| {
            let want = match b {
                None => a[i] as f32,
                Some((b, t)) => a[i] as f32 + (b[i] as f32 - a[i] as f32) * t,
            };
            (now[i] as f32 - want).abs() > change
        })
        .count();
    n as f32 / now.len() as f32
}

/// The whole frame as cells, `masked` rects (frame px) set to 0, and one hash per TILE x TILE cells.
pub fn tile_hashes(f: &Frame, s: u32, masked: &[Rect]) -> Vec<u64> {
    let (cw, ch) = (f.width.div_ceil(s), f.height.div_ceil(s));
    let mut gray: Vec<u8> = (0..ch)
        .flat_map(|cy| (0..cw).map(move |cx| (cx, cy)))
        .map(|(cx, cy)| cell(f, s, cx, cy))
        .collect();
    for m in masked {
        walk(*m, s, 0, (f.width, f.height), |cx, cy, _| {
            gray[(cy * cw + cx) as usize] = 0;
        });
    }
    let tw = cw.div_ceil(TILE) as usize;
    let mut hs = vec![DefaultHasher::new(); tw * ch.div_ceil(TILE) as usize];
    for (cy, row) in gray.chunks_exact(cw as usize).enumerate() {
        for (tx, part) in row.chunks(TILE as usize).enumerate() {
            hs[cy / TILE as usize * tw + tx].write(part);
        }
    }
    hs.into_iter().map(|h| h.finish()).collect()
}

/// Changed tiles -> 4-connected clusters -> their boxes padded by one tile (frame px, clamped), merged
/// where they overlap.
pub fn clusters(changed: &[bool], f: (u32, u32), s: u32) -> Vec<Rect> {
    let tw = f.0.div_ceil(s).div_ceil(TILE) as usize;
    let th = changed.len() / tw.max(1);
    let mut seen = vec![false; changed.len()];
    let mut out: Vec<Rect> = Vec::new();
    for start in 0..changed.len() {
        if !changed[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        let (mut stack, mut b) = (
            vec![start],
            [start % tw, start / tw, start % tw, start / tw],
        );
        while let Some(i) = stack.pop() {
            let (x, y) = (i % tw, i / tw);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            let near = [
                (x > 0).then(|| i - 1),
                (x + 1 < tw).then_some(i + 1),
                (y > 0).then(|| i - tw),
                (y + 1 < th).then_some(i + tw),
            ];
            for j in near.into_iter().flatten() {
                if changed[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        let px = (TILE * s) as usize;
        let (x0, y0) = (b[0].saturating_sub(1) * px, b[1].saturating_sub(1) * px);
        let (x1, y1) = (
            ((b[2] + 2) * px).min(f.0 as usize),
            ((b[3] + 2) * px).min(f.1 as usize),
        );
        let mut r = Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
        // merge every rect it touches (repeat: a merged rect can reach more)
        while let Some(i) = out.iter().position(|o| o.overlap(&r) > 0) {
            r = r.union(&out.swap_remove(i));
        }
        out.push(r);
    }
    out
}
