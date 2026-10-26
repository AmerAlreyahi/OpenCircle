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
    /// Height of the ink's dense band: x-height (mixed case) or cap height.
    /// An icon in the box (a radio button) makes `ink` tall, not this.
    pub band: f32,
    /// Mean stroke width in px: a weight hint.
    pub stroke: f32,
    /// How unsure the erase is (0-255): the mean mismatch of the two ends of
    /// the best direction over the hole, or three times the background's
    /// grain (fine texture comes back smoothed: a smear), whichever is
    /// larger. Flat or gradient panels ~6, a photo's fine detail high; 255
    /// when what it found is no text (none, most of the box, or the
    /// background's colour).
    pub residual: f32,
    /// Where the line's text may be redrawn, x0..x1: an icon at either end
    /// of the box (a radio button, a magnifier) is no text and stays, so the
    /// redraw keeps off it. Unbounded when there is none.
    pub text_x: [i32; 2],
}

/// The pixels an erase changed, with their old colour (byte index, RGB):
/// applied in reverse, it puts the line back.
pub type Undo = Vec<(usize, [u8; 3])>;

const PAD: i32 = 3;
const RING: i32 = 2;
const HALO: i32 = 4;
/// Longest ray a hole pixel walks to find background.
const REACH: i32 = 40;

/// Erases the glyphs inside `b` (x, y, w, h) of an RGBA image and returns
/// their style and what it changed. Alpha is left as is. A line on a bar
/// that is no taller than the box (a heading on a coloured band) has the
/// outside of the bar on its ring, so its "background" is the bar and the
/// text is lost (so does a neighbour's edge under a tall box: busy); the box
/// is tried again a little smaller, its ring inside.
pub fn erase_line(rgba: &mut [u8], iw: usize, ih: usize, b: [f32; 4]) -> (LineStyle, Undo) {
    let (style, undo) = erase_box(rgba, iw, ih, b);
    if style.residual <= super::BUSY || b[3] < 16.0 {
        return (style, undo);
    }
    let put_back = |rgba: &mut [u8], undo: &Undo| {
        for (at, old) in undo.iter().rev() {
            rgba[*at..at + 3].copy_from_slice(old);
        }
    };
    put_back(rgba, &undo);
    for inset in [0.35, 0.28, 0.2, 0.12] {
        let k = inset * b[3];
        let nb = [b[0], b[1] + k, b[2], b[3] - 2.0 * k];
        let (again, undo2) = erase_box(rgba, iw, ih, nb);
        // and the text must still be whole inside it
        let (top, bottom) = (nb[1] as i32 - PAD, (nb[1] + nb[3]).ceil() as i32 + PAD);
        if again.residual <= super::BUSY
            && again.ink[1] > top + RING
            && again.ink[3] < bottom - RING
        {
            return (again, undo2);
        }
        put_back(rgba, &undo2);
    }
    erase_box(rgba, iw, ih, b)
}

fn erase_box(rgba: &mut [u8], iw: usize, ih: usize, b: [f32; 4]) -> (LineStyle, Undo) {
    let x0 = (b[0] as i32 - PAD).clamp(0, iw as i32);
    let y0 = (b[1] as i32 - PAD).clamp(0, ih as i32);
    let x1 = ((b[0] + b[2]).ceil() as i32 + PAD).clamp(x0, iw as i32);
    let y1 = ((b[1] + b[3]).ceil() as i32 + PAD).clamp(y0, ih as i32);
    // The top and bottom ring rows must be background: in tight text a
    // neighbour line's descenders or ascenders reach into the box, and their
    // colour (the text's own) would pass for art crossing the box; so would
    // a picture's edge or a text field's border along the box. An edge
    // whose two rows are not quiet (nearly all one shade) moves to the first
    // quiet pair within 40 % of the box inward; over busy art no row is
    // quiet and nothing moves.
    let quiet = |y: i32| -> bool {
        let row = &rgba[(y as usize * iw + x0 as usize) * 4..(y as usize * iw + x1 as usize) * 4];
        let mut l: Vec<f32> = row.chunks(4).map(|p| luma([p[0], p[1], p[2]])).collect();
        l.sort_by(f32::total_cmp);
        let med = l.get(l.len() / 2).copied().unwrap_or(0.0);
        l.iter().filter(|v| (*v - med).abs() > 24.0).count() * 50 <= l.len()
    };
    let reach = (b[3] * 0.4) as i32;
    let (y0, y1) = if y1 - y0 > 2 * reach + 4 {
        let top = (y0..=y0 + reach).find(|&y| quiet(y) && quiet(y + 1));
        let bottom = (y1 - reach..=y1)
            .rev()
            .find(|&y| quiet(y - 1) && quiet(y - 2));
        (top.unwrap_or(y0), bottom.unwrap_or(y1))
    } else {
        (y0, y1)
    };
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
        let style = LineStyle {
            color: [255; 3],
            accent: None,
            shadow: None,
            ink: [x0, y0, x1, y1],
            band: 0.0,
            stroke: 0.0,
            residual: 0.0,
            text_x: [i32::MIN, i32::MAX],
        };
        return (style, Vec::new());
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
    let mut cand: Vec<bool> = (0..w * h)
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
    // A text field's or a button's frame is no text: a thin straight run
    // longer than any glyph (a column of 90 % of the OCR box's height, a row
    // as wide as the box or 2.5 heights long), not in an art colour, stays,
    // with its anti-aliasing.
    let ocr_h = b[3] / 1.3;
    let row_len = (0.85 * (w as f32 - 2.0 * RING as f32)).min(2.5 * ocr_h);
    let lines: Vec<bool> = (0..w * h).map(|i| cand[i] && !art(px(i))).collect();
    let (frame, frame_cols) = straight(&lines, w, 0.9 * ocr_h, row_len);
    let near_frame = dilate(&frame, w, 1);
    for (c, f) in cand.iter_mut().zip(&frame) {
        *c &= !f;
    }
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
    let mut text_cand: Vec<bool> = (0..w * h).map(|i| cand[i] && !is_art(i)).collect();
    // an icon at either end (a radio button, a magnifier) stays as it is,
    // and its colour is not the text's
    let [keep0, keep1] = icons(&text_cand, w);
    for i in (0..w * h).filter(|&i| i % w < keep0 || i % w >= keep1) {
        text_cand[i] = false;
    }
    let cl = clusters(&text_cand, w, &px, &|_| false);
    let mut glyph = vec![false; w * h];
    let mut ink_only = vec![false; w * h];
    // On a plain background every clear candidate is text, also the ones
    // between cluster colours (ClearType draws thin stems in fringe colours);
    // on busy art only colours near a glyph cluster are.
    let plain = sigma * 3.0 < 28.0;
    // cluster of each glyph pixel (usize::MAX: none)
    let mut member = vec![usize::MAX; w * h];
    for i in (0..w * h).filter(|&i| text_cand[i]) {
        match cl.iter().position(|c| near(px(i), c.color)) {
            Some(k) => {
                glyph[i] = true;
                ink_only[i] = cl[k].role != Role::Shadow;
                member[i] = k;
            }
            None if plain && strong[i] => (glyph[i], ink_only[i]) = (true, true),
            None => {}
        }
    }
    // the redraw keeps off an icon, and off a frame beside the text (a
    // button's edge)
    let inked: Vec<usize> = (0..w)
        .filter(|&x| (0..h).any(|y| glyph[y * w + x]))
        .collect();
    let (t0, t1) = (
        inked.first().copied().unwrap_or(0),
        inked.last().map_or(w, |x| x + 1),
    );
    // a thin line through the whole box (a panel's border behind a label)
    // is art for the erase, which leaves it, but the redraw keeps off it too
    let through = |c: usize| {
        c < w
            && (0..h).filter(|&y| cand[y * w + c]).count() * 10
                >= h.saturating_sub(2 * RING as usize) * 9
    };
    let frame_cols: Vec<usize> = (frame_cols.into_iter())
        .chain(
            (RING as usize..w)
                .filter(|&c| through(c) && !through(c + 2) && !(c >= 2 && through(c - 2))),
        )
        .collect();
    let left = (frame_cols.iter().filter(|&&c| c < t0).map(|&c| c + 2))
        .chain((keep0 > 0).then_some(keep0))
        .max();
    let right = (frame_cols.iter().filter(|&&c| c >= t1).map(|&c| c - 1))
        .chain((keep1 < w).then_some(keep1))
        .min();
    let text_x = [
        left.map_or(i32::MIN, |x| x0 + x as i32),
        right.map_or(i32::MAX, |x| x0 + x as i32),
    ];
    // the background's brightness: the median of the ring
    let mut ring_l: Vec<f32> = ring.iter().map(|&i| luma(px(i))).collect();
    ring_l.sort_by(f32::total_cmp);
    let bg_luma = ring_l[ring_l.len() / 2];
    // An outline or shadow hugs the fill: most fill pixels have it within
    // 2 px. Black words between blue links do not, and are text.
    let hugs_fill = |k: usize| {
        let near = dilate(&member.iter().map(|&m| m == k).collect::<Vec<_>>(), w, 2);
        let (n, by) = (0..w * h)
            .filter(|&i| member[i] == 0)
            .fold((0, 0), |(n, by), i| (n + 1, by + near[i] as usize));
        by * 5 >= n * 3
    };
    // Dark text on a light page: thin strokes are mostly anti-aliasing
    // (ClearType fringes are orange, blue or near white), so the biggest
    // cluster can be a fringe mix. It is the text's colour (a link's blue
    // among black brackets) only if about as dark as the darkest fifth of
    // the glyphs; otherwise that darkest fifth is. (Text clearly lighter than
    // a light background, white on a bright sky, keeps its cluster.)
    let color = match (cl.first(), darkest(&glyph, &px)) {
        (Some(c), Some(core)) if bg_luma > 170.0 && luma(c.color) < bg_luma + 8.0 => {
            let contrast = |c: [u8; 3]| bg_luma - luma(c);
            // Nor is a fringe colour around grey ink: small ClearType text
            // is warm on one side of a stem and cool on the other, about as
            // much of each (a link is mostly one hue).
            let sat = |c: [u8; 3]| c.iter().max().unwrap() - c.iter().min().unwrap();
            let (mut warm, mut cool) = (0, 0);
            for p in (0..w * h)
                .filter(|&i| glyph[i])
                .map(px)
                .filter(|&p| sat(p) > 60)
            {
                if p[0] > p[2] { warm += 1 } else { cool += 1 }
            }
            let fringe =
                sat(c.color) > 60 && sat(core) < 40 && warm.min(cool) * 5 >= warm.max(cool) * 2;
            if contrast(c.color) >= 0.7 * contrast(core) && !fringe {
                c.color
            } else {
                core
            }
        }
        (Some(c), _) => c.color,
        (None, _) => fill_c,
    };
    let contrast = |c: [u8; 3]| (luma(c) - bg_luma).abs();
    let fill_contrast = contrast(color);
    let (mut accent, mut shadow) = (None, None);
    for (k, c) in cl.iter().enumerate().skip(1) {
        match c.role {
            // (a faint fringe colour is no accent)
            Role::Accent if contrast(c.color) * 2.0 >= fill_contrast => {
                accent = accent.or(Some(c.color))
            }
            // On a light page only light text has a dark outline (white on a
            // bright sky); dark text's darker "outline" is its own core
            // inside ClearType fringes.
            Role::Shadow
                if luma(c.color) < bg_luma - 15.0
                    && (bg_luma <= 170.0 || luma(color) >= bg_luma - 40.0)
                    && hugs_fill(k) =>
            {
                shadow = shadow.or(Some((c.color, c.offset > 1.0)))
            }
            Role::Shadow => {
                // text of its own: its pixels are ink
                for i in (0..w * h).filter(|&i| member[i] == k) {
                    ink_only[i] = true;
                }
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
        *m &= !near_frame[i] && i % w >= keep0 && i % w < keep1;
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
    let (filled, unsure) = fill(&win, &mask, w, grain, &bg);
    // Not text at all: the "glyphs" cover most of the box (a picture or a
    // dark band crossing it), or their colour is the background's.
    let inner = (0..w * h).filter(|&i| !on_ring(i)).count().max(1);
    let ring_mid = [0, 1, 2].map(|k| {
        let mut v: Vec<u8> = ring.iter().map(|&i| px(i)[k]).collect();
        v.sort_unstable();
        v[v.len() / 2]
    });
    let like_bg = (0..3).all(|k| color[k].abs_diff(ring_mid[k]) < 30);
    let lost = filled.is_empty() || filled.len() * 4 > inner * 3 || like_bg;
    // fine texture (grain) is smoothed away inside the hole: a smear
    let residual = if lost { 255.0 } else { unsure.max(3.0 * grain) };
    let mut undo = Vec::with_capacity(filled.len());
    for (i, v) in filled {
        let px = &mut rgba[at(i)..at(i) + 3];
        undo.push((at(i), [px[0], px[1], px[2]]));
        px.copy_from_slice(&v);
    }
    let (ink, band) = extent(&ink_only, w, x0, y0);
    let style = LineStyle {
        color,
        accent,
        shadow,
        ink,
        band,
        stroke: stroke_width(&ink_only, w),
        residual,
        text_x,
    };
    (style, undo)
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
/// pixel (known pixels are only read, so the order does not matter) and the
/// mean mismatch of the best direction (how unsure the fill is).
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
) -> (Vec<(usize, [u8; 3])>, f32) {
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
    let mut unsure = 0.0;
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
        unsure += if n > 0 { best } else { 255.0 };
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
    let n = out.len().max(1) as f32;
    (out, unsure / n)
}

/// Frame lines in `m`: thin (one or two px side by side) straight runs, a
/// column run at least `col` long or a row run at least `row` long. Returns
/// their pixels and the columns of the column runs. A column run through the
/// whole window (ring to ring) is art crossing the box, not a frame (the
/// colour test handles it); a thick run is a stem, a trunk, a bar; a column
/// run must reach past the rest of the ink at both ends.
fn straight(m: &[bool], w: usize, col: f32, row: f32) -> (Vec<bool>, Vec<usize>) {
    let h = m.len() / w;
    let mut out = vec![false; m.len()];
    // runs (line, start, end) along each line of `lines`, `len` long
    let runs = |lines: usize, len: usize, idx: &dyn Fn(usize, usize) -> usize, min: f32| {
        let mut found = Vec::new();
        for l in 0..lines {
            let mut k = 0;
            while k < len {
                let start = k;
                while k < len && m[idx(l, k)] {
                    k += 1;
                }
                let through = len == h && start <= RING as usize && k + RING as usize >= len;
                if (k - start) as f32 >= min && !through {
                    found.push((l, start, k));
                }
                k += 1;
            }
        }
        // thin: no long run two lines away on either side
        let near = |l: usize, d: isize| found.iter().any(|r| r.0 as isize == l as isize + d);
        let thick = |l| near(l, -2) || near(l, 2) || (near(l, -1) && near(l, 1));
        let thin: Vec<_> = found.iter().filter(|r| !thick(r.0)).copied().collect();
        thin
    };
    // a column frame reaches past the rest of the ink, above and below (a
    // stem of the text's own height does not)
    for (y, x0, x1) in runs(h, w, &|y, x| y * w + x, row) {
        (x0..x1).for_each(|x| out[y * w + x] = true);
    }
    let cols = runs(w, h, &|x, y| y * w + x, col);
    let near_col = |x: usize| cols.iter().any(|r| r.0.abs_diff(x) <= 1);
    let rest: Vec<usize> = (0..h)
        .filter(|&y| (0..w).any(|x| m[y * w + x] && !out[y * w + x] && !near_col(x)))
        .collect();
    let cols: Vec<_> = match (rest.first(), rest.last()) {
        (Some(&top), Some(&bottom)) => (cols.iter())
            .filter(|r| r.1 + 2 <= top && r.2 >= bottom + 3)
            .copied()
            .collect(),
        _ => Vec::new(),
    };
    for &(x, y0, y1) in &cols {
        (y0..y1).for_each(|y| out[y * w + x] = true);
    }
    (out, cols.into_iter().map(|r| r.0).collect())
}

/// Icons at the ends of a line (a radio button before its label, a
/// magnifier in a search field): ink before the first (after the last) gap
/// of 3+ empty columns that is as wide as about two thirds of its height,
/// and a third taller than the rest of the line, reaching above it.
/// Returns the columns the text spans without them, x0..x1 (all: no icon).
// ponytail: shape-blind (a big round initial would pass); a real icon
// classifier if that ever happens.
fn icons(glyph: &[bool], w: usize) -> [usize; 2] {
    let h = glyph.len() / w;
    let inked = |x: usize| (0..h).any(|y| glyph[y * w + x]);
    // runs of inked columns, split at gaps of 3+ empty ones
    let mut segs: Vec<[usize; 2]> = Vec::new();
    for x in (0..w).filter(|&x| inked(x)) {
        match segs.last_mut() {
            Some(s) if x - s[1] < 3 => s[1] = x + 1,
            _ => segs.push([x, x + 1]),
        }
    }
    // the longest run of inked rows (one-row gaps bridged) over columns x0..x1
    let rows = |[x0, x1]: [usize; 2]| {
        let on: Vec<usize> = (0..h)
            .filter(|&y| (x0..x1).any(|x| glyph[y * w + x]))
            .collect();
        let (mut best, mut start) = ([0, 0], 0);
        for k in 0..on.len() {
            if k > 0 && on[k] - on[k - 1] > 2 {
                start = k;
            }
            if on[k] + 1 - on[start] > best[1] - best[0] {
                best = [on[start], on[k] + 1];
            }
        }
        best
    };
    let icon = |seg: [usize; 2], rest: [usize; 2]| {
        let (r, o) = (rows(seg), rows(rest));
        let (sh, oh) = ((r[1] - r[0]) as f32, (o[1] - o[0]) as f32);
        r[0] < o[0] && sh >= 1.3 * oh && (seg[1] - seg[0]) as f32 >= 0.6 * sh
    };
    let mut keep = [0, w];
    if let [first, .., last] = segs[..] {
        if icon(first, [segs[1][0], last[1]]) {
            keep[0] = segs[1][0];
        }
        if icon(last, [first[0], segs[segs.len() - 2][1]]) {
            keep[1] = segs[segs.len() - 2][1];
        }
    }
    keep
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

/// Ink box: the tallest run of inked rows, bridging gaps of up to two rows
/// (an i's dot), so a neighbour line's descenders or ascenders poking into
/// the window, or a frame line along it, are cut off by the blank rows in
/// between; and the columns inked in those rows.
/// Also the height of the dense band (rows with at least half the busiest
/// row's ink): the x-height in mixed-case text, the cap height in capitals.
fn extent(m: &[bool], w: usize, x0: i32, y0: i32) -> ([i32; 4], f32) {
    // (a row inked across 80 % of the window is a rule or a field border)
    let rows: Vec<usize> = (m.chunks(w))
        .map(|r| r.iter().filter(|&&on| on).count())
        .map(|n| if n * 5 > w * 4 { 0 } else { n })
        .collect();
    let (mut best, mut run) = ((0, 0, 0), None::<(usize, usize, usize)>);
    for (y, &n) in rows.iter().enumerate() {
        if n == 0 {
            continue;
        }
        run = match run {
            Some((top, last, ink)) if y - last <= 3 => Some((top, y, ink + n)),
            _ => Some((y, y, n)),
        };
        let (top, last, ink) = run.expect("just set");
        if (last - top, ink) > (best.1 - best.0, best.2) {
            best = (top, last, ink);
        }
    }
    let (top, bottom) = (best.0, best.1);
    let mut e = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for i in (top * w..(bottom + 1) * w).filter(|&i| m[i]) {
        let (x, y) = ((i % w) as i32 + x0, (i / w) as i32 + y0);
        e = [e[0].min(x), e[1].min(y), e[2].max(x + 1), e[3].max(y + 1)];
    }
    let peak = rows[top..=bottom].iter().max().copied().unwrap_or(0);
    let band = rows[top..=bottom]
        .iter()
        .filter(|&&n| n * 2 >= peak)
        .count() as f32;
    if e[0] == i32::MAX {
        ([x0, y0, x0 + w as i32, y0 + (m.len() / w) as i32], 0.0)
    } else {
        (e, band)
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
    // A neighbour's panel along a tenth of the ring (a popup's edge beside a
    // link) tilts a plain least-squares plane through the text: first drop
    // what is far from the ring's median colour (a gradient's ends stay: the
    // cut follows the ring's own spread).
    let med = [0, 1, 2].map(|c| {
        let mut v: Vec<u8> = pts.iter().map(|&i| px(i)[c]).collect();
        v.sort_unstable();
        v[v.len() / 2] as f32
    });
    let off = |i: usize| {
        (0..3)
            .map(|c| (px(i)[c] as f32 - med[c]).abs())
            .fold(0.0, f32::max)
    };
    let mut offs: Vec<f32> = pts.iter().map(|&i| off(i)).collect();
    offs.sort_by(f32::total_cmp);
    let cut = offs[offs.len() * 7 / 10].max(4.0) * 2.0;
    let near: Vec<usize> = pts.iter().copied().filter(|&i| off(i) <= cut).collect();
    let pts = if near.len() >= 8 { &near[..] } else { pts };
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

/// Mean colour of the darkest fifth of the masked pixels.
fn darkest(m: &[bool], px: &dyn Fn(usize) -> [u8; 3]) -> Option<[u8; 3]> {
    let mut ink: Vec<[u8; 3]> = (0..m.len()).filter(|&i| m[i]).map(px).collect();
    ink.sort_by(|a, b| luma(*a).total_cmp(&luma(*b)));
    let core = &ink[..ink.len().div_ceil(5)];
    (!core.is_empty()).then(|| {
        [0, 1, 2].map(|k| (core.iter().map(|c| c[k] as u32).sum::<u32>() / core.len() as u32) as u8)
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
