//! The live core on synthetic frames: fake engines, a layer that records every call, and a replay of the
//! cutscene harness' `talk` clip ground truth (tests/fixtures/live_core/talk.truth) at 1/4 scale.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use super::cache::Lru;
use super::*;

const PANEL: [u8; 3] = [8, 8, 12];

struct Img {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Img {
    fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![40; (w * h * 4) as usize],
        }
    }
    fn put(&mut self, x: u32, y: u32, v: [u8; 3]) {
        let i = ((y * self.w + x) * 4) as usize;
        self.px[i..i + 4].copy_from_slice(&[v[0], v[1], v[2], 255]);
    }
    fn fill(&mut self, r: Rect, v: [u8; 3]) {
        for y in r.y..r.bottom().min(self.h) {
            for x in r.x..r.right().min(self.w) {
                self.put(x, y, v);
            }
        }
    }
    /// A background that moves every frame.
    fn stripes(&mut self, n: u64) {
        for y in 0..self.h {
            for x in 0..self.w {
                let v = ((x as u64 + 2 * y as u64 + 7 * n) % 51 * 5) as u8;
                self.put(x, y, [v, v / 2, 255 - v]);
            }
        }
    }
    /// "Text": a pattern only `text` makes, ink where its bit is set; `bg` None = the pixels already there
    /// show between the strokes.
    fn text(&mut self, r: Rect, text: &str, bg: Option<[u8; 3]>) {
        for y in r.y..r.bottom().min(self.h) {
            for x in r.x..r.right().min(self.w) {
                let mut h = DefaultHasher::new();
                (text, x / 2, y / 2).hash(&mut h);
                if h.finish() & 1 == 1 {
                    self.put(x, y, [230, 230, 230]);
                } else if let Some(bg) = bg {
                    self.put(x, y, bg);
                }
            }
        }
    }
    fn frame(&self, n: u64) -> Frame<'_> {
        Frame {
            rgba: &self.px,
            width: self.w,
            height: self.h,
            at: at(n),
        }
    }
}

fn at(n: u64) -> Duration {
    Duration::from_nanos(n * 1_000_000_000 / 60)
}

/// What is on screen: text boxes and their words (the fake detector and reader see this).
struct Screen<'a>(&'a [(Rect, String)]);

impl Detector for Screen<'_> {
    fn detect(&mut self, _: &Frame, areas: &[Rect]) -> Vec<TextBox> {
        (self.0.iter())
            .filter(|(r, _)| areas.iter().any(|a| a.overlap(r) > 0))
            .map(|(r, _)| TextBox::from(*r))
            .collect()
    }
}

impl Reader for Screen<'_> {
    fn read(&mut self, crops: &[Crop]) -> Vec<String> {
        (crops.iter())
            .map(|c| {
                (self.0.iter())
                    .filter(|(r, _)| r.overlap(&c.at) > 0)
                    .max_by_key(|(r, _)| r.overlap(&c.at))
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default()
            })
            .collect()
    }
}

#[derive(Default)]
struct Tr {
    calls: usize,
}

impl Translator for Tr {
    fn translate(&mut self, text: &str) -> Result<String, String> {
        self.calls += 1;
        Ok(format!("<{text}>"))
    }
}

/// Paints the translation's bytes into the patch, so the layer can tell what each patch says.
struct Paint;

impl Painter for Paint {
    fn patch(&mut self, _: &Frame, region: Rect, text: &str) -> Option<(Rect, Vec<u8>)> {
        let mut rgba = vec![0; (region.area() * 4) as usize];
        let n = text.len().min(rgba.len());
        rgba[..n].copy_from_slice(&text.as_bytes()[..n]);
        Some((region, rgba))
    }
}

#[derive(Default)]
struct Recorder {
    frame: u64,
    shown: HashMap<RegionId, (Rect, String)>,
    /// (frame, where, what) of every show.
    log: Vec<(u64, Rect, String)>,
    shows: usize,
    alphas: usize,
    hides: usize,
    clears: usize,
}

impl Recorder {
    fn calls(&self) -> usize {
        self.shows + self.alphas + self.hides + self.clears
    }
    fn says(&self, text: &str) -> bool {
        self.shown.values().any(|(_, t)| t == text)
    }
}

impl Layer for Recorder {
    fn show(&mut self, id: RegionId, at: Rect, rgba: &[u8]) {
        let text: Vec<u8> = rgba.iter().take_while(|b| **b != 0).copied().collect();
        let text = String::from_utf8_lossy(&text).into_owned();
        self.shown.insert(id, (at, text.clone()));
        self.log.push((self.frame, at, text));
        self.shows += 1;
    }
    fn alpha(&mut self, _: RegionId, _: f32) {
        self.alphas += 1;
    }
    fn hide(&mut self, id: RegionId) {
        self.shown.remove(&id);
        self.hides += 1;
    }
    fn clear(&mut self) {
        self.shown.clear();
        self.clears += 1;
    }
}

/// The host: runs every job at once on the screen as it is, delivers its result `lat` frames later
/// ([detect, read, translate]).
struct Sim {
    live: Live,
    layer: Recorder,
    tr: Tr,
    screen: Vec<(Rect, String)>,
    pending: Vec<(u64, Done)>,
    lat: [u64; 3],
    n: u64,
    detects: usize,
    reads: Vec<String>,
    /// The screen's lines are one box (`Block`: found and read as one), not one box each.
    block: bool,
    /// The text boxes as a detect finds them (their lines in `screen`; empty: each screen line a box).
    boxes: Vec<TextBox>,
}

/// Boxes found where any of them changed (read by `Block`: the lines under the crop).
struct Boxes<'a>(&'a [TextBox]);

impl Detector for Boxes<'_> {
    fn detect(&mut self, _: &Frame, areas: &[Rect]) -> Vec<TextBox> {
        (self.0.iter())
            .filter(|b| areas.iter().any(|a| a.overlap(&b.at) > 0))
            .cloned()
            .collect()
    }
}

/// The screen's lines as ONE text box, as OCR groups a dialogue box's lines: found whole where any of
/// it changed, and read as all its lines under the crop, top to bottom.
struct Block<'a>(&'a [(Rect, String)]);

impl Detector for Block<'_> {
    fn detect(&mut self, _: &Frame, areas: &[Rect]) -> Vec<TextBox> {
        let lines: Vec<Rect> = (self.0.iter())
            .filter(|(r, _)| areas.iter().any(|a| a.overlap(r) > 0))
            .map(|(r, _)| *r)
            .collect();
        let Some(at) = lines.iter().copied().reduce(|a, b| a.union(&b)) else {
            return Vec::new();
        };
        vec![TextBox { at, lines }]
    }
}

impl Reader for Block<'_> {
    fn read(&mut self, crops: &[Crop]) -> Vec<String> {
        (crops.iter())
            .map(|c| {
                (self.0.iter())
                    .filter(|(r, _)| 2 * r.overlap(&c.at) > r.area())
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
}

impl Sim {
    fn new(cfg: Config, lat: [u64; 3]) -> Self {
        Self {
            live: Live::new(cfg),
            layer: Recorder::default(),
            tr: Tr::default(),
            screen: Vec::new(),
            pending: Vec::new(),
            lat,
            n: 0,
            detects: 0,
            reads: Vec::new(),
            block: false,
            boxes: Vec::new(),
        }
    }

    fn step(&mut self, img: &Img) {
        let f = img.frame(self.n);
        self.layer.frame = self.n;
        let jobs = self.live.on_frame(&f, &mut Paint, &mut self.layer);
        self.start(jobs, &f);
        while let Some(i) = self.pending.iter().position(|(due, _)| *due <= self.n) {
            let (_, done) = self.pending.remove(i);
            let jobs = self.live.complete(done, &f, &mut Paint, &mut self.layer);
            self.start(jobs, &f);
        }
        self.n += 1;
    }

    fn start(&mut self, jobs: Vec<Job>, f: &Frame) {
        for job in jobs {
            let lat = match &job {
                Job::Detect { .. } => {
                    self.detects += 1;
                    self.lat[0]
                }
                Job::Read(_) => self.lat[1],
                Job::Translate { .. } => self.lat[2],
            };
            let read = matches!(job, Job::Read(_));
            let done = match self.block {
                _ if !self.boxes.is_empty() => job.run(
                    f,
                    &mut Boxes(&self.boxes),
                    &mut Block(&self.screen),
                    &mut self.tr,
                ),
                true => job.run(
                    f,
                    &mut Block(&self.screen),
                    &mut Block(&self.screen),
                    &mut self.tr,
                ),
                false => job.run(
                    f,
                    &mut Screen(&self.screen),
                    &mut Screen(&self.screen),
                    &mut self.tr,
                ),
            };
            for done in done {
                if let (true, Done::Read { text, .. }) = (read, &done) {
                    self.reads.push(text.clone());
                }
                self.pending.push((self.n + lat, done));
            }
        }
    }

    fn state(&self) -> Option<State> {
        self.live.regions().next().map(|(_, _, s)| s.clone())
    }

    /// Steps until `f` holds (at most 120 frames).
    fn until(&mut self, img: &Img, f: impl Fn(&Sim) -> bool) {
        for _ in 0..120 {
            if f(self) {
                return;
            }
            self.step(img);
        }
        panic!("never happened");
    }
}

const BOX: Rect = Rect::new(40, 40, 240, 80);
const LINE: Rect = Rect::new(60, 60, 160, 24);

/// A panel with one line on it, over a background that moves when `n` is Some.
fn scene(text: &str, n: Option<u64>) -> Img {
    let mut img = Img::new(320, 180);
    if let Some(n) = n {
        img.stripes(n);
    }
    img.fill(BOX, PANEL);
    img.text(LINE, text, Some(PANEL));
    img
}

fn shown_sim(text: &str) -> (Sim, Img) {
    let mut sim = Sim::new(Config::default(), [0, 0, 0]);
    sim.screen = vec![(LINE, text.into())];
    let img = scene(text, None);
    sim.until(&img, |s| s.layer.says(&format!("<{text}>")));
    for _ in 0..10 {
        sim.step(&img); // the fade ends
    }
    (sim, img)
}

#[test]
fn changed_text_is_hidden_in_the_same_frame() {
    let (mut sim, _) = shown_sim("Open the gate");
    let img = scene("Close the gate", None);
    sim.screen = vec![(LINE, "Close the gate".into())];
    let f = img.frame(sim.n);
    let hides = sim.layer.hides;
    let jobs = sim.live.on_frame(&f, &mut Paint, &mut sim.layer);
    assert_eq!(sim.layer.hides, hides + 1, "hidden in the same on_frame");
    assert!(sim.layer.shown.is_empty());
    sim.start(jobs, &f);
    sim.n += 1;
    sim.until(&img, |s| s.layer.says("<Close the gate>"));
}

#[test]
fn unchanged_text_is_never_drawn_again() {
    let (mut sim, img) = shown_sim("Open the gate");
    assert_eq!(sim.layer.shows, 1);
    let calls = sim.layer.calls();
    for _ in 0..600 {
        sim.step(&img);
    }
    assert_eq!(
        sim.layer.calls(),
        calls,
        "no layer call over 600 still frames"
    );
    assert_eq!(sim.reads.len(), 1);
}

/// W28: a line emptied under its patch where captures never show our layer (Windows): the emptied box
/// looks like our erased patch, and taking it for our patch kept the patch up until the next line came.
/// Hidden in the frame it changed.
#[test]
fn a_line_emptied_under_a_patch_the_capture_never_shows_is_hidden_at_once() {
    let (mut sim, _) = shown_sim("Open the gate");
    sim.live.layer_seen(false);
    let mut img = scene("", None);
    // (the test painter's patch is black: the box now looks just like it)
    img.fill(LINE, [0, 0, 0]);
    sim.screen.clear();
    let hides = sim.layer.hides;
    let f = img.frame(sim.n);
    sim.live.on_frame(&f, &mut Paint, &mut sim.layer);
    assert_eq!(sim.layer.hides, hides + 1, "hidden in the same on_frame");
    assert!(sim.layer.shown.is_empty());
}

/// W28: a video's keyframe redraws the same text a little differently (4 cells of a question moved by
/// up to 28 every 2 s): no change, its patch stays and nothing is drawn again.
#[test]
fn a_keyframe_redrawing_the_same_text_is_no_change() {
    let (mut sim, mut img) = shown_sim("Open the gate");
    let (shows, hides) = (sim.layer.shows, sim.layer.hides);
    for k in 0..4 {
        let (x, y) = (LINE.x + 8 + 32 * k, LINE.y + 8);
        for py in y..y + 4 {
            for px in x..x + 4 {
                let i = ((py * img.w + px) * 4) as usize;
                for c in 0..3 {
                    img.px[i + c] = img.px[i + c].saturating_add(28);
                }
            }
        }
    }
    for _ in 0..60 {
        sim.step(&img);
    }
    assert_eq!((sim.layer.shows, sim.layer.hides), (shows, hides));
}

/// W28: a line with no box round it over a moving 3D scene: drawn while the scene held, taken down when
/// it moved through the text (nothing stale), and never drawn again while the same text stays (it was
/// drawn 27 times in 36 s: each a flicker).
#[test]
fn a_static_line_over_a_moving_scene_is_never_drawn_again() {
    let mut sim = Sim::new(Config::default(), [0, 0, 0]);
    sim.live.layer_seen(false);
    sim.screen = vec![(LINE, "Open the gate".into())];
    let at = |n: u64| {
        let mut img = Img::new(320, 180);
        img.stripes(n);
        img.text(LINE, "Open the gate", None);
        img
    };
    let still = at(0);
    sim.until(&still, |s| s.layer.says("<Open the gate>"));
    for round in 0..4 {
        for n in 1..40 {
            sim.step(&at(round * 100 + n));
        }
        // (the scene holds again, elsewhere in its motion: the line is found and read again)
        let still = at(round * 100 + 40);
        for _ in 0..60 {
            sim.step(&still);
        }
    }
    assert_eq!(sim.layer.shows, 1, "drawn once");
    assert!(
        sim.layer.shown.is_empty(),
        "taken down when the scene moved through it"
    );
}

#[test]
fn late_results_for_changed_text_are_dropped() {
    for slow in [1, 2] {
        // slow = 1: the read is late, 2: the translation is late
        let mut lat = [0, 0, 0];
        lat[slow] = 8;
        let mut sim = Sim::new(Config::default(), lat);
        sim.screen = vec![(LINE, "Old line".into())];
        let img = scene("Old line", None);
        sim.until(&img, |s| {
            matches!(
                (slow, s.state()),
                (1, Some(State::Reading { .. })) | (2, Some(State::Translating { .. }))
            )
        });
        let img = scene("New line", None);
        sim.screen = vec![(LINE, "New line".into())];
        sim.until(&img, |s| s.layer.says("<New line>"));
        assert!(
            sim.layer.log.iter().all(|(_, _, t)| t != "<Old line>"),
            "a late result never lands over new text ({slow})"
        );
    }
}

#[test]
fn typewriter_text_is_read_once_settled() {
    let full = "Welcome back, traveller";
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    let mut img = scene("", None);
    for k in 0..=12u32 {
        // one more piece of the line every frame for 12 frames (200 ms), then still
        let shown = full.len() * k as usize / 12;
        img.fill(LINE, PANEL);
        img.text(
            Rect::new(LINE.x, LINE.y, LINE.w * k / 12, LINE.h),
            full,
            Some(PANEL),
        );
        sim.screen = vec![(LINE, full[..shown].into())];
        sim.step(&img);
        assert!(
            sim.reads.is_empty(),
            "nothing read while it types (frame {k})"
        );
    }
    sim.until(&img, |s| s.layer.says(&format!("<{full}>")));
    assert_eq!(sim.reads, [full]);
}

#[test]
fn text_that_never_settles_is_read_after_settle_max_and_never_drawn() {
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    for n in 0..120 {
        let t = format!("scrolling {n}");
        sim.screen = vec![(LINE, t.clone())];
        sim.step(&scene(&t, None));
        if n < 24 {
            assert!(sim.reads.is_empty(), "read before settle_max ({n})");
        }
    }
    assert!(!sim.reads.is_empty(), "read after settle_max");
    assert_eq!(
        sim.layer.shows, 0,
        "a line that never stays still is never drawn"
    );
}

#[test]
fn a_moving_background_around_a_box_keeps_the_patch() {
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    sim.screen = vec![(LINE, "Hold the line".into())];
    let mut n = 0;
    sim.until(&scene("Hold the line", Some(0)), |s| {
        s.layer.says("<Hold the line>")
    });
    for _ in 0..10 {
        n += 1;
        sim.step(&scene("Hold the line", Some(n)));
    }
    let calls = sim.layer.calls();
    for _ in 0..300 {
        n += 1;
        sim.step(&scene("Hold the line", Some(n)));
    }
    assert_eq!(
        sim.layer.calls(),
        calls,
        "the box holds: no hide, no redraw"
    );
}

#[test]
fn text_on_a_moving_background_is_never_patched() {
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    sim.screen = vec![(LINE, "Critical".into())];
    for n in 0..200 {
        let mut img = Img::new(320, 180);
        img.stripes(n);
        img.text(LINE, "Critical", None);
        sim.step(&img);
    }
    assert!(sim.detects > 0);
    assert_eq!(sim.layer.shows, 0);
    assert!(sim.reads.is_empty(), "never even read");
}

#[test]
fn lru_hits_and_evicts_the_oldest() {
    let mut c = Lru::new(2);
    c.put("a", 1);
    c.put("b", 2);
    assert_eq!(c.get(&"a"), Some(1));
    c.put("c", 3);
    assert_eq!(c.get(&"b"), None, "b was the least recently used");
    assert_eq!((c.get(&"a"), c.get(&"c"), c.len()), (Some(1), Some(3), 2));
}

#[test]
fn a_line_seen_before_skips_reading_and_translating() {
    let (mut sim, a) = shown_sim("Continue");
    let b = scene("Skip", None);
    sim.screen = vec![(LINE, "Skip".into())];
    sim.until(&b, |s| s.layer.says("<Skip>"));
    let (reads, translations) = (sim.reads.len(), sim.tr.calls);
    sim.screen = vec![(LINE, "Continue".into())];
    sim.until(&a, |s| s.layer.says("<Continue>"));
    assert_eq!(
        (sim.reads.len(), sim.tr.calls),
        (reads, translations),
        "both caches hit"
    );
}

#[test]
fn back_off_slows_and_pauses_discover_never_the_watch() {
    // (a scene that moves all the time is looked at: the back-off alone paces it)
    let cfg = Config {
        tile_still: Duration::ZERO,
        ..Config::default()
    };
    let mut sim = Sim::new(cfg, [0, 0, 0]);
    sim.screen = vec![(LINE, "x".into())];
    let detects_in = |sim: &mut Sim, frames: u64| {
        let d = sim.detects;
        for _ in 0..frames {
            let n = sim.n;
            sim.step(&scene("x", Some(n)));
        }
        sim.detects - d
    };
    assert!(detects_in(&mut sim, 60) >= 4, "4 Hz while the screen moves");
    assert!(sim.layer.says("<x>"));
    for _ in 0..5 {
        assert!(!sim.live.report_burst(Duration::from_millis(100), at(sim.n)));
    }
    sim.n = 61 * 60; // a minute later
    let slow: Vec<bool> = (0..3)
        .map(|_| sim.live.report_burst(Duration::from_millis(250), at(sim.n)))
        .collect();
    assert_eq!(slow, [false, false, true], "3 slow bursts in a row");
    assert!(detects_in(&mut sim, 120) <= 3, "1 Hz after the back-off");
    sim.live.report_cpu(0.9, at(sim.n));
    detects_in(&mut sim, 130);
    sim.live.report_cpu(0.9, at(sim.n));
    assert_eq!(detects_in(&mut sim, 120), 0, "paused while the CPU is busy");
    let (hides, f) = (sim.layer.hides, scene("y", Some(sim.n)));
    let jobs = sim
        .live
        .on_frame(&f.frame(sim.n), &mut Paint, &mut sim.layer);
    assert!(
        jobs.is_empty() && sim.layer.hides == hides + 1,
        "the watch still hides at once"
    );
    sim.live.report_cpu(0.5, at(sim.n));
    assert!(detects_in(&mut sim, 120) > 0, "back after it calms down");
}

#[test]
fn boxes_found_before_a_change_never_place_the_new_line() {
    let short = Rect::new(LINE.x, LINE.y, 80, LINE.h);
    let mut sim = Sim::new(Config::default(), [6, 0, 0]);
    sim.screen = vec![(short, "Short".into())];
    let draw = |text: &str, r: Rect, n: u64| {
        let mut img = scene("", Some(n));
        img.text(r, text, Some(PANEL));
        img
    };
    while !sim.layer.says("<Short>") {
        let n = sim.n;
        sim.step(&draw("Short", short, n));
        assert!(sim.n < 300);
    }
    // change it while a detect of the old screen is still out (a mark beside the box makes one look
    // there: the box itself, read, is masked from the look)
    while !sim
        .pending
        .iter()
        .any(|(_, d)| matches!(d, Done::Detected { .. }))
    {
        let n = sim.n;
        let mut img = draw("Short", short, n);
        img.fill(Rect::new(236, 64, 12, 12), [200, 60, 60]);
        sim.step(&img);
        assert!(sim.n < 600, "a detect is asked");
    }
    sim.screen = vec![(LINE, "A much longer line".into())];
    while !sim.layer.says("<A much longer line>") {
        let n = sim.n;
        sim.step(&draw("A much longer line", LINE, n));
        assert!(sim.n < 600);
    }
    let (at, _) = sim.layer.shown.values().next().unwrap();
    assert_eq!(
        *at, LINE,
        "drawn over the new line's box, not the old one's"
    );
}

#[test]
fn a_result_is_dropped_when_the_newest_frame_already_differs() {
    let mut sim = Sim::new(Config::default(), [0, 0, 4]);
    sim.screen = vec![(LINE, "Old line".into())];
    let img = scene("Old line", None);
    sim.until(&img, |s| {
        matches!(s.state(), Some(State::Translating { .. }))
    });
    // the host completes the translation with a newer frame than on_frame has seen
    let new = scene("New line", None);
    let f = new.frame(sim.n);
    for (_, done) in std::mem::take(&mut sim.pending) {
        sim.live.complete(done, &f, &mut Paint, &mut sim.layer);
    }
    assert_eq!(
        sim.layer.shows, 0,
        "no translation of the old line over the new one"
    );
}

#[test]
fn stop_clears_the_layer() {
    let (mut sim, _) = shown_sim("Bye");
    sim.live.stop(&mut sim.layer);
    assert!(sim.layer.shown.is_empty());
    assert_eq!(sim.live.regions().count(), 0);
}

/// One text event of a harness clip's ground truth (frame px of the 1920x1080 clip).
struct Event {
    id: usize,
    kind: String,
    held: bool,
    on: u64,
    off: u64,
    rect: Rect,
    panel: Option<Rect>,
    text: String,
}

fn truth(src: &str) -> Vec<Event> {
    (src.lines().filter(|l| !l.starts_with('#')))
        .map(|l| {
            let f: Vec<&str> = l.splitn(14, ' ').collect();
            let n = |i: usize| f[i].parse::<u32>().unwrap();
            Event {
                id: n(0) as usize,
                kind: f[1].to_string(),
                held: f[2] == "1",
                on: n(3) as u64,
                off: n(4) as u64,
                rect: Rect::new(n(5), n(6), n(7), n(8)),
                panel: (f[9] != "-").then(|| Rect::new(n(9), n(10), n(11), n(12))),
                text: f[13].to_string(),
            }
        })
        .collect()
}

/// A clip rect at 1/4 scale (outward).
fn quarter(r: Rect) -> Rect {
    let (x, y) = (r.x / 4, r.y / 4);
    Rect::new(x, y, r.right().div_ceil(4) - x, r.bottom().div_ceil(4) - y)
}

#[test]
fn replay_of_the_talk_clip_has_no_stale_frame_and_no_redraw() {
    let events = truth(include_str!("../../tests/fixtures/live_core/talk.truth"));
    // the frames come in at 1/4 scale, so the checks run per pixel (scale 1) with a 2 px ring
    let cfg = Config {
        scale: 1,
        ring_px: 2,
        ..Config::default()
    };
    let mut sim = Sim::new(cfg, [4, 5, 6]); // detect 67 ms, read 83 ms, translate 100 ms
    let mut img = Img::new(480, 270);
    let (mut stale, mut redraws) = (0, 0);
    let mut drawn: HashSet<usize> = HashSet::new();
    let mut covered: HashMap<usize, u64> = HashMap::new();
    for n in 0..3600u64 {
        let on: Vec<&Event> = events.iter().filter(|e| e.on <= n && n < e.off).collect();
        img.stripes(n);
        // the clip draws the subtitle box only while a subtitle is up (gen_clips.py, Talk.panels)
        for e in on
            .iter()
            .filter(|e| e.kind == "subtitle" || e.kind == "hud")
        {
            img.fill(quarter(e.panel.unwrap()), PANEL);
        }
        for e in &on {
            img.text(quarter(e.rect), &e.text, None);
        }
        sim.screen = on
            .iter()
            .map(|e| (quarter(e.rect), e.text.clone()))
            .collect();
        sim.step(&img);
        let under = |r: &Rect| {
            on.iter()
                .filter(|e| quarter(e.rect).overlap(r) > 0)
                .max_by_key(|e| quarter(e.rect).overlap(r))
        };
        for (r, t) in sim.layer.shown.values() {
            match under(r) {
                Some(e) if *t == format!("<{}>", e.text) => {
                    covered.entry(e.id).or_insert(n - e.on);
                }
                _ => stale += 1,
            }
        }
        for (_, r, _) in sim.layer.log.iter().filter(|(f, _, _)| *f == n) {
            if let Some(e) = under(r)
                && !drawn.insert(e.id)
            {
                redraws += 1;
            }
        }
    }
    let held: Vec<&Event> = events
        .iter()
        .filter(|e| e.held && e.off - e.on >= 60)
        .collect();
    let mut lat: Vec<u64> = held
        .iter()
        .filter_map(|e| covered.get(&e.id).copied())
        .collect();
    lat.sort();
    let ms = |f: u64| f * 1000 / 60;
    println!(
        "talk replay: {} of {} held lines drawn, text -> drawn median {} ms, p95 {} ms, max {} ms; stale {stale}, redraws {redraws}",
        lat.len(),
        held.len(),
        ms(lat[lat.len() / 2]),
        ms(lat[lat.len() * 95 / 100]),
        ms(*lat.last().unwrap())
    );
    assert_eq!(stale, 0, "frames with a patch over other text");
    assert_eq!(redraws, 0, "unchanged lines drawn again");
    assert_eq!(lat.len(), held.len(), "every held line drawn");
}

/// The watch's cost: 10 shown lines on a still 1080p frame (release: `--ignored --nocapture`).
#[test]
#[ignore]
fn bench_on_frame_10_regions_1080p() {
    let mut img = Img::new(1920, 1080);
    let mut sim = Sim::new(Config::default(), [0, 0, 0]);
    let mut lines = Vec::new();
    for k in 0..10u32 {
        let line = Rect::new(200 + (k % 2) * 800, 60 + k * 100, 600, 60);
        img.fill(
            Rect::new(line.x - 20, line.y - 16, line.w + 40, line.h + 32),
            PANEL,
        );
        img.text(line, &format!("line {k}"), Some(PANEL));
        lines.push((line, format!("line {k}")));
    }
    sim.screen = lines;
    sim.until(&img, |s| s.layer.shown.len() == 10);
    for _ in 0..20 {
        sim.step(&img);
    }
    let runs = 600;
    let f = img.frame(sim.n);
    sim.live.on_frame(&f, &mut Paint, &mut sim.layer);
    let t = Instant::now();
    for _ in 0..runs {
        sim.live.on_frame(&f, &mut Paint, &mut sim.layer); // same time: the watch alone
    }
    let watch = t.elapsed() / runs as u32;
    let t = Instant::now();
    for _ in 0..runs {
        sim.step(&img); // 60 fps clock: the watch + the discover step at 4 Hz
    }
    let all = t.elapsed() / runs as u32;
    assert_eq!(sim.layer.shown.len(), 10);
    println!(
        "on_frame, 10 shown regions (600x60) at 1080p: watch {watch:?} per frame ({:?} per region); \
         with discover at 4 Hz {all:?} per frame",
        watch / 10
    );
}

/// Real video through a see-through box (the harness' talk clip at 720p): while the ring moves a bit,
/// a few cells of an unchanged line move a lot (the scene behind, compression noise): the patch stays.
/// The same few cells with the ring still are a new letter: hidden at once.
#[test]
fn a_moving_scene_through_the_box_is_no_new_text_a_new_letter_is() {
    for ring_moves in [true, false] {
        let (mut sim, mut img) = shown_sim("Open the gate");
        let hides = sim.layer.hides;
        if ring_moves {
            // 20 of the ring's ~200 cells, a little
            img.fill(Rect::new(52, 52, 80, 4), [24, 24, 28]);
        }
        // three 4x4 cells of the line, as a letter moves them (by ~100-230 in the type clip)
        for x in [64, 100, 140] {
            img.fill(Rect::new(x, 68, 4, 4), [230, 230, 230]);
        }
        sim.step(&img);
        assert_eq!(
            sim.layer.hides,
            hides + usize::from(!ring_moves),
            "ring moves: {ring_moves}"
        );
    }
}

/// A capture that shows our own patch (X11, Wayland, a capturable test layer) is unchanged text, also for a
/// patch of fine glyph strokes: the watch samples every second pixel, and the patch's expected cells must
/// be sampled the same way (a full average differed by ~120 here: every patch was hidden as soon as it
/// showed in the capture, measured on the Ubuntu VM).
#[test]
fn our_own_fine_patch_in_the_capture_is_no_change() {
    struct Fine;
    impl Painter for Fine {
        fn patch(&mut self, _: &Frame, region: Rect, _: &str) -> Option<(Rect, Vec<u8>)> {
            let px = (0..region.area())
                .flat_map(|i| match (i % region.w as u64) % 2 {
                    0 => [250, 250, 250, 255],
                    _ => [5, 5, 5, 255],
                })
                .collect();
            Some((region, px))
        }
    }
    let mut live = Live::new(Config {
        fade_in: Duration::ZERO,
        ..Config::default()
    });
    let (mut layer, mut tr) = (Recorder::default(), Tr::default());
    let screen = vec![(LINE, "Open the gate".to_string())];
    let mut img = scene("Open the gate", None);
    let mut patch = None;
    for n in 0..120 {
        let f = img.frame(n);
        let mut jobs = live.on_frame(&f, &mut Fine, &mut layer);
        while let Some(job) = jobs.pop() {
            for d in job.run(&f, &mut Screen(&screen), &mut Screen(&screen), &mut tr) {
                jobs.extend(live.complete(d, &f, &mut Fine, &mut layer));
            }
        }
        if layer.shows == 1 && patch.is_none() {
            patch = Some(Fine.patch(&f, LINE, "").unwrap());
        }
        // from the next frame on, the capture shows the patch
        if let Some((at, px)) = &patch {
            for y in 0..at.h {
                for x in 0..at.w {
                    let i = ((y * at.w + x) * 4) as usize;
                    img.put(at.x + x, at.y + y, [px[i], px[i + 1], px[i + 2]]);
                }
            }
        }
    }
    assert_eq!(
        (layer.shows, layer.hides),
        (1, 0),
        "shown once, never taken for new text"
    );
}

/// A background that moves all the time, fast or slowly, is never looked for text (on the fight and
/// talk clips a detect over the whole moving frame ran back to back: most of the CPU); a line that
/// appears on a box and holds still is, in an area around it alone.
#[test]
fn only_what_changed_and_held_still_is_looked_for_text() {
    for slow in [false, true] {
        let mut live = Live::new(Config::default());
        let (mut layer, frame_px) = (Recorder::default(), 320 * 180);
        let mut areas = Vec::new();
        for n in 0..180u64 {
            let mut img = scene(if n < 90 { "" } else { "Sky Rend" }, Some(n));
            if slow {
                // a ramp drifting half a pixel a frame
                for y in 0..img.h {
                    for x in 0..img.w {
                        if !(BOX.x..BOX.right()).contains(&x) || !(BOX.y..BOX.bottom()).contains(&y)
                        {
                            let v = ((2 * x as u64 + n) % 128 * 2) as u8;
                            img.put(x, y, [v, v, v]);
                        }
                    }
                }
            }
            let f = img.frame(n);
            for job in live.on_frame(&f, &mut Paint, &mut layer) {
                if let Job::Detect { job, areas: a } = job {
                    areas.push((n, a.iter().map(Rect::area).sum::<u64>()));
                    // (nothing found: the next discover may run)
                    let done = Done::Detected { job, boxes: vec![] };
                    live.complete(done, &f, &mut Paint, &mut layer);
                }
            }
        }
        // the first look takes it all in; then only the line, once it held still
        assert_eq!(areas[0], (0, frame_px));
        let later: Vec<_> = areas[1..].to_vec();
        assert!(
            !later.is_empty(),
            "the new line is looked for (slow: {slow})"
        );
        assert!(
            later.iter().all(|&(n, a)| n >= 90 && a < frame_px),
            "slow: {slow}, {later:?}"
        );
    }
}

/// A line found in two parts (its last words settled after the first detect, on the choices clip) is one
/// region: one patch, never two side by side.
#[test]
fn the_rest_of_a_line_found_later_joins_it() {
    let (left, right) = (Rect::new(60, 60, 70, 24), Rect::new(140, 60, 80, 24));
    let mut live = Live::new(Config::default());
    let mut layer = Recorder::default();
    let mut img = scene("", None);
    img.text(left, "Sky", Some(PANEL));
    let mut found = vec![left];
    for n in 0..60u64 {
        if n == 1 {
            img.text(right, "Rend", Some(PANEL));
        }
        let f = img.frame(n);
        for job in live.on_frame(&f, &mut Paint, &mut layer) {
            if let Job::Detect { job, .. } = job {
                let boxes = std::mem::take(&mut found)
                    .into_iter()
                    .map(TextBox::from)
                    .collect();
                found = vec![right];
                live.complete(Done::Detected { job, boxes }, &f, &mut Paint, &mut layer);
            }
        }
    }
    let rects: Vec<Rect> = live.regions().map(|(_, r, _)| r).collect();
    assert_eq!(rects, [left.union(&right)]);
}

/// The choices clip (local replay: "In the quiet valley" drawn 4 times): one detect finds a line in three
/// pieces, its two ends first (too far apart to be one line) and then the middle that bridges them: one
/// region, drawn once.
#[test]
fn a_line_found_in_pieces_in_one_detect_is_one_region() {
    let (a, b, c) = (
        Rect::new(60, 60, 60, 24),
        Rect::new(160, 60, 60, 24),
        Rect::new(110, 60, 60, 24),
    );
    let mut live = Live::new(Config::default());
    let mut layer = Recorder::default();
    let mut img = scene("", None);
    img.text(a.union(&b), "In the quiet valley", Some(PANEL));
    let mut found = vec![a, b, c];
    for n in 0..60u64 {
        let f = img.frame(n);
        for job in live.on_frame(&f, &mut Paint, &mut layer) {
            if let Job::Detect { job, .. } = job {
                let boxes = std::mem::take(&mut found)
                    .into_iter()
                    .map(TextBox::from)
                    .collect();
                live.complete(Done::Detected { job, boxes }, &f, &mut Paint, &mut layer);
            }
        }
    }
    let rects: Vec<Rect> = live.regions().map(|(_, r, _)| r).collect();
    assert_eq!(rects, [a.union(&b)]);
}

/// The fight clip's HUD label: a bar just as tall as its line, and a detect box padded a little past it
/// (PP-OCR's unclip), so the box's edge cells and most of its ring show the fight moving behind the bar.
/// The label never changes: drawn once, never hidden (it was hidden and drawn again every 1-2 s).
#[test]
fn a_tight_bar_over_a_moving_fight_keeps_its_patch() {
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    let bar = Rect::new(30, 58, 260, 28);
    sim.screen = vec![(Rect::new(58, 57, 164, 30), "Party Status".into())];
    for n in 0..300 {
        let mut img = Img::new(320, 180);
        img.stripes(n / 4); // (a fight that moves in steps, as video does)
        img.fill(bar, PANEL);
        img.text(LINE, "Party Status", Some(PANEL));
        sim.step(&img);
    }
    assert!(sim.layer.says("<Party Status>"));
    assert_eq!((sim.layer.shows, sim.layer.hides), (1, 0));
}

/// The fight clip's banners (local replay: 3 of 10 hidden and drawn again): read while the fight round
/// the bar held still a moment, then the fight sweeps through most of its ring. The capture keeps our
/// layer out (Windows, X11 window frames): the banner's own text is seen unchanged, so its patch stays;
/// a new banner still goes at once.
#[test]
fn a_fight_sweeping_round_a_banner_read_in_a_calm_moment_keeps_its_patch() {
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    let bar = Rect::new(30, 58, 260, 28);
    let draw = |label: &str, n: u64| {
        let mut img = Img::new(320, 180);
        if n >= 60 {
            img.stripes(n);
        }
        img.fill(bar, PANEL);
        img.text(Rect::new(60, 62, 160, 20), label, Some(PANEL));
        img
    };
    sim.screen = vec![(Rect::new(58, 54, 164, 36), "Storm Lance".into())];
    for n in 0..240 {
        sim.step(&draw("Storm Lance", n));
    }
    assert!(sim.layer.says("<Storm Lance>"));
    assert_eq!((sim.layer.shows, sim.layer.hides), (1, 0));
    let f = draw("Iron Bloom", 240);
    sim.live.on_frame(&f.frame(240), &mut Paint, &mut sim.layer);
    assert_eq!(sim.layer.hides, 1, "a new banner: hidden in that frame");
}

/// The talk clip's speaker name (W8, Windows): its box starts just inside its panel's top edge,
/// so the ring's top side is the moving scene; the other three sides are the panel: the name is held
/// (10 of 16 names were dropped as "text on a moving background").
#[test]
fn a_name_whose_box_reaches_past_its_panel_on_one_side_is_held() {
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    let (panel, name) = (Rect::new(30, 62, 260, 60), Rect::new(60, 66, 160, 24));
    sim.screen = vec![(Rect::new(58, 64, 164, 30), "Old Tavi".into())];
    for n in 0..200 {
        let mut img = Img::new(320, 180);
        img.stripes(n);
        img.fill(panel, PANEL);
        img.text(name, "Old Tavi", Some(PANEL));
        sim.step(&img);
    }
    assert!(sim.layer.says("<Old Tavi>"));
}

/// The talk clip's chapter label (local replay: hidden and drawn again 7 times in a minute): its detect box
/// is padded two cells past its panel on the left, so the box's outer columns show the scene; a shape
/// passing there long after the read is no change of the label. A new label still is.
#[test]
fn a_shape_passing_through_the_box_padding_past_its_panel_is_no_change() {
    let mut sim = Sim::new(Config::default(), [2, 3, 3]);
    let (panel, text, boxed) = (
        Rect::new(28, 40, 180, 32),
        Rect::new(40, 46, 140, 20),
        Rect::new(20, 36, 180, 40),
    );
    let draw = |label: &str, n: u64| {
        let mut img = Img::new(320, 180);
        img.fill(panel, PANEL);
        img.text(text, label, Some(PANEL));
        if n >= 100 {
            let v = (n * 37 % 200) as u8 + 40;
            img.fill(Rect::new(16, 36, 12, 40), [v, 255 - v, v / 2]);
        }
        img
    };
    sim.screen = vec![(boxed, "Chapter Two".into())];
    for n in 0..300 {
        sim.step(&draw("Chapter Two", n));
    }
    assert!(sim.layer.says("<Chapter Two>"));
    assert_eq!((sim.layer.shows, sim.layer.hides), (1, 0));
    sim.screen = vec![(boxed, "Chapter Six".into())];
    sim.until(&draw("Chapter Six", 300), |s| s.layer.says("<Chapter Six>"));
}

/// A detect over a changing scene finds no text and takes long (the fight clip: ~300 ms, ~60 % of the OCR's time
/// back to back): the next discover waits `discover_rest` times as long, and a retire still asks at once.
#[test]
fn a_long_detect_is_followed_by_a_rest() {
    let detects = |rest: u32| {
        let cfg = Config {
            discover_rest: rest,
            ..Config::default()
        };
        let (mut live, mut layer) = (Live::new(cfg), Recorder::default());
        let (mut pending, mut count): (Option<(u64, Done)>, usize) = (None, 0);
        for n in 0..600u64 {
            // a block of the picture changes every 20 frames and then holds still (an effect pausing)
            let mut img = Img::new(320, 180);
            let v = (40 + n / 20 * 53 % 200) as u8; // (never the same twice in a row)
            img.fill(Rect::new(100, 100, 120, 60), [v, v, v]);
            let f = img.frame(n);
            for job in live.on_frame(&f, &mut Paint, &mut layer) {
                if let Job::Detect { job, .. } = job {
                    // (nothing found, 30 frames = 0.5 s later)
                    pending = Some((n + 30, Done::Detected { job, boxes: vec![] }));
                    count += 1;
                }
            }
            if pending.as_ref().is_some_and(|(due, _)| *due <= n) {
                let (_, done) = pending.take().unwrap();
                live.complete(done, &f, &mut Paint, &mut layer);
            }
        }
        count
    };
    let (old, new) = (detects(0), detects(Config::default().discover_rest));
    assert!(old >= 12, "back to back: {old}");
    assert!(new * 3 <= old * 2, "with a rest: {new} of {old}");
}

/// Box memory: a box read before whose text changes with its panel still there: the patch goes in that
/// frame and the new text is read again in place, with no detect over it.
#[test]
fn a_known_box_is_read_again_in_place_without_a_detect() {
    let (mut sim, _) = shown_sim("Open the gate");
    sim.live.cfg.box_memory = true;
    let detects = sim.detects;
    let img = scene("Close the gate", None);
    sim.screen = vec![(LINE, "Close the gate".into())];
    let hides = sim.layer.hides;
    sim.step(&img);
    assert_eq!(sim.layer.hides, hides + 1, "hidden in the frame it changed");
    sim.until(&img, |s| s.layer.says("<Close the gate>"));
    assert_eq!(sim.detects, detects, "no detect over a known box");
    assert_eq!(sim.reads.last().map(String::as_str), Some("Close the gate"));
}

const TYPE_PANEL: Rect = Rect::new(20, 136, 440, 120);
/// (lighter than the test painter's black patch: an emptied box must not look like our patch)
const TYPE_FILL: [u8; 3] = [40, 44, 60];
const TYPE_ROWS: [Rect; 3] = [
    Rect::new(40, 146, 360, 22),
    Rect::new(40, 178, 360, 22),
    Rect::new(40, 210, 360, 22),
];

/// The typed box's panel with `lines` on it, the last one typed `w` px so far (`w` = None: whole);
/// sets the sim's screen to what OCR finds there.
fn typed(sim: &mut Sim, lines: &[&str], w: Option<u32>) -> Img {
    let mut img = Img::new(480, 270);
    img.fill(TYPE_PANEL, TYPE_FILL);
    sim.screen.clear();
    for (k, (row, text)) in TYPE_ROWS.iter().zip(lines).enumerate() {
        let part = match (w, k + 1 == lines.len()) {
            (Some(w), true) => w,
            _ => row.w,
        };
        let at = Rect::new(row.x, row.y, part, row.h);
        img.text(at, text, Some(TYPE_FILL));
        let shown = &text[..text.len() * part as usize / row.w as usize];
        sim.screen.push((at, shown.to_string()));
    }
    img
}

/// Types `lines` out into the box, 6 px every 2 frames (~30 letters a second at this size), asserting
/// nothing new is drawn meanwhile; returns the frame after the last letter.
fn type_out(sim: &mut Sim, lines: &[&str]) -> u64 {
    let shows = sim.layer.shows;
    for k in 1..=lines.len() {
        for w in (6..=TYPE_ROWS[0].w).step_by(6) {
            let img = typed(sim, &lines[..k], Some(w));
            for _ in 0..2 {
                sim.step(&img);
                assert_eq!(
                    sim.layer.shows, shows,
                    "nothing drawn while it types (line {k}, {w} px)"
                );
            }
        }
    }
    sim.n
}

/// A dialogue box typed out letter by letter (3 lines, each starting under the last): OCR finds only
/// what is typed so far. Nothing is drawn before the whole box is complete (a part of a sentence
/// misleads); then its whole text is read, translated and drawn ONCE, soon after the last letter (the
/// box's settle time is learned from its pauses between letters). Then auto-advance: the next
/// speaker's line types into the SAME box: the old patch goes in the frame the text is replaced, and
/// the new text is drawn once, whole, again soon after its last letter.
#[test]
fn a_typed_out_box_is_drawn_once_whole_soon_after_its_last_letter() {
    let texts = [
        "The road north is closed",
        "so the merchants wait",
        "in the valley tonight",
    ];
    let cfg = Config {
        typed_out: true,
        box_memory: true,
        ..Config::default()
    };
    let mut sim = Sim::new(cfg, [3, 3, 3]);
    sim.block = true;
    let empty = typed(&mut sim, &[], None);
    for _ in 0..30 {
        sim.step(&empty);
    }
    let last = type_out(&mut sim, &texts);
    let done = typed(&mut sim, &texts, None);
    let whole = format!("<{}>", texts.join(" "));
    sim.until(&done, |s| s.layer.says(&whole));
    let ms = |frames: u64| frames * 1000 / 60;
    assert!(
        ms(sim.n - last) <= 300,
        "drawn {} ms after the last letter",
        ms(sim.n - last)
    );
    for _ in 0..60 {
        sim.step(&done);
    }
    assert_eq!(sim.layer.shows, 1, "drawn once, whole");
    // the next speaker's line types into the same box
    let next = ["Then we leave at dawn, all of us"];
    let hides = sim.layer.hides;
    let img = typed(&mut sim, &next, Some(6));
    sim.step(&img);
    assert_eq!(
        sim.layer.hides,
        hides + 1,
        "the old patch goes in the frame its text is replaced"
    );
    let last = type_out(&mut sim, &next);
    let done = typed(&mut sim, &next, None);
    let line = format!("<{}>", next[0]);
    sim.until(&done, |s| s.layer.says(&line));
    assert!(
        ms(sim.n - last) <= 300,
        "the next line drawn {} ms after its last letter",
        ms(sim.n - last)
    );
    assert_eq!(sim.layer.shows, 2, "drawn once, whole");
}

/// One text event of a recorded clip's ground truth (`docs/internal/live-test/gen_clips.py`): on screen
/// from `from` (its first letter when typed out) to `off`, whole from `on`.
#[derive(Debug, Clone)]
struct Shot {
    id: usize,
    kind: String,
    held: bool,
    from: u64,
    on: u64,
    off: u64,
    rect: Rect,
    panel: Option<Rect>,
    text: String,
}

/// `id kind held from on off bx by bw bh (px py pw ph | - - - -) text` a line, `#` = a comment.
fn shots(src: &str) -> Vec<Shot> {
    (src.lines().filter(|l| !l.starts_with('#') && !l.is_empty()))
        .map(|l| {
            let f: Vec<&str> = l.splitn(15, ' ').collect();
            let n = |i: usize| f[i].parse::<u32>().unwrap();
            Shot {
                id: n(0) as usize,
                kind: f[1].to_string(),
                held: f[2] == "1",
                from: n(3) as u64,
                on: n(4) as u64,
                off: n(5) as u64,
                rect: Rect::new(n(6), n(7), n(8), n(9)),
                panel: (f[10] != "-").then(|| Rect::new(n(10), n(11), n(12), n(13))),
                text: f[14].to_string(),
            }
        })
        .collect()
}

impl Shot {
    /// Both typed into one box (the same panel, gone together): one text.
    fn boxed_with(&self, o: &Shot) -> bool {
        self.kind == "typed" && o.kind == "typed" && o.panel == self.panel && o.off == self.off
    }

    /// What it shows on frame `n` (typed out: its letters so far, as gen_clips.py draws them, its box as
    /// wide as they are), None = not on screen.
    fn at(&self, n: u64) -> Option<(Rect, String)> {
        if !(self.from..self.off).contains(&n) {
            return None;
        }
        if n >= self.on {
            return Some((self.rect, self.text.clone()));
        }
        let all = self.text.chars().count() as u64;
        let k = ((n - self.from + 1) * all / (self.on - self.from + 1)).max(1);
        let text: String = self.text.chars().take(k as usize).collect();
        let w = (self.rect.w as u64 * k / all).max(1) as u32;
        Some((Rect::new(self.rect.x, self.rect.y, w, self.rect.h), text))
    }
}

/// Replay knobs (`OC_BOXPAD`, `OC_LAT=detect,read,translate` frames) for the ignored whole-clip run.
fn box_pad() -> u32 {
    std::env::var("OC_BOXPAD").map_or(2, |v| v.parse().unwrap())
}
fn replay_lat() -> [u64; 3] {
    std::env::var("OC_LAT").map_or([4, 1, 3], |v| {
        let v: Vec<u64> = v.split(',').map(|x| x.parse().unwrap()).collect();
        [v[0], v[1], v[2]]
    })
}

/// A text on screen: its box, its text, the shots in it.
type Seen = (Rect, String, Vec<usize>);

/// What the screen holds on frame `n`, as OCR finds it: a line's box a little past its ink, the lines
/// typed into one box (same panel, gone together) as one block read top to bottom.
fn screen_at(all: &[Shot], n: u64, pad: u32) -> (Vec<Seen>, Vec<(Rect, String)>) {
    let mut out: Vec<Seen> = Vec::new();
    let mut lines = Vec::new();
    for s in all {
        let Some((r, t)) = s.at(n) else { continue };
        let r = r.padded(pad);
        lines.push((r, t.clone()));
        match (out.iter_mut()).find(|(_, _, ids)| s.boxed_with(&all[ids[0]])) {
            Some(b) => {
                b.0 = b.0.union(&r);
                b.1 = format!("{} {t}", b.1);
                b.2.push(s.id);
            }
            None => out.push((r, t, vec![s.id])),
        }
    }
    (out, lines)
}

/// A recorded clip's frames through the core with fake engines that know the ground truth: per text
/// (the same text again in the same box right after counts as one) the frames its patch stayed over
/// other text and how often it was drawn.
#[derive(Debug, Default)]
struct Replayed {
    /// (shot id under it, what the patch said, frames it was up over other text) per stale patch.
    stale: Vec<(usize, String, u64)>,
    /// (shot id, text, draws) per text drawn more than once.
    redraws: Vec<(usize, String, usize)>,
    /// Patches drawn for a part of their text (typed out, drawn before its last letter).
    partial: usize,
    /// Held texts drawn, of all held; frames from whole to first drawn.
    drawn: usize,
    held: usize,
    lat: Vec<u64>,
}

fn replay_frames(
    all: &[Shot],
    (w, h): (u32, u32),
    frames: std::ops::Range<u64>,
    mut next: impl FnMut(&mut Vec<u8>) -> bool,
    pad: u32,
    cfg: Config,
) -> Replayed {
    let mut chain: Vec<usize> = (0..all.len()).collect();
    for (i, s) in all.iter().enumerate() {
        if let Some(p) = (all[..i].iter()).rev().find(|p| {
            p.text == s.text && s.from == s.on && p.rect.same_place(&s.rect) && p.off == s.from
        }) {
            chain[i] = chain[p.id];
        }
    }
    let mut sim = Sim::new(cfg, replay_lat()); // detect 67 ms, read 17 ms, translate 50 ms
    sim.n = frames.start;
    let mut img = Img::new(w, h);
    let mut stale: HashMap<RegionId, (usize, String, u64)> = HashMap::new();
    let mut draws: HashMap<usize, usize> = HashMap::new();
    let mut first: HashMap<usize, u64> = HashMap::new();
    let (mut shows, mut partial) = (0, 0);
    for n in frames.clone() {
        if !next(&mut img.px) {
            break;
        }
        let (screen, lines) = screen_at(all, n, pad);
        // (a block's lines as OCR finds them: its box, each line; read as all of them under the crop)
        sim.boxes = (screen.iter())
            .map(|(at, _, _)| TextBox {
                at: *at,
                lines: (lines.iter())
                    .filter(|(l, _)| 2 * l.overlap(at) > l.area())
                    .map(|(l, _)| *l)
                    .collect(),
            })
            .collect();
        sim.screen = lines;
        sim.step(&img);
        if let Ok(t) = std::env::var("OC_TRACE")
            && let Some((a, b)) = t.split_once('-')
            && (a.parse::<u64>().unwrap()..b.parse().unwrap()).contains(&n)
        {
            let shows: Vec<_> = sim.layer.log[shows..]
                .iter()
                .map(|(_, r, t)| (r, t))
                .collect();
            eprintln!(
                "T {n} {} shows {shows:?} hides {}",
                sim.live.debug_regions(),
                sim.layer.hides
            );
        }
        let under = |r: &Rect| {
            (screen.iter())
                .filter(|(s, _, _)| s.overlap(r) > 0)
                .max_by_key(|(s, _, _)| s.overlap(r))
        };
        for (id, (r, said)) in &sim.layer.shown {
            match under(r) {
                Some((_, t, _)) if *said == format!("<{t}>") => {}
                u => {
                    let who = u.map_or(usize::MAX, |(_, _, ids)| ids[0]);
                    stale.entry(*id).or_insert((who, said.clone(), 0)).2 += 1;
                }
            }
        }
        for (_, r, said) in &sim.layer.log[shows..] {
            if let Some((_, t, ids)) = under(r)
                && *said == format!("<{t}>")
            {
                let c = chain[ids[0]];
                *draws.entry(c).or_default() += 1;
                // (from when it was whole: a typed box once its last line is)
                let whole = (all.iter())
                    .filter(|o| o.id == ids[0] || all[ids[0]].boxed_with(o))
                    .map(|o| o.on)
                    .max()
                    .unwrap_or(0);
                first.entry(c).or_insert(n.saturating_sub(whole));
                // (typed into one box: whole once all its lines are)
                let s = &all[ids[0]];
                partial += (all.iter())
                    .filter(|o| o.id == s.id || s.boxed_with(o))
                    .any(|o| n < o.on) as usize;
            }
        }
        shows = sim.layer.log.len();
    }
    let held: Vec<&Shot> = (all.iter())
        .filter(|s| s.held && chain[s.id] == s.id && s.on < frames.end && s.off > frames.start)
        .filter(|s| !all[..s.id].iter().any(|o| o.boxed_with(s)))
        .filter(|s| s.off - s.on >= 30)
        .collect();
    let mut lat: Vec<u64> = held
        .iter()
        .filter_map(|s| first.get(&s.id).copied())
        .collect();
    lat.sort();
    let mut out = Replayed {
        drawn: lat.len(),
        partial,
        held: held.len(),
        lat,
        ..Default::default()
    };
    out.stale = stale.into_values().collect();
    out.stale.sort();
    out.redraws = (draws.into_iter())
        .filter(|(_, d)| *d > 1)
        .map(|(c, d)| (c, all[c].text.clone(), d))
        .collect();
    out.redraws.sort();
    out
}

/// A whole recorded clip, decoded by ffmpeg at full size (`OC_CLIP=<dir>/<name>`: `<name>.mp4` and
/// `<name>.w31` = its truth as `shots` reads it). Run: `OC_CLIP=... cargo test --release
/// replay_a_recorded_clip -- --ignored --nocapture`.
#[test]
#[ignore]
fn replay_a_recorded_clip() {
    let Some(clip) = std::env::var_os("OC_CLIP") else {
        return;
    };
    let clip = std::path::PathBuf::from(clip);
    let all = shots(&std::fs::read_to_string(clip.with_extension("w31")).unwrap());
    let mut ff = std::process::Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i"])
        .arg(clip.with_extension("mp4"))
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = ff.stdout.take().unwrap();
    let r = replay_frames(
        &all,
        (1920, 1080),
        0..3600,
        |px| std::io::Read::read_exact(&mut out, px).is_ok(),
        box_pad(),
        Config {
            layer_seen: false,
            ..Config::default()
        },
    );
    let _ = ff.kill();
    let _ = ff.wait();
    for s in &r.stale {
        println!("stale {s:?}");
    }
    for d in &r.redraws {
        println!("redraw {d:?}");
    }
    let ms = |f: u64| f * 1000 / 60;
    println!(
        "{}: drawn {} of {} held, median {:?} ms; stale patches {} (> 1 frame {}), redraws {}, partial {}",
        clip.display(),
        r.drawn,
        r.held,
        r.lat.get(r.lat.len() / 2).map(|f| ms(*f)),
        r.stale.len(),
        r.stale.iter().filter(|s| s.2 > 1).count(),
        r.redraws.iter().map(|d| d.2 - 1).sum::<usize>(),
        r.partial
    );
}

/// A recorded fixture's frames (tests/fixtures/live_core/*.cells, written by the cutscene harness'
/// fixture_dump.py from a clip and its truth):
/// the core's 1/4-scale cells of a crop, each frame as deltas from the one before: runs of
/// (unchanged cells: a varint, changed cells: a count byte, then each delta as an i8; -128 = an i16
/// follows). Returns the size in cells and the frames.
fn recorded(data: &[u8]) -> ((u32, u32), Vec<Vec<u8>>) {
    let mut i = 0;
    let varint = |i: &mut usize| {
        let (mut v, mut shift) = (0usize, 0);
        loop {
            let b = data[*i];
            *i += 1;
            v |= ((b & 127) as usize) << shift;
            shift += 7;
            if b < 128 {
                return v;
            }
        }
    };
    let (w, h, n) = (varint(&mut i), varint(&mut i), varint(&mut i));
    let mut now = vec![0i16; w * h];
    let mut frames = Vec::with_capacity(n);
    for _ in 0..n {
        let mut k = 0;
        while k < now.len() {
            k += varint(&mut i);
            let run = data[i] as usize;
            i += 1;
            for _ in 0..run {
                let d = match data[i] as i8 {
                    -128 => {
                        i += 2;
                        i16::from_le_bytes([data[i - 1], data[i]])
                    }
                    d => d as i16,
                };
                i += 1;
                now[k] += d;
                k += 1;
            }
        }
        frames.push(now.iter().map(|&v| v as u8).collect());
    }
    ((w as u32, h as u32), frames)
}

/// The harness clips' real decoded frames around the owner-PC failures (W29: talk's subtitle -> blank
/// panel, choices' text -> text in one button, auto's and type's typed-out lines), as the app's capture
/// sees them (the layer never in them, as on Windows), through the core with fake engines that know the
/// truth: no patch left over changed text, no unchanged text drawn again, none drawn for a part of a
/// typed sentence, every held text drawn. `scripts/live-replay-check.sh` runs it in CI.
#[test]
fn recorded_frames_leave_nothing_stale_and_draw_each_text_once_whole() {
    let cases: &[(&str, &[u8], &str)] = &[
        (
            "talk-blank",
            include_bytes!("../../tests/fixtures/live_core/talk-blank.cells"),
            include_str!("../../tests/fixtures/live_core/talk-blank.w31"),
        ),
        (
            "choices-button",
            include_bytes!("../../tests/fixtures/live_core/choices-button.cells"),
            include_str!("../../tests/fixtures/live_core/choices-button.w31"),
        ),
        (
            "auto-typed",
            include_bytes!("../../tests/fixtures/live_core/auto-typed.cells"),
            include_str!("../../tests/fixtures/live_core/auto-typed.w31"),
        ),
        (
            "type-box",
            include_bytes!("../../tests/fixtures/live_core/type-box.cells"),
            include_str!("../../tests/fixtures/live_core/type-box.w31"),
        ),
    ];
    let mut bad = Vec::new();
    for (name, data, truth) in cases {
        let ((w, h), frames) = recorded(data);
        let all = shots(truth);
        let mut it = frames.iter();
        let r = replay_frames(
            &all,
            (w, h),
            0..frames.len() as u64,
            |px| {
                let Some(f) = it.next() else { return false };
                for (p, v) in px.as_chunks_mut::<4>().0.iter_mut().zip(f) {
                    *p = [*v, *v, *v, 255];
                }
                true
            },
            1,
            // (cells already: per pixel, the ring 2 cells as 8 px at full size)
            Config {
                scale: 1,
                ring_px: 2,
                layer_seen: false,
                ..Config::default()
            },
        );
        let ms = |f: u64| f * 1000 / 60;
        let redraws: usize = r.redraws.iter().map(|d| d.2 - 1).sum();
        println!(
            "recorded replay {name}: {} of {} held drawn, median {} ms; stale {}, redraws {redraws}, partial {}",
            r.drawn,
            r.held,
            r.lat.get(r.lat.len() / 2).map_or(0, |f| ms(*f)),
            r.stale.len(),
            r.partial
        );
        if !r.stale.is_empty() || redraws > 0 || r.partial > 0 || r.drawn < r.held {
            bad.push((name, r));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
