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
    fn detect(&mut self, _: &Frame, areas: &[Rect]) -> Vec<Rect> {
        (self.0.iter())
            .filter(|(r, _)| areas.iter().any(|a| a.overlap(r) > 0))
            .map(|(r, _)| *r)
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
            for done in job.run(
                f,
                &mut Screen(&self.screen),
                &mut Screen(&self.screen),
                &mut self.tr,
            ) {
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
    // change it while a detect of the old screen is still out
    while !sim
        .pending
        .iter()
        .any(|(_, d)| matches!(d, Done::Detected { .. }))
    {
        let n = sim.n;
        sim.step(&draw("Short", short, n));
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
        // three 4x4 cells of the line, a lot
        for x in [64, 100, 140] {
            img.fill(Rect::new(x, 68, 4, 4), [90, 90, 90]);
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
                let boxes = std::mem::take(&mut found);
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
                let boxes = std::mem::take(&mut found);
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
