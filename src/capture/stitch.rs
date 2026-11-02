//! Scrolling-capture stitcher (spike S5, production port).
//!
//! Feed it successive captures of one screen region while the content scrolls
//! down. Per frame: a colour signature per row in 4 px bins (right-hand scrollbar
//! strip left out), sticky header/footer = rows fixed in every moving pair,
//! scroll offset = lowest mean row cost over informative rows. Output pixels are always
//! copied from a real frame, never blended. Pure: no I/O, no platform code.

use super::shrink::Shrink;

/// Hard cap on the stitched height, in pixels.
pub const MAX_HEIGHT: u32 = 20_000;

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
/// Mean row cost (mismatched bins a row) under which a second offset, away
/// from the best, makes the match ambiguous.
const RIVAL: f64 = 0.05;
/// How far (px) a rival offset must be from the best to count: nearer ones
/// differ by a few rows over plain stretches, not by a section.
const RIVAL_GAP: usize = 8;
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

/// Scroll offset d (in `lo..=hi`) such that b body row y == a body row y+d:
/// the lowest mean row cost over b's evidence rows. Negative = scrolled back
/// up: tried too, so a page of near-identical lines (a document) scrolled
/// back is not taken for a step down by whole lines.
/// Evidence = informative rows that changed at their spot: a row that stayed
/// put (a sticky bar, even one that appeared later and so is not in the
/// fixed header) matches any offset whose overlap it fills, and so made a
/// short scroll back up read as a step down by nearly a screen (the owner's
/// README, 2026-11-02: sections twice, the limit reached).
/// `d: 0` also means "nothing changed that says anything, assume no
/// movement"; None = no trustworthy match (animated frame, too-large step).
/// Not `sure` when another offset, `RIVAL_GAP` away or more, with as much
/// evidence, matches about as well (repeated rows).
fn offset(a: &Rows, b: &Rows, t: usize, h: usize, (lo, hi): (isize, isize)) -> Option<Found> {
    let info: Vec<usize> = (0..h)
        .filter(|&y| b.info[t + y] && !same(a.row(t + y), b.row(t + y)))
        .collect();
    if info.is_empty() {
        return Some(Found { d: 0, sure: true });
    }
    let need = MIN_MATCH.min(info.len());
    let cost = |d: isize, y: usize| {
        let still = Some(a.row(t + y));
        bad(
            a.row(t + y.wrapping_add_signed(d)),
            b.row(t + y),
            still,
            b.cap,
        ) as u64
    };
    // b's evidence rows that a overlaps at d.
    let overlap = |d: isize| {
        let (lo, hi) = (d.min(0).unsigned_abs(), h - d.max(0) as usize);
        &info[info.partition_point(|&y| y < lo)..info.partition_point(|&y| y < hi)]
    };
    let (lo, hi) = (lo.max(1 - h as isize), hi.min(h as isize - 1));
    // Every 8th row first, to try the offsets best first: the full pass then
    // cuts the others short. Stable sort: on a tie 0 ("did not move") wins,
    // then down over up.
    let mut order: Vec<(u64, isize)> = (lo.max(0)..=hi)
        .chain((lo..=hi.min(-1)).rev())
        .filter_map(|d| {
            let rows = overlap(d);
            (rows.len() >= need).then(|| {
                let sample = rows.iter().step_by(8);
                let (c, n) = sample.fold((0, 0), |(c, n), &y| (c + cost(d, y), n + 1));
                (c * 1000 / n, d)
            })
        })
        .collect();
    order.sort_by_key(|&(c, _)| c);
    // (cost, rows, d): mean cost = cost / rows. On an equal mean the offset
    // with more evidence rows wins: a far step down overlaps only a few
    // rows, and a few rows match by chance.
    let mut best: Option<(u64, u64, isize)> = None;
    for &(_, d) in &order {
        let rows = overlap(d);
        let n = rows.len() as u64;
        let limit = ACCEPT_PCT * b.cap as u64 * n;
        let mut sum = 0u64;
        // Past what is accepted, or already worse than the best mean: give up on d.
        let beaten = rows.iter().any(|&y| {
            sum += cost(d, y);
            sum * 100 > limit || best.is_some_and(|(c, r, _)| sum * r > c * n)
        });
        let better = best.is_none_or(|(c, r, _)| sum * r < c * n || 4 * n > 5 * r);
        if !beaten && better {
            best = Some((sum, n, d));
        }
    }
    let (c, r, found) = best?;
    // A rival: an offset away from it, with as much evidence, whose mean
    // is within reach of it: no telling them apart (repeated rows).
    let reach = 1.2 * c as f64 / r as f64 + RIVAL;
    let rival = order.iter().any(|&(_, d)| {
        let rows = overlap(d);
        let limit = reach * rows.len() as f64;
        let mut sum = 0u64;
        d.abs_diff(found) > RIVAL_GAP
            && rows.len() as u64 >= r
            && !rows.iter().any(|&y| {
                sum += cost(d, y);
                sum as f64 > limit
            })
    });
    Some(Found {
        d: found,
        sure: !rival,
    })
}

/// An offset `offset` found; not `sure` when another one, at least
/// `RIVAL_GAP` away, matches about as well.
#[derive(Debug, Clone, Copy)]
struct Found {
    d: isize,
    sure: bool,
}

impl Found {
    /// Its offset, unless a rival makes it a guess at a real step down (back up
    /// adds nothing; a small step down cannot stitch a section twice).
    fn trusted(self) -> Option<isize> {
        (self.sure || self.d.unsigned_abs() <= RIVAL_GAP).then_some(self.d)
    }
}

/// The preview's shown width narrows in steps of this share (`whole`).
const STEP: f64 = 0.02;

/// `w0` narrowed by `i` steps.
fn width_at(w0: usize, i: u32) -> usize {
    ((w0 as f64 * (1.0 - STEP).powi(i as i32)).round() as usize).max(1)
}

/// `Stitcher::whole`'s state: the page at the widest preview width (`base`,
/// fed `fed` page rows, its finished rows `small`), and that scaled to the
/// shown width (`mini`: scaler, `small` rows fed, finished rows) at `step`.
struct Preview {
    /// The page's width.
    w: usize,
    base: Shrink,
    fed: usize,
    small: Vec<u8>,
    step: u32,
    mini: Option<(Shrink, usize, Vec<u8>)>,
}

impl Preview {
    fn new(w: usize, w0: usize) -> Self {
        Preview {
            w,
            base: Shrink::new(w, w0),
            fed: 0,
            small: Vec::new(),
            step: 0,
            mini: None,
        }
    }

    /// `page` (final rows when `settled`) then `foot`, at most `max_h` tall
    /// (`Stitcher::whole`).
    fn show(
        &mut self,
        page: &[u8],
        settled: bool,
        foot: &[u8],
        max_h: usize,
    ) -> (Vec<u8>, u32, u32) {
        let stride = self.w * 4;
        let w0 = self.base.out_w();
        if settled {
            for row in page[self.fed * stride..].chunks_exact(stride) {
                self.base.feed(row, &mut self.small);
            }
            self.fed = page.len() / stride;
        }
        let mut rest = self.base.clone();
        let mut end = Vec::new();
        let unsettled = page[self.fed * stride..].chunks_exact(stride);
        for row in unsettled.chain(foot.chunks_exact(stride)) {
            rest.feed(row, &mut end);
        }
        rest.flush(&mut end);
        // the widest step that fits the height: w0 * (1 - STEP)^i
        let total = (page.len() + foot.len()) / stride;
        let fits = |w1: usize| (total * w1).div_ceil(self.w) <= max_h;
        let mut i = self.step;
        while i > 0 && fits(width_at(w0, i - 1)) {
            i -= 1;
        }
        while width_at(w0, i) > 1 && !fits(width_at(w0, i)) {
            i += 1;
        }
        let w1 = width_at(w0, i);
        let (row0, row1) = (w0 * 4, w1 * 4);
        if w1 == w0 {
            self.step = 0;
            let mut out = self.small.clone();
            out.extend_from_slice(&end);
            let h = (out.len() / row0).min(max_h);
            out.truncate(h * row0);
            return (out, w0 as u32, h as u32);
        }
        if self.step != i || self.mini.is_none() {
            self.mini = Some((Shrink::new(w0, w1), 0, Vec::new()));
            self.step = i;
        }
        let Some((mini, fed, small)) = &mut self.mini else {
            return (Vec::new(), 0, 0);
        };
        for row in self.small[*fed * row0..].chunks_exact(row0) {
            mini.feed(row, small);
        }
        *fed = self.small.len() / row0;
        let mut rest = mini.clone();
        let mut out = small.clone();
        for row in end.chunks_exact(row0) {
            rest.feed(row, &mut out);
        }
        rest.flush(&mut out);
        let h = (out.len() / row1).min(max_h);
        out.truncate(h * row1);
        (out, w1 as u32, h as u32)
    }
}

/// Result of [`Stitcher::push`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// The frame scrolled by this many new rows, now part of the image (the first frame: its height).
    Added(u32),
    /// Same content as the last accepted frame (the page did not move), or
    /// part of it (scrolled back up): nothing new.
    NoMovement,
    /// Wrong size, no trustworthy match (animated or scrolled too far) or the height cap is reached.
    /// The frame is dropped and the next one is compared with the last accepted frame.
    Rejected,
}

/// Incremental stitcher: `push` frames, then `finish`. Keeps only the image so
/// far (`page`, down to the body's bottom in the last frame) and the last
/// frame's footer (`foot`): a frame is freed once pushed.
pub struct Stitcher {
    w: usize,
    h: usize,
    prev: Option<Rows>,
    /// Where the last frame placed is from `prev` (its offset; 0 or less).
    seen: isize,
    /// Sticky (header, footer) rows, the minimum over every accepted moving pair.
    fixed: Option<(usize, usize)>,
    page: Vec<u8>,
    foot: Vec<u8>,
    /// The height cap (`MAX_HEIGHT` unless `limit` lowers it).
    max: u32,
    /// The preview's scalers and what they made so far (`whole`).
    preview: Option<Preview>,
}

impl Stitcher {
    /// `width` x `height` is the size of every frame that will be pushed.
    pub fn new(width: u32, height: u32) -> Self {
        Stitcher {
            w: width as usize,
            h: height as usize,
            prev: None,
            seen: 0,
            fixed: None,
            page: Vec::new(),
            foot: Vec::new(),
            max: MAX_HEIGHT,
            preview: None,
        }
    }

    /// Stops growing at `max` px (at most `MAX_HEIGHT`; Settings > Snip).
    pub fn limit(mut self, max: u32) -> Self {
        self.max = max.clamp(1, MAX_HEIGHT);
        self
    }

    /// Height of the image `finish` would return (0 before the first frame).
    pub fn height(&self) -> u32 {
        ((self.page.len() + self.foot.len()) / (self.w * 4).max(1)) as u32
    }

    /// True once the height cap is reached; further frames are rejected.
    pub fn is_full(&self) -> bool {
        self.height() >= self.max
    }

    pub fn push(&mut self, rgba: &[u8]) -> Push {
        let stride = self.w * 4;
        if self.w == 0 || self.h == 0 || rgba.len() != stride * self.h {
            return Push::Rejected;
        }
        let rows = rows(rgba, self.w, self.h);
        let Some(prev) = &self.prev else {
            self.page = rgba.to_vec();
            self.prev = Some(rows);
            return Push::Added(self.h as u32);
        };
        let Some((t, b)) = fixed_rows(prev, &rows) else {
            self.seen = 0;
            return Push::NoMovement;
        };
        // Tentative: only kept when the pair turns out to be a real scroll.
        let (top, bot) = self.fixed.map_or((t, b), |(ft, fb)| (ft.min(t), fb.min(b)));
        // matched on this pair's own body: rows fixed in both frames (a
        // sticky bar that appeared after the first pairs) say nothing, and
        // a's bar must not be compared with b's page (a scroll back up
        // under it read as a step down)
        let body = self.h - t - b;
        let max_d = (body * MAX_STEP_PCT / 100) as isize;
        // Where the page was last seen from `prev` (back up: 0 or less). Once
        // it went back up, a step down from there that overlaps less than
        // half of what changed is taken only within half an area: a "step
        // down" from far above `prev` to below it on a few rows is a
        // lookalike (scrolled back up further than the bar or the search
        // lets it match, then a section matched one repeated below it).
        let seen = self.seen;
        let d = offset(prev, &rows, t, body, (-max_d, max_d))
            .filter(|f| f.d <= 0 || seen == 0 || f.d.abs_diff(seen) <= max_d as usize / 2)
            .and_then(Found::trusted);
        let d = match d {
            // (the last frame placed stays the one to follow from)
            None => return Push::Rejected,
            // scrolled back up: what it shows is in the image already
            Some(d) if d <= 0 => {
                self.seen = d;
                return Push::NoMovement;
            }
            Some(d) => d as usize,
        };
        if self.is_full() {
            return Push::Rejected;
        }
        // The page ends at the last frame's body bottom (`old` footer rows
        // above its end); in this frame that row is `d` higher. A footer found
        // smaller now (`bot` < `old`) takes the rows it gave back as well, so
        // the page has no gap. The first pair: the first frame's footer goes.
        let old = match self.fixed {
            Some((_, fb)) => fb,
            None => {
                self.page.truncate(self.page.len() - bot * stride);
                bot
            }
        };
        // ponytail: a footer shrinking by more than the step reaches into the
        // header (`top`); clamped there, a few rows may go missing once.
        let start = (self.h - old).saturating_sub(d).max(top);
        let end = self.h - bot;
        let room = (self.max as usize).saturating_sub(self.page.len() / stride + bot);
        let take = (end - start).min(room);
        self.fixed = Some((top, bot));
        self.page
            .extend_from_slice(&rgba[start * stride..(start + take) * stride]);
        self.foot = rgba[end * stride..].to_vec();
        self.prev = Some(rows);
        self.seen = 0;
        Push::Added(take as u32)
    }

    /// The whole image so far, miniature (owner, 2026-11-02, as iTop): scaled
    /// by min(`max_w` / width, `max_h` / height), never taller than `max_h`, so it
    /// starts `max_w` wide and narrows as the page grows; sharp (area average,
    /// `shrink`). Two stages, both fed only the rows added since the last call:
    /// the page at `max_w` (`base`, kept whole), then that at the shown width
    /// (`mini`). The shown width moves in steps of `STEP` (2 %), so `mini` is
    /// rebuilt from `base` once per 2 % of growth, not every call; the
    /// footer and the partial last rows are scaled on copies (they change).
    /// Empty before the first frame.
    pub fn whole(&mut self, max_w: u32, max_h: u32) -> (Vec<u8>, u32, u32) {
        if self.height() == 0 || max_w == 0 || max_h == 0 {
            return (Vec::new(), 0, 0);
        }
        let (w, w0) = (self.w, (max_w as usize).min(self.w));
        let p = self.preview.get_or_insert_with(|| Preview::new(w, w0));
        if p.base.out_w() != w0 {
            *p = Preview::new(w, w0);
        }
        // page rows are final once the first pair is matched (before it, the
        // first frame's footer may still be cut off the page)
        p.show(&self.page, self.fixed.is_some(), &self.foot, max_h as usize)
    }

    /// The stitched RGBA image and its width and height (empty when nothing was pushed).
    pub fn finish(mut self) -> (Vec<u8>, u32, u32) {
        let height = self.height();
        self.page.append(&mut self.foot);
        (self.page, self.w as u32, height)
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

    /// A chat: bubbles left (grey) and right (blue), a few text lines each,
    /// on a plain background, small gaps between them.
    pub fn chat(w: usize, h: usize, seed: u64) -> Img {
        let mut img = Img::white(w, h);
        img.fill(0, 0, w, h, [236, 240, 245]);
        let mut r = Rng::new(seed);
        let mut y = 12;
        while y + 80 < h {
            let mine = r.range(0, 2) == 0;
            let lines = r.range(1, 5) as usize;
            let bw = r.range(160, (w as u32 * 2 / 3).max(161)) as usize;
            let bh = 16 + 20 * lines;
            let x = if mine { w - 24 - bw } else { 24 };
            let (bg, ink) = match mine {
                true => ([37, 99, 235], [235, 240, 255]),
                false => ([255, 255, 255], [40, 44, 52]),
            };
            img.fill(x, y, bw, bh, bg);
            for l in 0..lines {
                let mut cx = x + 12;
                while cx + 20 < x + bw {
                    let cw = r.range(3, 9) as usize;
                    if r.range(0, 6) > 0 {
                        img.fill(cx, y + 12 + 20 * l, cw, 10, ink);
                    }
                    cx += cw + 2;
                }
            }
            y += bh + r.range(6, 30) as usize;
        }
        img
    }

    /// A document of `h / 22` lines that are the same text but for their
    /// 3-digit number ("line 001: the quick brown fox ...").
    pub fn document(w: usize, h: usize) -> Img {
        let mut img = Img::white(w, h);
        let mut r = Rng::new(99);
        let text: Vec<(usize, usize)> = (0..60).map(|_| (r.range(3, 9) as usize, 2)).collect();
        for (n, y) in (0..h / 22).map(|n| (n, n * 22 + 6)) {
            // the number: one glyph per digit, its shape set by the digit
            for (i, digit) in [n / 100, n / 10 % 10, n % 10].into_iter().enumerate() {
                let g = 30 + digit as u8 * 20;
                img.fill(20 + i * 9, y + digit % 4, 7, 10 - digit % 4, [g, g, g]);
            }
            let mut x = 60;
            for &(cw, gap) in &text {
                img.fill(x, y, cw, 10, [40, 40, 40]);
                x += cw + gap + 1;
                if x + 20 > w {
                    break;
                }
            }
        }
        img
    }

    /// A README at `scale` (device px per CSS px): headings, paragraphs, bullet
    /// lists, hairline rules and grey code blocks whose lines repeat (`cargo
    /// build` three times, a table of near-identical rows): many rows look alike.
    pub fn readme(w: usize, h: usize, scale: f32, seed: u64) -> Img {
        let mut img = Img::white(w, h);
        let mut r = Rng::new(seed);
        let px = |v: f32| (v * scale).round() as usize;
        let line = px(24.0);
        // a few fixed "words" so code lines repeat exactly
        let words: Vec<Vec<usize>> = (0..6)
            .map(|_| {
                (0..r.range(4, 9))
                    .map(|_| px(r.range(5, 9) as f32))
                    .collect()
            })
            .collect();
        let glyphs = |img: &mut Img, x0: usize, y: usize, word: &[usize], ink: [u8; 3]| {
            let mut x = x0;
            for &cw in word {
                img.fill(x, y, cw, px(11.0), ink);
                x += cw + px(2.0);
            }
            x
        };
        let mut y = px(16.0);
        while y < h {
            match r.range(0, 10) {
                // heading + hairline under it
                0..=1 => {
                    let mut x = px(16.0);
                    for _ in 0..r.range(1, 4) {
                        let cw = px(r.range(40, 120) as f32);
                        img.fill(x, y, cw, px(18.0), [31, 35, 40]);
                        x += cw + px(10.0);
                    }
                    y += px(30.0);
                    img.fill(px(16.0), y, w - px(32.0), 1.max(px(1.0)), [208, 215, 222]);
                    y += px(12.0);
                }
                // paragraph
                2..=4 => {
                    for _ in 0..r.range(2, 6) {
                        let mut x = px(16.0);
                        while x + px(60.0) < w {
                            let wd = &words[r.range(0, 6) as usize];
                            x = glyphs(&mut img, x, y, wd, [31, 35, 40]) + px(6.0);
                        }
                        y += line;
                    }
                }
                // bullet list
                5..=6 => {
                    for _ in 0..r.range(2, 6) {
                        img.fill(px(24.0), y + px(4.0), px(4.0), px(4.0), [31, 35, 40]);
                        let n = r.range(1, 4);
                        let mut x = px(40.0);
                        for _ in 0..n {
                            x = glyphs(
                                &mut img,
                                x,
                                y,
                                &words[r.range(0, 6) as usize],
                                [9, 105, 218],
                            ) + px(6.0);
                        }
                        y += line;
                    }
                }
                // code block: grey box, repeated lines
                _ => {
                    let n = r.range(2, 9) as usize;
                    let bh = n * px(20.0) + px(32.0);
                    img.fill(px(16.0), y, w - px(32.0), bh, [246, 248, 250]);
                    let same = r.range(0, 2) == 0;
                    for i in 0..n {
                        let wd = if same { &words[0] } else { &words[i % 3] };
                        let ly = y + px(16.0) + i * px(20.0);
                        let x = glyphs(&mut img, px(32.0), ly, &words[5], [130, 80, 223]);
                        glyphs(&mut img, x + px(7.0), ly, wd, [31, 35, 40]);
                    }
                    y += bh;
                }
            }
            y += px(16.0);
        }
        img
    }

    /// A browser over `page`: a `vh` px tall viewport, optionally a sticky bar
    /// on top (`head` px, from the start or only once scrolled past `head_from`)
    /// and a sticky side column (`toc` px wide on the right, its own content,
    /// fixed); positions come from `smooth`.
    pub struct Browser {
        pub page: Img,
        pub vh: usize,
        pub head: usize,
        pub head_from: usize,
        pub toc: usize,
        bar: Img,
        side: Img,
    }

    impl Browser {
        pub fn new(page: Img, vh: usize, head: usize, head_from: usize, toc: usize) -> Self {
            let w = page.w;
            let mut bar = Img::white(w, head.max(1));
            bar.fill(0, 0, w, head, [246, 248, 250]);
            bar.fill(12, head / 3, 90, head / 3, [31, 35, 40]);
            bar.fill(w / 2, head / 3, 60, head / 3, [9, 105, 218]);
            bar.fill(0, head.saturating_sub(1), w, 1, [208, 215, 222]);
            let side = match toc {
                0 => Img::white(1, 1),
                _ => super::synth::readme(toc, vh, 1.0, 77),
            };
            Browser {
                page,
                vh,
                head,
                head_from,
                toc,
                bar,
                side,
            }
        }
        pub fn max_scroll(&self) -> usize {
            self.page.h - self.vh
        }
        pub fn frame(&self, s: usize) -> Img {
            let s = s.min(self.max_scroll());
            let w = self.page.w;
            let mut f = Img {
                w,
                h: self.vh,
                px: self.page.rows(s, self.vh).to_vec(),
            };
            if self.head > 0 && s >= self.head_from {
                f.px[..self.head * w * 4].copy_from_slice(self.bar.rows(0, self.head));
            }
            if self.toc > 0 {
                let x0 = w - self.toc;
                for y in 0..self.vh {
                    let row = &self.side.px[y * self.toc * 4..(y + 1) * self.toc * 4];
                    f.px[(y * w + x0) * 4..(y * w + w) * 4].copy_from_slice(row);
                }
            }
            f
        }
    }

    /// Smooth scrolling as Chrome / Edge do it, grabbed every ~66 ms: each
    /// wheel notch (`notch` px) adds to the target, the position eases there
    /// over ~150 ms (frames land mid-way); notches `every` ms apart in bursts
    /// of `burst`, a pause between bursts; every `back`-th notch goes back up
    /// (0: never). Integer device px positions.
    pub fn smooth(
        notch: f32,
        every: u32,
        burst: u32,
        back: u32,
        until: usize,
        seed: u64,
    ) -> Vec<usize> {
        let mut r = Rng::new(seed);
        let (mut t, mut target, mut from, mut since) = (0u32, 0f32, 0f32, 0u32);
        let mut pos = vec![0usize];
        let mut next_notch = 100u32;
        let (mut in_burst, mut notches) = (0, 0);
        let mut at = 0f32;
        while (at as usize) < until && pos.len() < 4000 {
            let step = 60 + r.range(0, 13);
            for _ in 0..step {
                t += 1;
                if t >= next_notch {
                    from = at;
                    notches += 1;
                    target = match back > 0 && notches % back == 0 {
                        true => (target - notch * r.range(1, 4) as f32).max(0.0),
                        false => target + notch,
                    };
                    since = 0;
                    in_burst += 1;
                    next_notch = t + if in_burst >= burst {
                        in_burst = 0;
                        every * 3 + r.range(0, 200)
                    } else {
                        every
                    };
                }
                since += 1;
                let k = (since as f32 / 150.0).min(1.0);
                at = from + (target - from) * (1.0 - (1.0 - k).powi(3));
            }
            pos.push(at.max(0.0).round() as usize);
        }
        pos
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
        /// `page` behind the viewport, with a sticky header and footer.
        pub fn with(page: Img, vh: usize, seed: u64) -> Self {
            let w = page.w;
            Rig {
                page,
                header: super::synth::page(w, 60, seed + 1, 20),
                footer: super::synth::page(w, 40, seed + 2, 20),
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

    /// Stitches the frames at scroll positions `at` (any order, repeats).
    fn stitch_at(rig: &Rig, at: &[usize]) -> (Vec<u8>, u32, u32) {
        let fr: Vec<Img> = at.iter().map(|&s| rig.frame(s)).collect();
        stitch(&fr.iter().collect::<Vec<_>>())
    }

    /// By hand at ~15 fps: small steps, many frames that did not move.
    fn by_hand(seed: u64, n: usize) -> Vec<usize> {
        let mut r = Rng::new(seed);
        (0..n)
            .map(|_| match r.range(0, 3) {
                0 => 0,
                _ => r.range(1, 120) as usize,
            })
            .collect()
    }

    #[test]
    fn small_steps_by_hand_have_no_seams() {
        for seed in 1..=3 {
            // long blank runs: a footer first guessed too tall
            let rig = Rig::new(W, 6000, VH, seed, 150);
            assert_exact(&rig, &by_hand(seed, 160));
        }
    }

    #[test]
    fn scrolling_back_up_never_duplicates() {
        let rig = Rig::new(W, 6000, VH, 5, 40);
        // down, back up (twice, past the start), down again past where it was
        let mut at = vec![0, 100, 200, 300, 400, 500];
        at.extend([450, 380, 300, 200, 100, 0, 0, 60, 120]);
        at.extend((0..60).map(|i| 180 + i * 90));
        let (px, w, h) = stitch_at(&rig, &at);
        let truth = rig.truth(*at.last().unwrap());
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        assert!(px == truth.px, "scrolling back left a repeat or a gap");
    }

    #[test]
    fn a_chat_with_its_title_and_composer_is_seamless() {
        let rig = Rig::with(synth::chat(W, 7000, 3), VH, 3);
        assert_exact(&rig, &by_hand(8, 200));
        assert_exact(&rig, &random_steps(8, 60, 500, 30));
    }

    #[test]
    fn a_document_of_near_identical_lines_is_seamless() {
        // Only the line numbers differ: every other row matches 22 px away.
        let rig = Rig::with(synth::document(W, 6600), VH, 4);
        assert_exact(&rig, &by_hand(9, 200));
        let mut at = vec![0, 66, 132, 88, 44, 132, 198];
        at.extend((0..60).map(|i| 264 + i * 110));
        let (px, _, h) = stitch_at(&rig, &at);
        let truth = rig.truth(*at.last().unwrap());
        assert_eq!(h as usize, truth.h);
        assert!(px == truth.px);
    }

    #[test]
    fn only_the_image_so_far_is_kept() {
        // 10,000 px of a 780 px wide page by hand: one copy of it plus one
        // frame, never a copy per frame (frames are freed once stitched).
        let rig = Rig::new(W, 10_100, VH, 6, 40);
        let mut st = Stitcher::new(W as u32, VH as u32);
        let mut s = 0;
        while s < rig.max_scroll() {
            st.push(&rig.frame(s).px);
            s += 37;
        }
        let image = st.height() as usize * W * 4;
        let kept = st.page.capacity() + st.foot.capacity();
        assert!(st.height() > 10_000);
        assert!(
            kept <= 2 * image,
            "{kept} bytes kept for a {image} byte image"
        );
    }

    /// The preview is the WHOLE page so far, as iTop's (owner, 2026-11-02):
    /// `max_w` wide while it fits, then narrower as the page grows, never
    /// taller than `max_h` and filling it once tall. The owner's numbers: a
    /// 1720 px wide README on a 1440 px screen, iTop ~400 -> 330 -> 190 px
    /// wide; ours (280 px at most, 80 % of the free height: 1112) ~280 -> 250
    /// -> 150 at the same lengths (5,000 / 7,245 / 12,583 px).
    #[test]
    fn the_preview_is_the_whole_page_narrowing_as_it_grows() {
        let w = 1720;
        let page = synth::readme(w, 13_000, 1.0, 2);
        let mut p = Preview::new(w, 280);
        let mut widths = Vec::new();
        for h in (1440..=12_583).step_by(331).chain([12_583]) {
            let (px, pw, ph) = p.show(&page.px[..h * w * 4], true, &[], 1112);
            assert_eq!(px.len(), (pw * ph * 4) as usize);
            assert!(ph <= 1112);
            // all of it: as tall as the page at that width (give or take a row)
            let want = h as f64 * f64::from(pw) / w as f64;
            assert!((f64::from(ph) - want).abs() <= 2.0, "{h}: {ph} vs {want}");
            if pw < 280 {
                assert!(f64::from(ph) >= 0.97 * 1112.0, "fills the height: {ph}");
            }
            widths.push((h, pw));
        }
        assert!(widths.windows(2).all(|p| p[1].1 <= p[0].1), "{widths:?}");
        let at = |len: usize| widths.iter().find(|(h, _)| *h >= len).unwrap().1;
        assert_eq!(at(5000), 280, "{widths:?}");
        assert!((245..=265).contains(&at(7245)), "{widths:?}");
        assert!((145..=153).contains(&at(12_583)), "{widths:?}");
    }

    /// Fed a little at a time (each call scales only what is new, plus a
    /// rebuild per 2 % step) it is what one call at the end gives, and that
    /// is the exact area average of the finished image, give or take.
    #[test]
    fn the_preview_built_bit_by_bit_is_the_one_built_at_once() {
        let rig = Rig::new(W, 6000, VH, 7, 40);
        let mut st = Stitcher::new(W as u32, VH as u32);
        let mut once = Stitcher::new(W as u32, VH as u32);
        assert_eq!(st.whole(300, 900).1, 0, "nothing yet");
        let mut s = 0;
        while s < 5000 {
            st.push(&rig.frame(s).px);
            once.push(&rig.frame(s).px);
            let _ = st.whole(300, 900);
            s += 250;
        }
        let (px, pw, ph) = st.whole(300, 900);
        assert!(pw < 300 && ph > 870, "{pw} x {ph}");
        assert!(px == once.whole(300, 900).0);
        let (rgba, ..) = once.finish();
        let (all, _) = super::super::shrink::shrink(&rgba, W, pw as usize);
        let n = px.len().min(all.len());
        let diff: u64 = (px[..n].iter().zip(&all[..n]))
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        assert!(diff / n as u64 <= 3, "mean difference {}", diff / n as u64);
        // a short image comes whole, at full width
        let mut st = Stitcher::new(W as u32, VH as u32);
        st.push(&rig.frame(0).px);
        let (_, pw, ph) = st.whole(390, 5000);
        assert_eq!((pw, ph), (390, 350));
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
        // (300 new rows, and the rows a footer guessed too tall gives back)
        assert!(matches!(st.push(&fr[2].px), Push::Added(300..)));
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

    /// Settings > Snip's long-capture limit: the image stops at that height.
    #[test]
    fn a_lower_limit_caps_the_height_there() {
        let (w, vh) = (100, 700);
        let rig = Rig::new(w, 36_000, vh, 81, 40);
        let mut st = Stitcher::new(w as u32, vh as u32).limit(5_000);
        let mut s = 0;
        while !st.is_full() && s <= rig.max_scroll() {
            assert_ne!(st.push(&rig.frame(s).px), Push::Rejected);
            s += 500;
        }
        assert_eq!(st.height(), 5_000);
        assert_eq!(st.push(&rig.frame(s).px), Push::Rejected);
        assert_eq!(st.finish().2, 5_000);
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

    /// Smooth-scrolled README behind a browser: (stitched height, true
    /// height, rows of the page column that differ from the truth).
    fn browse(b: &synth::Browser, pos: &[usize]) -> (usize, usize, usize) {
        let mut st = Stitcher::new(b.page.w as u32, b.vh as u32);
        for &s in pos {
            st.push(&b.frame(s).px);
        }
        let last = pos.iter().copied().max().unwrap().min(b.max_scroll());
        let (px, w, h) = st.finish();
        let w = w as usize;
        let mut truth = b.page.px[..(last + b.vh) * w * 4].to_vec();
        if b.head > 0 && b.head_from == 0 {
            truth[..b.head * w * 4].copy_from_slice(&b.frame(0).px[..b.head * w * 4]);
        }
        let cols = (w - b.toc - scrollbar_strip(w)) * 4;
        let rows = (h as usize).min(last + b.vh);
        let bad = (0..rows)
            .filter(|&y| px[y * w * 4..y * w * 4 + cols] != truth[y * w * 4..y * w * 4 + cols])
            .count();
        (h as usize, last + b.vh, bad)
    }

    /// A README in a browser, scrolled smoothly (Chrome / Edge: frames land
    /// mid-animation) at 100 %, 150 % and 250 %: slow and fast wheel, down
    /// and back up, a touchpad's small steps; a sticky bar that shows only
    /// once scrolled (GitHub's) and a sticky side column. The owner's README
    /// (2026-11-02) came out with sections twice and hit the 20,000 px limit:
    /// here every case is exact, as tall as the page, no row repeated.
    #[test]
    fn a_smoothly_scrolled_readme_is_exact_at_every_scale() {
        for scale in [1.0f32, 1.5, 2.5] {
            let px = |v: f32| (v * scale) as usize;
            let page = synth::readme(px(260.0), px(1800.0), scale, 5);
            let browsers = [
                // a sticky bar from the start
                synth::Browser::new(page.clone(), px(320.0), px(40.0), 0, 0),
                // one that shows once scrolled past 300 px, and a side column
                synth::Browser::new(page, px(320.0), px(40.0), px(300.0), px(60.0)),
            ];
            for b in &browsers {
                for (speed, notch, every, burst, back) in [
                    ("slow", 100.0, 400, 1, 0),
                    ("fast", 60.0, 30, 3, 0),
                    ("down and up", 100.0, 120, 3, 4),
                    ("touchpad", 12.0, 16, 30, 7),
                ] {
                    let until = b.max_scroll() + 50;
                    let pos = synth::smooth(notch * scale, every, burst, back, until, 3);
                    let (h, truth, bad) = browse(b, &pos);
                    let case = format!("{scale}x {speed}, bar from {}", b.head_from);
                    assert_eq!((h, bad), (truth, 0), "{case}: {h} px, {bad} rows wrong");
                }
            }
        }
    }

    #[test]
    fn empty_stitcher_finishes_empty() {
        let (px, w, h) = Stitcher::new(10, 10).finish();
        assert!(px.is_empty() && w == 10 && h == 0);
    }
}
