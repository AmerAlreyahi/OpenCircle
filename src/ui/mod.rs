//! The iced daemon: owns the (pre-created, hidden) overlay window and its state.
mod bar;
mod edit;
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

pub use tray::start as start_tray;

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use iced::futures::stream::{self, Stream, StreamExt};
use iced::keyboard::{self, key::Named};
use iced::widget::{canvas, container, image, space, stack};
use iced::window::{self, Level, Mode};
use iced::{ContentFit, Element, Fill, Point, Rectangle, Size, Subscription, Task, Vector};

use crate::actions::{self, Action, Crop, Run, copy, lens, save, text};
use crate::config::{self, QuickAction, Target};
use crate::editor::{self, Output};
use crate::translate::pipeline::Cache;
use crate::{Trigger, capture};
use edit::Editing;
use quick::{Quick, Start};
use snip::Snip;

const HINT: &str = "Circle, drag or tap anything · Esc to close";

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
    /// From the translate session with this number (stale numbers are ignored).
    Translate(u32, translate::Msg),
    /// Alt went down / up (peek at the original under a translation).
    Peek(bool),
    Live(live_layer::Msg),
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
    stroke: Vec<Point>,
    drawing: bool,
    selection: Option<overlay::Selection>,
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
    /// Translate in place on this overlay, and its number.
    translate: Option<(u32, translate::Session)>,
    translates: u32,
    /// Translations by text, shared by one-shot and live mode.
    cache: Arc<Cache>,
    /// Live translate, while on.
    live: Option<live_layer::Live>,
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
            overlay,
            capturing: false,
            frame: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
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
            translate: None,
            translates: 0,
            cache: Arc::new(Cache::new(CACHE_SIZE)),
            live: None,
        };
        let start = app.windows.start().map(Message::Windows);
        (app, Task::batch([open.discard(), start]))
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
            Message::ToastDone if self.toast.is_some() => self.close(),
            Message::ToastDone => Task::none(),
            Message::Panel(id, msg) => self.on_panel(id, msg),
            Message::Run(action) => self.run(action),
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
        self.frame = Some(shot.image);
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
        window::move_to(self.overlay, self.origin)
            .chain(window::resize(self.overlay, self.screen))
            .chain(window::set_mode(self.overlay, Mode::Fullscreen))
            .chain(window::gain_focus(self.overlay))
    }

    fn on_overlay(&mut self, event: overlay::Event) -> Task<Message> {
        // The editor, a toast or a scrolling capture has the screen.
        if self.edit.as_ref().is_some_and(Editing::is_open)
            || self.toast.is_some()
            || self.scroll().is_some()
        {
            return Task::none();
        }
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
                overlay::Event::Release if overlay::select(&self.stroke).is_none() => {
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
            overlay::Event::Press(p) => {
                // A new selection: the panel and the edits were about the old one.
                self.panel = None;
                self.edit = None;
                self.stroke = vec![p];
                self.selection = None;
                self.drawing = true;
            }
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
            overlay::Event::Release => {
                self.drawing = false;
                self.selection = overlay::select(&self.stroke);
                let area = self.selection.map(|s| s.area);
                if let Some(area) = area {
                    self.regions.retain(|(origin, _)| *origin != self.origin);
                    self.regions.push((self.origin, area));
                }
                match &self.purpose {
                    Purpose::Snip(snip) => {
                        let at = self.stroke.last().copied().unwrap_or(Point::ORIGIN);
                        let next = snip.pick(area, at, self.screen);
                        return self.snip_next(next);
                    }
                    Purpose::Quick(quick, _) if area.is_some() => {
                        let quick = *quick;
                        return self.run_quick(quick);
                    }
                    _ if area.is_none() => self.stroke.clear(),
                    _ => {}
                }
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
        }
        Task::none()
    }

    /// Makes `area` the selection, drawn as a dragged rectangle.
    fn select(&mut self, area: Rectangle) {
        self.selection = Some(overlay::Selection { area, drag: true });
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
            _ => {}
        }
        let Some(crop) = self.crop() else {
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
                Some(crop) => self.open_panel(actions::Panel::Ask, crop, Some("Explain".into())),
                None => Task::none(),
            },
        }
    }

    /// Edit in place: the selection's pixels become the editor's canvas, at their
    /// place on screen; the toolbar takes the bar's place. Edits made before
    /// (then Back) are still there.
    fn open_editor(&mut self) -> Task<Message> {
        self.panel = None;
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
        actions::crop(pixels, *width, *height, self.scale, selection.area)
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
        self.drawing = false;
        self.purpose = Purpose::Search;
        self.edit = None;
        self.toast = None;
        self.translate = None;
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
        self.windows.title(window).to_string()
    }

    /// The live layer's see-through background; the rest follow the OS.
    fn theme(&self, window: window::Id) -> Option<iced::Theme> {
        let live = self.live.as_ref().and_then(live_layer::Live::window);
        (live == Some(window)).then(live_layer::theme)
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        if let Some(page) = self.windows.view(window) {
            return page.map(Message::Windows);
        }
        if let Some(live) = &self.live
            && live.window() == Some(window)
        {
            return live.view().map(Message::Live);
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
            opening: self.opening,
            first_draw: &self.first_draw,
            hover: snip.is_some_and(|s| s.kind == snip::Kind::Window) && self.edit.is_none(),
            dim: self.translate.is_none() || self.selection.is_some(),
        };
        let translation = self.translate.as_ref().and_then(|(_, s)| s.image());
        // The frame is its own layer: iced draws a canvas's images above its shapes.
        let mut layers = stack![
            image(translation.unwrap_or(frame).clone())
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Fill),
            Element::from(canvas(overlay).width(Fill).height(Fill)).map(Message::Overlay),
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
                Purpose::Search => bar::view(selection.area, self.screen, Message::Run),
                _ => space().into(),
            }
        } else {
            container(widgets::pill(HINT))
                .center_x(Fill)
                .padding(24)
                .into()
        };
        layers.push(top).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let triggers = Subscription::batch([
            Subscription::run(triggers).map(Message::Trigger),
            self.windows.subscription().map(Message::Windows),
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
        let mut app = App {
            overlay: window::Id::unique(),
            capturing: false,
            frame: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
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
            translate: None,
            translates: 0,
            cache: Arc::new(Cache::new(8)),
            live: None,
        };
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
}
