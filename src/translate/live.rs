//! Live translate, pure logic (no window, no capture): each new frame is
//! diffed against the last one in tiles; only the changed regions are read
//! again (their crops stacked into ONE OCR call), unchanged text keeps its
//! translation from the [`Cache`], and the layer gets [`Patch`]es to show.
//!
//! Our own layer is never read back: the platform excludes it from capture
//! (`crate::live`, Windows), and as a second guard (X11 can't exclude) a
//! patch region that shows our last patch is put back to what was under it
//! before the diff.

use std::borrow::Cow;
use std::hash::{DefaultHasher, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::pipeline::{
    Cache, Deps, Error, Timings, check_frame, cut_off, is_rtl, to_line, translate_blocks,
};
#[cfg(test)]
use super::pipeline::{Translation, translations};
use super::{Block, Drawn, Line, group_blocks, render};
use crate::ocr::TextBlock;

/// Diff granularity in pixels.
const TILE: u32 = 32;
/// Rows between stacked crops in the OCR image.
const GAP: u32 = 16;

/// What the layer draws: `rgba` (w x h x 4) at `bounds` (x, y, w, h, frame pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub bounds: [u32; 4],
    pub rgba: Vec<u8>,
    /// The frame's pixels under the patch, for the read-back guard.
    under: Vec<u8>,
}

/// Live-mode state between frames. One per live session.
#[derive(Default)]
pub struct Live {
    size: (u32, u32),
    hashes: Vec<u64>,
    lines: Vec<Line>,
    #[cfg(test)]
    blocks: Vec<Translation>,
    patches: Vec<Patch>,
    /// What the last render drew and left as it was (its rooms dropped).
    drawn: Drawn,
}

/// x0, y0, x1, y1 in frame pixels.
type Rect = [u32; 4];

impl Live {
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything the layer shows now (all blocks, not just the changed ones).
    pub fn patches(&self) -> &[Patch] {
        &self.patches
    }

    /// What the last changed frame's render drew and left as it was.
    pub fn drawn(&self) -> &Drawn {
        &self.drawn
    }

    /// The blocks on screen, for "click a block -> explanation".
    #[cfg(test)]
    pub fn blocks(&self) -> &[Translation] {
        &self.blocks
    }

    /// Feeds one captured frame. `Ok(None)`: nothing changed, keep the layer.
    /// `Ok(Some)`: redraw the layer from [`Live::patches`]. On an error the
    /// state is kept, so the next frame retries the same regions.
    #[cfg(test)]
    pub fn step(
        &mut self,
        frame: &[u8],
        width: u32,
        height: u32,
        target: &str,
        cache: &Cache,
        deps: &impl Deps,
    ) -> Result<Option<Timings>, Error> {
        self.step_hiding(frame, width, height, target, cache, deps, |_| {})
    }

    /// [`Live::step`], and before the slow part (OCR, translation): where the
    /// text under a patch changed, the patch goes at once and `hide` gets the
    /// ones left, so the layer never shows a stale translation over new text
    /// while the new one is read (#56).
    #[allow(clippy::too_many_arguments)]
    pub fn step_hiding(
        &mut self,
        frame: &[u8],
        width: u32,
        height: u32,
        target: &str,
        cache: &Cache,
        deps: &impl Deps,
        hide: impl FnOnce(&[Patch]),
    ) -> Result<Option<Timings>, Error> {
        let start = Instant::now();
        check_frame(frame, width, height)?;
        let fresh = self.size != (width, height);
        let mut frame = Cow::Borrowed(frame);
        if !fresh {
            self.unlayer(&mut frame, width);
        }
        let hashes = tile_hashes(&frame, width, height);
        let changed: Vec<bool> = if fresh {
            vec![true; hashes.len()]
        } else {
            hashes
                .iter()
                .zip(&self.hashes)
                .map(|(a, b)| a != b)
                .collect()
        };
        if !changed.contains(&true) {
            return Ok(None);
        }
        let tw = width.div_ceil(TILE);
        let stale = |p: &Patch| {
            let [x0, y0, x1, y1] = rect(p.bounds);
            (y0 / TILE..y1.div_ceil(TILE))
                .any(|ty| (x0 / TILE..x1.div_ceil(TILE)).any(|tx| changed[(ty * tw + tx) as usize]))
        };
        let before = self.patches.len();
        self.patches.retain(|p| !fresh && !stale(p));
        if self.patches.len() < before {
            hide(&self.patches);
        }
        let old = if fresh { &[][..] } else { &self.lines[..] };
        let regions = regions(&changed, width, height, old);
        let mut lines: Vec<Line> = old
            .iter()
            .filter(|l| !regions.iter().any(|r| overlaps(r, &outer(l.bounds))))
            .cloned()
            .chain(read_regions(&frame, width, &regions, deps)?)
            .collect();
        let ocr = start.elapsed();
        lines.sort_by(|a, b| {
            (a.bounds[1].total_cmp(&b.bounds[1])).then(a.bounds[0].total_cmp(&b.bounds[0]))
        });
        let blocks = group_blocks(&lines);
        let t = Instant::now();
        let tr = translate_blocks(&blocks, target, cache, deps).map_err(Error::Ai)?;
        let translate = t.elapsed();

        let t = Instant::now();
        let rtl = is_rtl(target);
        let mut out = frame.to_vec();
        let drawn = render(&mut out, width, height, &blocks, &tr, rtl);
        let patches = (blocks.iter().zip(&tr).zip(&drawn.rooms))
            .filter(|((_, t), _)| !t.trim().is_empty())
            .filter_map(|((b, _), room)| {
                patch(&frame, &out, width, window(b, *room, width, height))
            })
            .collect();
        let render = t.elapsed();

        *self = Live {
            size: (width, height),
            hashes,
            #[cfg(test)]
            blocks: translations(&blocks, &tr),
            lines,
            patches,
            drawn: Drawn {
                rooms: Vec::new(),
                ink: Vec::new(),
                ..drawn
            },
        };
        Ok(Some(Timings {
            ocr,
            translate,
            render,
            total: start.elapsed(),
            ..Timings::default()
        }))
    }

    /// Where the frame shows one of our patches (the layer was captured),
    /// puts back what was under it, so our own text is never diffed or read.
    fn unlayer(&self, frame: &mut Cow<[u8]>, width: u32) {
        // decide on the frame as captured: patches can overlap
        let shown: Vec<&Patch> = (self.patches.iter())
            .filter(|p| {
                let px = crop(frame, width, rect(p.bounds));
                let (ours, theirs) = (mad(&px, &p.rgba), mad(&px, &p.under));
                ours < 8.0 && ours < theirs
            })
            .collect();
        for p in shown {
            paste(frame.to_mut(), width, rect(p.bounds), &p.under);
        }
    }
}

/// One hash per TILE x TILE tile, row-major.
fn tile_hashes(rgba: &[u8], w: u32, h: u32) -> Vec<u64> {
    let tw = w.div_ceil(TILE) as usize;
    let mut hs = vec![DefaultHasher::new(); tw * h.div_ceil(TILE) as usize];
    for (y, row) in rgba.chunks_exact(w as usize * 4).enumerate() {
        for (tx, part) in row.chunks(TILE as usize * 4).enumerate() {
            hs[y / TILE as usize * tw + tx].write(part);
        }
    }
    hs.into_iter().map(|h| h.finish()).collect()
}

/// Changed tiles -> connected clusters -> their boxes padded by one tile,
/// grown over any known line they touch (a line is always read whole) and
/// merged where they overlap.
fn regions(changed: &[bool], w: u32, h: u32, lines: &[Line]) -> Vec<Rect> {
    let tw = w.div_ceil(TILE) as usize;
    let th = changed.len() / tw;
    let mut seen = vec![false; changed.len()];
    let mut rects = Vec::new();
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
            for ny in y.saturating_sub(1)..(y + 2).min(th) {
                for nx in x.saturating_sub(1)..(x + 2).min(tw) {
                    let j = ny * tw + nx;
                    if changed[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let t = TILE as usize;
        rects.push([
            (b[0].saturating_sub(1) * t) as u32,
            (b[1].saturating_sub(1) * t) as u32,
            ((b[2] + 2) * t).min(w as usize) as u32,
            ((b[3] + 2) * t).min(h as usize) as u32,
        ]);
    }
    let boxes: Vec<Rect> = lines.iter().map(|l| outer(l.bounds)).collect();
    let mut grew = true;
    while grew {
        grew = false;
        for r in rects.iter_mut() {
            for b in &boxes {
                if overlaps(r, b) && union(r, b) != *r {
                    *r = union(r, b);
                    grew = true;
                }
            }
        }
        let mut i = 0;
        while i < rects.len() {
            match (i + 1..rects.len()).find(|&j| overlaps(&rects[i], &rects[j])) {
                Some(j) => {
                    let other = rects.swap_remove(j);
                    rects[i] = union(&rects[i], &other);
                    grew = true;
                }
                None => i += 1,
            }
        }
    }
    rects
        .into_iter()
        .map(|r| [r[0], r[1], r[2].min(w), r[3].min(h)])
        .collect()
}

/// OCR of the regions in one call: crops stacked top to bottom, GAP rows of
/// black between them; each line found is moved back to frame coordinates.
fn read_regions(
    frame: &[u8],
    width: u32,
    regions: &[Rect],
    deps: &impl Deps,
) -> Result<Vec<Line>, Error> {
    let cw = regions.iter().map(|r| r[2] - r[0]).max().unwrap_or(0);
    let mut img = Vec::new();
    let mut tops = Vec::new();
    for r in regions {
        if !tops.is_empty() {
            img.extend([0, 0, 0, 255].repeat((GAP * cw) as usize));
        }
        tops.push((img.len() / 4) as u32 / cw);
        for row in crop(frame, width, *r).chunks_exact(((r[2] - r[0]) * 4) as usize) {
            img.extend_from_slice(row);
            img.extend([0, 0, 0, 255].repeat((cw - (r[2] - r[0])) as usize));
        }
    }
    if img.is_empty() {
        return Ok(Vec::new());
    }
    let ch = img.len() as u32 / 4 / cw;
    let fh = (frame.len() / 4) as u32 / width;
    let found = deps.recognize(&img, cw, ch).map_err(Error::Ocr)?;
    Ok(found
        .into_iter()
        .filter_map(|b| {
            let [x, y, w, h] = b.bounds;
            let cy = y + h / 2.0;
            let i = (0..regions.len()).find(|&i| {
                let top = tops[i] as f32;
                cy >= top && cy < top + (regions[i][3] - regions[i][1]) as f32
            })?;
            let (r, top) = (regions[i], tops[i] as f32);
            let bounds = [x + r[0] as f32, y - top + r[1] as f32, w, h];
            let b = TextBlock { bounds, ..b };
            (!cut_off(&b, (width, fh))).then(|| to_line(b, width, fh))
        })
        .collect())
}

/// Where `render` may have drawn for block `b`: its box and its room (see
/// `render`), one line height above and below, two to each side.
fn window(b: &Block, room: [f32; 4], w: u32, h: u32) -> Rect {
    let lh = b.lines[0].bounds[3];
    let [x, y, bw, bh] = b.bounds;
    let (x0, x1) = (x.min(room[0]), (x + bw).max(room[0] + room[2]));
    let r = outer([x0 - 2.0 * lh, y - lh, x1 - x0 + 4.0 * lh, bh + 2.0 * lh]);
    [r[0], r[1], r[2].min(w), r[3].min(h)]
}

/// The box around the pixels `render` changed inside `win`, cut from `out`.
fn patch(frame: &[u8], out: &[u8], width: u32, win: Rect) -> Option<Patch> {
    let mut b: Option<Rect> = None;
    for y in win[1]..win[3] {
        for x in win[0]..win[2] {
            let i = ((y * width + x) * 4) as usize;
            if frame[i..i + 4] != out[i..i + 4] {
                b = Some(b.map_or([x, y, x + 1, y + 1], |b| {
                    [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)]
                }));
            }
        }
    }
    let r = b?;
    Some(Patch {
        bounds: [r[0], r[1], r[2] - r[0], r[3] - r[1]],
        rgba: crop(out, width, r),
        under: crop(frame, width, r),
    })
}

/// Bounds (x, y, w, h, may be fractional or negative) -> the pixel rect around them.
fn outer(b: [f32; 4]) -> Rect {
    let px = |v: f32| v.max(0.0) as u32;
    [
        px(b[0].floor()),
        px(b[1].floor()),
        px((b[0] + b[2]).ceil()),
        px((b[1] + b[3]).ceil()),
    ]
}

fn rect(b: [u32; 4]) -> Rect {
    [b[0], b[1], b[0] + b[2], b[1] + b[3]]
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn union(a: &Rect, b: &Rect) -> Rect {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn crop(rgba: &[u8], width: u32, r: Rect) -> Vec<u8> {
    (r[1]..r[3])
        .flat_map(|y| &rgba[((y * width + r[0]) * 4) as usize..((y * width + r[2]) * 4) as usize])
        .copied()
        .collect()
}

fn paste(rgba: &mut [u8], width: u32, r: Rect, src: &[u8]) {
    let row = ((r[2] - r[0]) * 4) as usize;
    for (y, s) in (r[1]..r[3]).zip(src.chunks_exact(row)) {
        let i = ((y * width + r[0]) * 4) as usize;
        rgba[i..i + row].copy_from_slice(s);
    }
}

/// Mean absolute difference per byte.
fn mad(a: &[u8], b: &[u8]) -> f32 {
    let sum: u64 = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u64).sum();
    sum as f32 / a.len().max(1) as f32
}

// ---- The beat loop (no window, no capture; the clock is a parameter) ----

/// The loop's time source. `Wall` is the real one; a test advances its own.
pub trait Clock {
    /// Time since the clock started.
    fn elapsed(&self) -> Duration;
    fn sleep(&self, d: Duration);
}

/// The real clock.
pub struct Wall(Instant);

impl Wall {
    pub fn new() -> Self {
        Wall(Instant::now())
    }
}

impl Clock for Wall {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Calls `tick` on a fixed beat of `interval` until `stop` is set or `send`
/// fails (the layer is gone). One tick at a time, never piled up: a tick
/// that runs over skips the beats it missed and the next starts on the next
/// beat (#56), so a slow PC gets fewer steps, not back-to-back ones.
pub fn pump<T>(
    clock: &impl Clock,
    stop: &AtomicBool,
    interval: Duration,
    mut tick: impl FnMut() -> Option<T>,
    mut send: impl FnMut(T) -> bool,
) {
    while !stop.load(Ordering::Relaxed) {
        if let Some(out) = tick()
            && !send(out)
        {
            return;
        }
        let beats = (clock.elapsed().as_nanos())
            .checked_div(interval.as_nanos())
            .map_or(0, |n| n + 1);
        let next = interval.saturating_mul(beats as u32);
        while !stop.load(Ordering::Relaxed) && clock.elapsed() < next {
            let left = next.saturating_sub(clock.elapsed());
            clock.sleep(left.min(Duration::from_millis(50)));
        }
    }
}

/// A tick while the session is `away` (locked, switched, screen off: #56):
/// `Some` = skip it, nothing captured; the `bool` says the layer must be
/// cleared (the session just went away). Going away or coming back starts the
/// steps afresh: everything is read again.
pub fn away_gate(away: bool, was: &mut bool, steps: &mut Live) -> Option<bool> {
    let changed = std::mem::replace(was, away) != away;
    if changed {
        *steps = Live::new();
        crate::log!(
            "[opencircle] live translate {}",
            if away {
                "stopped: the session is away"
            } else {
                "resumed"
            }
        );
    }
    away.then_some(changed)
}

/// A step error once (not every second), then nothing until it changes.
pub fn report_once(last: &mut Option<String>, e: String) -> Option<String> {
    if last.as_ref() == Some(&e) {
        return None;
    }
    crate::log!("[opencircle] live translate: {e}");
    *last = Some(e.clone());
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::super::pipeline::tests::{Fake, frame, tb};
    use super::*;

    const W: u32 = 320;
    const H: u32 = 192;
    const OLD: [f32; 4] = [40.0, 40.0, 200.0, 24.0];

    /// Full frame: one known line; any crop: one new line at (5, 40) in it.
    fn fake() -> Fake {
        Fake::new(|_, w, h| match (w, h) {
            (W, H) => vec![tb("Open the gate", OLD)],
            _ => vec![tb("New quest", [5.0, 40.0, 60.0, 20.0])],
        })
    }

    /// An OCR box in frame coordinates as the blocks report it (padded).
    fn pad(text: &str, b: [f32; 4]) -> [f32; 4] {
        to_line(tb(text, b), W, H).bounds
    }

    #[test]
    fn same_frame_twice_reads_nothing_again() {
        let (img, fake, cache) = (frame(W, H, &[OLD]), fake(), Cache::new(16));
        let mut live = Live::new();
        assert!(
            live.step(&img, W, H, "ar", &cache, &fake)
                .unwrap()
                .is_some()
        );
        assert_eq!(*fake.ocr_calls.borrow(), [(W, H)]);
        assert_eq!(live.blocks()[0].translation, "ترجمة Open the gate");
        assert!(!live.patches().is_empty());
        assert!(
            live.step(&img, W, H, "ar", &cache, &fake)
                .unwrap()
                .is_none()
        );
        assert_eq!(fake.ocr_calls.borrow().len(), 1);
        assert_eq!(fake.ai_calls.borrow().len(), 1);
    }

    #[test]
    fn one_changed_tile_rereads_only_its_region() {
        let (img, fake, cache) = (frame(W, H, &[OLD]), fake(), Cache::new(16));
        let mut live = Live::new();
        live.step(&img, W, H, "ar", &cache, &fake).unwrap();
        // tile (7, 4) = x 224..256, y 128..160
        let mut next = img.clone();
        paste(&mut next, W, [230, 140, 250, 150], &[255; 20 * 10 * 4]);
        live.step(&next, W, H, "ar", &cache, &fake)
            .unwrap()
            .unwrap();
        // padded by one tile: x 192..288, y 96..192
        assert_eq!(fake.ocr_calls.borrow()[1], (96, 96));
        let texts: Vec<(&str, [f32; 4])> = live
            .blocks()
            .iter()
            .map(|b| (b.original.as_str(), b.bounds))
            .collect();
        assert_eq!(
            texts,
            [
                ("Open the gate", pad("x", OLD)),
                ("New quest", pad("x", [197.0, 136.0, 60.0, 20.0]))
            ]
        );
        // the unchanged line came from the cache
        assert_eq!(fake.ai_calls.borrow()[1], ["New quest"]);
    }

    #[test]
    fn a_change_touching_a_line_rereads_the_whole_line() {
        let r = regions(
            &tiles(&[(7, 1)]),
            W,
            H,
            &[Line {
                bounds: OLD,
                text: "x".into(),
            }],
        );
        assert_eq!(r, [[40, 0, 288, 96]]);
        // two far-apart changes, two regions; stacked for one OCR call
        let r = regions(&tiles(&[(0, 0), (9, 5)]), W, H, &[]);
        assert_eq!(r, [[0, 0, 64, 64], [256, 128, 320, 192]]);
        let mut fake = fake();
        fake.lines = Box::new(|_, w, h| {
            assert_eq!((w, h), (64, 64 + GAP + 64));
            vec![tb("b", [1.0, 64.0 + 16.0 + 2.0, 9.0, 9.0])]
        });
        let got = read_regions(&frame(W, H, &[]), W, &r, &fake).unwrap();
        assert_eq!(got[0].bounds, pad("b", [257.0, 130.0, 9.0, 9.0]));
    }

    fn tiles(on: &[(usize, usize)]) -> Vec<bool> {
        let tw = W.div_ceil(TILE) as usize;
        let mut t = vec![false; tw * H.div_ceil(TILE) as usize];
        for (x, y) in on {
            t[y * tw + x] = true;
        }
        t
    }

    #[test]
    fn our_own_layer_in_the_capture_is_ignored() {
        // two close lines: their patches overlap
        const NEXT: [f32; 4] = [40.0, 72.0, 120.0, 24.0];
        let (img, cache) = (frame(W, H, &[OLD, NEXT]), Cache::new(16));
        let fake = Fake::new(|_, _, _| vec![tb("Open the gate.", OLD), tb("Run", NEXT)]);
        let mut live = Live::new();
        live.step(&img, W, H, "ar", &cache, &fake).unwrap();
        let p = live.patches();
        assert!(p.len() == 2 && overlaps(&rect(p[0].bounds), &rect(p[1].bounds)));
        // X11: the capture shows the layer's patches over the game
        let mut shown = img.clone();
        for p in live.patches() {
            paste(&mut shown, W, rect(p.bounds), &p.rgba);
        }
        assert!(shown != img);
        assert!(
            live.step(&shown, W, H, "ar", &cache, &fake)
                .unwrap()
                .is_none()
        );
        assert_eq!(fake.ocr_calls.borrow().len(), 1);
    }

    /// New text under a patch: the patch goes before the new text is read
    /// (no stale translation over it meanwhile); one elsewhere stays.
    #[test]
    fn a_patch_whose_text_changed_is_hidden_before_the_reread() {
        const FAR: [f32; 4] = [40.0, 140.0, 120.0, 24.0];
        let (img, cache) = (frame(W, H, &[OLD, FAR]), Cache::new(16));
        let fake = Fake::new(|_, _, _| vec![tb("Open the gate", OLD), tb("Run", FAR)]);
        let mut live = Live::new();
        live.step(&img, W, H, "ar", &cache, &fake).unwrap();
        let [kept, gone] = [1, 0].map(|i| live.patches()[i].clone());
        let mut next = img.clone();
        paste(&mut next, W, [60, 44, 80, 54], &[255; 20 * 10 * 4]);
        let (mut left, mut reads) = (None, 0);
        let hide = |p: &[Patch]| {
            left = Some(p.to_vec());
            reads = fake.ocr_calls.borrow().len();
        };
        live.step_hiding(&next, W, H, "ar", &cache, &fake, hide)
            .unwrap();
        assert_eq!(left, Some(vec![kept]), "only the far patch stays");
        assert_eq!(reads, 1, "hidden before the new text is read");
        assert!(!live.patches().contains(&gone));

        // new text again, and its translation fails: still hidden
        let mut fake = fake;
        fake.fail_ai = true;
        fake.lines = Box::new(|_: &[u8], _, _| vec![tb("Shut the gate", OLD)]);
        let n = live.patches().len();
        paste(&mut next, W, [60, 44, 80, 54], &[9; 20 * 10 * 4]);
        let mut hidden = false;
        assert!(
            live.step_hiding(&next, W, H, "ar", &cache, &fake, |_| hidden = true)
                .is_err()
        );
        assert!(hidden && live.patches().len() < n);
        // nothing new under a patch: nothing hidden
        let mut called = false;
        live.step_hiding(&next, W, H, "ar", &cache, &fake, |_| called = true)
            .ok();
        assert!(!called);
    }

    #[test]
    fn ai_error_keeps_the_state_for_a_retry() {
        let (img, mut fake, cache) = (frame(W, H, &[OLD]), fake(), Cache::new(16));
        fake.fail_ai = true;
        let mut live = Live::new();
        assert!(live.step(&img, W, H, "ar", &cache, &fake).is_err());
        assert!(live.patches().is_empty());
        fake.fail_ai = false;
        assert!(
            live.step(&img, W, H, "ar", &cache, &fake)
                .unwrap()
                .is_some()
        );
        assert_eq!(fake.ocr_calls.borrow().len(), 2);
    }

    /// A clock that only moves when asked to sleep.
    #[derive(Default)]
    struct Manual(std::cell::Cell<Duration>);

    impl Clock for Manual {
        fn elapsed(&self) -> Duration {
            self.0.get()
        }
        fn sleep(&self, d: Duration) {
            self.0.set(self.0.get() + d);
        }
    }

    #[test]
    fn steps_never_overlap_and_stop_when_asked() {
        let clock = Manual::default();
        let stop = AtomicBool::new(false);
        let beat = Duration::from_millis(20);
        let mut starts = Vec::new();
        let tick = || {
            starts.push(clock.elapsed());
            clock.sleep(Duration::from_millis(5)); // the step's own time
            if starts.len() == 4 {
                stop.store(true, Ordering::SeqCst); // off, during a step
            }
            Some(())
        };
        pump(&clock, &stop, beat, tick, |()| true);
        assert_eq!(
            starts,
            [0, 20, 40, 60].map(Duration::from_millis),
            "paced; the running step ends it"
        );

        // the layer went away: the loop ends at the next result
        let stop = AtomicBool::new(false);
        let mut sent = 0;
        pump(
            &clock,
            &stop,
            Duration::ZERO,
            || Some(()),
            |()| {
                sent += 1;
                sent < 3
            },
        );
        assert_eq!(sent, 3);
    }

    /// #56: steps start on a fixed beat; one that runs over skips the beats
    /// it missed (the next waits for the next beat, never back to back).
    #[test]
    fn a_slow_step_skips_beats_instead_of_piling_up() {
        let clock = Manual::default();
        let beat = Duration::from_millis(40);
        let stop = AtomicBool::new(false);
        let mut starts = Vec::new();
        let tick = || {
            starts.push(clock.elapsed());
            if starts.len() == 1 {
                clock.sleep(beat * 3 / 2); // busy until 60 ms, past beat 1
            }
            if starts.len() == 4 {
                stop.store(true, Ordering::SeqCst);
            }
            Some(())
        };
        pump(&clock, &stop, beat, tick, |()| true);
        // the step ended at 60 ms: beat 1 (40 ms) is skipped, the next is at 80 ms
        assert_eq!(starts, [0, 80, 120, 160].map(Duration::from_millis));
    }

    /// #56: a locked (or switched, or dark) session captures nothing; the
    /// layer clears once as it goes away, and steps run again once back.
    #[test]
    fn live_stops_while_the_session_is_away_and_resumes_after() {
        let (mut was, mut steps) = (false, Live::new());
        assert_eq!(away_gate(false, &mut was, &mut steps), None, "steps run");
        assert_eq!(
            away_gate(true, &mut was, &mut steps),
            Some(true),
            "the layer clears as it goes"
        );
        for _ in 0..3 {
            assert_eq!(
                away_gate(true, &mut was, &mut steps),
                Some(false),
                "nothing captured, nothing sent"
            );
        }
        assert_eq!(away_gate(false, &mut was, &mut steps), None, "resumed");
    }

    #[test]
    fn a_repeated_error_is_reported_once() {
        let mut last = None;
        assert!(report_once(&mut last, "HTTP 429".into()).is_some());
        assert!(report_once(&mut last, "HTTP 429".into()).is_none());
        assert!(report_once(&mut last, "HTTP 503".into()).is_some());
    }
}
