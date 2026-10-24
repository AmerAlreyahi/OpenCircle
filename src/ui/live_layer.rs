//! Live translate (DESIGN.md "Full-screen translate", Ctrl+Shift+L): a
//! borderless, monitor-sized, always-on-top window made click-through (and,
//! on Windows, invisible to capture) by `crate::live::prepare_layer`. It shows
//! only the translated patches; everything else is see-through. A thread
//! captures the monitor about once a second and runs `translate::live::Live`,
//! one step at a time, and sends what changed.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use iced::widget::{container, image, pin, space, stack};
use iced::window::{self, Level, Position};
use iced::{ContentFit, Element, Fill, Point, Size, Task, Vector, task};

use super::live_controller::{self, Controller, Out};
use super::{background, border, theme, widgets};
use crate::capture;
use crate::config;
use crate::live::{self, LayerError};
use crate::ocr;
use crate::translate::live::Live as Steps;
use crate::translate::pipeline::Cache;

/// Time between captures (start to start; a slow step just delays the next).
// ponytail: fixed; a `translate.live_interval` setting when someone asks for it.
/// How often live mode re-captures (Settings > Translate > Live refresh).
fn interval() -> Duration {
    Duration::from_millis(crate::config::current().translate.live_interval_ms.into())
}
/// The first live run's note, shown once.
const NOTE: &str = "Games must run borderless or windowed";
const NOTE_TIME: Duration = Duration::from_secs(6);
/// Test hook: after the first patches (or error), the layer is saved here (PNG).
const TEST_SHOT: &str = "OPENCIRCLE_TEST_LIVE_SHOT";
/// The controller starts at the monitor's top-right corner, this far in.
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
}

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
        }
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

    /// Opens the mini-controller at the monitor's top-right corner.
    fn open_controller(&mut self, m: Monitor) -> Task<Msg> {
        let size = self.ctl.size();
        let (x, y) = (m.x as f32 / m.scale, m.y as f32 / m.scale);
        let width = m.width as f32 / m.scale;
        self.ctl_at = Point::new(
            x + width - size.width - CONTROLLER_MARGIN,
            y + CONTROLLER_MARGIN,
        );
        let (id, open) = window::open(window::Settings {
            position: Position::Specific(self.ctl_at),
            size,
            decorations: false,
            resizable: false,
            transparent: cfg!(not(windows)), // Windows keys out a colour
            level: Level::AlwaysOnTop,
            exit_on_close_request: false,
            ..window::Settings::default()
        });
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
        match self.ctl.update(msg) {
            Some(Out::Stop) => return Outcome::Off,
            Some(Out::Drag) => return Outcome::Task(window::drag(id)),
            None => {}
        }
        if self.ctl.paused != before.paused {
            self.paused.store(self.ctl.paused, Ordering::Relaxed);
            self.patches.clear(); // paused = the screen as it is
        }
        let langs = |c: &Controller| (c.picker.source.clone(), c.picker.target.clone());
        if langs(&self.ctl) != langs(&before) {
            if let Ok(mut now) = self.langs.lock() {
                *now = langs(&self.ctl);
            }
            self.patches.clear(); // the next step reads everything again
        }
        let (old, new) = (before.size(), self.ctl.size());
        if old == new {
            return Outcome::Task(Task::none());
        }
        // Grows / shrinks from its right edge (it starts at the right corner).
        self.ctl_at += Vector::new(old.width - new.width, 0.0);
        self.update_mask();
        Outcome::Task(window::resize(id, new).chain(window::move_to(id, self.ctl_at)))
    }

    pub fn update(&mut self, msg: Msg) -> Outcome {
        match msg {
            Msg::Monitor(Ok(monitor)) if matches!(self.phase, Phase::Starting) => {
                let (window, open) = window::open(layer_window(monitor));
                self.phase = Phase::Open { window, monitor };
                let prepare = window::run(window, |w| match w.window_handle() {
                    Ok(handle) => live::prepare_layer(handle.as_raw()),
                    Err(e) => Err(LayerError::Os(e.to_string())),
                });
                return Outcome::Task(open.discard().chain(prepare).map(Msg::Prepared));
            }
            Msg::Monitor(Err(e)) => {
                eprintln!("[opencircle] live translate: capture failed: {e}");
                return Outcome::Off;
            }
            Msg::Prepared(Ok(layer)) => {
                let Phase::Open { monitor, .. } = self.phase else {
                    return Outcome::Off;
                };
                eprintln!(
                    "[opencircle] live translate on ({}x{}, excluded from capture: {})",
                    monitor.width, monitor.height, layer.excluded_from_capture
                );
                let steps = self.run(monitor);
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
                    eprintln!("[opencircle] live controller: {e}");
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
                eprintln!("[opencircle] {e}; translating once instead");
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
                    eprintln!("[opencircle] could not save the config: {e}");
                }
                config::set_current(cfg);
            }
            Msg::TestShot(shot) => save_test_shot(&shot),
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
        let windows = self.window().into_iter().chain(self.controller);
        Task::batch(windows.map(window::close))
    }

    /// Captures and steps on a thread until `stop` (or the layer is dropped).
    fn run(&mut self, monitor: Monitor) -> Task<Msg> {
        let (stop, cache) = (self.stop.clone(), self.cache.clone());
        let (paused, chosen, mask) = (self.paused.clone(), self.langs.clone(), self.mask.clone());
        let (task, handle) = background(move |tx| {
            // The languages are the controller's (they start as the config's).
            let start = chosen.lock().map(|l| l.clone()).unwrap_or_default();
            let mut deps = match crate::actions::translate::deps_for(&start.0, &start.1) {
                Ok((deps, _)) => deps,
                Err(e) => {
                    let first = e.lines().next().unwrap_or_default().to_string();
                    let _ = tx.unbounded_send(Msg::Failed(first));
                    return;
                }
            };
            let mut steps = Steps::new();
            let mut last_error = None;
            let mut langs = start;
            let tick = || {
                if paused.load(Ordering::Relaxed) {
                    return None;
                }
                let now = chosen.lock().map(|l| l.clone()).unwrap_or_default();
                if now != langs {
                    steps = Steps::new(); // another language: read everything again
                    match crate::actions::translate::deps_for(&now.0, &now.1) {
                        Ok((d, _)) => deps = d,
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
                border::mask(&mut frame.rgba, w, h, monitor.scale);
                if let Some(rect) = mask.lock().ok().and_then(|m| *m) {
                    live_controller::mask(&mut frame.rgba, w, h, rect);
                }
                match steps.step(&frame.rgba, w, h, target, &cache, &deps) {
                    Ok(None) => None,
                    Ok(Some(t)) => {
                        last_error = None;
                        eprintln!(
                            "[opencircle] live step: ocr {} ms, translate {} ms, render {} ms, total {} ms; {} patches",
                            t.ocr.as_millis(),
                            t.translate.as_millis(),
                            t.render.as_millis(),
                            t.total.as_millis(),
                            steps.patches().len()
                        );
                        let shown = (steps.patches().iter())
                            .map(|p| {
                                let [_, _, pw, ph] = p.bounds;
                                Shown {
                                    bounds: p.bounds,
                                    image: image::Handle::from_rgba(pw, ph, p.rgba.clone()),
                                }
                            })
                            .collect();
                        Some(Msg::Patches(shown))
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
            border::Mode::Flow,
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
        let status = self.ctl.status();
        let status = widgets::layer_pill(match (&self.error, self.note) {
            (Some(e), _) => format!("{status} · {e}"),
            (None, true) => format!("{status} · {NOTE}"),
            (None, false) => status,
        });
        layers
            .push(container(status).center_x(Fill).padding(12))
            .into()
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

/// A step error once (not every second), then nothing until it changes.
fn report(last: &mut Option<String>, e: String) -> Option<Msg> {
    if last.as_ref() == Some(&e) {
        return None;
    }
    eprintln!("[opencircle] live translate: {e}");
    *last = Some(e.clone());
    Some(Msg::Failed(e))
}

/// Calls `tick` about every `interval` until `stop` is set or `send` fails
/// (the layer is gone). One tick at a time: a slow one delays the next.
fn pump<T>(
    stop: &AtomicBool,
    interval: Duration,
    mut tick: impl FnMut() -> Option<T>,
    mut send: impl FnMut(T) -> bool,
) {
    while !stop.load(Ordering::Relaxed) {
        let start = Instant::now();
        if let Some(out) = tick()
            && !send(out)
        {
            return;
        }
        while !stop.load(Ordering::Relaxed) && start.elapsed() < interval {
            let left = interval.saturating_sub(start.elapsed());
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
        eprintln!("test shot failed: {e}");
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

    fn live() -> Live {
        Live {
            note: true,
            ..Live::new(Arc::new(Cache::new(8)), "auto", "ar")
        }
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
        assert_eq!(l.ctl_at.y, CONTROLLER_MARGIN, "top-right corner");
        let right = l.ctl_at.x + l.ctl.size().width;
        assert_eq!(right, 1920.0 - CONTROLLER_MARGIN);

        // X11: captures show it, so the step blanks its spot
        let _ = l.update(Msg::ControllerReady(Ok(false)));
        let [x, y, w, h] = l.mask.lock().unwrap().unwrap();
        assert_eq!((x, y), (right as u32 - w, 24));
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

    #[test]
    fn a_repeated_error_is_reported_once() {
        let mut last = None;
        assert!(report(&mut last, "HTTP 429".into()).is_some());
        assert!(report(&mut last, "HTTP 429".into()).is_none());
        assert!(report(&mut last, "HTTP 503".into()).is_some());
    }
}
