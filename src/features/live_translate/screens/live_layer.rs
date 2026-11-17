//! Live translate (DESIGN.md "Full-screen translate", Alt+Shift+L): a
//! borderless, monitor-sized, always-on-top window made click-through (and,
//! on Windows, invisible to capture) by `crate::features::live_translate::prepare_layer`. It shows
//! only the translated patches; everything else is see-through. A frame thread
//! feeds every frame the monitor shows to the live core (`crate::live_core`:
//! a changed patch goes in that frame), a work thread runs its slow jobs (OCR,
//! translation), both below normal priority; the layer gets each patch once
//! (`Msg::Show`) and its removal (`Msg::Hide`).
//! Draw once (live-approach.md 5.5): the patches are a retained list of slots,
//! each its own layer, so only a patch that is added, fading or removed is
//! redrawn (the window compositor redraws and sends only its rectangle); a
//! removed patch leaves its slot empty instead of moving the others up. A
//! patch fades in over the core's `fade_in` (its opacity where the layer
//! blends, else a blend with the pixels under it: Windows' colour-keyed layer);
//! the layer fades out on Stop where the OS fades windows (Windows), and where it
//! blends its own pixels (X11 with a compositor) the glow and patches fade by their opacity;
//! the view stays drawn while it does (`leave`).
//! The edge: Live's Quiet glow (`border::live`), drawn once at the start and
//! never again while Live runs: in a window of its own where the layer can't
//! blend (`alpha_window`, Windows), on the layer where it can
//! (X11 with a compositor), else a thin solid line (X11 without one: the layer
//! is cut to its line and patches). Captures show it: `on_screen` takes it out
//! of ours.
//! KDE / wlroots Wayland: no window of ours can stay over a game there, so the
//! layer is a native layer-shell surface (`layer_shell`, its own thread) and
//! the game's frames come from the window the user picks in the desktop's
//! dialog (`Frames::front_window`); the slots below are mirrored to it
//! (`mirror`). GNOME (no layer-shell): one-shot per press.
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};

use iced::widget::{image, pin, space, stack};
use iced::window::{self, Level, Position};
use iced::{ContentFit, Element, Fill, Point, Size, Task, Vector, task};

use crate::app::{background::background, windows};
use crate::features::circle::screens::edge as border;
use crate::features::live_translate::live::{self as engines, Errors, Found};
use crate::features::live_translate::screens::live::controller::{
    self as live_controller, Controller, Out,
};
use crate::features::live_translate::{self, LayerError};
use crate::features::text_reading;
use crate::features::translate::gpu;
use crate::features::translate::mt::Pair;
use crate::features::translate::pipeline::Cache;
use crate::live_core::{self as core, Done, Job, Rect};
use crate::shared::config;
use crate::shared::kit::motion;
use crate::shared::on_screen::{self, Drawn};
use crate::shared::platform::alpha_window::AlphaWindow;
use crate::shared::platform::capture::{self, frames::Frames, window_rect};
use crate::shared::platform::layer_shell::{self, LayerShell};
use crate::shared::platform::shape;
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

/// One patch, ready to draw at `bounds` (x, y, w, h, monitor pixels); `under`:
/// the pixels it covers (RGBA, same size), where it fades in as a blend with
/// them (a layer that doesn't blend).
#[derive(Debug, Clone)]
pub struct Shown {
    bounds: [u32; 4],
    image: image::Handle,
    under: Option<Arc<Vec<u8>>>,
}

/// A patch in its slot: how far it faded in, and what is drawn for that.
#[derive(Debug, Clone)]
struct Slot {
    id: u64,
    shown: Shown,
    alpha: f32,
    drawn: image::Handle,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Monitor(Result<Monitor, String>),
    /// The layer is set up, with the edge glow's own window where it has one.
    Prepared(Result<(live_translate::Layer, Option<Arc<AlphaWindow>>), LayerError>),
    /// A patch to show (drawn once; it stays until `Hide`), faded in by `Alpha`.
    Show(u64, Shown),
    Alpha(u64, f32),
    /// A step of the edge's fade-in (drawn on the layer).
    EdgeFade,
    /// The fade-out on Stop is over: close for good.
    Closed,
    /// A patch down (sent at that instant: the log says how long it took to reach the layer).
    Hide(u64, Instant),
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
        /// The iced layer window (`None`: the native layer-shell surface).
        window: Option<window::Id>,
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
    /// What the layer can do (once it is set up).
    caps: Option<live_translate::Layer>,
    /// The edge: its own window (Windows), what of it captures show (taken
    /// out of ours while registered), whether the layer draws it (`Some(keyed)`:
    /// the solid line) and how far it faded in there, and the line's strips
    /// (x, y, w, h) where the layer is cut to it.
    glow: Option<Arc<AlphaWindow>>,
    glow_drawn: Option<Drawn>,
    drawn: Option<on_screen::OnScreen>,
    border: Option<bool>,
    /// The layer blends (a compositor keeps its pixels): only what changed
    /// goes to it (`windows::present_changes_only`).
    _changes_only: Option<crate::shared::platform::windows::ChangesOnly>,
    edge: (motion::Shown, f32),
    /// Stop is fading the glow and the patches out (the layer blends); `Closed` ends it.
    leaving: bool,
    line: Vec<[u32; 4]>,
    /// What the step thread reads each time: paused, the source and target
    /// languages, and where the controller is in a capture (monitor px;
    /// `None` = not in captures).
    paused: Arc<AtomicBool>,
    langs: Arc<Mutex<(String, String)>>,
    mask: Arc<Mutex<Option<[u32; 4]>>>,
    note: bool,
    error: Option<String>,
    /// The retained patches: a slot each, `None` once removed (the slots after
    /// it stay where they are: their layers are not redrawn).
    patches: Vec<Option<Slot>>,
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
    /// The native layer (KDE / wlroots Wayland) and what it was sent: per
    /// patch, the picture's id and its opacity.
    native: Option<LayerShell>,
    sent: HashMap<u64, (iced::advanced::image::Id, f32)>,
}

/// How live translate shows its patches on this desktop.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Here {
    /// A layer window of ours (Windows, X11).
    Window,
    /// Wayland with layer-shell AND a window screen cast (KDE, wlroots): a
    /// native layer over the game, whose own pixels are read.
    LayerShell,
    /// Wayland without them (GNOME): translate once per press.
    OneShot,
}

fn here() -> Here {
    #[cfg(target_os = "linux")]
    if capture::wayland_session() {
        return choose(
            true,
            layer_shell::available(),
            capture::screencast::can_stream_window(),
        );
    }
    Here::Window
}

/// `here` from what the desktop can do.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn choose(wayland: bool, layer_shell: bool, window_cast: bool) -> Here {
    match (wayland, layer_shell && window_cast) {
        (false, _) => Here::Window,
        (true, true) => Here::LayerShell,
        (true, false) => Here::OneShot,
    }
}

/// No click-through layer over other apps (GNOME Wayland).
pub fn unsupported() -> bool {
    here() == Here::OneShot
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
            caps: None,
            glow: None,
            glow_drawn: None,
            drawn: None,
            border: None,
            _changes_only: None,
            edge: (motion::Shown::opening(motion::WIDGET, Instant::now()), 0.0),
            leaving: false,
            line: Vec::new(),
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
            native: None,
            sent: HashMap::new(),
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
            Phase::Open { window, .. } => window,
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
            Some(Out::Stop) => return self.leave(),
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
        self.mirror();
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
                // the glow, made once: for its own window if the layer gets one
                let (w, h) = (monitor.width, monitor.height);
                let (strips, drawn) = border::live(w, h, monitor.scale, false).unzip();
                self.glow_drawn = drawn;
                if here() == Here::LayerShell {
                    return self.open_native(monitor, strips.unwrap_or_default());
                }
                let (window, open) = windows::screen(layer_window(monitor));
                self.phase = Phase::Open {
                    window: Some(window),
                    monitor,
                };
                let at = (monitor.x, monitor.y, w, h);
                let prepare = window::run(window, move |w| {
                    let handle = w
                        .window_handle()
                        .map_err(|e| LayerError::Os(e.to_string()))?;
                    let layer = live_translate::prepare_layer(handle.as_raw())?;
                    let glow = match (layer.blends, strips) {
                        (false, Some(strips)) => {
                            // (Windows: the layer is colour-keyed; the glow in an alpha window under it)
                            crate::shared::platform::alpha_window::show(handle.as_raw(), at, strips)
                        }
                        _ => None,
                    };
                    Ok((layer, glow.map(Arc::new)))
                });
                return Outcome::Task(open.discard().chain(prepare).map(Msg::Prepared));
            }
            Msg::Monitor(Err(e)) => {
                crate::log!("[opencircle] live translate: capture failed: {e}");
                return Outcome::Off;
            }
            Msg::Prepared(Ok((layer, glow))) => {
                let Phase::Open { monitor, .. } = self.phase else {
                    return Outcome::Off;
                };
                let fade = self.set_edge(monitor, layer, glow);
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
                return Outcome::Task(Task::batch([steps, note, controller, fade]));
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
                self.take(id);
                let slot = Slot {
                    id,
                    drawn: patch.image.clone(),
                    shown: patch,
                    alpha: 0.0,
                };
                match self.patches.iter_mut().find(|s| s.is_none()) {
                    Some(free) => *free = Some(slot),
                    None => self.patches.push(Some(slot)),
                }
                self.error = None;
                let shot = self.test_shot();
                return Outcome::Task(Task::batch([into_task(shot), self.cut()]));
            }
            Msg::Alpha(id, alpha) => {
                let slot = self.patches.iter_mut().flatten().find(|s| s.id == id);
                if let Some(slot) = slot {
                    slot.fade(alpha);
                }
            }
            Msg::EdgeFade => self.edge.1 = self.edge.0.at(Instant::now()),
            Msg::Closed => return Outcome::Off,
            Msg::Hide(id, sent) => {
                crate::log!(
                    "[opencircle] live: hide {id} reached the layer +{:.1} ms",
                    sent.elapsed().as_secs_f64() * 1000.0
                );
                self.take(id);
                return Outcome::Task(self.cut());
            }
            Msg::Clear => {
                self.patches.clear();
                return Outcome::Task(self.cut());
            }
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

    /// Stop pressed: the layer fades out with its view still drawn (the window must not be
    /// repainted without it: its default background is opaque, a white flash), then `Closed`
    /// ends it as `Outcome::Off`. Where the layer blends (X11 with a compositor) the glow and
    /// the patches fade by their opacity; where the OS fades windows (Windows) it fades the
    /// window, and the glow's own window goes at once. Else (or pressed again) it closes at once.
    pub fn leave(&mut self) -> Outcome {
        let blends = self.caps.is_some_and(|c| c.blends);
        let id = self.window();
        if id.is_none() && self.native.is_none() {
            return Outcome::Off;
        }
        if self.leaving || !(blends || shape::can_fade()) {
            return Outcome::Off;
        }
        self.leaving = true;
        self.stop.store(true, Ordering::Relaxed);
        if let (false, Some(id)) = (blends, id) {
            self.glow = None; // fades out on its own thread, as it is dropped
            return Outcome::Task(
                windows::fade(id, false, windows::POPUP, true).chain(Task::done(Msg::Closed)),
            );
        }
        self.edge.0.close(Instant::now());
        Outcome::Task(background(|tx| {
            let end = Instant::now() + motion::WIDGET.close.0 + motion::FRAME;
            while Instant::now() < end && tx.unbounded_send(Msg::EdgeFade).is_ok() {
                std::thread::sleep(motion::FRAME);
            }
            let _ = tx.unbounded_send(Msg::EdgeFade);
            let _ = tx.unbounded_send(Msg::Closed);
        }))
    }

    /// Turns live mode off: the step thread ends (after a running step), the
    /// window closes, the patches and the step state are freed (`leave` fades first).
    pub fn stop(self) -> Task<Msg> {
        self.stop.store(true, Ordering::Relaxed);
        // the edge's own window fades out on its thread (dropped with `self`)
        let layer = self.window().map(window::close);
        let controller = self.controller.map(window::close);
        Task::batch(layer.into_iter().chain(controller))
    }

    /// The edge, once the layer is set up (`layer`, the glow's own window if
    /// it got one): its own window, or on the layer as the glow or (no
    /// blending) the solid line; what captures will show of it registered;
    /// the fade-in on the layer.
    fn set_edge(
        &mut self,
        m: Monitor,
        layer: live_translate::Layer,
        glow: Option<Arc<AlphaWindow>>,
    ) -> Task<Msg> {
        let glow_drawn = self.glow_drawn.take();
        let keyed = glow.is_none() && !layer.blends;
        let drawn = match keyed {
            false => glow_drawn,
            true => border::live(m.width, m.height, m.scale, true).map(|(_, d)| d),
        };
        let (w, h) = (m.width, m.height);
        match (&glow, keyed) {
            (Some(_), _) => {
                crate::log!("[opencircle] live edge: its own window, drawn once ({w}x{h})")
            }
            (None, false) => {
                crate::log!("[opencircle] live edge: on the layer, drawn once ({w}x{h})")
            }
            (None, true) => {
                crate::log!("[opencircle] live edge: a solid line, drawn once ({w}x{h})")
            }
        }
        self.line = match layer.cut {
            true => border::live_line(m.width, m.height, m.scale),
            false => Vec::new(),
        };
        self.drawn = drawn.map(|d| on_screen::show(d.at((m.x, m.y))));
        self._changes_only = layer
            .blends
            .then(crate::shared::platform::windows::present_changes_only);
        self.caps = Some(layer);
        self.border = glow.is_none().then_some(keyed);
        self.glow = glow;
        self.edge = (motion::Shown::opening(motion::WIDGET, Instant::now()), 0.0);
        let fade = match self.border {
            // a step a frame till it is in (an `alpha_window` fades by itself)
            Some(_) => background(|tx| {
                let end = Instant::now() + motion::WIDGET.open.0 + motion::FRAME;
                while Instant::now() < end && tx.unbounded_send(Msg::EdgeFade).is_ok() {
                    std::thread::sleep(motion::FRAME);
                }
                let _ = tx.unbounded_send(Msg::EdgeFade);
            }),
            None => Task::none(),
        };
        Task::batch([fade, self.cut()])
    }

    /// The native layer-shell surface over `monitor` (KDE / wlroots Wayland),
    /// showing `strips` (the glow; it fades in like the layer's own): it blends
    /// per pixel, captures show it (`on_screen` takes the glow out of ours).
    fn open_native(&mut self, monitor: Monitor, strips: layer_shell::Strips) -> Outcome {
        #[cfg(target_os = "linux")]
        let opened = LayerShell::open((monitor.width, monitor.height), strips);
        #[cfg(not(target_os = "linux"))]
        let opened: Result<LayerShell, String> = Err(format!("{} strips", strips.len()));
        match opened {
            Ok(native) => {
                self.native = Some(native);
                self.phase = Phase::Open {
                    window: None,
                    monitor,
                };
                let layer = live_translate::Layer {
                    excluded_from_capture: false,
                    blends: true,
                    cut: false,
                };
                Outcome::Task(Task::done(Msg::Prepared(Ok((layer, None)))))
            }
            Err(e) => {
                crate::log!("[opencircle] live layer: {e}; translating once instead");
                Outcome::OneShot
            }
        }
    }

    /// The native layer shows what the slots show (each change sent once):
    /// the glow at its fade, each patch at its opacity (times the fade-out on
    /// Stop), a patch gone from the slots hidden.
    fn mirror(&mut self) {
        let Some(native) = &self.native else {
            return;
        };
        native.edge(self.edge.1);
        let mut now = HashMap::new();
        for s in self.patches.iter().flatten() {
            let image::Handle::Rgba { id, pixels, .. } = &s.shown.image else {
                continue;
            };
            let alpha = match self.leaving {
                true => s.alpha * self.edge.1,
                false => s.alpha,
            };
            let was = self.sent.remove(&s.id);
            if was.is_none_or(|(i, _)| i != *id) {
                native.show(s.id, s.shown.bounds, pixels.to_vec());
            }
            if was.is_none_or(|w| w != (*id, alpha)) {
                native.alpha(s.id, alpha);
            }
            now.insert(s.id, (*id, alpha));
        }
        for gone in self.sent.keys() {
            native.hide(*gone);
        }
        self.sent = now;
    }

    /// Takes the patch `id` out of its slot (the slot stays, empty; empty
    /// slots at the end go).
    fn take(&mut self, id: u64) {
        for slot in &mut self.patches {
            if slot.as_ref().is_some_and(|s| s.id == id) {
                *slot = None;
            }
        }
        while self.patches.last().is_some_and(Option::is_none) {
            self.patches.pop();
        }
    }

    /// Where the layer is cut to what it shows (no compositor): cut it to its
    /// edge line and patches again.
    fn cut(&self) -> Task<Msg> {
        let (Some(caps), Some(window)) = (self.caps, self.window()) else {
            return Task::none();
        };
        if !caps.cut {
            return Task::none();
        }
        let parts: Vec<(crate::shared::platform::shape::Px, u32)> = (self.line.iter())
            .chain(self.patches.iter().flatten().map(|s| &s.shown.bounds))
            .map(|&[x, y, w, h]| ((x as i32, y as i32, w, h), 0))
            .collect();
        window::run(window, move |w| {
            if let Ok(handle) = w.window_handle()
                && let Err(e) = crate::shared::platform::shape::parts(handle.as_raw(), &parts)
            {
                crate::log!("[opencircle] live layer cut: {e}");
            }
        })
        .discard()
    }

    /// The live session on its threads until `stop` (or the layer is dropped).
    fn run(&mut self, monitor: Monitor) -> Task<Msg> {
        self.stepping = true;
        let ui = Shared {
            stop: self.stop.clone(),
            paused: self.paused.clone(),
            chosen: self.langs.clone(),
            mask: self.mask.clone(),
            under: !self.caps.is_some_and(|c| c.blends),
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

    /// Patches up now (tests).
    #[cfg(test)]
    fn shown(&self) -> usize {
        self.patches.iter().flatten().count()
    }

    pub fn view(&self) -> Element<'static, Msg> {
        let Phase::Open { monitor, .. } = self.phase else {
            return space().into();
        };
        // The border first: in the window's base layer (each further stack
        // child is a layer, and each layer costs a clip-mask pass per changed
        // strip), and at a fixed index, so new patches never restart it.
        let base: Element<'static, Msg> = match self.border {
            Some(keyed) => border::view(border::Mode::Live, monitor.scale, keyed, self.edge.1),
            // (the window's size: the stack takes its first child's)
            None => space().width(Fill).height(Fill).into(),
        };
        let mut layers = stack![base];
        // (Paused: `patches` is empty.) A slot each, empty ones too: a layer
        // keeps its index, so the others are never redrawn.
        for slot in &self.patches {
            let Some(s) = slot.as_ref().filter(|s| s.alpha > 0.0) else {
                layers = layers.push(space());
                continue;
            };
            let [x, y, w, h] = s.shown.bounds.map(|v| v as f32 / monitor.scale);
            // a blend with what is under it is drawn whole; else its opacity
            let opacity = if s.shown.under.is_some() {
                1.0
            } else if self.leaving {
                s.alpha * self.edge.1
            } else {
                s.alpha
            };
            let patch = image(s.drawn.clone())
                .width(w)
                .height(h)
                .opacity(opacity)
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

impl Slot {
    /// The patch `alpha` of the way in: its opacity, or (`under` kept) a blend
    /// with the pixels it covers; whole at 1 (`under` is let go then).
    fn fade(&mut self, alpha: f32) {
        self.alpha = alpha.clamp(0.0, 1.0);
        let whole = self.alpha >= 1.0;
        if whole {
            self.shown.under = None;
            self.drawn = self.shown.image.clone();
            return;
        }
        let (
            Some(under),
            image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            },
        ) = (&self.shown.under, &self.shown.image)
        else {
            return;
        };
        if under.len() != pixels.len() {
            return;
        }
        // only where the patch draws (its clear pixels stay clear: on the
        // colour-keyed layer a part-clear one would show the key colour)
        let a = self.alpha;
        let mix = |p: u8, u: u8| (f32::from(u) + (f32::from(p) - f32::from(u)) * a).round() as u8;
        let mut blend = vec![0u8; pixels.len()];
        let px = (pixels.as_chunks::<4>().0.iter()).zip(under.as_chunks::<4>().0);
        for ((p, u), out) in px.zip(blend.as_chunks_mut::<4>().0) {
            if p[3] > 0 {
                *out = [mix(p[0], u[0]), mix(p[1], u[1]), mix(p[2], u[2]), p[3]];
            }
        }
        self.drawn = image::Handle::from_rgba(*width, *height, blend);
    }
}

/// An `Outcome` that is a task, as a task (the rest: none).
fn into_task(out: Outcome) -> Task<Msg> {
    match out {
        Outcome::Task(t) => t,
        _ => Task::none(),
    }
}

/// What the session's threads read from the UI each time.
struct Shared {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    /// The source and target languages (the controller's).
    chosen: Arc<Mutex<(String, String)>>,
    /// Where the controller is in a capture (monitor px; `None` = not in captures).
    mask: Arc<Mutex<Option<[u32; 4]>>>,
    /// Patches fade in as a blend with what they cover (the layer doesn't
    /// blend): each is sent with those pixels.
    under: bool,
}

/// The layer as the live core sees it: each patch sent to the UI once, each
/// removal at once (logged: the harness times them).
struct ToUi<S: FnMut(Msg) -> bool> {
    send: S,
    /// The UI is gone (the session ends).
    gone: bool,
    /// Patches go with the pixels they cover (`Shared::under`).
    under: bool,
    shows: u32,
    hides: u32,
    /// Fade-in steps sent (each redraws only its patch).
    fades: u32,
    /// When the screen showed the frame the core works on (its present time where the OS gives it).
    shown_at: Instant,
}

impl<S: FnMut(Msg) -> bool> ToUi<S> {
    fn send(&mut self, msg: Msg) {
        self.gone |= !(self.send)(msg);
    }
}

/// The layer with the frame the core works on: a patch shown takes the pixels
/// it covers from it.
struct Over<'a, S: FnMut(Msg) -> bool> {
    ui: &'a mut ToUi<S>,
    frame: &'a core::Frame<'a>,
}

impl<S: FnMut(Msg) -> bool> core::Layer for Over<'_, S> {
    fn show(&mut self, id: u64, at: Rect, rgba: &[u8]) {
        let under = self.ui.under.then(|| Arc::new(crop(self.frame, at)));
        self.ui.show_with(id, at, rgba, under);
    }
    fn alpha(&mut self, id: u64, alpha: f32) {
        self.ui.alpha(id, alpha);
    }
    fn hide(&mut self, id: u64) {
        self.ui.hide(id);
    }
    fn clear(&mut self) {
        self.ui.clear();
    }
}

/// `at` of `frame` (RGBA rows; outside the frame: clear).
fn crop(frame: &core::Frame, at: Rect) -> Vec<u8> {
    let mut out = vec![0; (at.w * at.h * 4) as usize];
    for row in 0..at.h {
        let y = at.y + row;
        if y >= frame.height || at.x >= frame.width {
            continue;
        }
        let w = at.w.min(frame.width - at.x) as usize * 4;
        let from = ((y * frame.width + at.x) * 4) as usize;
        let to = (row * at.w * 4) as usize;
        if let Some(src) = frame.rgba.get(from..from + w) {
            out[to..to + w].copy_from_slice(src);
        }
    }
    out
}

impl<S: FnMut(Msg) -> bool> ToUi<S> {
    fn show_with(&mut self, id: u64, at: Rect, rgba: &[u8], under: Option<Arc<Vec<u8>>>) {
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
        self.send(Msg::Show(
            id,
            Shown {
                bounds,
                image,
                under,
            },
        ));
    }
}

impl<S: FnMut(Msg) -> bool> core::Layer for ToUi<S> {
    fn show(&mut self, id: u64, at: Rect, rgba: &[u8]) {
        self.show_with(id, at, rgba, None);
    }
    fn alpha(&mut self, id: u64, alpha: f32) {
        self.fades += 1;
        self.send(Msg::Alpha(id, alpha));
    }
    fn hide(&mut self, id: u64) {
        self.hides += 1;
        crate::log!(
            "[opencircle] live: hidden {id} +{:.1} ms after the screen showed its frame",
            self.shown_at.elapsed().as_secs_f64() * 1000.0
        );
        self.send(Msg::Hide(id, Instant::now()));
    }
    fn clear(&mut self) {
        self.send(Msg::Clear);
    }
}

/// The core's thresholds here: its defaults (a patch fades in over 120 ms).
fn core_config() -> core::Config {
    core::Config::default()
}

/// Work for the work thread: a core job (with the frame a detect needs, and
/// the frame's size), or new languages.
enum Work {
    Job(Job, Option<Arc<Vec<u8>>>, (u32, u32)),
    Langs((String, String)),
    /// The graphics card's translator is ready (translations go there from now on).
    Gpu(Box<dyn OnGpu>),
}

/// A translator on the graphics card (`translate::gpu::GpuTranslator`; a test's fake).
pub(crate) trait OnGpu: core::Translator + Send {
    /// Lines got ~2x slower (the GPU rule's back-off): the processor for the rest of the session.
    fn slowed(&self) -> bool;
    /// Into another language (the controller's picker).
    fn set_target(&mut self, target: &str);
}

impl OnGpu for gpu::GpuTranslator {
    fn slowed(&self) -> bool {
        gpu::GpuTranslator::slowed(self)
    }
    fn set_target(&mut self, target: &str) {
        gpu::GpuTranslator::set_target(self, target);
    }
}

/// Where the GPU rule allows it, the graphics card's translator for the session, off every other
/// thread: the probe takes ~0.3 s, the first time the helper and model download (~1.2 GB) while the
/// processor translates; none (logged by `gpu::live`) = the processor all along.
fn start_gpu(target: String, to_work: mpsc::Sender<Work>) {
    start_gpu_with(gpu_wanted(), target, to_work, |t, p| gpu::live(t, p));
}

/// The GPU path is built but off (owner, 2026-11-16: no real leap over the processor): only a test
/// hook turns it on, so no probe, download or helper happens in normal use.
fn gpu_wanted() -> bool {
    crate::shared::platform::sys::test_hook(gpu::TEST_HOOK).is_some()
}

/// `start_gpu`'s body with the switch and the one GPU call (`gpu::live`) as arguments.
fn start_gpu_with<T: OnGpu + 'static>(
    wanted: bool,
    target: String,
    to_work: mpsc::Sender<Work>,
    live: impl FnOnce(&str, &mut dyn FnMut(u64, u64)) -> Option<T> + Send + 'static,
) {
    if !wanted {
        return;
    }
    std::thread::spawn(move || {
        let mut tenth = 0;
        let got = live(&target, &mut |done, total| {
            let now = done * 10 / total.max(1);
            if now > tenth {
                tenth = now;
                crate::log!("[opencircle] live: graphics card files {}0 %", now);
            }
        });
        if let Some(t) = got {
            let _ = to_work.send(Work::Gpu(Box::new(t)));
        }
    });
}

/// A job waiting on the work thread (as `Work::Job` carries it).
type Queued = (Job, Option<Arc<Vec<u8>>>, (u32, u32));

/// A finished job: its results, how long it ran, its first error.
struct Worked {
    done: Vec<Done>,
    /// `rank` of the job (translate, read, detect).
    kind: usize,
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
        under: ui.under,
        shows: 0,
        hides: 0,
        fades: 0,
        shown_at: Instant::now(),
    };
    let mut langs = ui.chosen.lock().map(|l| l.clone()).unwrap_or_default();
    let _ = to_work.send(Work::Langs(langs.clone()));
    start_gpu(langs.1.clone(), to_work.clone());
    let mut rtl = is_rtl(&langs.1);
    let region = (monitor.x, monitor.y, monitor.width, monitor.height);
    let (w, h) = (monitor.width, monitor.height);
    let mut frames = Frames::front_window(region);
    crate::log!("[opencircle] live: frames from {}", frames.describe());
    let mut live = core::Live::new(core_config());
    let started = Instant::now();
    log_clock();
    let (mut last, mut blanked): (Option<Vec<u8>>, Vec<[u32; 4]>) = (None, Vec::new());
    let (mut own, mut area) = (Vec::new(), [0, 0, w, h]);
    let mut looked: Option<Instant> = None;
    let mut glossary = None;
    let (mut was_away, mut idle, mut last_error) = (false, false, None);
    // grabs in a row that showed the same screen (grabs come as fast as asked: no "nothing new")
    let mut same = 0u32;
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
            if let Some(load) = cpu.sample(&ours()) {
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
            stats.busy[r.kind] += r.took;
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
                let mut over = Over {
                    ui: &mut layer,
                    frame: &frame,
                };
                jobs.extend(live.complete(d, &frame, &mut paint, &mut over));
            }
            log_notes(live.take_notes());
        }
        // the next frame (none new: the screen is as it was)
        match frames.next(FRAME_WAIT) {
            Ok(Some(mut rgba)) if rgba.len() == (w * h * 4) as usize => {
                // (our edge only where this frame shows the screen: not in the
                // game window's own pixels)
                let edge = !frames.of_window();
                blanked = blank(&mut rgba, (monitor, area), &own, &ui.mask, edge);
                if last.as_ref() == Some(&rgba) {
                    same = same.saturating_add(1);
                } else {
                    (last, same) = (Some(rgba), 0);
                    stats.new_frames += 1;
                    let age = frames.age();
                    layer.shown_at = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
                    stats.age += age;
                }
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
                // (a window the user picked: never asked again by itself)
                if !frames.reopen_asks() {
                    frames = Frames::front_window(region);
                }
                continue;
            }
        }
        if let Some(wait) = still_wait(same, frames.fast() || frames.of_window()) {
            std::thread::sleep(wait);
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
        let mut over = Over {
            ui: &mut layer,
            frame: &frame,
        };
        jobs.extend(live.on_frame(&frame, &mut paint, &mut over));
        (stats.frames, stats.watch) = (stats.frames + 1, stats.watch + t.elapsed());
        log_notes(live.take_notes());
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

/// The session clock (`Frame::at`, the notes' times) against the wall clock, once: the harness
/// lines the notes up with the clip's frames by it.
fn log_clock() {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    crate::log!("[opencircle] live: clock 0 = epoch {ms} ms");
}

/// The core's notes as log lines (the harness's stage timings and line lifetimes).
fn log_notes(notes: Vec<core::Note>) {
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    for n in notes {
        match n {
            core::Note::Drawn {
                id,
                how,
                seen,
                detected,
                settled,
                read,
                translated,
                shown,
            } => crate::log!(
                "[opencircle] live: drawn {id} {how:?} seen {:.0} ms, detect +{}, settle +{:.0}, read +{:.0}, \
                 translate +{:.0}, draw +{:.0} (total {:.0} ms)",
                ms(seen),
                detected.map_or("-".into(), |d| format!("{:.0}", ms(d.saturating_sub(seen)))),
                ms(settled.saturating_sub(detected.unwrap_or(seen))),
                ms(read.saturating_sub(settled)),
                ms(translated.saturating_sub(read)),
                ms(shown.saturating_sub(translated)),
                ms(shown.saturating_sub(seen)),
            ),
            core::Note::Gone {
                id,
                life,
                drawn,
                typed,
                settle,
            } => crate::log!(
                "[opencircle] live: gone {id} after {:.0} ms, drawn {drawn}, typed {typed} (settle {:.0} ms)",
                ms(life),
                ms(settle),
            ),
        }
    }
}

/// A screen grabbed again and again (`waits` = false: no window frames, no screen frames that
/// come only on a change) that showed the same picture `same` times in a row is grabbed ~10 times a
/// second instead of 60 (an idle Live took a quarter of a core on the X11 screen fallback); the
/// first change brings 60 back.
fn still_wait(same: u32, waits: bool) -> Option<Duration> {
    (!waits && same >= 6).then_some(Duration::from_millis(84))
}

/// Our own helper processes (OCR, the graphics card's translator): their load is ours, not a
/// busy PC's.
fn ours() -> Vec<u32> {
    [text_reading::helper_pid(), gpu::helper_pid()]
        .into_iter()
        .flatten()
        .collect()
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

/// Blanks what is never read out of a frame (black): what we drew over the
/// screen where the frame shows it (`edge`: taken out by `on_screen`, blended
/// pixels worked back, solid ones blanked), the controller, our own windows,
/// everything outside the focused window's `area` (x0, y0, x1, y1). Returns
/// the blanked parts (frame px, x, y, w, h).
fn blank(
    rgba: &mut [u8],
    (m, area): (Monitor, [u32; 4]),
    own: &[[i32; 4]],
    mask: &Mutex<Option<[u32; 4]>>,
    edge: bool,
) -> Vec<[u32; 4]> {
    let (w, h) = (m.width, m.height);
    let mut blanked = match edge {
        true => on_screen::take_out(rgba, (m.x, m.y, w, h)),
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
    let mut gpu: Option<Box<dyn OnGpu>> = None;
    let mut queue: VecDeque<Queued> = VecDeque::new();
    let take = |w: Work, queue: &mut VecDeque<_>, deps: &mut Engines, gpu: &mut Option<_>| match w {
        Work::Job(job, frame, size) => queue.push_back((job, frame, size)),
        Work::Gpu(t) => *gpu = Some(t),
        Work::Langs((source, target)) => {
            queue.clear(); // (the frame thread dropped those jobs)
            if let Some(g) = gpu.as_mut() {
                g.set_target(&target);
            }
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
                Ok(w) => take(w, &mut queue, &mut deps, &mut gpu),
                Err(_) => return,
            }
        }
        while let Ok(w) = jobs.try_recv() {
            take(w, &mut queue, &mut deps, &mut gpu);
        }
        let Some(next) = (0..queue.len()).min_by_key(|&i| rank(&queue[i].0)) else {
            continue;
        };
        let Some((job, frame, size)) = queue.remove(next) else {
            continue;
        };
        let (t, kind) = (Instant::now(), rank(&job));
        let errors = Errors::default();
        let done = run_job(
            job,
            frame.as_deref(),
            size,
            &deps,
            (cache, &mut gpu),
            found,
            &errors,
        );
        let error = errors.into_inner();
        let took = t.elapsed();
        let worked = Worked {
            done,
            kind,
            took,
            error,
        };
        if out.send(worked).is_err() {
            return;
        }
    }
}

/// The work thread's order: translations, then reads, then detects.
fn rank(job: &Job) -> usize {
    match job {
        Job::Translate { .. } => 0,
        Job::Read(_) => 1,
        Job::Detect { .. } => 2,
    }
}

/// One job on the engines (`frame`: the one a detect came with).
fn run_job(
    job: Job,
    frame: Option<&Vec<u8>>,
    size: (u32, u32),
    deps: &Engines,
    (cache, gpu): (&Cache, &mut Option<Box<dyn OnGpu>>),
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
    let mut cpu = engines::Translate {
        deps,
        cache,
        target,
    };
    let done = job.run(
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
        translator(gpu, &mut cpu),
    );
    back_off(gpu);
    done
}

/// The graphics card's translator while there is one, else the processor's.
fn translator<'a>(
    gpu: &'a mut Option<Box<dyn OnGpu>>,
    cpu: &'a mut dyn core::Translator,
) -> &'a mut dyn core::Translator {
    match gpu.as_deref_mut() {
        Some(g) => g,
        None => cpu,
    }
}

/// The GPU rule's back-off: a slowed graphics card translator is dropped (its helper ends), the
/// processor translates for the rest of the session.
fn back_off(gpu: &mut Option<Box<dyn OnGpu>>) {
    if gpu.as_ref().is_some_and(|g| g.slowed()) {
        *gpu = None;
        crate::log!(
            "[opencircle] live: the graphics card got ~2x slower; the processor translates from now on"
        );
    }
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
    /// The work thread's time on translations, reads, detects.
    busy: [Duration; 3],
    /// New frames' ages when read (from the screen showing them: `Frames::age`).
    age: Duration,
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
            busy: [Duration::ZERO; 3],
            age: Duration::ZERO,
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
             {} shown, {} hidden in all; regions now {:?} (changing, reading, translating, shown, idle); \
             work busy {:.0} % (detect {:.0} ms, read {:.0} ms, translate {:.0} ms); \
             {} fade steps; new frames {:.1} ms old when read",
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
            self.busy.iter().sum::<Duration>().as_secs_f64() * 100.0 / secs,
            self.busy[2].as_secs_f64() * 1000.0,
            self.busy[1].as_secs_f64() * 1000.0,
            self.busy[0].as_secs_f64() * 1000.0,
            layer.fades,
            self.age.as_secs_f64() * 1000.0 / f64::from(self.new_frames.max(1)),
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
            blends: true,
            cut: false,
        };
        let _ = l.update(Msg::Prepared(Ok((layer, None))));
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
        // Cancel = Stop (this layer blends: it fades out first, then it is off)
        assert!(matches!(
            l.update(Msg::Controller(C::Stop)),
            Outcome::Task(_)
        ));
        assert!(matches!(l.update(Msg::Closed), Outcome::Off));
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
            under: None,
        };
        let _ = l.update(Msg::Show(1, patch.clone()));
        let _ = l.update(Msg::Show(2, patch.clone()));
        let _ = l.update(Msg::Show(1, patch.clone()));
        assert_eq!(l.shown(), 2, "one per region");
        let _ = l.update(Msg::Hide(1, Instant::now()));
        assert_eq!(l.shown(), 1);
        let _ = l.update(Msg::Failed("HTTP 503".into()));
        assert_eq!(l.error.as_deref(), Some("HTTP 503"));
        let _ = l.update(Msg::Show(3, patch));
        assert_eq!(l.error, None, "the next patch clears it");
        let _ = l.update(Msg::Clear);
        assert!(l.shown() == 0);

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
            under: None,
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
        assert!(l.paused.load(Ordering::Relaxed) && l.shown() == 0);
        let _ = l.update(Msg::Show(3, patch()));
        assert!(l.shown() == 0);
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

        // Stop: where the OS fades windows (Windows) it fades first and `Closed` ends it
        let stopped = l.update(Msg::Controller(C::Stop));
        if shape::can_fade() {
            assert!(matches!(stopped, Outcome::Task(_)) && l.leaving);
            assert!(matches!(l.update(Msg::Closed), Outcome::Off));
        } else {
            assert!(matches!(stopped, Outcome::Off));
        }
        assert!(l.stop().units() >= 2, "the layer and the controller close");
    }

    /// Windows and X11: a layer window; Wayland: the native layer only with
    /// layer-shell AND a window screen cast (KDE, wlroots), else one-shot (GNOME).
    #[test]
    fn the_layer_follows_what_the_desktop_can_do() {
        assert_eq!(choose(false, false, false), Here::Window);
        assert_eq!(choose(false, true, true), Here::Window);
        assert_eq!(choose(true, true, true), Here::LayerShell);
        assert_eq!(choose(true, false, true), Here::OneShot, "GNOME");
        assert_eq!(choose(true, true, false), Here::OneShot, "no window source");
    }

    /// The native layer (KDE / wlroots) gets each patch once, each change of its
    /// opacity once, its removal, the glow's fade; Stop fades the patches with it.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_native_layer_is_sent_each_change_once() {
        use crate::shared::platform::layer_shell::Op;
        let (native, rx) = LayerShell::recording();
        let mut l = live();
        l.native = Some(native);
        l.phase = Phase::Open {
            window: None,
            monitor: MONITOR,
        };
        let layer = crate::features::live_translate::Layer {
            excluded_from_capture: false,
            blends: true,
            cut: false,
        };
        let _ = l.update(Msg::Prepared(Ok((layer, None))));
        let ops = |rx: &mpsc::Receiver<Op>| -> Vec<Op> {
            rx.try_iter()
                .filter(|o| !matches!(o, Op::Edge(_)))
                .collect()
        };
        let _ = ops(&rx);
        let patch = || Shown {
            bounds: [10, 10, 2, 2],
            image: image::Handle::from_rgba(2, 2, vec![0; 16]),
            under: None,
        };
        let _ = l.update(Msg::Show(5, patch()));
        assert!(matches!(ops(&rx)[..], [Op::Show(5, ..), Op::Alpha(5, 0.0)]));
        let _ = l.update(Msg::Alpha(5, 0.5));
        assert_eq!(ops(&rx), [Op::Alpha(5, 0.5)]);
        let _ = l.update(Msg::Alpha(5, 0.5));
        let _ = l.update(Msg::NoteDone);
        assert_eq!(ops(&rx), [], "nothing changed: nothing sent");
        let _ = l.update(Msg::Show(6, patch()));
        let _ = l.update(Msg::Hide(5, Instant::now()));
        assert!(matches!(
            ops(&rx)[..],
            [Op::Show(6, ..), Op::Alpha(6, 0.0), Op::Hide(5)]
        ));
        let _ = l.update(Msg::Alpha(6, 1.0));
        let _ = ops(&rx);
        // Stop: the glow and the patch fade together, then it is off
        assert!(matches!(l.leave(), Outcome::Task(_)), "fades first");
        l.edge.0.close(Instant::now() - motion::WIDGET.close.0);
        let _ = l.update(Msg::EdgeFade);
        let all: Vec<Op> = rx.try_iter().collect();
        assert!(
            matches!(all[..], [Op::Edge(e), Op::Alpha(6, a)] if e == a && e < 0.1),
            "{all:?}"
        );
        let _ = l.update(Msg::Clear);
        assert_eq!(ops(&rx), [Op::Hide(6)]);
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

    /// Stop on a layer that blends (X11 with a compositor) fades the glow and the patches out
    /// first: a frame clock of `EdgeFade`s, then `Closed` = off; a second Stop closes at once.
    #[test]
    fn stop_fades_a_blending_layer_out_before_it_closes() {
        let mut l = live();
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let layer = crate::features::live_translate::Layer {
            excluded_from_capture: false,
            blends: true,
            cut: false,
        };
        let _ = l.update(Msg::Prepared(Ok((layer, None))));
        let _ = l.update(Msg::Show(1, solid_patch()));
        let _ = l.update(Msg::Alpha(1, 1.0));
        let _ = l.update(Msg::EdgeFade);
        assert!(!l.leaving);
        assert!(matches!(l.leave(), Outcome::Task(_)), "it fades first");
        assert!(l.leaving && l.stop.load(Ordering::Relaxed));
        // the clock steps it down; at the end the edge is out
        l.edge.0 = motion::Shown::closing(motion::WIDGET, Instant::now() - Duration::from_secs(1));
        let _ = l.update(Msg::EdgeFade);
        assert_eq!(l.edge.1, 0.0, "the glow is out");
        assert!(
            matches!(l.leave(), Outcome::Off),
            "a second Stop closes at once"
        );
        assert!(matches!(l.update(Msg::Closed), Outcome::Off));
        // no blending (Windows' colour key, a cut layer): nothing to fade by hand
        let mut l = live();
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let keyed = crate::features::live_translate::Layer {
            blends: false,
            ..layer
        };
        let _ = l.update(Msg::Prepared(Ok((keyed, None))));
        assert_eq!(
            matches!(l.leave(), Outcome::Task(_)),
            shape::can_fade(),
            "the OS fades the window where it can, else it closes at once"
        );
    }

    fn solid_patch() -> Shown {
        Shown {
            bounds: [10, 10, 2, 2],
            image: image::Handle::from_rgba(2, 2, vec![255; 16]),
            under: None,
        }
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

    /// What we drew over the screen (the edge) is taken back out of a frame
    /// that shows it, where it is on screen (registered): the pixels under the
    /// glow come back within a level or two and nothing is blanked, so a
    /// subtitle near the bottom keeps its patch (black strips cost every patch
    /// near the edges, where games put subtitles); a frame of the game's own
    /// window (`edge` false) is left as it is.
    #[test]
    fn the_edge_is_taken_out_of_the_frames_that_show_it() {
        // (a monitor of its own: other tests register theirs at the origin)
        let m = Monitor {
            x: 9000,
            y: 0,
            width: 1280,
            height: 720,
            scale: 1.0,
        };
        let subtitle = [348, 596, 580, 50];
        let none = Mutex::new(None);
        let grey = vec![200u8; 1280 * 720 * 4];
        let (strips, drawn) = border::live(1280, 720, 1.0, false).unwrap();
        let mut shown = grey.clone();
        for ([x0, y0, sw, _], px) in &strips {
            for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
                let j = (((y0 + i as u32 / sw) * 1280 + x0 + i as u32 % sw) * 4) as usize;
                let a = f32::from(p[3]) / 255.0;
                for c in 0..3 {
                    let v = f32::from(shown[j + c]);
                    shown[j + c] = (v + (f32::from(p[c]) - v) * a).round() as u8;
                }
            }
        }
        assert_ne!(shown, grey, "the glow shows");
        let area = [0, 0, 1280, 720];
        let mut rgba = shown.clone();
        assert!(
            blank(&mut rgba, (m, area), &[], &none, true).is_empty(),
            "not on screen"
        );
        assert_eq!(rgba, shown);
        let on = on_screen::show(drawn.at((m.x, m.y)));
        let mut game = shown.clone();
        assert!(blank(&mut game, (m, area), &[], &none, false).is_empty());
        assert_eq!(game, shown, "the game's own window: nothing of ours in it");
        let blanked = blank(&mut rgba, (m, area), &[], &none, true);
        assert!(blanked.is_empty() && !engines::over_blanked(subtitle, &blanked));
        let off = (rgba.iter().zip(&grey)).map(|(a, b)| a.abs_diff(*b)).max();
        assert!(off <= Some(2), "{off:?}");
        drop(on);
    }

    /// Draw once: each patch is a slot of its own, so a new patch, a fade
    /// step or a removal redraws that patch's rectangle only, never another
    /// patch or the edge; a removed patch's slot stays (the ones after it don't
    /// move and aren't redrawn) and the next patch takes it; nothing changed =
    /// nothing redrawn.
    #[test]
    fn only_a_patch_that_changes_is_redrawn() {
        let mut l = live();
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let layer = crate::features::live_translate::Layer {
            excluded_from_capture: false,
            blends: true,
            cut: false,
        };
        let _ = l.update(Msg::Prepared(Ok((layer, None))));
        // the layer draws no edge (the glow in its own window, Windows) for
        // half the steps: patches are drawn and redrawn the same
        let glow_window = |l: &mut Live, i: usize| l.border = (i < 4).then_some(false);
        let patch = |x: u32| Shown {
            bounds: [x, 600, 200, 40],
            image: image::Handle::from_rgba(200, 40, vec![255; 200 * 40 * 4]),
            under: None,
        };
        let steps: Vec<Vec<Msg>> = vec![
            vec![],
            vec![Msg::Show(1, patch(100)), Msg::Alpha(1, 1.0)],
            vec![Msg::Show(2, patch(500)), Msg::Alpha(2, 0.3)],
            vec![Msg::Alpha(2, 1.0)],
            vec![Msg::Show(3, patch(900)), Msg::Alpha(3, 1.0)],
            vec![Msg::Hide(1, Instant::now())],
            vec![],
            vec![Msg::Show(4, patch(1300)), Msg::Alpha(4, 1.0)],
        ];
        let l = std::cell::RefCell::new(l);
        let damage = crate::shared::kit::testing::shot::damage(
            |i| {
                for msg in steps.get(i)?.clone() {
                    let _ = l.borrow_mut().update(msg);
                }
                glow_window(&mut l.borrow_mut(), i);
                Some(l.borrow().view())
            },
            Size::new(1920.0, 1080.0),
        );
        let only = |x: f32| {
            move |d: &Vec<iced::Rectangle>| {
                !d.is_empty()
                    && d.iter().all(|r| {
                        r.x >= x - 2.0
                            && r.x + r.width <= x + 202.0
                            && r.y >= 598.0
                            && r.y + r.height <= 642.0
                    })
            }
        };
        assert!(only(100.0)(&damage[1]), "{:?}", damage[1]);
        assert!(only(500.0)(&damage[2]), "{:?}", damage[2]);
        assert!(only(500.0)(&damage[3]), "the fade step: {:?}", damage[3]);
        assert!(only(900.0)(&damage[4]), "{:?}", damage[4]);
        assert!(only(100.0)(&damage[5]), "the removal only: {:?}", damage[5]);
        assert!(damage[6].is_empty(), "nothing new: {:?}", damage[6]);
        assert!(only(1300.0)(&damage[7]), "{:?}", damage[7]);
        let l = l.into_inner();
        // drawn where they are (the window's size comes from its first layer)
        let px = crate::shared::kit::testing::shot::render(l.view(), 1920, 1080);
        for (x, y) in [(1310, 610), (510, 620), (910, 630)] {
            let i = (y * 1920 + x) * 4;
            assert_eq!(px[i..i + 3], [255, 255, 255], "a patch at ({x}, {y})");
        }
        assert_eq!(l.shown(), 3);
        assert_eq!(
            l.patches[0].as_ref().map(|s| s.id),
            Some(4),
            "the free slot"
        );
    }

    /// A layer that doesn't blend (Windows' colour key, X11 without a
    /// compositor): a patch fades in as a blend with the pixels it covers,
    /// taken from the frame it is shown on, and is the patch itself once in
    /// (the covered pixels let go); the core fades in over 120 ms (a step a
    /// frame: about 7).
    #[test]
    fn a_patch_fades_in_over_what_it_covers() {
        assert_eq!(core_config().fade_in, Duration::from_millis(120));
        let rgba: Vec<u8> = (0..8 * 4 * 4).map(|i| (i % 256) as u8).collect();
        let frame = core::Frame {
            rgba: &rgba,
            width: 8,
            height: 4,
            at: Duration::ZERO,
        };
        let sent = std::cell::RefCell::new(Vec::new());
        let mut ui = ToUi {
            send: |m| {
                sent.borrow_mut().push(m);
                true
            },
            gone: false,
            under: true,
            shows: 0,
            hides: 0,
            fades: 0,
            shown_at: Instant::now(),
        };
        let at = Rect {
            x: 6,
            y: 1,
            w: 4,
            h: 2,
        };
        let mut over = Over {
            ui: &mut ui,
            frame: &frame,
        };
        // the patch: opaque white but its last pixel clear (unchanged there)
        let mut patch = [255u8; 4 * 2 * 4];
        patch[28..].fill(0);
        core::Layer::show(&mut over, 7, at, &patch);
        core::Layer::alpha(&mut over, 7, 0.5);
        assert_eq!(ui.fades, 1);

        let mut sent = sent.into_inner().into_iter();
        let Some(Msg::Show(7, shown)) = sent.next() else {
            panic!("no patch")
        };
        // the covered pixels: row 1 from x 6 (two inside the frame, two past it)
        let under = shown.under.clone().unwrap();
        assert_eq!(under[..8], rgba[(8 + 6) * 4..(8 + 8) * 4]);
        assert!(under[8..16].iter().all(|&v| v == 0));
        let mut slot = Slot {
            id: 7,
            drawn: shown.image.clone(),
            shown,
            alpha: 0.0,
        };
        slot.fade(0.5);
        let image::Handle::Rgba { pixels, .. } = &slot.drawn else {
            panic!("not pixels")
        };
        let half = |u: u8| (f32::from(u) + (255.0 - f32::from(u)) * 0.5).round() as u8;
        assert_eq!(
            pixels[..4],
            [half(under[0]), half(under[1]), half(under[2]), 255]
        );
        assert_eq!(
            pixels[28..],
            [0; 4],
            "a clear pixel stays clear (never the key colour)"
        );
        slot.fade(1.0);
        assert!(slot.shown.under.is_none());
        assert_eq!(slot.drawn, slot.shown.image);
    }

    /// A layer cut to what it shows (X11 without a compositor) draws the solid
    /// line and is cut to it and its patches whenever a patch comes or goes; a
    /// layer that blends draws the glow and isn't cut.
    #[test]
    fn a_cut_layer_is_cut_to_its_line_and_patches() {
        let mut l = live();
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let layer = crate::features::live_translate::Layer {
            excluded_from_capture: false,
            blends: false,
            cut: true,
        };
        let _ = l.update(Msg::Prepared(Ok((layer, None))));
        assert_eq!(l.border, Some(true), "the solid line");
        assert_eq!(l.line.len(), 4);
        let t = l.line[0][3];
        assert_eq!(l.line[0], [0, 0, 1920, t]);
        assert!(l.cut().units() > 0);
        let mut l = live();
        let _ = l.update(Msg::Monitor(Ok(MONITOR)));
        let layer = crate::features::live_translate::Layer {
            blends: true,
            cut: false,
            ..layer
        };
        let _ = l.update(Msg::Prepared(Ok((layer, None))));
        assert_eq!(l.border, Some(false));
        assert!(l.line.is_empty() && l.cut().units() == 0);
    }

    /// Live never asks the graphics card in normal use (owner, 2026-11-16): the switch is off by
    /// default, so the probe / download call is never made; the test hook turns it on.
    #[test]
    fn live_never_probes_the_graphics_card_unless_the_test_hook_is_set() {
        struct Never;
        impl core::Translator for Never {
            fn translate(&mut self, t: &str) -> Result<String, String> {
                Ok(t.into())
            }
        }
        impl OnGpu for Never {
            fn slowed(&self) -> bool {
                false
            }
            fn set_target(&mut self, _: &str) {}
        }
        assert!(!gpu_wanted(), "off without OPENCIRCLE_TEST_GPU");
        let (tx, rx) = mpsc::channel();
        let (called_tx, called) = mpsc::channel();
        let probe = move |_: &str, _: &mut dyn FnMut(u64, u64)| {
            let _ = called_tx.send(());
            None::<Never>
        };
        start_gpu_with(gpu_wanted(), "fr".into(), tx.clone(), probe.clone());
        assert!(called.recv_timeout(Duration::from_millis(300)).is_err());
        start_gpu_with(true, "fr".into(), tx, probe);
        assert!(called.recv_timeout(Duration::from_secs(5)).is_ok());
        drop(rx);
    }

    /// The graphics card translates while it has one; once it slows ~2x the processor does, for
    /// the rest of the session; a new language goes to the card too.
    #[test]
    fn the_graphics_card_translates_until_it_slows() {
        struct Card(u32, Arc<Mutex<String>>);
        impl core::Translator for Card {
            fn translate(&mut self, text: &str) -> Result<String, String> {
                self.0 += 1;
                Ok(format!("gpu {text}"))
            }
        }
        impl OnGpu for Card {
            fn slowed(&self) -> bool {
                self.0 >= 2
            }
            fn set_target(&mut self, target: &str) {
                *self.1.lock().unwrap() = target.into();
            }
        }
        struct Cpu;
        impl core::Translator for Cpu {
            fn translate(&mut self, text: &str) -> Result<String, String> {
                Ok(format!("cpu {text}"))
            }
        }
        let target = Arc::new(Mutex::new(String::new()));
        let mut gpu: Option<Box<dyn OnGpu>> = None;
        let line = |gpu: &mut Option<Box<dyn OnGpu>>| {
            let got = translator(gpu, &mut Cpu).translate("a").unwrap();
            back_off(gpu);
            got
        };
        assert_eq!(line(&mut gpu), "cpu a", "none yet: the processor");
        gpu = Some(Box::new(Card(0, target.clone())));
        gpu.as_mut().unwrap().set_target("fr");
        assert_eq!(*target.lock().unwrap(), "fr");
        assert_eq!(line(&mut gpu), "gpu a");
        assert_eq!(line(&mut gpu), "gpu a");
        assert!(gpu.is_none(), "slowed: dropped");
        assert_eq!(line(&mut gpu), "cpu a");
    }

    #[test]
    fn a_still_screen_is_grabbed_less_often_only_where_grabs_never_wait() {
        assert_eq!(still_wait(0, false), None, "it changes: every frame");
        assert_eq!(still_wait(5, false), None);
        assert!(still_wait(6, false).is_some(), "held still: fewer grabs");
        assert_eq!(still_wait(60, true), None, "frames come only on a change");
    }

    #[test]
    fn our_graphics_card_helper_is_not_a_busy_pc() {
        gpu::pretend_helper(4242);
        assert!(ours().contains(&4242));
        gpu::pretend_helper(0);
        assert!(!ours().contains(&4242));
    }
}
