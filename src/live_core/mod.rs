//! The live core: engine-agnostic live translation logic (docs/internal design "live-approach", sections
//! 5.1-5.4, 5.6, 6). It decides, frame by frame, what the translation layer shows, so that nothing stale is
//! ever left over changed text, unchanged text is drawn once and never again, and the slow work (detect,
//! read, translate) runs only where text appeared and has settled.
//!
//! Std only and nothing of OpenCircle (arch test `live_core_uses_only_std_and_itself`): the host plugs in
//! its OCR, translator, painter and layer, owns the threads and the capture, and feeds every new frame to
//! [`Live::on_frame`] (the cheap watch: changed patches are hidden in that same call) and every finished
//! [`Job`] to [`Live::complete`]. Pure and deterministic: the clock is the frame's `at`.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

mod cache;
mod pixels;
#[cfg(test)]
mod tests;

use cache::Lru;

pub type RegionId = u64;

/// A rectangle in frame pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
    pub const fn right(&self) -> u32 {
        self.x + self.w
    }
    pub const fn bottom(&self) -> u32 {
        self.y + self.h
    }
    pub const fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
    /// The area both cover.
    pub fn overlap(&self, o: &Rect) -> u64 {
        let w = self.right().min(o.right()).saturating_sub(self.x.max(o.x));
        let h = self
            .bottom()
            .min(o.bottom())
            .saturating_sub(self.y.max(o.y));
        w as u64 * h as u64
    }
    pub fn union(&self, o: &Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }
    /// Mostly the same place: they share more than half of the smaller one.
    fn same_place(&self, o: &Rect) -> bool {
        2 * self.overlap(o) > self.area().min(o.area())
    }
}

/// One captured frame, RGBA, `width * height * 4` bytes. `at` = when it was captured (any fixed origin).
pub struct Frame<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub at: Duration,
}

/// Finds text boxes inside `areas` of `frame` (OpenCircle: PP-OCR detection).
pub trait Detector {
    fn detect(&mut self, frame: &Frame, areas: &[Rect]) -> Vec<Rect>;
}

/// Reads each crop (one batch = one recognition call); one text per crop, same order, "" = no text.
pub trait Reader {
    fn read(&mut self, crops: &[Crop]) -> Vec<String>;
}

/// Translates one line (the host wraps its pair and glossary in it).
pub trait Translator {
    fn translate(&mut self, text: &str) -> Result<String, String>;
}

/// Erases `region` and draws `text` in its place: the patch's own bounds and its RGBA pixels
/// (`w * h * 4`); None = nothing to draw.
pub trait Painter {
    fn patch(&mut self, frame: &Frame, region: Rect, text: &str) -> Option<(Rect, Vec<u8>)>;
}

/// The click-through layer over the game. `show` puts a patch up fully transparent, `alpha` fades it
/// (0..1), `hide` takes it down at once, `clear` takes all down. A shown patch's pixels never change.
pub trait Layer {
    fn show(&mut self, id: RegionId, at: Rect, rgba: &[u8]);
    fn alpha(&mut self, id: RegionId, alpha: f32);
    fn hide(&mut self, id: RegionId);
    fn clear(&mut self);
}

/// The thresholds and timings (defaults: live-approach.md 5.1-5.4).
#[derive(Debug, Clone)]
pub struct Config {
    /// Frame pixels per signature cell side: 4 = the 1/4-scale signatures of full frames; 1 when the
    /// host hands frames already scaled down.
    pub scale: u32,
    /// Text area changed: mean absolute gray difference over its cells above this (0..255).
    pub change: f32,
    /// Ring (the box around the text) held: at most this share of its cells moved (0..1).
    pub ring: f32,
    /// The ring's width in frame pixels.
    pub ring_px: u32,
    /// A read region's ring moved at all (more than this share of its cells, still within `ring`):
    /// its box is see-through over a moving scene, and that seeps into the text area. A few cells
    /// moving a lot then means no new text unless `seep_cells` or more moved more than 4x `change`
    /// (measured on the harness' talk clip decoded at 720p: video noise through the subtitle box
    /// moved 2-8 cells over 2x `change` on unchanged lines, 0-5 over 4x; a new letter moves several
    /// over 4x with the ring still).
    pub seep: f32,
    pub seep_cells: u32,
    /// Settled = this many frames in a row without a change.
    pub settle_frames: u8,
    /// Read anyway after this long of changing (a patch still needs the text unchanged since the read).
    pub settle_max: Duration,
    pub fade_in: Duration,
    /// The discover step's period while frames arrive.
    pub discover_every: Duration,
    /// Crops per read batch (one batch in flight at a time).
    pub read_batch: usize,
    /// Translation cache entries (text -> translation).
    pub cache_texts: usize,
    /// Text-signature cache entries (signature -> recognized text).
    pub cache_signatures: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: 4,
            change: 10.0,
            ring: 0.25,
            ring_px: 8,
            seep: 0.02,
            seep_cells: 6,
            settle_frames: 3,
            settle_max: Duration::from_millis(400),
            fade_in: Duration::from_millis(120),
            discover_every: Duration::from_millis(250),
            read_batch: 8,
            cache_texts: 2000,
            cache_signatures: 512,
        }
    }
}

/// Where a region is (live-approach.md 5.3). A region whose text changes after it was read leaves the list
/// at once (its patch hidden in that frame): the discover step finds the new text as a new region.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// New text, waiting to settle.
    Changing,
    Reading {
        job: u64,
    },
    Translating {
        job: u64,
    },
    /// The patch is up since `since` (fading in for `fade_in`, then still).
    Shown {
        since: Duration,
    },
    /// Read, nothing drawn: no translation, the painter declined, or no held box behind it.
    Idle,
}

/// One crop to read: region `id`'s pixels at `at`, read for `job`.
#[derive(Debug, Clone)]
pub struct Crop {
    pub id: RegionId,
    pub job: u64,
    pub at: Rect,
    pub rgba: Vec<u8>,
}

/// Work the host runs off the frame thread; its result comes back through [`Live::complete`]. Every
/// job must be completed (a dropped one stalls that queue until [`Live::stop`]).
#[derive(Debug, Clone)]
pub enum Job {
    /// Find text boxes in `areas` of the frame this job came with.
    Detect {
        job: u64,
        areas: Vec<Rect>,
    },
    Read(Vec<Crop>),
    Translate {
        id: RegionId,
        job: u64,
        text: String,
    },
}

#[derive(Debug, Clone)]
pub enum Done {
    Detected {
        job: u64,
        boxes: Vec<Rect>,
    },
    Read {
        id: RegionId,
        job: u64,
        text: String,
    },
    Translated {
        id: RegionId,
        job: u64,
        text: Result<String, String>,
    },
}

impl Job {
    /// Runs this job on the host's engines (`frame` = the frame it came with).
    pub fn run(
        self,
        frame: &Frame,
        detector: &mut dyn Detector,
        reader: &mut dyn Reader,
        translator: &mut dyn Translator,
    ) -> Vec<Done> {
        match self {
            Job::Detect { job, areas } => vec![Done::Detected {
                job,
                boxes: detector.detect(frame, &areas),
            }],
            Job::Read(crops) => {
                let mut texts = reader.read(&crops).into_iter();
                (crops.iter())
                    .map(|c| Done::Read {
                        id: c.id,
                        job: c.job,
                        text: texts.next().unwrap_or_default(),
                    })
                    .collect()
            }
            Job::Translate { id, job, text } => {
                vec![Done::Translated {
                    id,
                    job,
                    text: translator.translate(&text),
                }]
            }
        }
    }
}

struct Region {
    id: RegionId,
    rect: Rect,
    born: Duration,
    state: State,
    /// Text-area and ring cells on the last frame, and how many frames in a row each stayed still.
    sig: Vec<u8>,
    ring: Vec<u8>,
    still: u8,
    ring_still: u8,
    /// The text-area and ring cells when it was read (what "unchanged" means from then on).
    reference: Vec<u8>,
    ring_ref: Vec<u8>,
    /// The shown patch: its bounds, and the cells as a capture that includes our layer shows them
    /// (X11, Wayland: "shows our patch" counts as unchanged).
    patch: Option<Rect>,
    ours: (Vec<u8>, Vec<u8>),
    alpha: f32,
    prev_alpha: f32,
}

/// The host's reports, for the back-off (5.6): the core slows the discover step, never the watch.
#[derive(Default)]
struct Backoff {
    /// First-minute burst times (sum, count), and how many bursts in a row ran ~2x slower since.
    base: (Duration, u32),
    slow_run: u8,
    slow: bool,
    busy_since: Option<Duration>,
    paused: bool,
}

/// One live session's state. Feed it every new frame ([`Live::on_frame`]) and every finished job
/// ([`Live::complete`], with the newest frame).
pub struct Live {
    cfg: Config,
    regions: Vec<Region>,
    next_id: RegionId,
    next_job: u64,
    size: (u32, u32),
    start: Option<Duration>,
    tiles: Vec<u64>,
    last_discover: Option<Duration>,
    detecting: Option<u64>,
    discover_now: bool,
    /// Rects of regions retired since the detect in flight was asked for (its boxes there are old).
    retired: Vec<Rect>,
    /// Read jobs in flight -> the signature they were read from.
    reading: HashMap<u64, u64>,
    /// Translate jobs in flight -> their source text.
    translating: HashMap<u64, String>,
    signatures: Lru<u64, String>,
    translations: Lru<String, String>,
    backoff: Backoff,
}

impl Live {
    pub fn new(cfg: Config) -> Self {
        Self {
            signatures: Lru::new(cfg.cache_signatures),
            translations: Lru::new(cfg.cache_texts),
            cfg,
            regions: Vec::new(),
            next_id: 0,
            next_job: 0,
            size: (0, 0),
            start: None,
            tiles: Vec::new(),
            last_discover: None,
            detecting: None,
            discover_now: false,
            retired: Vec::new(),
            reading: HashMap::new(),
            translating: HashMap::new(),
            backoff: Backoff::default(),
        }
    }

    /// EVERY new frame: the cheap per-region check. A changed patch is hidden on `layer` in this call;
    /// fades advance; settled text is queued for reading (or drawn at once from the caches); the discover
    /// step runs on its period. Returns the jobs to start; never blocks.
    pub fn on_frame(
        &mut self,
        frame: &Frame,
        painter: &mut dyn Painter,
        layer: &mut dyn Layer,
    ) -> Vec<Job> {
        if (frame.rgba.len() as u64) < frame.width as u64 * frame.height as u64 * 4 {
            return Vec::new();
        }
        if self.size != (frame.width, frame.height) {
            self.stop(layer);
            self.size = (frame.width, frame.height);
        }
        self.start.get_or_insert(frame.at);
        let (s, ringc, settle) = (
            self.cfg.scale.max(1),
            self.ring_cells(),
            self.cfg.settle_frames,
        );
        let mut jobs = Vec::new();

        // the watch: every tracked region against its reference
        let mut i = 0;
        while i < self.regions.len() {
            let r = &mut self.regions[i];
            let (sig, ring) = pixels::sample(frame, r.rect, s, ringc);
            let ring_moved = pixels::moved(&ring, &r.ring, None, self.cfg.change);
            r.ring_still = if ring_moved <= self.cfg.ring {
                r.ring_still.saturating_add(1)
            } else {
                0
            };
            if r.state == State::Changing {
                let still = !pixels::changed(&sig, &r.sig, None, self.cfg.change);
                r.still = if still { r.still.saturating_add(1) } else { 0 };
            } else {
                let (changed, drift) = self.against_reference(i, &sig, &ring);
                if changed || drift > self.cfg.ring {
                    self.retire(i, layer);
                    continue;
                }
            }
            let r = &mut self.regions[i];
            (r.sig, r.ring) = (sig, ring);
            if let State::Shown { since } = r.state
                && r.alpha < 1.0
            {
                let t = if self.cfg.fade_in.is_zero() {
                    1.0
                } else {
                    (frame.at.saturating_sub(since).as_secs_f32() / self.cfg.fade_in.as_secs_f32())
                        .min(1.0)
                };
                r.prev_alpha = r.alpha;
                r.alpha = 1.0 - (1.0 - t).powi(3);
                layer.alpha(r.id, r.alpha);
            }
            i += 1;
        }

        // settled (or changing too long) text: drop it where no box holds it, else read it
        let mut queue = Vec::new();
        let mut i = 0;
        while i < self.regions.len() {
            let r = &self.regions[i];
            let settled = r.still >= settle;
            if r.state != State::Changing
                || !(settled || frame.at.saturating_sub(r.born) >= self.cfg.settle_max)
            {
                i += 1;
                continue;
            }
            if r.ring_still < settle {
                // text on a moving background: never patched (live-approach.md 5.2)
                self.regions.remove(i);
                continue;
            }
            let key = hash(&r.sig);
            if let Some(text) = self.signatures.get(&key) {
                let r = &mut self.regions[i];
                (r.reference, r.ring_ref) = (r.sig.clone(), r.ring.clone());
                match self.known_text(i, text, frame, painter, layer) {
                    Some(Some(job)) => jobs.push(job),
                    Some(None) => {}
                    None => continue, // removed
                }
            } else {
                queue.push((i, settled, r.born, r.rect.area(), key));
            }
            i += 1;
        }
        if self.reading.is_empty() && !queue.is_empty() {
            // settled first, then newest, then smallest (5.4)
            queue.sort_by_key(|&(_, settled, born, area, _)| {
                (!settled, std::cmp::Reverse(born), area)
            });
            let crops = (queue.into_iter().take(self.cfg.read_batch))
                .map(|(i, _, _, _, key)| {
                    let job = self.job();
                    let r = &mut self.regions[i];
                    r.state = State::Reading { job };
                    (r.reference, r.ring_ref) = (r.sig.clone(), r.ring.clone());
                    self.reading.insert(job, key);
                    Crop {
                        id: r.id,
                        job,
                        at: r.rect,
                        rgba: crop(frame, r.rect),
                    }
                })
                .collect();
            jobs.push(Job::Read(crops));
        }

        if let Some(job) = self.discover(frame) {
            jobs.push(job);
        }
        jobs
    }

    /// A job finished (`frame` = the newest frame: the last one given to [`Live::on_frame`] or a later
    /// one). A result for a region that changed since its job started, in either, is dropped (it still
    /// fills the caches); otherwise the next step runs:
    /// read -> translate -> paint + show. Returns the jobs to start.
    pub fn complete(
        &mut self,
        done: Done,
        frame: &Frame,
        painter: &mut dyn Painter,
        layer: &mut dyn Layer,
    ) -> Vec<Job> {
        if (frame.width, frame.height) != self.size {
            return Vec::new();
        }
        match done {
            Done::Detected { job, boxes } => {
                if self.detecting != Some(job) {
                    return Vec::new();
                }
                self.detecting = None;
                let retired = std::mem::take(&mut self.retired);
                for b in boxes {
                    let b = clamp(b, self.size);
                    if b.area() == 0 || retired.iter().any(|r| r.same_place(&b) || b.same_place(r))
                    {
                        continue;
                    }
                    match self.regions.iter().position(|r| r.rect.same_place(&b)) {
                        // still changing (typewriter text growing): its newest box
                        Some(i) if self.regions[i].state == State::Changing => {
                            self.regions[i].rect = b;
                            self.reset_cells(i, frame);
                        }
                        Some(_) => {}
                        None => self.add(b, frame),
                    }
                }
                Vec::new()
            }
            Done::Read { id, job, text } => {
                let Some(key) = self.reading.remove(&job) else {
                    return Vec::new();
                };
                self.signatures.put(key, text.clone());
                match self.current(id, &State::Reading { job }, frame) {
                    Some(i) => self
                        .known_text(i, text, frame, painter, layer)
                        .flatten()
                        .into_iter()
                        .collect(),
                    None => Vec::new(),
                }
            }
            Done::Translated { id, job, text } => {
                let Some(source) = self.translating.remove(&job) else {
                    return Vec::new();
                };
                if let Ok(t) = &text {
                    self.translations.put(source, t.clone());
                }
                if let Some(i) = self.current(id, &State::Translating { job }, frame) {
                    match text {
                        Ok(t) => self.paint(i, &t, frame, painter, layer),
                        Err(_) => self.regions[i].state = State::Idle,
                    }
                }
                Vec::new()
            }
        }
    }

    /// The tracked regions (for the controller and tests).
    pub fn regions(&self) -> impl Iterator<Item = (RegionId, Rect, &State)> {
        self.regions.iter().map(|r| (r.id, r.rect, &r.state))
    }

    /// Ends the session's regions: every patch down, jobs in flight forgotten. The caches stay.
    pub fn stop(&mut self, layer: &mut dyn Layer) {
        if !self.regions.is_empty() {
            layer.clear();
        }
        self.regions.clear();
        self.tiles.clear();
        (self.last_discover, self.detecting, self.discover_now) = (None, None, false);
        self.retired.clear();
        self.reading.clear();
        self.translating.clear();
    }

    /// The translations no longer hold (another language pair, an edited glossary).
    pub fn forget_translations(&mut self) {
        self.translations.clear();
    }

    /// The host timed one OCR / MT burst. Returns true once, when bursts have run ~2x slower than the
    /// first minute's mean 3 times in a row: the host backs off (GPU -> CPU path); the discover step
    /// slows to 1 Hz from then on.
    pub fn report_burst(&mut self, took: Duration, at: Duration) -> bool {
        let b = &mut self.backoff;
        if at.saturating_sub(self.start.unwrap_or(at)) <= Duration::from_secs(60) {
            b.base = (b.base.0 + took, b.base.1 + 1);
            return false;
        }
        if b.base.1 == 0 || b.slow {
            return false;
        }
        let slow = took > 2 * b.base.0 / b.base.1;
        b.slow_run = if slow { b.slow_run + 1 } else { 0 };
        b.slow = b.slow_run >= 3;
        b.slow
    }

    /// The host's total CPU load (0..1): above 85 % for 2 s pauses the discover step until it is under
    /// 70 %. The watch never pauses.
    pub fn report_cpu(&mut self, load: f32, at: Duration) {
        let b = &mut self.backoff;
        if load > 0.85 {
            let since = *b.busy_since.get_or_insert(at);
            b.paused |= at.saturating_sub(since) >= Duration::from_secs(2);
        } else {
            b.busy_since = None;
            if load < 0.70 {
                b.paused = false;
            }
        }
    }

    fn job(&mut self) -> u64 {
        self.next_job += 1;
        self.next_job
    }

    fn ring_cells(&self) -> u32 {
        self.cfg.ring_px.div_ceil(self.cfg.scale.max(1))
    }

    /// A read region now: (its text area changed, share of ring cells moved) against what it looked like
    /// when read, or, when its patch is up, against how a capture with our layer in it shows the patch
    /// (during the fade: at the last two alphas). Unchanged against any of them counts as unchanged.
    fn against_reference(&self, i: usize, sig: &[u8], ring: &[u8]) -> (bool, f32) {
        let r = &self.regions[i];
        let change = self.cfg.change;
        let mut drift = pixels::moved(ring, &r.ring_ref, None, change);
        // (with the ring moving, a few strong cells are the scene through the box)
        let strong = match drift > self.cfg.seep {
            true => (4.0 * change, self.cfg.seep_cells),
            false => (2.0 * change, 2),
        };
        let mut changed = pixels::changed_by(sig, &r.reference, None, change, strong);
        if r.patch.is_some() && (changed || drift > self.cfg.ring) {
            let mut alphas = vec![1.0, r.alpha, r.prev_alpha];
            alphas.dedup();
            for t in alphas {
                changed &=
                    pixels::changed_by(sig, &r.reference, Some((&r.ours.0, t)), change, strong);
                drift = drift.min(pixels::moved(
                    ring,
                    &r.ring_ref,
                    Some((&r.ours.1, t)),
                    change,
                ));
            }
        }
        (changed, drift)
    }

    /// Region `i`'s text changed after it was read: its patch goes now, it leaves the list, and the next
    /// discover runs at once (the new text's box may differ).
    fn retire(&mut self, i: usize, layer: &mut dyn Layer) {
        let r = self.regions.remove(i);
        if r.patch.is_some() {
            layer.hide(r.id);
        }
        self.retired
            .push(r.patch.map_or(r.rect, |p| p.union(&r.rect)));
        self.discover_now = true;
    }

    /// Region `i`'s text is known: drawn now from the translation cache, or a translate job. None = the
    /// region left (no text there); Some(None) = nothing to start.
    fn known_text(
        &mut self,
        i: usize,
        text: String,
        frame: &Frame,
        painter: &mut dyn Painter,
        layer: &mut dyn Layer,
    ) -> Option<Option<Job>> {
        if text.trim().is_empty() {
            self.regions.remove(i);
            return None;
        }
        if let Some(t) = self.translations.get(&text) {
            self.paint(i, &t, frame, painter, layer);
            return Some(None);
        }
        let job = self.job();
        let r = &mut self.regions[i];
        r.state = State::Translating { job };
        self.translating.insert(job, text.clone());
        Some(Some(Job::Translate {
            id: r.id,
            job,
            text,
        }))
    }

    /// Draws region `i`'s translation once (it fades in from `on_frame`), or leaves it Idle.
    fn paint(
        &mut self,
        i: usize,
        t: &str,
        frame: &Frame,
        painter: &mut dyn Painter,
        layer: &mut dyn Layer,
    ) {
        let (s, ringc) = (self.cfg.scale.max(1), self.ring_cells());
        let held = self.regions[i].ring_still >= self.cfg.settle_frames;
        let r = &mut self.regions[i];
        r.state = State::Idle;
        if t.trim().is_empty() || !held {
            return;
        }
        let Some((at, rgba)) = painter.patch(frame, r.rect, t) else {
            return;
        };
        if (rgba.len() as u64) < at.area() * 4 || at.area() == 0 {
            return;
        }
        layer.show(r.id, at, &rgba);
        // the cells as a capture with our layer in it shows them: the patch where it covers, else the
        // read-time cells
        let (mut text, mut ring) = (Vec::new(), Vec::new());
        let (mut ti, mut ri) = (0, 0);
        let patch = Frame {
            rgba: &rgba,
            width: at.w,
            height: at.h,
            at: frame.at,
        };
        let size = self.size;
        pixels::walk(r.rect, s, ringc, size, |cx, cy, in_ring| {
            let read = if in_ring {
                r.ring_ref[ri]
            } else {
                r.reference[ti]
            };
            let v = patch_cell(&patch, at, s, (cx, cy), size, read);
            if in_ring {
                ring.push(v);
                ri += 1;
            } else {
                text.push(v);
                ti += 1;
            }
        });
        r.ours = (text, ring);
        r.patch = Some(at);
        r.state = State::Shown { since: frame.at };
        (r.prev_alpha, r.alpha) = (0.0, 0.0);
        if self.cfg.fade_in.is_zero() {
            r.alpha = 1.0;
            layer.alpha(r.id, 1.0);
        }
    }

    /// The region `id` if it is still in `state` and its text still looks as when read.
    fn current(&self, id: RegionId, state: &State, frame: &Frame) -> Option<usize> {
        let i = self
            .regions
            .iter()
            .position(|r| r.id == id && r.state == *state)?;
        let (sig, ring) = pixels::sample(
            frame,
            self.regions[i].rect,
            self.cfg.scale.max(1),
            self.ring_cells(),
        );
        let (changed, drift) = self.against_reference(i, &sig, &ring);
        (!changed && drift <= self.cfg.ring).then_some(i)
    }

    fn add(&mut self, rect: Rect, frame: &Frame) {
        self.next_id += 1;
        self.regions.push(Region {
            id: self.next_id,
            rect,
            born: frame.at,
            state: State::Changing,
            sig: Vec::new(),
            ring: Vec::new(),
            still: 0,
            ring_still: 0,
            reference: Vec::new(),
            ring_ref: Vec::new(),
            patch: None,
            ours: (Vec::new(), Vec::new()),
            alpha: 0.0,
            prev_alpha: 0.0,
        });
        self.reset_cells(self.regions.len() - 1, frame);
    }

    fn reset_cells(&mut self, i: usize, frame: &Frame) {
        let (s, ringc) = (self.cfg.scale.max(1), self.ring_cells());
        let r = &mut self.regions[i];
        (r.sig, r.ring) = pixels::sample(frame, r.rect, s, ringc);
        (r.still, r.ring_still) = (0, 0);
    }

    /// The discover step (5.1): on its period (or at once after a retire), the frame's tiles at 1/scale
    /// with the read regions masked; the changed ones -> areas to detect text in. One detect in flight.
    fn discover(&mut self, frame: &Frame) -> Option<Job> {
        let b = &self.backoff;
        if self.detecting.is_some() || b.paused {
            return None;
        }
        let every = if b.slow {
            self.cfg.discover_every.max(Duration::from_secs(1))
        } else {
            self.cfg.discover_every
        };
        let due = self
            .last_discover
            .is_none_or(|t| frame.at.saturating_sub(t) >= every);
        if !due && !(self.discover_now && !b.slow) {
            return None;
        }
        let s = self.cfg.scale.max(1);
        let masked: Vec<Rect> = (self.regions.iter())
            .filter(|r| r.state != State::Changing)
            .map(|r| r.patch.map_or(r.rect, |p| p.union(&r.rect)))
            .collect();
        let tiles = pixels::tile_hashes(frame, s, &masked);
        let changed: Vec<bool> = if tiles.len() == self.tiles.len() {
            tiles.iter().zip(&self.tiles).map(|(a, b)| a != b).collect()
        } else {
            vec![true; tiles.len()]
        };
        (self.tiles, self.last_discover, self.discover_now) = (tiles, Some(frame.at), false);
        let areas = pixels::clusters(&changed, self.size, s);
        if areas.is_empty() {
            return None;
        }
        let job = self.job();
        self.detecting = Some(job);
        self.retired.clear();
        Some(Job::Detect { job, areas })
    }
}

/// Cell (cx, cy) as a capture with the patch in it shows it (`at` = where the patch sits in the
/// frame): sampled exactly as `pixels::cell` samples a frame (the same pixels: a full average differs
/// from the sampled one by tens on fine glyphs, so the watch took our own patch for new text), each
/// sample under the patch from the patch, the others as the cell looked when read (`read`).
fn patch_cell(
    patch: &Frame,
    at: Rect,
    s: u32,
    (cx, cy): (u32, u32),
    (w, h): (u32, u32),
    read: u8,
) -> u8 {
    let step = if s >= 4 { 2 } else { 1 };
    let (x0, y0) = (cx * s, cy * s);
    let (x1, y1) = ((x0 + s).min(w), (y0 + s).min(h));
    let (mut sum, mut n, mut ours) = (0u32, 0u32, 0u32);
    for y in (y0..y1).step_by(step) {
        for x in (x0..x1).step_by(step) {
            n += 1;
            if (at.x..at.right()).contains(&x) && (at.y..at.bottom()).contains(&y) {
                let p = &patch.rgba[(((y - at.y) * at.w + (x - at.x)) * 4) as usize..][..4];
                sum += 77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32;
                ours += 1;
            } else {
                sum += (read as u32) << 8;
            }
        }
    }
    if ours == 0 {
        return read;
    }
    ((sum / n.max(1)) >> 8) as u8
}

fn crop(f: &Frame, r: Rect) -> Vec<u8> {
    let mut out = Vec::with_capacity(r.area() as usize * 4);
    for y in r.y..r.bottom() {
        out.extend_from_slice(
            &f.rgba[((y * f.width + r.x) * 4) as usize..((y * f.width + r.right()) * 4) as usize],
        );
    }
    out
}

fn clamp(r: Rect, (w, h): (u32, u32)) -> Rect {
    let (x, y) = (r.x.min(w), r.y.min(h));
    Rect::new(x, y, r.w.min(w - x), r.h.min(h - y))
}

fn hash(cells: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    cells.hash(&mut h);
    h.finish()
}
