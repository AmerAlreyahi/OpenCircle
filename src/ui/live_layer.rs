//! Live translate (DESIGN.md "Full-screen translate", Ctrl+Shift+L): a
//! borderless, monitor-sized, always-on-top window made click-through (and,
//! on Windows, invisible to capture) by `crate::live::prepare_layer`. It shows
//! only the translated patches; everything else is see-through. A thread at
//! below-normal priority captures the monitor every 1.5 s (by default) and runs
//! `translate::live::Live`, one step at a time, and sends what changed.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use iced::widget::{image, pin, space, stack};
use iced::window::{self, Level, Position};
use iced::{ContentFit, Element, Point, Size, Task, Vector, task};

use super::live_controller::{self, Controller, Out};
use super::{background, border, theme, widgets, windows};
use crate::ai::AiError;
use crate::capture::{self, window_rect};
use crate::config;
use crate::live::{self, LayerError};
use crate::mt::Pair;
use crate::ocr::{self, TextBlock};
use crate::translate::lang;
use crate::translate::live::{Live as Steps, Patch};
use crate::translate::pipeline::{Cache, Deps, Real};

/// How often live mode re-captures (Settings > Translate > Live refresh;
/// 1.5 s by default): steps start on this fixed beat, and a slow step skips
/// the beats it ran over (`pump`).
fn interval() -> Duration {
    Duration::from_millis(crate::config::current().translate.live_interval_ms.into())
}
/// The first live run's note, shown once.
const NOTE: &str = "Games must run borderless or windowed";
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
    Prepared(Result<live::Layer, LayerError>),
    /// Everything the layer shows now.
    Patches(Vec<Shown>),
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
    /// What the step thread reads each time: paused, the source and target
    /// languages, and where the controller is in a capture (monitor px;
    /// `None` = not in captures).
    paused: Arc<AtomicBool>,
    langs: Arc<Mutex<(String, String)>>,
    mask: Arc<Mutex<Option<[u32; 4]>>>,
    note: bool,
    error: Option<String>,
    patches: Vec<Shown>,
    cache: Arc<Cache>,
    /// Tells the step thread to end; set on `stop`.
    stop: Arc<AtomicBool>,
    work: Option<task::Handle>,
    shot_taken: bool,
    /// One warm OCR / translation helper while live mode is on.
    _helper: ocr::Hold,
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
            let monitor = capture::monitor_under_cursor().map(|f| Monitor {
                x: f.x,
                y: f.y,
                width: f.width,
                height: f.height,
                scale: f.scale,
            });
            let _ = tx.unbounded_send(Msg::Monitor(monitor));
        })
        .abortable();
        live.work = Some(handle.abort_on_drop());
        (live, task)
    }

    fn new(cache: Arc<Cache>, source: &str, target: &str) -> Live {
        let offline = crate::actions::translate::will_be_offline();
        Live {
            phase: Phase::Starting,
            controller: None,
            ctl: Controller::new(source, target, offline),
            ctl_at: Point::ORIGIN,
            excluded: false,
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
            _helper: ocr::hold(),
            check: crate::actions::translate::missing,
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
            let done = crate::actions::translate::download(&source, pair, |done, total| {
                let _ = tx.unbounded_send(Msg::Progress(done as f32 / total.max(1) as f32));
            });
            let _ = tx.unbounded_send(Msg::Downloaded(done));
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
        window::resize(id, new).chain(window::move_to(id, self.ctl_at))
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
            (None, true) => Some(NOTE.into()),
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
        let taskbar = super::taskbar_inset(Point::new(x, y), screen, m.scale);
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
            widgets::SURFACE_RADIUS,
            windows::POPUP,
        );
        self.controller = Some(id);
        let prepare = window::run(id, |w| match w.window_handle() {
            Ok(handle) => live::prepare_controller(handle.as_raw()),
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
                    Ok(handle) => live::prepare_layer(handle.as_raw()),
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
            // A step that was running when Pause was pressed.
            Msg::Patches(_) if self.ctl.paused => {}
            Msg::Prepared(Err(e)) => {
                crate::log!("[opencircle] {e}; translating once instead");
                return Outcome::OneShot;
            }
            Msg::Patches(patches) => {
                self.patches = patches;
                self.error = None;
                return self.test_shot();
            }
            Msg::Failed(e) => {
                self.error = Some(e);
                return self.test_shot();
            }
            Msg::NoteDone => {
                self.note = false;
                let mut cfg = config::current();
                cfg.translate.live_note_shown = true;
                if let Err(e) = cfg.save() {
                    crate::log!("[opencircle] could not save the config: {e}");
                }
                config::set_current(cfg);
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
            Some(window) if !self.shot_taken && std::env::var_os(TEST_SHOT).is_some() => {
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

    /// Captures and steps on a thread until `stop` (or the layer is dropped).
    fn run(&mut self, monitor: Monitor) -> Task<Msg> {
        self.stepping = true;
        let (stop, cache) = (self.stop.clone(), self.cache.clone());
        let (paused, chosen, mask) = (self.paused.clone(), self.langs.clone(), self.mask.clone());
        let (task, handle) = background(move |tx| {
            // capture, OCR and translation after the game and the desktop
            ocr::gently_here();
            // The languages are the controller's (they start as the config's).
            let start = chosen.lock().map(|l| l.clone()).unwrap_or_default();
            let deps_for = |(source, target): &(String, String)| {
                crate::actions::translate::deps_for(source, target).map(|(real, _)| Reader {
                    real,
                    scale: monitor.scale,
                })
            };
            let mut deps = match deps_for(&start) {
                Ok(deps) => deps,
                Err(e) => {
                    let first = e.lines().next().unwrap_or_default().to_string();
                    let _ = tx.unbounded_send(Msg::Failed(first));
                    return;
                }
            };
            let mut steps = Steps::new();
            let mut last_error = None;
            let mut langs = start;
            let mut was_away = false;
            let tick = || {
                if paused.load(Ordering::Relaxed) {
                    return None;
                }
                if let Some(clear) = away_gate(live::away(), &mut was_away, &mut steps) {
                    return clear;
                }
                let now = chosen.lock().map(|l| l.clone()).unwrap_or_default();
                if now != langs {
                    steps = Steps::new(); // another language: read everything again
                    match deps_for(&now) {
                        Ok(d) => deps = d,
                        Err(e) => {
                            let first = e.lines().next().unwrap_or_default().to_string();
                            return report(&mut last_error, first);
                        }
                    }
                    langs = now;
                }
                let target = &langs.1;
                let mut frame = match capture::monitor_under_cursor() {
                    Ok(f) => f,
                    Err(e) => return report(&mut last_error, format!("capture failed: {e}")),
                };
                // ponytail: the cursor on another monitor pauses live mode;
                // capture that monitor by its origin if people use two screens.
                let here = (frame.x, frame.y, frame.width, frame.height);
                if here != (monitor.x, monitor.y, monitor.width, monitor.height) {
                    return None;
                }
                let (w, h) = (frame.width, frame.height);
                // what is blanked out of the frame (black) is never shown as a patch
                let mut blanked = border::mask(&mut frame.rgba, w, h, monitor.scale);
                if let Some(rect) = mask.lock().ok().and_then(|m| *m) {
                    live_controller::mask(&mut frame.rgba, w, h, rect);
                    blanked.push(rect);
                }
                // our own UI is never read (a menu, a card, a toast)
                let own = live::own_windows();
                blanked.extend(blank_own(&mut frame.rgba, (frame.x, frame.y, w, h), &own));
                // the foreground window's page (a full-screen game: all of
                // it), unless the whole screen is asked for
                let area = (!config::current().translate.whole_screen)
                    .then(|| window_rect::region((frame.x, frame.y), (w, h), monitor.scale))
                    .flatten()
                    .unwrap_or([0, 0, w, h]);
                let (cw, ch) = (area[2] - area[0], area[3] - area[1]);
                let page = if (cw, ch) == (w, h) {
                    frame.rgba
                } else {
                    (area[1]..area[3])
                        .flat_map(|y| {
                            let row = (y * w) as usize * 4;
                            &frame.rgba[row + area[0] as usize * 4..row + area[2] as usize * 4]
                        })
                        .copied()
                        .collect()
                };
                // new text under a patch: it goes now, not after the re-read
                let hide = |left: &[Patch]| {
                    crate::log!("[opencircle] live: stale patches hidden, {} left", left.len());
                    let _ = tx.unbounded_send(Msg::Patches(shown(left, area, &blanked)));
                };
                match steps.step_hiding(&page, cw, ch, target, &cache, &deps, hide) {
                    Ok(None) => None,
                    Ok(Some(t)) => {
                        last_error = None;
                        let d = steps.drawn();
                        crate::log!(
                            "[opencircle] live step {cw}x{ch} at {},{}: ocr {} ms, translate {} ms, \
                             render {} ms, total {} ms; {} patches, {} left (busy background {}, no room {})",
                            area[0],
                            area[1],
                            t.ocr.as_millis(),
                            t.translate.as_millis(),
                            t.render.as_millis(),
                            t.total.as_millis(),
                            steps.patches().len(),
                            d.busy + d.no_room,
                            d.busy,
                            d.no_room,
                        );
                        Some(Msg::Patches(shown(steps.patches(), area, &blanked)))
                    }
                    Err(e) => report(&mut last_error, e.to_string()),
                }
            };
            pump(&stop, interval(), tick, |msg| tx.unbounded_send(msg).is_ok());
            // Live off: its frames and patches are freed; give the memory
            // back (glibc keeps it: the app stayed ~80 MB, not ~30).
            drop(steps);
            super::trim_heap();
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
            cfg!(windows)
        )];
        // (Paused: `patches` is empty.)
        for p in &self.patches {
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

/// The step's patches (page px) as the layer shows them: moved by the
/// page's top-left in the frame (`area`), none over a `blanked` part.
fn shown(patches: &[Patch], area: [u32; 4], blanked: &[[u32; 4]]) -> Vec<Shown> {
    (patches.iter())
        .map(|p| {
            let [px, py, pw, ph] = p.bounds;
            ([px + area[0], py + area[1], pw, ph], p)
        })
        // its erase saw black there: it would draw a black box
        .filter(|(bounds, _)| !over_blanked(*bounds, blanked))
        .map(|(bounds, p)| Shown {
            bounds,
            image: image::Handle::from_rgba(bounds[2], bounds[3], p.rgba.clone()),
        })
        .collect()
}

/// Live reads (#56): Windows OCR where Auto routing says so (#62: a screen in
/// ~0.1-0.3 s, in this process, at below-normal priority) and it reads the
/// source language, else PP-OCR in the helper (also when Windows' read
/// fails). Translation as `real` does it (offline by default).
struct Reader {
    real: Real,
    /// The monitor's (Windows OCR reads small text better doubled).
    scale: f32,
}

impl Reader {
    fn system(&self) -> bool {
        let reader = config::current().selection.text_reader;
        let tag = ocr::system::language();
        let base = |t: &str| {
            t.split(['-', '_'])
                .next()
                .unwrap_or_default()
                .to_lowercase()
        };
        ocr::route(reader, Some(tag)) == ocr::Source::System
            && (self.real.source == lang::AUTO || tag.is_some_and(|t| base(t) == self.real.source))
    }
}

impl Deps for Reader {
    fn recognize(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<TextBlock>, ocr::OcrError> {
        if self.system() {
            let k = *ocr::system::passes(self.scale, w, h).last().unwrap_or(&1);
            match ocr::system::recognize(rgba, w, h, k) {
                Ok(lines) => return Ok(lines),
                Err(e) => crate::log!("[opencircle] live: Windows OCR failed ({e}); PP-OCR reads"),
            }
        }
        self.real.recognize(rgba, w, h)
    }

    fn translate(&self, texts: &[String], target: &str) -> Result<Vec<String>, AiError> {
        self.real.translate(texts, target)
    }
}

/// A patch at `bounds` (x, y, w, h) touches a part of the frame that was
/// blanked out (black), so its erase and redraw would show black there.
fn over_blanked([x, y, w, h]: [u32; 4], blanked: &[[u32; 4]]) -> bool {
    (blanked.iter()).any(|&[bx, by, bw, bh]| x < bx + bw && bx < x + w && y < by + bh && by < y + h)
}

/// A tick while the session is `away` (locked, switched, screen off: #56):
/// `Some` = skip it, nothing captured, with a message clearing the layer
/// when it just went away. Going away or coming back starts the steps
/// afresh: everything is read again.
fn away_gate(away: bool, was: &mut bool, steps: &mut Steps) -> Option<Option<Msg>> {
    let changed = std::mem::replace(was, away) != away;
    if changed {
        *steps = Steps::new();
        crate::log!(
            "[opencircle] live translate {}",
            if away {
                "stopped: the session is away"
            } else {
                "resumed"
            }
        );
    }
    away.then(|| changed.then(|| Msg::Patches(Vec::new())))
}

/// A step error once (not every second), then nothing until it changes.
fn report(last: &mut Option<String>, e: String) -> Option<Msg> {
    if last.as_ref() == Some(&e) {
        return None;
    }
    crate::log!("[opencircle] live translate: {e}");
    *last = Some(e.clone());
    Some(Msg::Failed(e))
}

/// Calls `tick` on a fixed beat of `interval` until `stop` is set or `send`
/// fails (the layer is gone). One tick at a time, never piled up: a tick
/// that runs over skips the beats it missed and the next starts on the next
/// beat (#56), so a slow PC gets fewer steps, not back-to-back ones.
fn pump<T>(
    stop: &AtomicBool,
    interval: Duration,
    mut tick: impl FnMut() -> Option<T>,
    mut send: impl FnMut(T) -> bool,
) {
    let start = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if let Some(out) = tick()
            && !send(out)
        {
            return;
        }
        let beats = (start.elapsed().as_nanos())
            .checked_div(interval.as_nanos())
            .map_or(0, |n| n + 1);
        let next = interval.saturating_mul(beats as u32);
        while !stop.load(Ordering::Relaxed) && start.elapsed() < next {
            let left = next.saturating_sub(start.elapsed());
            std::thread::sleep(left.min(Duration::from_millis(50)));
        }
    }
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
        ..window::Settings::default()
    }
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
    let Some(path) = std::env::var_os(TEST_SHOT) else {
        return;
    };
    let (w, h) = (shot.size.width, shot.size.height);
    if let Err(e) = crate::actions::write_png(path.as_ref(), w, h, &shot.rgba) {
        crate::log!("test shot failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn steps_never_overlap_and_stop_when_asked() {
        let stop = Arc::new(AtomicBool::new(false));
        let (running, ticks) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let start = Instant::now();
        let flag = stop.clone();
        let tick = || {
            assert_eq!(running.fetch_add(1, Ordering::SeqCst), 0, "overlap");
            std::thread::sleep(Duration::from_millis(5));
            running.fetch_sub(1, Ordering::SeqCst);
            if ticks.fetch_add(1, Ordering::SeqCst) == 3 {
                flag.store(true, Ordering::SeqCst); // off, during a step
            }
            Some(())
        };
        pump(&stop, Duration::from_millis(20), tick, |()| true);
        assert_eq!(ticks.load(Ordering::SeqCst), 4, "the running step ends it");
        assert!(start.elapsed() >= Duration::from_millis(60), "paced");

        // the layer went away: the thread ends at the next result
        let stop = AtomicBool::new(false);
        let mut sent = 0;
        pump(
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
        let beat = Duration::from_millis(40);
        let stop = AtomicBool::new(false);
        let begun = Instant::now();
        let mut starts = Vec::new();
        let mut first_end = Duration::ZERO;
        let tick = || {
            starts.push(begun.elapsed());
            if starts.len() == 1 {
                std::thread::sleep(beat * 3 / 2); // busy past the second beat
                first_end = begun.elapsed();
            }
            if starts.len() == 3 {
                stop.store(true, Ordering::SeqCst);
            }
            Some(())
        };
        pump(&stop, beat, tick, |()| true);
        assert_eq!(starts.len(), 3);
        // the next step waits for the busy one, and every step has a beat of
        // its own (sleeps overshoot, by ~15 ms on Windows: no exact times)
        let slot = |t: Duration| t.as_millis() / beat.as_millis();
        assert!(starts[1] >= first_end, "{starts:?}: started while busy");
        assert!(
            slot(starts[1]) >= 2,
            "{starts:?}: the busy beat was not skipped"
        );
        assert!(
            slot(starts[2]) > slot(starts[1]),
            "{starts:?}: two steps in one beat"
        );
    }

    /// #56: a locked (or switched, or dark) session captures nothing; the
    /// layer clears once as it goes away, and steps run again once back.
    #[test]
    fn live_stops_while_the_session_is_away_and_resumes_after() {
        let (mut was, mut steps) = (false, Steps::new());
        assert!(
            away_gate(false, &mut was, &mut steps).is_none(),
            "steps run"
        );
        let gone = away_gate(true, &mut was, &mut steps);
        assert!(
            matches!(gone, Some(Some(Msg::Patches(ref p))) if p.is_empty()),
            "the layer clears as it goes"
        );
        for _ in 0..3 {
            assert!(
                matches!(away_gate(true, &mut was, &mut steps), Some(None)),
                "nothing captured, nothing sent"
            );
        }
        assert!(away_gate(false, &mut was, &mut steps).is_none(), "resumed");
    }

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
        let layer = crate::live::Layer {
            excluded_from_capture: false,
        };
        let _ = l.update(Msg::Prepared(Ok(layer)));
        assert!(l.controller().is_some(), "the controller opens to ask");
        assert!(
            !l.stepping && l.paused.load(Ordering::Relaxed),
            "no step thread"
        );
        let texts = widgets::shot::texts(l.controller_view());
        let ask = "Live translate needs the English text-recognition model (about 40 MB). \
                   Download it? Nothing you translate leaves your PC.";
        for want in [ask, "Download", "Cancel"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        widgets::shot::save("live-controller-consent", l.controller_view(), l.ctl.size());
        // Download: progress, still no step
        let _ = l.update(Msg::Controller(C::Download));
        assert!(l.download.is_some() && !l.stepping);
        let _ = l.update(Msg::Progress(0.4));
        assert_eq!(l.ctl.ask.as_ref().unwrap().progress, Some(0.4));
        widgets::shot::save(
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
        let _ = l.update(Msg::Patches(vec![patch]));
        assert_eq!(l.patches.len(), 1);
        let _ = l.update(Msg::Failed("HTTP 503".into()));
        assert_eq!(l.error.as_deref(), Some("HTTP 503"));
        let _ = l.update(Msg::Patches(vec![]));
        assert_eq!(l.error, None, "the next good step clears it");

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
        let taskbar = crate::ui::taskbar_inset(Point::ORIGIN, Size::new(1920.0, 1080.0), 1.0);
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
        let _ = l.update(Msg::Patches(vec![]));
        assert!(
            l.ctl.notice.is_none(),
            "an error clears with the next good step"
        );
        assert_eq!(l.ctl_at.y + l.ctl.size().height, bottom);
        // the language list opens upward and closes back
        let closed = l.ctl.size();
        {
            use crate::ui::picker::{Msg as P, Side};
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

        // pause: the step thread skips, the patches go, late ones are dropped
        let patch = || Shown {
            bounds: [10, 10, 2, 2],
            image: image::Handle::from_rgba(2, 2, vec![0; 16]),
        };
        let _ = l.update(Msg::Patches(vec![patch()]));
        let _ = l.update(Msg::Controller(C::Pause));
        assert!(l.paused.load(Ordering::Relaxed) && l.patches.is_empty());
        let _ = l.update(Msg::Patches(vec![patch()]));
        assert!(l.patches.is_empty());
        let _ = l.update(Msg::Controller(C::Pause));
        assert!(!l.paused.load(Ordering::Relaxed));

        // another language reaches the step thread
        use crate::ui::picker::{Msg as P, Side};
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
        assert!(over_blanked([5, 0, 10, 3], &blanked));
        assert!(over_blanked([35, 18, 2, 2], &blanked));
        assert!(!over_blanked([10, 2, 20, 13], &blanked));
        let black = |x: u32, y: u32| rgba[((y * w + x) * 4) as usize] == 0;
        let blanked: Vec<(u32, u32)> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| black(x, y))
            .collect();
        let want = |(x, y): (u32, u32)| (x < 10 && (2..10).contains(&y)) || (x >= 30 && y >= 15);
        assert_eq!(blanked.len(), 10 * 8 + 10 * 5);
        assert!(blanked.into_iter().all(want));
    }

    #[test]
    fn a_repeated_error_is_reported_once() {
        let mut last = None;
        assert!(report(&mut last, "HTTP 429".into()).is_some());
        assert!(report(&mut last, "HTTP 429".into()).is_none());
        assert!(report(&mut last, "HTTP 503".into()).is_some());
    }
}
