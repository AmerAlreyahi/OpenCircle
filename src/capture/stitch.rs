//! Scrolling-capture stitcher (spike S5, production port).
//!
//! Feed it successive captures of one screen region while the content scrolls
//! down. Per frame: a colour signature per row in 4 px bins (right-hand scrollbar
//! strip left out), sticky header/footer = rows fixed in every moving pair,
//! scroll offset = lowest mean row cost over informative rows. Output pixels are always
//! copied from a real frame, never blended. Pure: no I/O, no platform code.
#![allow(dead_code)] // wired by the snip UI later; the tests exercise everything

/// Hard cap on the stitched height, in pixels.
pub const MAX_HEIGHT: u32 = 30_000;

/// Pixels per signature bin. Fine enough that two text lines differing in one
/// character differ in a bin (64 bins over the width could not tell them apart).
const BIN_PX: usize = 4;
/// A bin whose mean differs more than this in any channel is a mismatch. Per
/// channel: pastel bands of equal luma (pink vs green) must not match.
const BIN_BAD: u8 = 24;
/// A row is informative when one channel's range reaches this (flat rows match anywhere).
const FLAT: u8 = 24;
/// Largest mean row cost accepted for an offset, in percent of the row cap: up to
/// ~40 % of the overlap may be covered by things that do not scroll with the page.
const ACCEPT_PCT: u64 = 40;
/// Informative overlap rows needed, at minimum, to trust an offset.
const MIN_MATCH: usize = 8;
/// Largest scroll step accepted, in percent of the scrolling body. Bigger steps
/// leave too little overlap to tell from a blank-row coincidence.
const MAX_STEP_PCT: usize = 90;

/// Per-row signatures: mean R, G, B of every `BIN_PX` bin, row after row.
struct Rows {
    /// Bytes per row (3 per bin).
    n: usize,
    /// Mismatched bins counted per row (1/8 of them): a row costs at most this,
    /// so rows spoilt by something that does not scroll (popup, video) weigh
    /// little, and a small animation inside a row still lets it match.
    cap: u32,
    sig: Vec<u8>,
    info: Vec<bool>,
}

impl Rows {
    fn row(&self, y: usize) -> &[u8] {
        &self.sig[y * self.n..(y + 1) * self.n]
    }
}

/// Right-hand strip (px) ignored when matching: where a scrollbar thumb moves.
fn scrollbar_strip(w: usize) -> usize {
    (w / 40).clamp(6, 24).min(w / 4)
}

fn rows(rgba: &[u8], w: usize, h: usize) -> Rows {
    let bins = ((w - scrollbar_strip(w)) / BIN_PX).max(1).min(w);
    let px = (bins * BIN_PX).min(w);
    let n = bins * 3;
    let (mut sig, mut info) = (Vec::with_capacity(n * h), Vec::with_capacity(h));
    for row in rgba.chunks_exact(w * 4).take(h) {
        let start = sig.len();
        for bin in row.as_chunks::<4>().0[..px].chunks(BIN_PX) {
            let mut sum = [0u32; 3];
            for p in bin {
                sum[0] += p[0] as u32;
                sum[1] += p[1] as u32;
                sum[2] += p[2] as u32;
            }
            sig.extend(sum.map(|v| (v / bin.len() as u32) as u8));
        }
        let r = &sig[start..];
        let range = |c: usize| {
            let (mn, mx) =
                (r.iter().skip(c).step_by(3)).fold((255u8, 0u8), |(a, b), &v| (a.min(v), b.max(v)));
            mx.saturating_sub(mn)
        };
        info.push((0..3).any(|c| range(c) >= FLAT));
    }
    Rows {
        n,
        cap: (bins as u32 / 8).max(4),
        sig,
        info,
    }
}

fn differ(x: &[u8; 3], y: &[u8; 3]) -> bool {
    x.iter().zip(y).any(|(p, q)| p.abs_diff(*q) > BIN_BAD)
}

/// Bins where two rows differ by more than `BIN_BAD` in a channel, counted up
/// to `cap`. With `still` (b's row in the previous frame at the same screen
/// spot), bins where b did not change are left out: a popup or chat bubble that
/// floats over the page stays put while the page scrolls under it.
fn bad(a: &[u8], b: &[u8], still: Option<&[u8]>, cap: u32) -> u32 {
    let mut n = 0;
    let s = still.unwrap_or(a).as_chunks::<3>().0;
    for ((x, y), z) in a.as_chunks::<3>().0.iter().zip(b.as_chunks::<3>().0).zip(s) {
        if differ(x, y) && differ(z, y) {
            n += 1;
            if n >= cap {
                break;
            }
        }
    }
    n
}

/// Same row, give or take one spoilt bin (a blinking cursor).
fn same(a: &[u8], b: &[u8]) -> bool {
    bad(a, b, None, 2) <= 1
}

/// Fixed (top, bottom) row counts between two frames, or None when the pair
/// looks unmoved (everything fixed), which says nothing about sticky bars.
fn fixed_rows(a: &Rows, b: &Rows) -> Option<(usize, usize)> {
    let h = a.info.len();
    let eq = |y: usize| same(a.row(y), b.row(y));
    let top = (0..h).take_while(|&y| eq(y)).count();
    let bot = (0..h).rev().take_while(|&y| eq(y)).count();
    (top + bot < h).then_some((top, bot))
}

/// Scroll offset d (at most `max_d`) such that b body row y == a body row y+d:
/// the lowest mean row cost over b's informative rows. `Some(0)` also means
/// "no informative rows, assume no movement"; None = no trustworthy match
/// (animated frame, too-large step).
fn offset(a: &Rows, b: &Rows, t: usize, h: usize, max_d: usize) -> Option<usize> {
    let info: Vec<usize> = (0..h).filter(|&y| b.info[t + y]).collect();
    if info.is_empty() {
        return Some(0);
    }
    let need = MIN_MATCH.min(info.len());
    let cost = |d: usize, y: usize| {
        let still = Some(a.row(t + y));
        bad(a.row(t + y + d), b.row(t + y), still, b.cap) as u64
    };
    // Every 8th row first, to try the offsets best first: the full pass then
    // cuts the others short. Stable sort: on a tie 0 ("did not move") wins.
    let mut order: Vec<(u64, usize, usize)> = (0..=max_d.min(h - 1))
        .filter_map(|d| {
            let rows = info.partition_point(|&y| y < h - d);
            (rows >= need).then(|| {
                let sample = info[..rows].iter().step_by(8);
                let (c, n) = sample.fold((0, 0), |(c, n), &y| (c + cost(d, y), n + 1));
                (c * 1000 / n, rows, d)
            })
        })
        .collect();
    order.sort_by_key(|&(c, ..)| c);
    // (cost, rows, d): mean cost = cost / rows.
    let mut best: Option<(u64, u64, usize)> = None;
    for (_, rows, d) in order {
        let limit = ACCEPT_PCT * b.cap as u64 * rows as u64;
        let mut sum = 0u64;
        let beaten = info[..rows].iter().any(|&y| {
            sum += cost(d, y);
            // Past what is accepted, or no better than the best mean: give up on d.
            sum * 100 > limit || best.is_some_and(|(c, r, _)| sum * r >= c * rows as u64)
        });
        if !beaten {
            best = Some((sum, rows as u64, d));
        }
    }
    best.map(|(.., d)| d)
}

/// Result of [`Stitcher::push`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// The frame scrolled by this many new rows, now part of the image (the first frame: its height).
    Added(u32),
    /// Same content as the last accepted frame: the page did not move.
    NoMovement,
    /// Wrong size, no trustworthy match (animated or scrolled too far) or the height cap is reached.
    /// The frame is dropped and the next one is compared with the last accepted frame.
    Rejected,
}

/// Rows of one accepted frame, kept down to the frame bottom so a footer found
/// later (the fixed-row estimate only shrinks) can still be cut off exactly.
struct Chunk {
    data: Vec<u8>,
    /// Frame row where `data` starts.
    top_row: usize,
    /// New rows this chunk adds (the first chunk: unused).
    d: usize,
    /// Rows dropped from the end of the new rows by the height cap.
    cut: usize,
}

/// Incremental stitcher: `push` frames, then `finish`.
pub struct Stitcher {
    w: usize,
    h: usize,
    prev: Option<Rows>,
    /// Sticky (header, footer) rows, the minimum over every accepted moving pair.
    fixed: Option<(usize, usize)>,
    chunks: Vec<Chunk>,
    height: usize,
}

impl Stitcher {
    /// `width` x `height` is the size of every frame that will be pushed.
    pub fn new(width: u32, height: u32) -> Self {
        Stitcher {
            w: width as usize,
            h: height as usize,
            prev: None,
            fixed: None,
            chunks: Vec::new(),
            height: 0,
        }
    }

    /// Height of the image `finish` would return (0 before the first frame).
    pub fn height(&self) -> u32 {
        self.height as u32
    }

    /// True once the height cap is reached; further frames are rejected.
    pub fn is_full(&self) -> bool {
        self.height >= MAX_HEIGHT as usize
    }

    /// Size of the frames this stitcher takes.
    pub fn frame_size(&self) -> (u32, u32) {
        (self.w as u32, self.h as u32)
    }

    pub fn push(&mut self, rgba: &[u8]) -> Push {
        let stride = self.w * 4;
        if self.w == 0 || self.h == 0 || rgba.len() != stride * self.h {
            return Push::Rejected;
        }
        let rows = rows(rgba, self.w, self.h);
        let Some(prev) = &self.prev else {
            self.chunks.push(Chunk {
                data: rgba.to_vec(),
                top_row: 0,
                d: self.h,
                cut: 0,
            });
            self.prev = Some(rows);
            self.height = self.h;
            return Push::Added(self.h as u32);
        };
        let Some((t, b)) = fixed_rows(prev, &rows) else {
            return Push::NoMovement;
        };
        // Tentative: only kept when the pair turns out to be a real scroll.
        let (top, bot) = self.fixed.map_or((t, b), |(ft, fb)| (ft.min(t), fb.min(b)));
        let body = self.h - top - bot;
        let max_d = body * MAX_STEP_PCT / 100;
        let d = match offset(prev, &rows, top, body, max_d) {
            None => return Push::Rejected,
            Some(0) => return Push::NoMovement,
            Some(d) => d,
        };
        let take = d.min((MAX_HEIGHT as usize).saturating_sub(self.height));
        if take == 0 {
            return Push::Rejected;
        }
        self.fixed = Some((top, bot));
        let top_row = self.h - bot - d;
        self.chunks.push(Chunk {
            data: rgba[top_row * stride..].to_vec(),
            top_row,
            d: take,
            cut: d - take,
        });
        self.prev = Some(rows);
        self.height += take;
        Push::Added(take as u32)
    }

    /// The stitched RGBA image and its width and height (empty when nothing was pushed).
    pub fn finish(self) -> (Vec<u8>, u32, u32) {
        let Some(last) = self.chunks.last() else {
            return (Vec::new(), self.w as u32, 0);
        };
        let stride = self.w * 4;
        let bot = self.fixed.map_or(0, |f| f.1);
        let end = self.h - bot;
        let mut out = Vec::with_capacity(self.height * stride);
        for (i, c) in self.chunks.iter().enumerate() {
            let (from, to) = if i == 0 {
                (0, end)
            } else {
                (end - c.cut - c.d, end - c.cut)
            };
            out.extend_from_slice(&c.data[(from - c.top_row) * stride..(to - c.top_row) * stride]);
        }
        out.extend_from_slice(&last.data[(end - last.top_row) * stride..]);
        let height = (out.len() / stride) as u32;
        (out, self.w as u32, height)
    }
}

/// Synthetic pages and viewport frames for tests (here and in `scroll`).
#[cfg(test)]
pub(super) mod synth {
    pub struct Rng(u64);

    impl Rng {
        pub fn new(seed: u64) -> Self {
            Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
        }
        pub fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// Uniform in lo..hi.
        pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
            lo + (self.next_u64() % (hi - lo) as u64) as u32
        }
    }

    /// RGBA image.
    #[derive(Clone, PartialEq)]
    pub struct Img {
        pub w: usize,
        pub h: usize,
        pub px: Vec<u8>,
    }

    impl Img {
        pub fn white(w: usize, h: usize) -> Self {
            Img {
                w,
                h,
                px: vec![255; w * h * 4],
            }
        }
        pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: [u8; 3]) {
            for yy in y..(y + h).min(self.h) {
                for xx in x..(x + w).min(self.w) {
                    let i = (yy * self.w + xx) * 4;
                    self.px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                }
            }
        }
        fn rows(&self, y: usize, n: usize) -> &[u8] {
            &self.px[y * self.w * 4..(y + n) * self.w * 4]
        }
    }

    /// Tall page: text-like lines, textured images, blank stretches up to `max_blank` px.
    pub fn page(w: usize, h: usize, seed: u64, max_blank: u32) -> Img {
        let mut img = Img::white(w, h);
        let mut r = Rng::new(seed);
        let mut y = 10;
        while y < h {
            match r.range(0, 10) {
                0..=5 => {
                    for _ in 0..r.range(3, 10) {
                        let (mut x, gh) = (20, r.range(9, 13) as usize);
                        let g = r.range(10, 90) as u8;
                        while x + 40 < w {
                            let cw = r.range(3, 9) as usize;
                            if r.range(0, 6) > 0 {
                                img.fill(x, y + 12 - gh, cw, gh, [g, g, g + 20]);
                            }
                            x += cw + 2 + if r.range(0, 7) == 0 { 8 } else { 0 };
                        }
                        y += 22;
                    }
                }
                6..=7 => {
                    let ih = r.range(80, 220) as usize;
                    for cy in (0..ih).step_by(8) {
                        for cx in (0..w.saturating_sub(40)).step_by(8) {
                            let c = [
                                r.range(0, 256) as u8,
                                r.range(0, 256) as u8,
                                r.range(0, 256) as u8,
                            ];
                            img.fill(20 + cx, y + cy, 8, 8, c);
                        }
                    }
                    y += ih;
                }
                _ => y += r.range(20, max_blank.max(21)) as usize,
            }
            y += 14;
        }
        img
    }

    /// A page behind a viewport with sticky header (60 px) and footer (40 px).
    pub struct Rig {
        pub page: Img,
        header: Img,
        footer: Img,
        pub vh: usize,
    }

    impl Rig {
        pub fn new(w: usize, page_h: usize, vh: usize, seed: u64, max_blank: u32) -> Self {
            Rig {
                page: page(w, page_h, seed, max_blank),
                header: page(w, 60, seed + 1, 20),
                footer: page(w, 40, seed + 2, 20),
                vh,
            }
        }
        pub fn body_h(&self) -> usize {
            self.vh - 100
        }
        pub fn max_scroll(&self) -> usize {
            self.page.h - self.body_h()
        }
        fn compose(&self, s: usize, rows: usize) -> Img {
            let mut px = Vec::with_capacity((100 + rows) * self.page.w * 4);
            px.extend_from_slice(self.header.rows(0, 60));
            px.extend_from_slice(self.page.rows(s, rows));
            px.extend_from_slice(self.footer.rows(0, 40));
            Img {
                w: self.page.w,
                h: 100 + rows,
                px,
            }
        }
        /// Viewport frame at scroll position `s` (clamped to the end of the page).
        pub fn frame(&self, s: usize) -> Img {
            self.compose(s.min(self.max_scroll()), self.body_h())
        }
        /// Ground truth after scrolling to `s` (clamped): everything seen from 0.
        pub fn truth(&self, s: usize) -> Img {
            self.compose(0, s.min(self.max_scroll()) + self.body_h())
        }
        /// Cumulative clamped scroll positions for the requested steps, starting at 0.
        pub fn positions(&self, steps: &[usize]) -> Vec<usize> {
            let mut p = vec![0];
            for st in steps {
                p.push((p[p.len() - 1] + st).min(self.max_scroll()));
            }
            p
        }
    }
}

#[cfg(test)]
mod tests {
    use super::synth::{Img, Rig, Rng};
    use super::*;

    const W: usize = 780;
    const VH: usize = 700; // 60 header + 600 body + 40 footer

    fn stitch(frames: &[&Img]) -> (Vec<u8>, u32, u32) {
        let mut st = Stitcher::new(frames[0].w as u32, frames[0].h as u32);
        for f in frames {
            st.push(&f.px);
        }
        st.finish()
    }

    fn frames(rig: &Rig, steps: &[usize]) -> (Vec<Img>, usize) {
        let pos = rig.positions(steps);
        (
            pos.iter().map(|&s| rig.frame(s)).collect(),
            *pos.last().unwrap(),
        )
    }

    fn random_steps(seed: u64, lo: u32, hi: u32, n: usize) -> Vec<usize> {
        let mut r = Rng::new(seed);
        (0..n).map(|_| r.range(lo, hi) as usize).collect()
    }

    fn assert_exact(rig: &Rig, steps: &[usize]) {
        let (fr, last) = frames(rig, steps);
        let truth = rig.truth(last);
        assert!(truth.h > 5000, "test must cover most of the page");
        let (px, w, h) = stitch(&fr.iter().collect::<Vec<_>>());
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        assert!(px == truth.px, "stitched image differs from the page");
    }

    #[test]
    fn random_steps_up_to_90_percent() {
        for seed in 1..=4 {
            let rig = Rig::new(W, 6000, VH, seed, 40);
            assert_exact(&rig, &random_steps(seed, 60, 541, 30));
        }
    }

    #[test]
    fn long_blank_stretches() {
        // Blank runs up to 150 px; steps <= 70 % keep >= 180 rows of overlap.
        for seed in 10..=13 {
            let rig = Rig::new(W, 6000, VH, seed, 150);
            assert_exact(&rig, &random_steps(seed, 100, 421, 40));
        }
    }

    #[test]
    fn end_of_page_short_last_scroll() {
        let rig = Rig::new(W, 6000, VH, 21, 40);
        // Overshoots the end: last real move is shorter, then no movement.
        assert_exact(&rig, &[500; 12]);
    }

    #[test]
    fn no_movement_never_duplicates() {
        let rig = Rig::new(W, 6000, VH, 31, 40);
        let (fr, _) = frames(&rig, &[300, 300, 300]);
        let seq = [0, 0, 1, 1, 1, 2, 3, 3];
        let dup: Vec<&Img> = seq.iter().map(|&i| &fr[i]).collect();
        assert!(stitch(&dup) == stitch(&fr.iter().collect::<Vec<_>>()));
        assert!(stitch(&[&fr[0]]).0 == fr[0].px);
        assert!(stitch(&[&fr[2], &fr[2]]).0 == fr[2].px);
        let mut st = Stitcher::new(W as u32, VH as u32);
        assert_eq!(st.push(&fr[0].px), Push::Added(VH as u32));
        assert_eq!(st.push(&fr[0].px), Push::NoMovement);
        assert_eq!(st.push(&fr[1].px), Push::Added(300));
        assert_eq!(st.height(), VH as u32 + 300);
    }

    fn noise(f: &mut Img, r: &mut Rng) {
        for p in f.px.as_chunks_mut::<4>().0.iter_mut() {
            for c in &mut p[..3] {
                *c = (*c as i32 + r.range(0, 5) as i32 - 2).clamp(0, 255) as u8;
            }
        }
    }

    /// Number of RGB pixels in `a` that differ from `b` by more than 2 (same size).
    fn bad_pixels(a: &[u8], b: &[u8]) -> usize {
        a.as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0.iter())
            .filter(|(p, q)| p[..3].iter().zip(&q[..3]).any(|(x, y)| x.abs_diff(*y) > 2))
            .count()
    }

    #[test]
    fn noise_and_blinking_cursor() {
        let rig = Rig::new(W, 6000, VH, 41, 40);
        let (mut fr, last) = frames(&rig, &random_steps(41, 80, 500, 14));
        let mut r = Rng::new(7);
        for (i, f) in fr.iter_mut().enumerate() {
            noise(f, &mut r);
            if i % 2 == 1 {
                f.fill(100, 300, 2, 16, [0, 0, 0]); // cursor, fixed on screen
                f.fill(400, 200, 20, 20, [255, 0, 0]); // animated blob
            }
        }
        let (px, w, h) = stitch(&fr.iter().collect::<Vec<_>>());
        let truth = rig.truth(last);
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        // Only the cursor/blob pixels of odd frames may differ.
        let bad = bad_pixels(&px, &truth.px);
        assert!(bad < 7 * (32 + 400), "bad pixels: {bad}");
    }

    #[test]
    fn scrollbar_thumb_is_ignored() {
        let rig = Rig::new(W, 6000, VH, 51, 40);
        let steps = random_steps(51, 80, 500, 25);
        let (mut fr, last) = frames(&rig, &steps);
        for (f, &s) in fr.iter_mut().zip(&rig.positions(&steps)) {
            f.fill(W - 14, 60, 14, 600, [235, 235, 235]); // track
            f.fill(W - 14, 60 + s * 600 / 6000, 14, 60, [90, 90, 90]); // thumb
        }
        let (px, w, h) = stitch(&fr.iter().collect::<Vec<_>>());
        let truth = rig.truth(last);
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        // Everything left of the bar is exact (the bar column itself is copied as seen).
        for (a, b) in px
            .as_chunks::<{ W * 4 }>()
            .0
            .iter()
            .zip(truth.px.as_chunks::<{ W * 4 }>().0)
        {
            assert!(a[..(W - 14) * 4] == b[..(W - 14) * 4]);
        }
    }

    #[test]
    fn animated_region_does_not_break_matching() {
        let rig = Rig::new(W, 6000, VH, 61, 40);
        let (mut fr, last) = frames(&rig, &random_steps(61, 80, 500, 20));
        let mut r = Rng::new(9);
        let frames_n = fr.len();
        for f in &mut fr {
            // A 60x60 "video" (5 bins wide) with new random colours every frame.
            for (y, x) in (300..360).flat_map(|y| (200..260).map(move |x| (y, x))) {
                f.fill(
                    x,
                    y,
                    1,
                    1,
                    [r.range(0, 256) as u8, r.range(0, 256) as u8, 0],
                );
            }
        }
        let (px, w, h) = stitch(&fr.iter().collect::<Vec<_>>());
        let truth = rig.truth(last);
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        let bad = bad_pixels(&px, &truth.px);
        assert!(bad <= frames_n * 3600, "bad pixels: {bad}");
    }

    #[test]
    fn mostly_animated_frame_is_rejected_and_skipped() {
        let rig = Rig::new(W, 6000, VH, 71, 40);
        let (fr, _) = frames(&rig, &[300, 300]);
        let mut garbage = fr[1].clone();
        let mut r = Rng::new(3);
        for p in garbage.px.as_chunks_mut::<4>().0.iter_mut() {
            p[..3].copy_from_slice(&[r.range(0, 256) as u8, r.range(0, 256) as u8, 0]);
        }
        let mut st = Stitcher::new(W as u32, VH as u32);
        assert_eq!(st.push(&fr[0].px), Push::Added(VH as u32));
        assert_eq!(st.push(&garbage.px), Push::Rejected);
        assert_eq!(st.push(&fr[1].px), Push::Added(300));
        assert_eq!(st.push(&fr[2].px), Push::Added(300));
        assert_eq!(st.push(&[0; 10]), Push::Rejected); // wrong size
        let (px, _, h) = st.finish();
        assert_eq!(h as usize, rig.truth(600).h);
        assert!(px == rig.truth(600).px);
    }

    #[test]
    fn height_is_capped() {
        let (w, vh) = (100, 700);
        let rig = Rig::new(w, 36_000, vh, 81, 40);
        let mut st = Stitcher::new(w as u32, vh as u32);
        let mut s = 0;
        while !st.is_full() && s <= rig.max_scroll() {
            assert_ne!(st.push(&rig.frame(s).px), Push::Rejected);
            s += 500;
        }
        assert_eq!(st.height(), MAX_HEIGHT);
        assert_eq!(st.push(&rig.frame(s).px), Push::Rejected);
        let (px, fw, fh) = st.finish();
        assert_eq!((fw, fh), (w as u32, MAX_HEIGHT));
        // All rows above the glued-on footer are the page.
        let keep = (MAX_HEIGHT as usize - 40) * w * 4;
        assert!(px[..keep] == rig.truth(rig.max_scroll()).px[..keep]);
    }

    /// A PNG fixture (grey or RGB) as RGBA, with its size.
    fn real(png_bytes: &[u8]) -> (Vec<u8>, u32, u32) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Grayscale => buf.iter().flat_map(|&v| [v, v, v, 255]).collect(),
            png::ColorType::Rgb => (buf.as_chunks::<3>().0.iter())
                .flat_map(|&[r, g, b]| [r, g, b, 255])
                .collect(),
            other => panic!("fixture colour type {other:?}"),
        };
        (rgba, info.width, info.height)
    }

    /// Two real captures from the Windows VM (161 DPI, cropped), 3 wheel notches apart.
    fn real_pair(a: &[u8], b: &[u8]) -> (Push, Push) {
        let ((a, w, h), (b, ..)) = (real(a), real(b));
        let mut st = Stitcher::new(w, h);
        assert_eq!(st.push(&a), Push::Added(h));
        (st.push(&b), st.push(&b))
    }

    #[test]
    fn real_notepad_lines_that_differ_in_a_few_digits() {
        // "OpenCircle QA test document line 001: the quick brown fox ..." x 200:
        // a 64-bin luma signature saw every line as the same ("no movement").
        let (moved, again) = real_pair(
            include_bytes!("testdata/notepad161_0.png"),
            include_bytes!("testdata/notepad161_1.png"),
        );
        assert_eq!((moved, again), (Push::Added(261), Push::NoMovement)); // 9 lines x 29 px
        // The same at 96 DPI, where a digit is only ~7 px wide.
        let (moved, again) = real_pair(
            include_bytes!("testdata/notepad96_0.png"),
            include_bytes!("testdata/notepad96_1.png"),
        );
        assert_eq!((moved, again), (Push::Added(162), Push::NoMovement)); // 9 lines x 18 px
    }

    #[test]
    fn real_edge_sticky_header_and_floating_popup() {
        // Pastel paragraph bands under a sticky header, with Edge's "Restore pages"
        // popup floating over the top right, scrolled 300 CSS px at 167.7 %
        // (503 device px): the popup rows never matched, so the frame was rejected.
        let (moved, again) = real_pair(
            include_bytes!("testdata/edge161_0.png"),
            include_bytes!("testdata/edge161_1.png"),
        );
        assert_eq!((moved, again), (Push::Added(503), Push::NoMovement));
    }

    #[test]
    fn empty_stitcher_finishes_empty() {
        let (px, w, h) = Stitcher::new(10, 10).finish();
        assert!(px.is_empty() && w == 10 && h == 0);
    }
}
