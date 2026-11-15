//! Live translate (DESIGN.md "Full-screen translate", Alt+Shift+L): a
//! borderless, monitor-sized, always-on-top window made click-through (and,
//! on Windows, invisible to capture) by `crate::features::live_translate::prepare_layer`. It shows
//! only the translated patches; everything else is see-through. A frame thread
//! feeds every frame the monitor shows to the live core (`crate::live_core`:
//! a changed patch goes in that frame), a work thread runs its slow jobs (OCR,
//! translation), both below normal priority; the layer gets each patch once
//! (`Msg::Show`) and its removal (`Msg::Hide`).
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};

use iced::widget::{image, pin, space, stack};
use iced::window::{self, Level, Position};
use iced::{ContentFit, Element, Point, Size, Task, Vector, task};

use crate::app::{background::background, windows};
use crate::features::circle::screens::edge as border;
use crate::features::live_translate::live::{self as engines, Errors, Found};
use crate::features::live_translate::screens::live::controller::{
    self as live_controller, Controller, Out,
};
use crate::features::live_translate::{self, LayerError};
use crate::features::text_reading;
use crate::features::translate::mt::Pair;
use crate::features::translate::pipeline::Cache;
use crate::live_core::{self as core, Done, Job, Rect};
use crate::shared::config;
use crate::shared::platform::capture::{self, frames::Frames, window_rect};
use crate::shared::theme;

/// The longest wait for a new frame: a still screen is fed again this often
/// (text that appeared and stays still settles), and the stop flag is read.
const FRAME_WAIT: Duration = Duration::from_millis(16);
/// Our own UI's place, the focused window's page and the glossary are looked
/// at again this often.
const LOOK_EVERY: Duration = Duration::from_millis(500);
/// The session's numbers go to the log this often (the harness reads them).
const STATS_EVERY: Duration = Duration::from_secs(10);
/// The first live run's note, shown once.
const NOTE_TIME: Duration = Duration::from_secs(6);
/// Test hook: after the first patches (or error), the layer is saved here (PNG).
const TEST_SHOT: &str = "OPENCIRCLE_TEST_LIVE_SHOT";
/// The controller starts at the monitor's bottom-right corner (above the
/// taskbar, clear of a maximised window's tab and title bars), this far in.
const CONTROLLER_MARGIN: f32 = 24.0;

/// The monitor the layer covers (physical px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f32,
}

/// One patch, ready to draw at `bounds` (x, y, w, h, monitor pixels).
#[derive(Debug, Clone)]
pub struct Shown {
    bounds: [u32; 4],
    image: image::Handle,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Monitor(Result<Monitor, String>),
    Prepared(Result<live_translate::Layer, LayerError>),
    /// A patch to show (drawn once; it stays until `Hide`).
    Show(u64, Shown),
    Hide(u64),
    /// Every patch down.
    Clear,
    /// The step failed (shown in the pill; the next step retries).
    Failed(String),
    NoteDone,
    TestShot(window::Screenshot),
    /// From the mini-controller.
    Controller(live_controller::Msg),
    /// The controller window is set up (`Ok(excluded from capture)`).
    ControllerReady(Result<bool, LayerError>),
    /// A window moved (logical px); the controller's matters.
    Moved(window::Id, Point),
    /// The models' download, 0..=1, then its end.
    Progress(f32),
    Downloaded(Result<(), String>),
}

/// What the languages need downloaded first (`actions::translate::missing`).
type Check = fn(&str, &str) -> Option<(u64, String, Option<&'static Pair>)>;

/// What the App must do after a message.
pub enum Outcome {
    Task(Task<Msg>),
    /// Turn live mode off (nothing to show it on).
    Off,
    /// No layer possible here: translate once instead.
    OneShot,
}

enum Phase {
    /// Finding the monitor (one capture).
    Starting,
    Open {
        window: window::Id,
        monitor: Monitor,
    },
}

pub struct Live {
    phase: Phase,
    /// The mini-controller's window (once the layer is up), its state and its
    /// top-left (logical px).
    controller: Option<window::Id>,
    ctl: Controller,
    ctl_at: Point,
    /// The controller is invisible to capture (else the step blanks it out).
    excluded: bool,
    /// Captures show the layer and its edge glow (X11; Windows before 10
    /// 2004; the capturable test hook): the step blanks the edge out.
    layer_captured: bool,
    /// What the step thread reads each time: paused, the source and target
    /// languages, and where the controller is in a capture (monitor px;
    /// `None` = not in captures).
    paused: Arc<AtomicBool>,
    langs: Arc<Mutex<(String, String)>>,
    mask: Arc<Mutex<Option<[u32; 4]>>>,
    note: bool,
    error: Option<String>,
    patches: Vec<(u64, Shown)>,
    cache: Arc<Cache>,
    /// Tells the step thread to end; set on `stop`.
    stop: Arc<AtomicBool>,
    work: Option<task::Handle>,
    shot_taken: bool,
    /// One warm OCR / translation helper while live mode is on.
    _helper: text_reading::Hold,
    /// What the languages need downloaded (a test swaps it).
    check: Check,
    /// The download asked for: the offline pair, or `Some(None)` = OCR's.
    pending: Option<Option<&'static Pair>>,
    /// The step thread runs (it starts only once the models are there).
    stepping: bool,
    download: Option<task::Handle>,
}

/// No click-through layer over other apps: native Wayland.
pub fn unsupported() -> bool {
    #[cfg(target_os = "linux")]
    return capture::wayland_session();
    #[cfg(not(target_os = "linux"))]
    false
}

impl Live {
    pub fn start(cache: Arc<Cache>) -> (Live, Task<Msg>) {
        let cfg = config::current();
        let mut live = Live {
            note: !cfg.translate.live_note_shown,
            ..Live::new(cache, &cfg.translate.source, &cfg.translate.target)
        };
        let (task, handle) = background(|tx| {
            let monitor = capture::monitor_under_cursor()
                .map(|f| Monitor {
                    x: f.x,
                    y: f.y,
                    width: f.width,
                    height: f.height,
                    scale: f.scale,
                })
                .map_err(|e| e.to_string());
            let _ = tx.unbounded_send(Msg::Monitor(monitor));
        })
        .abortable();
        live.work = Some(handle.abort_on_drop());
        (live, task)
    }

    fn new(cache: Arc<Cache>, source: &str, target: &str) -> Live {
        let offline = crate::features::translate::action::will_be_offline();
        Live {
            phase: Phase::Starting,
            controller: None,
            ctl: Controller::new(source, target, offline),
            ctl_at: Point::ORIGIN,
            excluded: false,
            layer_captured: true,
            paused: Arc::new(AtomicBool::new(false)),
            langs: Arc::new(Mutex::new((source.into(), target.into()))),
            mask: Arc::new(Mutex::new(None)),
            note: false,
            error: None,
            patches: Vec::new(),
            cache,
            stop: Arc::new(AtomicBool::new(false)),
            work: None,
            shot_taken: false,
            _helper: text_reading::hold(),
            check: crate::features::translate::action::missing,
            pending: None,
            stepping: false,
            download: None,
        }
    }

    /// Models the languages need are missing: the controller asks, and the
    /// step thread waits (paused) or isn't started. True if it asks.
    fn ask_if_missing(&mut self) -> bool {
        let (source, target) = (&self.ctl.picker.source, &self.ctl.picker.target);
        let Some((bytes, what, pair)) = (self.check)(source, target) else {
            return false;
        };
        self.ctl.ask(&what, bytes);
        self.pending = Some(pair);
        self.paused.store(true, Ordering::Relaxed);
        self.patches.clear();
        true
    }

    /// The user agreed: the download on a thread.
    fn start_download(&mut self) -> Task<Msg> {
        let source = self.ctl.picker.source.clone();
        let pair = self.pending.flatten();
        let (task, handle) = background(move |tx| {
            if cfg!(test) {
                return; // never the network (an aborted task's thread still runs)
            }
            let done =
                crate::features::translate::action::download(&source, pair, |done, total| {
                    let _ = tx.unbounded_send(Msg::Progress(done as f32 / total.max(1) as f32));
                });
            let _ = tx.unbounded_send(Msg::Downloaded(done.map_err(|e| e.to_string())));
        })
        .abortable();
        self.download = Some(handle.abort_on_drop());
        task
    }

    /// Fits the controller window to what it shows now (it was `old`
    /// big); it grows and shrinks from its right and bottom edges (it starts
    /// at the bottom right corner).
    fn fit_controller(&mut self, old: Size) -> Task<Msg> {
        let (Some(id), new) = (self.controller, self.ctl.size()) else {
            return Task::none();
        };
        if old == new {
            return Task::none();
        }
        self.ctl_at += Vector::new(old.width - new.width, old.height - new.height);
        self.update_mask();
        crate::shared::platform::monitor::resize(id, new).chain(window::move_to(id, self.ctl_at))
    }

    /// The layer window, once open.
    pub fn window(&self) -> Option<window::Id> {
        match self.phase {
            Phase::Open { window, .. } => Some(window),
            Phase::Starting => None,
        }
    }

    /// The mini-controller's window, once open.
    pub fn controller(&self) -> Option<window::Id> {
        self.controller
    }

    pub fn controller_view(&self) -> Element<'_, Msg> {
        self.ctl.view().map(Msg::Controller)
    }

    /// The notice line: the last step's error, else the first-run note.
    fn notice(&self) -> Option<String> {
        match (&self.error, self.note) {
            (Some(e), _) => Some(e.clone()),
            (None, true) => Some(crate::t!(live_note).into()),
            (None, false) => None,
        }
    }

    /// Opens the mini-controller at the monitor's bottom-right corner, above
    /// the taskbar.
    fn open_controller(&mut self, m: Monitor) -> Task<Msg> {
        self.ctl.set_notice(self.notice());
        let size = self.ctl.size();
        let (x, y) = (m.x as f32 / m.scale, m.y as f32 / m.scale);
        let screen = Size::new(m.width as f32 / m.scale, m.height as f32 / m.scale);
        let taskbar =
            crate::shared::platform::monitor::taskbar_inset(Point::new(x, y), screen, m.scale);
        self.ctl_at = Point::new(
            x + screen.width - size.width - CONTROLLER_MARGIN,
            y + screen.height - size.height - CONTROLLER_MARGIN - taskbar,
        );
        let (id, open) = windows::floating(
            window::Settings {
                position: Position::Specific(self.ctl_at),
                size,
                resizable: false,
                ..window::Settings::default()
            },
            live_controller::RADIUS,
            windows::POPUP,
        );
        self.controller = Some(id);
        let prepare = window::run(id, |w| match w.window_handle() {
            Ok(handle) => live_translate::prepare_controller(handle.as_raw()),
            Err(e) => Err(LayerError::Os(e.to_string())),
        });
        open.discard().chain(prepare).map(Msg::ControllerReady)
    }

    /// Where the step thread blanks the controller out of captures (monitor
    /// px), unless captures don't show it.
    fn update_mask(&self) {
        let Phase::Open { monitor: m, .. } = self.phase else {
            return;
        };
        let rect = (!self.excluded && self.controller.is_some()).then(|| {
            let size = self.ctl.size();
            let edge = |v: f32, origin: i32, max: u32, round: fn(f32) -> f32| {
                ((round(v * m.scale) as i32 - origin).max(0) as u32).min(max)
            };
            let x0 = edge(self.ctl_at.x, m.x, m.width, f32::floor);
            let y0 = edge(self.ctl_at.y, m.y, m.height, f32::floor);
            let x1 = edge(self.ctl_at.x + size.width, m.x, m.width, f32::ceil);
            let y1 = edge(self.ctl_at.y + size.height, m.y, m.height, f32::ceil);
            [x0, y0, x1 - x0, y1 - y0]
        });
        if let Ok(mut mask) = self.mask.lock() {
            *mask = rect;
        }
    }

    /// A controller message: pause, switch the language, hide, stop, drag.
    fn on_controller(&mut self, msg: live_controller::Msg) -> Outcome {
        let Some(id) = self.controller else {
            return Outcome::Task(Task::none());
        };
        let before = self.ctl.clone();
        let download = match self.ctl.update(msg) {
            Some(Out::Stop) => return Outcome::Off,
            Some(Out::Drag) => return Outcome::Task(window::drag(id)),
            Some(Out::Download) => self.start_download(),
            None => Task::none(),
        };
        // (while the download question is up, the thread stays paused)
        if self.ctl.paused != before.paused && self.ctl.ask.is_none() {
            self.paused.store(self.ctl.paused, Ordering::Relaxed);
            self.patches.clear(); // paused = the screen as it is
        }
        let langs = |c: &Controller| (c.picker.source.clone(), c.picker.target.clone());
        if langs(&self.ctl) != langs(&before) {
            if let Ok(mut now) = self.langs.lock() {
                *now = langs(&self.ctl);
            }
            self.patches.clear(); // the next step reads everything again
            // the new pair's models may be missing: ask (a download for
            // the old pair stops)
            (self.download, self.ctl.ask) = (None, None);
            if !self.ask_if_missing() {
                self.pending = None;
                self.paused.store(self.ctl.paused, Ordering::Relaxed);
            }
        }
        // a pair that is ready, picked while live mode waited for another
        let steps = match self.phase {
            Phase::Open { monitor, .. } if !self.stepping && self.ctl.ask.is_none() => {
                self.run(monitor)
            }
            _ => Task::none(),
        };
        let fit = self.fit_controller(before.size());
        Outcome::Task(Task::batch([download, steps, fit]))
    }

    pub fn update(&mut self, msg: Msg) -> Outcome {
        let out = self.update_inner(msg);
        // the notice follows the layer's error / note: the window fits it
        let before = self.ctl.size();
        self.ctl.set_notice(self.notice());
        match out {
            Outcome::Task(task) => Outcome::Task(Task::batch([task, self.fit_controller(before)])),
            out => out,
        }
    }

    fn update_inner(&mut self, msg: Msg) -> Outcome {
        match msg {
            Msg::Monitor(Ok(monitor)) if matches!(self.phase, Phase::Starting) => {
                let (window, open) = windows::screen(layer_window(monitor));
                self.phase = Phase::Open { window, monitor };
                let prepare = window::run(window, |w| match w.window_handle() {
                    Ok(handle) => live_translate::prepare_layer(handle.as_raw()),
                    Err(e) => Err(LayerError::Os(e.to_string())),
                });
                return Outcome::Task(open.discard().chain(prepare).map(Msg::Prepared));
            }
            Msg::Monitor(Err(e)) => {
                crate::log!("[opencircle] live translate: capture failed: {e}");
                return Outcome::Off;
            }
            Msg::Prepared(Ok(layer)) => {
                let Phase::Open { monitor, .. } = self.phase else {
                    return Outcome::Off;
                };
                crate::log!(
                    "[opencircle] live translate on ({}x{}, excluded from capture: {})",
                    monitor.width,
                    monitor.height,
                    layer.excluded_from_capture
                );
                self.layer_captured = !layer.excluded_from_capture;
                // Never a step without the models: ask first (in the controller).
                let steps = match self.ask_if_missing() {
                    true => Task::none(),
                    false => self.run(monitor),
                };
                let note = if self.note {
                    background(|tx| {
                        std::thread::sleep(NOTE_TIME);
                        let _ = tx.unbounded_send(Msg::NoteDone);
                    })
                } else {
                    Task::none()
                };
                let controller = self.open_controller(monitor);
                return Outcome::Task(Task::batch([steps, note, controller]));
            }
            Msg::Controller(msg) => return self.on_controller(msg),
            Msg::ControllerReady(ready) => {
                self.excluded = ready.unwrap_or_else(|e| {
                    crate::log!("[opencircle] live controller: {e}");
                    false
                });
                self.update_mask();
            }
            Msg::Moved(id, at) if Some(id) == self.controller => {
                self.ctl_at = at;
                self.update_mask();
            }
            Msg::Moved(..) => {}
            // A patch from just before Pause was pressed.
            Msg::Show(..) if self.ctl.paused => {}
            Msg::Prepared(Err(e)) => {
                crate::log!("[opencircle] {e}; translating once instead");
                return Outcome::OneShot;
            }
            Msg::Show(id, patch) => {
                self.patches.retain(|(p, _)| *p != id);
                self.patches.push((id, patch));
                self.error = None;
                return self.test_shot();
            }
            Msg::Hide(id) => self.patches.retain(|(p, _)| *p != id),
            Msg::Clear => self.patches.clear(),
            Msg::Failed(e) => {
                self.error = Some(e);
                return self.test_shot();
            }
            Msg::NoteDone => {
                self.note = false;
                std::thread::spawn(|| {
                    if let Err(e) = config::update(|c| c.translate.live_note_shown = true) {
                        crate::log!("[opencircle] could not save the config: {e}");
                    }
                });
            }
            Msg::TestShot(shot) => save_test_shot(&shot),
            Msg::Progress(done) => {
                if let Some(ask) = &mut self.ctl.ask {
                    ask.progress = Some(done);
                }
            }
            Msg::Downloaded(Err(e)) => {
                self.download = None;
                if let Some(ask) = &mut self.ctl.ask {
                    (ask.progress, ask.error) = (None, Some(e));
                }
            }
            Msg::Downloaded(Ok(())) => {
                self.download = None;
                let before = self.ctl.size();
                (self.ctl.ask, self.pending) = (None, None);
                self.paused.store(self.ctl.paused, Ordering::Relaxed);
                let steps = match self.phase {
                    Phase::Open { monitor, .. } if !self.stepping => self.run(monitor),
                    _ => Task::none(),
                };
                return Outcome::Task(Task::batch([steps, self.fit_controller(before)]));
            }
            Msg::Monitor(Ok(_)) => {}
        }
        Outcome::Task(Task::none())
    }

    /// Test hook: shoot the layer once, after its first patches or its first
    /// error (no key or no models: the layer and its border still show).
    fn test_shot(&mut self) -> Outcome {
        match self.window() {
            Some(window)
                if !self.shot_taken
                    && crate::shared::platform::sys::test_hook(TEST_SHOT).is_some() =>
            {
                self.shot_taken = true;
                Outcome::Task(shot_later(window))
            }
            _ => Outcome::Task(Task::none()),
        }
    }

    /// Turns live mode off: the step thread ends (after a running step), the
    /// window closes, the patches and the step state are freed.
    pub fn stop(self) -> Task<Msg> {
        self.stop.store(true, Ordering::Relaxed);
        let ours = self.window().into_iter().chain(self.controller);
        Task::batch(ours.map(window::close)) // at once: its state goes now
    }

    /// The live session on its threads until `stop` (or the layer is dropped).
    fn run(&mut self, monitor: Monitor) -> Task<Msg> {
        self.stepping = true;
        let ui = Shared {
            stop: self.stop.clone(),
            paused: self.paused.clone(),
            chosen: self.langs.clone(),
            mask: self.mask.clone(),
            edge: self.layer_captured,
        };
        let cache = self.cache.clone();
        let (task, handle) = background(move |tx| {
            session(monitor, ui, cache, |msg| tx.unbounded_send(msg).is_ok());
            // Live off: its frames and patches are freed; give the memory
            // back (glibc keeps it: the app stayed ~80 MB, not ~30).
            crate::shared::platform::sys::mem::trim_heap();
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let Phase::Open { monitor, .. } = self.phase else {
            return space().into();
        };
        // The border first: in the window's base layer (each further stack
        // child is a layer, and each layer costs a clip-mask pass per changed
        // strip), and at a fixed index, so new patches never restart it.
        let mut layers = stack![border::view(
            border::Mode::Live,
            monitor.scale,
            cfg!(windows),
            1.0,
        )];
        // (Paused: `patches` is empty.)
        for (_, p) in &self.patches {
            let [x, y, w, h] = p.bounds.map(|v| v as f32 / monitor.scale);
            let patch = image(p.image.clone())
                .width(w)
                .height(h)
                .content_fit(ContentFit::Fill);
            layers = layers.push(pin(patch).position(Point::new(x, y)));
        }
        layers.into()
    }
}

/// The layer window's theme: its background is see-through.
pub fn theme() -> iced::Theme {
    static THEME: LazyLock<iced::Theme> = LazyLock::new(|| {
        let mut palette = iced::Theme::Dark.palette();
        palette.background = theme::LAYER_CLEAR;
        iced::Theme::custom("live layer", palette)
    });
    THEME.clone()
}

/// Blanks `rects` (desktop px, x0, y0, x1, y1) out of a capture of the
/// monitor at `(x, y)`, `w` x `h` px: the parts on it, as the controller's.
/// Returns them (frame px, x, y, w, h; empty ones left out).
fn blank_own(
    rgba: &mut [u8],
    (x, y, w, h): (i32, i32, u32, u32),
    rects: &[[i32; 4]],
) -> Vec<[u32; 4]> {
    let mut blanked = Vec::new();
    for r in rects {
        let clip = |v: i32, o: i32, max: u32| (v - o).clamp(0, max as i32) as u32;
        let (x0, y0) = (clip(r[0], x, w), clip(r[1], y, h));
        let (x1, y1) = (clip(r[2], x, w), clip(r[3], y, h));
        if x1 > x0 && y1 > y0 {
            live_controller::mask(rgba, w, h, [x0, y0, x1 - x0, y1 - y0]);
            blanked.push([x0, y0, x1 - x0, y1 - y0]);
        }
    }
    blanked
}

/// What the session's threads read from the UI each time.
struct Shared {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    /// The source and target languages (the controller's).
    chosen: Arc<Mutex<(String, String)>>,
    /// Where the controller is in a capture (monitor px; `None` = not in captures).
    mask: Arc<Mutex<Option<[u32; 4]>>>,
    /// Captures show the edge glow (else it is never blanked: the black
    /// strips, ~100 px deep, cost every patch near the screen's edges, where
    /// games put subtitles).
    edge: bool,
}

/// The layer as the live core sees it: each patch sent to the UI once, each
/// removal at once (logged: the harness times them).
struct ToUi<S: FnMut(Msg) -> bool> {
    send: S,
    /// The UI is gone (the session ends).
    gone: bool,
    shows: u32,
    hides: u32,
}

impl<S: FnMut(Msg) -> bool> ToUi<S> {
    fn send(&mut self, msg: Msg) {
        self.gone |= !(self.send)(msg);
    }
}

impl<S: FnMut(Msg) -> bool> core::Layer for ToUi<S> {
    fn show(&mut self, id: u64, at: Rect, rgba: &[u8]) {
        self.shows += 1;
        crate::log!(
            "[opencircle] live: shown {id} at {},{} {}x{}",
            at.x,
            at.y,
            at.w,
            at.h
        );
        let image = image::Handle::from_rgba(at.w, at.h, rgba.to_vec());
        let bounds = [at.x, at.y, at.w, at.h];
        self.send(Msg::Show(id, Shown { bounds, image }));
    }
    // ponytail: today's layer shows a patch whole (`Config::fade_in` is 0 here);
    // the per-pixel-alpha layer (build step 4) fades it in.
    fn alpha(&mut self, _: u64, _: f32) {}
    fn hide(&mut self, id: u64) {
        self.hides += 1;
        crate::log!("[opencircle] live: hidden {id}");
        self.send(Msg::Hide(id));
    }
    fn clear(&mut self) {
        self.send(Msg::Clear);
    }
}

/// The core's thresholds here: its defaults, no fade (today's layer).
fn core_config() -> core::Config {
    core::Config {
        fade_in: Duration::ZERO,
        ..core::Config::default()
    }
}

/// Work for the work thread: a core job (with the frame a detect needs, and
/// the frame's size), or new languages.
enum Work {
    Job(Job, Option<Arc<Vec<u8>>>, (u32, u32)),
    Langs((String, String)),
}

/// A job waiting on the work thread (as `Work::Job` carries it).
type Queued = (Job, Option<Arc<Vec<u8>>>, (u32, u32));

/// A finished job: its results, how long it ran, its first error.
struct Worked {
    done: Vec<Done>,
    took: Duration,
    error: Option<String>,
}

/// The frame thread: every frame the monitor shows, our own UI blanked out
/// of it, through the live core; results from the work thread as they come.
fn session(monitor: Monitor, ui: Shared, cache: Arc<Cache>, send: impl FnMut(Msg) -> bool) {
    use crate::features::translate::glossary_screen as screen;
    use crate::features::translate::lang::is_rtl;
    text_reading::gently_here();
    // live is for games (the glossary applies; no address bar is read here)
    screen::begin();
    screen::saw_frame(false);
    let found = Arc::new(Mutex::new(Found::default()));
    let (to_work, work_rx) = mpsc::channel();
    let (work_tx, from_work) = mpsc::channel();
    let worker = {
        let (cache, found, scale) = (cache.clone(), found.clone(), monitor.scale);
        std::thread::spawn(move || work(work_rx, work_tx, &cache, &found, scale))
    };
    let mut layer = ToUi {
        send,
        gone: false,
        shows: 0,
        hides: 0,
    };
    let mut langs = ui.chosen.lock().map(|l| l.clone()).unwrap_or_default();
    let _ = to_work.send(Work::Langs(langs.clone()));
    let mut rtl = is_rtl(&langs.1);
    let region = (monitor.x, monitor.y, monitor.width, monitor.height);
    let (w, h) = (monitor.width, monitor.height);
    let mut frames = Frames::open(region);
    crate::log!("[opencircle] live: frames from {}", frames.describe());
    let mut live = core::Live::new(core_config());
    let started = Instant::now();
    let (mut last, mut blanked): (Option<Vec<u8>>, Vec<[u32; 4]>) = (None, Vec::new());
    let (mut own, mut area) = (Vec::new(), [0, 0, w, h]);
    let mut looked: Option<Instant> = None;
    let mut glossary = None;
    let (mut was_away, mut idle, mut last_error) = (false, false, None);
    let mut stats = Stats::default();
    let mut cpu = crate::shared::platform::sys::cpu::Load::default();
    let mut jobs: Vec<Job> = Vec::new();
    while !ui.stop.load(Ordering::Relaxed) && !layer.gone {
        let away = engines::away_gate(live_translate::away(), &mut was_away).is_some();
        if away || ui.paused.load(Ordering::Relaxed) {
            if !idle {
                live.stop(&mut layer);
                (idle, last) = (true, None);
                jobs.clear();
            }
            while from_work.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        idle = false;
        let now = ui.chosen.lock().map(|l| l.clone()).unwrap_or_default();
        if now != langs {
            // another language: everything is read again
            live.stop(&mut layer);
            live.forget_translations();
            jobs.clear();
            rtl = is_rtl(&now.1);
            langs = now;
            let _ = to_work.send(Work::Langs(langs.clone()));
        }
        if looked.is_none_or(|t| t.elapsed() >= LOOK_EVERY) {
            looked = Some(Instant::now());
            own = live_translate::own_windows();
            // the foreground window's page (a full-screen game: all of it),
            // unless the whole screen is asked for
            area = page_area(
                (!config::current().translate.whole_screen)
                    .then(|| window_rect::region((monitor.x, monitor.y), (w, h), monitor.scale))
                    .flatten(),
                (w, h),
            );
            // an edited glossary: its lines are translated again
            let now = crate::features::translate::glossary_store::current();
            if now != glossary {
                live.forget_translations();
                glossary = now;
            }
            // a busy PC (a game on the CPU): the core discovers less
            if let Some(load) = cpu.sample(text_reading::helper_pid()) {
                live.report_cpu(load, started.elapsed());
            }
            if stats.since.elapsed() >= STATS_EVERY {
                stats.found = found
                    .lock()
                    .map(|mut f| f.take_counts())
                    .unwrap_or_default();
                stats.log(&layer, &live, frames.describe());
            }
        }
        // finished jobs, against the newest frame
        let at = started.elapsed();
        while let Ok(r) = from_work.try_recv() {
            if let Some(e) = r.error
                && let Some(e) = engines::report_once(&mut last_error, e)
            {
                layer.send(Msg::Failed(e));
            }
            if live.report_burst(r.took, at) {
                crate::log!("[opencircle] live: OCR / translation ran 2x slower; discovering less");
            }
            let Some(rgba) = &last else { continue };
            let frame = core::Frame {
                rgba,
                width: w,
                height: h,
                at,
            };
            let mut paint = engines::Paint {
                found: &found,
                rtl,
                blanked: &blanked,
            };
            for d in r.done {
                jobs.extend(live.complete(d, &frame, &mut paint, &mut layer));
            }
        }
        // the next frame (none new: the screen is as it was)
        match frames.next(FRAME_WAIT) {
            Ok(Some(mut rgba)) if rgba.len() == (w * h * 4) as usize => {
                blanked = blank(&mut rgba, (monitor, area), &own, &ui.mask, ui.edge);
                last = Some(rgba);
                stats.new_frames += 1;
            }
            Ok(_) => {}
            Err(e) => {
                let e = crate::shared::app_language::fmt(
                    crate::shared::app_language::S::lv_capture_failed,
                    &[&e.to_string()],
                );
                if let Some(e) = engines::report_once(&mut last_error, e) {
                    layer.send(Msg::Failed(e));
                }
                std::thread::sleep(Duration::from_millis(500));
                frames = Frames::open(region);
                continue;
            }
        }
        let Some(rgba) = &last else { continue };
        let frame = core::Frame {
            rgba,
            width: w,
            height: h,
            at: started.elapsed(),
        };
        let mut paint = engines::Paint {
            found: &found,
            rtl,
            blanked: &blanked,
        };
        let t = Instant::now();
        jobs.extend(live.on_frame(&frame, &mut paint, &mut layer));
        (stats.frames, stats.watch) = (stats.frames + 1, stats.watch + t.elapsed());
        for job in jobs.drain(..) {
            let copy = matches!(job, Job::Detect { .. }).then(|| Arc::new(rgba.clone()));
            stats.count(&job);
            let _ = to_work.send(Work::Job(job, copy, (w, h)));
        }
    }
    live.stop(&mut layer);
    stats.log(&layer, &live, frames.describe());
    drop(to_work);
    let _ = worker.join();
}

/// The part of the monitor live translate reads (x0, y0, x1, y1): the
/// focused window's page, unless that is a small window (under a quarter of
/// the monitor: a browser's notification, a menu, a toast took the focus;
/// on the Windows VM Firefox's toast did, and nothing of the game was read).
fn page_area(focused: Option<[u32; 4]>, (w, h): (u32, u32)) -> [u32; 4] {
    match focused {
        Some(a)
            if 4 * u64::from(a[2] - a[0]) * u64::from(a[3] - a[1])
                >= u64::from(w) * u64::from(h) =>
        {
            a
        }
        _ => [0, 0, w, h],
    }
}

/// Blanks what is never read out of a frame (black): the edge glow where
/// captures show it (`edge`), the controller, our own windows, everything
/// outside the focused window's `area` (x0, y0, x1, y1). Returns the blanked
/// parts (frame px, x, y, w, h).
fn blank(
    rgba: &mut [u8],
    (m, area): (Monitor, [u32; 4]),
    own: &[[i32; 4]],
    mask: &Mutex<Option<[u32; 4]>>,
    edge: bool,
) -> Vec<[u32; 4]> {
    let (w, h) = (m.width, m.height);
    let mut blanked = match edge {
        true => border::mask(rgba, w, h, m.scale),
        false => Vec::new(),
    };
    if let Some(rect) = mask.lock().ok().and_then(|m| *m) {
        live_controller::mask(rgba, w, h, rect);
        blanked.push(rect);
    }
    // our own UI is never read (a menu, a card, a toast)
    blanked.extend(blank_own(rgba, (m.x, m.y, w, h), own));
    let [x0, y0, x1, y1] = area;
    let outside = [
        [0, 0, w, y0],
        [0, y1, w, h.saturating_sub(y1)],
        [0, y0, x0, y1.saturating_sub(y0)],
        [x1, y0, w.saturating_sub(x1), y1.saturating_sub(y0)],
    ];
    for r in outside.into_iter().filter(|r| r[2] > 0 && r[3] > 0) {
        live_controller::mask(rgba, w, h, r);
        blanked.push(r);
    }
    blanked
}

/// The engines for the languages the frame thread last sent (`Err`: none).
type Engines = Result<(crate::features::translate::pipeline::Real, String), String>;

/// The work thread: the core's jobs one at a time, translations first, then
/// reads, then detects (a new line shows sooner).
fn work(
    jobs: mpsc::Receiver<Work>,
    out: mpsc::Sender<Worked>,
    cache: &Cache,
    found: &Mutex<Found>,
    scale: f32,
) {
    text_reading::gently_here();
    let mut deps: Engines = Err(String::new());
    let mut queue: VecDeque<Queued> = VecDeque::new();
    let take = |w: Work, queue: &mut VecDeque<_>, deps: &mut Engines| match w {
        Work::Job(job, frame, size) => queue.push_back((job, frame, size)),
        Work::Langs((source, target)) => {
            queue.clear(); // (the frame thread dropped those jobs)
            *deps = crate::features::translate::action::deps_for(&source, &target).map(
                |(mut real, target)| {
                    real.scale = scale;
                    (real, target)
                },
            );
        }
    };
    loop {
        if queue.is_empty() {
            match jobs.recv() {
                Ok(w) => take(w, &mut queue, &mut deps),
                Err(_) => return,
            }
        }
        while let Ok(w) = jobs.try_recv() {
            take(w, &mut queue, &mut deps);
        }
        let rank = |j: &Job| match j {
            Job::Translate { .. } => 0,
            Job::Read(_) => 1,
            Job::Detect { .. } => 2,
        };
        let Some(next) = (0..queue.len()).min_by_key(|&i| rank(&queue[i].0)) else {
            continue;
        };
        let Some((job, frame, size)) = queue.remove(next) else {
            continue;
        };
        let t = Instant::now();
        let errors = Errors::default();
        let done = run_job(job, frame.as_deref(), size, &deps, cache, found, &errors);
        let error = errors.into_inner();
        let took = t.elapsed();
        if out.send(Worked { done, took, error }).is_err() {
            return;
        }
    }
}

/// One job on the engines (`frame`: the one a detect came with).
fn run_job(
    job: Job,
    frame: Option<&Vec<u8>>,
    size: (u32, u32),
    deps: &Engines,
    cache: &Cache,
    found: &Mutex<Found>,
    errors: &Errors,
) -> Vec<Done> {
    let (deps, target) = match deps {
        Ok((real, target)) => (real, target),
        Err(e) => {
            errors.replace(Some(e.lines().next().unwrap_or_default().to_string()));
            return nothing(job);
        }
    };
    let at = core::Frame {
        rgba: frame.map_or(&[][..], Vec::as_slice),
        width: size.0,
        height: size.1,
        at: Duration::ZERO,
    };
    job.run(
        &at,
        &mut engines::Detect {
            deps,
            found,
            errors,
        },
        &mut engines::Read {
            deps,
            found,
            errors,
            size,
        },
        &mut engines::Translate {
            deps,
            cache,
            target,
        },
    )
}

/// A job's results when it can't run (no engine for the languages): nothing
/// found, nothing read, no translation.
fn nothing(job: Job) -> Vec<Done> {
    match job {
        Job::Detect { job, .. } => vec![Done::Detected {
            job,
            boxes: Vec::new(),
        }],
        Job::Read(crops) => (crops.into_iter())
            .map(|c| Done::Read {
                id: c.id,
                job: c.job,
                text: String::new(),
            })
            .collect(),
        Job::Translate { id, job, .. } => vec![Done::Translated {
            id,
            job,
            text: Err(String::new()),
        }],
    }
}

/// The session's numbers, for the log.
struct Stats {
    since: Instant,
    /// Frames fed to the core (a still screen's again), new ones among them,
    /// and the core's time on them.
    frames: u32,
    new_frames: u32,
    watch: Duration,
    detects: u32,
    reads: u32,
    translates: u32,
    /// `Found::counts`: reads reused / OCR'd, patches made / declined.
    found: [u32; 4],
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            since: Instant::now(),
            frames: 0,
            new_frames: 0,
            watch: Duration::ZERO,
            detects: 0,
            reads: 0,
            translates: 0,
            found: [0; 4],
        }
    }
}

impl Stats {
    fn count(&mut self, job: &Job) {
        match job {
            Job::Detect { .. } => self.detects += 1,
            Job::Read(_) => self.reads += 1,
            Job::Translate { .. } => self.translates += 1,
        }
    }

    fn log<S: FnMut(Msg) -> bool>(&mut self, layer: &ToUi<S>, live: &core::Live, source: String) {
        let secs = self.since.elapsed().as_secs_f64().max(0.001);
        crate::log!(
            "[opencircle] live stats {secs:.0} s ({source}): {} frames ({} new, {:.0}/s), \
             core {:.0} us each; {} detects, {} reads ({} reused, {} OCR'd), {} translates; patches {} made, {} declined; \
             {} shown, {} hidden in all; regions now {:?} (changing, reading, translating, shown, idle)",
            self.frames,
            self.new_frames,
            f64::from(self.new_frames) / secs,
            self.watch.as_secs_f64() * 1e6 / f64::from(self.frames.max(1)),
            self.detects,
            self.reads,
            self.found[0],
            self.found[1],
            self.translates,
            self.found[2],
            self.found[3],
            layer.shows,
            layer.hides,
            states(live),
        );
        *self = Stats::default();
    }
}

/// How many regions are in each state (the stats line).
fn states(live: &core::Live) -> [u32; 5] {
    let mut n = [0; 5];
    for (_, _, s) in live.regions() {
        n[match s {
            core::State::Changing => 0,
            core::State::Reading { .. } => 1,
            core::State::Translating { .. } => 2,
            core::State::Shown { .. } => 3,
            core::State::Idle => 4,
        }] += 1;
    }
    n
}

fn layer_window(m: Monitor) -> window::Settings {
    window::Settings {
        position: Position::Specific(Point::new(m.x as f32 / m.scale, m.y as f32 / m.scale)),
        size: Size::new(m.width as f32, m.height as f32) * (1.0 / m.scale),
        decorations: false,
        resizable: false,
        // Windows keys out a colour instead (see `theme::LAYER_CLEAR`).
        transparent: cfg!(not(windows)),
        level: Level::AlwaysOnTop,
        exit_on_close_request: false,
        platform_specific: layer_platform(),
        ..window::Settings::default()
    }
}

/// X11: override-redirect, so the window manager never stacks a focused
/// full-screen window (the game) above the layer (GNOME X11 does that to an
/// "always on top" window: no patch was ever seen over a full-screen player).
fn layer_platform() -> window::settings::PlatformSpecific {
    #[cfg(target_os = "linux")]
    return window::settings::PlatformSpecific {
        override_redirect: true,
        ..Default::default()
    };
    #[cfg(not(target_os = "linux"))]
    window::settings::PlatformSpecific::default()
}

/// Test hook: shoot the layer once it has drawn its first patches.
fn shot_later(window: window::Id) -> Task<Msg> {
    background(|tx| {
        std::thread::sleep(Duration::from_millis(300));
        let _ = tx.unbounded_send(());
    })
    .then(move |()| window::screenshot(window))
    .map(Msg::TestShot)
}

fn save_test_shot(shot: &window::Screenshot) {
    let Some(path) = crate::shared::platform::sys::test_hook(TEST_SHOT) else {
        return;
    };
    let (w, h) = (shot.size.width, shot.size.height);
    if let Err(e) = crate::shared::picture::write_png(path.as_ref(), w, h, &shot.rgba) {
        crate::log!("test shot failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live mode with every model on disk (`check` finds nothing missing).
    fn live() -> Live {
        Live {
            note: true,
            check: |_, _| None,
            ..Live::new(Arc::new(Cache::new(8)), "auto", "ar")
        }
    }

    /// No models: the controller asks (Download / Cancel, with the size),
    /// and no step runs until the download is done. PNGs via the headless
    /// renderer (`OPENCIRCLE_RENDER_DIR=<dir>`).
    #[test]
    fn missing_models_are_asked_for_and_nothing_steps_without_them() {
        use live_controller::Msg as C;
        let mut l = Live {
            check: |_, _| {
                Some((
                    37_000_000,
                    "the English text-recognition model".into(),
                    None,
                ))
            },
            ..live()
        };
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let layer = crate::features::live_translate::Layer {
            excluded_from_capture: false,
        };
        let _ = l.update(Msg::Prepared(Ok(layer)));
        assert!(l.controller().is_some(), "the controller opens to ask");
        assert!(
            !l.stepping && l.paused.load(Ordering::Relaxed),
            "no step thread"
        );
        let texts = crate::shared::kit::testing::shot::texts(l.controller_view());
        let ask = "Live translate needs the English text-recognition model (about 40 MB). \
                   Download it? Nothing you translate leaves your PC.";
        for want in [ask, "Download", "Cancel"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        crate::shared::kit::testing::shot::save(
            "live-controller-consent",
            l.controller_view(),
            l.ctl.size(),
        );
        // Download: progress, still no step
        let _ = l.update(Msg::Controller(C::Download));
        assert!(l.download.is_some() && !l.stepping);
        let _ = l.update(Msg::Progress(0.4));
        assert_eq!(l.ctl.ask.as_ref().unwrap().progress, Some(0.4));
        crate::shared::kit::testing::shot::save(
            "live-controller-downloading",
            l.controller_view(),
            l.ctl.size(),
        );
        // it fails: the reason, and Download again
        let _ = l.update(Msg::Downloaded(Err("no network".into())));
        let ask = l.ctl.ask.as_ref().unwrap();
        assert_eq!(
            (ask.progress, ask.error.as_deref()),
            (None, Some("no network"))
        );
        assert!(!l.stepping);
        // done: the question goes, the steps start
        let asked = l.ctl.size();
        let _ = l.update(Msg::Controller(C::Download));
        let _ = l.update(Msg::Downloaded(Ok(())));
        assert!(l.stepping && l.ctl.ask.is_none() && !l.paused.load(Ordering::Relaxed));
        assert!(
            l.ctl.size().height < asked.height,
            "the controller shrinks back"
        );
        // Cancel = Stop
        assert!(matches!(l.update(Msg::Controller(C::Stop)), Outcome::Off));
    }

    const MONITOR: Monitor = Monitor {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };

    #[test]
    fn on_then_off_frees_the_layer() {
        let mut l = live();
        assert!(l.window().is_none());
        assert!(matches!(
            l.update(Msg::Monitor(Ok(MONITOR))),
            Outcome::Task(_)
        ));
        let window = l.window().expect("the layer window opens");
        // a second monitor message (stale) opens nothing more
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        assert_eq!(l.window(), Some(window));
        let patch = Shown {
            bounds: [10, 10, 2, 2],
            image: image::Handle::from_rgba(2, 2, vec![0; 16]),
        };
        let _ = l.update(Msg::Show(1, patch.clone()));
        let _ = l.update(Msg::Show(2, patch.clone()));
        let _ = l.update(Msg::Show(1, patch.clone()));
        assert_eq!(l.patches.len(), 2, "one per region");
        let _ = l.update(Msg::Hide(1));
        assert_eq!(l.patches.len(), 1);
        let _ = l.update(Msg::Failed("HTTP 503".into()));
        assert_eq!(l.error.as_deref(), Some("HTTP 503"));
        let _ = l.update(Msg::Show(3, patch));
        assert_eq!(l.error, None, "the next patch clears it");
        let _ = l.update(Msg::Clear);
        assert!(l.patches.is_empty());

        let stop = l.stop.clone();
        let close = l.stop();
        assert!(
            stop.load(Ordering::Relaxed),
            "the step thread is told to end"
        );
        assert!(close.units() > 0, "the window closes");
    }

    #[test]
    fn the_controller_pauses_switches_hides_and_stops() {
        use live_controller::Msg as C;
        let patch = || Shown {
            bounds: [10, 10, 2, 2],
            image: image::Handle::from_rgba(2, 2, vec![0; 16]),
        };
        let mut l = live();
        // before the layer is up there is no controller to act on
        assert!(matches!(
            l.update(Msg::Controller(C::Stop)),
            Outcome::Task(_)
        ));
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let _ = l.open_controller(MONITOR);
        let id = l.controller().expect("the controller opens");
        let bottom = l.ctl_at.y + l.ctl.size().height;
        let taskbar = crate::shared::platform::monitor::taskbar_inset(
            Point::ORIGIN,
            Size::new(1920.0, 1080.0),
            1.0,
        );
        assert_eq!(bottom, 1080.0 - CONTROLLER_MARGIN - taskbar, "bottom right");
        assert!(
            l.ctl.notice.is_some(),
            "the first-run note is in the controller"
        );
        let _ = l.update(Msg::NoteDone);
        assert!(
            l.ctl.notice.is_none(),
            "and goes with it, the window shrinks"
        );
        assert_eq!(
            l.ctl_at.y + l.ctl.size().height,
            bottom,
            "bottom edge fixed"
        );
        let _ = l.update(Msg::Failed("no key".into()));
        assert_eq!(l.ctl.notice.as_deref(), Some("no key"));
        let _ = l.update(Msg::Show(1, patch()));
        assert!(
            l.ctl.notice.is_none(),
            "an error clears with the next patch"
        );
        assert_eq!(l.ctl_at.y + l.ctl.size().height, bottom);
        // the language list opens upward and closes back
        let closed = l.ctl.size();
        {
            use crate::features::translate::screens::language_picker::{Msg as P, Side};
            let _ = l.update(Msg::Controller(C::Picker(P::Open(Side::Target))));
            assert!(l.ctl.size().height > closed.height);
            assert_eq!(l.ctl_at.y + l.ctl.size().height, bottom, "grows upward");
            let _ = l.update(Msg::Controller(C::Picker(P::Open(Side::Target))));
            assert_eq!(l.ctl.size(), closed);
            assert_eq!(l.ctl_at.y + closed.height, bottom);
        }
        let right = l.ctl_at.x + l.ctl.size().width;
        assert_eq!(right, 1920.0 - CONTROLLER_MARGIN);

        // X11: captures show it, so the step blanks its spot
        let _ = l.update(Msg::ControllerReady(Ok(false)));
        let [x, y, w, h] = l.mask.lock().unwrap().unwrap();
        assert_eq!((x, y), (right as u32 - w, bottom as u32 - h));
        assert_eq!(h as f32, l.ctl.size().height);
        let _ = l.update(Msg::Moved(id, Point::new(100.0, 200.0)));
        assert_eq!(l.mask.lock().unwrap().unwrap()[..2], [100, 200]);

        // pause: the session idles, the patches go, late ones are dropped
        let _ = l.update(Msg::Show(2, patch()));
        let _ = l.update(Msg::Controller(C::Pause));
        assert!(l.paused.load(Ordering::Relaxed) && l.patches.is_empty());
        let _ = l.update(Msg::Show(3, patch()));
        assert!(l.patches.is_empty());
        let _ = l.update(Msg::Controller(C::Pause));
        assert!(!l.paused.load(Ordering::Relaxed));

        // another language reaches the step thread
        use crate::features::translate::screens::language_picker::{Msg as P, Side};
        let _ = l.update(Msg::Controller(C::Picker(P::Open(Side::Target))));
        let _ = l.update(Msg::Controller(C::Picker(P::Pick("ja".into()))));
        assert_eq!(l.langs.lock().unwrap().1, "ja");
        let _ = l.update(Msg::Controller(C::Picker(P::Open(Side::Source))));
        let _ = l.update(Msg::Controller(C::Picker(P::Pick("fr".into()))));
        assert_eq!(*l.langs.lock().unwrap(), ("fr".into(), "ja".into()));

        // hiding keeps its right edge; Windows' exclusion drops the mask
        let right = l.ctl_at.x + l.ctl.size().width;
        let _ = l.update(Msg::Controller(C::Minimise));
        assert_eq!(l.ctl_at.x + l.ctl.size().width, right);
        let _ = l.update(Msg::ControllerReady(Ok(true)));
        assert_eq!(*l.mask.lock().unwrap(), None);

        assert!(matches!(l.update(Msg::Controller(C::Stop)), Outcome::Off));
        assert_eq!(l.stop().units(), 2, "the layer and the controller close");
    }

    #[test]
    fn no_layer_falls_back_to_one_shot() {
        let mut l = live();
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let wayland = LayerError::Unsupported("Wayland");
        assert!(matches!(
            l.update(Msg::Prepared(Err(wayland))),
            Outcome::OneShot
        ));
        let mut l = live();
        assert!(matches!(
            l.update(Msg::Monitor(Err("no screen".into()))),
            Outcome::Off
        ));
        assert!(l.stop().units() == 0, "no window to close");
    }

    /// The focused window's page is read; a small focused window (a toast) is
    /// not a game: the whole monitor is read then.
    #[test]
    fn a_small_focused_window_never_narrows_what_is_read() {
        let (w, h) = (1280, 720);
        assert_eq!(page_area(None, (w, h)), [0, 0, w, h]);
        assert_eq!(
            page_area(Some([0, 40, 1280, 720]), (w, h)),
            [0, 40, 1280, 720]
        );
        assert_eq!(page_area(Some([832, 384, 1248, 704]), (w, h)), [0, 0, w, h]);
    }

    /// Our own windows (a menu, a card) are blanked out of the capture the
    /// step reads, on whichever part of them lies on the monitor; the rest
    /// of the screen is read as it is.
    #[test]
    fn own_windows_are_left_out_of_what_is_read() {
        // a 40 x 20 monitor at desktop (100, 50), all white
        let (w, h) = (40u32, 20u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        // a menu half off the monitor's left edge, a card past its bottom
        let rects = [[90, 52, 110, 60], [130, 65, 200, 90]];
        let blanked = blank_own(&mut rgba, (100, 50, w, h), &rects);
        assert_eq!(blanked, [[0, 2, 10, 8], [30, 15, 10, 5]]);
        // a patch over a blanked part is never shown (it would be a black
        // box); one beside it is
        assert!(engines::over_blanked([5, 0, 10, 3], &blanked));
        assert!(engines::over_blanked([35, 18, 2, 2], &blanked));
        assert!(!engines::over_blanked([10, 2, 20, 13], &blanked));
        let black = |x: u32, y: u32| rgba[((y * w + x) * 4) as usize] == 0;
        let blanked: Vec<(u32, u32)> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| black(x, y))
            .collect();
        let want = |(x, y): (u32, u32)| (x < 10 && (2..10).contains(&y)) || (x >= 30 && y >= 15);
        assert_eq!(blanked.len(), 10 * 8 + 10 * 5);
        assert!(blanked.into_iter().all(want));
    }

    /// Where captures don't show the layer (Windows), its edge glow is never
    /// blanked: a subtitle near the bottom keeps its patch (the Windows VM drew
    /// 6 of 35 subtitles while the ~100 px edge strips were blanked anyway).
    #[test]
    fn the_edge_is_blanked_only_where_captures_show_it() {
        let m = Monitor {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
            scale: 1.0,
        };
        // the talk clip's subtitle patch on the VM (1280x720)
        let subtitle = [348, 596, 580, 50];
        let none = Mutex::new(None);
        let mut rgba = vec![200u8; 1280 * 720 * 4];
        let blanked = blank(&mut rgba, (m, [0, 0, 1280, 720]), &[], &none, false);
        assert!(blanked.is_empty() && rgba.iter().all(|&v| v == 200));
        assert!(!engines::over_blanked(subtitle, &blanked));
        // X11: the glow is in the capture, so it is blanked (and patches over it declined)
        let blanked = blank(&mut rgba, (m, [0, 0, 1280, 720]), &[], &none, true);
        assert!(!blanked.is_empty() && rgba.contains(&0));
    }
}
