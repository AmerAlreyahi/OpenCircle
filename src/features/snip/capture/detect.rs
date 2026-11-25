//! What the frozen picture itself shows (planning #200): straight edges to
//! snap an area's edge to, and the panel, button or menu under the pointer as
//! a rectangle. It reads pixels only, so it works on every platform: where the
//! OS gives windows and controls (Windows UI Automation, X11 windows) they
//! come first and this fills the gaps (custom-drawn UI, games); on Wayland,
//! which gives neither, it is the only element finder.
//! All coordinates are physical px of the one frame.

/// A frozen frame: RGBA, row by row.
#[derive(Clone, Copy)]
pub struct Pic<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// A pixel counts as an edge when its brightness differs from its left / upper
/// neighbour by this much (of 255; a soft shadow stays under it).
const EDGE: i32 = 28;
/// A straight edge is at least this long (physical px): text strokes and icon
/// details are shorter and are not caught.
const RUN: i32 = 16;
/// How far along the edge the run may be looked for from the pointer, and how
/// far it counts at most (longer edges win, up to this).
const LOOK: i32 = 4;
const LONG: i32 = 64;

impl Pic<'_> {
    fn ok(&self) -> bool {
        self.rgba.len() >= (self.width * self.height * 4) as usize
            && self.width > 2
            && self.height > 2
    }

    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    fn luma(&self, x: i32, y: i32) -> i32 {
        let [r, g, b] = self.at(x, y);
        (2 * i32::from(r) + 5 * i32::from(g) + i32::from(b)) / 8
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32
    }

    /// A vertical edge between column `x - 1` and `x` at row `y`.
    fn edge_x(&self, x: i32, y: i32) -> bool {
        self.inside(x, y)
            && self.inside(x - 1, y)
            && (self.luma(x, y) - self.luma(x - 1, y)).abs() >= EDGE
    }

    /// A horizontal edge between row `y - 1` and `y` at column `x`.
    fn edge_y(&self, x: i32, y: i32) -> bool {
        self.inside(x, y)
            && self.inside(x, y - 1)
            && (self.luma(x, y) - self.luma(x, y - 1)).abs() >= EDGE
    }

    /// How far the straight edge through `(x, y)` runs (both ways, up to
    /// `LONG`), the line being `along` cells long on the other axis.
    fn run(&self, x: i32, y: i32, vertical: bool) -> i32 {
        let on = |t: i32| match vertical {
            true => self.edge_x(x, y + t),
            false => self.edge_y(x + t, y),
        };
        // the pointer may be a few px off the edge's ends: start on its line
        let Some(start) = (-LOOK..=LOOK).find(|&t| on(t)) else {
            return 0;
        };
        let (mut lo, mut hi) = (start, start);
        while hi - lo < LONG && on(hi + 1) {
            hi += 1;
        }
        while hi - lo < LONG && on(lo - 1) {
            lo -= 1;
        }
        hi - lo + 1
    }
}

/// The straight edge nearest `(x, y)` within `reach` px on each axis: the
/// column a vertical edge sits at and the row a horizontal one sits at, if
/// there is one. A longer edge beats a shorter one a little nearer.
pub fn edges(pic: Pic<'_>, x: i32, y: i32, reach: i32) -> (Option<i32>, Option<i32>) {
    if !pic.ok() {
        return (None, None);
    }
    let best = |vertical: bool| -> Option<i32> {
        let centre = if vertical { x } else { y };
        let mut best: Option<(i32, i32)> = None; // (score, position)
        for d in -reach..=reach {
            let at = centre + d;
            let run = match vertical {
                true => pic.run(at, y, true),
                false => pic.run(x, at, false),
            };
            if run < RUN {
                continue;
            }
            let score = run * 2 - d.abs();
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, at));
            }
        }
        best.map(|(_, at)| at)
    };
    (best(true), best(false))
}

/// A pixel is the region's own when it is this near its colour (per channel).
const TOL: i32 = 10;
/// A line of the region is kept while this share of it is the region's colour (a row of dense
/// text is about half ink, the Windows VM's first run showed a panel cut at 0.5):
/// text and icons inside a panel stay in it, a border or another panel ends it.
const SHARE: f32 = 0.4;
/// The most of the frame a region may cover (2 / 5): more is the page.
/// The smallest rectangle worth outlining.
const SMALLEST: i32 = 12;
/// The most pixels one search reads (a whole 4K page is about 8 M).
const BUDGET: usize = 6_000_000;

/// The panel, button, menu or field under `(x, y)`: the rectangle round it in
/// the colour it stands on, grown until its edge meets something else (x0, y0,
/// x1, y1, ends exclusive). `None`: over a picture or a gradient, or the
/// region is the whole frame (the screen is its own pick).
pub fn region(pic: Pic<'_>, x: i32, y: i32) -> Option<(i32, i32, i32, i32)> {
    if !pic.ok() || !pic.inside(x, y) {
        return None;
    }
    let bg = background(pic, x, y)?;
    let same = |px: [u8; 3]| (0..3).all(|c| (i32::from(px[c]) - i32::from(bg[c])).abs() <= TOL);
    let (w, h) = (pic.width as i32, pic.height as i32);
    let reads = std::cell::Cell::new(0usize);
    // the share of the colour along one line of the rectangle
    let line = |vertical: bool, at: i32, from: i32, to: i32| -> bool {
        let mut n = 0;
        for t in from..to {
            let (px, py) = if vertical { (at, t) } else { (t, at) };
            if pic.inside(px, py) && same(pic.at(px, py)) {
                n += 1;
            }
        }
        reads.set(reads.get() + (to - from).max(0) as usize);
        (n as f32) >= SHARE * (to - from).max(1) as f32
    };
    let r = 6;
    let (mut x0, mut y0) = ((x - r).max(0), (y - r).max(0));
    let (mut x1, mut y1) = ((x + r + 1).min(w), (y + r + 1).min(h));
    // each side grows while its next line is the colour; a side that stopped
    // is tried again once the others have made it a good deal longer (a button
    // beside the pointer cut the panel's bottom only while the span was short)
    let mut stopped: [Option<i32>; 4] = [None; 4]; // left, top, right, bottom: the span it stopped at
    loop {
        let mut grew = false;
        for side in 0..4 {
            let span = if side % 2 == 0 { y1 - y0 } else { x1 - x0 };
            let at_edge = [x0 <= 0, y0 <= 0, x1 >= w, y1 >= h][side];
            if at_edge || stopped[side].is_some_and(|s| span < s + (s / 8).max(8)) {
                continue;
            }
            let pass = match side {
                0 => line(true, x0 - 1, y0, y1),
                1 => line(false, y0 - 1, x0, x1),
                2 => line(true, x1, y0, y1),
                _ => line(false, y1, x0, x1),
            };
            stopped[side] = (!pass).then_some(span);
            if pass {
                grew = true;
                match side {
                    0 => x0 -= 1,
                    1 => y0 -= 1,
                    2 => x1 += 1,
                    _ => y1 += 1,
                }
            }
        }
        if !grew {
            break;
        }
        // too big a flat area (the page, the wallpaper): not an element, and
        // not worth reading on: the window or the screen is that pick
        if reads.get() >= BUDGET || (x1 - x0) * (y1 - y0) > (w * h) * 2 / 5 {
            return None;
        }
    }
    // the first grown rectangle may sit across an edge the pointer is next to:
    // take back lines that are not the colour
    while x1 - x0 > SMALLEST && !line(true, x0, y0, y1) {
        x0 += 1;
    }
    while x1 - x0 > SMALLEST && !line(true, x1 - 1, y0, y1) {
        x1 -= 1;
    }
    while y1 - y0 > SMALLEST && !line(false, y0, x0, x1) {
        y0 += 1;
    }
    while y1 - y0 > SMALLEST && !line(false, y1 - 1, x0, x1) {
        y1 -= 1;
    }
    let whole = (x1 - x0) * (y1 - y0) >= (w * h) * 19 / 20;
    let small = x1 - x0 < SMALLEST || y1 - y0 < SMALLEST;
    (!whole && !small && x0 <= x && x < x1 && y0 <= y && y < y1).then_some((x0, y0, x1, y1))
}

/// The colour the pointer stands on: the most common colour (16 levels per
/// channel) in the 17 x 17 square round it, averaged; `None` over a picture
/// (no colour covers a third of the square).
fn background(pic: Pic<'_>, x: i32, y: i32) -> Option<[u8; 3]> {
    use std::collections::HashMap;
    let mut count: HashMap<[u8; 3], (u32, [u32; 3])> = HashMap::new();
    let mut total = 0;
    for dy in -8..=8 {
        for dx in -8..=8 {
            let (px, py) = (x + dx, y + dy);
            if !pic.inside(px, py) {
                continue;
            }
            let c = pic.at(px, py);
            let e = count.entry(c.map(|v| v >> 4)).or_default();
            e.0 += 1;
            for (sum, v) in e.1.iter_mut().zip(c) {
                *sum += u32::from(v);
            }
            total += 1;
        }
    }
    let (n, sum) = count.values().max_by_key(|(n, _)| *n).copied()?;
    (n * 3 >= total).then(|| sum.map(|s| (s / n) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page (white) with a grey panel at (100, 60)-(300, 200) holding a blue
    /// button at (120, 150)-(180, 180) and three text lines (thin dark bars).
    fn screen() -> (Vec<u8>, u32, u32) {
        let (w, h) = (400u32, 300u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 3]| {
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 3].copy_from_slice(&c);
                }
            }
        };
        fill(100, 60, 300, 200, [225, 225, 230]);
        fill(120, 150, 180, 180, [40, 110, 230]);
        for row in 0..3 {
            let y = 80 + row * 14;
            // text: short dark dashes, 40 % of the line
            for k in 0..16 {
                fill(110 + k * 12, y, 110 + k * 12 + 5, y + 3, [30, 30, 30]);
            }
        }
        (rgba, w, h)
    }

    fn pic(s: &(Vec<u8>, u32, u32)) -> Pic<'_> {
        Pic {
            rgba: &s.0,
            width: s.1,
            height: s.2,
        }
    }

    /// 200-1: the panel under the pointer, from an empty spot of it and from
    /// over its text (text does not cut the panel).
    #[test]
    fn g200_1_a_panel_is_found_from_its_empty_part_and_from_its_text() {
        let s = screen();
        assert_eq!(region(pic(&s), 250, 130), Some((100, 60, 300, 200)));
        assert_eq!(region(pic(&s), 130, 81), Some((100, 60, 300, 200)));
    }

    /// 200-2: a button inside the panel is its own region (finer than the panel).
    #[test]
    fn g200_2_a_button_is_found_inside_its_panel() {
        let s = screen();
        assert_eq!(region(pic(&s), 150, 165), Some((120, 150, 180, 180)));
    }

    /// 200-3: the bare page is the whole frame: no region (the screen is its
    /// own pick); a picture-like noise gives none either.
    #[test]
    fn g200_3_the_whole_frame_and_pictures_are_not_regions() {
        // (bare page beside a panel is a strip of the page: still a region; the
        // flat frame below is the whole frame)
        let flat = (vec![250u8; 400 * 300 * 4], 400, 300);
        assert_eq!(region(pic(&flat), 20, 20), None);
        let (w, h) = (200u32, 200u32);
        let noise: Vec<u8> = (0..w * h * 4)
            .map(|i| (i.wrapping_mul(2654435761u32) >> 13 & 255) as u8)
            .collect();
        let p = Pic {
            rgba: &noise,
            width: w,
            height: h,
        };
        assert_eq!(region(p, 100, 100), None);
    }

    /// 200-4: magnetic edges: near the panel's left edge the pointer's column
    /// goes to it, near its top edge its row; far from any edge nothing; the
    /// short text dashes are not edges.
    #[test]
    fn g200_4_straight_edges_catch_and_short_strokes_do_not() {
        let s = screen();
        assert_eq!(edges(pic(&s), 104, 130, 8), (Some(100), None));
        assert_eq!(edges(pic(&s), 200, 66, 8), (None, Some(60)));
        // the corner catches both
        assert_eq!(edges(pic(&s), 296, 196, 8), (Some(300), Some(200)));
        // the middle of the panel: the dashes (3 px) are no edge
        assert_eq!(edges(pic(&s), 200, 112, 8), (None, None));
        // the button's left edge is a short edge (30 px) but long enough
        assert_eq!(edges(pic(&s), 123, 165, 8).0, Some(120));
    }

    /// 200-5: the search stays cheap: a full-size 4K frame is read in a bounded
    /// number of pixels (the budget), and a flat one gives no region.
    #[test]
    fn g200_5_a_4k_frame_is_searched_within_the_budget() {
        let (w, h) = (3840u32, 2160u32);
        let rgba = vec![250u8; (w * h * 4) as usize];
        let p = Pic {
            rgba: &rgba,
            width: w,
            height: h,
        };
        let t = std::time::Instant::now();
        assert_eq!(region(p, 1000, 1000), None);
        assert!(t.elapsed().as_millis() < 2000, "{:?}", t.elapsed());
    }

    /// 200-16: a row of very dense text (more than half ink) does not cut its panel (the Windows VM
    /// run: dashes 8 px wide every 15 px cut it at the first row).
    #[test]
    fn g200_16_dense_text_rows_do_not_cut_their_panel() {
        let (w, h) = (400u32, 300u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 3]| {
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 3].copy_from_slice(&c);
                }
            }
        };
        fill(100, 60, 300, 200, [225, 225, 230]);
        for row in 0..5 {
            for k in 0..13 {
                fill(
                    105 + k * 15,
                    80 + row * 28,
                    105 + k * 15 + 8,
                    84 + row * 28,
                    [30, 30, 30],
                );
            }
        }
        let s = (rgba, w, h);
        assert_eq!(region(pic(&s), 250, 180), Some((100, 60, 300, 200)));
    }
}
