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
    /// `o` makes it wider by more than half its height (more of its line).
    fn grows_by(&self, o: &Rect) -> bool {
        2 * (self.union(o).w - self.w) > self.h
    }
    /// Parts of one line: they share at least half of the lower one's height and are at most that
    /// height apart side by side (a word space is about a third of it).
    fn same_line(&self, o: &Rect) -> bool {
        let h = self.h.min(o.h);
        let rows = self
            .bottom()
            .min(o.bottom())
            .saturating_sub(self.y.max(o.y));
        let gap = self.x.max(o.x).saturating_sub(self.right().min(o.right()));
        h > 0 && 2 * rows >= h && gap <= h
    }
    /// Grown by `p` on every side (from 0).
    fn padded(&self, p: u32) -> Rect {
        let (x, y) = (self.x.saturating_sub(p), self.y.saturating_sub(p));
        Rect::new(x, y, self.right() + p - x, self.bottom() + p - y)
    }
    /// `o` is the next line under this one: side by side they overlap, and it starts below this one's
    /// middle, at most two of its heights under it.
    fn just_above(&self, o: &Rect) -> bool {
        let side = self.right().min(o.right()) > self.x.max(o.x);
        side && o.y >= self.y + self.h / 2 && o.y <= self.bottom() + 2 * self.h
    }
}

/// One captured frame, RGBA, `width * height * 4` bytes. `at` = when it was captured (any fixed origin).
pub struct Frame<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub at: Duration,
}

/// A text box found: its bounds and its lines' (top to bottom; one line = `[at]`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextBox {
    pub at: Rect,
    pub lines: Vec<Rect>,
}

impl From<Rect> for TextBox {
    fn from(at: Rect) -> Self {
        TextBox {
            at,
            lines: vec![at],
        }
    }
}

/// Finds text boxes inside `areas` of `frame` (OpenCircle: PP-OCR detection).
pub trait Detector {
    fn detect(&mut self, frame: &Frame, areas: &[Rect]) -> Vec<TextBox>;
}

/// How a region's text was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// In a new place: the discover step's detect.
    New,
    /// In a box read before, its panel still there: read again in place (no detect over it).
    Box,
    /// A box whose text was typed out letter by letter: read, translated and drawn once, as a whole,
    /// when its last letter came (never a part of it: a part of a sentence misleads).
    Typed,
}

/// What the host logs (the harness reads it): a patch drawn, with its text's way through the stages
/// (frame times, `Frame::at`), or a text that left the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    Drawn {
        id: RegionId,
        how: How,
        /// The change was seen (the discover that asked for the detect; a known box: the frame it changed).
        seen: Duration,
        detected: Option<Duration>,
        /// It held still: read asked (or a cache answered).
        settled: Duration,
        read: Duration,
        translated: Duration,
        shown: Duration,
    },
    /// A text that was read left (changed, or went): it was on screen `life` (from `seen`), drawn or not;
    /// its box's reveal (typed out or all at once) and the settle time it got.
    Gone {
        id: RegionId,
        life: Duration,
        drawn: bool,
        typed: bool,
        settle: Duration,
    },
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
    /// After a detect that took T the next discover waits this many T more (a detect over the moving
    /// scene finds nothing and runs ~300 ms: back to back it kept the OCR busy 60 % of the time on the
    /// fight clip). 0 = the period alone. A retire still asks for a discover at once.
    pub discover_rest: u32,
    /// A discover tile changed / held still: the mean gray difference of its cells above / at most this
    /// (0..255; video noise moves a still tile by ~1, new text by tens).
    pub tile_change: f32,
    /// A changed tile is looked for text only once it held still over this long before the discover
    /// (text settles, a moving background or effect never does: on the fight and talk clips a detect
    /// over the whole moving frame ran back to back and took most of the CPU); 0 = never waits.
    pub tile_still: Duration,
    /// Crops per read batch (one batch in flight at a time).
    pub read_batch: usize,
    /// Translation cache entries (text -> translation).
    pub cache_texts: usize,
    /// Text-signature cache entries (signature -> recognized text).
    pub cache_signatures: usize,
    /// A line being typed out is finished once no new letter came for its box's settle time: learned
    /// per box from its pauses between letters (twice their mean, a quarter of its lines' lifetime at
    /// most), within these bounds; the upper one until the box has shown how it types.
    pub typed_settle: (Duration, Duration),
    /// Where a box's next letters land is watched: this many line heights right of its last line, and
    /// one line height under it.
    pub front_lines: u32,
    /// Tell a box typed out letter by letter from its front and read it only once whole. Off: on the
    /// harness's fight clip a scene's shapes still passed for letters (3 of 10 banners lost in the
    /// replay); the box memory and the rest work without it.
    pub typed_out: bool,
    /// Box memory: a text box whose text changes with its panel held is read again in place, with no
    /// detect over it. Off (lead, 2026-11-17): its in-place re-read pays a full OCR of the crop and lost
    /// lines on the Windows VM (fast clip 56 -> 29 of 66 drawn); on once that read is cheap.
    pub box_memory: bool,
    /// The frames show our layer (X11 screen capture, the capturable test hook): a shown patch's cells
    /// count as unchanged. False where captures never show it (Windows, a game window's own pixels):
    /// there "looks like our patch" is only the game's own pixels (an emptied panel looks like our
    /// erased box), and taking it for our patch kept stale patches up (W28). Set per frame by the host
    /// ([`Live::layer_seen`]).
    pub layer_seen: bool,
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
            discover_rest: 1,
            tile_change: 3.0,
            tile_still: Duration::from_millis(100),
            read_batch: 8,
            cache_texts: 2000,
            cache_signatures: 512,
            typed_settle: (Duration::from_millis(50), Duration::from_millis(300)),
            front_lines: 4,
            typed_out: false,
            box_memory: false,
            layer_seen: true,
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
        boxes: Vec<TextBox>,
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
    /// Frames since each text-area and ring cell last moved (while it settles), and from the read on
    /// the cells that were still moving then: the scene beside or through the box (a detect box
    /// reaches past a tight HUD bar or banner, so its edge cells and ring see the fight behind it),
    /// left out of "unchanged" (`against_reference`).
    age: (Vec<u8>, Vec<u8>),
    ring_moved: Vec<bool>,
    scene: (Vec<bool>, Vec<bool>),
    /// The shown patch: its bounds, and the cells as a capture that includes our layer shows them
    /// (X11, Wayland: "shows our patch" counts as unchanged).
    patch: Option<Rect>,
    ours: (Vec<u8>, Vec<u8>),
    alpha: f32,
    prev_alpha: f32,
    how: How,
    stages: Stages,
    /// Its lines as last found (frame px, top to bottom).
    lines: Vec<Rect>,
    /// A box read before whose text changed with its panel still there: read again in place, the
    /// discover step leaves it alone (box memory).
    known: bool,
    /// Its text is being typed out: read once no new letter came for the box's settle time.
    typing: bool,
    /// When the last letter landed (while typing).
    letter_at: Duration,
    /// When its text area last changed (while it settles).
    moved_at: Duration,
    /// Frames in a row (each soon after the last) with letters landing at its front.
    hits: u8,
    /// Right of its last line, and under it (where the next line starts).
    front: [Front; 2],
    style: Style,
    /// A patch went up for it at least once (a text box: box memory keeps it).
    boxed: bool,
    /// Its patch went up for the current text.
    drawn: bool,
    /// The translation its patch shows ("" = none).
    said: String,
}

/// The stage times of a region's current text (`Note::Drawn`).
#[derive(Debug, Clone, Copy, Default)]
struct Stages {
    seen: Duration,
    detected: Option<Duration>,
    settled: Option<Duration>,
    read: Option<Duration>,
    translated: Option<Duration>,
}

/// How a box shows its text, learned while Live runs (no game list, no setting): typed out or all at
/// once, the longest pause between letters of its last typed line, its lines' lifetime.
#[derive(Debug, Clone, Copy, Default)]
struct Style {
    typed: bool,
    gap: Duration,
    life: Option<Duration>,
}

/// Where the next letters of a box land (a band right of its last line, or under it), cell by cell:
/// the gray at the start, now, when it last jumped (or the band started), whether it held still before
/// that jump, and whether that jump was counted as a letter.
#[derive(Debug, Clone, Default)]
struct Front {
    at: Rect,
    /// Cell coordinates of `at` (x0, y0, columns).
    cells: (u32, u32, u32),
    first: Vec<u8>,
    now: Vec<u8>,
    jump: Vec<Duration>,
    calm: Vec<bool>,
    counted: Vec<bool>,
    /// The panel's gray (the ring's median at the start): only cells that showed it can get a letter;
    /// none where the ring is no panel (`flat`: most of it within `FLAT` of that gray).
    panel: u8,
    flat: bool,
    /// Columns of the band on the panel (past them: the scene beyond its edge).
    reach: u32,
    /// A letter lands at most this far (px) past the last one (or the band's start).
    near: u32,
    /// The rightmost letter seen in it (frame px), 0 = none.
    right: u32,
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
    /// The discover step's grid of cells (each tile as it last held still), and a grid taken
    /// `tile_still` before the next discover (with its time).
    tiles: Vec<u8>,
    before: Option<(Duration, Vec<u8>)>,
    last_discover: Option<Duration>,
    /// How long the last detect took (from its start to its result).
    took: Duration,
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
    /// When the detect in flight was asked for.
    asked: Duration,
    notes: Vec<Note>,
    /// The styles of boxes that left (a new region in the same place takes it over).
    styles: Vec<(Rect, Style)>,
    /// Places whose patch went because the scene moved through its text (no panel round it), with
    /// what it said and when: the same text there again is not drawn again (a redraw of unchanged
    /// text: a static line over a moving 3D scene was shown 27 times in 36 s, W28).
    moving: Vec<(Rect, String, Duration)>,
}

/// Places of patches the scene moved through, kept; and for how long the same text there isn't drawn.
const MOVING_KEPT: usize = 16;
const MOVING_FOR: Duration = Duration::from_secs(10);

/// Styles of boxes that left, kept for a new region in their place.
const STYLES_KEPT: usize = 32;

impl Live {
    pub fn new(cfg: Config) -> Self {
        Self {
            asked: Duration::ZERO,
            notes: Vec::new(),
            styles: Vec::new(),
            signatures: Lru::new(cfg.cache_signatures),
            translations: Lru::new(cfg.cache_texts),
            cfg,
            regions: Vec::new(),
            next_id: 0,
            next_job: 0,
            size: (0, 0),
            start: None,
            tiles: Vec::new(),
            before: None,
            last_discover: None,
            took: Duration::ZERO,
            detecting: None,
            discover_now: false,
            retired: Vec::new(),
            moving: Vec::new(),
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
            // (the scene beside the box left out once known: `take_reference`; before that, the
            // side of the ring that moved most: a padded detect box reaches past its panel's edge on
            // one side, as a speaker name's or a HUD label's often does)
            let ring_moved = match r.scene.1.len() == ring.len() {
                true => pixels::moved(
                    &keep(&ring, &r.scene.1),
                    &keep(&r.ring, &r.scene.1),
                    None,
                    self.cfg.change,
                ),
                false => {
                    let sides = pixels::ring_sides(r.rect, s, ringc, (frame.width, frame.height));
                    pixels::moved_but_one_side(&ring, &r.ring, &sides, self.cfg.change)
                }
            };
            r.ring_still = if ring_moved <= self.cfg.ring {
                r.ring_still.saturating_add(1)
            } else {
                0
            };
            if r.state == State::Changing {
                let still = !pixels::changed(&sig, &r.sig, None, self.cfg.change);
                r.still = if still { r.still.saturating_add(1) } else { 0 };
                let change = self.cfg.change;
                let older = |age: &mut Vec<u8>, now: &[u8], was: &[u8]| {
                    age.resize(now.len(), 0);
                    for ((a, n), w) in age.iter_mut().zip(now).zip(was) {
                        *a = match (*n as f32 - *w as f32).abs() > change {
                            true => 0,
                            false => a.saturating_add(1),
                        };
                    }
                };
                older(&mut r.age.0, &sig, &r.sig);
                older(&mut r.age.1, &ring, &r.ring);
                for (m, a) in r.ring_moved.iter_mut().zip(&r.age.1) {
                    *m |= *a == 0;
                }
                // while it types, its pauses between changes teach its box's settle time
                if !still {
                    let gap = frame.at.saturating_sub(r.moved_at);
                    if r.typing && gap < Duration::from_millis(600) {
                        r.style.gap = gap_mean(r.style.gap, gap);
                    }
                    r.moved_at = frame.at;
                    // (a new text typed into a box read before lands inside it, its panel still)
                    if r.known && r.typing && r.ring_still > 0 {
                        r.letter_at = frame.at;
                    }
                }
                // the next letters of a box being typed out land right of its last line or under it
                let letters = r.front[0].step(frame, s, change) + r.front[1].step(frame, s, change);
                if letters > 0 {
                    let gap = frame.at.saturating_sub(r.letter_at);
                    if r.typing && gap < Duration::from_millis(600) {
                        r.style.gap = gap_mean(r.style.gap, gap);
                    }
                    // typing: letters on a panel (the ring round it held still a while) in several
                    // frames close together, as a typewriter adds them; a scene's shape that holds a
                    // moment now and then is no typing (the fight clip: one every ~0.5 s)
                    if letters >= 2 && r.ring_still >= 2 * settle {
                        let close =
                            frame.at.saturating_sub(r.letter_at) <= Duration::from_millis(150);
                        r.hits = if close { r.hits.saturating_add(1) } else { 1 };
                        r.typing |= r.hits >= 4 && self.cfg.typed_out;
                    }
                    r.letter_at = frame.at;
                }
                // a box re-read in place whose new text spills past it (or whose panel went): found
                // by a detect like new text
                if r.known && !self.box_held(i, &ring) {
                    self.regions[i].known = false;
                }
            } else {
                // letters landing beside a box found and read: its line is still being typed out, so
                // what was read is a part of it (drawn, it was left over the growing line, W31). Only
                // with its ring still (a scene starting to move round a box isn't letters), and not
                // for a box the typed-out path or box memory grew itself (its letters are taken in)
                let change = self.cfg.change;
                let r = &mut self.regions[i];
                let grew = r.front.iter_mut().fold(false, |grew, f| {
                    f.step(frame, s, change);
                    grew | f.grew(change)
                }) && r.how == How::New
                    && r.ring_still > 0;
                let (changed, drift) = self.against_reference(i, &sig, &ring);
                let changed = changed || grew;
                // (box memory only for a text box: one whose translation was drawn, a few lines;
                // a detect box over a whole scene is looked for again)
                let boxed = self.cfg.box_memory
                    && self.regions[i].boxed
                    && self.regions[i].lines.len() <= 4;
                if changed && boxed && self.box_held(i, &ring) {
                    self.reuse(i, frame, layer);
                    i += 1;
                    continue;
                }
                if changed || drift > self.cfg.ring {
                    let r = &self.regions[i];
                    // (no panel round it when it was read: a box going away is its text leaving)
                    if r.patch.is_some() && !panel(&r.ring_ref).1 && !self.box_held(i, &ring) {
                        if self.moving.len() >= MOVING_KEPT {
                            self.moving.remove(0);
                        }
                        self.moving.push((r.rect, r.said.clone(), frame.at));
                    }
                    self.retire(i, frame, layer);
                    continue;
                }
                // ring cells that keep moving while the text holds are the scene beside the box from
                // now on (all at once, as a closing box, is a change: above)
                let (r, change) = (&mut self.regions[i], self.cfg.change);
                if r.scene.1.len() == ring.len() && r.ring.len() == ring.len() {
                    for ((s, now), was) in r.scene.1.iter_mut().zip(&ring).zip(&r.ring) {
                        *s |= now.abs_diff(*was) as f32 > change;
                    }
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
            let since = |t: Duration| frame.at.saturating_sub(t);
            // (typed out: no new letter for the box's settle time; a letter landing now: not yet)
            let typed = self.typed_settle(&r.style);
            let settled = r.still >= settle
                && !r.front.iter().any(|f| f.landing(frame.at, self.cfg.change))
                && match r.typing {
                    // (a new text typed into a box read before lands inside it)
                    true => since(r.letter_at) >= typed && since(r.moved_at) >= typed,
                    // (letters landing beside it may be the start of typing: a few letter times)
                    false => since(r.letter_at) >= QUIET,
                };
            let waited = since(r.born) >= self.cfg.settle_max
                && (!r.typing
                    || since(r.letter_at) >= self.cfg.settle_max
                    // (a box that never stops changing is read in the end)
                    || since(r.born) >= TYPED_MAX);
            if r.state != State::Changing || !(settled || waited) {
                i += 1;
                continue;
            }
            if r.typing && self.finish_typing(i, frame) {
                i += 1; // (its box grew to its last letters: it settles again)
                continue;
            }
            let r = &self.regions[i];
            if r.ring_still < settle {
                // text on a moving background: never patched (live-approach.md 5.2)
                self.regions.remove(i);
                continue;
            }
            let key = hash(&r.sig);
            if let Some(text) = self.signatures.get(&key) {
                let cols = self.cols(self.regions[i].rect);
                let r = &mut self.regions[i];
                r.take_reference(settle, settled, cols);
                (r.stages.settled, r.stages.read) = (Some(frame.at), Some(frame.at));
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
                .map(|(i, settled, _, _, key)| {
                    let job = self.job();
                    let cols = self.cols(self.regions[i].rect);
                    let r = &mut self.regions[i];
                    r.state = State::Reading { job };
                    r.take_reference(settle, settled, cols);
                    r.stages.settled = Some(frame.at);
                    self.reading.insert(job, key);
                    // (a box read again in place, or grown to its last letters, is no detect's box:
                    // read with room round it, a recognizer finds no line cut tight at its edges)
                    let at = match r.known || r.how != How::New {
                        true => clamp(r.rect.padded(r.rect.h / 2), (frame.width, frame.height)),
                        false => r.rect,
                    };
                    Crop {
                        id: r.id,
                        job,
                        at,
                        rgba: crop(frame, at),
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
                self.took = frame
                    .at
                    .saturating_sub(self.last_discover.unwrap_or(frame.at));
                let retired = std::mem::take(&mut self.retired);
                let size = self.size;
                for b in boxes {
                    let at = clamp(b.at, size);
                    if at.area() == 0
                        || retired
                            .iter()
                            .any(|r| r.same_place(&at) || at.same_place(r))
                    {
                        continue;
                    }
                    let lines: Vec<Rect> = (b.lines.iter().map(|l| clamp(*l, size)))
                        .filter(|l| l.area() > 0)
                        .collect();
                    self.place(TextBox { at, lines }, frame, layer);
                }
                Vec::new()
            }
            Done::Read { id, job, text } => {
                let Some(key) = self.reading.remove(&job) else {
                    return Vec::new();
                };
                self.signatures.put(key, text.clone());
                match self.current(id, &State::Reading { job }, frame) {
                    Some(i) => {
                        self.regions[i].stages.read = Some(frame.at);
                        (self.known_text(i, text, frame, painter, layer).flatten())
                            .into_iter()
                            .collect()
                    }
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
                    self.regions[i].stages.translated = Some(frame.at);
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

    #[cfg(test)]
    pub fn debug_regions(&self) -> String {
        (self.regions.iter())
            .map(|r| {
                format!(
                    "[{} {:?} {:?} {:?} typing {} known {} still {} ring {} letter {} born {}]",
                    r.id,
                    r.rect,
                    r.state,
                    r.how,
                    r.typing,
                    r.known,
                    r.still,
                    r.ring_still,
                    r.letter_at.as_millis(),
                    r.born.as_millis()
                )
            })
            .collect()
    }

    /// What happened since the last call, for the host's log ([`Note`]).
    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// Box `b` found by a detect: joins the region at its place or on its line, or is a new one.
    fn place(&mut self, b: TextBox, frame: &Frame, layer: &mut dyn Layer) {
        // (the largest: a part of the line may still be tracked beside the whole)
        let place = (0..self.regions.len())
            .filter(|&i| self.regions[i].rect.same_place(&b.at))
            .max_by_key(|&i| self.regions[i].rect.area());
        let line = || self.regions.iter().position(|r| r.rect.same_line(&b.at));
        match (place, place.or_else(line)) {
            // still changing (typewriter text growing), or the rest of a line found earlier in part
            // (its other words settled later): one region for the line (growth alone is no sign of
            // typing: a banner found in pieces, a scene's shapes round it, grow it too; the letters
            // landing at its front are: `Front`)
            (_, Some(i)) if self.regions[i].state == State::Changing => {
                let r = &mut self.regions[i];
                r.rect = r.rect.union(&b.at);
                // (the newest lines: the last one grows while it types)
                if b.lines.len() >= r.lines.len() {
                    r.lines = b.lines;
                }
                self.join_line(i, frame, layer);
            }
            (_, Some(i)) if self.regions[i].rect.grows_by(&b.at) => {
                // (it grew after it was read: typed out)
                let (line, style) = (self.regions[i].rect.union(&b.at), self.regions[i].style);
                self.retire(i, frame, layer);
                self.add(line, frame);
                let k = self.regions.len() - 1;
                (self.regions[k].typing, self.regions[k].style) = (true, style);
                self.join_line(k, frame, layer);
            }
            (None, None) => {
                // the next line of a box being typed out: the box grows (one text, read once whole)
                let above = (0..self.regions.len()).find(|&j| {
                    let t = &self.regions[j];
                    let line = t.lines.iter().map(|l| l.h).min().unwrap_or(t.rect.h);
                    t.typing
                        && t.state == State::Changing
                        && t.rect.just_above(&b.at)
                        && 2 * b.at.h <= 3 * line
                });
                match above {
                    Some(j) => {
                        let r = &mut self.regions[j];
                        r.rect = r.rect.union(&b.at);
                        r.lines.extend(b.lines);
                        r.lines.sort_by_key(|l| l.y);
                        r.letter_at = frame.at;
                        self.join_line(j, frame, layer);
                    }
                    None => {
                        self.add(b.at, frame);
                        let k = self.regions.len() - 1;
                        if b.lines.len() > 1 {
                            self.regions[k].lines = b.lines;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Region `i` was being typed out and is finished: it takes in the letters that landed right of
    /// its last line or under it since it was last found. True when it grew (it settles again before
    /// it is read).
    fn finish_typing(&mut self, i: usize, frame: &Frame) -> bool {
        let w = self.size.0;
        let r = &mut self.regions[i];
        (r.typing, r.style.typed) = (false, true);
        if r.how == How::New {
            r.how = How::Typed;
        }
        let line = r.lines.iter().map(|l| l.h).min().unwrap_or(r.rect.h).max(1);
        // (right of its last line only: a line under it is found by a detect and joins it)
        let right = r.front[0].right;
        let grown = Rect::new(
            r.rect.x,
            r.rect.y,
            (right + line / 4).min(w).max(r.rect.right()) - r.rect.x,
            r.rect.h,
        );
        if grown == r.rect {
            return false;
        }
        // (it settles again from now: read "anyway" only after `settle_max` more)
        (r.rect, r.born) = (grown, frame.at);
        if r.lines.len() <= 1 {
            r.lines = vec![r.rect];
        }
        self.reset_cells(i, frame);
        true
    }

    /// The settle time of a line typed out in a box of this style (`Config::typed_settle`).
    fn typed_settle(&self, style: &Style) -> Duration {
        let (lo, hi) = self.cfg.typed_settle;
        let mut t = match style.gap.is_zero() {
            true => hi,
            false => 2 * style.gap + Duration::from_millis(17),
        };
        if let Some(life) = style.life {
            t = t.min(life / 4);
        }
        t.clamp(lo, hi.max(lo))
    }

    /// Region `i`'s box (its panel) is still there and its text stays inside it: on every side of
    /// its ring at most a sixth of the cells moved since it was read (where a patch is up, the patch
    /// as a capture with our layer shows it counts as unmoved). A side that is all scene can't tell:
    /// not held.
    fn box_held(&self, i: usize, ring: &[u8]) -> bool {
        let r = &self.regions[i];
        let (s, ringc) = (self.cfg.scale.max(1), self.ring_cells());
        let sides = pixels::ring_sides(r.rect, s, ringc, self.size);
        if sides.len() != ring.len() || r.ring_ref.len() != ring.len() {
            return false;
        }
        let (mut all, mut n, mut m) = ([0u32; 4], [0u32; 4], [0u32; 4]);
        for k in 0..ring.len() {
            let side = sides[k] as usize;
            all[side] += 1;
            if r.scene.1.get(k).copied().unwrap_or(false) {
                continue;
            }
            let mut d = ring[k].abs_diff(r.ring_ref[k]);
            if r.patch.is_some()
                && self.cfg.layer_seen
                && let Some(o) = r.ours.1.get(k)
            {
                d = d.min(ring[k].abs_diff(*o));
            }
            n[side] += 1;
            m[side] += (d as f32 > self.cfg.change) as u32;
        }
        (0..4).all(|k| all[k] == 0 || (n[k] > 0 && 6 * m[k] <= n[k]))
    }

    /// Region `i`'s text changed with its box still there (box memory): its patch goes now, and the
    /// new text is read again in place once it settles, with no detect over it.
    fn reuse(&mut self, i: usize, frame: &Frame, layer: &mut dyn Layer) {
        self.note_gone(i, frame);
        self.next_id += 1;
        let id = self.next_id;
        let r = &mut self.regions[i];
        if r.patch.take().is_some() {
            layer.hide(r.id);
        }
        (r.id, r.state, r.known, r.how, r.born, r.drawn) =
            (id, State::Changing, true, How::Box, frame.at, false);
        r.stages = Stages {
            seen: frame.at,
            ..Stages::default()
        };
        // (a box that typed its last text out will again: wait for its letters)
        r.typing = r.style.typed;
        r.letter_at = if r.typing { frame.at } else { Duration::ZERO };
        (r.alpha, r.prev_alpha) = (0.0, 0.0);
        self.reset_cells(i, frame);
    }

    /// Region `i`'s text leaves the screen: noted (its lifetime, drawn or not) and learned by its box.
    fn note_gone(&mut self, i: usize, frame: &Frame) {
        let settle = self.typed_settle(&self.regions[i].style);
        let r = &mut self.regions[i];
        if r.stages.settled.is_none() {
            return;
        }
        let life = frame.at.saturating_sub(r.stages.seen);
        r.style.life = Some(r.style.life.map_or(life, |l| (3 * l + life) / 4));
        self.notes.push(Note::Gone {
            id: r.id,
            life,
            drawn: r.drawn,
            typed: r.style.typed,
            settle: if r.style.typed {
                settle
            } else {
                Duration::ZERO
            },
        });
    }

    /// Ends the session's regions: every patch down, jobs in flight forgotten. The caches stay.
    pub fn stop(&mut self, layer: &mut dyn Layer) {
        if !self.regions.is_empty() {
            layer.clear();
        }
        self.regions.clear();
        self.tiles.clear();
        self.before = None;
        (self.last_discover, self.detecting, self.discover_now) = (None, None, false);
        self.took = Duration::ZERO;
        self.retired.clear();
        self.moving.clear();
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

    /// Whether the frames show our layer from now on ([`Config::layer_seen`]).
    pub fn layer_seen(&mut self, seen: bool) {
        self.cfg.layer_seen = seen;
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

    /// How many cells wide a region's text area is (as `pixels::walk` clamps it).
    fn cols(&self, r: Rect) -> usize {
        let (s, cw) = (
            self.cfg.scale.max(1),
            self.size.0.div_ceil(self.cfg.scale.max(1)),
        );
        (r.right().div_ceil(s).min(cw) - (r.x / s).min(cw)) as usize
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
        // where the capture shows our patch over the text (X11 without window frames, Wayland), a box
        // closing under it shows in the ring alone: then at most 3/4 of the ring is the scene's
        let ours_seen = r.patch.is_some()
            && self.cfg.layer_seen
            && !pixels::changed(sig, &r.reference, Some((&r.ours.0, 1.0)), change)
            && pixels::changed(sig, &r.reference, None, change);
        let scene = r.scene.1.iter().filter(|&&s| s).count();
        let none = Vec::new();
        let ring_scene = match ours_seen && 4 * scene > 3 * r.scene.1.len() {
            true => &none,
            false => &r.scene.1,
        };
        let (sig, ring) = (&keep(sig, &r.scene.0), &keep(ring, ring_scene));
        let (reference, ring_ref) = (
            &keep(&r.reference, &r.scene.0),
            &keep(&r.ring_ref, ring_scene),
        );
        let ours = (&keep(&r.ours.0, &r.scene.0), &keep(&r.ours.1, ring_scene));
        let mut drift = pixels::moved(ring, ring_ref, None, change);
        // (with the ring moving, a few strong cells are the scene through the box; else two cells moved
        // a lot is a new letter: a stroke moves a cell by ~100, a video's keyframe redrawing the same
        // text moved 4 cells of a choices question by up to 28 every 2 s, and it was drawn again, W28)
        let strong = match drift > self.cfg.seep {
            true => (4.0 * change, self.cfg.seep_cells),
            false => (4.0 * change, 2),
        };
        let mut changed = pixels::changed_by(sig, reference, None, change, strong);
        if r.patch.is_some() && self.cfg.layer_seen && (changed || drift > self.cfg.ring) {
            let mut alphas = vec![1.0, r.alpha, r.prev_alpha];
            alphas.dedup();
            for t in alphas {
                changed &= pixels::changed_by(sig, reference, Some((ours.0, t)), change, strong);
                drift = drift.min(pixels::moved(ring, ring_ref, Some((ours.1, t)), change));
            }
        }
        // a shown patch the capture doesn't show (our layer kept out of it, the game window's own
        // pixels): the text itself is seen, and it alone says it changed; a scene moving round its
        // box (a fight round a banner) is no change
        if r.patch.is_some() && !ours_seen {
            drift = 0.0;
        }
        (changed, drift)
    }

    /// Region `i`'s text changed after it was read: its patch goes now, it leaves the list, and the next
    /// discover runs at once (the new text's box may differ).
    fn retire(&mut self, i: usize, frame: &Frame, layer: &mut dyn Layer) {
        self.note_gone(i, frame);
        let r = self.regions.remove(i);
        if r.patch.is_some() {
            layer.hide(r.id);
        }
        if self.styles.len() >= STYLES_KEPT {
            self.styles.remove(0);
        }
        self.styles.push((r.rect, r.style));
        let at = r.patch.map_or(r.rect, |p| p.union(&r.rect));
        self.retired.push(at);
        // (and its place is looked at again: a detect in flight drops its boxes there, and its tiles
        // already showed the text, so it was never found again: a typed line lost, W31)
        let s = self.cfg.scale.max(1);
        let cw = self.size.0.div_ceil(s);
        if self.tiles.len() as u64 == cw as u64 * self.size.1.div_ceil(s) as u64 {
            pixels::walk(at, s, 0, self.size, |cx, cy, _| {
                self.tiles[(cy * cw + cx) as usize] = 0;
            });
        }
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
            if self.regions[i].known {
                // (an empty box read before: it waits for its next text)
                self.regions[i].state = State::Idle;
                return Some(None);
            }
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
        // (the same text where the scene moved through it before: not drawn again, nothing flickers)
        let rect = self.regions[i].rect;
        if let Some(m) = (self.moving.iter_mut()).find(|(at, said, when)| {
            at.same_place(&rect) && said == t && frame.at.saturating_sub(*when) < MOVING_FOR
        }) {
            m.2 = frame.at;
            self.regions[i].state = State::Idle;
            return;
        }
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
        r.said = t.to_string();
        r.patch = Some(at);
        r.state = State::Shown { since: frame.at };
        (r.drawn, r.boxed) = (true, true);
        let st = r.stages;
        let read = st.read.unwrap_or(frame.at);
        self.notes.push(Note::Drawn {
            id: r.id,
            how: r.how,
            seen: st.seen,
            detected: st.detected,
            settled: st.settled.unwrap_or(read),
            read,
            translated: st.translated.unwrap_or(read),
            shown: frame.at,
        });
        let r = &mut self.regions[i];
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
            age: (Vec::new(), Vec::new()),
            ring_moved: Vec::new(),
            scene: (Vec::new(), Vec::new()),
            patch: None,
            ours: (Vec::new(), Vec::new()),
            said: String::new(),
            alpha: 0.0,
            prev_alpha: 0.0,
            how: How::New,
            stages: Stages {
                seen: self.asked,
                detected: Some(frame.at),
                ..Stages::default()
            },
            lines: vec![rect],
            known: false,
            typing: false,
            letter_at: Duration::ZERO,
            moved_at: frame.at,
            hits: 0,
            front: Default::default(),
            // (a box that left this place taught how it shows its text)
            style: (self.styles.iter().rev())
                .find(|(at, _)| at.same_place(&rect))
                .map_or(Style::default(), |(_, s)| *s),
            drawn: false,
            boxed: false,
        });
        self.reset_cells(self.regions.len() - 1, frame);
    }

    /// Region `i` (changing) grew: the other parts of its line join it (a changing part on its line; a
    /// read one it now covers, whose patch goes), so a line found in pieces is one region, drawn once.
    fn join_line(&mut self, mut i: usize, frame: &Frame, layer: &mut dyn Layer) -> usize {
        let part = |r: &Region, line: &Rect| match r.state {
            State::Changing => r.rect.same_line(line),
            _ => r.rect.same_line(line) && r.rect.overlap(line) > 0,
        };
        while let Some(j) = (0..self.regions.len())
            .find(|&j| j != i && part(&self.regions[j], &self.regions[i].rect))
        {
            self.regions[i].rect = self.regions[i].rect.union(&self.regions[j].rect);
            self.regions[i].typing |= self.regions[j].typing;
            if self.regions[j].state == State::Changing {
                self.regions.remove(j);
            } else {
                self.retire(j, frame, layer);
            }
            i -= (j < i) as usize;
        }
        if self.regions[i].lines.len() <= 1 {
            self.regions[i].lines = vec![self.regions[i].rect];
        }
        self.reset_cells(i, frame);
        i
    }

    fn reset_cells(&mut self, i: usize, frame: &Frame) {
        let (s, ringc) = (self.cfg.scale.max(1), self.ring_cells());
        let r = &mut self.regions[i];
        (r.sig, r.ring) = pixels::sample(frame, r.rect, s, ringc);
        (r.still, r.ring_still, r.moved_at) = (0, 0, frame.at);
        r.age = (vec![0; r.sig.len()], vec![0; r.ring.len()]);
        r.ring_moved = vec![false; r.ring.len()];
        let line = r.lines.iter().map(|l| l.h).min().unwrap_or(r.rect.h).max(1);
        let last = r
            .lines
            .iter()
            .max_by_key(|l| l.y)
            .copied()
            .unwrap_or(r.rect);
        let (fw, fh) = (frame.width, frame.height);
        let x = last.right().min(fw);
        let right = Rect::new(
            x,
            last.y + last.h / 4,
            (line * self.cfg.front_lines).min(fw - x),
            (last.h / 2)
                .max(1)
                .min(fh.saturating_sub(last.y + last.h / 4)),
        );
        let y = r.rect.bottom().min(fh);
        let under = Rect::new(r.rect.x, y, r.rect.w, line.min(fh - y));
        r.front = [
            // (the panel past the line is judged just above it, clear of its letters)
            Front::new(
                frame,
                (
                    right,
                    Rect {
                        y: last.y.saturating_sub((last.h / 4).max(1)),
                        h: (last.h / 4).max(1),
                        ..right
                    },
                    line,
                ),
                s,
                &r.ring,
                &r.front[0],
            ),
            Front::new(frame, (under, under, 2 * line), s, &r.ring, &r.front[1]),
        ];
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
        }
        // (at most 1.5 s: the first detect also loads the engine)
        .max((self.took * (1 + self.cfg.discover_rest)).min(Duration::from_millis(1500)));
        let due = self
            .last_discover
            .is_none_or(|t| frame.at.saturating_sub(t) >= every);
        let s = self.cfg.scale.max(1);
        let masked = || -> Vec<Rect> {
            (self.regions.iter())
                .filter(|r| r.state != State::Changing || r.known)
                .map(|r| r.patch.map_or(r.rect, |p| p.union(&r.rect)))
                .collect()
        };
        if !due && !(self.discover_now && !b.slow) {
            return None;
        }
        let masked = masked();
        let now = pixels::grid(frame, s, &masked);
        // (a region masked since the last look is no change: its cells go out of the old look too,
        // else every text just read was looked for again once)
        if self.tiles.len() == now.len() {
            let cw = frame.width.div_ceil(s);
            for m in &masked {
                pixels::walk(*m, s, 0, (frame.width, frame.height), |cx, cy, _| {
                    self.tiles[(cy * cw + cx) as usize] = 0;
                });
            }
        }
        // a look `tile_still` ago first (taken now when there is none: a detect that ran longer than
        // the period left none, and then everything that moved was looked at)
        if !self.tiles.is_empty() && !self.cfg.tile_still.is_zero() {
            match &self.before {
                None => {
                    self.before = Some((frame.at, now));
                    return None;
                }
                Some((at, _)) if frame.at.saturating_sub(*at) < self.cfg.tile_still => return None,
                Some(_) => {}
            }
        }
        let cw = frame.width.div_ceil(s);
        let limit = self.cfg.tile_change;
        let (tw, t) = (cw.div_ceil(pixels::TILE) as usize, pixels::TILE as usize);
        let tile = |i: usize| i / cw as usize / t * tw + i % cw as usize / t;
        let changed: Vec<bool> = if now.len() == self.tiles.len() {
            let moved = pixels::tile_diffs(&now, &self.tiles, cw);
            // how much each tile moved lately (since `before`), and the share of the time since the
            // last discover that is
            let (lately, share) = match self.before.take() {
                Some((at, before)) if before.len() == now.len() => {
                    let since = self.last_discover.map_or(Duration::ZERO, |t| frame.at - t);
                    let lately = frame.at.saturating_sub(at).as_secs_f32().max(0.001);
                    let share = (since.as_secs_f32() / lately).max(1.0);
                    (pixels::tile_diffs(&now, &before, cw), share)
                }
                _ => (vec![0.0; moved.len()], 1.0),
            };
            // a tile that hasn't held still keeps its old cells: compared again next time
            for (i, (old, now)) in self.tiles.iter_mut().zip(&now).enumerate() {
                if lately[tile(i)] <= limit {
                    *old = *now;
                }
            }
            // new = changed, held still lately (within half of `tile_change`: a scene in part of a tile
            // moves it a little), and by more than its slow motion explains (~that rate x time since)
            (moved.iter().zip(&lately))
                .map(|(&m, &l)| m > limit && 2.0 * l <= limit && m > 2.0 * l * share)
                .collect()
        } else {
            self.tiles = now;
            vec![true; tw * frame.height.div_ceil(s).div_ceil(pixels::TILE) as usize]
        };
        (self.last_discover, self.discover_now) = (Some(frame.at), false);
        let areas = pixels::clusters(&changed, self.size, s);
        if areas.is_empty() {
            return None;
        }
        let job = self.job();
        (self.detecting, self.asked) = (Some(job), frame.at);
        self.retired.clear();
        Some(Job::Detect { job, areas })
    }
}

impl Front {
    /// Band `at` (clamped by the caller; `probe`: the strip over its columns where the panel is judged);
    /// `ring` = the region's ring cells now (their median is the
    /// panel's gray); a cell `old` also watched keeps what it knew (a box that grows moves its band).
    fn new(
        f: &Frame,
        (at, probe, near): (Rect, Rect, u32),
        s: u32,
        ring: &[u8],
        old: &Front,
    ) -> Front {
        let now = match at.area() {
            0 => Vec::new(),
            _ => pixels::sample(f, at, s, 0).0,
        };
        let (cw, ch) = (f.width.div_ceil(s), f.height.div_ceil(s));
        let (x0, y0) = ((at.x / s).min(cw), (at.y / s).min(ch));
        let cols = at.right().div_ceil(s).min(cw) - x0;
        let (median, flat) = panel(ring);
        // (the panel's extent: the band ends at the first column hardly any of whose cells show it,
        // the scene past the panel's edge)
        let seen = match probe.area() {
            0 => Vec::new(),
            _ => pixels::sample(f, probe, s, 0).0,
        };
        let rows = seen.len() / (cols as usize).max(1);
        let reach = (0..cols as usize)
            .find(|&c| {
                let panel =
                    (0..rows).filter(|&r| seen[r * cols as usize + c].abs_diff(median) <= FLAT);
                3 * panel.count() < rows
            })
            .unwrap_or(cols as usize) as u32;
        let mut fr = Front {
            at,
            cells: (x0, y0, cols),
            first: now.clone(),
            // (still since before it was watched)
            jump: vec![Duration::ZERO; now.len()],
            calm: vec![true; now.len()],
            counted: vec![false; now.len()],
            now,
            panel: median,
            flat: flat && !ring.is_empty(),
            reach,
            near,
            right: 0,
        };
        let (ox, oy, oc) = old.cells;
        for k in 0..fr.now.len() {
            let (cx, cy) = (x0 + k as u32 % cols.max(1), y0 + k as u32 / cols.max(1));
            if (ox..ox + oc).contains(&cx) && cy >= oy {
                let j = ((cy - oy) * oc + cx - ox) as usize;
                if j < old.now.len() {
                    (fr.first[k], fr.jump[k], fr.calm[k], fr.counted[k]) =
                        (old.first[j], old.jump[j], old.calm[j], old.counted[j]);
                    fr.panel = old.panel;
                }
            }
        }
        fr
    }

    /// Cell `k` may hold a letter: it held still before its last jump, it showed the panel's gray at the
    /// start, and it shows another gray now.
    fn inked(&self, k: usize, change: f32) -> bool {
        let far = |a: u8, b: u8| a.abs_diff(b) as f32 > change;
        self.flat
            && (k as u32 % self.cells.2.max(1)) < self.reach
            && self.calm[k]
            && !far(self.first[k], self.panel)
            && far(self.now[k], self.first[k])
    }

    /// The next frame: how many cells got a letter now (a calm panel cell jumped to another gray and
    /// held it `HOLD`: a moving scene's cells keep changing, or come back).
    fn step(&mut self, f: &Frame, s: u32, change: f32) -> u32 {
        if self.at.area() == 0 {
            return 0;
        }
        let now = pixels::sample(f, self.at, s, 0).0;
        if now.len() != self.now.len() {
            return 0;
        }
        let (x0, _, cols) = self.cells;
        let mut letters = 0;
        for (k, v) in now.into_iter().enumerate() {
            if v.abs_diff(self.now[k]) as f32 > change {
                self.calm[k] = f.at.saturating_sub(self.jump[k]) >= CALM;
                (self.jump[k], self.counted[k]) = (f.at, false);
            }
            self.now[k] = v;
            // (letters land at the front, next to the last one: a scene's shapes anywhere in it)
            let x = (x0 + k as u32 % cols.max(1)) * s;
            if !self.counted[k]
                && f.at.saturating_sub(self.jump[k]) >= HOLD
                && self.inked(k, change)
                && x <= self.right.max(self.at.x) + self.near
            {
                self.counted[k] = true;
                letters += 1;
                self.right = self.right.max(x + s);
            }
        }
        letters
    }

    /// A letter landed in it since it was set up (held or not): on a cell that showed the panel's own
    /// gray (within `FLAT / 3`), clear of the panel's last column (a cell half over the scene past a
    /// tight HUD panel, or the scene just under it, took a passing shape for a letter, W31).
    fn grew(&self, change: f32) -> bool {
        let cols = self.cells.2.max(1);
        (0..self.now.len()).any(|k| {
            k as u32 % cols + 1 < self.reach
                && self.first[k].abs_diff(self.panel) <= FLAT / 3
                && self.inked(k, change)
        })
    }

    /// A letter is landing now (it jumped less than `HOLD` ago).
    fn landing(&self, at: Duration, change: f32) -> bool {
        (0..self.now.len()).any(|k| at.saturating_sub(self.jump[k]) < HOLD && self.inked(k, change))
    }
}

/// A front cell held still this long before it jumped: a letter can land there (a scene moving in
/// steps holds a few frames, then moves on).
const CALM: Duration = Duration::from_millis(120);
/// A letter holds its gray this long once it landed.
const HOLD: Duration = Duration::from_millis(45);
const FLAT: u8 = 12;

/// Ring cells' median gray, and whether they are a panel: most of them show that one gray (round a
/// tight bar, or text with no box, they are the scene's).
fn panel(ring: &[u8]) -> (u8, bool) {
    let mut sorted = ring.to_vec();
    sorted.sort_unstable();
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0);
    let flat = 5 * ring.iter().filter(|c| c.abs_diff(median) <= FLAT).count() >= 3 * ring.len();
    (median, flat)
}
/// Text that is not known to be typing settles only this long after a letter landed beside it.
const QUIET: Duration = Duration::from_millis(150);

/// A box's usual pause between letters: a running mean (a pause at a line's end or a comma moves it a
/// little, not to its own length).
fn gap_mean(mean: Duration, gap: Duration) -> Duration {
    match mean.is_zero() {
        true => gap,
        false => (3 * mean + gap) / 4,
    }
}
/// A box being typed out that never stops changing is read after this long anyway.
const TYPED_MAX: Duration = Duration::from_secs(6);

impl Region {
    /// What "unchanged" means from now on: the cells now, without the scene's. Of the text area (`cols`
    /// cells wide): the outer cells, a quarter of its height deep (a detect box is padded past its text:
    /// PP-OCR's unclip; past a tight HUD bar, a banner or a label's panel they show the scene behind it) and, once the text `settled`, those that moved
    /// in the last `frames` frames (it held still that long; read after `settle_max` it may still be
    /// typing); never most of it (text over a see-through box keeps every cell: `Config::seep`). Of the
    /// ring: every cell that moved while the text settled (and, from then on, while it holds: `on_frame`).
    fn take_reference(&mut self, frames: u8, settled: bool, cols: usize) {
        (self.reference, self.ring_ref) = (self.sig.clone(), self.ring.clone());
        let n = self.sig.len();
        let rows = n / cols.max(1);
        let mut text: Vec<bool> = (0..n)
            .map(|i| settled && self.age.0.get(i).is_some_and(|&a| a < frames))
            .collect();
        if 2 * text.iter().filter(|&&m| m).count() > n {
            text.fill(false);
        }
        // (the padding: about a quarter of a line's height on every side, rounded up: a cell half on
        // the panel moves with the scene too; a third of the rows and columns at least stay, round the
        // text's middle. Of a LINE: a quarter of a three-line box's height left its first and last
        // lines out, and the last one typed on under a patch for 2 s, W31)
        let line = self.lines.iter().map(|l| l.h).min().unwrap_or(self.rect.h);
        let line_rows =
            (rows as u64 * line.min(self.rect.h) as u64).div_ceil(self.rect.h.max(1) as u64);
        let pad = (line_rows as usize)
            .div_ceil(4)
            .min(rows.saturating_sub(2) / 3);
        if pad > 0 && cols >= 3 * pad + 2 {
            for (i, t) in text.iter_mut().enumerate() {
                let (x, y) = (i % cols, i / cols);
                *t |= x < pad || y < pad || x + pad >= cols || y + pad >= rows;
            }
        }
        let ring = (0..self.ring.len())
            .map(|i| {
                self.ring_moved.get(i).copied().unwrap_or(false)
                    || self.age.1.get(i).is_some_and(|&a| a < frames)
            })
            .collect();
        self.scene = (text, ring);
    }
}

/// `cells` without the ones `scene` marks (all of them when `scene` is empty).
fn keep(cells: &[u8], scene: &[bool]) -> Vec<u8> {
    if scene.len() != cells.len() {
        return cells.to_vec();
    }
    (cells.iter().zip(scene))
        .filter(|(_, s)| !**s)
        .map(|(c, _)| *c)
        .collect()
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
