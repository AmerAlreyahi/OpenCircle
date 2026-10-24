//! The iced daemon: owns the (pre-created, hidden) overlay window and its state.
mod bar;
mod border;
mod edit;
mod live_controller;
mod live_layer;
mod onboarding;
mod overlay;
mod panel;
mod quick;
mod settings;
mod snip;
pub mod theme;
mod translate;
mod tray;
pub(crate) mod widgets;
mod windows;
mod word;

pub use tray::start as start_tray;

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use iced::futures::stream::{self, Stream, StreamExt};
use iced::keyboard::{self, key::Named};
use iced::widget::{canvas, container, image, opaque, space, stack};
use iced::window::{self, Level, Mode};
use iced::{ContentFit, Element, Fill, Point, Rectangle, Size, Subscription, Task, Vector};

use crate::actions::{self, Action, Crop, Run, copy, lens, qr, save, text};
use crate::config::{self, QuickAction, Target};
use crate::editor::{self, Output};
use crate::ocr::{self, TextBlock};
use crate::translate::pipeline::Cache;
use crate::{Trigger, capture};
use edit::Editing;
use quick::{Quick, Start};
use snip::Snip;

const HINT: &str = "Circle, drag or tap anything · Esc to close";
/// Shown over a selection that came out black (DRM / protected windows).
const PROTECTED: &str = "This window is protected; it can't be captured";

/// The ask bar at the bottom of the overlay: its text field, width, and the
/// height kept free for it (the action bar stays above it).
const ASK_INPUT: &str = "overlay-ask";
const ASK_WIDTH: f32 = 480.0;
const ASK_ZONE: f32 = widgets::CONTROL + 2.0 * widgets::SURFACE_PADDING + 2.0 * ASK_MARGIN;
const ASK_MARGIN: f32 = 24.0;

/// Between stacked crops (several selections sent as one image).
const STACK_GAP: u32 = 16;

/// Handed from `run` to the subscription (a subscription builder can't capture).
static TRIGGERS: Mutex<Option<UnboundedReceiver<Trigger>>> = Mutex::new(None);

/// Call first in `main`, before any thread starts (glibc fixes the arena limit
/// at the first thread's first allocation). Frames and panel work are allocated
/// on short-lived threads; with per-thread arenas each overlay session kept a
/// whole frame (~20 MB at 3440x1440) after closing. One arena keeps the app at
/// its idle size.
pub fn limit_malloc_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: only changes an allocator setting; no memory is touched.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

pub fn run(triggers: UnboundedReceiver<Trigger>) -> iced::Result {
    *TRIGGERS.lock().unwrap() = Some(triggers);
    crate::lens::sweep_stale(); // pages left by a crash
    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .font(widgets::ICON_FONT_BYTES)
        // Arabic for the word card and the Text panel's translation
        .font(include_bytes!("../../assets/fonts/NotoSansArabic.ttf").as_slice())
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Trigger(Trigger),
    /// The capture thread is done (`Instant` = trigger time, latency log).
    Captured(Instant, Result<Shot, String>),
    Overlay(overlay::Event),
    /// Esc, wherever the focus is.
    Escape,
    /// From the panel with this number (stale numbers are ignored).
    Panel(u32, panel::Msg),
    /// Run an action on the selection (action bar button or its key).
    Run(&'static Action),
    TestShot(window::Screenshot),
    /// While closing: a frame was drawn / the window was resized (see `Hiding`).
    Drawn,
    Resized(Size),
    /// Settings / onboarding windows and the OS theme (`windows.rs`).
    Windows(windows::Msg),
    /// From the annotation editor (Edit in place, Snip).
    Editor(editor::Message),
    Snip(snip::Msg),
    /// The copy-text quick action read this.
    TextRead(Result<String, String>),
    /// The toast has been shown long enough.
    ToastDone,
    /// The selection with this area was scanned (a stale area is ignored).
    Scanned(Rectangle, qr::Scan),
    /// The QR chip's Copy / Open link.
    Qr(qr::Act),
    /// From the translate session with this number (stale numbers are ignored).
    Translate(u32, translate::Msg),
    /// Alt went down / up (peek at the original under a translation).
    Peek(bool),
    Live(live_layer::Msg),
    /// OCR around tap number `.0` at `.1` (image px) read these lines (frame px of
    /// a crop at `.2`).
    Tapped(u32, Point, [u32; 2], Result<Vec<TextBlock>, String>),
    /// The ask bar's text, and Enter in it.
    AskInput(String),
    AskSubmit,
}

/// A press on the selection: resizing it by a corner handle, or moving it.
struct Grab {
    /// The corner (index into the handles), or `None` = move.
    corner: Option<usize>,
    /// Where the press was (image px).
    from: Point,
    /// The selection and its stroke when the press began.
    start: (overlay::Selection, Vec<Point>),
    /// It moved further than a tap.
    moved: bool,
}

/// What the open overlay is for.
enum Purpose {
    /// Circle to Search: select, then the action bar.
    Search,
    /// A quick action: runs on its target's area, or on the first selection.
    Quick(Quick, Target),
    /// Snip: pick an area, then the editor (no action bar).
    Snip(Box<Snip>),
}

/// Closing steps. The renderer frees the frame cache and resizes its buffers only
/// when it draws, and a hidden window never draws, so shrink first, then hide.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hiding {
    No,
    /// Wait for one empty full-size frame (drops the cached frame).
    Empty,
    /// Left full-screen and asked for 1x1; wait for the resize to land (frames so far).
    Shrinking(u8),
    /// Wait for one frame at the small size (shrinks the buffers), then hide.
    Shrunk,
}

/// Below this (logical px) the window counts as shrunk.
const SHRUNK: f32 = 64.0;
/// Hide anyway after this many frames without the shrink (a WM may refuse it): a
/// window left on top of the screen would be far worse than unshrunk buffers.
// ponytail: fixed frame count, not a timer; frames come back to back while closing.
const SHRINK_FRAMES: u8 = 10;

/// A captured monitor, ready to show.
#[derive(Debug, Clone)]
struct Shot {
    image: image::Handle,
    scale: f32,
    /// Logical px: the monitor's origin and size.
    origin: Point,
    size: Size,
    /// Snip mode: the mode to start in, and the windows on this monitor (logical
    /// px, topmost first).
    snip: Option<(snip::Kind, Vec<Rectangle>)>,
}

struct App {
    overlay: window::Id,
    /// A capture is running on its thread.
    capturing: bool,
    /// The frozen frame while the overlay is open; `None` = hidden, frame freed.
    /// Its RGBA bytes are also what actions crop (shared, never copied whole).
    frame: Option<image::Handle>,
    /// Physical px per logical px of `frame`.
    scale: f32,
    /// The overlay's logical size (the monitor's).
    screen: Size,
    /// The stroke being drawn or the selection's (image px; see `overlay::View`).
    stroke: Vec<Point>,
    drawing: bool,
    selection: Option<overlay::Selection>,
    /// Selections kept with Shift before the current one, with their strokes.
    others: Vec<(overlay::Selection, Vec<Point>)>,
    /// The stroke being drawn adds to `others` (Shift).
    adding: bool,
    /// Resizing or moving the selection.
    grab: Option<Grab>,
    /// The zoomed / panned part of the frame on screen; a pan's start.
    view: overlay::View,
    pan: Option<(Point, overlay::View)>,
    /// The tapped word(s) the selection is, and the latest tap's number.
    word: Option<word::Picked>,
    taps: u32,
    /// The ask bar's text.
    ask: String,
    /// Bumped on every open (resets the overlay's pointer state).
    opening: u32,
    /// Trigger time + expected size of the full-screen frame not drawn yet (latency log).
    first_draw: Cell<Option<(Instant, Size)>>,
    hiding: Hiding,
    /// The open result panel and its number.
    panel: Option<(u32, panel::Panel)>,
    panels: u32,
    /// Lens pages opened from this overlay, released when it closes.
    lens_pages: Vec<lens::Held>,
    windows: windows::Windows,
    purpose: Purpose,
    /// The annotation editor on the selection; kept when going back to the bar.
    edit: Option<Editing>,
    /// The shown monitor's origin (logical px): the key of `regions`.
    origin: Point,
    /// The last selection per monitor, for `last_region` (memory only).
    regions: Vec<(Point, Rectangle)>,
    /// A short status over the frame (copy text).
    toast: Option<&'static str>,
    /// What the current selection holds (QR code, protected capture); `None` until scanned.
    scan: Option<qr::Scan>,
    /// Translate in place on this overlay, and its number.
    translate: Option<(u32, translate::Session)>,
    translates: u32,
    /// Translations by text, shared by one-shot and live mode.
    cache: Arc<Cache>,
    /// Live translate, while on.
    live: Option<live_layer::Live>,
    /// One warm OCR / translation helper while the overlay is open (tap a
    /// word, Text, Translate), started by the first of them.
    helper: Option<ocr::Hold>,
}

/// Translations kept for reuse (each a short text).
const CACHE_SIZE: usize = 512;

impl App {
    fn boot() -> (Self, Task<Message>) {
        // Created once, hidden, so a trigger only has to show it (latency).
        let (overlay, open) = window::open(window::Settings {
            visible: false,
            decorations: false,
            resizable: false,
            level: Level::AlwaysOnTop,
            exit_on_close_request: false,
            ..window::Settings::default()
        });
        let mut app = App {
            cache: Arc::new(Cache::new(CACHE_SIZE)),
            ..App::idle(overlay)
        };
        let start = app.windows.start().map(Message::Windows);
        (app, Task::batch([open.discard(), start]))
    }

    /// Nothing open yet (the overlay window `overlay` hidden).
    fn idle(overlay: window::Id) -> App {
        App {
            overlay,
            capturing: false,
            frame: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
            others: Vec::new(),
            adding: false,
            grab: None,
            view: overlay::View::new(1, 1, Size::new(1.0, 1.0)),
            pan: None,
            word: None,
            taps: 0,
            ask: String::new(),
            opening: 0,
            first_draw: Cell::new(None),
            hiding: Hiding::No,
            panel: None,
            panels: 0,
            lens_pages: Vec::new(),
            windows: windows::Windows::default(),
            purpose: Purpose::Search,
            edit: None,
            origin: Point::ORIGIN,
            regions: Vec::new(),
            toast: None,
            scan: None,
            translate: None,
            translates: 0,
            cache: Arc::new(Cache::new(8)),
            live: None,
            helper: None,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Trigger(Trigger::Quit) => iced::exit(),
            Message::Trigger(Trigger::OpenSettings) => self
                .windows
                .open(windows::Kind::Settings)
                .map(Message::Windows),
            Message::Windows(msg) => self.windows.update(msg).map(Message::Windows),
            Message::Trigger(Trigger::Quick(at, qa))
                if qa.live && qa.action == actions::translate::ACTION.id =>
            {
                self.toggle_live(at)
            }
            Message::Live(msg) => self.on_live(msg),
            Message::Translate(id, msg) => self.on_translate(id, msg),
            Message::Peek(down) => {
                if let Some((_, session)) = &mut self.translate {
                    let _ = session.update(translate::Msg::Peek(down));
                }
                Task::none()
            }
            Message::Trigger(Trigger::Capture(_) | Trigger::Quick(..))
                if self.frame.is_some() || self.capturing =>
            {
                Task::none()
            }
            Message::Trigger(Trigger::Capture(at)) => {
                self.purpose = Purpose::Search;
                self.capture(at, None)
            }
            Message::Trigger(Trigger::Quick(at, qa)) => {
                self.purpose = Purpose::Search;
                if qa.action == "snip" {
                    return self.capture(at, Some(snip::Kind::for_target(qa.target)));
                }
                match Quick::for_id(&qa.action) {
                    Some(q) => self.purpose = Purpose::Quick(q, qa.target),
                    None => eprintln!("unknown action {}; opening the overlay", qa.action),
                }
                self.capture(at, None)
            }
            Message::Captured(at, shot) => self.open(at, shot),
            Message::Overlay(event) => self.on_overlay(event),
            Message::Escape if self.frame.is_none() => Task::none(),
            Message::Escape if let Some(scroll) = self.scroll() => {
                scroll.stop(); // what was gathered opens in the editor
                Task::none()
            }
            // The editor has its own Esc (finish typing, close the picker, back).
            Message::Escape if self.edit.as_ref().is_some_and(Editing::is_open) => Task::none(),
            Message::Escape if std::env::var_os(TEST_SHOT).is_some() => self.test_shot(),
            Message::Escape => self.close(),
            Message::Editor(msg) => self.on_editor(msg),
            Message::Snip(msg) => self.on_snip(msg),
            Message::TextRead(read) if self.toast.is_some() => {
                let copied = read.and_then(|t| match t.trim() {
                    "" => Err("no text found".into()),
                    _ => copy::text(t),
                });
                self.toast = Some(match copied {
                    Ok(()) => "Text copied",
                    Err(e) => {
                        eprintln!("copy text failed: {e}");
                        "No text copied"
                    }
                });
                background(|tx| {
                    std::thread::sleep(Duration::from_millis(900));
                    let _ = tx.unbounded_send(());
                })
                .map(|()| Message::ToastDone)
            }
            Message::TextRead(_) => Task::none(), // closed meanwhile
            Message::Scanned(area, scan) => {
                if self.selection.map(|s| s.area) == Some(area) {
                    self.scan = Some(scan);
                }
                Task::none()
            }
            Message::Qr(act) => self.on_qr(act),
            Message::ToastDone if self.toast.is_some() => self.close(),
            Message::ToastDone => Task::none(),
            Message::Panel(id, msg) => self.on_panel(id, msg),
            Message::Run(action) => self.run(action),
            Message::Tapped(id, at, origin, read) => self.on_tapped(id, at, origin, read),
            Message::AskInput(text) => {
                self.ask = text;
                Task::none()
            }
            Message::AskSubmit => {
                let question = std::mem::take(&mut self.ask);
                let crop = (!question.trim().is_empty()).then(|| self.stacked());
                match crop.flatten() {
                    Some(crop) => self.open_panel(actions::Panel::Ask, crop, Some(question)),
                    None => Task::none(),
                }
            }
            // Leave full-screen while still shown (on Windows, leaving it while hidden
            // restores a placement saved while visible: shows it again), then shrink.
            // open() resizes it back before showing.
            Message::Drawn if self.hiding == Hiding::Empty => {
                self.hiding = Hiding::Shrinking(0);
                window::set_mode(self.overlay, Mode::Windowed)
                    .chain(window::resize(self.overlay, Size::new(1.0, 1.0)))
            }
            Message::Resized(size)
                if matches!(self.hiding, Hiding::Shrinking(_))
                    && size.width.max(size.height) < SHRUNK =>
            {
                self.hiding = Hiding::Shrunk;
                Task::none()
            }
            Message::Drawn
                if matches!(
                    self.hiding,
                    Hiding::Shrunk | Hiding::Shrinking(SHRINK_FRAMES..)
                ) =>
            {
                self.hiding = Hiding::No;
                // Panel threads (PNG, HTTP, OCR I/O) leave freed memory in their
                // own malloc arenas; give it back to the OS while idle.
                #[cfg(all(target_os = "linux", target_env = "gnu"))]
                // SAFETY: malloc_trim only releases free heap pages.
                unsafe {
                    libc::malloc_trim(0);
                }
                window::set_mode(self.overlay, Mode::Hidden)
            }
            Message::Drawn => {
                if let Hiding::Shrinking(frames) = &mut self.hiding {
                    *frames += 1;
                }
                Task::none()
            }
            Message::Resized(_) => Task::none(),
            Message::TestShot(shot) => {
                save_test_shot(&shot);
                self.close()
            }
        }
    }

    /// Captures on a thread: a Wayland portal may wait minutes for the user's OK.
    /// `snip`: Snip mode in this kind; also lists the windows, while the overlay is
    /// still hidden.
    fn capture(&mut self, at: Instant, snip: Option<snip::Kind>) -> Task<Message> {
        self.capturing = true;
        background(move |tx| {
            let shot = capture::monitor_under_cursor().map(|frame| {
                let scale = frame.scale;
                let logical = |x: i32, y: i32, w: u32, h: u32| {
                    let (x, y) = ((x - frame.x) as f32, (y - frame.y) as f32);
                    Rectangle::new(Point::new(x, y), Size::new(w as f32, h as f32)) * (1.0 / scale)
                };
                let snip = snip.map(|kind| {
                    let list = capture::window_rect::list();
                    let windows = list.into_iter().map(|(x, y, w, h)| logical(x, y, w, h));
                    (kind, windows.collect())
                });
                Shot {
                    origin: Point::new(frame.x as f32 / scale, frame.y as f32 / scale),
                    size: Size::new(frame.width as f32, frame.height as f32) * (1.0 / scale),
                    image: image::Handle::from_rgba(frame.width, frame.height, frame.rgba),
                    scale,
                    snip,
                }
            });
            let _ = tx.unbounded_send(shot);
        })
        .map(move |shot| Message::Captured(at, shot))
    }

    fn open(&mut self, at: Instant, shot: Result<Shot, String>) -> Task<Message> {
        self.capturing = false;
        let shot = match shot {
            Ok(shot) => shot,
            Err(e) => {
                eprintln!("capture failed: {e}");
                self.purpose = Purpose::Search;
                return Task::none();
            }
        };
        eprintln!(
            "[opencircle] capture {}x{} took {} ms",
            shot.size.width * shot.scale,
            shot.size.height * shot.scale,
            at.elapsed().as_millis()
        );
        let size = shot.size;
        let px = |v: f32| (v * shot.scale).round() as u32;
        self.view = overlay::View::new(px(size.width), px(size.height), size);
        self.frame = Some(shot.image);
        self.helper = Some(ocr::hold());
        self.scale = shot.scale;
        self.screen = size;
        self.origin = shot.origin;
        self.opening = self.opening.wrapping_add(1);
        self.first_draw.set(Some((at, size)));
        self.hiding = Hiding::No;
        let mut first = None;
        if let Some((kind, windows)) = shot.snip {
            let mut snip = Snip::new(windows, size, snip::can_scroll());
            first = snip.start(kind, size); // Full = the editor on the whole screen
            self.purpose = Purpose::Snip(Box::new(snip));
        }
        let Purpose::Quick(quick, target) = self.purpose else {
            let editor = first.map(|next| self.snip_next(Some(next)));
            return editor.unwrap_or_else(Task::none).chain(self.show());
        };
        let Start::Area(area) = quick::start(target, size, self.last_region()) else {
            return self.show(); // the first selection runs it
        };
        self.select(area);
        if let Quick::Action(
            action @ Action {
                run: Run::Finish(_),
                ..
            },
        ) = quick
        {
            // Copy, Save: nothing to show; the window stays hidden.
            let done = self.run(action);
            self.hiding = Hiding::No;
            return done;
        }
        let run = self.run_quick(quick);
        self.show().chain(run)
    }

    /// Shows the (hidden or pill-sized) window over the whole monitor.
    fn show(&self) -> Task<Message> {
        // Place and size the window to cover the monitor BEFORE set_mode shows it:
        // iced makes it visible before going full-screen, so whatever is shown
        // first must already cover the monitor (no small black box flash).
        // ponytail: mixed-DPI multi-monitor placement untested (needs real hardware).
        let show = window::move_to(self.overlay, self.origin)
            .chain(window::resize(self.overlay, self.screen))
            .chain(window::set_mode(self.overlay, Mode::Fullscreen))
            .chain(window::gain_focus(self.overlay));
        // A Lens / Ask hand-off to the browser drops the overlay out of the topmost
        // band behind iced's back (`lens::hand_off_foreground`): Normal then
        // AlwaysOnTop makes it apply again.
        #[cfg(windows)]
        let show = window::set_level(self.overlay, Level::Normal)
            .chain(window::set_level(self.overlay, Level::AlwaysOnTop))
            .chain(show);
        show
    }

    fn on_overlay(&mut self, event: overlay::Event) -> Task<Message> {
        // The editor, a toast or a scrolling capture has the screen.
        if self.edit.as_ref().is_some_and(Editing::is_open)
            || self.toast.is_some()
            || self.scroll().is_some()
        {
            return Task::none();
        }
        // Zoom and pan move the view; pointer events go on in image px.
        let event = match event {
            overlay::Event::Zoom { at, notches } => {
                if self.can_zoom() {
                    self.view = self.view.zoomed(at, notches);
                }
                return Task::none();
            }
            overlay::Event::PanStart(at) => {
                self.pan = self.can_zoom().then_some((at, self.view));
                return Task::none();
            }
            overlay::Event::PanMove(at) => {
                if let Some((from, start)) = self.pan {
                    self.view = start.panned(at - from);
                }
                return Task::none();
            }
            overlay::Event::ZoomReset => {
                self.view = self.view.reset();
                return Task::none();
            }
            overlay::Event::Press { at, shift } => overlay::Event::Press {
                at: self.view.to_image(at),
                shift,
            },
            overlay::Event::Move(at) => overlay::Event::Move(self.view.to_image(at)),
            overlay::Event::Hover(at) => overlay::Event::Hover(self.view.to_image(at)),
            event => event,
        };
        // Over a translation: a tap opens a block's word card, E / A are the
        // card's; a new selection leaves the translation.
        if let Some((id, session)) = &mut self.translate {
            let id = *id;
            match event {
                overlay::Event::Key {
                    letter: 'e',
                    ctrl: false,
                } if session.has_card() => {
                    return self.on_translate(id, translate::Msg::Explain);
                }
                overlay::Event::Key {
                    letter: 'a',
                    ctrl: false,
                } if session.has_card() => {
                    return self.on_translate(id, translate::Msg::Ask);
                }
                overlay::Event::Key { .. } | overlay::Event::Enter => return Task::none(),
                overlay::Event::Release if overlay::select(&self.stroke, 1.0).is_none() => {
                    let at = self.stroke.last().copied().unwrap_or(Point::ORIGIN);
                    session.click(Point::new(at.x * self.scale, at.y * self.scale));
                    self.drawing = false;
                    self.stroke.clear();
                    return Task::none();
                }
                overlay::Event::Release => self.translate = None,
                _ => {}
            }
        }
        match event {
            overlay::Event::Press { at: p, shift } => {
                // A new selection: the panel and the edits were about the old one.
                self.panel = None;
                self.edit = None;
                self.scan = None;
                self.taps = self.taps.wrapping_add(1); // a tap still reading is stale
                if !shift && let Some(grab) = self.grab_at(p) {
                    self.grab = Some(grab);
                    return Task::none();
                }
                // Shift keeps the selection and adds the next one.
                self.adding = shift && matches!(self.purpose, Purpose::Search);
                match self.selection.take() {
                    Some(sel) if self.adding => {
                        self.others.push((sel, std::mem::take(&mut self.stroke)));
                    }
                    _ => self.others.clear(),
                }
                self.stroke = vec![p];
                self.drawing = true;
            }
            overlay::Event::Move(p) if self.grab.is_some() => self.drag_grab(p),
            overlay::Event::Move(p) if self.drawing => self.stroke.push(p),
            overlay::Event::Move(_) => {}
            overlay::Event::Hover(p) => {
                // Snip's Window mode: the window under the pointer lights up.
                if let Purpose::Snip(snip) = &self.purpose {
                    match snip.window_at(p) {
                        Some(w) if self.selection.map(|s| s.area) != Some(w) => self.select(w),
                        Some(_) => {}
                        None => {
                            self.selection = None;
                            self.stroke.clear();
                        }
                    }
                }
            }
            overlay::Event::Release if let Some(grab) = self.grab.take() => {
                return if grab.moved {
                    self.selected()
                } else {
                    self.tap(grab.from) // a click on the selection
                };
            }
            overlay::Event::Release => {
                self.drawing = false;
                let zoom = self.view.zoom();
                self.selection = overlay::select(&self.stroke, zoom);
                if let Purpose::Snip(snip) = &self.purpose {
                    let area = self.selection.map(|s| s.area);
                    let at = self.stroke.last().copied().unwrap_or(Point::ORIGIN);
                    let next = snip.pick(area, at, self.screen);
                    self.remember_region();
                    return self.snip_next(next);
                }
                if std::mem::take(&mut self.adding)
                    && self.selection.is_none()
                    && let Some((sel, stroke)) = self.others.pop()
                {
                    // Nothing added (a tap, a line): the last one is current again.
                    (self.selection, self.stroke) = (Some(sel), stroke);
                    return Task::none();
                }
                if self.selection.is_some() {
                    self.word = None;
                } else if overlay::is_tap(&self.stroke, zoom) {
                    return self.tap(self.stroke[0]);
                }
                return self.selected();
            }
            overlay::Event::Enter if matches!(self.purpose, Purpose::Search) => {
                return self.run(actions::default_for(&config::current()));
            }
            overlay::Event::Key { letter, ctrl } if matches!(self.purpose, Purpose::Search) => {
                if let Some(action) = actions::for_key(letter, ctrl) {
                    return self.run(action);
                }
            }
            overlay::Event::Enter | overlay::Event::Key { .. } => {}
            // mapped above
            overlay::Event::Zoom { .. }
            | overlay::Event::PanStart(_)
            | overlay::Event::PanMove(_)
            | overlay::Event::ZoomReset => {}
        }
        Task::none()
    }

    /// Scans the new selection for a QR code and a black (protected) capture, on a
    /// thread; the result comes back as `Scanned` and is dropped if the selection moved on.
    fn scan(&mut self, area: Rectangle) -> Task<Message> {
        let Some(crop) = self.crop() else {
            return Task::none();
        };
        background(move |tx| {
            let _ = tx.unbounded_send(qr::scan(&crop));
        })
        .map(move |scan| Message::Scanned(area, scan))
    }

    /// The QR chip was clicked: copy its text, or open it (a link, on this click only).
    fn on_qr(&mut self, act: qr::Act) -> Task<Message> {
        let Some(text) = self.scan.as_ref().and_then(|s| s.qr.clone()) else {
            return Task::none();
        };
        let failed = |e: String| {
            eprintln!("QR chip failed: {e}");
            "That didn't work"
        };
        match act {
            qr::Act::Copy => {
                let message = copy::text(text).map_or_else(failed, |()| "QR text copied");
                self.toast_then_close(message)
            }
            qr::Act::Open => match qr::web_url(&text).map(crate::lens::open_link) {
                Some(Ok(())) => self.close(), // the browser has the screen now
                Some(Err(e)) => {
                    let message = failed(e.to_string());
                    self.toast_then_close(message)
                }
                None => Task::none(),
            },
        }
    }

    /// Shows `message` over the frame for a moment, then closes the overlay.
    fn toast_then_close(&mut self, message: &'static str) -> Task<Message> {
        self.toast = Some(message);
        background(|tx| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = tx.unbounded_send(());
        })
        .map(|()| Message::ToastDone)
    }

    /// The frame can be zoomed: Circle to Search and quick actions (Snip, the
    /// editor and a translation draw at 1x).
    fn can_zoom(&self) -> bool {
        matches!(self.purpose, Purpose::Search | Purpose::Quick(..))
            && self.edit.is_none()
            && self.translate.is_none()
    }

    /// The selection shows its handles (and can be resized / moved) while the
    /// action bar is there.
    fn handles(&self) -> bool {
        matches!(self.purpose, Purpose::Search)
            && !self.drawing
            && self.panel.is_none()
            && self.edit.is_none()
            && self.translate.is_none()
    }

    /// A press at `p` (image px) on the selection: a corner handle resizes it,
    /// anywhere inside moves it.
    fn grab_at(&self, p: Point) -> Option<Grab> {
        let sel = self.selection.filter(|_| self.handles())?;
        let on_screen = self.view.rect_to_screen(sel.area);
        let corner = overlay::handle_at(on_screen, self.view.to_screen(p));
        (corner.is_some() || sel.area.contains(p)).then(|| Grab {
            corner,
            from: p,
            start: (sel, self.stroke.clone()),
            moved: false,
        })
    }

    /// Resizes / moves the grabbed selection to the pointer at `p` (image px). A
    /// freehand loop scales with its box; anything else becomes that box.
    fn drag_grab(&mut self, p: Point) {
        let Some(grab) = &mut self.grab else { return };
        let zoom = self.view.zoom();
        grab.moved |= !overlay::is_tap(&[grab.from, p], zoom);
        if !grab.moved {
            return;
        }
        let (start, stroke) = &grab.start;
        let area = match grab.corner {
            Some(corner) => overlay::resize(start.area, corner, p, overlay::MIN_SIDE / zoom),
            None => start.area + (p - grab.from),
        };
        let kind = match start.kind {
            overlay::Kind::Loop => overlay::Kind::Loop,
            overlay::Kind::Drag | overlay::Kind::Word => overlay::Kind::Drag,
        };
        self.stroke = overlay::refit(stroke, start.area, area);
        self.selection = Some(overlay::Selection { area, kind });
        self.word = None; // the words are no longer what is selected
    }

    /// The selection on the shown monitor, for `last_region`.
    fn remember_region(&mut self) {
        if let Some(sel) = self.selection {
            self.regions.retain(|(origin, _)| *origin != self.origin);
            self.regions.push((self.origin, sel.area));
        }
    }

    /// A selection was made (drawn, moved, a word tapped): remembered, and a
    /// quick action waiting for it runs. No selection: the stroke goes.
    fn selected(&mut self) -> Task<Message> {
        if self.selection.is_none() {
            self.stroke.clear();
            return Task::none();
        }
        self.remember_region();
        match (&self.purpose, self.selection) {
            (Purpose::Quick(quick, _), _) => {
                let quick = *quick;
                self.run_quick(quick)
            }
            // A tapped word is text OCR just read: no QR code, not protected.
            (Purpose::Search, Some(sel)) if sel.kind != overlay::Kind::Word => self.scan(sel.area),
            _ => Task::none(),
        }
    }

    /// A tap at `at` (image px): the word there becomes the selection. Lines read
    /// for an earlier tap are reused (a tap on the same line makes a range);
    /// otherwise OCR reads around the tap on a thread. Without the OCR models a
    /// tap does nothing (it never asks to download them).
    fn tap(&mut self, at: Point) -> Task<Message> {
        if self.selection.is_none() {
            self.stroke.clear(); // the tap's own dot
        }
        if let Some(word) = &mut self.word
            && word.tap(at)
        {
            self.select_word();
            return self.selected();
        }
        // A click on a selection keeps it unless a word is found there.
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.frame
        else {
            return Task::none();
        };
        if !text::models_present() {
            return Task::none();
        }
        let region = word::region(at, self.screen);
        let (Some([x0, y0, ..]), Some(crop)) = (
            actions::crop_bounds(*width, *height, self.scale, region),
            actions::crop(pixels, *width, *height, self.scale, region),
        ) else {
            return Task::none();
        };
        self.taps = self.taps.wrapping_add(1);
        let id = self.taps;
        background(move |tx| {
            let read = ocr::recognize_in_child(&crop.rgba, crop.width, crop.height);
            let _ = tx.unbounded_send(read.map_err(|e| e.to_string()));
        })
        .map(move |read| Message::Tapped(id, at, [x0, y0], read))
    }

    /// OCR for tap `id` is back: the word under the tap is the selection.
    fn on_tapped(
        &mut self,
        id: u32,
        at: Point,
        origin: [u32; 2],
        read: Result<Vec<TextBlock>, String>,
    ) -> Task<Message> {
        if id != self.taps || self.frame.is_none() {
            return Task::none(); // pressed again meanwhile, or closed
        }
        let blocks = match read {
            Ok(blocks) => blocks,
            Err(e) => {
                eprintln!("[opencircle] reading the tapped word failed: {e}");
                return Task::none();
            }
        };
        let Some(picked) = word::Picked::new(word::lines(blocks, origin, self.scale), at) else {
            return Task::none(); // no text there
        };
        self.word = Some(picked);
        self.select_word();
        self.selected()
    }

    /// The tapped word(s) as the (only) selection: a rounded highlight on their box.
    fn select_word(&mut self) {
        self.others.clear();
        if let Some(word) = &self.word {
            let area = word.area();
            self.selection = Some(overlay::Selection {
                area,
                kind: overlay::Kind::Word,
            });
            self.stroke = vec![area.position(), area.position() + Vector::from(area.size())];
        }
    }

    /// Makes `area` the selection, drawn as a dragged rectangle.
    fn select(&mut self, area: Rectangle) {
        self.selection = Some(overlay::Selection {
            area,
            kind: overlay::Kind::Drag,
        });
        self.stroke = vec![area.position(), area.position() + Vector::from(area.size())];
    }

    /// The previous selection on the shown monitor.
    fn last_region(&self) -> Option<Rectangle> {
        let mine = self
            .regions
            .iter()
            .find(|(origin, _)| *origin == self.origin);
        mine.map(|(_, area)| *area)
    }

    /// Runs `action` on the selection: a finishing action closes the overlay first
    /// (so dialogs aren't hidden under it); a panel action opens its panel; Edit
    /// turns the bar into the editing toolbar. No selection yet = nothing to do.
    fn run(&mut self, action: &'static Action) -> Task<Message> {
        match action.run {
            Run::Edit => return self.open_editor(),
            Run::Translate => return self.start_translate(),
            // A tapped word: Copy copies its text.
            Run::Finish(_) if action.id == copy::ACTION.id && self.edit.is_none() => {
                if let Some(text) = self.word.as_ref().map(word::Picked::text) {
                    let closing = self.close();
                    if let Err(e) = copy::text(text) {
                        eprintln!("copy text failed: {e}");
                    }
                    return closing;
                }
            }
            _ => {}
        }
        // Several selections: Ask AI and Text get each of them (stacked), the
        // rest the box around all of them.
        let all = matches!(
            action.run,
            Run::Panel(actions::Panel::Ask | actions::Panel::Text)
        );
        let Some(crop) = (if all { self.stacked() } else { self.crop() }) else {
            return Task::none();
        };
        match action.run {
            Run::Finish(_) => self.finish(action, crop),
            Run::Panel(kind) => self.open_panel(kind, crop, None),
            Run::Edit | Run::Translate => Task::none(), // handled above
        }
    }

    /// Closes the overlay, then runs a finishing action (Copy, Save) on `crop`.
    fn finish(&mut self, action: &Action, crop: Crop) -> Task<Message> {
        let Run::Finish(finish) = action.run else {
            return Task::none();
        };
        let closing = self.close();
        if let Err(e) = finish(crop) {
            eprintln!("{} failed: {e}", action.id);
        }
        closing
    }

    /// Runs a quick action on the selection (no action bar). Only once: a new
    /// selection after it gets the bar.
    fn run_quick(&mut self, quick: Quick) -> Task<Message> {
        self.purpose = Purpose::Search;
        match quick {
            Quick::Action(action) => self.run(action),
            // No OCR models yet: the Text panel asks before downloading them.
            Quick::CopyText if !text::models_present() => self.run(&text::ACTION),
            Quick::CopyText => {
                let Some(crop) = self.crop() else {
                    return Task::none();
                };
                self.toast = Some("Reading text…");
                background(move |tx| {
                    let _ = tx.unbounded_send(text::read(&crop, |_| {}));
                })
                .map(Message::TextRead)
            }
            Quick::Explain => match self.crop() {
                Some(crop) => self.open_panel(
                    actions::Panel::Ask,
                    crop,
                    Some(actions::ask::PRESETS[0].text(&config::current().translate.target)),
                ),
                None => Task::none(),
            },
        }
    }

    /// Edit in place: the selection's pixels become the editor's canvas, at their
    /// place on screen; the toolbar takes the bar's place. Edits made before
    /// (then Back) are still there.
    fn open_editor(&mut self) -> Task<Message> {
        self.panel = None;
        self.view = self.view.reset(); // the editor works at 1x
        if let Some(edit) = &mut self.edit {
            edit.reopen();
            return Task::none();
        }
        let (
            Some(image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            }),
            Some(selection),
        ) = (&self.frame, self.selection)
        else {
            return Task::none();
        };
        let bounds = actions::crop_bounds(*width, *height, self.scale, selection.area);
        let crop = actions::crop(pixels, *width, *height, self.scale, selection.area);
        if let (Some(bounds), Some(crop)) = (bounds, crop) {
            self.edit = Some(Editing::in_place(crop, bounds, self.scale));
        }
        Task::none()
    }

    fn on_editor(&mut self, msg: editor::Message) -> Task<Message> {
        // Test hook: the editor's Esc (Back) takes the shot of the editor instead.
        if matches!(msg, editor::Message::Back) && std::env::var_os(TEST_SHOT).is_some() {
            return self.test_shot();
        }
        let Some(edit) = &mut self.edit else {
            return Task::none();
        };
        match edit.editor.update(msg) {
            None => Task::none(),
            // Snip has no action bar: back to picking an area.
            Some(Output::Back) if matches!(self.purpose, Purpose::Snip(_)) => {
                self.edit = None;
                self.selection = None;
                self.stroke.clear();
                Task::none()
            }
            Some(Output::Back) => {
                edit.back();
                Task::none()
            }
            Some(Output::Copy(crop)) => self.finish(&copy::ACTION, crop),
            Some(Output::Save(crop)) => self.finish(&save::ACTION, crop),
            Some(Output::Pin(crop)) => self.pin(crop),
        }
    }

    /// Pin: the overlay closes and the image floats where the selection was.
    fn pin(&mut self, crop: Crop) -> Task<Message> {
        let at = (self.selection).map(|s| self.origin + Vector::new(s.area.x, s.area.y));
        let pinned = self.windows.pin(crop, at, self.scale).map(Message::Windows);
        Task::batch([self.close(), pinned])
    }

    /// The running scrolling capture.
    fn scroll(&self) -> Option<&snip::Scroll> {
        match &self.purpose {
            Purpose::Snip(snip) => snip.scroll.as_ref(),
            _ => None,
        }
    }

    fn on_snip(&mut self, msg: snip::Msg) -> Task<Message> {
        let Purpose::Snip(snip) = &mut self.purpose else {
            return Task::none();
        };
        match msg {
            snip::Msg::Kind(kind) => {
                self.selection = None;
                self.stroke.clear();
                let next = snip.switch(kind, self.screen);
                self.snip_next(next)
            }
            snip::Msg::Stop => {
                if let Some(scroll) = &snip.scroll {
                    scroll.stop();
                }
                Task::none()
            }
            snip::Msg::Progress(height) => {
                if let Some(scroll) = &mut snip.scroll {
                    scroll.progress(height);
                }
                Task::none()
            }
            snip::Msg::Done(stitched) => {
                snip.scroll = None;
                self.selection = None;
                self.stroke.clear();
                match stitched {
                    Ok(crop) if crop.height > 0 => {
                        self.edit = Some(Editing::fitted(crop, self.screen, self.scale));
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("scrolling capture failed: {e}"),
                }
                self.show()
            }
        }
    }

    /// Where a snip pick leads: the editor on the area, or a scrolling capture.
    fn snip_next(&mut self, next: Option<snip::Next>) -> Task<Message> {
        match next {
            None => {
                self.selection = None;
                self.stroke.clear();
                Task::none()
            }
            Some(snip::Next::Edit(area)) => {
                self.select(area);
                self.open_editor()
            }
            Some(snip::Next::Scroll(area)) => self.start_scroll(area),
        }
    }

    /// Auto-scrolls and stitches `area` on a thread. The overlay shrinks to the
    /// progress pill beside the area, so it is in none of the frames.
    fn start_scroll(&mut self, area: Rectangle) -> Task<Message> {
        let (Some(image::Handle::Rgba { width, height, .. }), Purpose::Snip(snip)) =
            (&self.frame, &mut self.purpose)
        else {
            return Task::none();
        };
        let Some([x0, y0, x1, y1]) = actions::crop_bounds(*width, *height, self.scale, area) else {
            return Task::none();
        };
        let at = |v: f32, offset: u32| (v * self.scale).round() as i32 + offset as i32;
        let region = (
            at(self.origin.x, x0),
            at(self.origin.y, y0),
            x1 - x0,
            y1 - y0,
        );
        let stop = Arc::new(AtomicBool::new(false));
        snip.scroll = Some(snip::Scroll::new(x1 - x0, stop.clone()));
        let work = background(move |tx| {
            // Let the window shrink first.
            std::thread::sleep(Duration::from_millis(150));
            let progress = |height| {
                let _ = tx.unbounded_send(snip::Msg::Progress(height));
            };
            let stitched = snip::scroll_capture(region, &stop, progress);
            let _ = tx.unbounded_send(snip::Msg::Done(stitched));
        })
        .map(Message::Snip);
        // ponytail: an area as tall as the screen leaves the pill over its bottom
        // edge (in the frames); the stitcher drops it like a sticky footer.
        let at = bar::place(area, snip::PILL, self.screen);
        let pill = self.origin + Vector::new(at.x, at.y);
        window::set_mode(self.overlay, Mode::Windowed)
            .chain(window::resize(self.overlay, snip::PILL))
            .chain(window::move_to(self.overlay, pill))
            .chain(window::gain_focus(self.overlay))
            .chain(work)
    }

    /// Test hook: shoot what the overlay shows after the next frame, then close.
    fn test_shot(&self) -> Task<Message> {
        // No message (= no relayout) in between: a relayout drops the text
        // editor's drawn text.
        let overlay = self.overlay;
        background(|tx| {
            std::thread::sleep(Duration::from_millis(150));
            let _ = tx.unbounded_send(());
        })
        .then(move |()| window::screenshot(overlay))
        .map(Message::TestShot)
    }

    fn open_panel(
        &mut self,
        kind: actions::Panel,
        crop: Crop,
        question: Option<String>,
    ) -> Task<Message> {
        self.panels = self.panels.wrapping_add(1);
        let id = self.panels;
        let (panel, task) = panel::Panel::open(kind, crop, question);
        self.panel = Some((id, panel));
        task.map(move |msg| Message::Panel(id, msg))
    }

    fn on_panel(&mut self, id: u32, msg: panel::Msg) -> Task<Message> {
        // Every opened page is held, whichever panel is showing now.
        if let panel::Msg::LensOpened(Ok(page)) = &msg {
            if self.frame.is_some() {
                self.lens_pages.push(page.clone());
            } else {
                page.clone().release(); // the overlay closed meanwhile
            }
        }
        let Some((_, panel)) = self.panel.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            panel::Msg::Cancel => self.panel = None,
            panel::Msg::Search => return self.run(&lens::ACTION),
            panel::Msg::CopyText => {
                let text = panel.text().unwrap_or_default();
                let closing = self.close();
                if let Err(e) = actions::copy::text(text) {
                    eprintln!("copy text failed: {e}");
                }
                return closing;
            }
            // Lens follow-up: hand the question over to Ask AI.
            panel::Msg::Submit if panel.kind() == actions::Panel::Lens => {
                let question = panel.take_input();
                if question.trim().is_empty() {
                    return Task::none();
                }
                let Some(crop) = self.crop() else {
                    return Task::none();
                };
                return self.open_panel(actions::Panel::Ask, crop, Some(question));
            }
            msg => return panel.update(msg).map(move |msg| Message::Panel(id, msg)),
        }
        Task::none()
    }

    /// The selection cut out of the frozen frame, in physical pixels; with the
    /// edits once it was edited (Ask AI then sees the annotations).
    fn crop(&self) -> Option<Crop> {
        if let Some(edit) = &self.edit {
            return Some(edit.image());
        }
        let (
            Some(image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            }),
            Some(selection),
        ) = (&self.frame, self.selection)
        else {
            return None;
        };
        let all = (self.others.iter()).fold(selection.area, |all, (s, _)| all.union(&s.area));
        actions::crop(pixels, *width, *height, self.scale, all)
    }

    /// Every selection's crop, in the order made, stacked top to bottom on white
    /// (one image: the Text panel reads them in that order, Ask AI sees them all).
    /// One selection: just its crop.
    // ponytail: one stacked image, not one image per selection: the Ask panel
    // (panel.rs) sends one; give `ai::Message` several images when it can.
    fn stacked(&self) -> Option<Crop> {
        if self.others.is_empty() || self.edit.is_some() {
            return self.crop();
        }
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.frame
        else {
            return None;
        };
        let areas = (self.others.iter().map(|(s, _)| s.area)).chain(self.selection.map(|s| s.area));
        let crops: Vec<Crop> = areas
            .filter_map(|area| actions::crop(pixels, *width, *height, self.scale, area))
            .collect();
        stack_crops(&crops)
    }

    /// Frees the frame and drops the panel (stopping its work); Lens pages go
    /// once old enough. The window is shrunk and hidden in steps (see `Hiding`).
    fn close(&mut self) -> Task<Message> {
        if let Some(scroll) = self.scroll() {
            scroll.stop();
        }
        self.panel = None;
        self.lens_pages.drain(..).for_each(lens::Held::release);
        self.frame = None;
        self.stroke = Vec::new();
        self.selection = None;
        self.others = Vec::new();
        self.adding = false;
        self.grab = None;
        self.pan = None;
        self.word = None;
        self.ask = String::new();
        self.drawing = false;
        self.purpose = Purpose::Search;
        self.edit = None;
        self.toast = None;
        self.scan = None;
        self.translate = None;
        self.helper = None;
        self.hiding = Hiding::Empty;
        Task::none()
    }

    /// Translate in place: the selection (or, with no selection left, the whole
    /// frame) is translated on a thread, then swapped in.
    fn start_translate(&mut self) -> Task<Message> {
        let (Some(frame @ image::Handle::Rgba { width, height, .. }), Some(selection)) =
            (&self.frame, self.selection)
        else {
            return Task::none();
        };
        let Some(area) = actions::crop_bounds(*width, *height, self.scale, selection.area) else {
            return Task::none();
        };
        self.panel = None;
        self.edit = None;
        self.view = self.view.reset(); // the translation is drawn at 1x
        let models = text::models_present();
        let (session, task) =
            translate::Session::start(frame.clone(), area, self.cache.clone(), models);
        if session.full() {
            // nothing to outline: the translation is the whole screen
            self.selection = None;
            self.stroke.clear();
        }
        self.translates = self.translates.wrapping_add(1);
        let id = self.translates;
        self.translate = Some((id, session));
        task.map(move |msg| Message::Translate(id, msg))
    }

    fn on_translate(&mut self, id: u32, msg: translate::Msg) -> Task<Message> {
        let Some((_, session)) = self.translate.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            translate::Msg::Close => self.close(),
            translate::Msg::OpenSettings => {
                let closing = self.close();
                let settings = self.windows.open(windows::Kind::Settings);
                closing.chain(settings.map(Message::Windows))
            }
            // The word card goes on as an Ask AI chat (its crop, its explanation).
            translate::Msg::Ask => match session.take_chat() {
                Some((crop, chat)) => {
                    self.panels = self.panels.wrapping_add(1);
                    let id = self.panels;
                    let (panel, task) = panel::Panel::open_chat(crop, chat);
                    self.panel = Some((id, panel));
                    task.map(move |msg| Message::Panel(id, msg))
                }
                None => Task::none(),
            },
            msg => session
                .update(msg)
                .map(move |msg| Message::Translate(id, msg)),
        }
    }

    /// Ctrl+Shift+L (or the tray): live translate on, or off again.
    fn toggle_live(&mut self, at: Instant) -> Task<Message> {
        if let Some(live) = self.live.take() {
            eprintln!("[opencircle] live translate off");
            return live.stop().map(Message::Live);
        }
        if live_layer::unsupported() {
            eprintln!(
                "[opencircle] live translate: Wayland has no layer over other apps; translating once"
            );
            return self.translate_once(at);
        }
        let (live, task) = live_layer::Live::start(self.cache.clone());
        self.live = Some(live);
        task.map(Message::Live)
    }

    fn on_live(&mut self, msg: live_layer::Msg) -> Task<Message> {
        let Some(live) = &mut self.live else {
            return Task::none();
        };
        let off = |app: &mut Self| match app.live.take() {
            Some(live) => live.stop().map(Message::Live),
            None => Task::none(),
        };
        match live.update(msg) {
            live_layer::Outcome::Task(task) => task.map(Message::Live),
            live_layer::Outcome::Off => off(self),
            live_layer::Outcome::OneShot => {
                let stopped = off(self);
                stopped.chain(self.translate_once(Instant::now()))
            }
        }
    }

    /// The one-shot full-screen translate (Ctrl+Shift+T).
    fn translate_once(&mut self, at: Instant) -> Task<Message> {
        let once = QuickAction {
            action: actions::translate::ACTION.id.into(),
            shortcut: String::new(),
            target: Target::FullScreen,
            live: false,
        };
        self.update(Message::Trigger(Trigger::Quick(at, once)))
    }

    fn title(&self, window: window::Id) -> String {
        if self.live.as_ref().and_then(live_layer::Live::window) == Some(window) {
            return "OpenCircle live translate".into();
        }
        if self.live.as_ref().and_then(live_layer::Live::controller) == Some(window) {
            return "OpenCircle live translate controls".into();
        }
        self.windows.title(window).to_string()
    }

    /// The live layer's (and its controller's) see-through background; the rest
    /// follow the OS.
    fn theme(&self, window: window::Id) -> Option<iced::Theme> {
        let live = self.live.as_ref()?;
        [live.window(), live.controller()]
            .contains(&Some(window))
            .then(live_layer::theme)
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        if let Some(page) = self.windows.view(window) {
            return page.map(Message::Windows);
        }
        if let Some(live) = &self.live {
            if live.window() == Some(window) {
                return live.view().map(Message::Live);
            }
            if live.controller() == Some(window) {
                return live.controller_view().map(Message::Live);
            }
        }
        let Some(frame) = &self.frame else {
            return space().into();
        };
        let snip = match &self.purpose {
            Purpose::Snip(snip) => Some(snip),
            _ => None,
        };
        // Scrolling: the window is only the progress pill.
        if let Some(pill) = snip.and_then(|s| s.progress_view()) {
            return pill.map(Message::Snip);
        }
        let overlay = overlay::Overlay {
            stroke: &self.stroke,
            selection: self.selection,
            others: &self.others,
            view: self.view,
            handles: self.handles(),
            opening: self.opening,
            first_draw: &self.first_draw,
            hover: snip.is_some_and(|s| s.kind == snip::Kind::Window) && self.edit.is_none(),
            dim: self.translate.is_none() || self.selection.is_some(),
        };
        let translation = self.translate.as_ref().and_then(|(_, s)| s.image());
        // The frame is its own layer: iced draws a canvas's images above its shapes.
        let mut shown = image(translation.unwrap_or(frame).clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Fill);
        if let Some(part) = self.view.crop() {
            shown = shown.crop(part); // zoomed in
        }
        let mut layers = stack![
            shown,
            Element::from(canvas(overlay).width(Fill).height(Fill)).map(Message::Overlay),
            border::view(
                match snip {
                    Some(_) => border::Mode::Static,
                    None => border::Mode::Sweep,
                },
                self.scale,
                false,
            ),
        ];
        if let Some(edit) = &self.edit {
            if edit.is_open() {
                return layers
                    .push(edit.view(self.screen).map(Message::Editor))
                    .into();
            }
            layers = layers.extend(edit.flat_view());
        }
        let top = if let Some(toast) = self.toast {
            container(widgets::pill(toast))
                .center_x(Fill)
                .padding(24)
                .into()
        } else if let Some((id, panel)) = &self.panel {
            let id = *id;
            panel
                .view(self.screen)
                .map(move |msg| Message::Panel(id, msg))
        } else if let Some((id, session)) = &self.translate {
            let id = *id;
            session
                .view(self.scale, self.screen)
                .map(move |msg| Message::Translate(id, msg))
        } else if let Some(snip) = snip {
            snip.view().map(Message::Snip)
        } else if let Some(selection) = self.selection {
            match self.purpose {
                Purpose::Search if !self.drawing => {
                    let chip = self
                        .scan
                        .as_ref()
                        .and_then(|s| s.qr.as_deref())
                        .map(|text| {
                            let open = qr::web_url(text).map(|_| Message::Qr(qr::Act::Open));
                            bar::qr_chip(text, Message::Qr(qr::Act::Copy), open)
                        });
                    // The action bar keeps clear of the ask bar at the bottom.
                    let above = Size::new(self.screen.width, self.screen.height - ASK_ZONE);
                    let area = self.view.rect_to_screen(selection.area);
                    stack![bar::view(area, above, chip, Message::Run), self.ask_bar()].into()
                }
                _ => space().into(),
            }
        } else {
            container(widgets::pill(HINT))
                .center_x(Fill)
                .padding(24)
                .into()
        };
        layers = layers.push(top);
        // A black capture (DRM / protected window): say so, but keep Copy, Save and Text.
        if self.selection.is_some() && self.scan.as_ref().is_some_and(|s| s.protected) {
            layers = layers.push(
                container(widgets::pill(PROTECTED))
                    .center_x(Fill)
                    .padding(24),
            );
        }
        layers.into()
    }

    /// "Ask about your selection…" at the bottom centre: Enter opens Ask AI with it.
    fn ask_bar(&self) -> Element<'_, Message> {
        let field = widgets::input(
            ASK_INPUT,
            "Ask about your selection…",
            &self.ask,
            Message::AskInput,
            Message::AskSubmit,
        );
        // opaque: a press on it doesn't start a new selection.
        let bar = opaque(widgets::surface(field.width(ASK_WIDTH)));
        container(bar)
            .center_x(Fill)
            .align_bottom(Fill)
            .padding(ASK_MARGIN)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        // Live translate: where its controller was dragged.
        let moved = match self.live {
            Some(_) => iced::event::listen_with(|event, _, id| match event {
                iced::Event::Window(window::Event::Moved(at)) => {
                    Some(Message::Live(live_layer::Msg::Moved(id, at)))
                }
                _ => None,
            }),
            None => Subscription::none(),
        };
        let triggers = Subscription::batch([
            Subscription::run(triggers).map(Message::Trigger),
            self.windows.subscription().map(Message::Windows),
            moved,
        ]);
        if self.frame.is_some() {
            // Also when a text field took the key (it only unfocuses on Esc).
            let escape = iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                }) => Some(Message::Escape),
                _ => None,
            });
            // Alt held = the original under a translation.
            let alt = iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                    Some(Message::Peek(m.alt()))
                }
                _ => None,
            });
            let alt = if self.translate.is_some() {
                alt
            } else {
                Subscription::none()
            };
            Subscription::batch([triggers, escape, alt])
        } else if self.hiding != Hiding::No {
            // One stream for both, so a resize is always seen before the frames after it.
            // Every message makes iced redraw, so frames keep coming while this runs.
            let steps = iced::event::listen_raw(|event, _, _| match event {
                iced::Event::Window(window::Event::RedrawRequested(_)) => Some(Message::Drawn),
                iced::Event::Window(window::Event::Resized(size)) => Some(Message::Resized(size)),
                _ => None,
            });
            Subscription::batch([triggers, steps])
        } else {
            triggers
        }
    }
}

/// Test hook: on Esc, save what the overlay shows before closing (WSLg can't grab it).
const TEST_SHOT: &str = "OPENCIRCLE_TEST_SHOT";

fn save_test_shot(shot: &window::Screenshot) {
    let Some(path) = std::env::var_os(TEST_SHOT) else {
        return;
    };
    let result = actions::write_png(path.as_ref(), shot.size.width, shot.size.height, &shot.rgba);
    if let Err(e) = result {
        eprintln!("test shot failed: {e}");
    }
}

/// `crops` one under the other, left-aligned, `STACK_GAP` px of white between
/// them and right of the narrower ones. `None` for no crops.
fn stack_crops(crops: &[Crop]) -> Option<Crop> {
    let width = crops.iter().map(|c| c.width).max()?;
    let gaps = STACK_GAP * (crops.len() as u32 - 1);
    let height = crops.iter().map(|c| c.height).sum::<u32>() + gaps;
    let mut rgba = vec![255; width as usize * height as usize * 4];
    let mut y = 0;
    for c in crops {
        let row = c.width as usize * 4;
        for (i, line) in c.rgba.chunks_exact(row).enumerate() {
            let at = (y + i) * width as usize * 4;
            rgba[at..at + row].copy_from_slice(line);
        }
        y += (c.height + STACK_GAP) as usize;
    }
    Some(Crop {
        width,
        height,
        rgba,
    })
}

/// Runs `work` on its own thread; what it sends arrives as messages, so the UI
/// never waits on it. Dropping (aborting) the task disconnects `work`'s sender.
fn background<T: Send + 'static>(
    work: impl FnOnce(&UnboundedSender<T>) + Send + 'static,
) -> Task<T> {
    let (tx, rx) = mpsc::unbounded();
    let spawned = std::thread::Builder::new()
        .name("background".into())
        .spawn(move || work(&tx));
    if let Err(e) = spawned {
        eprintln!("cannot start a thread: {e}");
    }
    Task::stream(rx)
}

fn triggers() -> impl Stream<Item = Trigger> {
    stream::iter(TRIGGERS.lock().unwrap().take()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closing_app() -> App {
        let mut app = App::idle(window::Id::unique());
        let _ = app.close();
        app
    }

    #[test]
    fn close_shrinks_then_hides() {
        let mut app = closing_app();
        let _ = app.update(Message::Drawn);
        let _ = app.update(Message::Drawn); // a full-size frame before the resize lands
        let _ = app.update(Message::Resized(Size::new(3440.0, 1440.0)));
        assert_eq!(app.hiding, Hiding::Shrinking(1));
        let _ = app.update(Message::Resized(Size::new(1.0, 1.0)));
        let _ = app.update(Message::Drawn);
        assert_eq!(app.hiding, Hiding::No);
    }

    #[test]
    fn close_hides_even_if_the_window_never_shrinks() {
        let mut app = closing_app();
        for _ in 0..=SHRINK_FRAMES {
            let _ = app.update(Message::Drawn);
        }
        assert_ne!(app.hiding, Hiding::No);
        let _ = app.update(Message::Drawn);
        assert_eq!(app.hiding, Hiding::No);
    }

    #[test]
    fn a_translation_ends_with_its_overlay() {
        let mut app = closing_app();
        let frame = image::Handle::from_rgba(40, 20, vec![0; 40 * 20 * 4]);
        // no models: the session waits for consent (no thread starts)
        let (session, _) =
            translate::Session::start(frame.clone(), [0, 0, 40, 20], app.cache.clone(), false);
        app.frame = Some(frame);
        app.hiding = Hiding::No;
        app.translate = Some((7, session));
        let _ = app.update(Message::Translate(6, translate::Msg::Close));
        assert!(app.frame.is_some(), "a stale session's message is ignored");
        let _ = app.update(Message::Peek(true));
        // declining the download closes the overlay and drops the session
        let _ = app.update(Message::Translate(7, translate::Msg::Close));
        assert!(app.translate.is_none() && app.frame.is_none());
        assert_eq!(app.hiding, Hiding::Empty);
    }

    /// An open overlay over `frame` with all of it selected (nothing scanned yet).
    fn selected_app(frame: &Crop) -> (App, Rectangle) {
        let mut app = closing_app();
        let area = Rectangle::with_size(Size::new(frame.width as f32, frame.height as f32));
        app.frame = Some(image::Handle::from_rgba(
            frame.width,
            frame.height,
            frame.rgba.clone(),
        ));
        app.hiding = Hiding::No;
        app.screen = area.size();
        app.view = overlay::View::new(frame.width, frame.height, app.screen);
        app.selection = Some(overlay::Selection {
            area,
            kind: overlay::Kind::Drag,
        });
        (app, area)
    }

    fn overlay_texts(app: &App) -> Vec<String> {
        widgets::shot::texts(app.view(app.overlay))
    }

    #[test]
    fn a_qr_code_in_the_selection_gets_a_chip_and_nothing_opens() {
        // A code (selected) on the right of a wider grey screen.
        let code = qr::tests::qr_crop("https://example.com/menu", 5, 0);
        let mut frame = Crop {
            width: 1200,
            height: 600,
            rgba: [140, 140, 140, 255].repeat(1200 * 600),
        };
        for y in 0..code.height as usize {
            let from = y * code.width as usize * 4;
            let to = ((y + 20) * 1200 + 700) * 4;
            let row = code.width as usize * 4;
            frame.rgba[to..to + row].copy_from_slice(&code.rgba[from..from + row]);
        }
        let (mut app, _) = selected_app(&frame);
        let area = Rectangle::new(
            Point::new(700.0, 20.0),
            Size::new(code.width as f32, code.height as f32),
        );
        app.selection = Some(overlay::Selection {
            area,
            kind: overlay::Kind::Drag,
        });
        // The scan of a selection that has since moved on is dropped.
        let moved = Rectangle::with_size(area.size() * 0.5);
        let _ = app.update(Message::Scanned(moved, qr::scan(&frame)));
        assert!(app.scan.is_none());
        assert!(!overlay_texts(&app).iter().any(|t| t.starts_with("QR code")));

        let scan = qr::scan(&app.crop().unwrap());
        let _ = app.update(Message::Scanned(area, scan));
        let texts = overlay_texts(&app);
        assert!(
            texts.contains(&"QR code: https://example.com/menu".into()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Open link · example.com".into()));
        assert!(
            app.toast.is_none() && app.frame.is_some(),
            "nothing happened by itself"
        );
        let size = app.screen;
        widgets::shot::save("overlay-qr-chip", app.view(app.overlay), size);

        // A new press forgets the old code.
        let _ = app.update(Message::Overlay(overlay::Event::Press {
            at: Point::new(5.0, 5.0),
            shift: false,
        }));
        assert!(app.scan.is_none());
    }

    #[test]
    fn a_black_capture_says_it_is_protected_and_keeps_the_bar() {
        let black = Crop {
            width: 600,
            height: 360,
            rgba: [0, 0, 0, 255].repeat(600 * 360),
        };
        let (mut app, area) = selected_app(&black);
        assert!(!overlay_texts(&app).contains(&PROTECTED.to_string()));
        let _ = app.update(Message::Scanned(area, qr::scan(&app.crop().unwrap())));
        let texts = overlay_texts(&app);
        assert!(texts.contains(&PROTECTED.to_string()), "{texts:?}");
        assert!(texts.contains(&"Copy".to_string()) && texts.contains(&"Text".to_string()));
        widgets::shot::save("overlay-protected", app.view(app.overlay), area.size());
        // An ordinary capture shows no such pill.
        let grey = Crop {
            rgba: [90, 90, 90, 255].repeat(600 * 360),
            ..black
        };
        let (mut app, area) = selected_app(&grey);
        let _ = app.update(Message::Scanned(area, qr::scan(&app.crop().unwrap())));
        assert!(!overlay_texts(&app).contains(&PROTECTED.to_string()));
    }

    /// An open overlay on a `w` x `h` frame at `scale`; pixel (x, y) is
    /// [x % 256, y % 256, x / 256, 255].
    fn open_app(w: u32, h: u32, scale: f32) -> App {
        let mut app = App::idle(window::Id::unique());
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, (x >> 8) as u8, 255]))
            .collect();
        app.frame = Some(image::Handle::from_rgba(w, h, rgba));
        app.scale = scale;
        app.screen = Size::new(w as f32 / scale, h as f32 / scale);
        app.view = overlay::View::new(w, h, app.screen);
        app
    }

    fn event(app: &mut App, e: overlay::Event) {
        let _ = app.on_overlay(e);
    }

    /// A press-drag-release on screen from `a` to `b`.
    fn drag(app: &mut App, a: Point, b: Point, shift: bool) {
        event(app, overlay::Event::Press { at: a, shift });
        for i in 1..=10 {
            event(app, overlay::Event::Move(a + (b - a) * (i as f32 / 10.0)));
        }
        event(app, overlay::Event::Release);
    }

    fn ring(cx: f32, cy: f32, r: f32) -> Vec<Point> {
        (0..=32)
            .map(|i| {
                let a = i as f32 / 32.0 * std::f32::consts::TAU;
                Point::new(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    }

    fn circle(app: &mut App, c: Point, r: f32, shift: bool) {
        let ring = ring(c.x, c.y, r);
        event(app, overlay::Event::Press { at: ring[0], shift });
        ring[1..]
            .iter()
            .for_each(|&p| event(app, overlay::Event::Move(p)));
        event(app, overlay::Event::Release);
    }

    fn close_to(a: Rectangle, b: Rectangle) -> bool {
        [
            (a.x, b.x),
            (a.y, b.y),
            (a.width, b.width),
            (a.height, b.height),
        ]
        .iter()
        .all(|(p, q)| (p - q).abs() < 0.01)
    }

    #[test]
    fn handles_resize_and_moving_drags_at_any_zoom_and_scale() {
        // scale 2 (a 400x300 overlay on 800x600 pixels), zoomed in 1.5625x
        let mut app = open_app(800, 600, 2.0);
        event(
            &mut app,
            overlay::Event::Zoom {
                at: Point::new(200.0, 150.0),
                notches: 2.0,
            },
        );
        let v = app.view;
        assert!(v.zoom() > 1.5);
        drag(
            &mut app,
            Point::new(100.0, 100.0),
            Point::new(200.0, 180.0),
            false,
        );
        let sel = app.selection.expect("a rectangle");
        assert!(
            app.selected().units() > 0,
            "a drawn selection is scanned (QR)"
        );
        assert_eq!(sel.kind, overlay::Kind::Drag);
        let on_screen = v.rect_to_screen(sel.area);
        let want = Rectangle::new(Point::new(100.0, 100.0), Size::new(100.0, 80.0));
        assert!(close_to(on_screen, want), "{on_screen:?}");
        assert!(app.handles());

        // the bottom-right handle, 20 px right and 10 down on screen
        drag(
            &mut app,
            Point::new(200.0, 180.0),
            Point::new(220.0, 190.0),
            false,
        );
        let resized = v.rect_to_screen(app.selection.unwrap().area);
        let want = Rectangle::new(Point::new(100.0, 100.0), Size::new(120.0, 90.0));
        assert!(close_to(resized, want), "{resized:?}");

        // drag inside: it moves, same size
        drag(
            &mut app,
            Point::new(150.0, 150.0),
            Point::new(130.0, 160.0),
            false,
        );
        let moved = v.rect_to_screen(app.selection.unwrap().area);
        let want = Rectangle::new(Point::new(80.0, 110.0), Size::new(120.0, 90.0));
        assert!(close_to(moved, want), "{moved:?}");
        // the crop is what is under it: image px x 2, the top-left pixel included
        let crop = app.crop().unwrap();
        let area = app.selection.unwrap().area;
        let first = [area.x * 2.0, area.y * 2.0].map(|v| v.floor() as u32);
        assert_eq!(crop.rgba[..2], [first[0] as u8, first[1] as u8]);
        assert!((crop.width as f32 - area.width * 2.0).abs() <= 1.0);

        // a freehand loop resized by a corner scales its stroke with the box
        circle(&mut app, Point::new(300.0, 150.0), 40.0, false);
        let lp = app.selection.unwrap();
        assert_eq!(lp.kind, overlay::Kind::Loop);
        let box_ = v.rect_to_screen(lp.area);
        let corner = Point::new(box_.x, box_.y);
        drag(&mut app, corner, corner - Vector::new(40.0, 0.0), false);
        let grown = app.selection.unwrap();
        assert_eq!(grown.kind, overlay::Kind::Loop, "still the drawn shape");
        assert!((grown.area.width - lp.area.width * 1.5).abs() < 0.5);
        let xs = app.stroke.iter().map(|p| p.x);
        let min = xs.fold(f32::MAX, f32::min);
        assert!(
            (min - grown.area.x).abs() < 0.01,
            "the stroke fills the new box"
        );

        // double right-click: back to 1x
        event(&mut app, overlay::Event::ZoomReset);
        assert_eq!(app.view.zoom(), 1.0);
    }

    #[test]
    fn shift_adds_selections_and_actions_get_all_of_them() {
        let mut app = open_app(400, 300, 1.0);
        circle(&mut app, Point::new(60.0, 60.0), 30.0, false);
        circle(&mut app, Point::new(300.0, 200.0), 40.0, true);
        assert_eq!(app.others.len(), 1, "two selections");
        // a Shift tap adds nothing and keeps both
        drag(
            &mut app,
            Point::new(10.0, 290.0),
            Point::new(11.0, 290.0),
            true,
        );
        assert_eq!(app.others.len(), 1);
        assert!(app.selection.is_some());
        let (a, b) = (app.others[0].0.area, app.selection.unwrap().area);

        // Lens / Copy: the box around all of them
        let union = app.crop().unwrap();
        assert_eq!(
            (union.width, union.height),
            (
                (b.x + b.width).ceil() as u32 - a.x.floor() as u32,
                (b.y + b.height).ceil() as u32 - a.y.floor() as u32
            )
        );
        // Ask AI / Text: each one, stacked in the order made
        let stacked = app.stacked().unwrap();
        let (ca, cb) = (
            actions::crop(&pixels(&app), 400, 300, 1.0, a).unwrap(),
            actions::crop(&pixels(&app), 400, 300, 1.0, b).unwrap(),
        );
        assert_eq!(stacked.width, ca.width.max(cb.width));
        assert_eq!(stacked.height, ca.height + STACK_GAP + cb.height);
        assert_eq!(stacked.rgba[..4], ca.rgba[..4]);
        let second = (ca.height + STACK_GAP) as usize * stacked.width as usize * 4;
        assert_eq!(stacked.rgba[second..second + 4], cb.rgba[..4]);
        assert_eq!(
            stacked.rgba[(ca.width as usize) * 4..][..4],
            [255; 4],
            "white beside"
        );

        // a new selection without Shift starts over
        circle(&mut app, Point::new(200.0, 100.0), 30.0, false);
        assert!(app.others.is_empty());
        assert_eq!(app.stacked().unwrap().height, app.crop().unwrap().height);
        let _ = app.close();
        assert!(app.others.is_empty() && app.word.is_none() && app.ask.is_empty());
    }

    fn pixels(app: &App) -> Vec<u8> {
        match &app.frame {
            Some(image::Handle::Rgba { pixels, .. }) => pixels.to_vec(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_tapped_word_is_the_selection_and_its_range_grows() {
        let mut app = open_app(400, 300, 2.0);
        // OCR read one line in a crop at frame px (100, 40): "Press any key", 10 image px a char
        let block = TextBlock {
            text: "Press any key".into(),
            bounds: [20.0, 20.0, 260.0, 40.0],
            confidence: 1.0,
        };
        app.taps = 7;
        let at = Point::new(135.0, 40.0); // image px: on "any" (image x 120..150)
        let stale = app.on_tapped(6, at, [100, 40], Ok(vec![block.clone()]));
        assert!(stale.units() == 0 && app.selection.is_none(), "an old tap");
        let scan = app.on_tapped(7, at, [100, 40], Ok(vec![block]));
        assert_eq!(scan.units(), 0, "a word is not scanned for a QR code");
        let sel = app.selection.expect("the word");
        assert_eq!(sel.kind, overlay::Kind::Word);
        assert_eq!(app.word.as_ref().unwrap().text(), "any");
        // a second tap on the line (no OCR: the lines are kept) makes a range
        let key = app.view.to_screen(Point::new(175.0, 40.0));
        drag(&mut app, key, key, false);
        assert_eq!(app.word.as_ref().unwrap().text(), "any key");
        assert!(app.selection.unwrap().area.width > sel.area.width);
        // resizing turns it into a plain box: no longer the words
        let r = app.view.rect_to_screen(app.selection.unwrap().area);
        let corner = Point::new(r.x + r.width, r.y + r.height);
        drag(&mut app, corner, corner + Vector::new(30.0, 0.0), false);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Drag);
        assert!(app.word.is_none());
    }

    #[test]
    fn the_ask_bar_shows_with_a_selection_and_opens_ask_ai() {
        let mut app = open_app(400, 300, 1.0);
        drag(
            &mut app,
            Point::new(50.0, 50.0),
            Point::new(150.0, 120.0),
            false,
        );
        let _ = app.update(Message::AskInput("what is this?".into()));
        let texts = widgets::shot::texts(app.view(app.overlay));
        assert!(texts.contains(&"what is this?".to_string()), "{texts:?}");
        let _ = app.update(Message::AskInput("   ".into()));
        let _ = app.update(Message::AskSubmit);
        assert!(app.panel.is_none(), "a blank question opens nothing");
        let _ = app.update(Message::AskInput("what is this?".into()));
        let _ = app.update(Message::AskSubmit);
        let (_, panel) = app.panel.as_ref().expect("Ask AI opens");
        assert_eq!(panel.kind(), actions::Panel::Ask);
        assert!(app.ask.is_empty());
    }

    #[test]
    fn stacked_crops_keep_their_pixels() {
        let red = Crop {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 255, 0, 0, 255],
        };
        let blue = Crop {
            width: 1,
            height: 2,
            rgba: vec![0, 0, 255, 255, 0, 0, 255, 255],
        };
        let s = stack_crops(&[red, blue]).unwrap();
        assert_eq!((s.width, s.height), (2, 1 + STACK_GAP + 2));
        let px = |x: u32, y: u32| &s.rgba[((y * 2 + x) * 4) as usize..][..4];
        assert_eq!(px(1, 0), [255, 0, 0, 255]);
        assert_eq!(px(0, 1), [255; 4], "the gap");
        assert_eq!(px(0, 1 + STACK_GAP), [0, 0, 255, 255]);
        assert_eq!(px(1, 1 + STACK_GAP), [255; 4], "beside the narrow one");
        assert!(stack_crops(&[]).is_none());
    }
}
