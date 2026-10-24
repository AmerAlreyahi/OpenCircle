//! The iced daemon: owns the overlay window (pre-created and hidden between uses
//! where windows can hide, see `can_hide_windows`) and its state.
mod bar;
mod border;
mod edit;
mod glass;
mod live_controller;
mod live_layer;
mod onboarding;
mod overlay;
mod panel;
mod picker;
mod quick;
mod settings;
mod shape;
mod snip;
mod song;
pub mod theme;
mod translate;
mod tray;
mod update_card;
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
use crate::config::{self, After, QuickAction, SearchWith, SelectStyle, SelectionConfig, Target};
use crate::editor::{self, Output};
use crate::ocr::{self, TextBlock};
use crate::translate::pipeline::Cache;
use crate::{Trigger, capture};
use edit::Editing;
use quick::{Quick, Start};
use snip::Snip;
use widgets::card;

const HINT: &str = "Circle, drag or tap anything · Esc to close";
/// Shown over a selection that came out black (DRM / protected windows).
const PROTECTED: &str = "This window is protected; it can't be captured";

/// The ask bar at the bottom of the overlay: its text field, width, and the
/// height kept free for it (the action bar stays above it).
const ASK_INPUT: &str = "overlay-ask";
const ASK_WIDTH: f32 = 480.0;
const ASK_ZONE: f32 = widgets::CONTROL + 2.0 * widgets::SURFACE_PADDING + 2.0 * ASK_MARGIN;
const ASK_MARGIN: f32 = 24.0;

/// A capture still unanswered after this is given up (a Wayland portal may wait
/// on a dialog; its late answer is dropped).
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(20);

/// Snip reads the controls (UI Automation) of this many topmost windows.
const UIA_WINDOWS: usize = 4;

/// How long the screen's text read waits after the overlay opens.
const SCREEN_READ_DELAY: Duration = Duration::from_millis(150);

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

/// Gives freed heap memory back to the OS (glibc keeps it otherwise). Cheap
/// (a few ms); call once big buffers are gone and the app is going idle.
pub(crate) fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim only releases free heap pages.
    unsafe {
        libc::malloc_trim(0);
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
    /// Capture number `.0`'s thread is done (`Instant` = trigger time, latency log).
    Captured(u32, Instant, Result<Shot, String>),
    /// Capture number `.0` has waited `CAPTURE_TIMEOUT`.
    CaptureTimedOut(u32),
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
    /// OCR for a tap or a text drag read these lines.
    Tapped(WordRead, Result<Vec<TextBlock>, String>),
    /// The bottom bar: its ask field's text, Enter in it, its Translate button.
    AskInput(String),
    AskSubmit,
    TranslateScreen,
    /// OCR read the whole frame of this opening (main mode, at open).
    ScreenText(u32, Result<Vec<word::Line>, String>),
    /// The "Select text" chip under a snapped box, and the lines OCR read in it.
    SelectText,
    TextLines(Rectangle, Result<Vec<word::Line>, String>),
    /// The "More" chip: the action bar beside an auto-search.
    More,
    /// The select-text popup.
    Picked(PickAct),
    /// While the result card is dragged: the pointer moved / was let go.
    CardMoved(Point),
    CardReleased,
    /// The music button (or the song_search quick action).
    SongStart,
    /// From the song search with this number (stale numbers are ignored).
    Song(u32, song::Msg),
    /// Whether a song-search key is saved (checked once, off the UI thread).
    SongKey,
    /// Try the waiting update again (it waits while OpenCircle is in use).
    UpdateTick,
}

/// What the select-text popup does with the picked words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickAct {
    Copy,
    Translate,
    Search,
    Ask,
}

/// Select-text mode on a snapped box: its words (`None` while OCR reads them)
/// and the ones picked by a press-drag over them.
struct TextMode {
    lines: Option<Vec<word::Line>>,
    boxes: Vec<Rectangle>,
    picked: Option<word::Picked>,
    picking: bool,
}

/// Between the chips under a snapped box, and between them and the box
/// (logical px).
const CHIP_GAP: f32 = 8.0;
const CHIP_SPACE: f32 = 12.0;

/// What an OCR read for words was asked for, to place its answer.
#[derive(Debug, Clone, Copy)]
struct WordRead {
    /// The tap's number (a newer press makes older reads stale).
    id: u32,
    /// The tap, or where the drag ended (image px).
    at: Point,
    /// The crop's top-left (frame px) and extent (image px).
    origin: [u32; 2],
    region: Rectangle,
    /// Stretch the picked words to `at` (else: pick the word at `at`).
    extend: bool,
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
    /// The press began on highlighted text: dragging stretches the words.
    text: bool,
    /// On a word selection's handle: its start (`true`) or end.
    end: Option<bool>,
    /// Where the pointer is now.
    to: Point,
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

/// Whether a window can be hidden and shown again. Not on Wayland: winit's
/// `set_visible(false)` is a no-op there, so a "hidden" window stays on the
/// desktop as an empty gray box. There windows are closed instead, and opened
/// again when needed.
pub(crate) fn can_hide_windows() -> bool {
    #[cfg(target_os = "linux")]
    return !winit_picks_wayland(
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_SOCKET").as_deref(),
    );
    #[cfg(not(target_os = "linux"))]
    true
}

/// winit's own rule (0.30): Wayland whenever either variable is set and not empty.
#[cfg(any(target_os = "linux", test))]
fn winit_picks_wayland(
    display: Option<&std::ffi::OsStr>,
    socket: Option<&std::ffi::OsStr>,
) -> bool {
    [display, socket]
        .into_iter()
        .any(|v| v.is_some_and(|v| !v.is_empty()))
}

/// The overlay window: hidden (pre-created), or full-screen at `size`.
fn overlay_window(shown: Option<Size>) -> window::Settings {
    window::Settings {
        size: shown.unwrap_or(window::Settings::default().size),
        visible: shown.is_some(),
        fullscreen: shown.is_some(),
        decorations: false,
        resizable: false,
        level: Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..window::Settings::default()
    }
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
    /// Snip mode: the mode to start in, the windows on this monitor (logical
    /// px, topmost first) and the window the user was in (Windows).
    snip: Option<(snip::Kind, Vec<snip::Pick>, Option<isize>)>,
}

struct App {
    /// The overlay window's id; a closed window's while `overlay_open` is false.
    overlay: window::Id,
    /// The overlay window exists: always where it is pre-created and hidden
    /// (`can_hide`), else only while it is shown.
    overlay_open: bool,
    /// `can_hide_windows()`, read once at boot.
    can_hide: bool,
    /// The number of the capture running on its thread. Cleared by Esc or the
    /// timeout: that capture's late answer is then dropped.
    capturing: Option<u32>,
    /// Captures started (numbers them).
    captures: u32,
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
    /// Main mode: every text line on the frame (image px), read at open;
    /// `None` until OCR answers (or without its models).
    screen_text: Option<Vec<word::Line>>,
    /// A press that began on text: its stroke so far (image px). It selects
    /// words unless it turns into a loop (`overlay::winds`).
    text_press: Option<Vec<Point>>,
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
    /// How selections behave (`[selection]`), read when the overlay opens.
    behaviour: SelectionConfig,
    /// A snapped selection's drawn stroke (image px) morphing into its box.
    morph: Option<(Vec<Point>, Instant)>,
    text_mode: Option<TextMode>,
    /// The action bar is shown although the selection searched by itself.
    more: bool,
    /// Where the user moved / sized the result card (this run of the app).
    card: Option<Rectangle>,
    /// The card's grip or corner is held, and the pointer's last position.
    card_drag: Option<(widgets::card::Drag, Option<Point>)>,
    /// The pointer is on the card's resize edge (its grip shows).
    card_hover: bool,
    /// Song search on this overlay, and its number.
    song: Option<(u32, song::Session)>,
    songs: u32,
    /// A verified update waiting until nothing is open (`is_idle`).
    update: Option<crate::update::Staged>,
}

/// Translations kept for reuse (each a short text).
const CACHE_SIZE: usize = 512;

impl App {
    fn boot() -> (Self, Task<Message>) {
        let can_hide = can_hide_windows();
        // Where windows can hide: created once, hidden, so a trigger only has to
        // show it (latency). Elsewhere `show` opens it on each use.
        let (overlay, open) = match can_hide {
            true => {
                let (id, open) = windows::screen(overlay_window(None));
                (id, open.discard())
            }
            false => (window::Id::unique(), Task::none()),
        };
        let mut app = App {
            cache: Arc::new(Cache::new(CACHE_SIZE)),
            can_hide,
            overlay_open: can_hide,
            ..App::idle(overlay)
        };
        let start = app.windows.start().map(Message::Windows);
        // Started by an update: it is confirmed (the old program goes), notes shown.
        let notes = match crate::update::confirm() {
            Some((version, notes, url)) => app
                .windows
                .update_card(update_card::Card::installed(&version, &notes, &url))
                .map(Message::Windows),
            None => Task::none(),
        };
        (app, Task::batch([open, start, notes]))
    }

    /// Nothing open yet (the overlay window `overlay` pre-created, hidden).
    fn idle(overlay: window::Id) -> App {
        App {
            overlay,
            overlay_open: true,
            can_hide: true,
            capturing: None,
            captures: 0,
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
            screen_text: None,
            text_press: None,
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
            // As before the setting existed; `open` reads the config.
            behaviour: SelectionConfig {
                style: SelectStyle::Shape,
                after: After::Wait,
                search_with: SearchWith::Lens,
            },
            morph: None,
            text_mode: None,
            more: false,
            card: None,
            card_drag: None,
            card_hover: false,
            song: None,
            songs: 0,
            update: None,
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
            Message::Trigger(Trigger::Update(event)) => self.on_update(event),
            Message::UpdateTick => self.try_update(),
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
            // Lens / Ask handed off to the browser (Windows): the shortcut brings
            // the overlay, with its panel, back to the front.
            Message::Trigger(Trigger::Capture(_))
                if self.frame.is_some() && crate::lens::take_handed_off() =>
            {
                self.show()
            }
            // The snip toast is only a pill: a new shot replaces it.
            Message::Trigger(trigger @ (Trigger::Capture(_) | Trigger::Quick(..)))
                if self.snip_toast() =>
            {
                let closing = self.close();
                closing.chain(self.update(Message::Trigger(trigger)))
            }
            Message::Trigger(Trigger::Capture(_) | Trigger::Quick(..))
                if self.frame.is_some() || self.capturing.is_some() =>
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
            // Cancelled or timed out meanwhile: the frame is dropped (the portal's
            // file is already deleted).
            Message::Captured(n, ..) if self.capturing != Some(n) => Task::none(),
            Message::Captured(_, at, shot) => {
                let opened = self.open(at, shot);
                Task::batch([opened, self.check_song_key()])
            }
            Message::SongKey => Task::none(), // remembered by `music::check_key`
            Message::SongStart => self.start_song(),
            Message::Song(id, msg) => self.on_song(id, msg),
            Message::CaptureTimedOut(n) if self.capturing == Some(n) => {
                eprintln!(
                    "capture failed: no screenshot after {} s; try again",
                    CAPTURE_TIMEOUT.as_secs()
                );
                self.capturing = None;
                self.purpose = Purpose::Search;
                Task::none()
            }
            Message::CaptureTimedOut(_) => Task::none(),
            Message::Escape if self.capturing.is_some() => {
                eprintln!("[opencircle] capture cancelled");
                self.capturing = None;
                self.purpose = Purpose::Search;
                Task::none()
            }
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
            Message::Tapped(what, read) => self.on_tapped(what, read),
            Message::SelectText => self.select_text(),
            Message::TextLines(area, read) => {
                self.on_text_lines(area, read);
                Task::none()
            }
            Message::More => {
                self.more = true;
                Task::none()
            }
            Message::Picked(act) => self.on_picked(act),
            Message::CardMoved(p) => {
                if let Some((how, last)) = self.card_drag {
                    if let Some(from) = last {
                        let at = self.card_at();
                        self.card = Some(widgets::card::drag(at, how, p - from, self.screen));
                    }
                    self.card_drag = Some((how, Some(p)));
                }
                Task::none()
            }
            Message::CardReleased => {
                self.card_drag = None;
                Task::none()
            }
            Message::AskInput(text) => {
                self.ask = text;
                Task::none()
            }
            // Before a selection it asks about the whole screen.
            Message::AskSubmit => {
                let question = std::mem::take(&mut self.ask);
                let crop =
                    (!question.trim().is_empty()).then(|| self.stacked().or_else(|| self.whole()));
                match crop.flatten() {
                    Some(crop) => self.open_panel(actions::Panel::Ask, crop, Some(question)),
                    None => Task::none(),
                }
            }
            Message::TranslateScreen => {
                self.others.clear();
                self.word = None;
                self.select(Rectangle::with_size(self.screen));
                self.start_translate()
            }
            Message::ScreenText(opening, read)
                if opening == self.opening && self.frame.is_some() =>
            {
                match read {
                    Ok(lines) => self.screen_text = Some(lines),
                    Err(e) => eprintln!("[opencircle] reading the screen's text failed: {e}"),
                }
                Task::none()
            }
            Message::ScreenText(..) => Task::none(), // closed or opened again meanwhile
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
                trim_heap();
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
    /// After `CAPTURE_TIMEOUT` the capture is given up (the next trigger works) and
    /// its answer, if it still comes, is dropped.
    fn capture(&mut self, at: Instant, snip: Option<snip::Kind>) -> Task<Message> {
        self.captures = self.captures.wrapping_add(1);
        let n = self.captures;
        self.capturing = Some(n);
        let timeout = background(|tx| {
            std::thread::sleep(CAPTURE_TIMEOUT);
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::CaptureTimedOut(n));
        let shot = background(move |tx| {
            let shot = capture::monitor_under_cursor().map(|mut frame| {
                let scale = frame.scale;
                let snip = snip.map(|kind| {
                    if config::current().snip.cursor {
                        capture::cursor::draw(&mut frame);
                    }
                    let foreground = capture::window_rect::foreground();
                    let list = capture::window_rect::list();
                    let windows = list.into_iter().map(|w| {
                        let rect = snip::to_logical(w.rect, (frame.x, frame.y), scale);
                        snip::Pick::window(rect, &w.title, w.handle)
                    });
                    (kind, windows.collect(), foreground)
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
        .map(move |shot| Message::Captured(n, at, shot));
        Task::batch([shot, timeout])
    }

    fn open(&mut self, at: Instant, shot: Result<Shot, String>) -> Task<Message> {
        self.capturing = None;
        let shot = match shot {
            Ok(shot) => shot,
            Err(e) => {
                eprintln!("capture failed: {e}");
                self.purpose = Purpose::Search;
                if !capture::refused(&e) {
                    return Task::none();
                }
                // Wayland: the desktop asks for permission only while we are in front.
                return self
                    .windows
                    .open(windows::Kind::Permission)
                    .map(Message::Windows);
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
        glass::set_frame(self.frame.as_ref(), shot.scale);
        self.helper = Some(ocr::hold());
        self.scale = shot.scale;
        self.screen = size;
        self.origin = shot.origin;
        self.opening = self.opening.wrapping_add(1);
        self.first_draw.set(Some((at, size)));
        self.hiding = Hiding::No;
        self.behaviour = config::current().selection;
        let mut first = None;
        let mut extras = Task::none();
        if let Some((kind, windows, foreground)) = shot.snip {
            let mut snip = Snip::new(windows, size, snip::can_scroll(), foreground);
            first = snip.start(kind, size); // Full = the whole screen at once
            let handles = snip.window_handles(UIA_WINDOWS);
            self.purpose = Purpose::Snip(Box::new(snip));
            let px = |v: f32| (v * shot.scale).round() as i32;
            let origin = (px(shot.origin.x), px(shot.origin.y));
            extras = self.snip_extras(handles, origin, shot.scale);
        }
        let Purpose::Quick(quick, target) = self.purpose else {
            let next = first.map_or_else(Task::none, |next| self.snip_next(Some(next)));
            if self.snip_pill() {
                return Task::batch([next, extras]); // the toast is the window
            }
            return Task::batch([next.chain(self.show()), extras, self.read_screen()]);
        };
        if let Quick::Song = quick {
            let song = self.run_quick(quick); // listens at once, no area
            return self.show().chain(song);
        }
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

    /// Shows the (hidden or pill-sized) window over the whole monitor, or opens
    /// it full-screen where it was closed (see `can_hide_windows`).
    fn show(&mut self) -> Task<Message> {
        if !self.overlay_open {
            let (id, open) = windows::screen(overlay_window(Some(self.screen)));
            eprintln!("[opencircle] overlay window opened");
            (self.overlay, self.overlay_open) = (id, true);
            return open.discard().chain(window::gain_focus(id));
        }
        // Place and size the window to cover the monitor BEFORE set_mode shows it:
        // iced makes it visible before going full-screen, so whatever is shown
        // first must already cover the monitor (no small black box flash).
        // ponytail: mixed-DPI multi-monitor placement untested (needs real hardware).
        let show = windows::uncut(self.overlay)
            .map(Message::Windows)
            .chain(window::move_to(self.overlay, self.origin))
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
            || self.snip_pill()
        {
            return Task::none();
        }
        // Zoom and pan move the view; pointer events go on in image px.
        let event = match event {
            overlay::Event::Zoom { at, notches } => {
                if self.can_zoom() {
                    self.view = self.view.zoomed(at, notches);
                } else if let Purpose::Snip(snip) = &mut self.purpose {
                    snip.walk(notches); // control -> window -> screen, and back
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
        // Snip: the loupe follows the pointer; a drag's ends snap to edges.
        let event = match (&mut self.purpose, event) {
            (Purpose::Snip(snip), overlay::Event::Press { at, shift }) => {
                snip.hover(at);
                overlay::Event::Press {
                    at: snip.snap(at),
                    shift,
                }
            }
            (Purpose::Snip(snip), overlay::Event::Move(at)) => {
                snip.hover(at);
                overlay::Event::Move(snip.snap(at))
            }
            (Purpose::Snip(snip), overlay::Event::Hover(at)) => {
                snip.hover(at);
                overlay::Event::Hover(at)
            }
            (_, event) => event,
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
            // Select-text mode: a press on the box picks words, a drag stretches them.
            overlay::Event::Press {
                at: p,
                shift: false,
            } if self.picks_text_at(p) => {
                let area = self.area();
                if let Some(mode) = &mut self.text_mode {
                    let lines = mode.lines.clone().unwrap_or_default();
                    mode.picked = word::Picked::new(lines, area, p);
                    mode.picking = mode.picked.is_some();
                }
            }
            overlay::Event::Move(p) if self.text_mode.as_ref().is_some_and(|m| m.picking) => {
                if let Some(picked) = self.text_mode.as_mut().and_then(|m| m.picked.as_mut()) {
                    picked.extend(p);
                }
            }
            overlay::Event::Release if self.text_mode.as_ref().is_some_and(|m| m.picking) => {
                if let Some(mode) = &mut self.text_mode {
                    mode.picking = false;
                }
            }
            overlay::Event::Press { at: p, shift } => {
                // A new selection: the panel and the edits were about the old one.
                self.panel = None;
                self.text_mode = None;
                self.morph = None;
                self.more = false;
                self.edit = None;
                self.scan = None;
                self.taps = self.taps.wrapping_add(1); // a tap still reading is stale
                if !shift && let Some(grab) = self.grab_at(p) {
                    if let (Some(start), Some(word)) = (grab.end, &mut self.word) {
                        word.hold(start); // a handle: that end follows the drag
                    }
                    self.grab = Some(grab);
                    return Task::none();
                }
                // On text (once the screen is read): the word there, and a drag
                // along the text selects words, like selecting text.
                if !shift && let Some(word) = self.text_at(p) {
                    self.others.clear();
                    self.word = Some(word);
                    self.select_word();
                    self.text_press = Some(vec![p]);
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
            overlay::Event::Move(p) if self.text_press.is_some() => self.drag_text(p),
            overlay::Event::Move(p) if self.grab.is_some() => self.drag_grab(p),
            overlay::Event::Move(p) if self.drawing => self.stroke.push(p),
            overlay::Event::Move(_) => {}
            overlay::Event::Hover(p) => {
                // Snip's Window mode: the window under the pointer lights up.
                if let Purpose::Snip(snip) = &self.purpose
                    && snip.kind == snip::Kind::Window
                {
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
            overlay::Event::Release if self.text_press.take().is_some() => return self.selected(),
            overlay::Event::Release if let Some(grab) = self.grab.take() => {
                return if grab.moved && grab.text {
                    self.extend_words(grab.to)
                } else if grab.moved {
                    // The search first: a waiting quick action (`selected`)
                    // turns the purpose back to Search, and must not get it too.
                    let search = self.auto_search();
                    Task::batch([self.selected(), search])
                } else {
                    self.tap(grab.from) // a click on the selection
                };
            }
            overlay::Event::Release => {
                self.drawing = false;
                let zoom = self.view.zoom();
                let mut settled = overlay::settle(&self.stroke, zoom, self.screen);
                // Snap: the stroke becomes a rounded box (it morphs there).
                if let Some((sel, kept)) = &mut settled
                    && self.snaps()
                {
                    if sel.kind != overlay::Kind::Drag {
                        self.morph = Some((kept.clone(), Instant::now()));
                    }
                    (*sel, *kept) = overlay::snap(sel.area);
                }
                self.selection = settled.as_ref().map(|(sel, _)| *sel);
                let drawn = match settled {
                    Some((_, kept)) => std::mem::replace(&mut self.stroke, kept),
                    None => self.stroke.clone(),
                };
                if let Purpose::Snip(snip) = &self.purpose {
                    let area = self.selection.map(|s| s.area);
                    let at = drawn.last().copied().unwrap_or(Point::ORIGIN);
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
                    // Shift+click on text with words picked: stretch them there.
                    if sel.kind == overlay::Kind::Word
                        && self.word.is_some()
                        && overlay::is_tap(&drawn, zoom)
                    {
                        return self.extend_words(drawn[0]);
                    }
                    return Task::none();
                }
                if self.selection.is_some() {
                    self.word = None;
                } else if overlay::is_tap(&drawn, zoom) {
                    return self.tap(drawn[0]);
                }
                // The search first: a waiting quick action (`selected`) turns
                // the purpose back to Search, and must not get it too.
                let search = match self.others.is_empty() {
                    true => self.auto_search(),
                    false => Task::none(), // Shift: more to come
                };
                return Task::batch([self.selected(), search]);
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

    /// From the update thread.
    fn on_update(&mut self, event: crate::update::Event) -> Task<Message> {
        use crate::update::Event;
        match event {
            Event::Status(status) => self
                .windows
                .update(windows::Msg::UpdateStatus(status))
                .map(Message::Windows),
            Event::Manual {
                version,
                notes,
                url,
            } => self
                .windows
                .update_card(update_card::Card::available(&version, &notes, &url))
                .map(Message::Windows),
            Event::Ready(staged) => {
                let toast = self
                    .windows
                    .toast(&update_card::ready_toast(&staged.version))
                    .map(Message::Windows);
                self.update = Some(staged);
                // the notice stays readable before this process exits to update
                let later = background(|tx| {
                    std::thread::sleep(Duration::from_secs(6));
                    let _ = tx.unbounded_send(());
                })
                .map(|()| Message::UpdateTick);
                Task::batch([toast, later])
            }
        }
    }

    /// Nothing of OpenCircle is in use: no overlay (editor, snip, panel), capture,
    /// live translate, Settings, onboarding or pinned image.
    fn is_idle(&self) -> bool {
        self.frame.is_none()
            && self.capturing.is_none()
            && self.live.is_none()
            && self.hiding == Hiding::No
            && !self.windows.busy()
    }

    /// Installs the waiting update now if idle (then this process exits), else
    /// looks again in a minute.
    fn try_update(&mut self) -> Task<Message> {
        if self.update.is_none() {
            return Task::none();
        }
        if !self.is_idle() {
            return background(|tx| {
                std::thread::sleep(Duration::from_secs(60));
                let _ = tx.unbounded_send(());
            })
            .map(|()| Message::UpdateTick);
        }
        let Some(staged) = self.update.take() else {
            return Task::none();
        };
        match crate::update::install(&staged) {
            Ok(()) => iced::exit(),
            Err(e) => {
                eprintln!("[opencircle] update {}: {e}", staged.version);
                // can't replace itself (e.g. installed for all users): the page instead
                self.windows
                    .update_card(update_card::Card::available(
                        &staged.version,
                        &staged.notes,
                        &staged.url,
                    ))
                    .map(Message::Windows)
            }
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
        let snapped = self
            .selection
            .is_some_and(|s| s.kind == overlay::Kind::Snap);
        matches!(self.purpose, Purpose::Search)
            && !self.drawing
            && (self.panel.is_none() || snapped)
            && self.text_mode.is_none()
            && self.edit.is_none()
            && self.translate.is_none()
    }

    /// A press at `p` (image px) on the selection: a corner handle resizes it,
    /// anywhere inside moves it.
    fn grab_at(&self, p: Point) -> Option<Grab> {
        let sel = self.selection.filter(|_| self.handles())?;
        let on_screen = self.view.rect_to_screen(sel.area);
        let grab = |corner, text, end| Grab {
            corner,
            from: p,
            start: (sel, self.stroke.clone()),
            moved: false,
            text,
            end,
            to: p,
        };
        // Words: their two handles move the range's ends.
        if sel.kind == overlay::Kind::Word {
            let stroke: Vec<Point> = self
                .stroke
                .iter()
                .map(|&q| self.view.to_screen(q))
                .collect();
            if let Some(start) = overlay::word_end_at(&stroke, self.view.to_screen(p)) {
                return Some(grab(None, true, Some(start)));
            }
        }
        let corner = overlay::grip_at(sel.kind, on_screen, self.view.to_screen(p));
        // On highlighted words a drag stretches them, it doesn't move the box.
        let text = corner.is_none() && self.word.as_ref().is_some_and(|w| w.on_text(p));
        (corner.is_some() || sel.area.contains(p)).then(|| grab(corner, text, None))
    }

    /// Resizes / moves the grabbed selection to the pointer at `p` (image px). A
    /// freehand loop scales with its box; anything else becomes that box.
    fn drag_grab(&mut self, p: Point) {
        let Some(grab) = &mut self.grab else { return };
        let zoom = self.view.zoom();
        grab.moved |= !overlay::is_tap(&[grab.from, p], zoom);
        grab.to = p;
        if !grab.moved {
            return;
        }
        if grab.text {
            // Words in the lines already read follow at once; a drag past them is
            // read when it is released (`extend_words`).
            if let Some(word) = &mut self.word
                && word.extend(p)
            {
                self.select_word();
            }
            return;
        }
        let (start, stroke) = &grab.start;
        let area = match grab.corner {
            Some(corner) => {
                // the corner follows the pointer from where it was grabbed (a bracket arm is off it)
                let to = overlay::corner(start.area, corner) + (p - grab.from);
                overlay::resize(start.area, corner, to, overlay::MIN_SIDE / zoom)
            }
            None => start.area + (p - grab.from),
        };
        let kind = match start.kind {
            overlay::Kind::Loop | overlay::Kind::Scribble | overlay::Kind::Snap => start.kind,
            overlay::Kind::Drag | overlay::Kind::Word => overlay::Kind::Drag,
        };
        self.morph = None;
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
        if let Some(word) = self.text_at(at) {
            self.word = Some(word);
            self.select_word();
            return self.selected();
        }
        if self.screen_text.is_some() {
            return Task::none(); // the screen is read: no text here
        }
        // A click on a selection keeps it unless a word is found there.
        self.read_words(word::region(at, self.screen), at, false)
    }

    /// The word at `p` (image px) in the screen's text, read at open (main mode only).
    fn text_at(&self, p: Point) -> Option<word::Picked> {
        let lines = self.screen_text.as_ref()?;
        let main = matches!(self.purpose, Purpose::Search) && self.translate.is_none();
        let all = Rectangle::with_size(self.screen);
        main.then(|| word::Picked::new(lines.clone(), all, p))?
    }

    /// A press that began on text moved to `p` (image px): the words stretch
    /// there, unless the stroke loops, which makes it a circle after all.
    fn drag_text(&mut self, p: Point) {
        let Some(stroke) = &mut self.text_press else {
            return;
        };
        stroke.push(p);
        if overlay::winds(stroke, self.view.zoom()) {
            self.stroke = self.text_press.take().unwrap_or_default();
            self.drawing = true;
            self.word = None;
            self.selection = None;
            return;
        }
        if let Some(word) = &mut self.word
            && word.extend(p)
        {
            self.select_word();
        }
    }

    /// Main mode: OCR reads the whole frame on a thread (through the warm
    /// helper) so that a press on text selects words at once. Until it answers,
    /// gestures behave as without it. Without the OCR models nothing is read.
    fn read_screen(&self) -> Task<Message> {
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.frame
        else {
            return Task::none();
        };
        if !matches!(self.purpose, Purpose::Search) || !text::models_present() {
            return Task::none();
        }
        let (width, height, pixels) = (*width, *height, pixels.clone());
        let (scale, opening) = (self.scale, self.opening);
        background(move |tx| {
            // ponytail: a fixed head start so the helper's start-up doesn't
            // compete with the first frame; wait for that frame if it still does.
            std::thread::sleep(SCREEN_READ_DELAY);
            let read = ocr::recognize_in_child(&pixels, width, height);
            let lines = read.map(|blocks| word::lines(blocks, [0, 0], scale));
            let _ = tx.unbounded_send(lines.map_err(|e| e.to_string()));
        })
        .map(move |read| Message::ScreenText(opening, read))
    }

    /// The whole frozen frame (Ask before a selection).
    fn whole(&self) -> Option<Crop> {
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.frame
        else {
            return None;
        };
        let all = Rectangle::with_size(self.screen);
        actions::crop(pixels, *width, *height, self.scale, all)
    }

    /// The picked words stretch to `at` (image px), where a drag ended or a Shift
    /// click landed. Past the lines already read, OCR reads a band across the
    /// screen first (on a thread, through the warm helper).
    fn extend_words(&mut self, at: Point) -> Task<Message> {
        let Some(word) = &mut self.word else {
            return Task::none();
        };
        if !word.covers(at) {
            let band = word.band(at, self.screen);
            return self.read_words(band, at, true);
        }
        if word.extend(at) {
            self.select_word();
        }
        self.selected()
    }

    /// Reads the text in `region` (image px) on a thread, for the tap or drag
    /// end at `at`; the answer comes back as `Tapped`. Without the OCR models
    /// nothing happens (it never asks to download them).
    fn read_words(&mut self, region: Rectangle, at: Point, extend: bool) -> Task<Message> {
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
        let (Some([x0, y0, ..]), Some(crop)) = (
            actions::crop_bounds(*width, *height, self.scale, region),
            actions::crop(pixels, *width, *height, self.scale, region),
        ) else {
            return Task::none();
        };
        self.taps = self.taps.wrapping_add(1);
        let what = WordRead {
            id: self.taps,
            at,
            origin: [x0, y0],
            region,
            extend,
        };
        background(move |tx| {
            let read = ocr::recognize_in_child(&crop.rgba, crop.width, crop.height);
            let _ = tx.unbounded_send(read.map_err(|e| e.to_string()));
        })
        .map(move |read| Message::Tapped(what, read))
    }

    /// OCR for tap or drag `what` is back: the word under the tap is the
    /// selection, or the picked words stretch to where the drag ended.
    fn on_tapped(&mut self, what: WordRead, read: Result<Vec<TextBlock>, String>) -> Task<Message> {
        if what.id != self.taps || self.frame.is_none() {
            return Task::none(); // pressed again meanwhile, or closed
        }
        let blocks = match read {
            Ok(blocks) => blocks,
            Err(e) => {
                eprintln!("[opencircle] reading the tapped word failed: {e}");
                return Task::none();
            }
        };
        let lines = word::lines(blocks, what.origin, self.scale);
        if what.extend {
            let Some(word) = &mut self.word else {
                return Task::none();
            };
            word.reread(lines, what.region);
            word.extend(what.at);
        } else {
            let Some(picked) = word::Picked::new(lines, what.region, what.at) else {
                return Task::none(); // no text there
            };
            self.word = Some(picked);
        }
        self.select_word();
        self.selected()
    }

    /// The picked words as the (only) selection: a rounded highlight per line
    /// (the stroke holds a top-left / bottom-right pair for each).
    fn select_word(&mut self) {
        self.others.clear();
        if let Some(word) = &self.word {
            self.selection = Some(overlay::Selection {
                area: word.area(),
                kind: overlay::Kind::Word,
            });
            self.stroke = (word.rects().iter())
                .flat_map(|r| [r.position(), r.position() + Vector::from(r.size())])
                .collect();
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

    /// A finished stroke snaps to a box (`[selection] style`; not in Snip).
    fn snaps(&self) -> bool {
        self.behaviour.style == SelectStyle::Snap
            && matches!(self.purpose, Purpose::Search | Purpose::Quick(..))
    }

    /// The selection's box (image px), or nothing.
    fn area(&self) -> Rectangle {
        self.selection.map(|s| s.area).unwrap_or_default()
    }

    /// What runs by itself once a selection is made.
    fn auto_actions(&self) -> &'static [&'static Action] {
        let word = self
            .selection
            .is_some_and(|s| s.kind == overlay::Kind::Word);
        if !matches!(self.purpose, Purpose::Search) || word || self.selection.is_none() {
            return &[];
        }
        auto_targets(self.behaviour)
    }

    /// Runs `auto_actions` (Lens and Ask AI: the Lens page opens in the browser
    /// and stays held whichever panel then shows).
    fn auto_search(&mut self) -> Task<Message> {
        let runs: Vec<_> = (self.auto_actions().iter()).map(|a| self.run(a)).collect();
        Task::batch(runs)
    }

    /// The "Select text" chip: OCR reads the box (the warm helper) and every
    /// word in it becomes pickable. Without the OCR models the Text panel asks
    /// before downloading them.
    fn select_text(&mut self) -> Task<Message> {
        if self.text_mode.take().is_some() {
            return Task::none(); // "Done"
        }
        if !text::models_present() {
            return self.run(&text::ACTION);
        }
        let area = self.area();
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.frame
        else {
            return Task::none();
        };
        let (Some([x0, y0, ..]), Some(crop)) = (
            actions::crop_bounds(*width, *height, self.scale, area),
            actions::crop(pixels, *width, *height, self.scale, area),
        ) else {
            return Task::none();
        };
        self.panel = None;
        self.text_mode = Some(TextMode {
            lines: None,
            boxes: Vec::new(),
            picked: None,
            picking: false,
        });
        let scale = self.scale;
        background(move |tx| {
            let read = ocr::recognize_in_child(&crop.rgba, crop.width, crop.height);
            let lines = read.map(|blocks| word::lines(blocks, [x0, y0], scale));
            let _ = tx.unbounded_send(lines.map_err(|e| e.to_string()));
        })
        .map(move |read| Message::TextLines(area, read))
    }

    /// OCR read the box `area` for select-text mode (dropped if the selection
    /// moved on or the mode was left).
    fn on_text_lines(&mut self, area: Rectangle, read: Result<Vec<word::Line>, String>) {
        let same = self.area() == area;
        let Some(mode) = self.text_mode.as_mut().filter(|_| same) else {
            return;
        };
        match read {
            Ok(lines) => {
                mode.boxes = word::boxes(&lines);
                mode.lines = Some(lines);
            }
            Err(e) => {
                eprintln!("[opencircle] select text failed: {e}");
                self.text_mode = None;
            }
        }
    }

    /// A press at `p` (image px) picks words: select-text mode is on, read, and
    /// `p` is in the box.
    fn picks_text_at(&self, p: Point) -> bool {
        self.text_mode.as_ref().is_some_and(|m| m.lines.is_some()) && self.area().contains(p)
    }

    /// The select-text popup: Copy closes with the words copied; Translate and
    /// Search make the words' box the selection and translate it in place / search it.
    fn on_picked(&mut self, act: PickAct) -> Task<Message> {
        let picked = (self.text_mode.as_ref())
            .and_then(|m| m.picked.clone())
            .or_else(|| self.word.clone());
        let Some(picked) = picked else {
            return Task::none();
        };
        if act == PickAct::Copy {
            let closing = self.close();
            if let Err(e) = copy::text(picked.text()) {
                eprintln!("copy text failed: {e}");
            }
            return closing;
        }
        // Words picked in a snapped box: their box becomes the selection (a
        // word selection already is it).
        if self.text_mode.take().is_some() {
            self.morph = None;
            (self.selection, self.stroke) = {
                let (sel, stroke) = overlay::snap(picked.area());
                (Some(sel), stroke)
            };
            self.remember_region();
        }
        match act {
            PickAct::Translate => self.start_translate(),
            PickAct::Ask => self.run(&actions::ask::ACTION),
            _ => self.run(&lens::ACTION),
        }
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
        // Before close: Copy's text read keeps the overlay's warm OCR helper.
        if let Err(e) = finish(crop) {
            eprintln!("{} failed: {e}", action.id);
        }
        self.close()
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
            Quick::Song => self.start_song(),
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
        let out = edit.update(msg);
        // Add capture: the area picked on the frozen frame joins the board.
        if let Some(area) = edit.take_picked()
            && let Some(image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            }) = &self.frame
            && let Some(crop) = actions::crop(pixels, *width, *height, self.scale, area)
        {
            edit.add(crop, self.screen);
        }
        match out {
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
            Some(Output::CopyWithText(crop, words)) => {
                // Before close: the text read keeps the overlay's warm OCR helper.
                if let Err(e) = copy::with_text(crop, words) {
                    eprintln!("copy failed: {e}");
                }
                self.close()
            }
            Some(Output::AddCapture) => Task::none(), // `edit` picks the area now
            Some(Output::Dragged) => self.close(),
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
        // The timer ran out and the overlay is gone (hidden or closed): capture
        // again in that mode, unless something else opened meanwhile.
        if let snip::Msg::Shoot(kind) = msg {
            if self.frame.is_some() || self.capturing.is_some() {
                return Task::none();
            }
            return self.capture(Instant::now(), Some(kind));
        }
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
                let next = match stitched {
                    Ok(crop) if crop.height > 0 => self.snip_captured(Some(crop), true),
                    Ok(_) => Task::none(),
                    Err(e) => {
                        eprintln!("scrolling capture failed: {e}");
                        Task::none()
                    }
                };
                if self.snip_pill() {
                    return next; // the toast keeps the window small
                }
                next.chain(self.show())
            }
            snip::Msg::Delay(secs) => {
                let Some(id) = snip.delay(secs) else {
                    return Task::none();
                };
                let foreground = snip.foreground;
                self.selection = None;
                self.stroke.clear();
                if !self.can_hide {
                    // Wayland: no pill (the window can't shrink or hide), so it
                    // closes, waits, and captures again.
                    let _ = snip.shoot(id);
                    let kind = snip.kind;
                    let wait = Duration::from_secs(secs.into()) + Duration::from_millis(300);
                    return self.close().chain(Self::shoot_after(wait, kind));
                }
                let ticks = background(move |tx| {
                    // The overlay is a pill now: the user's app gets the focus back.
                    std::thread::sleep(Duration::from_millis(150));
                    capture::window_rect::activate(foreground);
                    for left in (0..secs).rev() {
                        std::thread::sleep(Duration::from_secs(1));
                        if tx.unbounded_send(left).is_err() {
                            return;
                        }
                    }
                })
                .map(move |left| Message::Snip(snip::Msg::Tick(id, left)));
                self.pill_at_top().chain(ticks)
            }
            // Time's up: the overlay goes first (hidden or closed, as anything
            // closes), so the pill is in no capture.
            snip::Msg::Tick(id, left) => match snip.tick(id, left) {
                Some(true) if snip.shoot(id) => {
                    let kind = snip.kind;
                    let wait = Duration::from_millis(300);
                    self.close().chain(Self::shoot_after(wait, kind))
                }
                _ => Task::none(),
            },
            snip::Msg::Shoot(_) => Task::none(), // handled above
            snip::Msg::Cancel => self.close(),
            snip::Msg::Edit => {
                let Some(toast) = snip.take_toast() else {
                    return Task::none();
                };
                if toast.in_place {
                    let _ = self.open_editor();
                } else {
                    self.edit = Some(Editing::fitted(toast.crop, self.screen, self.scale));
                }
                self.show()
            }
            snip::Msg::ToastDone(id) if snip.toast_showing(id) => self.close(),
            snip::Msg::ToastDone(_) => Task::none(),
            snip::Msg::Controls(opening, controls) => {
                if opening == self.opening {
                    snip.add_controls(controls);
                }
                Task::none()
            }
            snip::Msg::Shift(held) => {
                snip.set_shift(held);
                Task::none()
            }
            snip::Msg::Recent(recent) => {
                snip.set_recent(recent);
                Task::none()
            }
            snip::Msg::OpenRecent(path) => background(move |tx| {
                let _ = tx.unbounded_send(snip::open_recent(&path));
            })
            .map(|read| Message::Snip(snip::Msg::Loaded(read))),
            snip::Msg::Loaded(Ok(crop)) => {
                self.selection = None;
                self.stroke.clear();
                self.snip_captured(Some(crop), false)
            }
            snip::Msg::Loaded(Err(e)) => {
                eprintln!("[opencircle] recent capture unreadable: {e}");
                Task::none()
            }
            snip::Msg::ClearRecent => {
                snip.set_recent(Vec::new());
                snip::clear_recent();
                Task::none()
            }
        }
    }

    /// The window is only Snip's pill (scrolling, the timer, the toast).
    fn snip_pill(&self) -> bool {
        matches!(&self.purpose, Purpose::Snip(s) if s.is_pill())
    }

    /// Snip's "Copied" toast is showing.
    fn snip_toast(&self) -> bool {
        matches!(&self.purpose, Purpose::Snip(s) if s.has_toast())
    }

    /// Shrinks the overlay to the pill at the top of the monitor (no focus taken).
    fn pill_at_top(&self) -> Task<Message> {
        let x = (self.screen.width - snip::PILL.width) / 2.0;
        self.pill_window(Point::new(x.max(0.0), ASK_MARGIN))
    }

    /// THE one way Snip's pills (toast, timer, scrolling progress) become a
    /// window: the overlay shrunk to `snip::PILL` at `at` (logical px on this
    /// monitor). Switch it to the shared floating-window helper when it lands.
    /// Wayland can't move, shrink or hide a window (`can_hide_windows`): the
    /// overlay stays full-screen there and `view` draws the pill over the frame.
    fn pill_window(&self, at: Point) -> Task<Message> {
        if !self.can_hide {
            return Task::none();
        }
        window::set_mode(self.overlay, Mode::Windowed)
            .chain(window::resize(self.overlay, snip::PILL))
            .chain(window::move_to(
                self.overlay,
                self.origin + Vector::new(at.x, at.y),
            ))
            // The window is exactly the pill: cut it to the pill's shape (no box).
            .chain(windows::cut(self.overlay, widgets::SURFACE_RADIUS).map(Message::Windows))
    }

    /// `Shoot(kind)` after `wait` (the overlay is gone by then).
    fn shoot_after(wait: Duration, kind: snip::Kind) -> Task<Message> {
        background(move |tx| {
            std::thread::sleep(wait);
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::Shoot(kind)))
    }

    /// Snip's slow extras, on threads: the Recent strip, and (Windows) the
    /// controls of the topmost windows (`handles`), for outlines and snapping.
    fn snip_extras(&self, handles: Vec<isize>, origin: (i32, i32), scale: f32) -> Task<Message> {
        let recent = background(|tx| {
            let _ = tx.unbounded_send(snip::load_recent());
        })
        .map(|recent| Message::Snip(snip::Msg::Recent(recent)));
        if !cfg!(windows) || handles.is_empty() {
            return recent;
        }
        let opening = self.opening;
        let controls = background(move |tx| {
            // Topmost first: the window under the pointer is usually ready first.
            for window in handles {
                let controls = (capture::uia::controls(window).into_iter())
                    .map(|(r, what)| {
                        snip::Pick::control(snip::to_logical(r, origin, scale), &what, window)
                    })
                    .collect();
                if tx.unbounded_send(controls).is_err() {
                    return;
                }
            }
        })
        .map(move |rects| Message::Snip(snip::Msg::Controls(opening, rects)));
        Task::batch([recent, controls])
    }

    /// A snip is taken: the selection on the frozen frame (`given: None`), or a
    /// stitched / recent capture. Kept in Recent (`remember`, if history is on),
    /// then copied (a toast offers Edit) and / or edited, as `snip.after` says.
    fn snip_captured(&mut self, given: Option<Crop>, remember: bool) -> Task<Message> {
        let in_place = given.is_none();
        let Some(crop) = given.or_else(|| self.crop()) else {
            return Task::none();
        };
        if remember {
            snip::remember(&crop);
        }
        let (copy, edit) = snip::route(config::current().snip.after);
        let copied = copy
            && match copy::ACTION.run {
                Run::Finish(copy) => copy(crop.clone())
                    .inspect_err(|e| eprintln!("copy failed: {e}"))
                    .is_ok(),
                _ => false,
            };
        if edit || !copied {
            // Copy failed: the editor, so the capture isn't lost.
            if in_place {
                return self.open_editor();
            }
            self.edit = Some(Editing::fitted(crop, self.screen, self.scale));
            return Task::none();
        }
        let Purpose::Snip(snip) = &mut self.purpose else {
            return Task::none();
        };
        let id = snip.toast(crop, in_place);
        let foreground = snip.foreground;
        let done = background(move |tx| {
            // Once the pill is up, the user's app (their AI chat) gets the focus back.
            std::thread::sleep(Duration::from_millis(150));
            capture::window_rect::activate(foreground);
            std::thread::sleep(Duration::from_secs(snip::TOAST_SECS));
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::ToastDone(id)));
        self.pill_at_top().chain(done)
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
                self.snip_captured(None, true)
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
        self.pill_window(at)
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
                // The search is in the browser now. Windows put the overlay
                // behind it (`lens::take_handed_off`); Linux can't (Wayland
                // has no z-order and can't hide a window), so the overlay,
                // its panel and pills would stay over the browser: close.
                if !cfg!(windows) {
                    return self.close();
                }
            } else {
                page.clone().release(); // the overlay closed meanwhile
            }
        }
        let Some((_, panel)) = self.panel.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            panel::Msg::Card(card::Event::Grab(how)) => self.card_drag = Some((how, None)),
            panel::Msg::Card(card::Event::Hover(on)) => self.card_hover = on,
            panel::Msg::Card(card::Event::Reset) => self.card = None,
            panel::Msg::Card(card::Event::Close) => self.panel = None,
            panel::Msg::Cancel => self.panel = None,
            // The user took the results to the browser: on Linux get out of
            // its way (see the Lens page above); Windows already went behind.
            panel::Msg::Sheet(crate::sheet::Event::Left) if !cfg!(windows) => return self.close(),
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
        let _ = crate::lens::take_handed_off();
        self.frame = None;
        glass::set_frame(None, 1.0);
        self.stroke = Vec::new();
        self.selection = None;
        self.others = Vec::new();
        self.adding = false;
        self.grab = None;
        self.pan = None;
        self.word = None;
        self.screen_text = None;
        self.text_press = None;
        self.ask = String::new();
        self.drawing = false;
        self.purpose = Purpose::Search;
        self.edit = None;
        self.toast = None;
        self.scan = None;
        self.translate = None;
        self.helper = None;
        self.morph = None;
        self.text_mode = None;
        self.more = false;
        self.song = None; // stops listening
        if self.can_hide {
            self.hiding = Hiding::Empty;
            return Task::none();
        }
        if !std::mem::take(&mut self.overlay_open) {
            return Task::none(); // never shown (Copy, Save)
        }
        eprintln!("[opencircle] overlay window closed");
        trim_heap();
        window::close(self.overlay)
    }

    /// Song search: listens on a thread; its pill, then its card, over the overlay.
    fn start_song(&mut self) -> Task<Message> {
        self.songs = self.songs.wrapping_add(1);
        let id = self.songs;
        // The song's card takes the result card's turn: one card at a time.
        self.panel = None;
        let (session, work) = song::Session::start();
        self.song = Some((id, session));
        work.map(move |msg| Message::Song(id, msg))
    }

    fn on_song(&mut self, id: u32, msg: song::Msg) -> Task<Message> {
        let Some((current, session)) = &mut self.song else {
            return Task::none();
        };
        if *current != id {
            return Task::none();
        }
        match session.update(msg) {
            song::Outcome::None => Task::none(),
            song::Outcome::Close => {
                self.song = None;
                Task::none()
            }
            song::Outcome::Retry => self.start_song(),
            // The browser comes to the front: the overlay closes so it isn't on top.
            song::Outcome::Open(url) => {
                let open = background(move |_: &UnboundedSender<()>| {
                    if let Err(e) = crate::lens::open_link(&url) {
                        eprintln!("[opencircle] can't open the link: {e}");
                    }
                })
                .discard();
                Task::batch([open, self.close()])
            }
        }
    }

    /// The music button shows only once a key is known to be saved.
    fn has_song_key(&self) -> bool {
        crate::music::key_known() == Some(true)
    }

    /// First open: ask the keyring (off the UI thread) whether a song key is saved.
    fn check_song_key(&self) -> Task<Message> {
        if crate::music::key_known().is_some() || cfg!(test) {
            return Task::none();
        }
        background(|tx| {
            let _ = tx.unbounded_send(crate::music::check_key());
        })
        .map(|_| Message::SongKey)
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
        // the monitor's origin in desktop px (the window to translate is found there)
        let at = |v: f32| (v * self.scale).round() as i32;
        let monitor = Some((at(self.origin.x), at(self.origin.y), self.scale));
        let (session, task) =
            translate::Session::start(frame.clone(), area, self.cache.clone(), models, monitor);
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
            translate::Msg::Card(card::Event::Grab(how)) => {
                self.card_drag = Some((how, None));
                Task::none()
            }
            translate::Msg::Card(card::Event::Hover(on)) => {
                self.card_hover = on;
                Task::none()
            }
            translate::Msg::Card(card::Event::Reset) => {
                self.card = None;
                Task::none()
            }
            // the word card's ×: back to the translation; a prompt's: done
            translate::Msg::Card(card::Event::Close) if session.has_card() => {
                session.close_card();
                Task::none()
            }
            translate::Msg::Card(card::Event::Close) | translate::Msg::Close => self.close(),
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
        if windows::is_floating(window) {
            return Some(windows::clear_theme());
        }
        let live = self.live.as_ref()?;
        [live.window(), live.controller()]
            .contains(&Some(window))
            .then(live_layer::theme)
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        glass::building_overlay(false);
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
        glass::building_overlay(true);
        let snip = match &self.purpose {
            Purpose::Snip(snip) => Some(snip),
            _ => None,
        };
        // Scrolling, the timer, the toast: the window is only a pill (Wayland:
        // the pill over the frame, see `pill_window`).
        if self.can_hide
            && let Some(pill) = snip.and_then(|s| s.pill_view())
        {
            return pill.map(Message::Snip);
        }
        let picked: Vec<Rectangle> = (self.text_mode.as_ref())
            .and_then(|m| m.picked.as_ref())
            .map_or_else(Vec::new, word::Picked::rects);
        let overlay = overlay::Overlay {
            stroke: &self.stroke,
            selection: self.selection,
            others: &self.others,
            view: self.view,
            handles: self.handles(),
            opening: self.opening,
            first_draw: &self.first_draw,
            hover: snip.is_some_and(|s| s.kind.picks()) && self.edit.is_none(),
            dim: self.translate.is_none() || self.selection.is_some(),
            live_drag: snip.is_some(),
            look: config::look(),
            morph: (self.morph.as_ref()).map(|(from, at)| (from.as_slice(), *at)),
            words: (self.text_mode.as_ref()).map_or(&[], |m| m.boxes.as_slice()),
            picked,
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
                    Some(_) => border::Mode::Snip,
                    None => border::Mode::Overlay,
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
        // Snip: the loupe beside the pointer (under the mode switch).
        if self.edit.is_none()
            && let Some(loupe) = snip.and_then(|s| s.guides(frame, self.scale, self.drawing))
        {
            layers = layers.push(loupe.map(Message::Snip));
        }
        let top = if let Some(pill) = snip.and_then(|s| s.pill_view()) {
            pill.map(Message::Snip) // Wayland only (above)
        } else if let Some(toast) = self.toast {
            container(widgets::pill(toast))
                .center_x(Fill)
                .padding(24)
                .into()
        } else if let Some((id, song)) = &self.song {
            let id = *id;
            song.view(self.screen)
                .map(move |msg| Message::Song(id, msg))
        } else if let Some((id, panel)) = &self.panel {
            let id = *id;
            panel
                .view(self.card_at(), self.screen, self.card_hover)
                .map(move |msg| Message::Panel(id, msg))
        } else if let Some((id, session)) = &self.translate {
            let id = *id;
            session
                .view(self.screen, self.card_at(), self.card_hover)
                .map(move |msg| Message::Translate(id, msg))
        } else if let Some(snip) = snip {
            snip.view().map(Message::Snip)
        } else if let Some(selection) = self.selection {
            match self.purpose {
                Purpose::Search if !self.drawing => self.selection_ui(selection),
                _ => space().into(),
            }
        } else {
            container(widgets::pill(HINT))
                .center_x(Fill)
                .padding(24)
                .into()
        };
        layers = layers.push(top);
        if self.bottom_bar_shown() {
            layers = layers.push(self.bottom_bar());
        }
        // Beside a panel (an auto-search's): the chips, and the bar after More.
        if let (Some(_), Some(selection), Purpose::Search, false) =
            (&self.panel, self.selection, &self.purpose, self.drawing)
        {
            layers = layers.push(self.selection_ui(selection));
        }
        // A black capture (DRM / protected window): say so, but keep Copy, Save and Text.
        if self.selection.is_some() && self.scan.as_ref().is_some_and(|s| s.protected) {
            let room = self.room();
            let pill = container(widgets::pill(PROTECTED))
                .center_x(room.width)
                .padding(24);
            layers = layers.push(iced::widget::pin(pill).position(room.position()));
        }
        layers.into()
    }

    /// Where the result card shows: where the user left it, else away from
    /// the selection (`widgets::card::place`).
    fn card_at(&self) -> Rectangle {
        let selection = self.selection.map(|s| self.view.rect_to_screen(s.area));
        widgets::card::place(self.screen, selection, self.card)
    }

    /// The part of the overlay a result card leaves free (all of it without
    /// one): the bar, chips, popup and pills go there.
    fn room(&self) -> Rectangle {
        card::free(self.screen, self.panel.as_ref().map(|_| self.card_at()))
    }

    /// What floats around `selection`: the chips under a snapped box ("Select
    /// text", "More" while the bar waits behind an auto-search), the
    /// select-text popup, the action bar and the ask bar (not beside a panel).
    fn selection_ui(&self, selection: overlay::Selection) -> Element<'_, Message> {
        let area = self.view.rect_to_screen(selection.area);
        let panel = self.panel.is_some();
        let word = selection.kind == overlay::Kind::Word;
        let waits = self.behaviour.after == After::Wait || word;
        let bar_shown = self.text_mode.is_none() && ((waits && !panel) || self.more);
        let mut chips: Vec<Element<'_, Message>> = Vec::new();
        if selection.kind == overlay::Kind::Snap {
            let label = match &self.text_mode {
                None => "Select text",
                Some(TextMode { lines: None, .. }) => "Reading text…",
                Some(_) => "Done",
            };
            chips.push(widgets::float_chip(label, Message::SelectText).into());
        }
        if !word && !waits && !self.more && self.text_mode.is_none() {
            chips.push(widgets::float_chip("More", Message::More).into());
        }
        let room = self.room();
        let mut layers = stack![];
        let mut below = area;
        if !chips.is_empty() {
            let n = chips.len() as f32;
            let size = Size::new(
                n * widgets::TEXT_BUTTON_WIDTH + (n - 1.0) * CHIP_GAP,
                widgets::CONTROL,
            );
            let at = chips_at(area, size, room);
            below = area.union(&Rectangle::new(at, size)); // the bar goes past them
            let row = iced::widget::row(chips).spacing(CHIP_GAP);
            layers = layers.push(iced::widget::pin(opaque(row)).position(at));
        }
        // Picked words (select-text mode, or a word selection once let go):
        // the text actions instead of the bar.
        let picked = match &self.text_mode {
            Some(mode) => mode.picked.as_ref().filter(|_| !mode.picking),
            None => (self.word.as_ref())
                .filter(|_| word && self.text_press.is_none() && self.grab.is_none()),
        };
        // The bar and the popup keep clear of the card and of the bottom bar.
        let mut room = room;
        if self.bottom_bar_shown() {
            room.height = room.height.min(self.screen.height - ASK_ZONE);
        }
        if let Some(picked) = picked {
            layers = layers.push(pick_popup(self.view.rect_to_screen(picked.area()), room));
        } else if bar_shown && !word {
            let chip = (self.scan.as_ref())
                .and_then(|s| s.qr.as_deref())
                .map(|text| {
                    let open = qr::web_url(text).map(|_| Message::Qr(qr::Act::Open));
                    bar::qr_chip(text, Message::Qr(qr::Act::Copy), open)
                });
            layers = layers.push(bar::view(below, room, chip, Message::Run));
        }
        layers.into()
    }

    /// The bottom bar shows in main mode from the first frame, through drawing
    /// and selecting; a result card, a translation, the song card or a toast
    /// take its place.
    fn bottom_bar_shown(&self) -> bool {
        matches!(self.purpose, Purpose::Search)
            && self.panel.is_none()
            && self.translate.is_none()
            && self.song.is_none()
            && self.toast.is_none()
    }

    /// The bottom bar: "Ask about your screen…" (your selection, once there is
    /// one; Enter opens Ask AI with it), Translate the screen, and the music
    /// button once a song key is saved. Its field takes the keys only once
    /// clicked: until then letters are the action shortcuts.
    fn bottom_bar(&self) -> Element<'_, Message> {
        let placeholder = match self.selection {
            Some(_) => "Ask about your selection…",
            None => "Ask about your screen…",
        };
        let field = widgets::input(
            ASK_INPUT,
            placeholder,
            &self.ask,
            Message::AskInput,
            Message::AskSubmit,
        );
        let translate = widgets::hint(
            widgets::icon_button(
                actions::translate::ACTION.icon,
                false,
                Message::TranslateScreen,
            ),
            "Translate the screen",
            widgets::Why::Icon,
            iced::widget::tooltip::Position::Top,
        );
        let mut bar = iced::widget::row![field.width(ASK_WIDTH), translate]
            .spacing(4)
            .align_y(iced::Center);
        if let Some(music) = song::button(self.has_song_key(), Message::SongStart) {
            bar = bar.push(music);
        }
        // opaque: a press on it doesn't start a new selection.
        let bar = opaque(widgets::surface(bar));
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
        // Also when a text field took the key (it only unfocuses on Esc).
        let escape = || {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                }) => Some(Message::Escape),
                _ => None,
            })
        };
        // Esc in any of our windows cancels a pending capture.
        let triggers = match self.capturing {
            Some(_) => Subscription::batch([triggers, escape()]),
            None => triggers,
        };
        if self.frame.is_some() {
            let escape = escape();
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
            // Dragging the result card: the pointer anywhere, until let go.
            let card = match self.card_drag {
                Some(_) => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::CardMoved(position))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::CardReleased),
                    _ => None,
                }),
                None => Subscription::none(),
            };
            // Snip: Shift held = no snapping.
            let shift = match self.purpose {
                Purpose::Snip(_) => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        Some(Message::Snip(snip::Msg::Shift(m.shift())))
                    }
                    _ => None,
                }),
                _ => Subscription::none(),
            };
            Subscription::batch([triggers, escape, alt, card, shift])
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

/// Where the chips (`size`) go: centred under the box `area`, `CHIP_SPACE`
/// below it, else above it; inside `room` (logical px), never on the box
/// unless it fills the screen.
fn chips_at(area: Rectangle, size: Size, room: Rectangle) -> Point {
    let x = (area.center_x() - size.width / 2.0)
        .min(room.x + room.width - bar::GAP - size.width)
        .max(room.x + bar::GAP);
    let (top, bottom) = (room.y + bar::GAP, room.y + room.height - bar::GAP);
    let below = area.y + area.height + CHIP_SPACE;
    let above = area.y - CHIP_SPACE - size.height;
    let y = if below + size.height <= bottom {
        below
    } else if above >= top {
        above
    } else {
        bottom - size.height // the box fills the screen: on it, at the bottom
    };
    Point::new(x, y)
}

/// The text actions for the picked words (Copy, Translate, Search, Ask AI), by their box `picked`.
fn pick_popup<'a>(picked: Rectangle, room: Rectangle) -> Element<'a, Message> {
    let buttons = [
        (copy::ACTION.icon, "Copy", PickAct::Copy),
        (
            actions::translate::ACTION.icon,
            "Translate",
            PickAct::Translate,
        ),
        (lens::ACTION.icon, "Search", PickAct::Search),
        (actions::ask::ACTION.icon, "Ask AI", PickAct::Ask),
    ]
    .map(|(icon, label, act)| {
        widgets::text_button(icon, label, String::new(), false, Message::Picked(act)).into()
    });
    let n = buttons.len() as f32;
    let size = Size::new(
        n * widgets::TEXT_BUTTON_WIDTH + (n - 1.0) * 4.0 + 2.0 * widgets::SURFACE_PADDING,
        widgets::CONTROL + 2.0 * widgets::SURFACE_PADDING,
    );
    // above the words (the text below stays readable), else under them
    let mut at = bar::place_in(picked, size, room);
    let above = picked.y - bar::GAP - size.height;
    if above >= room.y + bar::GAP {
        at.y = above;
    }
    let popup = widgets::surface(iced::widget::row(buttons).spacing(4));
    iced::widget::pin(opaque(popup)).position(at).into()
}

/// The actions a selection runs by itself under `behaviour`.
fn auto_targets(behaviour: SelectionConfig) -> &'static [&'static Action] {
    match (behaviour.after, behaviour.search_with) {
        (After::Wait, _) => &[],
        (After::Search, SearchWith::Lens) => &[&lens::ACTION],
        (After::Search, SearchWith::Ai) => &[&actions::ask::ACTION],
        (After::Search, SearchWith::Both) => &[&lens::ACTION, &actions::ask::ACTION],
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

    fn a_shot() -> Result<Shot, String> {
        Ok(Shot {
            image: image::Handle::from_rgba(4, 4, vec![9; 4 * 4 * 4]),
            scale: 1.0,
            origin: Point::ORIGIN,
            size: Size::new(4.0, 4.0),
            snip: None,
        })
    }

    #[test]
    fn an_update_waits_until_nothing_is_open() {
        use crate::update::{Event, Install, Staged};
        let staged = Staged {
            version: "9.9.9".into(),
            file: "missing".into(),
            sha256: [0; 32],
            dir: std::env::temp_dir().join("oc-update-test-never-made"),
            install: Install::Manual, // even if it ran, it could not touch anything
            notes: String::new(),
            url: String::new(),
        };
        let mut app = App::idle(window::Id::unique());
        assert!(app.is_idle());
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        assert!(!app.is_idle(), "a capture is out");
        let _ = app.update(Message::Trigger(Trigger::Update(Event::Ready(
            staged.clone(),
        ))));
        assert_eq!(app.update, Some(staged.clone()), "kept for later");
        let _ = app.update(Message::Captured(
            app.capturing.unwrap(),
            Instant::now(),
            a_shot(),
        ));
        assert!(!app.is_idle(), "the overlay is open");
        let _ = app.update(Message::UpdateTick);
        assert_eq!(app.update, Some(staged), "still waiting");
        let _ = app.close();
        app.hiding = Hiding::No; // the hidden window drew its last frames
        assert!(app.is_idle());
        let _ = app.update(Message::UpdateTick);
        assert_eq!(app.update, None, "tried once idle");
    }

    #[test]
    fn a_timed_out_capture_frees_the_trigger_and_its_late_answer_is_dropped() {
        let mut app = App::idle(window::Id::unique());
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let first = app.capturing.expect("pending");
        // A second press while it is out does nothing.
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        assert_eq!(app.capturing, Some(first));
        let _ = app.update(Message::CaptureTimedOut(first));
        assert_eq!(app.capturing, None, "the next trigger may start");
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let second = app.capturing.expect("a new capture started");
        assert_ne!(first, second);
        // The first capture's answer arrives late: ignored, the new one still waits.
        let _ = app.update(Message::Captured(first, Instant::now(), a_shot()));
        assert!(app.frame.is_none());
        assert_eq!(app.capturing, Some(second));
        // A stale timeout doesn't touch the new capture.
        let _ = app.update(Message::CaptureTimedOut(first));
        assert_eq!(app.capturing, Some(second));
        let _ = app.update(Message::Captured(second, Instant::now(), a_shot()));
        assert!(app.frame.is_some(), "the live one opens");
        assert_eq!(app.capturing, None);
    }

    #[test]
    fn esc_cancels_a_pending_capture() {
        let mut app = App::idle(window::Id::unique());
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let n = app.capturing.expect("pending");
        let _ = app.update(Message::Escape);
        assert_eq!(app.capturing, None);
        let _ = app.update(Message::Captured(n, Instant::now(), a_shot()));
        assert!(app.frame.is_none(), "the overlay doesn't open");
        assert_eq!(app.hiding, Hiding::No);
    }

    #[test]
    fn windows_hide_except_where_winit_runs_on_wayland() {
        use std::ffi::OsStr;
        let set = Some(OsStr::new("wayland-0"));
        let empty = Some(OsStr::new(""));
        assert!(winit_picks_wayland(set, None));
        assert!(winit_picks_wayland(None, set));
        assert!(winit_picks_wayland(empty, set));
        assert!(!winit_picks_wayland(None, None));
        assert!(!winit_picks_wayland(empty, empty));
    }

    /// Trigger, capture, Esc: the overlay's window id while shown, and after.
    fn open_and_escape(app: &mut App) -> (window::Id, window::Id) {
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let n = app.capturing.expect("pending");
        let _ = app.update(Message::Captured(n, Instant::now(), a_shot()));
        assert!(app.frame.is_some() && app.overlay_open);
        let shown = app.overlay;
        let _ = app.update(Message::Escape);
        assert!(app.frame.is_none());
        (shown, app.overlay)
    }

    #[test]
    fn where_windows_cannot_hide_each_opening_has_its_own_window_closed_after() {
        let mut app = App {
            can_hide: false,
            overlay_open: false,
            ..App::idle(window::Id::unique())
        };
        let mut ids = Vec::new();
        for _ in 0..3 {
            let (shown, _) = open_and_escape(&mut app);
            assert!(!app.overlay_open, "closed, not hidden");
            assert_eq!(app.hiding, Hiding::No, "no hide steps on a closed window");
            assert!(!ids.contains(&shown), "a fresh window each time");
            ids.push(shown);
        }
        // closing again (or a Copy that never showed it) closes nothing
        let _ = app.close();
        assert!(!app.overlay_open);
        assert_eq!(app.hiding, Hiding::No);
    }

    #[test]
    fn where_windows_can_hide_the_one_pre_created_window_is_reused() {
        let first = window::Id::unique();
        let mut app = App::idle(first);
        for _ in 0..3 {
            let (shown, after) = open_and_escape(&mut app);
            assert_eq!((shown, after), (first, first));
            assert!(app.overlay_open, "kept, to be hidden");
            assert_eq!(app.hiding, Hiding::Empty);
            app.hiding = Hiding::No; // (the hide steps run on frames)
        }
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
        let (session, _) = translate::Session::start(
            frame.clone(),
            [0, 0, 40, 20],
            app.cache.clone(),
            false,
            None,
        );
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
        let read = |id| WordRead {
            id,
            at,
            origin: [100, 40],
            region: Rectangle::new(Point::new(50.0, 20.0), Size::new(100.0, 80.0)),
            extend: false,
        };
        let stale = app.on_tapped(read(6), Ok(vec![block.clone()]));
        assert!(stale.units() == 0 && app.selection.is_none(), "an old tap");
        let scan = app.on_tapped(read(7), Ok(vec![block]));
        assert_eq!(scan.units(), 0, "a word is not scanned for a QR code");
        let sel = app.selection.expect("the word");
        assert_eq!(sel.kind, overlay::Kind::Word);
        assert_eq!(app.word.as_ref().unwrap().text(), "any");
        // a second tap on the line (no OCR: the lines are kept) makes a range
        let key = app.view.to_screen(Point::new(175.0, 40.0));
        drag(&mut app, key, key, false);
        assert_eq!(app.word.as_ref().unwrap().text(), "any key");
        assert!(app.selection.unwrap().area.width > sel.area.width);
        // its corner is the end handle: the range's end follows, still the words
        let r = app.view.rect_to_screen(app.selection.unwrap().area);
        let corner = Point::new(r.x + r.width, r.y + r.height);
        let press = app.view.to_screen(Point::new(105.0, 40.0));
        drag(&mut app, corner, press, false);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Word);
        assert_eq!(app.word.as_ref().unwrap().text(), "Press any");
    }

    const TEXT: [&str; 6] = [
        "Circle to Search keeps the page",
        "frozen while you drag a finger",
        "across the words you want to",
        "copy, translate or look up.",
        "Press Shift to add a second",
        "selection anywhere on screen.",
    ];
    /// Monospace, 22 px: about this many px per character.
    const CHAR: f32 = 13.2;

    /// What OCR would read of `TEXT` drawn by `text_frame`, as one block per line.
    fn text_blocks() -> Vec<TextBlock> {
        (TEXT.iter().enumerate())
            .map(|(i, t)| TextBlock {
                text: (*t).into(),
                bounds: [60.0, 43.0 + 32.0 * i as f32, t.len() as f32 * CHAR, 26.0],
                confidence: 1.0,
            })
            .collect()
    }

    /// A word on line `i`: image px at its middle (`c` = the character it starts at).
    fn at_word(i: usize, c: f32) -> Point {
        Point::new(60.0 + (c + 2.0) * CHAR, 43.0 + 32.0 * i as f32 + 13.0)
    }

    /// An overlay on a 960x420 frame with `TEXT` drawn on it.
    fn text_app() -> App {
        use iced::widget::{column, text};
        let lines = TEXT.map(|t| -> Element<'_, Message> {
            let line = text(t).size(22).font(iced::Font::MONOSPACE);
            line.line_height(iced::Pixels(32.0)).into()
        });
        let page = container(column(lines)).padding([40, 60]);
        let rgba = widgets::shot::render(Element::from(page), 960, 420);
        let mut app = open_app(960, 420, 1.0);
        app.frame = Some(image::Handle::from_rgba(960, 420, rgba));
        app
    }

    /// OCR read `blocks` for the read `extend`/`at`, over `region`, for the latest press.
    fn read_back(app: &mut App, at: Point, region: Rectangle, extend: bool) {
        let what = WordRead {
            id: app.taps,
            at,
            origin: [0, 0],
            region,
            extend,
        };
        // OCR reads the lines that lie in the region it was given
        let inside = |b: &TextBlock| {
            region.y <= b.bounds[1] && b.bounds[1] + b.bounds[3] <= region.y + region.height
        };
        let blocks = text_blocks().into_iter().filter(inside).collect();
        let _ = app.on_tapped(what, Ok(blocks));
    }

    fn page() -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(960.0, 420.0))
    }

    #[test]
    fn dragging_from_the_highlight_selects_across_lines() {
        let mut app = text_app();
        let start = at_word(1, 7.0); // "while"
        app.taps += 1;
        read_back(&mut app, start, page(), false);
        assert_eq!(app.word.as_ref().unwrap().text(), "while");
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Word);

        // press on the highlighted word and drag down to line 3
        drag(&mut app, start, at_word(3, 6.0), false);
        let word = app.word.as_ref().unwrap();
        assert_eq!(
            word.text(),
            "while you drag a finger\nacross the words you want to\ncopy, translate"
        );
        let sel = app.selection.unwrap();
        assert_eq!(sel.kind, overlay::Kind::Word, "not turned into a moved box");
        assert_eq!(sel.area, word.area());
        assert_eq!(
            app.stroke.len(),
            6,
            "a (top-left, bottom-right) pair per line"
        );
        assert!(app.others.is_empty() && app.grab.is_none());

        // dragging back up shrinks it; the anchor stays where the press was
        drag(&mut app, at_word(1, 7.0), at_word(0, 9.0), false);
        assert_eq!(
            app.word.as_ref().unwrap().text(),
            "Search keeps the page\nfrozen while"
        );
        assert_eq!(app.stroke.len(), 4);

        // a plain tap on the highlight is still the same-line range
        let mut app = text_app();
        app.taps += 1;
        read_back(&mut app, start, page(), false);
        drag(&mut app, start, start, false);
        assert_eq!(app.word.as_ref().unwrap().text(), "while");
    }

    #[test]
    fn a_drag_past_the_lines_read_reads_a_band_first() {
        let mut app = text_app();
        let start = at_word(1, 7.0);
        let first = Rectangle::new(Point::new(0.0, 60.0), Size::new(960.0, 80.0)); // lines 1-2
        app.taps += 1;
        read_back(&mut app, start, first, false);
        let end = at_word(5, 10.0); // "anywhere": not read yet
        drag(&mut app, start, end, false);
        let word = app.word.as_ref().unwrap();
        assert!(word.text().starts_with("while you drag a finger\nacross"));
        assert_eq!(
            word.text().lines().count(),
            2,
            "as far as the lines read go"
        );
        assert!(!word.covers(end), "the end is past what was read");
        // the band OCR reads for it: whole width, every line the drag crosses
        let band = word.band(end, app.screen);
        assert!(band.width == 960.0 && band.y <= 60.0 && band.y + band.height >= end.y);
        // its answer (not stale: nothing pressed since) stretches the words there
        read_back(&mut app, end, band, true);
        let word = app.word.as_ref().unwrap();
        assert_eq!(word.text().lines().count(), 5);
        assert!(word.text().starts_with("while you drag"));
        assert!(word.text().ends_with("selection anywhere"));
        assert_eq!(app.stroke.len(), 10);
        // a read for an earlier press is dropped
        let before = word.text();
        app.taps += 1;
        let stale = WordRead {
            id: app.taps - 1,
            at: at_word(0, 1.0),
            origin: [0, 0],
            region: band,
            extend: true,
        };
        let _ = app.on_tapped(stale, Ok(text_blocks()));
        assert_eq!(app.word.as_ref().unwrap().text(), before);
    }

    #[test]
    fn shift_click_stretches_the_words_across_lines() {
        let mut app = text_app();
        app.taps += 1;
        read_back(&mut app, at_word(1, 7.0), page(), false);
        let end = at_word(4, 14.0); // "add"
        drag(&mut app, end, end, true);
        assert_eq!(
            app.word.as_ref().unwrap().text(),
            "while you drag a finger\nacross the words you want to\ncopy, translate or look up.\nPress Shift to add"
        );
        assert!(app.others.is_empty(), "stretched, not a second selection");
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Word);
        // Shift+click elsewhere on the first line picks backwards in reading order
        drag(&mut app, at_word(0, 9.0), at_word(0, 9.0), true);
        let text = app.word.as_ref().unwrap().text();
        assert_eq!(text, "Search keeps the page\nfrozen while");
        // a Shift drag still adds a second selection
        circle(&mut app, Point::new(700.0, 300.0), 40.0, true);
        assert_eq!(app.others.len(), 1);
        assert!(app.word.is_none());
    }

    #[test]
    fn a_scribble_selects_a_padded_hull_that_resizes_as_one() {
        let mut app = open_app(960, 420, 1.0);
        let zig: Vec<Point> = (0..=100)
            .map(|i| {
                let t = i as f32 / 100.0;
                let leg = (t * 4.0).floor();
                let x = if leg % 2.0 == 0.0 {
                    (t * 4.0).fract()
                } else {
                    1.0 - (t * 4.0).fract()
                };
                Point::new(300.0 + 200.0 * x, 150.0 + 80.0 * t)
            })
            .collect();
        event(
            &mut app,
            overlay::Event::Press {
                at: zig[0],
                shift: false,
            },
        );
        zig[1..]
            .iter()
            .for_each(|&p| event(&mut app, overlay::Event::Move(p)));
        event(&mut app, overlay::Event::Release);
        let sel = app.selection.expect("a scribble selects");
        assert_eq!(sel.kind, overlay::Kind::Scribble);
        assert!(app.word.is_none() && app.handles());
        // the kept stroke is the hull and the area its box, grown past the scribble's own box
        assert!(app.stroke.len() > 8);
        assert!(sel.area.x < 300.0 && sel.area.y < 150.0);
        assert!(sel.area.x + sel.area.width > 500.0 && sel.area.y + sel.area.height > 230.0);
        assert!(app.selected().units() > 0, "scanned like a drawn area");
        // the crop is that box
        let crop = app.crop().unwrap();
        assert!((crop.width as f32 - sel.area.width).abs() <= 2.0);
        // a corner drag resizes it and keeps the shape
        let corner = Point::new(sel.area.x + sel.area.width, sel.area.y + sel.area.height);
        drag(&mut app, corner, corner + Vector::new(30.0, 20.0), false);
        let grown = app.selection.unwrap();
        assert_eq!(grown.kind, overlay::Kind::Scribble);
        assert!((grown.area.width - sel.area.width - 30.0).abs() < 0.5);
        let xs = app.stroke.iter().map(|p| p.x);
        assert!((xs.fold(f32::MIN, f32::max) - (grown.area.x + grown.area.width)).abs() < 0.01);
        // a loop or a drag next to it still work as before
        circle(&mut app, Point::new(200.0, 300.0), 40.0, false);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Loop);
        drag(
            &mut app,
            Point::new(600.0, 50.0),
            Point::new(800.0, 120.0),
            false,
        );
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Drag);
    }

    /// `OPENCIRCLE_SELECTION_PNG=<dir> cargo test selection_shots -- --ignored`
    /// writes the overlay as drawn (headless tiny-skia, 960x420): words dragged
    /// across three lines, a scribble over the text, and the same two in the
    /// idle state for comparison.
    #[test]
    #[ignore = "writes PNGs to look at"]
    fn selection_shots() {
        let dir = std::path::PathBuf::from(
            std::env::var("OPENCIRCLE_SELECTION_PNG").expect("OPENCIRCLE_SELECTION_PNG=<dir>"),
        );
        let shot = |app: &App, name: &str| {
            let rgba = widgets::shot::render(app.view(app.overlay), 960, 420);
            actions::write_png(&dir.join(name), 960, 420, &rgba).unwrap();
        };
        let mut app = text_app();
        shot(&app, "0-page.png");
        let start = at_word(1, 7.0);
        app.taps += 1;
        read_back(&mut app, start, page(), false);
        shot(&app, "1-word.png");
        drag(&mut app, start, at_word(3, 6.0), false);
        shot(&app, "2-highlight-across-lines.png");

        let mut app = text_app();
        let wave: Vec<Point> = (0..=140)
            .map(|i| {
                let t = i as f32 / 140.0;
                let x = ((t * 5.0).fract() - 0.5).abs() * 2.0; // a triangle wave
                Point::new(150.0 + 460.0 * x, 100.0 + 70.0 * t + 6.0 * (i % 3) as f32)
            })
            .collect();
        event(
            &mut app,
            overlay::Event::Press {
                at: wave[0],
                shift: false,
            },
        );
        wave[1..]
            .iter()
            .for_each(|&p| event(&mut app, overlay::Event::Move(p)));
        shot(&app, "3-scribble-drawing.png");
        event(&mut app, overlay::Event::Release);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Scribble);
        shot(&app, "4-scribble-selected.png");
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

    /// `text_app` with its screen read (as the read at open answers).
    fn read_app() -> App {
        let mut app = text_app();
        let lines = word::lines(text_blocks(), [0, 0], 1.0);
        let _ = app.update(Message::ScreenText(app.opening, Ok(lines)));
        assert!(app.screen_text.is_some());
        app
    }

    /// A press-drag-release through `points` (image px = screen px at 1x).
    fn stroke(app: &mut App, points: &[Point]) {
        let at = points[0];
        event(app, overlay::Event::Press { at, shift: false });
        for pair in points.windows(2) {
            for i in 1..=8 {
                let p = pair[0] + (pair[1] - pair[0]) * (i as f32 / 8.0);
                event(app, overlay::Event::Move(p));
            }
        }
        event(app, overlay::Event::Release);
    }

    fn picked_text(app: &App) -> Option<String> {
        let word = app.selection.filter(|s| s.kind == overlay::Kind::Word);
        word.and(app.word.as_ref()).map(word::Picked::text)
    }

    #[test]
    fn with_the_screen_read_a_press_on_text_selects_words() {
        // a drag along a line selects its words, highlighted, not a stroke
        let mut app = read_app();
        stroke(&mut app, &[at_word(1, 7.0), at_word(1, 19.0)]);
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
        assert!(app.text_press.is_none() && !app.drawing);
        // to a line's end and back to the start of the next ones: reading order
        let end_of_1 = Point::new(60.0 + 30.0 * CHAR, at_word(1, 0.0).y);
        stroke(
            &mut app,
            &[at_word(1, 7.0), end_of_1, at_word(3, 0.0), at_word(3, 6.0)],
        );
        assert_eq!(
            picked_text(&app).as_deref(),
            Some("while you drag a finger\nacross the words you want to\ncopy, translate")
        );
        // a tap on a word selects that word
        let mut app = read_app();
        stroke(&mut app, &[at_word(4, 6.0), at_word(4, 6.0)]);
        assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
        // a loop that starts on text is a circle
        let mut app = read_app();
        let start = at_word(2, 11.0);
        let around: Vec<Point> = (0..=40)
            .map(|i| {
                let a = i as f32 / 40.0 * std::f32::consts::TAU + std::f32::consts::PI;
                start + Vector::new(80.0 + 80.0 * a.cos(), 60.0 * a.sin())
            })
            .collect();
        stroke(&mut app, &around);
        let sel = app.selection.expect("a circle");
        assert_eq!(sel.kind, overlay::Kind::Loop);
        assert!(app.word.is_none());
        // a press off the text draws as before
        let mut app = read_app();
        stroke(
            &mut app,
            &[Point::new(600.0, 250.0), Point::new(800.0, 400.0)],
        );
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Drag);
        // before the screen is read, a drag along text is a stroke too
        let mut app = text_app();
        stroke(&mut app, &[at_word(1, 7.0), at_word(1, 19.0)]);
        assert!(picked_text(&app).is_none());
    }

    #[test]
    fn a_text_selection_has_handles_and_the_text_actions() {
        let mut app = read_app();
        stroke(&mut app, &[at_word(1, 7.0), at_word(1, 19.0)]); // "while you drag"
        let texts = overlay_texts(&app);
        for label in ["Copy", "Translate", "Search", "Ask AI"] {
            assert!(texts.contains(&label.to_string()), "{label}: {texts:?}");
        }
        assert!(!texts.contains(&"Edit".to_string()), "not the action bar");
        // the end handle (right of "drag") stretches the range to the next line
        let end = *app.stroke.last().unwrap();
        stroke(&mut app, &[end, at_word(2, 11.0)]);
        assert_eq!(
            picked_text(&app).as_deref(),
            Some("while you drag a finger\nacross the words")
        );
        // the start handle (left of "while") moves the start back
        let start = app.stroke[0];
        stroke(&mut app, &[start, at_word(1, 0.0)]);
        assert_eq!(
            picked_text(&app).as_deref(),
            Some("frozen while you drag a finger\nacross the words")
        );
        // Ask AI asks about the words' box, still the selection
        let _ = app.update(Message::Picked(PickAct::Ask));
        let (_, panel) = app.panel.as_ref().expect("Ask AI opens");
        assert_eq!(panel.kind(), actions::Panel::Ask);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Word);
    }

    #[test]
    fn the_bottom_bar_is_there_from_the_first_frame() {
        let mut app = open_app(800, 600, 1.0);
        let _ = app.update(Message::AskInput("what is on my screen?".into()));
        let has_field = |app: &App| overlay_texts(app).contains(&app.ask);
        assert!(has_field(&app), "before any selection");
        let translate = Point::new(642.0, 600.0 - ASK_ZONE / 2.0);
        let clicked = widgets::shot::click(app.view(app.overlay), app.screen, translate);
        assert!(
            clicked
                .iter()
                .any(|m| matches!(m, Message::TranslateScreen))
        );
        // while drawing, and with a selection
        event(
            &mut app,
            overlay::Event::Press {
                at: Point::new(50.0, 50.0),
                shift: false,
            },
        );
        event(&mut app, overlay::Event::Move(Point::new(150.0, 120.0)));
        assert!(has_field(&app), "while drawing");
        event(&mut app, overlay::Event::Release);
        assert!(app.selection.is_some() && has_field(&app));
        // Enter with no selection asks about the whole screen
        let mut app = open_app(800, 600, 1.0);
        let _ = app.update(Message::AskInput("what is this?".into()));
        let _ = app.update(Message::AskSubmit);
        assert_eq!(
            app.panel.as_ref().map(|(_, p)| p.kind()),
            Some(actions::Panel::Ask)
        );
        let bottom = |t: &String| t.starts_with("Ask about your");
        assert!(
            !overlay_texts(&app).iter().any(bottom),
            "the card's own field takes over"
        );
        // Translate translates the whole screen
        let mut app = open_app(800, 600, 1.0);
        let _ = app.update(Message::TranslateScreen);
        assert!(app.translate.is_some() && app.selection.is_none());
    }

    /// What one key press (letter `c`) on the overlay produces, after the
    /// widget operations `task` asks for (focusing a field, say) ran on it.
    fn key_after(app: &App, task: Task<Message>, click: Option<Point>, c: char) -> Vec<Message> {
        use iced::advanced::clipboard;
        use iced::keyboard::{Key, Location, Modifiers, key};
        use iced_runtime::user_interface::{Cache, UserInterface};
        let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
        let view = app.view(app.overlay);
        let mut ui = UserInterface::build(view, app.screen, Cache::default(), &mut renderer);
        let actions = iced_runtime::task::into_stream(task)
            .map(iced::futures::executor::block_on_stream)
            .into_iter()
            .flatten();
        for action in actions {
            if let iced_runtime::Action::Widget(mut op) = action {
                ui.operate(&renderer, op.as_mut());
            }
        }
        let letter = Key::Character(c.to_string().into());
        let press = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: letter.clone(),
            modified_key: letter,
            physical_key: key::Physical::Code(key::Code::KeyT),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: Some(c.to_string().into()),
            repeat: false,
        });
        let mut events = Vec::new();
        if let Some(at) = click {
            use iced::mouse::{Button, Event as Mouse};
            events.extend([
                iced::Event::Mouse(Mouse::CursorMoved { position: at }),
                iced::Event::Mouse(Mouse::ButtonPressed(Button::Left)),
                iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)),
            ]);
        }
        events.push(press);
        let mut messages = Vec::new();
        let cursor = click.map_or(
            iced::mouse::Cursor::Unavailable,
            iced::mouse::Cursor::Available,
        );
        ui.update(
            &events,
            cursor,
            &mut renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        messages
    }

    fn is_key(m: &Message, c: char) -> bool {
        matches!(m, Message::Overlay(overlay::Event::Key { letter, ctrl: false }) if *letter == c)
    }

    /// Letters are the action shortcuts until a text field is clicked: a
    /// result card (Ask AI, once ready) doesn't take the keys by itself.
    #[test]
    fn letters_stay_shortcuts_with_a_result_card_open() {
        let mut app = open_app(800, 600, 1.0);
        drag(
            &mut app,
            Point::new(50.0, 50.0),
            Point::new(150.0, 120.0),
            false,
        );
        let crop = app.crop().unwrap();
        let opened = app.open_panel(actions::Panel::Ask, crop.clone(), None);
        drop(opened); // its work (the setup) is done below by hand
        let (id, _) = app.panel.as_ref().map(|(id, p)| (*id, p.kind())).unwrap();
        let setup = actions::ask::setup(&crop).map(Arc::new);
        assert!(setup.is_ok(), "the no-key provider needs no setup");
        let ready = app.update(Message::Panel(id, panel::Msg::Ready(setup)));
        let keys = key_after(&app, ready, None, 't');
        assert!(
            keys.iter().any(|m| is_key(m, 't')),
            "T is Translate, not typing"
        );
        // the bottom bar's field doesn't take them either
        let app = open_app(800, 600, 1.0);
        let keys = key_after(&app, Task::none(), None, 'a');
        assert!(keys.iter().any(|m| is_key(m, 'a')));
        // once clicked, it has them: a typed question, not a shortcut
        let field = Point::new(300.0, 600.0 - ASK_ZONE / 2.0);
        let keys = key_after(&app, Task::none(), Some(field), 'a');
        assert!(!keys.iter().any(|m| is_key(m, 'a')));
        assert!(
            keys.iter()
                .any(|m| matches!(m, Message::AskInput(t) if t == "a"))
        );
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

    fn behaviour(style: SelectStyle, after: After, search_with: SearchWith) -> SelectionConfig {
        SelectionConfig {
            style,
            after,
            search_with,
        }
    }

    #[test]
    fn auto_search_runs_what_the_setting_says_and_only_for_a_search() {
        let ids = |b| auto_targets(b).iter().map(|a| a.id).collect::<Vec<_>>();
        let snap = SelectStyle::Snap;
        assert!(ids(behaviour(snap, After::Wait, SearchWith::Both)).is_empty());
        assert_eq!(
            ids(behaviour(snap, After::Search, SearchWith::Lens)),
            ["lens"]
        );
        assert_eq!(ids(behaviour(snap, After::Search, SearchWith::Ai)), ["ai"]);
        assert_eq!(
            ids(behaviour(snap, After::Search, SearchWith::Both)),
            ["lens", "ai"]
        );
        // the defaults: snap, then Lens at once
        assert_eq!(
            config::Config::default().selection,
            behaviour(snap, After::Search, SearchWith::Lens)
        );
        // an App: a drawn selection searches; a tapped word, Snip or nothing doesn't
        let mut app = open_app(400, 300, 1.0);
        app.behaviour = behaviour(snap, After::Search, SearchWith::Ai);
        assert!(app.auto_actions().is_empty(), "no selection yet");
        app.select(Rectangle::new(
            Point::new(10.0, 10.0),
            Size::new(80.0, 60.0),
        ));
        assert_eq!(app.auto_actions().len(), 1);
        app.selection.as_mut().unwrap().kind = overlay::Kind::Word;
        assert!(app.auto_actions().is_empty(), "a tapped word waits");
        app.selection.as_mut().unwrap().kind = overlay::Kind::Snap;
        app.purpose = Purpose::Snip(Box::new(Snip::new(Vec::new(), app.screen, false, None)));
        assert!(app.auto_actions().is_empty(), "Snip has no search");
        // an App opened before the setting existed behaves as before
        assert!(auto_targets(App::idle(window::Id::unique()).behaviour).is_empty());
    }

    #[test]
    fn a_circle_snaps_to_a_box_that_morphs_and_resizes_by_its_brackets() {
        let mut app = open_app(800, 600, 1.0);
        app.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
        circle(&mut app, Point::new(300.0, 250.0), 80.0, false);
        let sel = app.selection.unwrap();
        assert_eq!(sel.kind, overlay::Kind::Snap);
        assert!(close_to(
            sel.area,
            Rectangle::new(Point::new(220.0, 170.0), Size::new(160.0, 160.0))
        ));
        assert_eq!(app.stroke.len(), 2, "the box's corners");
        let (from, _) = app.morph.as_ref().expect("it morphs from the drawn loop");
        assert!(from.len() > 20);
        // a straight drag is already a box: no morph
        drag(
            &mut app,
            Point::new(100.0, 100.0),
            Point::new(300.0, 200.0),
            false,
        );
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Snap);
        assert!(app.morph.is_none());
        // the bottom-right bracket's arm resizes it; it stays a snapped box
        let arm = Point::new(303.0, 185.0);
        drag(&mut app, arm, arm + Vector::new(50.0, 40.0), false);
        let grown = app.selection.unwrap();
        assert_eq!(grown.kind, overlay::Kind::Snap);
        assert!(close_to(
            grown.area,
            Rectangle::new(Point::new(100.0, 100.0), Size::new(250.0, 140.0))
        ));
        // "As drawn": the loop as before
        app.behaviour.style = SelectStyle::Shape;
        circle(&mut app, Point::new(500.0, 300.0), 60.0, false);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Loop);
        assert!(app.morph.is_none());
    }

    #[test]
    fn a_circle_snaps_and_searches_only_on_release() {
        let mut app = open_app(800, 600, 1.0);
        app.behaviour = behaviour(SelectStyle::Snap, After::Search, SearchWith::Lens);
        let ring = ring(300.0, 250.0, 80.0);
        let press = overlay::Event::Press {
            at: ring[0],
            shift: false,
        };
        event(&mut app, press);
        for &p in &ring[1..] {
            event(&mut app, overlay::Event::Move(p));
            assert!(app.selection.is_none() && app.morph.is_none(), "drawing");
            assert!(app.auto_actions().is_empty(), "nothing searches yet");
        }
        event(&mut app, overlay::Event::Release);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Snap);
        assert!(app.morph.is_some(), "the stroke morphs into the box");
        assert!(!app.auto_actions().is_empty(), "then the search runs");
    }

    /// A snapped box over `text_app`'s lines 1-3, in select-text mode with its words read.
    fn select_text_app() -> App {
        let mut app = text_app();
        app.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
        let (sel, stroke) = overlay::snap(Rectangle::new(
            Point::new(40.0, 66.0),
            Size::new(480.0, 112.0),
        ));
        (app.selection, app.stroke) = (Some(sel), stroke);
        app.text_mode = Some(TextMode {
            lines: None,
            boxes: Vec::new(),
            picked: None,
            picking: false,
        });
        let lines = word::lines(text_blocks(), [0, 0], 1.0);
        let inside: Vec<_> = (lines.into_iter())
            .filter(|l| sel.area.contains(l.bounds.center()))
            .collect();
        app.on_text_lines(sel.area, Ok(inside));
        app
    }

    #[test]
    fn select_text_highlights_every_word_and_a_drag_picks_some() {
        let mut app = select_text_app();
        let texts = widgets::shot::texts(app.view(app.overlay));
        assert!(texts.contains(&"Done".to_string()), "{texts:?}");
        let mode = app.text_mode.as_ref().unwrap();
        assert_eq!(mode.boxes.len(), 17, "every word of lines 1-3");
        // a stale read (the box moved on) is dropped
        app.on_text_lines(Rectangle::default(), Ok(Vec::new()));
        assert_eq!(app.text_mode.as_ref().unwrap().boxes.len(), 17);
        // press on "while", drag to "words": two lines picked, the box stays
        drag(&mut app, at_word(1, 7.0), at_word(2, 11.0), false);
        let mode = app.text_mode.as_ref().unwrap();
        let picked = mode.picked.as_ref().unwrap();
        assert_eq!(picked.text(), "while you drag a finger\nacross the words");
        assert!(!mode.picking);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Snap);
        // the popup offers the text actions; the action bar waits
        let texts = widgets::shot::texts(app.view(app.overlay));
        for want in ["Copy", "Translate", "Search", "Ask AI"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        assert!(!texts.iter().any(|t| t == "Edit"), "{texts:?}");
        // Done leaves the mode; a press outside the box starts over
        let _ = app.update(Message::SelectText);
        assert!(app.text_mode.is_none());
        let _ = app.update(Message::SelectText); // no OCR models here: the Text panel asks
        drag(
            &mut app,
            Point::new(700.0, 300.0),
            Point::new(900.0, 400.0),
            false,
        );
        assert!(app.text_mode.is_none());
    }

    #[test]
    fn after_an_auto_search_the_bar_is_one_click_away() {
        let mut app = open_app(800, 600, 1.0);
        app.behaviour = behaviour(SelectStyle::Snap, After::Search, SearchWith::Lens);
        let (sel, stroke) = overlay::snap(Rectangle::new(
            Point::new(200.0, 150.0),
            Size::new(300.0, 200.0),
        ));
        (app.selection, app.stroke) = (Some(sel), stroke);
        let texts = widgets::shot::texts(app.view(app.overlay));
        for want in ["Select text", "More"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        assert!(
            !texts.iter().any(|t| t == "Ask AI"),
            "no bar yet: {texts:?}"
        );
        let _ = app.update(Message::More);
        let texts = widgets::shot::texts(app.view(app.overlay));
        assert!(texts.iter().any(|t| t == "Ask AI"), "{texts:?}");
        assert!(!texts.iter().any(|t| t == "More"));
        // waiting for an action: the bar at once, no More
        app.behaviour.after = After::Wait;
        app.more = false;
        let texts = widgets::shot::texts(app.view(app.overlay));
        assert!(texts.iter().any(|t| t == "Ask AI") && !texts.iter().any(|t| t == "More"));
    }

    #[test]
    fn chips_go_under_the_box_else_above_never_on_it() {
        let screen = Rectangle::with_size(Size::new(1920.0, 1080.0));
        let size = Size::new(280.0, 44.0);
        let area = Rectangle::new(Point::new(400.0, 300.0), Size::new(500.0, 200.0));
        let at = chips_at(area, size, screen);
        assert_eq!(at, Point::new(510.0, 512.0), "centred, 12 px under the box");
        // no room below: above
        let low = Rectangle::new(Point::new(400.0, 900.0), Size::new(500.0, 150.0));
        let at = chips_at(low, size, screen);
        assert_eq!(at.y, 900.0 - CHIP_SPACE - 44.0);
        for a in [area, low] {
            let chips = Rectangle::new(chips_at(a, size, screen), size);
            assert!(chips.intersection(&a).is_none(), "{a:?} -> {chips:?}");
        }
        // beside a card: kept in the free part
        let room = Rectangle::new(Point::ORIGIN, Size::new(1112.0, 1080.0));
        let edge = Rectangle::new(Point::new(950.0, 300.0), Size::new(150.0, 100.0));
        let at = chips_at(edge, size, room);
        assert!(at.x + size.width <= room.width - bar::GAP);
        // the whole screen selected: on it, at the bottom, still on screen
        let all = chips_at(screen, size, screen);
        assert_eq!(all.y, 1080.0 - bar::GAP - 44.0);
    }

    #[test]
    fn nothing_floats_over_the_result_card() {
        let mut app = open_app(1920, 1080, 1.0);
        // a selection right up to where the card goes, bar shown beside it
        drag(
            &mut app,
            Point::new(700.0, 300.0),
            Point::new(940.0, 500.0),
            false,
        );
        let _ = app.update(Message::Run(&text::ACTION));
        app.more = true;
        let card = app.card_at();
        let room = app.room();
        assert!(room.intersection(&card).is_none(), "{room:?} vs {card:?}");
        assert!(room.width > 0.0);
        // and with no card, the whole screen
        app.panel = None;
        assert_eq!(app.room(), Rectangle::with_size(app.screen));
    }

    #[test]
    fn a_quick_action_on_a_drawn_area_runs_alone_not_the_auto_search_too() {
        let mut app = open_app(1920, 1080, 1.0);
        app.behaviour = SelectionConfig::default(); // auto search: Lens
        let ask = Quick::Action(&actions::ask::ACTION);
        app.purpose = Purpose::Quick(ask, config::Target::Circle);
        drag(
            &mut app,
            Point::new(100.0, 100.0),
            Point::new(400.0, 300.0),
            false,
        );
        let panel = app.panel.as_ref().map(|(_, p)| p.kind());
        assert_eq!(panel, Some(actions::Panel::Ask), "Ask AI, not Lens over it");
    }

    #[test]
    fn lens_in_the_browser_closes_the_overlay_where_it_cannot_go_behind() {
        let mut app = open_app(1920, 1080, 1.0);
        drag(
            &mut app,
            Point::new(100.0, 100.0),
            Point::new(400.0, 300.0),
            false,
        );
        let _ = app.update(Message::Run(&lens::ACTION));
        let id = app.panel.as_ref().expect("the Lens panel").0;
        let opened = panel::Msg::LensOpened(Ok(lens::Held::for_test()));
        let _ = app.update(Message::Panel(id, opened));
        // Windows: behind the browser, back on the shortcut; elsewhere: closed.
        assert_eq!(app.frame.is_some(), cfg!(windows));
        assert_eq!(app.panel.is_some(), cfg!(windows));
        // The same when the user takes the results card's page to the browser.
        let mut app = open_app(1920, 1080, 1.0);
        drag(
            &mut app,
            Point::new(100.0, 100.0),
            Point::new(400.0, 300.0),
            false,
        );
        let _ = app.update(Message::Run(&lens::ACTION));
        let id = app.panel.as_ref().expect("the Lens panel").0;
        let left = panel::Msg::Sheet(crate::sheet::Event::Left);
        let _ = app.update(Message::Panel(id, left));
        assert_eq!(app.frame.is_some(), cfg!(windows));
    }

    #[test]
    fn the_result_card_opens_away_from_the_selection_and_moves_by_its_grip() {
        let mut app = open_app(1920, 1080, 1.0);
        drag(
            &mut app,
            Point::new(100.0, 100.0),
            Point::new(400.0, 300.0),
            false,
        );
        let _ = app.update(Message::Run(&text::ACTION)); // no models: the consent card
        assert!(app.panel.is_some());
        let at = app.card_at();
        assert_eq!(at.x + at.width, 1920.0 - card::GAP, "right");
        // the grip: the card follows the pointer, and stays there this session
        let id = app.panel.as_ref().unwrap().0;
        let grab = panel::Msg::Card(card::Event::Grab(card::Drag::Move));
        let _ = app.update(Message::Panel(id, grab));
        let _ = app.update(Message::CardMoved(Point::new(1700.0, 200.0)));
        let _ = app.update(Message::CardMoved(Point::new(1100.0, 200.0)));
        let _ = app.update(Message::CardReleased);
        assert_eq!(
            app.card_at().position(),
            at.position() + Vector::new(-600.0, 0.0)
        );
        let _ = app.update(Message::CardMoved(Point::new(0.0, 0.0)));
        assert_eq!(app.card_at().x, at.x - 600.0, "let go: no more moves");
        // a selection on the right: a new card would go left, but the user's place wins
        app.card = None;
        drag(
            &mut app,
            Point::new(1500.0, 100.0),
            Point::new(1800.0, 300.0),
            false,
        );
        assert_eq!(app.card_at().x, widgets::card::GAP);
    }

    /// `OPENCIRCLE_LOOK_PNG=<dir> cargo test ui::tests::look_shots -- --ignored`:
    /// a snapped selection (mid-morph and settled, with its chips), select-text
    /// mode with the popup, and the bar after More, over a frame with the edge glow.
    #[test]
    #[ignore = "writes PNGs to look at"]
    fn look_shots() {
        let dir = std::path::PathBuf::from(
            std::env::var("OPENCIRCLE_LOOK_PNG").expect("OPENCIRCLE_LOOK_PNG=<dir>"),
        );
        let shot = |app: &App, name: &str| {
            let rgba = widgets::shot::render(app.view(app.overlay), 960, 420);
            actions::write_png(&dir.join(name), 960, 420, &rgba).unwrap();
        };
        let glowing_at = |app: &mut App, w: u32, h: u32| {
            if let Some(image::Handle::Rgba { pixels, .. }) = &app.frame {
                let mut rgba = pixels.to_vec();
                border::paint(&mut rgba, w, h, 1.0, &config::look(), border::Mode::Overlay);
                app.frame = Some(image::Handle::from_rgba(w, h, rgba));
            }
        };
        let glowing = |app: &mut App| {
            if let Some(image::Handle::Rgba { pixels, .. }) = &app.frame {
                let mut rgba = pixels.to_vec();
                let look = config::look();
                border::paint(&mut rgba, 960, 420, 1.0, &look, border::Mode::Overlay);
                app.frame = Some(image::Handle::from_rgba(960, 420, rgba));
            }
        };
        let mut app = text_app();
        glowing(&mut app);
        app.behaviour = behaviour(SelectStyle::Snap, After::Search, SearchWith::Lens);
        let ring = ring(280.0, 122.0, 110.0)
            .into_iter()
            .map(|p| Point::new(p.x * 1.9 - 250.0, p.y * 0.9 + 12.0))
            .collect::<Vec<_>>();
        let (sel, stroke) = overlay::snap(overlay::select(&ring, 1.0).unwrap().area);
        (app.selection, app.stroke) = (Some(sel), stroke);
        app.morph = Some((ring, Instant::now() - overlay::MORPH / 3));
        shot(&app, "look-1-snap-morphing.png");
        app.morph = None;
        shot(&app, "look-2-snapped-chips.png");
        let _ = app.update(Message::More);
        shot(&app, "look-3-more-bar.png");
        let _ = app.update(Message::Run(&text::ACTION)); // no models here: the consent card
        shot(&app, "look-6-result-card.png");
        // Ask AI beside a selection at 1440x900, the bar kept out of the card,
        // the resize grip showing (hovered)
        let mut wide = open_app(1440, 900, 1.0);
        glowing_at(&mut wide, 1440, 900);
        drag(
            &mut wide,
            Point::new(120.0, 140.0),
            Point::new(640.0, 520.0),
            false,
        );
        let _ = wide.update(Message::Run(&actions::ask::ACTION));
        wide.more = true;
        wide.card_hover = true;
        let rgba = widgets::shot::render(wide.view(wide.overlay), 1440, 900);
        actions::write_png(&dir.join("look-7-ask-card-1440.png"), 1440, 900, &rgba).unwrap();
        let mut app = select_text_app();
        glowing(&mut app);
        shot(&app, "look-4-select-text.png");
        drag(&mut app, at_word(1, 7.0), at_word(2, 11.0), false);
        shot(&app, "look-5-select-text-popup.png");
    }

    /// `OPENCIRCLE_LOOK_PNG=<dir> cargo test ui::tests::widget_theme_shots -- --ignored`:
    /// the bar and the Ask AI card in every widget theme, glass on and off, over a busy frame.
    #[test]
    #[ignore = "writes PNGs to look at"]
    fn widget_theme_shots() {
        use crate::config::WidgetTheme::{Default, Frost, Graphite};
        let dir = std::path::PathBuf::from(
            std::env::var("OPENCIRCLE_LOOK_PNG").expect("OPENCIRCLE_LOOK_PNG=<dir>"),
        );
        let (w, h) = (1100u32, 620u32);
        let busy: Vec<u8> = (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    let stripe = ((x + y) / 24) % 2 == 0;
                    let wave =
                        ((x as f32 / 37.0).sin() * 40.0 + (y as f32 / 29.0).cos() * 40.0) as i32;
                    let text = (y / 11) % 3 == 0 && (x / 7) % 5 != 0 && y > 60;
                    let base = if stripe { [230, 90, 70] } else { [40, 90, 200] };
                    let c = base.map(|v| (v + wave).clamp(0, 255) as u8);
                    if text {
                        [250, 250, 245, 255]
                    } else {
                        [c[0], c[1], c[2], 255]
                    }
                })
            })
            .collect();
        for (name, theme, glass) in [
            ("default", Default, true),
            ("frost-glass", Frost, true),
            ("frost-solid", Frost, false),
            ("graphite-glass", Graphite, true),
            ("graphite-solid", Graphite, false),
        ] {
            config::set_current(config::Config {
                widget_theme: theme,
                glass_effect: glass,
                ..config::Config::default()
            });
            let mut app = open_app(w, h, 1.0);
            app.frame = Some(image::Handle::from_rgba(w, h, busy.clone()));
            glass::set_frame(app.frame.as_ref(), 1.0);
            drag(
                &mut app,
                Point::new(90.0, 160.0),
                Point::new(420.0, 400.0),
                false,
            );
            let _ = app.update(Message::Run(&actions::ask::ACTION));
            app.more = true;
            let rgba = widgets::shot::render(app.view(app.overlay), w, h);
            actions::write_png(&dir.join(format!("theme-{name}.png")), w, h, &rgba).unwrap();
        }
        config::set_current(config::Config::default());
    }

    /// Add capture: the editor steps aside, an area dragged on the frozen frame
    /// joins the board (Esc first: back to the editor, nothing added).
    #[test]
    fn add_capture_puts_a_picked_area_on_the_board() {
        const BLUE: [u8; 4] = [20, 60, 230, 255];
        // Left half white, right half blue.
        let rgba = (0..1200 * 800)
            .flat_map(|i| if i % 1200 < 600 { [255; 4] } else { BLUE })
            .collect();
        let frame = Crop {
            width: 1200,
            height: 800,
            rgba,
        };
        let (mut app, _) = selected_app(&frame);
        app.select(Rectangle::new(
            Point::new(10.0, 10.0),
            Size::new(300.0, 200.0),
        ));
        let _ = app.open_editor();
        let mut editor = |msg| {
            let _ = app.update(Message::Editor(msg));
        };
        editor(editor::Message::AddCapture);
        editor(editor::Message::Back);
        editor(editor::Message::AddCapture);
        editor(editor::Message::Press(Point::new(700.0, 100.0)));
        editor(editor::Message::Move(Point::new(1000.0, 400.0)));
        editor(editor::Message::Release);
        let edit = app.edit.as_ref().expect("still editing");
        assert_eq!(edit.editor.doc.tiles().len(), 2);
        let board = edit.image();
        assert!(
            board.rgba.chunks(4).any(|p| p == BLUE),
            "the blue area is on it"
        );
        let r = edit.canvas_rect();
        assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.width <= 1200.0 && r.y + r.height <= 800.0);
    }
}
