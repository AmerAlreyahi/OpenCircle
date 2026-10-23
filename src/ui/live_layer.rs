//! Live translate (DESIGN.md "Full-screen translate", Ctrl+Shift+L): a
//! borderless, monitor-sized, always-on-top window made click-through (and,
//! on Windows, invisible to capture) by `crate::live::prepare_layer`. It shows
//! only the translated patches; everything else is see-through. A thread
//! captures the monitor about once a second and runs `translate::live::Live`,
//! one step at a time, and sends what changed.
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use iced::widget::{container, image, pin, space, stack};
use iced::window::{self, Level, Position};
use iced::{ContentFit, Element, Fill, Point, Size, Task, task};

use super::{background, theme, widgets};
use crate::capture;
use crate::config;
use crate::live::{self, LayerError};
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
/// Test hook: after the first patches, the layer is saved here (PNG).
const TEST_SHOT: &str = "OPENCIRCLE_TEST_LIVE_SHOT";

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
    /// "Live · EN → AR".
    status: String,
    note: bool,
    error: Option<String>,
    patches: Vec<Shown>,
    cache: Arc<Cache>,
    /// Tells the step thread to end; set on `stop`.
    stop: Arc<AtomicBool>,
    work: Option<task::Handle>,
    shot_taken: bool,
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
            phase: Phase::Starting,
            status: format!("Live · EN → {}", cfg.translate.target.to_uppercase()),
            note: !cfg.translate.live_note_shown,
            error: None,
            patches: Vec::new(),
            cache,
            stop: Arc::new(AtomicBool::new(false)),
            work: None,
            shot_taken: false,
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

    /// The layer window, once open.
    pub fn window(&self) -> Option<window::Id> {
        match self.phase {
            Phase::Open { window, .. } => Some(window),
            Phase::Starting => None,
        }
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
                return Outcome::Task(Task::batch([steps, note]));
            }
            Msg::Prepared(Err(e)) => {
                eprintln!("[opencircle] {e}; translating once instead");
                return Outcome::OneShot;
            }
            Msg::Patches(patches) => {
                self.patches = patches;
                self.error = None;
                if let (Some(window), false) = (self.window(), self.shot_taken)
                    && std::env::var_os(TEST_SHOT).is_some()
                {
                    self.shot_taken = true;
                    return Outcome::Task(shot_later(window));
                }
            }
            Msg::Failed(e) => self.error = Some(e),
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

    /// Turns live mode off: the step thread ends (after a running step), the
    /// window closes, the patches and the step state are freed.
    pub fn stop(self) -> Task<Msg> {
        self.stop.store(true, Ordering::Relaxed);
        match self.window() {
            Some(window) => window::close(window),
            None => Task::none(),
        }
    }

    /// Captures and steps on a thread until `stop` (or the layer is dropped).
    fn run(&mut self, monitor: Monitor) -> Task<Msg> {
        let (stop, cache) = (self.stop.clone(), self.cache.clone());
        let (task, handle) = background(move |tx| {
            let (deps, target) = match crate::actions::translate::deps() {
                Ok(deps) => deps,
                Err(e) => {
                    let first = e.lines().next().unwrap_or_default().to_string();
                    let _ = tx.unbounded_send(Msg::Failed(first));
                    return;
                }
            };
            let mut steps = Steps::new();
            let mut last_error = None;
            let tick = || {
                let frame = match capture::monitor_under_cursor() {
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
                match steps.step(&frame.rgba, w, h, &target, &cache, &deps) {
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
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let Phase::Open { monitor, .. } = self.phase else {
            return space().into();
        };
        let mut layers = stack![space().width(Fill).height(Fill)];
        for p in &self.patches {
            let [x, y, w, h] = p.bounds.map(|v| v as f32 / monitor.scale);
            let patch = image(p.image.clone())
                .width(w)
                .height(h)
                .content_fit(ContentFit::Fill);
            layers = layers.push(pin(patch).position(Point::new(x, y)));
        }
        let status = widgets::layer_pill(match (&self.error, self.note) {
            (Some(e), _) => format!("{} · {e}", self.status),
            (None, true) => format!("{} · {NOTE}", self.status),
            (None, false) => self.status.clone(),
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
            phase: Phase::Starting,
            status: "Live · EN → AR".into(),
            note: true,
            error: None,
            patches: Vec::new(),
            cache: Arc::new(Cache::new(8)),
            stop: Arc::new(AtomicBool::new(false)),
            work: None,
            shot_taken: false,
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
