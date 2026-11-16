//! Live translate's engines for the live core (`crate::live_core`, design:
//! live-approach.md 5-6): OpenCircle's OCR, translation and erase-and-redraw
//! behind the core's small traits. No window, no capture, no threads here
//! (`screens::live_layer` runs the loop).
//!
//! - [`Detect`]: the text in the areas the core saw change, found in ONE
//!   call (crops stacked, `Deps::detect`: Windows OCR alone where it reads the
//!   language), grouped into blocks: each block is a region.
//! - [`Read`]: a region once it has settled. Where the detect read it as a
//!   full read would, that text is used again while the crop still looks the
//!   same (no second OCR); else the crops are read in one call.
//! - [`Translate`]: one block through `translate_blocks` (its cache, the
//!   glossary, the user's engine).
//! - [`Paint`]: `translate::render` on a window around the block; the patch
//!   is what it changed, every other pixel left see-through.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use crate::features::text_reading::TextBlock;
use crate::features::translate::pipeline::{
    Cache, Deps, Error, cut_off, to_line, translate_blocks,
};
use crate::features::translate::{Block, Line, group_blocks, render};
use crate::live_core::{Crop, Detector, Frame, Painter, Reader, Rect, TextBox, Translator};

/// Rows between stacked crops in the OCR image.
const GAP: u32 = 16;
/// Regions whose lines are kept for a later read / the paint.
const FOUND_MAX: usize = 256;
/// A crop reads as the one OCR saw while its gray samples differ less than
/// this on average (0..255: video noise, a see-through box over a moving
/// scene) and at most `SAME_STRONG` of them by more than 40 (a new letter).
const SAME_CROP: f32 = 6.0;
const SAME_STRONG: f32 = 0.01;

/// x0, y0, x1, y1 in frame pixels.
type Box4 = [u32; 4];

/// What OCR found where: each region's lines (frame px) and its gray cells
/// then (`cells`), newest last, at most `FOUND_MAX`.
#[derive(Default)]
pub struct Found {
    /// Lines, cells, and whether the lines were read in full (a reader may
    /// use them as they are).
    map: HashMap<Rect, (Vec<Line>, Vec<u8>, bool)>,
    order: VecDeque<Rect>,
    /// Since the last `take_counts`: reads answered by the detect's OCR,
    /// reads OCR'd again, patches made, patches declined (render left the
    /// block as it was: a busy background, no room; or over a blanked part).
    pub counts: [u32; 4],
}

impl Found {
    fn put(&mut self, at: Rect, lines: Vec<Line>, cells: Vec<u8>, full: bool) {
        if self.map.insert(at, (lines, cells, full)).is_none() {
            self.order.push_back(at);
        }
        while self.order.len() > FOUND_MAX {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    /// The lines found at `at`, or read round it (a crop with room round
    /// the box: the smallest such read).
    fn lines(&self, at: &Rect) -> Option<&[Line]> {
        let holds = |r: &Rect| {
            r.x <= at.x && r.y <= at.y && r.right() >= at.right() && r.bottom() >= at.bottom()
        };
        (self.map.get(at))
            .or_else(|| {
                (self.map.iter())
                    .filter(|(r, (_, _, full))| *full && holds(r))
                    .min_by_key(|(r, _)| r.area())
                    .map(|(_, v)| v)
            })
            .map(|(l, _, _)| l.as_slice())
    }

    pub fn take_counts(&mut self) -> [u32; 4] {
        std::mem::take(&mut self.counts)
    }
}

/// A gray value for every 4th pixel of every 4th row of an RGBA image.
fn cells(rgba: &[u8], w: u32) -> Vec<u8> {
    let row = w as usize * 4;
    (rgba.chunks_exact(row.max(4)).step_by(4))
        .flat_map(|r| r.as_chunks::<4>().0.iter().step_by(4))
        .map(|p| ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8) as u8)
        .collect()
}

fn same_cells(a: &[u8], b: &[u8]) -> bool {
    let (mut sum, mut strong) = (0u32, 0u32);
    for (x, y) in a.iter().zip(b) {
        let d = x.abs_diff(*y) as u32;
        sum += d;
        strong += (d > 40) as u32;
    }
    let n = a.len().max(1) as f32;
    a.len() == b.len() && sum as f32 <= SAME_CROP * n && strong as f32 <= SAME_STRONG * n
}

/// A block's text: its lines in reading order, joined.
fn text_of(lines: &[Line]) -> String {
    (group_blocks(lines).iter())
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first error of a job, for the controller's notice.
pub type Errors = RefCell<Option<String>>;

fn note(errors: &Errors, e: impl std::fmt::Display) {
    errors.borrow_mut().get_or_insert_with(|| e.to_string());
}

/// The core's detector: OCR of the changed areas, each block a region.
pub struct Detect<'a, D: Deps> {
    pub deps: &'a D,
    pub found: &'a Mutex<Found>,
    pub errors: &'a Errors,
}

impl<D: Deps> Detector for Detect<'_, D> {
    fn detect(&mut self, frame: &Frame, areas: &[Rect]) -> Vec<TextBox> {
        let regions: Vec<Box4> = (areas.iter())
            .map(|a| [a.x, a.y, a.right(), a.bottom()])
            .collect();
        let (mut lines, full) = match read_regions(frame.rgba, frame.width, &regions, self.deps) {
            Ok(read) => read,
            Err(e) => {
                note(self.errors, e);
                return Vec::new();
            }
        };
        lines.sort_by(|a, b| {
            (a.bounds[1].total_cmp(&b.bounds[1])).then(a.bounds[0].total_cmp(&b.bounds[0]))
        });
        if crate::shared::platform::sys::test_hook("OPENCIRCLE_LIVE_LOG").is_some() {
            let boxes: Vec<_> = lines.iter().map(|l| l.bounds.map(|v| v as i32)).collect();
            crate::log!("[opencircle] live: detect {areas:?} -> {boxes:?} (full {full})");
        }
        let mut found = self.found.lock().unwrap_or_else(|e| e.into_inner());
        let rect = |b: [f32; 4]| {
            let [x0, y0, x1, y1] = outer(b);
            let (x0, y0) = (x0.min(frame.width), y0.min(frame.height));
            Rect::new(x0, y0, x1.min(frame.width) - x0, y1.min(frame.height) - y0)
        };
        (group_blocks(&lines).into_iter())
            .filter_map(|b| {
                let at = rect(b.bounds);
                (at.area() > 0).then(|| {
                    let crop = crop(
                        frame.rgba,
                        frame.width,
                        [at.x, at.y, at.right(), at.bottom()],
                    );
                    let lines = b.lines.iter().map(|l| rect(l.bounds)).collect();
                    found.put(at, b.lines, cells(&crop, at.w), full);
                    TextBox { at, lines }
                })
            })
            .collect()
    }
}

/// The core's reader: what the detect read, while the crop still looks the
/// same; else the crops in one OCR call.
pub struct Read<'a, D: Deps> {
    pub deps: &'a D,
    pub found: &'a Mutex<Found>,
    pub errors: &'a Errors,
    /// The frame's size (the lines' boxes are clamped to it).
    pub size: (u32, u32),
}

impl<D: Deps> Reader for Read<'_, D> {
    fn read(&mut self, crops: &[Crop]) -> Vec<String> {
        let mut texts = vec![String::new(); crops.len()];
        let mut todo = Vec::new();
        {
            let mut found = self.found.lock().unwrap_or_else(|e| e.into_inner());
            for (i, c) in crops.iter().enumerate() {
                match found.map.get(&c.at) {
                    Some((lines, seen, true)) if same_cells(seen, &cells(&c.rgba, c.at.w)) => {
                        texts[i] = text_of(lines);
                    }
                    _ => todo.push(i),
                }
            }
            found.counts[0] += (crops.len() - todo.len()) as u32;
            found.counts[1] += todo.len() as u32;
        }
        if todo.is_empty() {
            return texts;
        }
        let parts: Vec<(&[u8], u32, u32)> = (todo.iter())
            .map(|&i| (crops[i].rgba.as_slice(), crops[i].at.w, crops[i].at.h))
            .collect();
        let read = match read_stacked(&parts, self.deps, false) {
            Ok((read, _)) => read,
            Err(e) => {
                note(self.errors, e);
                return texts;
            }
        };
        let mut found = self.found.lock().unwrap_or_else(|e| e.into_inner());
        for (&i, blocks) in todo.iter().zip(read) {
            let c = &crops[i];
            let lines: Vec<Line> = (blocks.into_iter())
                .map(|b| {
                    let [x, y, w, h] = b.bounds;
                    let bounds = [x + c.at.x as f32, y + c.at.y as f32, w, h];
                    to_line(TextBlock { bounds, ..b }, self.size.0, self.size.1)
                })
                .collect();
            texts[i] = text_of(&lines);
            found.put(c.at, lines, cells(&c.rgba, c.at.w), true);
        }
        texts
    }
}

/// The core's translator: one block into `target` (the shared cache, the
/// glossary's terms on a game screen, the user's engine).
pub struct Translate<'a, D: Deps> {
    pub deps: &'a D,
    pub cache: &'a Cache,
    pub target: &'a str,
}

impl<D: Deps> Translator for Translate<'_, D> {
    fn translate(&mut self, text: &str) -> Result<String, String> {
        let line = Line {
            bounds: [0.0, 0.0, 1.0, 1.0],
            text: text.into(),
        };
        let block = Block {
            lines: vec![line],
            text: text.into(),
            bounds: [0.0, 0.0, 1.0, 1.0],
        };
        let out = translate_blocks(&[block], self.target, self.cache, self.deps)
            .map(|t| t.into_iter().next().unwrap_or_default())
            .map_err(|e| e.to_string());
        // the model keeping the text as it is (a name): nothing is drawn; the test harness counts those apart
        if matches!(&out, Ok(t) if t.trim().is_empty())
            && crate::shared::platform::sys::test_hook("OPENCIRCLE_LIVE_LOG").is_some()
        {
            crate::log!("[opencircle] live: kept as is: {text}");
        }
        out
    }
}

/// The core's painter: `render` on a window around the region (its lines
/// as read), cut to what it changed. The rest of the patch is see-through
/// (alpha 0, its colour the frame's: what a capture with our layer shows).
pub struct Paint<'a> {
    pub found: &'a Mutex<Found>,
    pub rtl: bool,
    /// Parts of the frame blanked out (black; frame px x, y, w, h): no patch
    /// there (its erase would draw a black box).
    pub blanked: &'a [[u32; 4]],
}

impl Painter for Paint<'_> {
    fn patch(&mut self, frame: &Frame, region: Rect, text: &str) -> Option<(Rect, Vec<u8>)> {
        let lines = {
            let found = self.found.lock().unwrap_or_else(|e| e.into_inner());
            found.lines(&region).map(<[Line]>::to_vec)
        }
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| {
            vec![Line {
                bounds: [region.x, region.y, region.w, region.h].map(|v| v as f32),
                text: String::new(),
            }]
        });
        let win = window(region, lines[0].bounds[3], (frame.width, frame.height));
        let (ww, wh) = (win[2] - win[0], win[3] - win[1]);
        let src = crop(frame.rgba, frame.width, win);
        let (dx, dy) = (win[0] as f32, win[1] as f32);
        let lines: Vec<Line> = (lines.into_iter())
            .map(|l| {
                let [x, y, w, h] = l.bounds;
                Line {
                    bounds: [x - dx, y - dy, w, h],
                    ..l
                }
            })
            .collect();
        let bounds = (lines.iter().skip(1)).fold(lines[0].bounds, |b, l| {
            let [x0, y0] = [b[0].min(l.bounds[0]), b[1].min(l.bounds[1])];
            let x1 = (b[0] + b[2]).max(l.bounds[0] + l.bounds[2]);
            let y1 = (b[1] + b[3]).max(l.bounds[1] + l.bounds[3]);
            [x0, y0, x1 - x0, y1 - y0]
        });
        let block = Block {
            text: text_of(&lines),
            lines,
            bounds,
        };
        let mut out = src.clone();
        render(&mut out, ww, wh, &[block], &[text.to_string()], self.rtl);
        let count = |i: usize| self.found.lock().unwrap_or_else(|e| e.into_inner()).counts[i] += 1;
        let why = |w: &str| {
            if crate::shared::platform::sys::test_hook("OPENCIRCLE_LIVE_LOG").is_some() {
                crate::log!("[opencircle] live: declined {region:?}: {w}");
            }
        };
        let Some(changed) = changed_box(&src, &out, ww) else {
            why("render changed nothing");
            count(3);
            return None;
        };
        let at = Rect::new(
            win[0] + changed[0],
            win[1] + changed[1],
            changed[2] - changed[0],
            changed[3] - changed[1],
        );
        if over_blanked([at.x, at.y, at.w, at.h], self.blanked) {
            why(&format!("patch {at:?} over a blanked part"));
            count(3);
            return None;
        }
        count(2);
        let mut rgba = crop(&out, ww, changed);
        let before = crop(&src, ww, changed);
        for (p, b) in (rgba.as_chunks_mut::<4>().0.iter_mut()).zip(before.as_chunks::<4>().0) {
            if p == b {
                p[3] = 0;
            }
        }
        Some((at, rgba))
    }
}

/// Where `render` may draw for a region: one line height above and below
/// it, two to each side (its room), clamped to the frame. x0, y0, x1, y1.
fn window(r: Rect, line: f32, (w, h): (u32, u32)) -> Box4 {
    let lh = line.max(1.0) as u32;
    [
        r.x.saturating_sub(2 * lh),
        r.y.saturating_sub(lh),
        (r.right() + 2 * lh).min(w),
        (r.bottom() + lh).min(h),
    ]
}

/// The box (x0, y0, x1, y1) around the pixels that differ between two
/// images `width` wide; none if they are the same.
fn changed_box(a: &[u8], b: &[u8], width: u32) -> Option<Box4> {
    let mut found: Option<Box4> = None;
    let (pa, pb) = (a.as_chunks::<4>().0, b.as_chunks::<4>().0);
    for (i, (x, y)) in pa.iter().zip(pb).enumerate() {
        if x != y {
            let (px, py) = ((i as u32) % width, (i as u32) / width);
            found = Some(found.map_or([px, py, px + 1, py + 1], |f| {
                [
                    f[0].min(px),
                    f[1].min(py),
                    f[2].max(px + 1),
                    f[3].max(py + 1),
                ]
            }));
        }
    }
    found
}

/// A patch at `bounds` (x, y, w, h) touches a part of the frame that was
/// blanked out (black), so its erase and redraw would show black there.
pub fn over_blanked([x, y, w, h]: [u32; 4], blanked: &[[u32; 4]]) -> bool {
    (blanked.iter()).any(|&[bx, by, bw, bh]| x < bx + bw && bx < x + w && y < by + bh && by < y + h)
}

/// The text of the regions of a frame found in one call (`Deps::detect`):
/// crops stacked, each line moved back to frame coordinates (lines an edge
/// of the frame cuts through are left out); and whether they were read in full.
fn read_regions(
    frame: &[u8],
    width: u32,
    regions: &[Box4],
    deps: &impl Deps,
) -> Result<(Vec<Line>, bool), Error> {
    let crops: Vec<Vec<u8>> = regions.iter().map(|r| crop(frame, width, *r)).collect();
    let parts: Vec<(&[u8], u32, u32)> = (crops.iter().zip(regions))
        .map(|(c, r)| (c.as_slice(), r[2] - r[0], r[3] - r[1]))
        .collect();
    let fh = (frame.len() / 4) as u32 / width.max(1);
    let (read, full) = read_stacked(&parts, deps, true)?;
    let lines = (read.into_iter().zip(regions))
        .flat_map(|(blocks, r)| {
            blocks.into_iter().filter_map(move |b| {
                let [x, y, w, h] = b.bounds;
                let b = TextBlock {
                    bounds: [x + r[0] as f32, y + r[1] as f32, w, h],
                    ..b
                };
                (!cut_off(&b, (width, fh))).then(|| to_line(b, width, fh))
            })
        })
        .collect();
    Ok((lines, full))
}

/// OCR of RGBA images (`rgba`, width, height) in ONE call (`detect`: only
/// to find the lines, `Deps::detect`): stacked top to bottom, GAP black rows
/// between them; each one's lines, in its own pixels, and whether they were
/// read in full.
fn read_stacked(
    parts: &[(&[u8], u32, u32)],
    deps: &impl Deps,
    detect: bool,
) -> Result<(Vec<Vec<TextBlock>>, bool), Error> {
    let cw = parts.iter().map(|p| p.1).max().unwrap_or(0);
    let mut img = Vec::new();
    let mut tops = Vec::new();
    for (rgba, w, h) in parts {
        if !tops.is_empty() {
            img.extend([0, 0, 0, 255].repeat((GAP * cw) as usize));
        }
        tops.push((img.len() / 4) as u32 / cw.max(1));
        for row in rgba
            .chunks_exact((*w * 4).max(4) as usize)
            .take(*h as usize)
        {
            img.extend_from_slice(row);
            img.extend([0, 0, 0, 255].repeat((cw - w) as usize));
        }
    }
    let mut out = vec![Vec::new(); parts.len()];
    if img.is_empty() || cw == 0 {
        return Ok((out, true));
    }
    let ch = img.len() as u32 / 4 / cw;
    let (blocks, full) = match detect {
        true => deps.detect(&img, cw, ch),
        false => deps.recognize(&img, cw, ch).map(|b| (b, true)),
    }
    .map_err(Error::Ocr)?;
    for b in blocks {
        let [x, y, w, h] = b.bounds;
        let cy = y + h / 2.0;
        let Some(i) = (0..parts.len()).find(|&i| {
            let top = tops[i] as f32;
            cy >= top && cy < top + parts[i].2 as f32
        }) else {
            continue;
        };
        let bounds = [x, y - tops[i] as f32, w, h];
        out[i].push(TextBlock { bounds, ..b });
    }
    Ok((out, full))
}

/// Bounds (x, y, w, h, may be fractional or negative) -> the pixel rect around them.
fn outer(b: [f32; 4]) -> Box4 {
    let px = |v: f32| v.max(0.0) as u32;
    [
        px(b[0].floor()),
        px(b[1].floor()),
        px((b[0] + b[2]).ceil()),
        px((b[1] + b[3]).ceil()),
    ]
}

fn crop(rgba: &[u8], width: u32, r: Box4) -> Vec<u8> {
    (r[1]..r[3])
        .flat_map(|y| &rgba[((y * width + r[0]) * 4) as usize..((y * width + r[2]) * 4) as usize])
        .copied()
        .collect()
}

/// A tick while the session is `away` (locked, switched, screen off: #56):
/// `Some` = skip it, nothing captured; the `bool` says the layer must be
/// cleared (the session just went away).
pub fn away_gate(away: bool, was: &mut bool) -> Option<bool> {
    let changed = std::mem::replace(was, away) != away;
    if changed {
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

/// An error once (not every frame), then nothing until it changes.
pub fn report_once(last: &mut Option<String>, e: String) -> Option<String> {
    if last.as_ref() == Some(&e) {
        return None;
    }
    crate::log!("[opencircle] live translate: {e}");
    *last = Some(e.clone());
    Some(e)
}

/// A live session run in step with its frames: each job runs at once on
/// `deps` (tests and benches; the app runs them on its work thread).
#[cfg(test)]
pub struct InStep<'a, D: Deps> {
    pub live: crate::live_core::Live,
    pub found: Mutex<Found>,
    pub deps: &'a D,
    pub cache: &'a Cache,
    pub target: &'a str,
    /// The next frame's time (each frame is 1/60 s after the last).
    pub at: std::time::Duration,
}

#[cfg(test)]
impl<'a, D: Deps> InStep<'a, D> {
    pub fn new(deps: &'a D, cache: &'a Cache, target: &'a str) -> Self {
        let cfg = crate::live_core::Config {
            fade_in: std::time::Duration::ZERO,
            ..Default::default()
        };
        InStep {
            live: crate::live_core::Live::new(cfg),
            found: Mutex::default(),
            deps,
            cache,
            target,
            at: std::time::Duration::ZERO,
        }
    }

    /// One frame through the core, then every job it starts (and the ones
    /// they start) on this same frame.
    pub fn frame(
        &mut self,
        rgba: &[u8],
        (w, h): (u32, u32),
        layer: &mut dyn crate::live_core::Layer,
    ) {
        let f = Frame {
            rgba,
            width: w,
            height: h,
            at: self.at,
        };
        self.at += std::time::Duration::from_micros(16_667);
        let rtl = crate::features::translate::lang::is_rtl(self.target);
        let mut paint = Paint {
            found: &self.found,
            rtl,
            blanked: &[],
        };
        let mut jobs = self.live.on_frame(&f, &mut paint, layer);
        while let Some(job) = jobs.pop() {
            let errors = Errors::default();
            let (deps, found, errors) = (self.deps, &self.found, &errors);
            let done = job.run(
                &f,
                &mut Detect {
                    deps,
                    found,
                    errors,
                },
                &mut Read {
                    deps,
                    found,
                    errors,
                    size: (w, h),
                },
                &mut Translate {
                    deps,
                    cache: self.cache,
                    target: self.target,
                },
            );
            for d in done {
                jobs.extend(self.live.complete(d, &f, &mut paint, layer));
            }
        }
    }
}

/// The layer's calls, in order (tests).
#[cfg(test)]
#[derive(Default)]
pub struct Calls(pub Vec<String>);

#[cfg(test)]
impl crate::live_core::Layer for Calls {
    fn show(&mut self, id: u64, _: Rect, _: &[u8]) {
        self.0.push(format!("show {id}"));
    }
    fn alpha(&mut self, _: u64, _: f32) {}
    fn hide(&mut self, id: u64) {
        self.0.push(format!("hide {id}"));
    }
    fn clear(&mut self) {
        self.0.push("clear".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::translate::pipeline::tests::{Fake, frame, tb};

    /// The whole host path on a real-sized frame: a line appears, is found,
    /// read, translated and shown once; while it stays nothing is drawn
    /// again (one OCR call in all: the read reuses the detect's); when its
    /// text changes the patch goes in that same frame.
    #[test]
    fn a_line_is_shown_once_and_hidden_in_the_frame_its_text_changes() {
        let (w, h) = (640u32, 360u32);
        let line = [100.0, 200.0, 300.0, 28.0];
        // OCR finds the "glyphs" (`frame`'s light bar) wherever they are
        let fake = Fake::new(|rgba, w, _| {
            let lit = (rgba.as_chunks::<4>().0.iter().enumerate())
                .filter(|(_, p)| p[..3] == [240, 236, 226])
                .map(|(i, _)| ((i as u32 % w) as f32, (i as u32 / w) as f32));
            let b = lit.fold(None, |b: Option<[f32; 4]>, (x, y)| {
                Some(b.map_or([x, y, x, y], |b| {
                    [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]
                }))
            });
            (b.into_iter())
                .map(|[x0, y0, x1, y1]| {
                    tb(
                        "Open the gate",
                        [x0, y0 - 7.0, x1 - x0 + 1.0, y1 - y0 + 14.0],
                    )
                })
                .collect()
        });
        let cache = Cache::new(16);
        let mut s = InStep::new(&fake, &cache, "fr");
        let mut layer = Calls::default();
        let empty = frame(w, h, &[]);
        let text = frame(w, h, &[line]);
        for _ in 0..3 {
            s.frame(&empty, (w, h), &mut layer);
        }
        for _ in 0..60 {
            s.frame(&text, (w, h), &mut layer);
        }
        assert_eq!(layer.0, ["show 1"], "shown once, never redrawn");
        assert_eq!(fake.ai_calls.borrow().len(), 1);
        let ocr = fake.ocr_calls.borrow().len();
        // another line in its place
        let mut next = text.clone();
        for y in 207..221 {
            for x in 110..380 {
                let i = ((y * w + x) * 4) as usize;
                next[i..i + 3].copy_from_slice(&[20, 24, 32]);
            }
        }
        s.frame(&next, (w, h), &mut layer);
        assert_eq!(layer.0, ["show 1", "hide 1"], "hidden in the same frame");
        assert!(fake.ocr_calls.borrow().len() >= ocr);
    }

    const W: u32 = 320;
    const H: u32 = 192;
    const LINE: [f32; 4] = [40.0, 40.0, 200.0, 24.0];

    fn at(f: &[u8]) -> Frame<'_> {
        Frame {
            rgba: f,
            width: W,
            height: H,
            at: Default::default(),
        }
    }

    /// Two far-apart areas are read in ONE OCR call (stacked), and each line
    /// lands back where it is in the frame, as one region per block.
    #[test]
    fn changed_areas_are_read_in_one_call_and_become_regions() {
        let img = frame(W, H, &[LINE]);
        let fake = Fake::new(|_, w, h| {
            assert_eq!((w, h), (240, 64 + GAP + 64));
            // in the first crop (x 0..240, y 20..84): the line at frame (40, 40)
            vec![tb("Open the gate", [40.0, 20.0, 200.0, 24.0])]
        });
        let (found, errors) = (Mutex::default(), Errors::default());
        let mut d = Detect {
            deps: &fake,
            found: &found,
            errors: &errors,
        };
        let areas = [Rect::new(0, 20, 240, 64), Rect::new(0, 120, 100, 64)];
        let got = d.detect(&at(&img), &areas);
        assert_eq!(fake.ocr_calls.borrow().len(), 1);
        let want = outer(to_line(tb("x", LINE), W, H).bounds);
        let rect = Rect::new(want[0], want[1], want[2] - want[0], want[3] - want[1]);
        assert_eq!(got, [TextBox::from(rect)]);
        assert_eq!(
            found.lock().unwrap().lines(&got[0].at).unwrap()[0].text,
            "Open the gate"
        );
        assert!(errors.borrow().is_none());
    }

    /// The read of a settled region reuses what its detect read while the
    /// crop looks the same (no second OCR); a crop that differs is read again.
    #[test]
    fn a_settled_region_is_read_again_only_if_its_crop_changed() {
        let img = frame(W, H, &[LINE]);
        // the detect's area (240 wide) or the region's own crop
        let fake = Fake::new(|_, w, _| match w {
            240 => vec![tb("Open the gate", [40.0, 20.0, 200.0, 24.0])],
            _ => vec![tb("Open the gate", [4.0, 3.0, 200.0, 24.0])],
        });
        let (found, errors) = (Mutex::default(), Errors::default());
        let region = Detect {
            deps: &fake,
            found: &found,
            errors: &errors,
        }
        .detect(&at(&img), &[Rect::new(0, 20, 240, 64)])[0]
            .at;
        let b = [region.x, region.y, region.right(), region.bottom()];
        let crop_of = |f: &[u8]| Crop {
            id: 1,
            job: 1,
            at: region,
            rgba: crop(f, W, b),
        };
        let mut r = Read {
            deps: &fake,
            found: &found,
            errors: &errors,
            size: (W, H),
        };
        assert_eq!(r.read(&[crop_of(&img)]), ["Open the gate"]);
        assert_eq!(
            fake.ocr_calls.borrow().len(),
            1,
            "the detect's read, reused"
        );
        // the text under it changed: read again (in crop pixels)
        let mut next = img.clone();
        for y in 45..60 {
            for x in 50..200 {
                let i = ((y * W + x) * 4) as usize;
                next[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        assert_eq!(r.read(&[crop_of(&next)]), ["Open the gate"]);
        assert_eq!(fake.ocr_calls.borrow().len(), 2);
    }

    /// A detect that only found the lines (Windows OCR alone): the settled
    /// region is read in full, even with its crop unchanged.
    #[test]
    fn a_region_found_by_a_quick_detect_is_read_in_full() {
        struct Quick(Fake);
        impl Deps for Quick {
            fn recognize(
                &self,
                rgba: &[u8],
                w: u32,
                h: u32,
            ) -> Result<Vec<TextBlock>, crate::features::text_reading::OcrError> {
                self.0.recognize(rgba, w, h)
            }
            fn translate(
                &self,
                texts: &[String],
                target: &str,
            ) -> Result<Vec<String>, crate::features::ask_ai::AiError> {
                self.0.translate(texts, target)
            }
            fn detect(
                &self,
                rgba: &[u8],
                w: u32,
                h: u32,
            ) -> Result<(Vec<TextBlock>, bool), crate::features::text_reading::OcrError>
            {
                self.0.recognize(rgba, w, h).map(|l| (l, false))
            }
        }
        let img = frame(W, H, &[LINE]);
        let quick = Quick(Fake::new(|_, w, _| match w {
            240 => vec![tb("Open teh gate", [40.0, 20.0, 200.0, 24.0])],
            _ => vec![tb("Open the gate", [4.0, 3.0, 200.0, 24.0])],
        }));
        let (found, errors) = (Mutex::default(), Errors::default());
        let region = Detect {
            deps: &quick,
            found: &found,
            errors: &errors,
        }
        .detect(&at(&img), &[Rect::new(0, 20, 240, 64)])[0]
            .at;
        let b = [region.x, region.y, region.right(), region.bottom()];
        let crop = Crop {
            id: 1,
            job: 1,
            at: region,
            rgba: crop(&img, W, b),
        };
        let mut r = Read {
            deps: &quick,
            found: &found,
            errors: &errors,
            size: (W, H),
        };
        assert_eq!(r.read(&[crop]), ["Open the gate"], "the full read's text");
        assert_eq!(quick.0.ocr_calls.borrow().len(), 2);
    }

    /// The patch is only what `render` changed: the frame's own pixels in its
    /// box are see-through, and nothing is drawn over a blanked part.
    #[test]
    fn the_patch_is_what_render_changed_and_the_rest_is_see_through() {
        let img = frame(W, H, &[LINE]);
        let found = Mutex::new(Found::default());
        let region = Rect::new(36, 36, 210, 30);
        found.lock().unwrap().put(
            region,
            vec![to_line(tb("Open the gate", LINE), W, H)],
            Vec::new(),
            true,
        );
        let mut p = Paint {
            found: &found,
            rtl: false,
            blanked: &[],
        };
        let (at_, rgba) = p.patch(&at(&img), region, "Ouvre la porte").unwrap();
        assert_eq!(rgba.len() as u64, at_.area() * 4);
        let clear = rgba.as_chunks::<4>().0.iter().filter(|p| p[3] == 0).count();
        assert!(clear > 0, "unchanged pixels are see-through");
        assert!(clear < rgba.len() / 4, "and the redraw is not");
        // over a blanked part: nothing
        p.blanked = &[[0, 0, 60, 60]];
        assert!(p.patch(&at(&img), region, "Ouvre la porte").is_none());
    }

    /// The translator sends one block and gets its translation (the cache
    /// answers the second time).
    #[test]
    fn one_block_is_translated_through_the_cache() {
        let fake = Fake::new(|_, _, _| Vec::new());
        let cache = Cache::new(8);
        let mut t = Translate {
            deps: &fake,
            cache: &cache,
            target: "ar",
        };
        assert_eq!(t.translate("Open the gate").unwrap(), "ترجمة Open the gate");
        assert_eq!(t.translate("Open the gate").unwrap(), "ترجمة Open the gate");
        assert_eq!(fake.ai_calls.borrow().len(), 1);
    }

    /// #56: a locked (or switched, or dark) session captures nothing; the
    /// layer clears once as it goes away, and frames are read again once back.
    #[test]
    fn live_stops_while_the_session_is_away_and_resumes_after() {
        let mut was = false;
        assert_eq!(away_gate(false, &mut was), None, "frames are read");
        assert_eq!(away_gate(true, &mut was), Some(true), "the layer clears");
        for _ in 0..3 {
            assert_eq!(away_gate(true, &mut was), Some(false), "nothing captured");
        }
        assert_eq!(away_gate(false, &mut was), None, "resumed");
    }

    #[test]
    fn a_repeated_error_is_reported_once() {
        let mut last = None;
        assert!(report_once(&mut last, "HTTP 429".into()).is_some());
        assert!(report_once(&mut last, "HTTP 429".into()).is_none());
        assert!(report_once(&mut last, "HTTP 503".into()).is_some());
    }

    /// Replays a harness clip (`OC_CLIP=<clips>/<name>`: its .mp4 and .truth.json, from the internal
    /// live-test harness) through the whole host path on the decoded video, as the Windows VM shows it
    /// (1280x720, 30 fps): real OCR (the models in the data dir), a translator that reverses the line;
    /// each job takes its own measured time of clip time (one at a time, translations first), so the
    /// numbers are the core's on real frames: text shown -> drawn, patches left after their text
    /// changed, redraws of unchanged text. Run:
    /// `OC_CLIP=.../talk XDG_DATA_HOME=... cargo test --release replay_a_harness_clip -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn replay_a_harness_clip() {
        use crate::features::ask_ai::AiError;
        use crate::features::text_reading::{Engine, OcrError, Read as R, models_dir};
        use std::time::{Duration, Instant};
        struct Real(std::sync::Mutex<Engine>);
        impl Deps for Real {
            fn recognize(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<TextBlock>, OcrError> {
                self.0.lock().unwrap().recognize(rgba, w, h, R::One(0))
            }
            fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
                Ok(texts.iter().map(|t| t.chars().rev().collect()).collect())
            }
        }
        #[derive(Default)]
        struct Log(Vec<(u32, bool, u64, Rect)>, u32);
        impl crate::live_core::Layer for Log {
            fn show(&mut self, id: u64, at: Rect, _: &[u8]) {
                self.0.push((self.1, true, id, at));
            }
            fn alpha(&mut self, _: u64, _: f32) {}
            fn hide(&mut self, id: u64) {
                self.0.push((self.1, false, id, Rect::default()));
            }
            fn clear(&mut self) {}
        }
        let Some(clip) = std::env::var_os("OC_CLIP") else {
            return;
        };
        let clip = std::path::PathBuf::from(clip);
        let truth: serde_json::Value =
            serde_json::from_slice(&std::fs::read(clip.with_extension("truth.json")).unwrap())
                .unwrap();
        // `OC_PAD=1`: as the Ubuntu VM's 1280x813 window shows it (the video 46 px down); `OC_SLOW=2.5`:
        // each job takes that many times its time (the VM's slower OCR)
        let pad = std::env::var_os("OC_PAD").is_some();
        let slow: f64 = std::env::var("OC_SLOW").map_or(1.0, |v| v.parse().unwrap());
        let (w, h, k, dy) = match pad {
            true => (1280u32, 813u32, 1280.0 / 1920.0, 46.0),
            false => (1280u32, 720u32, 1280.0 / 1920.0, 0.0),
        };
        let deps = Real(std::sync::Mutex::new(Engine::load(&models_dir()).unwrap()));
        let cache = Cache::new(512);
        let mut s = InStep::new(&deps, &cache, "fr");
        let mut ffmpeg = std::process::Command::new("ffmpeg")
            .args(["-loglevel", "error", "-i"])
            .arg(clip.with_extension("mp4"))
            .args([
                "-vf",
                if pad {
                    "fps=30,scale=1280:720,pad=1280:813:0:46"
                } else {
                    "fps=30,scale=1280:720"
                },
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = ffmpeg.stdout.take().unwrap();
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let mut layer = Log::default();
        // jobs: (ready at frame, done); the worker is free at frame `free`
        let (mut queue, mut running): (
            Vec<crate::live_core::Job>,
            Vec<(u32, Vec<crate::live_core::Done>)>,
        ) = (Vec::new(), Vec::new());
        let (mut free, mut busy) = (0u32, Duration::ZERO);
        let rank = |j: &crate::live_core::Job| match j {
            crate::live_core::Job::Translate { .. } => 0,
            crate::live_core::Job::Read(_) => 1,
            crate::live_core::Job::Detect { .. } => 2,
        };
        let mut n = 0u32;
        while std::io::Read::read_exact(&mut out, &mut rgba).is_ok() {
            layer.1 = n;
            let f = Frame {
                rgba: &rgba,
                width: w,
                height: h,
                at: Duration::from_micros(33_333 * n as u64),
            };
            let mut paint = Paint {
                found: &s.found,
                rtl: false,
                blanked: &[],
            };
            // finished jobs first, against this newest frame
            let (done, left): (Vec<_>, Vec<_>) = running.drain(..).partition(|(at, _)| *at <= n);
            running = left;
            for (_, d) in done {
                for d in d {
                    queue.extend(s.live.complete(d, &f, &mut paint, &mut layer));
                }
            }
            queue.extend(s.live.on_frame(&f, &mut paint, &mut layer));
            if std::env::var_os("OC_DBG2").is_some() && n.is_multiple_of(15) {
                eprintln!("R {n} {}", s.live.debug_regions());
            }
            while free <= n && !queue.is_empty() {
                let i = (0..queue.len()).min_by_key(|&i| rank(&queue[i])).unwrap();
                let job = queue.remove(i);
                let errors = Errors::default();
                let t = Instant::now();
                let done = job.run(
                    &f,
                    &mut Detect {
                        deps: &deps,
                        found: &s.found,
                        errors: &errors,
                    },
                    &mut Read {
                        deps: &deps,
                        found: &s.found,
                        errors: &errors,
                        size: (w, h),
                    },
                    &mut Translate {
                        deps: &deps,
                        cache: &cache,
                        target: "fr",
                    },
                );
                let took = t.elapsed();
                busy += took;
                free = n
                    + ((took.as_micros() as f64 * slow) as u32)
                        .div_ceil(33_333)
                        .max(1);
                running.push((free, done));
            }
            n += 1;
        }
        let _ = ffmpeg.wait();
        // per held event (clip frames at 60 fps; ours at 30)
        let (mut lat, mut stale, mut redraws, mut flicker, mut drawn, mut held) =
            (Vec::new(), 0, 0, 0, 0, 0);
        // typed-out text (W19): drawn only once its whole box is complete, never in part; how long each
        // translation was up before its text went, and the texts never drawn while up
        let (mut early, mut visible, mut never) = (0, Vec::new(), 0);
        let events = truth["events"].as_array().unwrap();
        for e in events {
            let goes_on = |x: &&serde_json::Value| {
                x["off"].as_u64() == e["on"].as_u64()
                    && x["text"] == e["text"]
                    && x["kind"] == e["kind"]
            };
            // (a continued line is counted with its start)
            if !e["held"].as_bool().unwrap() || events.iter().any(|x| goes_on(&x)) {
                continue;
            }
            held += 1;
            let b: Vec<f64> = e["box"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let r = Rect::new(
                (b[0] * k) as u32,
                (b[1] * k + dy) as u32,
                (b[2] * k) as u32,
                (b[3] * k) as u32,
            );
            let (mut on, off) = (
                e["on"].as_u64().unwrap() as u32 / 2,
                e["off"].as_u64().unwrap() as u32 / 2,
            );
            if let Some(from) = e["typing_from"].as_u64() {
                // the box is complete when its last line is
                let whole = (events.iter())
                    .filter(|x| x["panel"] == e["panel"] && x["off"] == e["off"])
                    .filter_map(|x| x["on"].as_u64())
                    .max()
                    .unwrap_or(0) as u32
                    / 2;
                early += (layer.0.iter())
                    .filter(|(f, show, _, at)| {
                        *show
                            && (from as u32 / 2..whole).contains(f)
                            && 3 * at.overlap(&r) > r.area()
                    })
                    .count();
                on = whole;
            }
            // the same text again in the same place right after: not a change
            let mut end = e["off"].as_u64().unwrap();
            while let Some(next) = (events.iter()).find(|x| {
                x["on"].as_u64() == Some(end) && x["text"] == e["text"] && x["kind"] == e["kind"]
            }) {
                end = next["off"].as_u64().unwrap();
            }
            let still = end as u32 / 2;
            let mine: Vec<_> = (layer.0.iter())
                .filter(|(f, show, _, at)| {
                    *show && (on..off).contains(f) && 3 * at.overlap(&r) > r.area()
                })
                .collect();
            let Some(first) = mine.first() else {
                never += 1;
                if std::env::var_os("OC_DBG").is_some() {
                    eprintln!(
                        "DBG missed {:?} {:?} on {on} off {off} box {r:?}",
                        e["text"], e["kind"]
                    );
                }
                continue;
            };
            drawn += 1;
            lat.push((first.0 - on) as f64 * 33.3);
            visible.push((off - first.0) as f64 * 33.3);
            redraws += mine.len() - 1;
            if mine.len() > 1 && std::env::var_os("OC_DBG").is_some() {
                eprintln!("DBG redraw {:?} on {on} off {off}: {:?}", e["text"], mine);
            }
            for (_, _, id, _) in &mine {
                let gone = (layer.0.iter())
                    .find(|(_, show, i, _)| !show && i == id)
                    .map_or(n, |g| g.0);
                if gone < off {
                    flicker += 1;
                }
                if gone > still + 1 && still < n {
                    stale += 1;
                    eprintln!("stale: {:?} gone at {gone}, text off at {still}", e["text"]);
                }
            }
        }
        if std::env::var_os("OC_DBG2").is_some() {
            for e in layer.0.iter().filter(|e| (50..140).contains(&e.0)) {
                eprintln!("L {e:?}");
            }
        }
        eprintln!(
            "frames; held: reads reused / OCR'd, patches made / declined: {:?}",
            s.found.lock().unwrap().counts
        );
        lat.sort_by(f64::total_cmp);
        let p = |q: f64| {
            lat.get(((lat.len() as f64 - 1.0) * q).round() as usize)
                .copied()
        };
        eprintln!(
            "{}: {n} frames; held texts drawn {drawn} of {held}; shown -> drawn median {:?} ms, p95 {:?} ms; \
             stale > 1 frame {stale}; redraws of unchanged {redraws}; hidden while unchanged {flicker}; \
             OCR / MT busy {:.0} % of clip time; {} shows, {} hides",
            clip.display(),
            p(0.5),
            p(0.95),
            busy.as_secs_f64() * 100.0 / (n as f64 / 30.0),
            layer.0.iter().filter(|e| e.1).count(),
            layer.0.iter().filter(|e| !e.1).count(),
        );
        visible.sort_by(f64::total_cmp);
        eprintln!(
            "drawn before its box was complete {early}; never drawn while up {never}; up before it went median {:?} ms, min {:?} ms",
            visible.get(visible.len() / 2),
            visible.first()
        );
    }
}
