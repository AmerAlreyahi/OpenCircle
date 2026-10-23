//! Erase one text line in place, no box: find the glyph pixels, then fill
//! them back in from the surrounding background.
//!
//! Mask: fit a background plane (flat or gradient) on a ring around the line
//! box; pixels that differ from it by more than the ring's own noise are
//! candidates; candidates are grouped into colour clusters (fill, outline or
//! shadow, accent) and only pixels near a cluster colour are kept. Colours
//! that also sit on the box's top or bottom edge are art crossing the box
//! (trees behind a translucent panel) and are kept out of the mask, so the
//! holes stay as thin as the glyphs. Then dilate a little and add nearby
//! leftovers: weak anti-aliasing and a faint darker shadow halo.
//!
//! Fill: edge-aware directional interpolation. Each hole pixel looks both ways
//! along 8 directions for the nearest background pixels and blends the
//! straight-line interpolations, weighting directions whose two ends agree.
//! A background edge (a tree behind a translucent panel, a frame line) crosses
//! a thin glyph hole as a near-straight line, so the direction along the edge
//! wins and the edge is continued instead of smudged. Flat and gradient
//! panels are linear along every direction and come back exact.

/// What the redraw needs to know about the erased line.
#[derive(Clone, Copy, Debug)]
pub struct LineStyle {
    /// Main glyph fill colour.
    pub color: [u8; 3],
    /// A second, saturated glyph colour (e.g. a gold name), if any.
    pub accent: Option<[u8; 3]>,
    /// Dark outline/shadow colour; `true` = offset drop shadow, `false` = outline.
    pub shadow: Option<([u8; 3], bool)>,
    /// Ink extent x0, y0, x1, y1 (exclusive), fill/accent pixels only.
    pub ink: [i32; 4],
    /// Mean stroke width in px: a weight hint.
    pub stroke: f32,
}

const PAD: i32 = 3;
const RING: i32 = 2;
const HALO: i32 = 4;
/// Longest ray a hole pixel walks to find background.
const REACH: i32 = 40;

/// Erases the glyphs inside `b` (x, y, w, h) of an RGBA image and returns
/// their style. Alpha is left as is.
pub fn erase_line(rgba: &mut [u8], iw: usize, ih: usize, b: [f32; 4]) -> LineStyle {
    let x0 = (b[0] as i32 - PAD).clamp(0, iw as i32);
    let y0 = (b[1] as i32 - PAD).clamp(0, ih as i32);
    let x1 = ((b[0] + b[2]).ceil() as i32 + PAD).clamp(x0, iw as i32);
    let y1 = ((b[1] + b[3]).ceil() as i32 + PAD).clamp(y0, ih as i32);
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let at = |i: usize| ((i / w + y0 as usize) * iw + i % w + x0 as usize) * 4;
    let win: Vec<[u8; 3]> = (0..w * h)
        .map(|i| [rgba[at(i)], rgba[at(i) + 1], rgba[at(i) + 2]])
        .collect();
    let px = |i: usize| win[i];
    let on_ring = |i: usize| {
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        x < RING || y < RING || x >= w as i32 - RING || y >= h as i32 - RING
    };
    let ring: Vec<usize> = (0..w * h).filter(|&i| on_ring(i)).collect();
    if w <= 2 * RING as usize || h <= 2 * RING as usize || ring.len() < 8 {
        return LineStyle {
            color: [255; 3],
            accent: None,
            shadow: None,
            ink: [x0, y0, x1, y1],
            stroke: 0.0,
        };
    }

    // 1. background plane per channel, fitted on the ring
    let plane = fit_plane_robust(&ring, w, &px);
    let bg = |i: usize| {
        let (x, y) = ((i % w) as f32, (i / w) as f32);
        plane.map(|p| p[0] + p[1] * x + p[2] * y)
    };
    let dist: Vec<f32> = (0..w * h)
        .map(|i| {
            let (p, b) = (px(i), bg(i));
            (0..3)
                .map(|c| (p[c] as f32 - b[c]).abs())
                .fold(0.0, f32::max)
        })
        .collect();

    // 2. candidates: clearly off the background, relative to the ring's own noise
    let mut ring_d: Vec<f32> = ring.iter().map(|&i| dist[i]).collect();
    ring_d.sort_by(f32::total_cmp);
    let sigma = ring_d[ring_d.len() / 2] * 1.48;
    let t = (sigma * 3.0).max(28.0);
    let t_lo = (sigma * 2.0).max(8.0); // faint anti-aliasing next to glyphs
    let strong: Vec<bool> = (0..w * h).map(|i| dist[i] > t && !on_ring(i)).collect();
    // over busy art the ring noise (so `t`) is high and an outline or shadow
    // falls under it: next to strong glyph pixels the base threshold is enough
    let next_to = dilate(&strong, w, HALO as usize);
    let cand: Vec<bool> = (0..w * h)
        .map(|i| strong[i] || (next_to[i] && dist[i] > 28.0 && !on_ring(i)))
        .collect();

    // 3. colour clusters, keep only pixels near a cluster colour. Text stays
    // between the box's top and bottom edges (its ends, outline included, may
    // poke out sideways), so colours on the top and bottom ring rows are art
    // crossing the box (trees behind a translucent panel): not text, even
    // when they are near a text colour (dark trees next to a black outline).
    let bin =
        |c: [u8; 3]| (c[0] as usize >> 4) << 8 | (c[1] as usize >> 4) << 4 | c[2] as usize >> 4;
    let mut seen = vec![0u16; 4096];
    for i in (0..w * h).filter(|&i| i / w < RING as usize || i / w >= h - RING as usize) {
        seen[bin(px(i))] += 1;
    }
    // a bin is art if it or a neighbouring bin was seen twice (colours near a bin edge)
    let art = |c: [u8; 3]| {
        let c = c.map(|v| (v >> 4) as i32);
        (-1..=1).any(|dr| {
            (-1..=1).any(|dg| {
                (-1..=1).any(|db| {
                    let n = [c[0] + dr, c[1] + dg, c[2] + db];
                    n.iter().all(|v| (0..16).contains(v))
                        && seen[(n[0] << 8 | n[1] << 4 | n[2]) as usize] >= 2
                })
            })
        })
    };
    // First pass without art colours finds the fill. A shadow or outline in
    // an art colour (black over dark trees) hugs the fill: art-coloured pixels
    // are then much denser within 3 px of the fill than farther out, and the
    // near ones count as text. Otherwise every art colour is background.
    let near = |p: [u8; 3], c: [u8; 3]| (0..3).all(|k| (p[k] as i32 - c[k] as i32).abs() <= 64);
    let fill_c = clusters(&cand, w, &px, &art)
        .first()
        .map_or([255; 3], |c| c.color);
    let fill_px: Vec<bool> = (0..w * h).map(|i| cand[i] && near(px(i), fill_c)).collect();
    let by_fill = dilate(&fill_px, w, 3);
    let art_px = |i: usize| art(px(i)) && !(0..3).all(|k| px(i)[k].abs_diff(fill_c[k]) <= 24);
    let (mut n_near, mut a_near, mut n_far, mut a_far) = (0, 0, 0, 0);
    for i in (0..w * h).filter(|&i| !on_ring(i)) {
        let hit = (cand[i] && art_px(i)) as usize;
        if by_fill[i] {
            (n_near, a_near) = (n_near + hit, a_near + 1);
        } else {
            (n_far, a_far) = (n_far + hit, a_far + 1);
        }
    }
    let hugs = n_near * a_far.max(1) > 3 * n_far * a_near.max(1);
    let is_art = |i: usize| art_px(i) && !(hugs && by_fill[i]);
    let text_cand: Vec<bool> = (0..w * h).map(|i| cand[i] && !is_art(i)).collect();
    let cl = clusters(&text_cand, w, &px, &|_| false);
    let mut glyph = vec![false; w * h];
    let mut ink_only = vec![false; w * h];
    for i in (0..w * h).filter(|&i| text_cand[i]) {
        if let Some(c) = cl.iter().find(|c| near(px(i), c.color)) {
            glyph[i] = true;
            ink_only[i] = c.role != Role::Shadow;
        }
    }
    let bg_luma = luma(bg(w * h / 2 + w / 2).map(|v| v.clamp(0.0, 255.0) as u8));
    let (mut accent, mut shadow) = (None, None);
    for c in cl.iter().skip(1) {
        match c.role {
            Role::Accent => accent = accent.or(Some(c.color)),
            Role::Shadow if luma(c.color) < bg_luma - 15.0 => {
                shadow = shadow.or(Some((c.color, c.offset > 1.0)))
            }
            _ => {}
        }
    }

    // 4. dilate (anti-aliasing), plus a darker-than-background halo (soft shadows)
    let mut mask = glyph.clone();
    // Weak pixels only count when the text explains them: a dark halo only if
    // a shadow was found, faint pixels only if they lean toward a glyph colour.
    // Dark art next to the glyphs (a tree behind a translucent panel) stays,
    // so the hole stays thin and the fill can continue the art's edges.
    let dark = |i: usize| {
        shadow.is_some()
            && !is_art(i)
            && luma(px(i))
                < luma(bg(i).map(|v| v.clamp(0.0, 255.0) as u8)) - (2.0 * sigma).max(10.0)
    };
    let toward = |i: usize| {
        let (p, b) = (px(i), bg(i));
        let d = [0, 1, 2].map(|k| p[k] as f32 - b[k]);
        !is_art(i)
            && cl.iter().any(|c| {
                let g = [0, 1, 2].map(|k| c.color[k] as f32 - b[k]);
                let len = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt().max(1.0);
                (d[0] * g[0] + d[1] * g[1] + d[2] * g[2]) / len > t_lo
            })
    };
    // round-ish dilations: radius^2 <= 5 (anti-aliasing) and <= ~9 (faint
    // pixels) as square-then-plus steps; the dark halo may be square
    let d2 = plus(&dilate(&glyph, w, 1), w);
    let d3 = plus(&d2, w);
    let d4 = dilate(&glyph, w, HALO as usize);
    for (i, m) in mask.iter_mut().enumerate() {
        *m |=
            !on_ring(i) && (d2[i] || (d3[i] && dist[i] > t_lo && toward(i)) || (d4[i] && dark(i)));
    }

    // 5. fill the holes from the background around them
    // grain = median step between neighbouring pixels on the top ring row;
    // unlike `sigma` it stays small over sharp-edged art
    let mut steps: Vec<u8> = (1..w)
        .map(|x| {
            (0..3)
                .map(|k| win[x][k].abs_diff(win[x - 1][k]))
                .max()
                .unwrap()
        })
        .collect();
    steps.sort_unstable();
    let grain = steps
        .get(steps.len() / 2)
        .map_or(2.0, |&s| (s as f32).max(2.0));
    for (i, v) in fill(&win, &mask, w, grain, &bg) {
        rgba[at(i)..at(i) + 3].copy_from_slice(&v);
    }
    LineStyle {
        color: cl.first().map_or(fill_c, |c| c.color),
        accent,
        shadow,
        ink: extent(&ink_only, w, x0, y0),
        stroke: stroke_width(&ink_only, w),
    }
}

/// 8 directions, 22.5 degrees apart (a line through the pixel is two rays).
const DIRS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (0.924, 0.383),
    (0.707, 0.707),
    (0.383, 0.924),
    (0.0, 1.0),
    (-0.383, 0.924),
    (-0.707, 0.707),
    (-0.924, 0.383),
];

/// Edge-aware fill: for every hole pixel and direction, find the nearest
/// background pixel on both sides, interpolate linearly between them and
/// weight by how well the two ends agree (`noise` = the background's own
/// noise level) and how short the span is. Returns (index, colour) per hole
/// pixel; known pixels are only read, so the order does not matter.
/// The mask must be clear on the window border (the ring), so a ray always
/// meets background before it leaves the window.
/// ponytail: interpolation smooths fine grain inside the hole (texture is not
/// re-synthesised); MI-GAN on the OCR worker's ONNX Runtime is the upgrade.
fn fill(
    win: &[[u8; 3]],
    mask: &[bool],
    w: usize,
    noise: f32,
    fallback: &dyn Fn(usize) -> [f32; 3],
) -> Vec<(usize, [u8; 3])> {
    // index offset of step t along each ray (8 directions, both ways)
    let rays: Vec<[isize; REACH as usize + 2]> = DIRS
        .iter()
        .flat_map(|&(dx, dy)| [(dx, dy), (-dx, -dy)])
        .map(|(dx, dy)| {
            std::array::from_fn(|t| {
                (dy * t as f32).round() as isize * w as isize + (dx * t as f32).round() as isize
            })
        })
        .collect();
    // first background pixel along a ray: its distance and colour (mean of
    // the first two known pixels, so one noisy pixel does not set the edge)
    let probe = |i: usize, ray: &[isize; REACH as usize + 2]| -> Option<(f32, [f32; 3])> {
        let at = |t: usize| (i as isize + ray[t]) as usize;
        let t = (1..=REACH as usize).find(|&t| !mask[at(t)])?;
        let c = win[at(t)].map(f32::from);
        Some((
            t as f32,
            if mask[at(t + 1)] {
                c
            } else {
                [0, 1, 2].map(|k| (c[k] + win[at(t + 1)][k] as f32) / 2.0)
            },
        ))
    };
    let mut out = Vec::new();
    for i in (0..mask.len()).filter(|&i| mask[i]) {
        // (mismatch of the two ends, span, interpolated colour) per direction
        let mut cands = [(0f32, 0f32, [0f32; 3]); 8];
        let mut n = 0;
        for pair in rays.chunks(2) {
            let (Some((da, ca)), Some((db, cb))) = (probe(i, &pair[0]), probe(i, &pair[1])) else {
                continue;
            };
            let miss = (0..3).map(|k| (ca[k] - cb[k]).abs()).fold(0.0, f32::max);
            let v = [0, 1, 2].map(|k| (ca[k] * db + cb[k] * da) / (da + db));
            cands[n] = (miss, da + db, v);
            n += 1;
        }
        // only directions about as consistent as the best one: along an edge
        // that is one direction, on texture it is most of them
        let best = cands[..n].iter().map(|c| c.0).fold(f32::MAX, f32::min);
        let (mut acc, mut wsum) = ([0f32; 3], 0f32);
        for &(miss, span, v) in cands[..n].iter().filter(|c| c.0 <= best + 3.0 * noise) {
            let wt = 1.0 / ((miss + noise).powi(2) * span * span);
            for k in 0..3 {
                acc[k] += wt * v[k];
            }
            wsum += wt;
        }
        let v = if wsum > 0.0 {
            acc.map(|a| a / wsum)
        } else {
            fallback(i)
        };
        out.push((i, v.map(|c| c.round().clamp(0.0, 255.0) as u8)));
    }
    out
}

/// Dilation by one pixel up, down, left and right.
fn plus(m: &[bool], w: usize) -> Vec<bool> {
    (0..m.len())
        .map(|i| {
            m[i] || (i % w > 0 && m[i - 1])
                || (i % w + 1 < w && m[i + 1])
                || (i >= w && m[i - w])
                || (i + w < m.len() && m[i + w])
        })
        .collect()
}

/// Square dilation by `r` (separable: rows, then columns).
fn dilate(m: &[bool], w: usize, r: usize) -> Vec<bool> {
    let h = m.len() / w;
    let pass = |m: &[bool], len: usize, idx: &dyn Fn(usize, usize) -> usize, lines: usize| {
        let mut out = vec![false; m.len()];
        for l in 0..lines {
            // last set position seen, scanning forward then backward
            let mut last = None;
            for k in 0..len {
                if m[idx(l, k)] {
                    last = Some(k);
                }
                out[idx(l, k)] = last.is_some_and(|j| k - j <= r);
            }
            last = None;
            for k in (0..len).rev() {
                if m[idx(l, k)] {
                    last = Some(k);
                }
                out[idx(l, k)] |= last.is_some_and(|j| j - k <= r);
            }
        }
        out
    };
    let rows = pass(m, w, &|y, x| y * w + x, h);
    pass(&rows, h, &|x, y| y * w + x, w)
}

fn extent(m: &[bool], w: usize, x0: i32, y0: i32) -> [i32; 4] {
    let mut e = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for i in (0..m.len()).filter(|&i| m[i]) {
        let (x, y) = ((i % w) as i32 + x0, (i / w) as i32 + y0);
        e = [e[0].min(x), e[1].min(y), e[2].max(x + 1), e[3].max(y + 1)];
    }
    if e[0] == i32::MAX {
        [x0, y0, x0 + w as i32, y0 + (m.len() / w) as i32]
    } else {
        e
    }
}

/// Mean stroke width: a stroke of width s and length L has area L*s and
/// outline 2L, so s = 2 * area / outline (4-neighbour edge count).
fn stroke_width(m: &[bool], w: usize) -> f32 {
    let (mut area, mut edges) = (0, 0);
    for i in (0..m.len()).filter(|&i| m[i]) {
        area += 1;
        let (x, y) = (i % w, i / w);
        let nb = [
            x > 0 && m[i - 1],
            x + 1 < w && m[i + 1],
            y > 0 && m[i - w],
            i + w < m.len() && m[i + w],
        ];
        edges += nb.iter().filter(|&&on| !on).count();
    }
    2.0 * area as f32 / edges.max(1) as f32
}

/// Least-squares plane a + b*x + c*y per channel; one outlier-rejection pass
/// so a frame line or a stray glyph on the ring does not tilt it.
fn fit_plane_robust(pts: &[usize], w: usize, px: &dyn Fn(usize) -> [u8; 3]) -> [[f32; 3]; 3] {
    let plane = fit_plane(pts.iter().copied(), w, px);
    let err = |i: usize| {
        let (p, (x, y)) = (px(i), ((i % w) as f32, (i / w) as f32));
        (0..3)
            .map(|c| (p[c] as f32 - plane[c][0] - plane[c][1] * x - plane[c][2] * y).abs())
            .fold(0.0, f32::max)
    };
    let mut errs: Vec<f32> = pts.iter().map(|&i| err(i)).collect();
    errs.sort_by(f32::total_cmp);
    let cut = errs[errs.len() * 7 / 10].max(4.0) * 2.0;
    let kept: Vec<usize> = pts.iter().copied().filter(|&i| err(i) <= cut).collect();
    if kept.len() >= 8 {
        fit_plane(kept.into_iter(), w, px)
    } else {
        plane
    }
}

fn fit_plane(
    pts: impl Iterator<Item = usize>,
    w: usize,
    px: &dyn Fn(usize) -> [u8; 3],
) -> [[f32; 3]; 3] {
    // normal equations for [1, x, y]
    let mut m = [[0f64; 3]; 3];
    let mut v = [[0f64; 3]; 3]; // per channel
    for i in pts {
        let f = [1.0, (i % w) as f64, (i / w) as f64];
        let p = px(i);
        for a in 0..3 {
            for b in 0..3 {
                m[a][b] += f[a] * f[b];
            }
            for (c, vc) in v.iter_mut().enumerate() {
                vc[a] += f[a] * p[c] as f64;
            }
        }
    }
    v.map(|b| solve3(m, b).map(|k| k as f32))
}

fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-9 {
        return [b[0] / m[0][0].max(1.0), 0.0, 0.0]; // degenerate: flat mean
    }
    [0, 1, 2].map(|k| {
        let mut mk = m;
        for r in 0..3 {
            mk[r][k] = b[r];
        }
        det(mk) / d
    })
}

fn luma(c: [u8; 3]) -> f32 {
    0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Fill,
    Accent,
    Shadow,
}

struct Cluster {
    color: [u8; 3],
    role: Role,
    /// Centroid offset (dx + dy) from the fill: > 0 = shadow falls down-right.
    offset: f64,
}

/// Up to three glyph colours from the strongest candidate pixels.
/// The fill is the big cluster that touches the background least (an outline
/// wraps the fill, so the outline touches the background more).
fn clusters(
    cand: &[bool],
    w: usize,
    px: &dyn Fn(usize) -> [u8; 3],
    is_bg: &dyn Fn([u8; 3]) -> bool,
) -> Vec<Cluster> {
    let idx: Vec<usize> = (0..cand.len()).filter(|&i| cand[i]).collect();
    if idx.is_empty() {
        return Vec::new();
    }
    // 4-bit-per-channel bins: [count, r, g, b, x, y, touches background]
    let mut bins = vec![[0f64; 7]; 4096];
    for &i in &idx {
        let p = px(i);
        let touches = [i.wrapping_sub(1), i + 1, i.wrapping_sub(w), i + w]
            .iter()
            .any(|&n| n < cand.len() && !cand[n]);
        let b =
            &mut bins[(p[0] as usize >> 4) << 8 | (p[1] as usize >> 4) << 4 | p[2] as usize >> 4];
        let vals = [
            1.0,
            p[0] as f64,
            p[1] as f64,
            p[2] as f64,
            (i % w) as f64,
            (i / w) as f64,
            touches as u8 as f64,
        ];
        for (k, v) in vals.into_iter().enumerate() {
            b[k] += v;
        }
    }
    let col = |b: &[f64; 7]| [1, 2, 3].map(|k| (b[k] / b[0]) as u8);
    let mut order: Vec<usize> = (0..4096).filter(|&b| bins[b][0] > 0.0).collect();
    if order.iter().any(|&b| !is_bg(col(&bins[b]))) {
        order.retain(|&b| !is_bg(col(&bins[b])));
    }
    order.sort_by(|a, b| bins[*b][0].total_cmp(&bins[*a][0]));
    let far = |a: [u8; 3], b: [u8; 3]| (0..3).any(|k| (a[k] as i32 - b[k] as i32).abs() > 64);
    // pick distinct big bins
    let mut picked: Vec<[f64; 7]> = Vec::new();
    for &o in &order {
        let b = bins[o];
        if b[0] < bins[order[0]][0] * 0.08 || picked.len() == 3 {
            break;
        }
        if let Some(p) = picked.iter_mut().find(|p| !far(col(p), col(&b))) {
            for k in 0..7 {
                p[k] += b[k]; // merge into the near cluster
            }
        } else {
            picked.push(b);
        }
    }
    // the biggest cluster was an outline around the fill: it touches the
    // background more, or (over busy art, where everything is a candidate) it
    // is a dark grey with a much brighter cluster inside
    let grey = |c: [u8; 3]| *c.iter().max().unwrap() - *c.iter().min().unwrap() < 40;
    if picked.len() > 1
        && picked[1][0] > picked[0][0] * 0.25
        && (picked[1][6] / picked[1][0] < 0.6 * picked[0][6] / picked[0][0]
            || (grey(col(&picked[0])) && luma(col(&picked[0])) + 100.0 < luma(col(&picked[1]))))
    {
        picked.swap(0, 1);
    }
    let main = picked[0];
    picked
        .iter()
        .enumerate()
        .map(|(n, b)| {
            let c = col(b);
            let sat = *c.iter().max().unwrap() as i32 - *c.iter().min().unwrap() as i32;
            let role = if n == 0 {
                Role::Fill
            } else if luma(c) < luma(col(&main)) - 60.0 && sat < 60 {
                Role::Shadow
            } else if sat > 60 {
                Role::Accent
            } else {
                Role::Fill
            };
            let offset = (b[4] / b[0] - main[4] / main[0]) + (b[5] / b[0] - main[5] / main[0]);
            Cluster {
                color: c,
                role,
                offset,
            }
        })
        .collect()
}
