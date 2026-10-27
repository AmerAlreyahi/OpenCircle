//! The iced daemon: owns the overlay window (pre-created and hidden between uses
//! where windows can hide, see `can_hide_windows`) and its state.
mod about;
mod bar;
mod border;
mod edit;
/// Frames as the window draws them (damage), and the animations' timing.
#[cfg(test)]
mod frame_times;
mod glass;
mod keys;
mod live_controller;
mod live_layer;
pub(crate) mod motion;
mod onboarding;
mod overlay;
mod panel;
mod picker;
mod quick;
mod sapphire;
mod settings;
pub(crate) mod setup;
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

pub use tray::{refresh as refresh_tray, start as start_tray};

use std::cell::{Cell, RefCell};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use iced::futures::stream::{self, Stream, StreamExt};
use iced::keyboard::{self, key::Named};
use iced::widget::{canvas, container, image, opaque, operation, space, stack};
use iced::window::{self, Level, Mode};
use iced::{ContentFit, Element, Fill, Point, Rectangle, Size, Subscription, Task, Vector};

use crate::actions::{self, Action, Crop, Run, copy, lens, qr, save, text};
use crate::config::{
    self, After, BarAnchor, QuickAction, SearchWith, SelectStyle, SelectionConfig, Target,
    TextReader,
};
use crate::editor::{self, Output};
use crate::labs::{self, BarButton, Labs};
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
/// The bottom bar (the new design's search bar, owner 2026-10-26: "circle, then
/// choose"): grip, the AI field, the pills Search in browser · Copy text ·
/// Translate (`labs::bar`), then Live translate (Beta) and, while their Labs
/// switch is on, Song search and Edit. All there in every state, so it never
/// changes size; Search in browser and Copy text are dimmed until something is
/// selected. Nothing is sent until the user picks one (`[selection] after`).
/// The field's placeholder.
const ASK_PLACEHOLDER: &str = "Ask OpenCircle AI about your screen…";
/// Shorter ones where the font or a narrow screen leaves the field too little
/// room for it (#144): never cut off mid-word.
const ASK_PLACEHOLDERS: [&str; 4] = [
    ASK_PLACEHOLDER,
    "Ask AI about your screen…",
    "Ask OpenCircle AI…",
    "Ask AI…",
];

/// The placeholder that fits a field `width` wide, in the UI font.
fn ask_placeholder(width: f32) -> &'static str {
    let room = width - widgets::AI_FIELD_LEFT - widgets::AI_FIELD_RIGHT;
    widgets::fitting(&ASK_PLACEHOLDERS, 14.0, room)
}
/// How long the bar takes to turn into the editing toolbar (and back),
/// and to settle on a new spot after a drag (the design's drag release).
const BAR_MORPH: Duration = motion::MORPH;
const GLIDE: Duration = motion::DOCK;
/// A QR code's chip under a selection (its room; the chip is centred in it).
const QR_WIDTH: f32 = 520.0;
/// The bottom bar's Live translate icon (lucide "radio").
const LIVE: char = '\u{e145}';
const ASK_MARGIN: f32 = 24.0;

/// A capture still unanswered after this is given up (a Wayland portal may wait
/// on a dialog; its late answer is dropped).
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(20);

/// Snip reads the controls (UI Automation) of this many topmost windows at
/// once, of any other when the pointer first goes over it.
const UIA_WINDOWS: usize = 4;

/// How far (logical px) the hint and the bottom bar slide in as the overlay opens.
const SLIDE: f32 = 80.0;

/// Presses this close together are a double (triple) click.
// (Windows' default; also forgiving when a busy machine delays the second press.)
const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// The text reader's helper starts this long after the opening animation.
const WARM_AFTER: Duration = Duration::from_millis(400);

/// The overlay fades in at most this long after it was shown, its first
/// full frame drawn or not (never left invisible over the screen).
const REVEAL_WAIT: Duration = Duration::from_millis(400);

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
    let mut app = iced::daemon(App::boot, App::update, App::view);
    for font in widgets::FONTS {
        app = app.font(font);
    }
    app.default_font(widgets::ui_font())
        .title(App::title)
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
    /// Esc that a widget took (a field let go of the focus; the editor): it
    /// only closes what the widget does not own.
    EscapeTaken,
    /// A focused field gave a key back (`keys`).
    Field(keys::Keyed),
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
    /// The copy-text quick action read the text and copied it (or why not).
    TextRead(Result<(), String>),
    /// A Copy / Save run off the UI thread is done: its pill, where the
    /// overlay was (its origin, size and scale).
    Confirm((Point, Size, f32), Result<String, String>),
    /// A link opened (or text copied) off the UI thread: the overlay closes,
    /// after this pill if any.
    ThenClose(Option<&'static str>),
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
    /// The bottom bar's buttons: Ask AI (Enter, or the presets with an empty
    /// field), Translate the screen, Live translate, the whole screen in the editor.
    AskButton,
    /// One of the bar's labelled pills (Search in browser, Copy text, Translate).
    Pill(BarButton),
    TranslateScreen,
    LiveToggle,
    EditScreen,
    /// A frame of the overlay's opening animation was drawn at this time.
    Frame(Instant),
    /// OCR read this band of this opening's frame (main mode, on demand).
    ScreenText(u32, Rectangle, Result<Vec<word::Line>, String>),
    /// The text reader's helper of this opening has started (after its
    /// opening animation).
    Warmed(u32),
    /// The note over the bar's Download (the text reader's models), and how
    /// that download for this opening goes (`Reading` = done).
    ReaderDownload,
    Reader(u32, Result<text::Progress, String>),
    /// The "Select text" chip under a snapped box, and the lines OCR read in it.
    SelectText,
    TextLines(Rectangle, Result<Vec<word::Line>, String>),
    /// The bottom bar's handle went down / the pointer moved / it was let go.
    BarGrab,
    BarMoved(Point),
    BarReleased,
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
    /// A snapped stroke finished morphing into its box: the selection's actions show.
    Settled,
}

/// The shortcuts of the bottom bar's buttons, as the tooltips show them ("" = none).
#[derive(Debug, Default, Clone, PartialEq)]
struct BarKeys {
    search: String,
    copy_text: String,
    translate: String,
    live: String,
    song: String,
    edit: String,
}

impl BarKeys {
    fn of(cfg: &config::Config) -> BarKeys {
        let quick = |id: &str, live: bool| {
            let bound = (cfg.quick_actions.iter())
                .find(|q| q.action == id && q.live == live && !q.shortcut.is_empty());
            bound.map(|q| q.shortcut.clone()).unwrap_or_default()
        };
        let key = |a: &Action| bar::key_hint(a.key);
        BarKeys {
            search: key(&lens::ACTION),
            copy_text: quick("copy_text", false),
            translate: quick(actions::translate::ACTION.id, false),
            live: quick(actions::translate::ACTION.id, true),
            song: quick("song_search", false),
            edit: key(&actions::edit::ACTION),
        }
    }
}

/// What the select-text popup does with the picked words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickAct {
    Copy,
    Translate,
    Search,
}

/// Select-text mode on a snapped box: its words (`None` while OCR reads them)
/// and the ones picked by a press-drag over them.
struct TextMode {
    lines: Option<Vec<word::Line>>,
    boxes: Vec<Rectangle>,
    picked: Option<word::Picked>,
    picking: bool,
}

/// The text reader (OCR) behind text selection in main mode, as the note over
/// the bar tells it: without its models a press on text can only draw.
#[derive(Debug, Clone, PartialEq)]
enum Reader {
    /// Nothing to say: the screen's text is read (or nothing reads it).
    Idle,
    /// Its models aren't downloaded (this many bytes to go): the note offers
    /// the download (one click).
    Missing(u64),
    /// Downloading them (0..=1), after that click.
    Downloading(f32),
    /// The download failed (why); the note offers it again.
    Failed(String),
}

/// What waits for the band under it to be read (main mode).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Await {
    /// A tap: the word there (a triple tap: the line) becomes the selection.
    Tap(word::Unit),
    /// A Shift+click: the text selection stretches there.
    Extend,
}

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

/// A press on the selection: resizing it by a corner or an edge, or moving it.
struct Grab {
    /// The corner or edge, or `None` = move.
    grip: Option<overlay::Grip>,
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

/// The overlay window: hidden (pre-created), or full-screen at `size`. Never on
/// the taskbar (Windows): it leaves full-screen visible while it closes (`Hiding`).
fn overlay_window(shown: Option<Size>) -> window::Settings {
    #[cfg(windows)]
    let platform_specific = window::settings::PlatformSpecific {
        skip_taskbar: true,
        ..Default::default()
    };
    #[cfg(not(windows))]
    let platform_specific = Default::default();
    window::Settings {
        platform_specific,
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

/// The toast pill's gap to the screen's right edge and to the taskbar (design: 16).
const TOAST_MARGIN: f32 = 16.0;

/// A captured monitor, ready to show.
#[derive(Debug, Clone)]
struct Shot {
    image: image::Handle,
    scale: f32,
    /// Logical px: the monitor's origin and size.
    origin: Point,
    size: Size,
    /// Snip mode: the mode to start in, every window (physical desktop px,
    /// topmost first), the window the user was in (Windows), and every monitor
    /// with its frame (this one too).
    snip: Option<SnipShot>,
}

type SnipShot = (
    snip::Kind,
    Vec<snip::Pick>,
    Option<isize>,
    Vec<(snip::Place, image::Handle)>,
);

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
    /// The frame cut into tiles for drawing (`overlay::tiles`), and its id.
    tiles: Option<(iced::advanced::image::Id, Vec<(Point, image::Handle)>)>,
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
    /// Alt is held: a press inside the box moves it, even over text.
    alt: bool,
    /// Resizing or moving the selection.
    grab: Option<Grab>,
    /// The zoomed / panned part of the frame on screen; a pan's start.
    view: overlay::View,
    pan: Option<(Point, overlay::View)>,
    /// The tapped word(s) the selection is, and the latest tap's number.
    word: Option<word::Picked>,
    taps: u32,
    /// Main mode: the frame's text lines (image px), read on demand band by
    /// band (#123): never at open, the band the user touches first.
    screen_text: word::Screen,
    /// The band OCR reads now (one at a time: the helper serves one request
    /// at a time anyway).
    reading: Option<Rectangle>,
    /// A tap or Shift+click waiting for the band under it.
    awaits: Option<(Point, Await)>,
    /// Ctrl+A waits for the whole screen to be read.
    want_all: bool,
    /// A drag let go before the band under its press was read, and the
    /// overlay events after it: replayed once it is.
    held: Option<Vec<overlay::Event>>,
    /// Text was selected in this opening (or, off Windows, its helper
    /// started: `Warmed`): the rest of the screen is read in the background
    /// (gently), nearest the pointer first.
    text_used: bool,
    /// Where the pointer was last seen (image px).
    pointer: Point,
    /// The text reader's models are here (checked at open).
    ocr_ready: bool,
    /// When the overlay drew each frame since the last `log_frames` (the
    /// VM frame budget: the opening, a bar or card drag, the glide after).
    frame_times: RefCell<Vec<Instant>>,
    /// How that read is going (the note over the bar).
    reader: Reader,
    /// Who reads this opening's text (`ocr::route`), once its first read
    /// started: Windows OCR (the whole screen at once) or the helper (bands).
    text_by: Option<ocr::Source>,
    /// A press that began on text: its stroke so far (image px). It selects
    /// words unless it turns into a loop (`overlay::winds`).
    text_press: Option<Vec<Point>>,
    /// The opening animation (`overlay::APPEAR`): the latest frame's time and
    /// how far it is (0..1, linear); `None` until the first frame (t = 0).
    appear: Option<(Instant, f32)>,
    /// The bar's highlighted pill: Search in browser at every opening, then the
    /// one last pressed (the new design's active pill).
    pill: BarButton,
    /// The shortcuts the bottom bar's tooltips show (read at open).
    bar_keys: BarKeys,
    /// Which unfinished features are on (read at open; Settings changes apply to the next one).
    labs: Labs,
    /// The last press: when, where (image px), and how many in a row.
    clicks: Option<(Instant, Point, u8)>,
    /// The ask bar's text.
    ask: String,
    /// Bumped on every open (resets the overlay's pointer state).
    opening: u32,
    /// Trigger time + expected size of the full-screen frame not drawn yet (latency log).
    first_draw: Cell<Option<(Instant, Size)>>,
    /// Shown but still fully clear (`show`): fades in once its first full
    /// frame is drawn (`first_draw` taken), or after `REVEAL_WAIT` anyway.
    reveal: Option<Instant>,
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
    /// (shared with the screen's text reads: one that starts after the
    /// overlay closed reads nothing, `read_band`)
    helper: Option<Arc<ocr::Hold>>,
    /// How selections behave (`[selection]`), read when the overlay opens.
    behaviour: SelectionConfig,
    /// A snapped selection's drawn stroke (image px) morphing into its box.
    morph: Option<(Vec<Point>, Instant)>,
    text_mode: Option<TextMode>,
    /// The bottom bar turning into the editing toolbar in its place: 0 = the
    /// bar, 1 = the toolbar (linear), and while it turns: since when, from
    /// and to where (`None`: at rest).
    swap: f32,
    swap_at: Option<(Instant, f32, f32)>,
    /// The editor just went back to the bar (until the next frame): an
    /// Escape now is the key that did it.
    esc_back: bool,
    /// Where Tab put the focus in the bottom bar (the field's own focus is the
    /// text input's); none = the overlay has it, so the letters are shortcuts.
    bar_focus: Option<bar::Stop>,
    /// The next snip capture is the one a timer waited for: no timer again.
    timed: bool,
    /// The bar's spot (`config::BarConfig`); while its handle is held, the
    /// pointer's last position and where the bar follows it (top-left);
    /// where it is drawn now (gliding to its spot); how much of the screen's
    /// bottom the taskbar takes (logical px).
    bar_anchor: BarAnchor,
    bar_drag: Option<(Option<Point>, Point)>,
    bar_now: Option<Point>,
    /// Its glide to a new spot: since when, from and to where.
    bar_glide: Option<(Instant, Point, Point)>,
    work_inset: f32,
    /// Where the user moved / sized the result card (this run of the app).
    card: Option<Rectangle>,
    /// The card's grip or corner is held, and the pointer's last position.
    card_drag: Option<(widgets::card::Drag, Option<Point>)>,
    /// The result card opening (`motion::WIDGET`): which panel, its motion,
    /// and how open it was at the latest frame (it grows from `OPEN_SCALE`).
    card_open: Option<(u32, motion::Shown, f32)>,
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
            tiles: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
            others: Vec::new(),
            adding: false,
            alt: false,
            grab: None,
            view: overlay::View::new(1, 1, Size::new(1.0, 1.0)),
            pan: None,
            word: None,
            taps: 0,
            screen_text: word::Screen::default(),
            reading: None,
            awaits: None,
            want_all: false,
            held: None,
            text_used: false,
            pointer: Point::ORIGIN,
            ocr_ready: false,
            frame_times: RefCell::default(),
            reader: Reader::Idle,
            text_by: None,
            text_press: None,
            clicks: None,
            appear: None,
            pill: BarButton::Search,
            bar_keys: BarKeys::default(),
            labs: Labs::default(),
            ask: String::new(),
            opening: 0,
            first_draw: Cell::new(None),
            reveal: None,
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
                text_reader: TextReader::Auto,
            },
            morph: None,
            text_mode: None,
            swap: 0.0,
            swap_at: None,
            esc_back: false,
            bar_focus: None,
            timed: false,
            bar_anchor: BarAnchor::Bottom,
            bar_drag: None,
            bar_now: None,
            bar_glide: None,
            work_inset: 0.0,
            card: None,
            card_drag: None,
            card_open: None,
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
            // A hidden (Labs) feature does nothing, whoever asks (no shortcut or tray
            // item reaches here; this is the one place that also covers a late trigger).
            Message::Trigger(Trigger::Quick(_, qa))
                if !labs::quick_on(&config::current().labs, &qa) =>
            {
                crate::log!("[opencircle] \"{}\" is off (Settings > Labs)", qa.action);
                Task::none()
            }
            Message::Trigger(Trigger::Update(event)) => self.on_update(event),
            Message::UpdateTick => self.try_update(),
            // the rebuilt view shows the actions; the opt-in auto action runs now
            Message::Settled if self.others.is_empty() => self.auto_search(),
            Message::Settled => Task::none(),
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
            // The snip toast is only a pill: a new shot replaces it.
            Message::Trigger(trigger @ (Trigger::Capture(_) | Trigger::Quick(..)))
                if self.snip_toast() =>
            {
                let closing = self.close();
                closing.chain(self.update(Message::Trigger(trigger)))
            }
            Message::Trigger(Trigger::Capture(_) | Trigger::Quick(..))
                if self.capturing.is_some() =>
            {
                crate::log!("[opencircle] shortcut ignored: a capture is still on its way");
                Task::none()
            }
            // Open but maybe behind another app (Alt+Tab, a click elsewhere): its
            // keys (Esc) went there. The shortcut brings it back in front, with
            // the keyboard, instead of nothing happening.
            Message::Trigger(Trigger::Capture(_) | Trigger::Quick(..)) if self.frame.is_some() => {
                crate::log!(
                    "[opencircle] shortcut while the overlay is open: brought to the front"
                );
                window::gain_focus(self.overlay)
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
                    None => crate::log!("unknown action {}; opening the overlay", qa.action),
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
            // Song search is opt-in: without a key the button opens its setup
            // (Settings > Song search) and nothing is recorded.
            Message::SongStart => self.song(self.has_song_key()),
            Message::Song(id, msg) => self.on_song(id, msg),
            Message::CaptureTimedOut(n) if self.capturing == Some(n) => {
                crate::log!(
                    "capture failed: no screenshot after {} s; try again",
                    CAPTURE_TIMEOUT.as_secs()
                );
                self.capturing = None;
                self.purpose = Purpose::Search;
                Task::none()
            }
            Message::CaptureTimedOut(_) => Task::none(),
            Message::Escape if self.capturing.is_some() => {
                crate::log!("[opencircle] capture cancelled");
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
            // The editor's Esc went back to the bar: the same key's Escape
            // (it may come after) doesn't close the overlay too.
            Message::Escape if self.esc_back => {
                self.esc_back = false;
                Task::none()
            }
            Message::Escape => self.close(),
            Message::EscapeTaken
                if self.edit.is_some()
                    || self.esc_back
                    || self.capturing.is_some()
                    || self.scroll().is_some() =>
            {
                self.update(Message::Escape)
            }
            // A field took it: it was that field's Esc (a second one closes).
            Message::EscapeTaken => Task::none(),
            Message::Field(keys::Keyed::Left) => {
                self.bar_focus = None;
                Task::none()
            }
            // (a click put the focus in the bar's field: Tab goes on from there)
            Message::Field(keys::Keyed::Focus) => {
                self.bar_focus = Some(bar::Stop::Field);
                Task::none()
            }
            Message::Field(keys::Keyed::Copy) => self.on_overlay(overlay::Event::Key {
                letter: 'c',
                ctrl: true,
            }),
            Message::Editor(msg) => self.on_editor(msg),
            Message::Snip(msg) => self.on_snip(msg),
            Message::Confirm(place, result) => self.toast_for(place, result),
            Message::ThenClose(Some(pill)) => self.toast_then_close(pill),
            Message::ThenClose(None) => self.close(),
            Message::TextRead(copied) if self.toast.is_some() => {
                self.toast = Some(match copied {
                    Ok(()) => "Text copied",
                    Err(e) => {
                        crate::log!("copy text failed: {e}");
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
            Message::BarGrab => {
                self.bar_drag = Some((None, self.bar_at()));
                self.frame_times.borrow_mut().clear();
                Task::none()
            }
            // While held it follows the pointer (kept on screen).
            Message::BarMoved(p) => {
                if let Some((last, at)) = self.bar_drag {
                    let size = self.dock_size();
                    let at = match last {
                        Some(from) => {
                            let to = at + (p - from);
                            let max = |whole: f32, side: f32| (whole - side).max(0.0);
                            Point::new(
                                to.x.clamp(0.0, max(self.screen.width, size.width)),
                                to.y.clamp(0.0, max(self.screen.height, size.height)),
                            )
                        }
                        None => at,
                    };
                    self.bar_drag = Some((Some(p), at));
                    self.bar_now = Some(at);
                }
                Task::none()
            }
            // Let go: it glides to the nearest spot, remembered.
            Message::BarReleased => match self.bar_drag.take() {
                Some((_, at)) => {
                    let size = self.dock_size();
                    self.bar_anchor = bar::nearest(at, self.screen, size, self.work_inset);
                    crate::log!("[opencircle] bar docked: {:?}", self.bar_anchor);
                    self.log_frames("bar drag");
                    self.bar_now = Some(at);
                    self.remember_bar()
                }
                None => Task::none(),
            },
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
            // Let go: the card stays where the user put it, until the overlay closes.
            Message::CardReleased => {
                match self.card_drag.take() {
                    Some((card::Drag::Move, _)) => self.log_frames("card drag"),
                    Some((card::Drag::Resize, _)) => self.log_frames("card resize"),
                    None => {}
                }
                Task::none()
            }
            Message::AskInput(text) => {
                self.ask = text;
                Task::none()
            }
            // The AI field (the new design): Enter, its logo or its "AI ↵" badge.
            // A question asks about the selection (before one: the whole
            // screen); an empty field opens Ask AI with its presets.
            Message::AskSubmit | Message::AskButton => {
                let question = std::mem::take(&mut self.ask);
                let question = (!question.trim().is_empty()).then_some(question);
                match self.stacked().or_else(|| self.whole()) {
                    Some(crop) => self.open_panel(actions::Panel::Ask, crop, question),
                    None => Task::none(),
                }
            }
            Message::Pill(pill) => {
                self.pill = pill;
                self.on_pill(pill)
            }
            // Live translate runs over the live screen: the frozen one goes.
            Message::LiveToggle => {
                let closing = self.close();
                closing.chain(self.toggle_live(Instant::now()))
            }
            Message::EditScreen => {
                self.others.clear();
                self.word = None;
                self.select(Rectangle::with_size(self.screen));
                self.open_editor()
            }
            Message::TranslateScreen => {
                self.others.clear();
                self.word = None;
                self.select(Rectangle::with_size(self.screen));
                self.start_translate()
            }
            Message::Frame(now) => {
                self.esc_back = false;
                // Shown fully clear: once its first full frame is up (or after
                // REVEAL_WAIT), the window fades in and the opening's clock starts.
                let mut revealed = Task::none();
                if let Some(since) = self.reveal {
                    if self.first_draw.get().is_some()
                        && now.saturating_duration_since(since) < REVEAL_WAIT
                    {
                        return Task::none();
                    }
                    self.reveal = None;
                    self.appear = None;
                    self.frame_times.borrow_mut().clear();
                    revealed = windows::fade(self.overlay, true, windows::POPUP, false);
                }
                // a new result card grows open from its first frame
                self.card_open = match (&self.panel, self.card_open) {
                    (Some((id, _)), Some((was, shown, _))) if *id == was => {
                        Some((was, shown, shown.at(now)))
                    }
                    (Some((id, _)), _) => {
                        let shown = motion::Shown::opening(motion::WIDGET, now);
                        Some((*id, shown, shown.at(now)))
                    }
                    (None, _) => None,
                };
                let opening = self.appearing() || self.appear.is_none();
                let gliding = self.bar_glide.is_some();
                // every motion by the clock since it began (`motion`)
                self.appear = match self.appear {
                    Some((start, t)) if t < 1.0 => {
                        Some((start, motion::done(start, now, overlay::APPEAR)))
                    }
                    None => Some((now, motion::done(now, now, overlay::APPEAR))),
                    done => done,
                };
                let (target, was) = (self.swap_target(), self.swap);
                // to the toolbar (enter) or back to the bar (exit), from where it is
                self.swap_at = match self.swap_at {
                    Some(m @ (_, _, to)) if to == target => Some(m),
                    _ if self.swap == target => None,
                    _ => Some((now, self.swap, target)),
                };
                if let Some((began, from, to)) = self.swap_at {
                    let length = BAR_MORPH; // the design: the same rise both ways
                    let t = match self.purpose {
                        Purpose::Search => motion::done(began, now, length),
                        _ => 1.0, // Snip has no bar to turn into the toolbar
                    };
                    self.swap = from + (to - from) * t;
                    if t >= 1.0 {
                        self.swap_at = None;
                    }
                }
                if self.bar_drag.is_none() {
                    let to = self.bar_target();
                    self.bar_now = Some(match (self.bar_now, self.bar_glide) {
                        // turning into the toolbar (or back): it stays on its spot
                        _ if self.swap != was => to,
                        (Some(at), _) if at == to => to,
                        // on its way there already
                        (Some(_), Some((began, from, end))) if end == to => {
                            let t = motion::progress(began, now, GLIDE, motion::Curve::Standard);
                            from + (to - from) * t
                        }
                        // a new place: it glides there from where it is
                        (Some(at), _) => {
                            self.bar_glide = Some((now, at, to));
                            at
                        }
                        (None, _) => to,
                    });
                    if self.bar_now == Some(to) {
                        self.bar_glide = None;
                    }
                }
                if gliding && self.bar_glide.is_none() && !opening {
                    self.log_frames("bar glide");
                }
                if !opening || self.appearing() {
                    return revealed;
                }
                // The opening animation is over: its frame times to the log
                // (the VM's budget), then the text reader's helper starts.
                self.log_frames("opening animation");
                Task::batch([revealed, self.warm()])
            }
            Message::ScreenText(opening, band, read)
                if opening == self.opening && self.frame.is_some() =>
            {
                self.reading = None;
                match read {
                    Ok(lines) => {
                        let system = self.text_by == Some(ocr::Source::System);
                        if system {
                            // Windows OCR's closer read replaces its quick one
                            self.screen_text = word::Screen::default();
                        }
                        let first = self.screen_text == word::Screen::default();
                        self.screen_text.add(band, lines, self.screen);
                        // (the helper: its first band and its last, the VM's timings)
                        let whole = self.screen_text.is_whole(self.screen.height);
                        if system || first || whole {
                            let what = match system || whole {
                                true => "screen text ready",
                                false => "first screen text",
                            };
                            match self.appear {
                                Some((at, _)) => crate::log!(
                                    "[opencircle] {what} {} ms after the first frame",
                                    at.elapsed().as_millis()
                                ),
                                None => crate::log!("[opencircle] {what} before the first frame"),
                            }
                        }
                        let done = self.more_text();
                        let released = self.release_held();
                        Task::batch([done, released, self.read_more()])
                    }
                    // Windows OCR couldn't (no recognizer, #63): the helper reads
                    Err(e) if self.text_by == Some(ocr::Source::System) => {
                        crate::log!("[opencircle] Windows OCR didn't read the screen: {e}");
                        self.text_by = Some(ocr::Source::Helper);
                        self.note_reader();
                        let released = self.release_held();
                        Task::batch([released, self.warm(), self.read_more()])
                    }
                    // (the next press tries again; the helper backs off by itself)
                    Err(e) => {
                        crate::log!("[opencircle] reading the screen's text failed: {e}");
                        self.release_held()
                    }
                }
            }
            Message::ScreenText(..) => Task::none(), // closed or opened again meanwhile
            // Off Windows (no Windows OCR, #62) the whole screen is read now,
            // gently, band by band from the pointer out: the I-beam shows over
            // text before any press, as it does on Windows.
            Message::Warmed(opening) if opening == self.opening && !cfg!(windows) => {
                self.text_used = true;
                self.read_more()
            }
            Message::Warmed(_) => Task::none(),
            // Only on this click: nothing is downloaded without it.
            Message::ReaderDownload if self.frame.is_some() => {
                self.reader = Reader::Downloading(0.0);
                if cfg!(test) {
                    return Task::none(); // never the network
                }
                // ponytail: closed mid-download, the download goes on unseen; a
                // click on the next opening's note starts a second one (the
                // checksums catch a clash). Share one download if that shows up.
                let opening = self.opening;
                background(|tx| {
                    let progress = |p| {
                        let _ = tx.unbounded_send(Ok(p));
                    };
                    let done = text::download(progress).map(|()| text::Progress::Reading);
                    let _ = tx.unbounded_send(done);
                })
                .map(move |step| Message::Reader(opening, step))
            }
            Message::Reader(opening, step) if opening == self.opening && self.frame.is_some() => {
                match step {
                    Ok(text::Progress::Download(done)) => self.reader = Reader::Downloading(done),
                    Ok(text::Progress::Reading) => {
                        (self.ocr_ready, self.reader) = (text::models_present(), Reader::Idle);
                        self.note_reader();
                        return self.warm();
                    }
                    Err(e) => self.reader = Reader::Failed(e),
                }
                Task::none()
            }
            Message::ReaderDownload | Message::Reader(..) => Task::none(),
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
            let shot = match snip {
                Some(kind) => Self::snip_shot(kind),
                // The monitor of the window the user is in (else the pointer's).
                None => capture::active_monitor().map(|frame| {
                    let (place, image) = snip::screen_of(frame);
                    Shot {
                        origin: place.logical_origin(),
                        size: place.size(),
                        image,
                        scale: place.scale,
                        snip: None,
                    }
                }),
            };
            let _ = tx.unbounded_send(shot);
        })
        .map(move |shot| Message::Captured(n, at, shot));
        Task::batch([shot, timeout])
    }

    /// Snip: every monitor at once (the overlay shows the pointer's, then follows
    /// the pointer) and every window, while the overlay is still hidden.
    fn snip_shot(kind: snip::Kind) -> Result<Shot, String> {
        let (mut frames, on) = capture::every_monitor()?;
        if config::current().snip.cursor {
            frames.iter_mut().for_each(capture::cursor::draw);
        }
        let foreground = capture::window_rect::foreground();
        let windows = (capture::window_rect::list().into_iter())
            .map(|w| snip::Pick::window(snip::desktop_rect(w.rect), &w.title, w.handle))
            .collect();
        let screens: Vec<_> = frames.into_iter().map(snip::screen_of).collect();
        let (place, image) = screens.get(on).cloned().ok_or("no monitor found")?;
        Ok(Shot {
            origin: place.logical_origin(),
            size: place.size(),
            image,
            scale: place.scale,
            snip: Some((kind, windows, foreground, screens)),
        })
    }

    fn open(&mut self, at: Instant, shot: Result<Shot, String>) -> Task<Message> {
        self.capturing = None;
        let shot = match shot {
            Ok(shot) => shot,
            Err(e) => {
                crate::log!("capture failed: {e}");
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
        crate::log!(
            "[opencircle] capture {}x{} took {} ms",
            shot.size.width * shot.scale,
            shot.size.height * shot.scale,
            at.elapsed().as_millis()
        );
        let size = shot.size;
        self.set_frame(shot.image, shot.scale, shot.origin, size);
        self.helper = Some(Arc::new(ocr::hold()));
        self.ocr_ready = text::models_present();
        self.frame_times.borrow_mut().clear();
        self.opening = self.opening.wrapping_add(1);
        self.first_draw.set(Some((at, size)));
        self.hiding = Hiding::No;
        let cfg = config::current();
        self.appear = None; // its first frame is t = 0
        self.pill = BarButton::Search;
        self.bar_anchor = cfg.bar.anchor;
        self.bar_now = None;
        self.bar_glide = None;
        motion::refresh();
        self.behaviour = cfg.selection;
        self.bar_keys = BarKeys::of(&cfg);
        self.labs = cfg.labs;
        let mut first = None;
        let mut extras = Task::none();
        let mut timer = None;
        if let Some((kind, windows, foreground, screens)) = shot.snip {
            let mut snip = Snip::new(windows, self.place(), snip::can_scroll(), foreground);
            snip.set_screens(screens);
            first = snip.start(kind, size); // Full = the whole screen at once
            let wait = config::current().snip.delay;
            if !std::mem::take(&mut self.timed) && wait > 0 && first.is_none() {
                timer = Some(wait); // Settings > Snip > Timer: hide, count down, capture again
            }
            let (reader, read) = std::sync::mpsc::channel();
            if cfg!(windows) {
                snip.set_reader(reader, UIA_WINDOWS); // else dropped: no controls
            }
            self.purpose = Purpose::Snip(Box::new(snip));
            extras = self.snip_extras(read);
        }
        let Purpose::Quick(quick, target) = self.purpose else {
            let next = match (first, timer) {
                (Some(next), _) => self.snip_next(Some(next)),
                (None, Some(secs)) => self.on_snip(snip::Msg::Delay(secs)),
                (None, None) => Task::none(),
            };
            if self.snip_pill() {
                return Task::batch([next, extras]); // the toast is the window
            }
            self.note_reader();
            // Windows OCR reads the whole screen while the overlay comes up
            // (#62: ready ~0.1 s after its first frame); the helper waits (#123).
            let text = match self.system_text() {
                true => self.read_more(),
                false => Task::none(),
            };
            return Task::batch([next.chain(self.show()), extras, text]);
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

    /// The frozen frame of the monitor at `origin` (logical px, `size`) becomes
    /// what the overlay shows.
    fn set_frame(&mut self, image: image::Handle, scale: f32, origin: Point, size: Size) {
        let px = |v: f32| (v * scale).round() as u32;
        self.view = overlay::View::new(px(size.width), px(size.height), size);
        self.tiles = overlay::tiles(&image).map(|t| (image.id(), t));
        self.frame = Some(image);
        glass::set_frame(self.frame.as_ref(), scale);
        self.scale = scale;
        self.screen = size;
        self.origin = origin;
        self.work_inset = taskbar_inset(origin, size, scale);
    }

    /// The shown monitor in physical desktop px.
    fn place(&self) -> snip::Place {
        let px = |v: f32| (v * self.scale).round();
        snip::Place {
            origin: (px(self.origin.x) as i32, px(self.origin.y) as i32),
            px: (px(self.screen.width) as u32, px(self.screen.height) as u32),
            scale: self.scale,
        }
    }

    /// Shows the (hidden or pill-sized) window over the whole monitor, or opens
    /// it full-screen where it was closed (see `can_hide_windows`).
    fn show(&mut self) -> Task<Message> {
        if !self.overlay_open {
            let (id, open) = windows::screen(overlay_window(Some(self.screen)));
            crate::log!("[opencircle] overlay window opened");
            (self.overlay, self.overlay_open) = (id, true);
            return open.discard().chain(window::gain_focus(id));
        }
        // Shown fully clear: it fades in as a whole once its first full frame
        // is drawn (`Message::Frame`), so the CPU never redraws a fade.
        let clear = match shape::can_fade() {
            true => {
                self.reveal = Some(Instant::now());
                if self.first_draw.get().is_none() {
                    self.first_draw.set(Some((Instant::now(), self.screen)));
                }
                windows::opacity(self.overlay, 0.0)
            }
            false => Task::none(),
        };
        // Place and size the window to cover the monitor BEFORE set_mode shows it:
        // iced makes it visible before going full-screen, so whatever is shown
        // first must already cover the monitor (no small black box flash).
        let show = windows::uncut(self.overlay)
            .map(Message::Windows)
            .chain(clear)
            .chain(place_window(self.overlay, self.place()))
            .chain(window::set_mode(self.overlay, Mode::Fullscreen))
            .chain(window::gain_focus(self.overlay));
        // A hand-off to the browser drops the overlay out of the topmost band
        // behind iced's back (`lens::hand_off_foreground`): Normal then
        // AlwaysOnTop makes it apply again.
        #[cfg(windows)]
        let show = window::set_level(self.overlay, Level::Normal)
            .chain(window::set_level(self.overlay, Level::AlwaysOnTop))
            .chain(show);
        show
    }

    fn on_overlay(&mut self, event: overlay::Event) -> Task<Message> {
        if let overlay::Event::Alt(alt) = event {
            self.alt = alt;
            return Task::none();
        }
        // A drag let go before the band under its press was read waits for
        // it, with the keys after it (Ctrl+C); a new press goes on without.
        if let Some(held) = &mut self.held {
            match event {
                overlay::Event::Press { .. } => {
                    let released = self.release_held();
                    return Task::batch([released, self.on_overlay(event)]);
                }
                overlay::Event::Hover(_) | overlay::Event::Move(_) => {}
                _ => {
                    held.push(event);
                    return Task::none();
                }
            }
        }
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
            overlay::Event::Press { at, shift } => {
                self.bar_focus = None; // a click puts the focus back on the overlay
                overlay::Event::Press {
                    at: self.view.to_image(at),
                    shift,
                }
            }
            overlay::Event::Move(at) => overlay::Event::Move(self.view.to_image(at)),
            overlay::Event::Hover(at) => overlay::Event::Hover(self.view.to_image(at)),
            event => event,
        };
        // Snip: the loupe follows the pointer; a drag's ends snap to edges.
        let event = match (&mut self.purpose, event) {
            (Purpose::Snip(snip), overlay::Event::Press { at, shift }) => {
                snip.hover(at);
                overlay::Event::Press {
                    at: snip.press(at),
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
                overlay::Event::Key { .. } | overlay::Event::Enter | overlay::Event::Space => {
                    return Task::none();
                }
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
                let (area, unit) = (self.area(), self.click(p));
                if let Some(mode) = &mut self.text_mode {
                    let lines = mode.lines.clone().unwrap_or_default();
                    mode.picked = word::Picked::new(lines, area, p, unit);
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
                self.pointer = p;
                // A new selection: the panel and the edits were about the old one.
                self.panel = None;
                self.text_mode = None;
                self.morph = None;
                self.edit = None;
                self.scan = None;
                self.taps = self.taps.wrapping_add(1); // a tap still reading is stale
                let unit = self.click(p);
                if !shift && let Some(grab) = self.grab_at(p) {
                    self.grab = Some(grab);
                    return Task::none();
                }
                // On text (once the screen is read): selected like in a document.
                // Shift+click stretches the text selection there.
                if shift
                    && self.text_at(p, unit).is_some()
                    && let Some(word) = &mut self.word
                    && word.extend(p)
                {
                    self.select_word();
                    self.text_press = Some(vec![p]);
                    return Task::none();
                }
                if !shift && let Some(word) = self.text_at(p, unit) {
                    self.word = Some(word);
                    self.text_used = true;
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
                // perhaps on text not read yet: its band now
                return self.read_more();
            }
            overlay::Event::Move(p) if self.text_press.is_some() => return self.drag_text(p),
            overlay::Event::Move(p) if self.grab.is_some() => self.drag_grab(p),
            overlay::Event::Move(p) if self.drawing => self.stroke.push(p),
            overlay::Event::Move(_) => {}
            overlay::Event::Hover(p) => {
                self.pointer = p;
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
            overlay::Event::Release if self.text_press.take().is_some() => {
                if self.word.as_ref().is_some_and(word::Picked::is_empty) {
                    self.word = None; // a click on text: nothing selected
                }
                return self.selected();
            }
            overlay::Event::Release if let Some(grab) = self.grab.take() => {
                return if grab.moved {
                    // The search first: a waiting quick action (`selected`)
                    // turns the purpose back to Search, and must not get it too.
                    let search = self.auto_search();
                    Task::batch([self.selected(), search])
                } else {
                    self.tap(grab.from) // a click on the selection
                };
            }
            // A drag that may have begun on text not read yet: the band under
            // its press decides (text or a stroke) once read (`release_held`).
            overlay::Event::Release
                if !self.adding
                    && let Some(&start) = self.stroke.first()
                    && self.reading.is_some_and(|band| band.contains(start))
                    && !self.screen_text.knows(start.y)
                    && !overlay::is_tap(&self.stroke, self.view.zoom()) =>
            {
                self.held = Some(Vec::new());
            }
            overlay::Event::Release => {
                self.drawing = false;
                let mut settle = Task::none();
                let zoom = self.view.zoom();
                let mut settled = overlay::settle(&self.stroke, zoom, self.screen);
                // Snap: the stroke becomes a rounded box (it morphs there).
                if let Some((sel, kept)) = &mut settled
                    && self.snaps()
                {
                    if sel.kind != overlay::Kind::Drag {
                        self.morph = Some((kept.clone(), Instant::now()));
                        settle = background(|tx| {
                            std::thread::sleep(overlay::MORPH + Duration::from_millis(20));
                            let _ = tx.unbounded_send(());
                        })
                        .map(|()| Message::Settled);
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
                return Task::batch([self.selected(), search, settle]);
            }
            // Tab moved the focus onto a bar button: Enter / Space press it.
            overlay::Event::Enter | overlay::Event::Space
                if let Some(stop) = self.bar_focus
                    && self.bottom_bar_shown() =>
            {
                return self.activate(stop);
            }
            overlay::Event::Tab { back } if self.bottom_bar_shown() => return self.tab_bar(back),
            overlay::Event::Enter if matches!(self.purpose, Purpose::Search) => {
                return self.run(actions::default_for(&config::current()));
            }
            // Ctrl+A: all the text on the screen.
            overlay::Event::Key {
                letter: 'a',
                ctrl: true,
            } if self.reads_text() => {
                // all of it once read (the rest is read now, nearest the pointer first)
                self.want_all = true;
                self.text_used = true;
                let done = self.more_text();
                return Task::batch([done, self.read_more()]);
            }
            overlay::Event::Key { letter, ctrl } if matches!(self.purpose, Purpose::Search) => {
                // Nothing selected: Ask AI, Translate and Edit take the whole screen
                // (as the bottom bar's buttons do).
                let bar = match actions::for_key(&self.labs, letter, ctrl).map(|a| a.id) {
                    Some(id) if id == actions::ask::ACTION.id => Some(Message::AskButton),
                    Some(id) if id == actions::translate::ACTION.id => {
                        Some(Message::TranslateScreen)
                    }
                    Some(id) if id == actions::edit::ACTION.id => Some(Message::EditScreen),
                    _ => None,
                };
                if let (None, Some(message)) = (self.selection, bar) {
                    return self.update(message);
                }
                if let Some(action) = actions::for_key(&self.labs, letter, ctrl) {
                    return self.run(action);
                }
            }
            // Snip: Ctrl+C copies what a click would capture here (the picked area,
            // else the window or control outlined under the pointer), whatever
            // `snip.after` says.
            overlay::Event::Key {
                letter: 'c',
                ctrl: true,
            } if matches!(self.purpose, Purpose::Snip(_)) => {
                let Purpose::Snip(snip) = &self.purpose else {
                    return Task::none();
                };
                let at = snip.pointer.unwrap_or(Point::ORIGIN);
                let area = self.selection.map(|s| s.area);
                let Some(snip::Next::Edit(area)) = snip.pick(area, at, self.screen) else {
                    return Task::none();
                };
                self.select(area);
                let Some(crop) = self.crop() else {
                    return Task::none();
                };
                return self.snip_routed(crop, true, (true, false));
            }
            overlay::Event::Arrow {
                dx,
                dy,
                ctrl,
                shift,
            } => self.nudge(dx, dy, ctrl, shift),
            overlay::Event::Enter
            | overlay::Event::Space
            | overlay::Event::Tab { .. }
            | overlay::Event::Alt(_)
            | overlay::Event::Key { .. } => {}
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
            crate::log!("QR chip failed: {e}");
            "That didn't work"
        };
        match act {
            qr::Act::Copy => background(move |tx| {
                let pill = copy::text(text).map_or_else(failed, |()| "QR text copied");
                let _ = tx.unbounded_send(Message::ThenClose(Some(pill)));
            }),
            qr::Act::Open => match qr::web_url(&text).map(str::to_string) {
                // the browser has the screen now
                Some(url) => background(move |tx| {
                    let opened = crate::lens::open_link(&url);
                    let pill = opened.err().map(|e| failed(e.to_string()));
                    let _ = tx.unbounded_send(Message::ThenClose(pill));
                }),
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
                    .toast(&update_card::ready_toast(&staged.version), true, None, 5000)
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
                crate::log!("[opencircle] update {}: {e}", staged.version);
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
            && self.edit.is_none()
            && self.translate.is_none()
    }

    /// A press at `p` (image px) on the selection: a corner or an edge resizes
    /// it; inside moves it, unless the press is on text (that selects text).
    fn grab_at(&self, p: Point) -> Option<Grab> {
        // Selected text is not a box: a press on it starts a new selection.
        let sel = (self.selection).filter(|s| self.handles() && s.kind != overlay::Kind::Word)?;
        let on_screen = self.view.rect_to_screen(sel.area);
        let grip = overlay::grip_at(sel.kind, on_screen, self.view.to_screen(p));
        let inside = sel.area.contains(p)
            && (self.alt
                || self.text_mode.is_none() && self.text_at(p, word::Unit::Caret).is_none());
        (grip.is_some() || inside).then(|| Grab {
            grip,
            from: p,
            start: (sel, self.stroke.clone()),
            moved: false,
        })
    }

    /// Resizes / moves the grabbed selection to the pointer at `p` (image px),
    /// never off the screen. A freehand loop scales with its box; anything
    /// else becomes that box.
    fn drag_grab(&mut self, p: Point) {
        let Some(grab) = &mut self.grab else { return };
        let zoom = self.view.zoom();
        grab.moved |= !overlay::is_tap(&[grab.from, p], zoom);
        if !grab.moved {
            return;
        }
        let (start, stroke) = &grab.start;
        let min = overlay::MIN_SIDE / zoom;
        let delta = p - grab.from;
        let on = |to: Point| {
            Point::new(
                to.x.clamp(0.0, self.screen.width),
                to.y.clamp(0.0, self.screen.height),
            )
        };
        let area = match grab.grip {
            // the corner follows the pointer from where it was grabbed (a bracket arm is off it)
            Some(overlay::Grip::Corner(c)) => overlay::resize(
                start.area,
                c,
                on(overlay::corner(start.area, c) + delta),
                min,
            ),
            // (an edge's coordinate is its first corner's)
            Some(overlay::Grip::Edge(e)) => overlay::resize_edge(
                start.area,
                e,
                on(overlay::corner(start.area, e) + delta),
                min,
            ),
            None => overlay::keep_inside(start.area + delta, self.screen),
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

    /// An arrow key on the box (`overlay::Event::Arrow`): 1 px (Shift: 10) on
    /// the overlay, kept on the screen and over the minimum size.
    fn nudge(&mut self, dx: i8, dy: i8, resize: bool, big: bool) {
        let Some(sel) = self
            .selection
            .filter(|s| self.handles() && s.kind != overlay::Kind::Word)
        else {
            return;
        };
        let one = self.view.to_image(Point::new(1.0, 1.0)) - self.view.to_image(Point::ORIGIN);
        let by = Vector::new(dx as f32 * one.x, dy as f32 * one.y) * if big { 10.0 } else { 1.0 };
        let min = overlay::MIN_SIDE / self.view.zoom();
        let a = sel.area;
        let area = if resize {
            let fit = |len: f32, at: f32, max: f32| (len).min(max - at).max(min);
            Rectangle::new(
                a.position(),
                Size::new(
                    fit(a.width + by.x, a.x, self.screen.width),
                    fit(a.height + by.y, a.y, self.screen.height),
                ),
            )
        } else {
            overlay::keep_inside(a + by, self.screen)
        };
        self.morph = None;
        self.panel = None;
        self.text_mode = None;
        self.scan = None;
        self.stroke = overlay::refit(&self.stroke, a, area);
        self.selection = Some(overlay::Selection { area, ..sel });
        self.word = None;
        self.remember_region();
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
        if self.reads_text() {
            if self.screen_text.knows(at.y) {
                return Task::none(); // read: no text here
            }
            // the word there once its band is read (a triple tap: the line)
            let unit = match self.clicks {
                Some((_, _, n)) if n >= 3 => word::Unit::Line,
                _ => word::Unit::Word,
            };
            self.awaits = Some((at, Await::Tap(unit)));
            return self.read_more();
        }
        // A click on a selection keeps it unless a word is found there.
        self.read_words(word::region(at, self.screen), at, false)
    }

    /// A press at `p` (image px) on the screen's text read there (main mode
    /// only): the caret there, or its word or line (`unit`).
    fn text_at(&self, p: Point, unit: word::Unit) -> Option<word::Picked> {
        let main = matches!(self.purpose, Purpose::Search) && self.translate.is_none();
        let all = Rectangle::with_size(self.screen);
        let read = main && self.screen_text.knows(p.y);
        read.then(|| word::Picked::new(self.screen_text.lines(), all, p, unit))?
    }

    /// Counts a press at `p` (image px) as the first, second or third click in
    /// a row (each within `DOUBLE_CLICK` of the last and near it): what it
    /// selects on text.
    fn click(&mut self, p: Point) -> word::Unit {
        let now = Instant::now();
        let n = match self.clicks {
            Some((at, last, n)) if now - at <= DOUBLE_CLICK && last.distance(p) <= 4.0 => n + 1,
            _ => 1,
        };
        self.clicks = Some((now, p, n));
        match n {
            1 => word::Unit::Caret,
            2 => word::Unit::Word,
            _ => word::Unit::Line,
        }
    }

    /// A press that began on text moved to `p` (image px): the words stretch
    /// there, unless the stroke loops, which makes it a circle after all.
    /// Into rows not read yet: they are read first (`read_more`).
    fn drag_text(&mut self, p: Point) -> Task<Message> {
        let Some(stroke) = &mut self.text_press else {
            return Task::none();
        };
        stroke.push(p);
        if overlay::winds(stroke, self.view.zoom()) {
            self.stroke = self.text_press.take().unwrap_or_default();
            self.drawing = true;
            self.word = None;
            self.selection = None;
            return Task::none();
        }
        if let Some(word) = &mut self.word
            && word.extend(p)
        {
            self.select_word();
        }
        self.read_more()
    }

    /// The screen's text has just been read while a press drew: one that
    /// began on text becomes a text selection to where the pointer is now
    /// (a loop stays a circle, `drag_text`).
    fn drag_became_text(&mut self) {
        let Some(&start) = self.stroke.first().filter(|_| self.drawing && !self.adding) else {
            return;
        };
        let Some(word) = self.text_at(start, word::Unit::Caret) else {
            return;
        };
        let stroke = std::mem::take(&mut self.stroke);
        self.drawing = false;
        self.word = Some(word);
        self.text_used = true;
        self.select_word();
        self.text_press = Some(vec![start]);
        for &p in &stroke[1..] {
            match self.text_press {
                Some(_) => _ = self.drag_text(p), // (the caller reads on)
                None => self.stroke.push(p),      // it looped: a circle after all
            }
        }
    }

    /// Main mode without the OCR models: the note over the bar says so and
    /// offers them (`Reader::Missing`). Nothing is read at open (#123).
    fn note_reader(&mut self) {
        if matches!(self.purpose, Purpose::Search) && !self.ocr_ready && !self.system_text() {
            self.reader = Reader::Missing(ocr::download_size());
        }
    }

    /// Main mode's text can be read: Windows OCR or the OCR models here, no
    /// translation shown.
    fn reads_text(&self) -> bool {
        (self.ocr_ready || self.system_text())
            && self.frame.is_some()
            && matches!(self.purpose, Purpose::Search)
            && self.translate.is_none()
    }

    /// Windows OCR reads this opening's text (#62): decided at its first read,
    /// until then what `ocr::route` says now.
    fn system_text(&self) -> bool {
        let route = || ocr::route(self.behaviour.text_reader, ocr::system::known());
        self.text_by.unwrap_or_else(route) == ocr::Source::System
    }

    /// After the opening animation: Windows OCR reads the whole screen (#62)
    /// if it didn't start at the capture, or the held helper starts and loads its models (gently), so the first
    /// text read is quick. Nothing during it.
    fn warm(&mut self) -> Task<Message> {
        if !self.reads_text() {
            return Task::none();
        }
        if self.system_text() {
            return self.read_more();
        }
        let held = self.helper.as_ref().map(Arc::downgrade).unwrap_or_default();
        let opening = self.opening;
        background(move |tx| {
            // after the bar has settled too; closed meanwhile: nothing starts
            std::thread::sleep(WARM_AFTER);
            if let Some(_held) = held.upgrade()
                && let Err(e) = ocr::warm()
            {
                crate::log!("[opencircle] the text reader didn't start: {e}");
            }
            let _ = tx.unbounded_send(Message::Warmed(opening));
        })
    }

    /// Main mode: reads the next band of the screen's text, if none is being
    /// read: where the user touches text (a press, a drag going on into
    /// unread rows, a tap, a Shift+click) first; then, once text was used
    /// (or Ctrl+A waits), the rest, nearest the pointer first.
    fn read_more(&mut self) -> Task<Message> {
        if self.reading.is_some() || !self.reads_text() {
            return Task::none();
        }
        // Windows OCR: the whole screen at once, the first time (from the
        // capture, `open`; else after the opening animation, `warm`)
        if self.system_text() {
            if self.text_by.is_some() {
                return Task::none(); // read (or being read)
            }
            self.text_by = Some(ocr::Source::System);
            return self.read_band(Rectangle::with_size(self.screen), false);
        }
        self.text_by = Some(ocr::Source::Helper);
        let text = &self.screen_text;
        let pressed = (self.drawing && !self.adding)
            .then(|| self.stroke.first().copied())
            .flatten();
        let dragged = self.text_press.as_ref().and_then(|s| s.last().copied());
        let touched = [dragged, pressed, self.awaits.map(|(p, _)| p)]
            .into_iter()
            .flatten()
            .find(|p| !text.knows(p.y));
        let (band, gently) = match touched {
            Some(p) => (word::Screen::band(p.y, self.screen), false),
            None if self.want_all || self.text_used => {
                match text.next(self.pointer.y, self.screen) {
                    Some(band) => (band, !self.want_all), // (Ctrl+A: the user waits)
                    None => return Task::none(),
                }
            }
            None => return Task::none(),
        };
        self.read_band(band, gently)
    }

    /// OCR reads `band` (image px) of the frame on a thread: at once where the
    /// user waits for it, else gently (the helper on at most two cores, below
    /// normal priority).
    fn read_band(&mut self, band: Rectangle, gently: bool) -> Task<Message> {
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.frame
        else {
            return Task::none();
        };
        let (width, height, pixels) = (*width, *height, pixels.clone());
        let Some([x0, y0, ..]) = actions::crop_bounds(width, height, self.scale, band) else {
            return Task::none();
        };
        // only the lines it is read for (frame px in the crop)
        let Some((top, bottom)) = self.screen_text.unread(band, self.screen) else {
            return Task::none();
        };
        let at = |y: f32| ((y * self.scale) as u32).saturating_sub(y0);
        let rows = (at(top), at(bottom));
        self.reading = Some(band);
        let (scale, opening) = (self.scale, self.opening);
        let held = self.helper.as_ref().map(Arc::downgrade).unwrap_or_default();
        let system =
            (self.text_by == Some(ocr::Source::System)).then_some(self.behaviour.text_reader);
        background(move |tx| {
            // closed meanwhile: nothing to read for (and no helper started)
            let Some(_held) = held.upgrade() else {
                return;
            };
            let send = |read: Result<Vec<TextBlock>, ocr::OcrError>| {
                let lines = read.map(|blocks| {
                    let mut lines = word::lines(blocks, [x0, y0], scale);
                    word::measure(&mut lines, &pixels, width, height, scale);
                    lines
                });
                let _ = tx.unbounded_send(lines.map_err(|e| e.to_string()));
            };
            let Some(c) = actions::crop(&pixels, width, height, scale, band) else {
                return send(Err(ocr::OcrError::BadImage("an empty band".into())));
            };
            let Some(setting) = system else {
                return send(ocr::recognize_band(
                    &c.rgba, c.width, c.height, rows, gently,
                ));
            };
            // its language known now: a right-to-left one goes to the helper
            if ocr::route(setting, Some(ocr::system::language())) == ocr::Source::Helper {
                let rtl = "Windows OCR: a right-to-left language";
                return send(Err(ocr::OcrError::Engine(rtl.into())));
            }
            // fast, then (small text) doubled: each read replaces the last
            for (i, &k) in ocr::system::passes(scale, c.width, c.height)
                .iter()
                .enumerate()
            {
                match ocr::system::recognize(&c.rgba, c.width, c.height, k) {
                    Err(e) if i > 0 => crate::log!("[opencircle] Windows OCR's second read: {e}"),
                    read => send(read),
                }
            }
        })
        .map(move |read| Message::ScreenText(opening, band, read))
    }

    /// The held release (and the events after it) go on, now that the band
    /// under the press is read (or failed): a text selection or a stroke.
    fn release_held(&mut self) -> Task<Message> {
        let Some(events) = self.held.take() else {
            return Task::none();
        };
        let mut tasks = vec![self.on_overlay(overlay::Event::Release)];
        tasks.extend(events.into_iter().map(|e| self.on_overlay(e)));
        Task::batch(tasks)
    }

    /// More of the screen's text was read: a text selection takes in the new
    /// lines (a drag can go on into them), a press that began on text becomes
    /// a text selection, and a tap, Shift+click or Ctrl+A waiting for it is
    /// done.
    fn more_text(&mut self) -> Task<Message> {
        let all = Rectangle::with_size(self.screen);
        if let Some(word) = &mut self.word {
            word.reread(self.screen_text.lines(), all);
            if let Some(&p) = self.text_press.as_ref().and_then(|s| s.last()) {
                word.extend(p);
            }
            self.select_word();
        }
        self.drag_became_text();
        let mut done = Task::none();
        if let Some((at, wait)) = self.awaits
            && self.screen_text.knows(at.y)
        {
            self.awaits = None;
            match wait {
                Await::Tap(unit) => {
                    if let Some(word) = self.text_at(at, unit) {
                        self.word = Some(word);
                        self.text_used = true;
                        self.select_word();
                        done = self.selected();
                    }
                }
                Await::Extend => {
                    if let Some(word) = &mut self.word
                        && word.extend(at)
                    {
                        self.select_word();
                        done = self.selected();
                    }
                }
            }
        }
        if self.want_all && self.screen_text.is_whole(self.screen.height) {
            self.want_all = false;
            self.word = word::Picked::all(self.screen_text.lines(), all);
            self.select_word();
            done = self.selected();
        }
        done
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
        if self.word.is_some() && self.reads_text() && !self.screen_text.knows(at.y) {
            self.awaits = Some((at, Await::Extend));
            return self.read_more();
        }
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
                crate::log!("[opencircle] reading the tapped word failed: {e}");
                return Task::none();
            }
        };
        let mut lines = word::lines(blocks, what.origin, self.scale);
        if let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.frame
        {
            word::measure(&mut lines, pixels, *width, *height, self.scale);
        }
        if what.extend {
            let Some(word) = &mut self.word else {
                return Task::none();
            };
            word.reread(lines, what.region);
            word.extend(what.at);
        } else {
            let Some(picked) = word::Picked::new(lines, what.region, what.at, word::Unit::Word)
            else {
                return Task::none(); // no text there
            };
            self.word = Some(picked);
        }
        self.select_word();
        self.selected()
    }

    /// The selected text as the (only) selection: a flat highlight per line
    /// (the stroke holds a top-left / bottom-right pair for each). Nothing
    /// selected yet (a caret): no selection.
    fn select_word(&mut self) {
        self.others.clear();
        self.selection = None;
        self.stroke.clear();
        if let Some(word) = self.word.as_ref().filter(|w| !w.is_empty()) {
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
        // (after the snap: the Settled message runs them)
        if !matches!(self.purpose, Purpose::Search)
            || word
            || self.selection.is_none()
            || self.settling()
        {
            return &[];
        }
        auto_targets(self.behaviour)
    }

    /// Words picked in a snapped box (select-text mode), not still being dragged.
    fn picked_in_box(&self) -> bool {
        (self.text_mode.as_ref()).is_some_and(|m| m.picked.is_some() && !m.picking)
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
        let (scale, frame) = (self.scale, (*width, *height, pixels.clone()));
        background(move |tx| {
            let read = ocr::recognize_in_child(&crop.rgba, crop.width, crop.height);
            let lines = read.map(|blocks| {
                let mut lines = word::lines(blocks, [x0, y0], scale);
                word::measure(&mut lines, &frame.2, frame.0, frame.1, scale);
                lines
            });
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
                crate::log!("[opencircle] select text failed: {e}");
                self.text_mode = None;
            }
        }
    }

    /// A press at `p` (image px) picks words: select-text mode is on, read, and
    /// `p` is in the box.
    fn picks_text_at(&self, p: Point) -> bool {
        self.text_mode.as_ref().is_some_and(|m| m.lines.is_some())
            && self.area().contains(p)
            && self.grab_at(p).is_none() // (a bracket or edge adjusts the box)
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
            let text = picked.text();
            return self.confirm_after(move || {
                (copy::text(text))
                    .map(|()| copy::COPIED_PILL.to_string())
                    .map_err(|e| {
                        crate::log!("copy text failed: {e}");
                        "Couldn't copy. Close what holds the clipboard and try again.".to_string()
                    })
            });
        }
        if act == PickAct::Search {
            return self.search_text(picked.text());
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
        self.start_translate()
    }

    /// Picked words (a word selection, or select-text mode's picks): their text.
    fn picked_text(&self) -> Option<String> {
        let picked = (self.text_mode.as_ref()).and_then(|m| m.picked.as_ref());
        picked.or(self.word.as_ref()).map(word::Picked::text)
    }

    /// Search in browser on a text selection: the text, as a Google search in
    /// the user's own browser (the user picked it; no web view). Like Lens,
    /// the overlay closes.
    fn search_text(&mut self, text: String) -> Task<Message> {
        let text = text.trim();
        if text.is_empty() {
            return Task::none();
        }
        let url = crate::ai::browser::search_url(text);
        let (text, crop) = (text.to_string(), self.crop());
        background(move |tx| {
            let pill = match crate::lens::open_link(&url) {
                Ok(()) => {
                    crate::search_history::keep("text", &text, &url, crop.as_ref());
                    None
                }
                Err(e) => {
                    crate::log!("[opencircle] text search not opened: {e}");
                    Some("Couldn't open your browser")
                }
            };
            let _ = tx.unbounded_send(Message::ThenClose(pill));
        })
    }

    /// A pill of the bottom bar: Search in browser and Copy text act on the
    /// selection (dimmed without one); Translate on it, else the whole screen.
    fn on_pill(&mut self, pill: BarButton) -> Task<Message> {
        let picked = self.picked_in_box();
        match pill {
            BarButton::Search => self.run(&lens::ACTION),
            BarButton::CopyText if picked || self.word.is_some() => self.on_picked(PickAct::Copy),
            BarButton::CopyText => self.run_quick(Quick::CopyText),
            BarButton::Translate if self.selection.is_none() => {
                self.update(Message::TranslateScreen)
            }
            BarButton::Translate if picked => self.on_picked(PickAct::Translate),
            BarButton::Translate => self.run(&actions::translate::ACTION),
            _ => Task::none(),
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
        // Search in browser on picked words: their text, not a picture of them.
        if action.id == lens::ACTION.id
            && self.edit.is_none()
            && let Some(text) = self.picked_text()
        {
            return self.search_text(text);
        }
        match action.run {
            Run::Edit => return self.open_editor(),
            Run::Translate => return self.start_translate(),
            // Picked words (a word selection, or select-text mode's picks): Copy
            // (the bar's or Ctrl+C) copies exactly their text.
            Run::Finish(_)
                if action.id == copy::ACTION.id
                    && self.edit.is_none()
                    && (self.word.is_some()
                        || (self.text_mode.as_ref()).is_some_and(|m| m.picked.is_some())) =>
            {
                return self.on_picked(PickAct::Copy);
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
        let id = action.id;
        self.confirm_after(move || {
            let result = finish(crop);
            if let Err(e) = &result {
                crate::log!("{id} failed: {e}");
            }
            result
        })
    }

    /// Closes the overlay and tells the user how Copy / Save went: the toast pill
    /// in its own window (bottom right, above the taskbar), so the screen is the
    /// user's again at once and the pill outlives the overlay. `work`
    /// (encoding, the clipboard, a file) runs on a thread: the pill comes when
    /// it is done (#123).
    fn confirm_after(
        &mut self,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) -> Task<Message> {
        let place = (self.origin, self.screen, self.scale);
        let done = background(move |tx| {
            let _ = tx.unbounded_send(work());
        })
        .map(move |result| Message::Confirm(place, result));
        Task::batch([self.close(), done])
    }

    /// The Copy / Save pill for an overlay at `place` (origin, size, scale).
    fn toast_for(
        &mut self,
        (origin, screen, scale): (Point, Size, f32),
        result: Result<String, String>,
    ) -> Task<Message> {
        let (text, ok) = match result {
            Ok(text) => (text, true),
            Err(text) => (text, false),
        };
        let size = windows::toast_size(&text, ok);
        let inset = taskbar_inset(origin, screen, scale);
        let at = Point::new(
            origin.x + screen.width - size.width - TOAST_MARGIN,
            origin.y + screen.height - size.height - TOAST_MARGIN - inset,
        );
        self.windows
            .toast(&text, ok, Some(at), windows::TOAST_MS)
            .map(Message::Windows)
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
                    let read = text::read(&crop, |_| {});
                    let copied = read.and_then(|t| match t.trim() {
                        "" => Err("no text found".into()),
                        _ => copy::text(t),
                    });
                    let _ = tx.unbounded_send(copied);
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
        if !self.labs.has(labs::Feature::Editor) {
            return Task::none();
        }
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
            let mut edit = Editing::in_place(crop, bounds, self.scale);
            // Scrolling capture needs the page under the area: Snip, in place.
            edit.editor.scroll = matches!(&self.purpose, Purpose::Snip(s) if s.can_scroll());
            self.edit = Some(edit);
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
            edit.add(crop, work_area(self.screen, self.work_inset));
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
                self.esc_back = true;
                Task::none()
            }
            Some(Output::Copy(crop)) => self.finish(&copy::ACTION, crop),
            Some(Output::Save(crop)) => self.finish(&save::ACTION, crop),
            Some(Output::Pin(_)) if !self.labs.has(labs::Feature::Pins) => Task::none(),
            Some(Output::Pin(crop)) => self.pin(crop),
            // Scrolling capture of the area under the editor.
            Some(Output::Scroll) => match self.selection {
                Some(selection) => {
                    self.edit = None;
                    self.start_scroll(selection.area)
                }
                None => Task::none(),
            },
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
            self.timed = true;
            return self.capture(Instant::now(), Some(kind));
        }
        let Purpose::Snip(snip) = &mut self.purpose else {
            return Task::none();
        };
        match msg {
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
                let visible = windows::hide_from_capture(self.overlay, false).map(Message::Windows);
                self.selection = None;
                self.stroke.clear();
                let next = match stitched {
                    Ok(crop) if crop.height > 0 => self.snip_captured(Some(crop), true),
                    Ok(_) => Task::none(),
                    Err(e) => {
                        crate::log!("scrolling capture failed: {e}");
                        Task::none()
                    }
                };
                if self.snip_pill() {
                    return visible.chain(next); // the toast keeps the window small
                }
                visible.chain(next).chain(self.show())
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
                    self.edit = Some(Editing::fitted(
                        toast.crop,
                        work_area(self.screen, self.work_inset),
                        self.scale,
                    ));
                }
                self.show()
            }
            snip::Msg::ToastDone(id) if snip.toast_showing(id) => self.close(),
            snip::Msg::CopyFailed(id) if snip.toast_showing(id) => self.on_snip(snip::Msg::Edit),
            snip::Msg::CopyFailed(_) => Task::none(),
            snip::Msg::ToastDone(_) => Task::none(),
            snip::Msg::Controls(opening, controls) => {
                if opening == self.opening {
                    snip.add_controls(controls);
                }
                Task::none()
            }
            // The pointer went to another monitor: the overlay goes with it (that
            // monitor's frame was frozen with this one), unless a drag, the
            // editor or a pill holds this one.
            snip::Msg::Left => {
                if self.drawing || self.edit.is_some() {
                    return Task::none();
                }
                let Some((place, image)) = capture::pointer().and_then(|p| snip.follow(p)) else {
                    return Task::none();
                };
                self.selection = None;
                self.stroke.clear();
                self.set_frame(image, place.scale, place.logical_origin(), place.size());
                // Full-screen stays on its monitor: windowed first.
                window::set_mode(self.overlay, Mode::Windowed).chain(self.show())
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
                crate::log!("[opencircle] recent capture unreadable: {e}");
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
        // never left clear by an opening that hadn't faded in yet (`show`)
        window::set_mode(self.overlay, Mode::Windowed)
            .chain(windows::opacity(self.overlay, 1.0))
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
    /// controls of each window the snip asks for (`read`, see
    /// `Snip::set_reader`), for outlines and snapping.
    fn snip_extras(&self, read: std::sync::mpsc::Receiver<isize>) -> Task<Message> {
        let recent = background(|tx| {
            let _ = tx.unbounded_send(snip::load_recent());
        })
        .map(|recent| Message::Snip(snip::Msg::Recent(recent)));
        if !cfg!(windows) {
            return recent;
        }
        let opening = self.opening;
        // Ends when the snip (the sender) is gone and every read is in.
        let controls = background(move |tx| {
            // A thread per window: a slow one (a huge page) holds up no other.
            // ponytail: a hung app's thread lives until UIA's timeout (uia.rs).
            for window in read {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let controls = (capture::uia::controls(window).into_iter())
                        .map(|(r, what)| snip::Pick::control(snip::desktop_rect(r), &what, window))
                        .collect();
                    let _ = tx.unbounded_send(controls);
                });
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
        // No editor (Labs): a snip is only copied.
        let route = match self.labs.has(labs::Feature::Editor) {
            true => snip::route(config::current().snip.after),
            false => (true, false),
        };
        self.snip_routed(crop, in_place, route)
    }

    /// Copies and / or edits a snip's `crop` as `route` says; Copy gets the toast.
    fn snip_routed(&mut self, crop: Crop, in_place: bool, route: (bool, bool)) -> Task<Message> {
        let (copy, edit) = route;
        if edit || !copy {
            if in_place {
                return self.open_editor();
            }
            self.edit = Some(Editing::fitted(
                crop,
                work_area(self.screen, self.work_inset),
                self.scale,
            ));
            return Task::none();
        }
        let Purpose::Snip(snip) = &mut self.purpose else {
            return Task::none();
        };
        let id = snip.toast(crop.clone(), in_place);
        let foreground = snip.foreground;
        // The copy runs on a thread (encoding a big capture takes a while);
        // if it fails, the editor opens, so the capture isn't lost.
        let copied = background(move |tx| {
            if let Run::Finish(copy) = copy::ACTION.run
                && let Err(e) = copy(crop)
            {
                crate::log!("copy failed: {e}");
                let _ = tx.unbounded_send(Message::Snip(snip::Msg::CopyFailed(id)));
            }
        });
        let done = background(move |tx| {
            // Once the pill is up, the user's app (their AI chat) gets the focus back.
            std::thread::sleep(Duration::from_millis(150));
            capture::window_rect::activate(foreground);
            std::thread::sleep(Duration::from_secs(snip::TOAST_SECS));
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::ToastDone(id)));
        Task::batch([copied, self.pill_at_top().chain(done)])
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
        // An area as tall as the screen leaves the pill over it: it is kept out of
        // the frames (`hide_from_capture`, Windows; elsewhere it stays beside the area).
        let at = bar::place(area, snip::PILL, self.screen);
        self.pill_window(at)
            .chain(windows::hide_from_capture(self.overlay, true).map(Message::Windows))
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
                // The search is in the browser now: the overlay, its panel and
                // pills must not stay on screen behind or over it. Close.
                return self.close();
            } else {
                page.clone().release(); // the overlay closed meanwhile
            }
        }
        let Some((_, panel)) = self.panel.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            panel::Msg::Card(card::Event::Grab(how)) => {
                self.card_drag = Some((how, None));
                self.frame_times.borrow_mut().clear();
            }
            panel::Msg::Card(card::Event::Hover(on)) => self.card_hover = on,
            panel::Msg::Card(card::Event::Reset) => self.card = None,
            panel::Msg::Card(card::Event::Close) => self.panel = None,
            panel::Msg::Cancel => self.panel = None,
            panel::Msg::Search => return self.run(&lens::ACTION),
            // A card's focused field gave Ctrl+C back (nothing selected in it).
            panel::Msg::Key(keys::Keyed::Copy) => {
                return self.update(Message::Field(keys::Keyed::Copy));
            }
            panel::Msg::Key(_) => {}
            panel::Msg::CopyText => {
                let text = panel.text().unwrap_or_default();
                let copy = background(move |_: &UnboundedSender<()>| {
                    if let Err(e) = actions::copy::text(text) {
                        crate::log!("copy text failed: {e}");
                    }
                });
                return Task::batch([self.close(), copy.discard()]);
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
        self.tiles = None;
        glass::set_frame(None, 1.0);
        self.stroke = Vec::new();
        self.selection = None;
        self.others = Vec::new();
        self.adding = false;
        self.grab = None;
        self.pan = None;
        self.word = None;
        self.screen_text = word::Screen::default();
        self.reading = None;
        self.awaits = None;
        self.want_all = false;
        self.held = None;
        self.text_used = false;
        self.reader = Reader::Idle;
        self.text_by = None;
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
        self.bar_drag = None;
        self.swap = 0.0;
        self.esc_back = false;
        self.song = None; // stops listening
        self.reveal = None; // the next show clears it again
        if self.can_hide {
            // Gone at once: the hide steps draw the empty view (the theme's
            // background) full-screen, then restore the window from full-screen,
            // all while it is still shown. Unseen, at opacity 0 (`show` clears
            // and fades it in again).
            windows::clear_now(self.overlay);
            self.hiding = Hiding::Empty;
            return Task::none();
        }
        if !std::mem::take(&mut self.overlay_open) {
            return Task::none(); // never shown (Copy, Save)
        }
        crate::log!("[opencircle] overlay window closed");
        trim_heap();
        window::close(self.overlay)
    }

    /// The music button: song search with a key (`has_key`); without one its
    /// setup (Settings > Song search), and nothing is recorded.
    fn song(&mut self, has_key: bool) -> Task<Message> {
        if has_key {
            return self.start_song();
        }
        let closing = self.close();
        let settings = self.windows.open(windows::Kind::Settings);
        let section = self
            .windows
            .update(windows::Msg::Settings(settings::Message::Section(
                settings::Section::Song,
            )));
        closing.chain(settings.chain(section).map(Message::Windows))
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
                        crate::log!("[opencircle] can't open the link: {e}");
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
        let (session, task) =
            translate::Session::start(frame.clone(), area, self.scale, self.cache.clone(), models);
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
                self.frame_times.borrow_mut().clear();
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
            crate::log!("[opencircle] live translate off");
            return live.stop().map(Message::Live);
        }
        if live_layer::unsupported() {
            crate::log!(
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
            frame_times: &self.frame_times,
            hover: snip.is_some_and(|s| s.kind.picks()) && self.edit.is_none(),
            dim: self.translate.is_none() || self.selection.is_some(),
            live_drag: snip.is_some(),
            look: config::look(),
            morph: (self.morph.as_ref()).map(|(from, at)| (from.as_slice(), *at)),
            words: (self.text_mode.as_ref()).map_or(&[], |m| m.boxes.as_slice()),
            picked,
            text: match self.translate {
                None if matches!(self.purpose, Purpose::Search) => {
                    self.screen_text.iter().map(|l| l.bounds).collect()
                }
                _ => Vec::new(),
            },
        };
        let translation = self.translate.as_ref().and_then(|(_, s)| s.image());
        // The frame is its own layer: iced draws a canvas's images above its shapes.
        let mut shown = image(translation.unwrap_or(frame).clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Fill);
        match self.view.crop() {
            Some(part) => shown = shown.crop(part), // zoomed in
            // pixel for pixel: no filtering (a full redraw, e.g. while the dim
            // fades in, costs a copy, not a resample)
            None => shown = shown.filter_method(image::FilterMethod::Nearest),
        }
        // At rest (not zoomed, no translation): drawn as tiles (`overlay::tiles`).
        let shown: Element<'_, Message> = match &self.tiles {
            Some((id, tiles))
                if *id == frame.id() && translation.is_none() && self.view.crop().is_none() =>
            {
                let scale = self.scale;
                let frozen = overlay::Frozen { tiles, scale };
                canvas(frozen).width(Fill).height(Fill).into()
            }
            _ => shown.into(),
        };
        let mut layers = stack![
            shown,
            Element::from(canvas(overlay).width(Fill).height(Fill)).map(Message::Overlay),
            // static: drawn once, with the window's fade-in
            border::view(
                if snip.is_some() {
                    border::Mode::Snip
                } else {
                    border::Mode::Overlay
                },
                self.scale,
                false,
                // the glow breathes once the opening is over, and rests while
                // anything else moves
                match self.appear {
                    Some((_, t)) if t >= 1.0 => {
                        if self.animating() {
                            border::Breathe::Paused
                        } else {
                            border::Breathe::On
                        }
                    }
                    _ => border::Breathe::Off,
                },
            ),
        ];
        if let Some(edit) = &self.edit {
            if edit.is_open() {
                // the toolbar once the bar has turned into it, in its place
                let at = self.bar_at();
                let toolbar = (self.swapped() >= 1.0).then_some(at);
                layers =
                    layers.push(edit.view(self.screen, toolbar, Message::Editor, Message::BarGrab));
                if toolbar.is_none() {
                    layers = layers.push(self.morphing_bar(at));
                }
                return layers.into();
            }
            layers = layers.extend(edit.flat_view());
        }
        // Snip: the loupe beside the pointer (under the hint).
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
            let at = self.card_at();
            let card = panel
                .view(at, self.screen, self.card_hover)
                .map(move |msg| Message::Panel(id, msg));
            grow(card, at, self.card_grown())
        } else if let Some((id, session)) = &self.translate {
            let id = *id;
            session
                .view(self.screen, self.card_at(), self.card_hover)
                .map(move |msg| Message::Translate(id, msg))
        } else if let Some(snip) = snip {
            snip.view().map(Message::Snip)
        } else if let Some(selection) = self.selection {
            match self.purpose {
                Purpose::Search if !self.drawing && !self.settling() => {
                    self.selection_ui(selection)
                }
                _ => space().into(),
            }
        } else {
            // slides down into place as the overlay opens
            let hint = container(widgets::pill(HINT)).center_x(Fill).padding(24);
            let rise = (1.0 - self.appeared()) * SLIDE;
            iced::widget::float(hint)
                .translate(move |_, _| Vector::new(0.0, -rise))
                .into()
        };
        layers = layers.push(top);
        if self.bottom_bar_shown() {
            // slides up into place as the overlay opens
            let rise = (1.0 - self.appeared()) * SLIDE;
            let at = self.bar_at() + Vector::new(0.0, rise);
            layers = layers.push(match self.swapped() > 0.0 {
                true => self.morphing_bar(at), // the toolbar turning back into it
                false => iced::widget::pin(self.bottom_bar()).position(at).into(),
            });
            if let Some(note) = self.reader_note() {
                layers = layers.push(self.beside_bar(note, at));
            }
        }
        // Beside a panel (an auto-search's): the chips.
        if let (Some(_), Some(selection), Purpose::Search, false) = (
            &self.panel,
            self.selection,
            &self.purpose,
            self.drawing || self.settling(),
        ) {
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

    /// What floats around `selection`: only small chips ("Select text" under a
    /// snapped box, a QR code's); every action is in the bottom bar.
    fn selection_ui(&self, selection: overlay::Selection) -> Element<'_, Message> {
        let area = self.view.rect_to_screen(selection.area);
        let room = self.room();
        let mut layers = stack![];
        let mut below = area;
        if selection.kind == overlay::Kind::Snap {
            let label = match &self.text_mode {
                None => "Select text",
                Some(TextMode { lines: None, .. }) => "Reading text…",
                Some(_) => "Done",
            };
            let size = Size::new(widgets::TEXT_BUTTON_WIDTH, widgets::CONTROL);
            let at = chips_at(area, size, room);
            below = area.union(&Rectangle::new(at, size));
            let chip = widgets::float_chip(label, Message::SelectText);
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        if let Some(text) = self.scan.as_ref().and_then(|s| s.qr.as_deref()) {
            let open = qr::web_url(text).map(|_| Message::Qr(qr::Act::Open));
            let chip = bar::qr_chip(text, Message::Qr(qr::Act::Copy), open);
            let size = Size::new(QR_WIDTH, widgets::CONTROL + 2.0 * widgets::SURFACE_PADDING);
            let at = chips_at(below, size, room);
            let chip = container(chip).center_x(QR_WIDTH);
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        layers.into()
    }

    /// How open the result card is (0..1; 0 before its first frame, 1 at once
    /// when the OS asks for no animations).
    fn card_grown(&self) -> f32 {
        match (&self.panel, self.card_open) {
            _ if motion::reduced() => 1.0,
            (Some((id, _)), Some((was, _, grown))) if *id == was => grown,
            _ => 0.0,
        }
    }

    /// Something on the overlay moves by the clock (`Message::Frame` wanted):
    /// the opening, the bar turning or gliding, a card opening.
    fn animating(&self) -> bool {
        self.appear.is_none()
            || self.reveal.is_some()
            || self.appearing()
            || self.swapping()
            || self.bar_gliding()
            || self.card_opening()
            || self.bar_now.is_none()
    }

    /// The result card still opening (frames wanted).
    fn card_opening(&self) -> bool {
        self.frame.is_some() && self.panel.is_some() && self.card_grown() < 1.0
    }

    /// The gaps between the overlay's frames since the last log, as `what`
    /// (the VM frame budget, #23: p95 <= 16 ms, none over 25 ms); then forgets
    /// them.
    fn log_frames(&self, what: &str) {
        let drawn = std::mem::take(&mut *self.frame_times.borrow_mut());
        let mut gaps: Vec<u128> = (drawn.windows(2))
            .map(|w| (w[1] - w[0]).as_millis())
            .collect();
        gaps.sort_unstable();
        if let Some(&max) = gaps.last() {
            let p95 = gaps[(gaps.len() * 95).div_ceil(100) - 1];
            crate::log!(
                "[opencircle] {what}: {} frames, p95 {p95} ms, max {max} ms apart",
                gaps.len() + 1
            );
        }
    }

    /// How far the opening animation is (0..1, eased out); 0 on its first frame.
    fn appeared(&self) -> f32 {
        self.appear.map_or(0.0, |(_, t)| motion::Curve::Out.ease(t))
    }

    /// A drawn stroke is still morphing into its box: the selection's actions
    /// wait for it (draw, release, snap, then the user acts).
    fn settling(&self) -> bool {
        (self.morph.as_ref())
            .is_some_and(|(_, at)| overlay::morph_progress(*at, Instant::now()) < 1.0)
    }

    /// The opening animation still runs (a frame drawn and more to come).
    fn appearing(&self) -> bool {
        self.frame.is_some() && self.appeared() < 1.0
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

    /// The editing toolbar is up (not back at the bar).
    fn editing(&self) -> bool {
        self.edit.as_ref().is_some_and(Editing::is_open)
    }

    /// The bar is the toolbar (1) while editing, else the bar (0).
    fn swap_target(&self) -> f32 {
        if self.editing() { 1.0 } else { 0.0 }
    }

    /// The bar is turning into the toolbar, or back.
    fn swapping(&self) -> bool {
        self.frame.is_some() && self.swap != self.swap_target()
    }

    /// How far the bar has turned into the toolbar (0..1, eased); Snip, which
    /// has no bar, is always there.
    fn swapped(&self) -> f32 {
        match self.purpose {
            // back to the bar: the same curve run backwards, an ease-in
            Purpose::Search => motion::Curve::Out.ease(self.swap),
            _ => self.swap_target(),
        }
    }

    /// The bottom bar's field width and size: the same in every state (no
    /// selection, a selection, after a card), so it never jumps.
    fn bar_size(&self) -> (f32, Size) {
        let icons = labs::bar(&self.labs)
            .iter()
            .filter(|b| !b.is_pill())
            .count();
        bar::bottom_size(self.screen.width, icons as f32)
    }

    /// The size of what is docked at the bar's spot now: the bar, the editing
    /// toolbar, or the shape between them while one turns into the other.
    fn dock_size(&self) -> Size {
        let (_, bar) = self.bar_size();
        let tool = Editing::toolbar_size(self.screen.width);
        let t = self.swapped();
        Size::new(
            bar.width + (tool.width - bar.width) * t,
            bar.height + (tool.height - bar.height) * t,
        )
    }

    /// Where the bar is drawn now (top-left): on its way to `bar_target`.
    fn bar_at(&self) -> Point {
        self.bar_now.unwrap_or_else(|| self.bar_target())
    }

    /// Where the bar (or the toolbar in its place) goes: following the pointer
    /// while held, else its spot, or the nearest free one while the selection
    /// (the editor's canvas) is where it would be.
    fn bar_target(&self) -> Point {
        if let Some((_, at)) = self.bar_drag {
            return at;
        }
        let size = self.dock_size();
        let canvas = self.edit.as_ref().map(Editing::canvas_rect);
        let blocked: Vec<Rectangle> = (self.selection.iter())
            .chain(self.others.iter().map(|(s, _)| s))
            .map(|s| self.view.rect_to_screen(s.area))
            .chain(canvas)
            .collect();
        let spot = bar::free(
            self.bar_anchor,
            &blocked,
            self.screen,
            size,
            self.work_inset,
        );
        bar::anchor_at(spot, self.screen, size, self.work_inset)
    }

    /// The bar is on its way somewhere.
    fn bar_gliding(&self) -> bool {
        self.frame.is_some() && self.bar_now.is_some_and(|at| at != self.bar_target())
    }

    /// The bar was dropped: its place is remembered (config `[bar]`).
    fn remember_bar(&self) -> Task<Message> {
        let mut cfg = config::current();
        cfg.bar.anchor = self.bar_anchor;
        config::set_current(cfg.clone());
        if cfg!(test) {
            return Task::none(); // never the user's file
        }
        background(move |_: &UnboundedSender<()>| {
            if let Err(e) = cfg.save() {
                crate::log!("[opencircle] the bar's place was not saved: {e}");
            }
        })
        .discard()
    }

    /// Something is selected for the bar's pills to act on.
    fn selected_for_bar(&self) -> bool {
        self.selection.is_some() && !self.settling()
    }

    /// What a bar button does (also what Enter / Space do on it once Tab
    /// focused it); none = dimmed: Search in browser and Copy text need a
    /// selection, Translate takes the whole screen without one.
    fn bar_message(&self, which: BarButton) -> Option<Message> {
        let selected = self.selected_for_bar();
        match which {
            BarButton::Search | BarButton::CopyText => selected.then_some(Message::Pill(which)),
            BarButton::Translate => Some(Message::Pill(which)),
            BarButton::Live => Some(Message::LiveToggle),
            BarButton::Song => Some(Message::SongStart),
            BarButton::Edit if selected => Some(Message::Run(&actions::edit::ACTION)),
            BarButton::Edit => Some(Message::EditScreen),
        }
    }

    /// Tab / Shift+Tab: the focus moves on through the bar (`bar::stops`).
    /// The AI field takes the text input's focus, any other stop lets go of it.
    fn tab_bar(&mut self, back: bool) -> Task<Message> {
        let stops = bar::stops(self.selected_for_bar(), &labs::bar(&self.labs));
        self.bar_focus = bar::tab(&stops, self.bar_focus, back);
        match self.bar_focus {
            Some(bar::Stop::Field) => operation::focus(ASK_INPUT),
            _ => operation::focus("overlay-none"), // focusing nothing unfocuses the field
        }
    }

    /// Enter / Space on the stop Tab focused.
    fn activate(&mut self, stop: bar::Stop) -> Task<Message> {
        match stop {
            bar::Stop::Logo => self.update(Message::AskButton),
            bar::Stop::Badge => self.update(Message::AskSubmit),
            bar::Stop::Button(which) => match self.bar_message(which) {
                Some(message) => self.update(message),
                None => Task::none(),
            },
            bar::Stop::Field => Task::none(), // the field's own Enter submits
        }
    }

    /// The bottom bar, the one action hub, as the new design's search bar: a
    /// handle to drag it, the AI field ("Ask OpenCircle AI about your
    /// screen…": Enter, the logo or the "AI ↵" badge ask about the selection,
    /// else the whole screen; empty, they open Ask AI), the pills Search in
    /// browser (highlighted until another is pressed), Copy text and
    /// Translate, then the round icon buttons. Search in browser and Copy text
    /// are dimmed until something is selected; Translate without a selection
    /// takes the whole screen. Always the same size (`bar_size`). Its field
    /// takes the keys only once clicked: until then letters are the action
    /// shortcuts (L, A, T still search, ask and translate).
    fn bottom_bar(&self) -> Element<'_, Message> {
        let (width, _) = self.bar_size();
        let input = widgets::input(
            ASK_INPUT,
            ask_placeholder(width),
            &self.ask,
            Message::AskInput,
            Message::AskSubmit,
        );
        let kbd = |stop| self.bar_focus == Some(stop);
        let field = widgets::ai_field(
            input,
            width,
            (Message::AskButton, kbd(bar::Stop::Logo)),
            (Message::AskSubmit, kbd(bar::Stop::Badge)),
        );
        let field = keys::guard(field, Message::Field);
        let keys = &self.bar_keys;
        let selected = self.selected_for_bar();
        let shown = labs::bar(&self.labs);
        let icons = shown.iter().filter(|b| !b.is_pill()).count() as f32;
        let widths = bar::pill_widths(self.screen.width, icons);
        let pills = [
            (
                BarButton::Search,
                lens::ACTION.icon,
                lens::ACTION.label,
                &keys.search,
            ),
            (
                BarButton::CopyText,
                text::ACTION.icon,
                "Copy text",
                &keys.copy_text,
            ),
            (
                BarButton::Translate,
                actions::translate::ACTION.icon,
                actions::translate::ACTION.label,
                &keys.translate,
            ),
        ]
        .into_iter()
        .zip(widths)
        .map(|((which, glyph, label, keys), width)| {
            let pill = widgets::bar_pill(
                glyph,
                label,
                width,
                self.pill == which,
                self.bar_message(which),
                kbd(bar::Stop::Button(which)),
            );
            // Its label is written on it; an icon-only pill (narrow screens)
            // names itself and its shortcut in a tooltip.
            if width > widgets::BUBBLE {
                return pill.into();
            }
            let why = match keys.is_empty() {
                true => widgets::Why::Icon,
                false => widgets::Why::Keys(keys),
            };
            widgets::hint(pill, label, why, iced::widget::tooltip::Position::Top)
        });
        let icons = [
            (
                BarButton::Live,
                LIVE,
                "Live translate (Beta)",
                &keys.live,
                self.live.is_some(),
            ),
            (
                BarButton::Song,
                song::MUSIC,
                "Song search",
                &keys.song,
                false,
            ),
            (
                BarButton::Edit,
                actions::edit::ACTION.icon,
                if selected {
                    "Edit image"
                } else {
                    "Edit the screen"
                },
                &keys.edit,
                false,
            ),
        ]
        .into_iter()
        .filter(|(which, ..)| shown.contains(which))
        .map(|(which, glyph, label, keys, active)| {
            let message = self.bar_message(which);
            bar_button(
                glyph,
                label,
                keys,
                active,
                message,
                kbd(bar::Stop::Button(which)),
            )
        });
        let handle = container(widgets::icon(widgets::DRAG))
            .center_x(bar::HANDLE)
            .center_y(widgets::CONTROL);
        let mut row = iced::widget::row![handle, field, widgets::toolbar_divider()]
            .extend(pills)
            .spacing(bar::SPACING)
            .align_y(iced::Center);
        let icons: Vec<_> = icons.collect();
        if !icons.is_empty() {
            row = row.push(widgets::toolbar_divider()).extend(icons);
        }
        // A press on the handle or the padding (not taken by the field or a
        // button) drags the bar; opaque: a press on it never starts a selection.
        let bar = iced::widget::mouse_area(widgets::surface(row))
            .on_press(Message::BarGrab)
            .interaction(iced::mouse::Interaction::Grab);
        opaque(bar)
    }

    /// The note on the text reader (`Reader`): its download offered with its
    /// size, its progress, a failure to try again, or "Reading text…".
    fn reader_note(&self) -> Option<Element<'_, Message>> {
        // what it says, then its button or progress bar
        let (say, then): (Element<'_, Message>, Element<'_, Message>) = match &self.reader {
            Reader::Idle => return None,
            Reader::Missing(bytes) => {
                let mb = bytes.div_ceil(1_000_000).max(1);
                let say = format!("Text selection needs the text reader (~{mb} MB).");
                let download = widgets::chip("Download", Message::ReaderDownload);
                (widgets::body(say).into(), download.into())
            }
            Reader::Downloading(done) => {
                let percent = (done * 100.0).round();
                let say = widgets::body(format!("Downloading the text reader… {percent}%"));
                (
                    say.into(),
                    container(widgets::progress(*done)).width(120).into(),
                )
            }
            Reader::Failed(e) => {
                let say = widgets::error(format!("The text reader didn't download: {e}"));
                let again = widgets::chip("Try again", Message::ReaderDownload);
                (say.into(), again.into())
            }
        };
        let row = iced::widget::row![say, then]
            .spacing(12)
            .padding([0, 10])
            .height(widgets::CONTROL)
            .align_y(iced::Center);
        Some(widgets::surface(row).into())
    }

    /// `note` centred on the bar at `at`: above it, or below it when the bar
    /// is at the top; opaque, so a press on it never starts a selection.
    fn beside_bar<'a>(&self, note: Element<'a, Message>, at: Point) -> Element<'a, Message> {
        let (_, size) = self.bar_size();
        let height = widgets::CONTROL + 2.0 * widgets::SURFACE_PADDING;
        let (y, align) = match at.y > self.screen.height / 2.0 {
            true => (at.y - bar::SPACING - height, iced::Bottom),
            false => (at.y + size.height + bar::SPACING, iced::Top),
        };
        let note = container(opaque(note))
            .center_x(size.width)
            .height(height)
            .align_y(align);
        iced::widget::pin(note).position(Point::new(at.x, y)).into()
    }

    /// The bar turning into the editing toolbar (or back) at `at`: its
    /// surface only, sized between the two (`dock_size`); opaque, so a press
    /// on it never starts a selection.
    // ponytail: a plain size morph; the full motion polish is v0.1.1.
    fn morphing_bar(&self, at: Point) -> Element<'_, Message> {
        let size = self.dock_size();
        // (the surface pads its content: the content takes the rest)
        let inner = size - Size::new(2.0, 2.0) * widgets::SURFACE_PADDING;
        let shape = widgets::surface(space().width(inner.width).height(inner.height));
        iced::widget::pin(opaque(shape)).position(at).into()
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
        // Also when a widget took the key: a text field only lets go of the
        // focus on Esc, and the overlay stays (`EscapeTaken`).
        let escape = || {
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                }) => Some(match status {
                    iced::event::Status::Captured => Message::EscapeTaken,
                    iced::event::Status::Ignored => Message::Escape,
                }),
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
            // Snip: Shift held = no snapping; the pointer leaving may be for
            // another monitor.
            let shift = match self.purpose {
                Purpose::Snip(_) => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        Some(Message::Snip(snip::Msg::Shift(m.shift())))
                    }
                    iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
                        Some(Message::Snip(snip::Msg::Left))
                    }
                    _ => None,
                }),
                _ => Subscription::none(),
            };
            // One message per frame while something moves, none at rest.
            let frames = match self.animating() {
                true => motion::frames().map(Message::Frame),
                false => Subscription::none(),
            };
            // Dragging the bar by its handle: the pointer anywhere, until let go. On
            // whenever the bar shows (not only once held), so no move or release
            // is lost while the grab is still being handled.
            let bar = match self.bottom_bar_shown() || self.editing() {
                true => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::BarMoved(position))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::BarReleased),
                    _ => None,
                }),
                false => Subscription::none(),
            };
            Subscription::batch([triggers, escape, alt, card, shift, frames, bar])
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

/// The card at `at`, `grown` of the way open: from `motion::OPEN_SCALE` to its
/// size, about its own middle (the design's open, without its fade: the CPU
/// renderer has no alpha for a group of widgets). Plain once open.
fn grow(card: Element<'_, Message>, at: Rectangle, grown: f32) -> Element<'_, Message> {
    if grown >= 1.0 {
        return card;
    }
    let s = motion::OPEN_SCALE + (1.0 - motion::OPEN_SCALE) * grown;
    let middle = at.center();
    iced::widget::float(card)
        .scale(s)
        .translate(move |bounds, _| (middle - bounds.center()) * (1.0 - s))
        .into()
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

/// The part of a `screen` above the taskbar (`inset` px of its bottom).
fn work_area(screen: Size, inset: f32) -> Size {
    Size::new(screen.width, screen.height - inset)
}

/// A round button of the bottom bar, named `label` (its tooltip adds `keys`);
/// `message: None` = dimmed (nothing to act on yet).
fn bar_button<'a>(
    glyph: char,
    label: &'a str,
    keys: &'a str,
    active: bool,
    message: Option<Message>,
    kbd: bool,
) -> Element<'a, Message> {
    let why = match keys.is_empty() {
        true => widgets::Why::Icon,
        false => widgets::Why::Keys(keys),
    };
    let button = widgets::round_button(glyph, active, message, kbd);
    widgets::hint(button, label, why, iced::widget::tooltip::Position::Top)
}

/// Puts the overlay window over the monitor at `place`, so full-screen (which
/// covers the window's own monitor) lands there. Windows: in physical px; iced
/// moves in logical px at the window's current scale, another monitor's when
/// their scales differ.
fn place_window(id: window::Id, place: snip::Place) -> Task<Message> {
    #[cfg(windows)]
    return window::run(id, move |w| {
        use ::windows::Win32::Foundation::HWND;
        use ::windows::Win32::UI::WindowsAndMessaging::{
            SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        };
        use iced::window::raw_window_handle::RawWindowHandle;
        let Ok(handle) = w.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(h) = handle.as_raw() else {
            return;
        };
        let ((x, y), (width, height)) = (place.origin, place.px);
        // Twice: a move onto a monitor of another scale makes winit resize the
        // window for it (WM_DPICHANGED); the second move, at the new scale, sticks.
        for _ in 0..2 {
            // SAFETY: `h` is this window's live handle (an iced window of ours).
            let moved = unsafe {
                SetWindowPos(
                    HWND(h.hwnd.get() as *mut _),
                    None,
                    x,
                    y,
                    width as i32,
                    height as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            if let Err(e) = moved {
                crate::log!("[opencircle] overlay not moved: {e}");
            }
        }
    })
    .discard();
    #[cfg(not(windows))]
    window::move_to(id, place.logical_origin()).chain(window::resize(id, place.size()))
}

/// How much of the bottom of the monitor at `origin` (`size`, logical px at
/// `scale`) the taskbar takes: the bottom bar keeps above it.
// ponytail: the bottom edge only (a taskbar on the left, right or top
// doesn't push the bar); mixed-DPI monitors map approximately.
#[cfg(windows)]
fn taskbar_inset(origin: Point, size: Size, scale: f32) -> f32 {
    use ::windows::Win32::Foundation::POINT;
    use ::windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    let centre = POINT {
        x: ((origin.x + size.width / 2.0) * scale) as i32,
        y: ((origin.y + size.height / 2.0) * scale) as i32,
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: plain Win32 queries; `info` is sized as cbSize says.
    let read = unsafe {
        let monitor = MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info).as_bool()
    };
    match read {
        true => ((info.rcMonitor.bottom - info.rcWork.bottom) as f32 / scale).max(0.0),
        false => 0.0,
    }
}

/// Elsewhere the desktop's panels aren't known: the bar keeps its own gap.
#[cfg(not(windows))]
fn taskbar_inset(_: Point, _: Size, _: f32) -> f32 {
    0.0
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
        crate::log!("test shot failed: {e}");
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
    // (tests: the test's name, so what it copies is its own, `copy::copied`)
    let name = match cfg!(test) {
        true => std::thread::current().name().unwrap_or_default().into(),
        false => "background".into(),
    };
    let spawned = std::thread::Builder::new()
        .name(name)
        .spawn(move || work(&tx));
    if let Err(e) = spawned {
        crate::log!("cannot start a thread: {e}");
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
        app.labs = Labs::all_on();
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
        app.labs = Labs::all_on();
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
    fn a_shortcut_while_open_brings_the_overlay_back_and_says_so() {
        let mut app = App::idle(window::Id::unique());
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let n = app.capturing.expect("pending");
        let said = |line: &str| crate::log::tail(200).iter().any(|l| l.contains(line));
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        assert!(
            said("a capture is still on its way"),
            "a press left no line"
        );
        let _ = app.update(Message::Captured(n, Instant::now(), a_shot()));
        assert!(app.frame.is_some());
        let again = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        assert!(said("brought to the front"), "a press left no line");
        assert!(again.units() > 0, "the overlay takes the focus back");
        assert!(app.frame.is_some(), "still open, same frame");
    }

    #[test]
    fn a_timed_out_capture_frees_the_trigger_and_its_late_answer_is_dropped() {
        let mut app = App::idle(window::Id::unique());
        app.labs = Labs::all_on();
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
        app.labs = Labs::all_on();
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

    /// The owner's gray flash: Esc drew the empty view (the theme's background)
    /// over the whole screen, then the window left full-screen, still shown.
    /// The window is cleared in the very update that closes it, before any of
    /// those frames (a `Task` would land after one may be on screen).
    #[test]
    fn esc_clears_the_overlay_before_its_last_frames() {
        let first = window::Id::unique();
        let mut app = App::idle(first);
        let _ = open_and_escape(&mut app);
        let cleared = windows::CLEARED.with(|c| std::mem::take(&mut *c.borrow_mut()));
        assert_eq!(cleared, vec![first], "not cleared at the close");
        assert_eq!(app.hiding, Hiding::Empty, "then hidden by the usual steps");
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
            translate::Session::start(frame.clone(), [0, 0, 40, 20], 1.0, app.cache.clone(), false);
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

    /// What Search in browser hands to Lens is exactly the selected pixels, never
    /// blank: through the real opening (`open`), at 100 % on the primary screen,
    /// at 150 % on a monitor left of (and above) it, and at 125 % on its right.
    #[test]
    fn the_crop_lens_gets_is_the_selection_at_any_scale_and_monitor() {
        // pixel (x, y) of the frame = [x, y, x >> 8 | (y >> 8) << 4, 255]
        let px = |x: u32, y: u32| [x as u8, y as u8, (x >> 8 | (y >> 8) << 4) as u8, 255];
        for (scale, origin) in [
            (1.0, Point::ORIGIN),
            (1.5, Point::new(-1280.0, -100.0)),
            (1.25, Point::new(1920.0, 0.0)),
        ] {
            let (w, h) = ((1280.0 * scale) as u32, (720.0 * scale) as u32);
            let rgba: Vec<u8> = (0..h)
                .flat_map(|y| (0..w).flat_map(move |x| px(x, y)))
                .collect();
            let mut app = App::idle(window::Id::unique());
            let shot = Shot {
                image: image::Handle::from_rgba(w, h, rgba),
                scale,
                origin,
                size: Size::new(1280.0, 720.0),
                snip: None,
            };
            let _ = app.open(Instant::now(), Ok(shot));
            app.behaviour = config::Config::default().selection;
            drag(
                &mut app,
                Point::new(100.0, 120.0),
                Point::new(300.0, 220.0),
                false,
            );
            assert!(app.panel.is_none(), "{scale}: nothing sent before a choice");
            let crop = app.crop().expect("a crop");
            let (x0, y0) = ((100.0 * scale) as u32, (120.0 * scale) as u32);
            assert_eq!(
                (crop.width, crop.height),
                ((200.0 * scale) as u32, (100.0 * scale) as u32),
                "{scale}"
            );
            for (i, p) in crop.rgba.as_chunks::<4>().0.iter().enumerate() {
                let (x, y) = (x0 + i as u32 % crop.width, y0 + i as u32 / crop.width);
                assert_eq!(*p, px(x, y), "{scale}: pixel ({x}, {y})");
            }
            assert!(
                lens::lens_png(&crop).is_ok(),
                "{scale}: never refused as blank"
            );
        }
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
        let texts = overlay_texts(&app);
        assert!(texts.contains(&lens::ACTION.icon.to_string()), "{texts:?}");
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
    pub(super) fn open_app(w: u32, h: u32, scale: f32) -> App {
        let mut app = App::idle(window::Id::unique());
        app.labs = Labs::all_on();
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, (x >> 8) as u8, 255]))
            .collect();
        let frame = image::Handle::from_rgba(w, h, rgba);
        app.tiles = overlay::tiles(&frame).map(|t| (frame.id(), t));
        app.frame = Some(frame);
        app.scale = scale;
        app.screen = Size::new(w as f32 / scale, h as f32 / scale);
        app.view = overlay::View::new(w, h, app.screen);
        app.bar_keys = BarKeys::of(&config::Config::default());
        app.appear = Some((Instant::now(), 1.0)); // opened
        app
    }

    fn event(app: &mut App, e: overlay::Event) {
        let _ = app.on_overlay(e);
    }

    /// Runs `task` to its end (its threads' work done) and hands its messages
    /// to the app (their own tasks are dropped).
    fn run_task(app: &mut App, task: Task<Message>) {
        let actions = iced_runtime::task::into_stream(task)
            .map(iced::futures::executor::block_on_stream)
            .into_iter()
            .flatten();
        for action in actions {
            if let iced_runtime::Action::Output(message) = action {
                let _ = app.update(message);
            }
        }
    }

    /// What this test's background work copied, once it has (within 10 s).
    fn copied_image() -> Option<(u32, u32)> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            match copy::copied::image() {
                Some(size) => return Some(size),
                None if Instant::now() > until => return None,
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    /// A press-drag-release on screen from `a` to `b`.
    pub(super) fn drag(app: &mut App, a: Point, b: Point, shift: bool) {
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

    /// A snip over a 800x600 screen with one window (`area`) the pointer is on.
    fn snipping(area: Rectangle) -> App {
        let mut app = open_app(800, 600, 1.0);
        let window = snip::Pick::window(area, "App", 1);
        app.purpose = Purpose::Snip(Box::new(Snip::new(vec![window], app.place(), true, None)));
        event(&mut app, overlay::Event::Hover(area.center()));
        app
    }

    #[test]
    fn snip_ctrl_c_copies_the_area_under_the_pointer_as_an_image() {
        let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
        let mut app = snipping(area);
        let _ = copy::copied::image();
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'c',
                ctrl: true,
            },
        );
        assert_eq!(copied_image(), Some((300, 200)), "that area");
        assert!(copy::copied::text().is_none(), "the picture only, no text");
        assert!(app.snip_toast(), "and the toast says so");
        // off every window it is what a click takes there: the whole screen
        let mut app = snipping(area);
        event(&mut app, overlay::Event::Hover(Point::new(700.0, 500.0)));
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'c',
                ctrl: true,
            },
        );
        assert_eq!(copied_image(), Some((800, 600)));
    }

    /// The two shortcuts of #52: Ctrl+C copies the selection's picture, Ctrl+S saves
    /// it in the save folder, each with a pill; a failure shows the pill without a dot.
    #[test]
    fn ctrl_c_and_ctrl_s_confirm_with_a_pill_and_failures_say_so() {
        let dir = std::env::temp_dir().join(format!("oc-pill-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let before = config::current();
        let mut cfg = before.clone();
        cfg.general.save_folder = dir.to_string_lossy().into_owned();
        config::set_current(cfg);
        let key = |letter| overlay::Event::Key { letter, ctrl: true };
        let selected = || {
            let mut app = open_app(800, 600, 1.0);
            app.select(Rectangle::new(
                Point::new(100.0, 80.0),
                Size::new(300.0, 200.0),
            ));
            app
        };
        // Ctrl+C: the picture on the clipboard and the "Copied" pill
        // (#123: the encoding and the clipboard off the UI thread: the overlay
        // closes at once, the pill comes once the copy is done)
        let mut app = selected();
        let _ = copy::copied::image();
        let task = app.on_overlay(key('c'));
        assert!(app.frame.is_none() && app.windows.toasts().is_empty());
        run_task(&mut app, task);
        assert_eq!(copy::copied::image(), Some((300, 200)));
        assert_eq!(app.windows.toasts(), [("Copied".to_string(), true)]);
        // Ctrl+S: a file in the save folder and "Saved to <folder>"
        let mut app = selected();
        let task = app.on_overlay(key('s'));
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "not yet");
        run_task(&mut app, task);
        let toasts = app.windows.toasts();
        assert_eq!(toasts.len(), 1, "{toasts:?}");
        assert!(
            toasts[0].1 && toasts[0].0.starts_with("Saved to "),
            "{toasts:?}"
        );
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        assert!(
            files[0]
                .file_name()
                .to_string_lossy()
                .starts_with("OpenCircle-")
        );
        // an error: the same pill, the error text, no dot
        let mut app = selected();
        let place = (app.origin, app.screen, app.scale);
        let failed = Err("Couldn't save. Pick a folder you can write to.".into());
        let _ = app.update(Message::Confirm(place, failed));
        assert_eq!(
            app.windows.toasts(),
            [(
                "Couldn't save. Pick a folder you can write to.".to_string(),
                false
            )]
        );
        // it goes after the design's 1800 ms
        assert_eq!(windows::TOAST_MS, 1800);
        config::set_current(before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_snip_timer_comes_from_settings_and_runs_once_per_snip() {
        let shot = || {
            let mut shot = a_shot().unwrap();
            shot.snip = Some((snip::Kind::Region, Vec::new(), None, Vec::new()));
            Ok(shot)
        };
        // (only the timer changes: other tests share the config)
        let with_delay = |secs| {
            let mut cfg = config::current();
            cfg.snip.delay = secs;
            config::set_current(cfg);
        };
        with_delay(3);
        let mut app = App::idle(window::Id::unique());
        app.labs = Labs::all_on();
        let _ = app.open(Instant::now(), shot());
        assert!(app.snip_pill(), "counting down, the overlay is only a pill");
        // the capture the timer waited for opens plain
        app.timed = true;
        let _ = app.open(Instant::now(), shot());
        assert!(!app.snip_pill() && !app.timed);
        // no timer in Settings: no pill
        with_delay(0);
        let mut app = App::idle(window::Id::unique());
        app.labs = Labs::all_on();
        let _ = app.open(Instant::now(), shot());
        assert!(!app.snip_pill());
    }

    #[test]
    fn the_editors_scroll_button_starts_a_scrolling_capture_of_the_area() {
        let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
        let mut app = snipping(area);
        app.select(area);
        let _ = app.open_editor();
        let _ = app.update(Message::Editor(editor::Message::Scroll));
        assert!(app.edit.is_none(), "the editor steps aside");
        assert!(app.scroll().is_some(), "scrolling and stitching has begun");
        // no auto-scroll here (Wayland) or no snip (Edit in place): the button is dimmed
        let mut app = open_app(800, 600, 1.0);
        app.select(area);
        let _ = app.open_editor();
        let _ = app.update(Message::Editor(editor::Message::Scroll));
        assert!(app.edit.is_some() && app.scroll().is_none());
    }

    #[test]
    fn after_a_drag_out_the_editor_is_still_there_or_the_overlay_closes_whole() {
        let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
        // nothing took the drop: the overlay is back, toolbar and all
        let mut app = snipping(area);
        app.select(area);
        let _ = app.open_editor();
        editor::dragout::TEST_DROP.set(Some(false));
        let _ = app.update(Message::Editor(editor::Message::DragOut));
        assert!(app.editing() && app.frame.is_some());
        // an app took it: closed like Copy, nothing of the editor or the frame left
        editor::dragout::TEST_DROP.set(Some(true));
        let _ = app.update(Message::Editor(editor::Message::DragOut));
        assert!(app.edit.is_none() && app.frame.is_none() && !app.editing());
        assert_eq!(app.hiding, Hiding::Empty, "hides by the usual steps");
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
    fn a_tapped_word_is_the_selection() {
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

    /// The caret before character `c` of line `i` (image px, mid-height).
    fn caret(i: usize, c: f32) -> Point {
        Point::new(60.0 + c * CHAR, 43.0 + 32.0 * i as f32 + 13.0)
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
    fn a_shift_click_past_the_lines_read_reads_a_band_first() {
        let mut app = text_app();
        let start = at_word(1, 7.0);
        let first = Rectangle::new(Point::new(0.0, 60.0), Size::new(960.0, 80.0)); // lines 1-2
        app.taps += 1;
        read_back(&mut app, start, first, false);
        let end = caret(5, 18.0); // after "anywhere": not read yet
        drag(&mut app, end, end, true);
        let word = app.word.as_ref().unwrap();
        assert_eq!(word.text(), "while", "unchanged until that is read");
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
        let end = caret(4, 18.0); // after "add"
        drag(&mut app, end, end, true);
        assert_eq!(
            app.word.as_ref().unwrap().text(),
            "while you drag a finger\nacross the words you want to\ncopy, translate or look up.\nPress Shift to add"
        );
        assert!(app.others.is_empty(), "stretched, not a second selection");
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Word);
        // Shift+click on the first line: from there to where it began ("while")
        drag(&mut app, caret(0, 10.0), caret(0, 10.0), true);
        let text = app.word.as_ref().unwrap().text();
        assert_eq!(text, "Search keeps the page\nfrozen");
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
        // The design: an empty field opens Ask AI (its presets), asking nothing.
        let _ = app.update(Message::AskInput("   ".into()));
        let _ = app.update(Message::AskSubmit);
        let (_, panel) = app.panel.as_mut().expect("Ask AI opens");
        assert_eq!(panel.kind(), actions::Panel::Ask);
        assert!(panel.take_input().is_empty(), "no question");
        app.panel = None;
        let _ = app.update(Message::AskInput("what is this?".into()));
        let _ = app.update(Message::AskSubmit);
        let (_, panel) = app.panel.as_ref().expect("Ask AI opens");
        assert_eq!(panel.kind(), actions::Panel::Ask);
        assert!(app.ask.is_empty());
    }

    /// `text_app` with its text reader (the OCR models) here: main mode reads
    /// the screen's text on demand.
    fn reader_app() -> App {
        let mut app = text_app();
        app.ocr_ready = true;
        app
    }

    /// OCR answers the band being read (`band`) of `text_app`'s page.
    fn answer(app: &mut App, band: Rectangle) -> Task<Message> {
        let inside = |b: &TextBlock| {
            band.y <= b.bounds[1] && b.bounds[1] + b.bounds[3] <= band.y + band.height
        };
        let blocks = text_blocks().into_iter().filter(inside).collect();
        let lines = word::lines(blocks, [0, 0], 1.0);
        app.update(Message::ScreenText(app.opening, band, Ok(lines)))
    }

    /// `reader_app` with all its screen read.
    fn read_app() -> App {
        let mut app = reader_app();
        let _ = answer(&mut app, page());
        assert!(app.screen_text.is_whole(420.0));
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

    /// A click (press and release, no move) at `p`.
    fn click_at(app: &mut App, p: Point, shift: bool) {
        event(app, overlay::Event::Press { at: p, shift });
        event(app, overlay::Event::Release);
    }

    #[test]
    fn with_the_screen_read_text_is_selected_like_in_a_document() {
        // a drag selects from the caret under the press to the one under the
        // pointer: characters, not whole words; a flat highlight, not a stroke
        let mut app = read_app();
        stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
        assert!(app.text_press.is_none() && !app.drawing);
        stroke(&mut app, &[caret(1, 9.4), caret(1, 18.6)]);
        assert_eq!(picked_text(&app).as_deref(), Some("ile you dr"));
        // to a line's end and on to the next lines: reading order, middle lines whole
        let end_of_1 = caret(1, 30.0);
        stroke(
            &mut app,
            &[caret(1, 7.0), end_of_1, caret(3, 0.0), caret(3, 15.0)],
        );
        assert_eq!(
            picked_text(&app).as_deref(),
            Some("while you drag a finger\nacross the words you want to\ncopy, translate")
        );
        let rects = app.word.as_ref().unwrap().rects();
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[1].x, 60.0, "the middle line from its start");
        assert!((rects[1].x + rects[1].width - (60.0 + 28.0 * CHAR)).abs() < 0.01);
        // one click on text selects nothing; a double click the word, a triple the line
        // (with text selected before, as on the VM)
        let shift = caret(4, 8.0); // in "Shift"
        click_at(&mut app, shift, false);
        assert!(app.selection.is_none() && app.word.is_none());
        click_at(&mut app, shift, false);
        assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
        click_at(&mut app, shift, false);
        assert_eq!(
            picked_text(&app).as_deref(),
            Some("Press Shift to add a second")
        );
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
        stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
        assert!(picked_text(&app).is_none());
    }

    #[test]
    fn selected_text_gets_the_text_actions_shift_extends_ctrl_a_takes_all() {
        let mut app = read_app();
        stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]); // "while you drag"
        // the bottom bar's Copy and Translate take the text (no pill)
        let texts = overlay_texts(&app);
        assert!(
            !texts.contains(&"Copy".to_string()),
            "no floating pill: {texts:?}"
        );
        let click = |p| widgets::shot::click(app.view(app.overlay), app.screen, p);
        let copy = click(bar_pill(&app, 1));
        assert!(
            copy.iter()
                .any(|m| matches!(m, Message::Pill(BarButton::CopyText)))
        );
        assert!(
            click(bar_pill(&app, 2))
                .iter()
                .any(|m| matches!(m, Message::Pill(BarButton::Translate)))
        );
        assert!(app.picked_text().is_some(), "the pills take the text");
        assert!(
            !app.handles() || app.grab_at(caret(1, 21.0)).is_none(),
            "no handles"
        );
        // Shift+click stretches it from where it began
        click_at(&mut app, caret(2, 16.0), true);
        assert_eq!(
            picked_text(&app).as_deref(),
            Some("while you drag a finger\nacross the words")
        );
        assert!(app.others.is_empty());
        // Ctrl+A: all the text on the screen
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'a',
                ctrl: true,
            },
        );
        assert_eq!(picked_text(&app).as_deref(), Some(TEXT.join("\n").as_str()));
        // A (Ask AI) asks about the text's box, still the selection
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'a',
                ctrl: false,
            },
        );
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
        let clicked = widgets::shot::click(app.view(app.overlay), app.screen, bar_pill(&app, 2));
        assert!(
            clicked
                .iter()
                .any(|m| matches!(m, Message::Pill(BarButton::Translate)))
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
        let bottom = |t: &String| ASK_PLACEHOLDERS.contains(&t.as_str());
        assert!(
            !overlay_texts(&app).iter().any(bottom),
            "the card's own field takes over"
        );
        // Translate translates the whole screen
        let mut app = open_app(800, 600, 1.0);
        let _ = app.update(Message::Pill(BarButton::Translate));
        assert!(app.translate.is_some() && app.selection.is_none());
    }

    /// Where the bottom bar's pills start (x), and its middle (y), as laid out.
    fn bar_row(app: &App) -> (f32, f32) {
        let (field, size) = app.bar_size();
        let at = app.bar_at();
        let x = at.x + widgets::SURFACE_PADDING + bar::HANDLE + bar::SPACING + field;
        (
            x + bar::SPACING + 1.0 + bar::SPACING,
            at.y + size.height / 2.0,
        )
    }

    /// The bar's pills' widths as laid out.
    fn pill_widths(app: &App) -> [f32; 3] {
        let icons = labs::bar(&app.labs).iter().filter(|b| !b.is_pill()).count();
        bar::pill_widths(app.screen.width, icons as f32)
    }

    /// The centre of the bottom bar's pill `k` (0 = Search in browser, 1 = Copy
    /// text, 2 = Translate).
    fn bar_pill(app: &App, k: usize) -> Point {
        let (x, y) = bar_row(app);
        let widths = pill_widths(app);
        let before: f32 = widths[..k].iter().map(|w| w + bar::SPACING).sum();
        Point::new(x + before + widths[k] / 2.0, y)
    }

    /// The centre of the bottom bar's round button `k` (0 = Live translate,
    /// then Song search and Edit when on), after the pills and their divider.
    fn bar_button(app: &App, k: usize) -> Point {
        let (x, y) = bar_row(app);
        let pills: f32 = pill_widths(app).iter().map(|w| w + bar::SPACING).sum();
        let first = x + pills + 1.0 + bar::SPACING;
        let x = first + k as f32 * (widgets::BUBBLE + bar::SPACING) + widgets::BUBBLE / 2.0;
        Point::new(x, y)
    }

    /// A point in the bottom bar's field.
    fn bar_field(app: &App) -> Point {
        let (_, size) = app.bar_size();
        let at = app.bar_at();
        let x = at.x + widgets::SURFACE_PADDING + bar::HANDLE + bar::SPACING + 60.0;
        Point::new(x, at.y + size.height / 2.0)
    }

    /// One selection, the old action bar gone: only the bottom bar, whose pills
    /// act on the selection.
    #[test]
    fn a_selection_brings_its_actions_into_the_bottom_bar_not_a_second_bar() {
        let mut app = open_app(1280, 800, 1.0);
        let pill = |app: &App, k| {
            widgets::shot::click(app.view(app.overlay), app.screen, bar_pill(app, k))
        };
        // before a selection: Search in browser and Copy text are dimmed
        let pressed = |got: Vec<Message>| got.iter().any(|m| matches!(m, Message::Pill(_)));
        assert!(!pressed(pill(&app, 0)) && !pressed(pill(&app, 1)));
        drag(
            &mut app,
            Point::new(200.0, 150.0),
            Point::new(500.0, 350.0),
            false,
        );
        assert!(app.selection.is_some());
        let texts = overlay_texts(&app);
        for label in ["Search in browser", "Copy text", "Translate"] {
            assert!(texts.iter().any(|t| t == label), "{label}: {texts:?}");
        }
        for label in ["Search", "Ask AI", "Text", "Edit", "Copy", "Save"] {
            assert!(
                !texts.iter().any(|t| t == label),
                "no labelled action bar: {texts:?}"
            );
        }
        for (k, which) in [
            (0, BarButton::Search),
            (1, BarButton::CopyText),
            (2, BarButton::Translate),
        ] {
            let sent = pill(&app, k);
            assert!(
                sent.iter()
                    .any(|m| matches!(m, Message::Pill(b) if *b == which)),
                "{which:?}: {sent:?}"
            );
        }
        let live = widgets::shot::click(app.view(app.overlay), app.screen, bar_button(&app, 0));
        assert!(live.iter().any(|m| matches!(m, Message::LiveToggle)));
        // the pressed pill is the highlighted one; Translate takes the selection
        let _ = app.update(Message::Pill(BarButton::Translate));
        assert_eq!(app.pill, BarButton::Translate);
        assert!(app.translate.is_some() && app.selection.is_some());
        // picked words in a snapped box: the pills take their text
        let mut words = select_text_app();
        drag(&mut words, caret(1, 7.0), caret(2, 16.0), false);
        assert!(words.picked_text().is_some_and(|t| !t.trim().is_empty()));
    }

    /// What a click on the reader's note (over the bottom bar, on a row across
    /// its width) sends.
    fn note_clicks(app: &App) -> Vec<Message> {
        let (_, size) = app.bar_size();
        let at = app.bar_at();
        let height = widgets::CONTROL + 2.0 * widgets::SURFACE_PADDING;
        let y = at.y - bar::SPACING - height / 2.0;
        (0..(size.width as usize / 8))
            .flat_map(|i| {
                let p = Point::new(at.x + 8.0 * i as f32, y);
                widgets::shot::click(app.view(app.overlay), app.screen, p)
            })
            .collect()
    }

    /// A fresh install has no text reader (OCR models): a drag on text can only
    /// draw. The overlay says so over the bar, with the size, and downloads
    /// only on its button; then its progress, a failure to try again, and
    /// "Reading text…" while the screen is read.
    #[test]
    fn without_the_text_reader_the_bar_offers_its_download() {
        let mut app = open_app(1280, 800, 1.0);
        app.note_reader(); // as at open (the tests have no models)
        let Reader::Missing(bytes) = app.reader else {
            panic!("{:?}", app.reader);
        };
        assert!(bytes > 1_000_000, "{bytes}");
        let texts = overlay_texts(&app);
        let mb = bytes.div_ceil(1_000_000);
        let say = format!("Text selection needs the text reader (~{mb} MB).");
        assert!(texts.contains(&say), "{texts:?}");
        assert!(texts.contains(&"Download".to_string()), "{texts:?}");
        widgets::shot::save("reader-missing", app.view(app.overlay), app.screen);
        // nothing fetched by itself: drawing, selecting, frames leave it offered
        drag(
            &mut app,
            Point::new(200.0, 150.0),
            Point::new(500.0, 350.0),
            false,
        );
        settle_bar(&mut app);
        assert!(matches!(app.reader, Reader::Missing(_)));
        // one click on Download
        let clicks = note_clicks(&app);
        assert!(
            clicks.iter().any(|m| matches!(m, Message::ReaderDownload)),
            "{clicks:?}"
        );
        let _ = app.update(Message::ReaderDownload);
        assert_eq!(app.reader, Reader::Downloading(0.0));
        let opening = app.opening;
        let step = move |p| Message::Reader(opening, p);
        let _ = app.update(step(Ok(text::Progress::Download(0.42))));
        let texts = overlay_texts(&app);
        assert!(
            texts.contains(&"Downloading the text reader… 42%".to_string()),
            "{texts:?}"
        );
        widgets::shot::save("reader-downloading", app.view(app.overlay), app.screen);
        // another opening's download says nothing here
        let stale = Message::Reader(app.opening.wrapping_add(1), Err("x".into()));
        let _ = app.update(stale);
        assert_eq!(app.reader, Reader::Downloading(0.42));
        let _ = app.update(step(Err("no network".into())));
        let texts = overlay_texts(&app);
        assert!(
            texts.contains(&"The text reader didn't download: no network".to_string()),
            "{texts:?}"
        );
        assert!(
            note_clicks(&app)
                .iter()
                .any(|m| matches!(m, Message::ReaderDownload))
        );
        // done: the screen is read (here, still no models: offered again)
        let _ = app.update(step(Ok(text::Progress::Reading)));
        assert!(matches!(app.reader, Reader::Missing(_)), "{:?}", app.reader);
    }

    /// A drag on text that began before the screen's text was read becomes a
    /// text selection as soon as it is (not a stroke snapped to a box).
    #[test]
    fn a_drag_on_text_selects_it_once_the_words_arrive() {
        let mut app = reader_app();
        event(
            &mut app,
            overlay::Event::Press {
                at: caret(1, 7.0),
                shift: false,
            },
        );
        for i in 1..=8 {
            let p = caret(1, 7.0) + (caret(1, 15.0) - caret(1, 7.0)) * (i as f32 / 8.0);
            event(&mut app, overlay::Event::Move(p));
        }
        assert!(app.drawing, "a stroke until the words are read");
        let band = app.reading.expect("the band under the press is read");
        let _ = answer(&mut app, band);
        assert!(!app.drawing && app.text_press.is_some());
        event(&mut app, overlay::Event::Move(caret(1, 21.0)));
        event(&mut app, overlay::Event::Release);
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
        // off the text it stays a stroke
        let mut app = reader_app();
        event(
            &mut app,
            overlay::Event::Press {
                at: Point::new(600.0, 250.0),
                shift: false,
            },
        );
        event(&mut app, overlay::Event::Move(Point::new(700.0, 300.0)));
        let band = app.reading.expect("a press may be on text");
        let _ = answer(&mut app, band);
        assert!(app.drawing && app.word.is_none());
        assert!(
            !app.text_used && app.reading.is_none(),
            "no text: no more is read"
        );
    }

    /// #123: opening the overlay reads no text and starts no helper while the
    /// opening animation runs (it was a full-screen OCR read 150 ms in, every
    /// core busy); the helper starts once it ends. A press reads the band
    /// under it first; once text is used, the rest follows, nearest first.
    #[test]
    fn the_screen_is_read_on_demand_the_touched_band_first() {
        text::MODELS_HERE.set(Some(true));
        let page = text_app();
        let mut app = App::idle(window::Id::unique());
        app.labs = Labs::all_on();
        let shot = Shot {
            image: page.frame.clone().unwrap(),
            scale: 1.0,
            origin: Point::ORIGIN,
            size: page.screen,
            snip: None,
        };
        let _ = app.open(Instant::now(), Ok(shot));
        assert!(app.ocr_ready && app.reader == Reader::Idle);
        assert_eq!(app.reading, None, "nothing read at open");
        // the opening animation: nothing read, no helper started
        let start = Instant::now();
        let mut ms = 0;
        let mut last = Task::none();
        while app.appear.is_none() || app.appearing() {
            let revealing = app.reveal.is_some();
            last = app.update(Message::Frame(start + Duration::from_millis(ms)));
            ms += 16;
            // the reveal's frame fades the window in (Windows, #23): not work
            if app.appearing() && !(revealing && app.reveal.is_none()) {
                assert_eq!(last.units(), 0, "work at {ms} ms of the opening");
            }
            assert_eq!(app.reading, None);
        }
        assert!(last.units() > 0, "its end starts the helper");
        // a press on text: the band under it is read first, not the screen
        let at = caret(1, 7.0);
        event(&mut app, overlay::Event::Press { at, shift: false });
        let first = app.reading.expect("the touched band");
        assert_eq!(first, word::Screen::band(at.y, app.screen));
        assert!(first.height < app.screen.height / 1.5 && first.contains(at));
        for i in 1..=8 {
            let p = at + (caret(1, 21.0) - at) * (i as f32 / 8.0);
            event(&mut app, overlay::Event::Move(p));
        }
        let _ = answer(&mut app, first);
        event(&mut app, overlay::Event::Release);
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
        // text was used: the rest of the screen, band by band
        let next = app.reading.expect("the rest, in the background");
        assert_ne!(next, first);
        let mut bands = 1;
        while let Some(band) = app.reading {
            let _ = answer(&mut app, band);
            bands += 1;
            assert!(bands < 10);
        }
        assert!(app.screen_text.is_whole(app.screen.height));
        assert_eq!(app.screen_text.lines().len(), TEXT.len());
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"), "kept");
        text::MODELS_HERE.set(None);
    }

    /// Off Windows (no Windows OCR): once the helper has started after the
    /// opening animation, the whole screen is read gently, band by band from
    /// the pointer out, with no press: the I-beam shows over its text (the
    /// overlay gets its lines), as on Windows. A late `Warmed` of an earlier
    /// opening reads nothing.
    #[test]
    #[cfg(not(windows))]
    fn off_windows_the_screen_is_read_once_the_helper_is_warm() {
        let mut app = reader_app();
        app.pointer = caret(2, 3.0);
        let _ = app.update(Message::Warmed(app.opening.wrapping_sub(1)));
        assert_eq!(app.reading, None, "an earlier opening's helper");
        let _ = app.update(Message::Warmed(app.opening));
        let first = app.reading.expect("read with no press");
        assert!(first.contains(app.pointer), "the pointer's band first");
        let mut bands = 0;
        while let Some(band) = app.reading {
            let _ = answer(&mut app, band);
            bands += 1;
            assert!(bands < 10);
        }
        assert!(app.screen_text.is_whole(app.screen.height));
        assert_eq!(app.screen_text.lines().len(), TEXT.len());
        assert!(
            app.word.is_none() && app.selection.is_none(),
            "nothing selected"
        );
    }

    /// #62: with Windows OCR (no PP-OCR models needed: no download note) the
    /// whole screen is read in one go from the capture on (while the overlay
    /// comes up, one read only); a drag begun on text before it arrives
    /// becomes a text selection when it does. Nothing more is read after it
    /// (no bands, no helper).
    #[test]
    fn windows_ocr_reads_the_whole_screen_from_the_capture() {
        text::MODELS_HERE.set(Some(false));
        ocr::system::LANGUAGE_HERE.set(Some(Some("en-US")));
        let page = text_app();
        let mut app = App::idle(window::Id::unique());
        app.labs = Labs::all_on();
        let shot = Shot {
            image: page.frame.clone().unwrap(),
            scale: 1.0,
            origin: Point::ORIGIN,
            size: page.screen,
            snip: None,
        };
        let opened = app.open(Instant::now(), Ok(shot));
        assert_eq!(app.reader, Reader::Idle, "no download offered");
        let whole = Rectangle::with_size(app.screen);
        assert_eq!(app.reading, Some(whole), "the whole screen, at the capture");
        assert_eq!(app.text_by, Some(ocr::Source::System));
        assert!(opened.units() > 0);
        let start = Instant::now();
        let mut ms = 0;
        while app.appear.is_none() || app.appearing() {
            let _ = app.update(Message::Frame(start + Duration::from_millis(ms)));
            ms += 16;
            assert_eq!(app.reading, Some(whole), "one read at {ms} ms");
        }
        // a drag on text while it is read: a stroke for now
        let at = caret(1, 7.0);
        event(&mut app, overlay::Event::Press { at, shift: false });
        for i in 1..=8 {
            let p = at + (caret(1, 21.0) - at) * (i as f32 / 8.0);
            event(&mut app, overlay::Event::Move(p));
        }
        assert!(app.drawing && app.word.is_none());
        let _ = answer(&mut app, whole);
        assert!(app.screen_text.is_whole(app.screen.height));
        event(&mut app, overlay::Event::Release);
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
        assert_eq!(app.reading, None, "nothing more to read");
        // every word is selectable now, at once
        click_at(&mut app, at_word(4, 6.0), false);
        click_at(&mut app, at_word(4, 6.0), false);
        assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
        assert_eq!(app.reading, None);
        // the closer (doubled) read replaces the quick one; the selection stays
        let blocks: Vec<TextBlock> = text_blocks().into_iter().skip(1).collect();
        let closer = word::lines(blocks, [0, 0], 1.0);
        let _ = app.update(Message::ScreenText(app.opening, whole, Ok(closer)));
        assert_eq!(app.screen_text.lines().len(), TEXT.len() - 1, "replaced");
        assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
        ocr::system::LANGUAGE_HERE.set(None);
        text::MODELS_HERE.set(None);
    }

    /// #63: Windows OCR can't read (no recognizer for the user's languages;
    /// here: no Windows at all): the helper reads instead, band by band, and
    /// without its models the bar offers their download, as before.
    #[test]
    #[cfg(not(windows))] // Windows' real OCR would read
    fn without_windows_ocr_the_helper_reads() {
        ocr::system::LANGUAGE_HERE.set(Some(Some("en-US"))); // as if it were there
        let mut app = text_app();
        assert!(app.reads_text() && app.reader == Reader::Idle);
        app.helper = Some(Arc::new(ocr::hold())); // open, as `open` does
        let task = app.warm();
        assert_eq!(app.text_by, Some(ocr::Source::System));
        run_task(&mut app, task); // the real read: there is none here
        assert_eq!(app.text_by, Some(ocr::Source::Helper));
        assert!(matches!(app.reader, Reader::Missing(_)), "{:?}", app.reader);
        assert!(!app.reads_text() && app.screen_text.lines().is_empty());
        // with the models, a press reads the band under it (#123)
        let mut app = reader_app();
        app.text_by = Some(ocr::Source::Helper);
        let at = caret(1, 7.0);
        event(&mut app, overlay::Event::Press { at, shift: false });
        assert_eq!(app.reading, Some(word::Screen::band(at.y, app.screen)));
        ocr::system::LANGUAGE_HERE.set(None);
    }

    /// Windows OCR reads the frame in physical px: the whole screen's band is
    /// the whole frame and its lines land on their text at 100/150/175 %
    /// (the frame is the screen's own, so on a second screen too, #47).
    #[test]
    fn windows_ocr_lines_land_on_the_frame_at_any_scale() {
        for scale in [1.0, 1.5, 1.75] {
            let screen = Size::new(1920.0 / scale, 1080.0 / scale);
            let whole = Rectangle::with_size(screen);
            let crop = actions::crop_bounds(1920, 1080, scale, whole);
            assert_eq!(crop, Some([0, 0, 1920, 1080]), "{scale}x");
            // "Hello" at image (100, 40), 60 x 20, as Windows reads it (frame px)
            let px = |v: f32| v * scale;
            let word = (
                "Hello".to_string(),
                [px(100.0), px(40.0), px(60.0), px(20.0)],
            );
            let lines = word::lines(ocr::system::blocks(vec![vec![word]]), [0, 0], scale);
            let b = lines[0].bounds;
            let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
            assert!(
                near(b.x, 100.0) && near(b.y, 40.0) && near(b.width, 60.0) && near(b.height, 20.0),
                "{scale}x: {b:?}"
            );
        }
    }

    /// A quick drag on text let go (and Ctrl+C pressed) before the band under
    /// its press is read: both wait for it, and the text is what is copied.
    /// Off text, the held drag is a stroke as before.
    #[test]
    fn a_drag_let_go_before_its_band_is_read_waits_for_it() {
        let mut app = reader_app();
        stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
        let band = app.reading.expect("the band under the press");
        assert!(app.held.is_some() && app.selection.is_none(), "held");
        let ctrl_c = overlay::Event::Key {
            letter: 'c',
            ctrl: true,
        };
        event(&mut app, ctrl_c);
        let _ = copy::copied::text();
        let task = answer(&mut app, band);
        run_task(&mut app, task);
        assert_eq!(copy::copied::text().as_deref(), Some("while you drag"));
        assert!(app.held.is_none());
        // off text: a stroke, snapped to a box as before
        let mut app = reader_app();
        stroke(
            &mut app,
            &[Point::new(600.0, 250.0), Point::new(800.0, 400.0)],
        );
        let band = app.reading.expect("the band under the press");
        let _ = answer(&mut app, band);
        assert!(app.held.is_none() && app.word.is_none());
        assert_eq!(app.selection.map(|s| s.kind), Some(overlay::Kind::Drag));
    }

    /// A tap, a Shift+click and Ctrl+A before their text is read wait for it.
    #[test]
    fn taps_shift_clicks_and_ctrl_a_wait_for_their_band() {
        // a tap: the word there once its band is read
        let mut app = reader_app();
        let tap = at_word(4, 6.0); // "Shift"
        click_at(&mut app, tap, false);
        let band = app.reading.expect("the tapped band");
        assert!(band.contains(tap) && band.height < 420.0);
        let _ = answer(&mut app, band);
        assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
        // Shift+click past the rows read: that band, then the words stretch there
        let mut app = reader_app();
        let band = word::Screen::band(caret(0, 0.0).y, app.screen);
        let _ = answer(&mut app, band);
        stroke(&mut app, &[caret(0, 7.0), caret(0, 16.0)]);
        assert_eq!(picked_text(&app).as_deref(), Some("to Search"));
        let end = caret(5, 9.0);
        assert!(!app.screen_text.knows(end.y));
        click_at(&mut app, end, true);
        while let Some(band) = app.reading {
            let _ = answer(&mut app, band); // the background band, then the clicked one
        }
        let text = picked_text(&app).unwrap();
        assert!(
            text.starts_with("to Search") && text.ends_with("selection"),
            "{text}"
        );
        // Ctrl+A: everything, once all is read
        let mut app = reader_app();
        event(&mut app, overlay::Event::Hover(caret(3, 0.0)));
        let ctrl_a = overlay::Event::Key {
            letter: 'a',
            ctrl: true,
        };
        event(&mut app, ctrl_a);
        let first = app.reading.expect("from the pointer");
        assert!(first.contains(caret(3, 0.0)));
        while let Some(band) = app.reading {
            assert!(app.selection.is_none(), "not before all is read");
            let _ = answer(&mut app, band);
        }
        assert_eq!(picked_text(&app).unwrap(), TEXT.join("\n"));
    }

    /// The bottom bar (owner, 2026-10-25): no Search, Ask AI or Text buttons
    /// (a selection searches by itself, Enter asks, text is selected directly);
    /// their keys L, A, T still work. In "wait" mode Enter in the empty field
    /// is the search, and the field says so.
    /// The names of the bottom bar's buttons (their tooltips' first words).
    fn bar_names(app: &App) -> Vec<String> {
        let _ = widgets::tests_support::shown();
        let _ = overlay_texts(app);
        let mut names: Vec<String> = widgets::tests_support::shown()
            .iter()
            .map(|t| t.split(" · ").next().unwrap_or_default().to_string())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn hidden_features_have_no_bar_button_key_or_editor_and_turning_them_on_brings_them_back() {
        let mut app = open_app(1280, 800, 1.0);
        let (_, full) = app.bar_size();
        app.labs = Labs::default();
        let (_, small) = app.bar_size();
        assert!(
            small.width < full.width,
            "two buttons fewer: {small:?} < {full:?}"
        );
        let names = bar_names(&app);
        assert_eq!(
            names,
            ["Ask AI", "Open OpenCircle AI chat"],
            "the design's bar, no Live translate, no Song search, no Edit"
        );
        let texts = overlay_texts(&app);
        for pill in ["Search in browser", "Copy text", "Translate"] {
            assert!(texts.iter().any(|t| t == pill), "{pill}: {texts:?}");
        }
        // E does nothing without a selection and with one (no editor, no whole-screen editor).
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'e',
                ctrl: false,
            },
        );
        assert!(app.edit.is_none() && !app.editing());
        drag(
            &mut app,
            Point::new(200.0, 150.0),
            Point::new(500.0, 350.0),
            false,
        );
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'e',
                ctrl: false,
            },
        );
        let _ = app.update(Message::Run(&actions::edit::ACTION));
        let _ = app.update(Message::EditScreen);
        assert!(app.edit.is_none(), "the editor never opens");
        // Pins and a snip's editor are off too.
        assert!(!app.labs.has(labs::Feature::Pins));
        // Song search on: its button is back (one more), the editor still off.
        app.labs.set(labs::Feature::Song, true);
        assert!(bar_names(&app).iter().any(|n| n == "Song search"));
        // (the first icon also brings the divider before the icons)
        assert!(app.bar_size().1.width >= small.width + widgets::BUBBLE + bar::SPACING);
        // Live translate on: its button is back.
        app.labs.set(labs::Feature::Live, true);
        assert!(bar_names(&app).iter().any(|n| n == "Live translate (Beta)"));
        // Editor on: Edit is back, and E opens it.
        app.labs.set(labs::Feature::Editor, true);
        assert!(bar_names(&app).iter().any(|n| n.starts_with("Edit")));
        let _ = app.update(Message::Run(&actions::edit::ACTION));
        assert!(app.editing());
    }

    #[test]
    fn a_pin_needs_its_labs_switch() {
        let mut app = open_app(1280, 800, 1.0);
        drag(
            &mut app,
            Point::new(200.0, 150.0),
            Point::new(500.0, 350.0),
            false,
        );
        let _ = app.open_editor();
        app.labs.set(labs::Feature::Pins, false);
        let _ = app.update(Message::Editor(editor::Message::Pin));
        assert!(
            app.frame.is_some() && app.edit.is_some(),
            "the overlay stays: nothing was pinned"
        );
        app.labs.set(labs::Feature::Pins, true);
        let _ = app.update(Message::Editor(editor::Message::Pin));
        assert!(app.hiding != Hiding::No, "pinned: the overlay closes");
    }

    #[test]
    fn a_hidden_features_trigger_does_nothing() {
        // The shared config is the defaults here: Snip and Song are off.
        let mut app = open_app(1280, 800, 1.0);
        app.frame = None;
        let qa = |action: &str| QuickAction {
            action: action.into(),
            shortcut: String::new(),
            target: Target::Circle,
            live: false,
        };
        for id in ["snip", "song_search"] {
            let _ = app.update(Message::Trigger(Trigger::Quick(Instant::now(), qa(id))));
            assert!(app.capturing.is_none(), "{id} did not start a capture");
        }
    }

    /// #144: the ask field's placeholder fits its text area at every bar width,
    /// in the UI font on this machine (Inter here, Segoe UI on Windows).
    #[test]
    fn ask_placeholder_always_fits_its_field() {
        for icons in 0..=5 {
            for screen in (600..=3840).step_by(40) {
                let (field, _) = bar::bottom_size(screen as f32, icons as f32);
                let room = field - widgets::AI_FIELD_LEFT - widgets::AI_FIELD_RIGHT;
                let p = ask_placeholder(field);
                assert!(
                    p == "Ask AI…" || widgets::text_width(p, 14.0) <= room,
                    "{p:?} is cut in a {field} px field"
                );
            }
        }
        // the design's 320 px field keeps the sentence's meaning
        assert!(ask_placeholder(bar::FIELD).ends_with("screen…"));
    }

    #[test]
    fn the_bar_is_the_designs_and_nothing_is_sent_before_a_choice() {
        // The default (and every migrated) config: nothing runs after a selection.
        let mut app = open_app(1280, 800, 1.0);
        app.labs = Labs::all_on();
        app.behaviour = config::Config::default().selection;
        drag(
            &mut app,
            Point::new(200.0, 150.0),
            Point::new(500.0, 350.0),
            false,
        );
        assert!(app.selection.is_some());
        assert!(app.auto_actions().is_empty(), "nothing runs by itself");
        assert!(app.panel.is_none(), "no Lens, no Ask AI before a choice");
        assert!(app.lens_pages.is_empty(), "nothing handed to the browser");
        let _ = widgets::tests_support::shown();
        let texts = overlay_texts(&app);
        let tips = widgets::tests_support::shown();
        let labels = |t: &String| t.split(" · ").next().unwrap_or_default().to_string();
        let mut names: Vec<String> = tips.iter().map(labels).collect();
        names.sort();
        names.dedup();
        assert_eq!(
            names,
            [
                "Ask AI",
                "Edit image",
                "Live translate (Beta)",
                "Open OpenCircle AI chat",
                "Song search",
            ],
            "{tips:?}"
        );
        assert!(
            texts.iter().any(|t| ASK_PLACEHOLDERS.contains(&t.as_str())),
            "{texts:?}"
        );
        // the three labelled pills, in the design's order
        let pills: Vec<&String> = (texts.iter())
            .filter(|t| ["Search in browser", "Copy text", "Translate"].contains(&t.as_str()))
            .collect();
        assert_eq!(pills, ["Search in browser", "Copy text", "Translate"]);
        // Enter with nothing typed: Ask AI opens (the design), nothing is searched
        let _ = app.update(Message::AskInput("  ".into()));
        let _ = app.update(Message::AskSubmit);
        assert_eq!(
            app.panel.as_ref().map(|(_, p)| p.kind()),
            Some(actions::Panel::Ask)
        );
        // "Auto action" opted back in: the selection searches by itself
        let mut app = open_app(1280, 800, 1.0);
        app.behaviour = behaviour(SelectStyle::Snap, After::Search, SearchWith::Lens);
        drag(
            &mut app,
            Point::new(200.0, 150.0),
            Point::new(500.0, 350.0),
            false,
        );
        assert_eq!(
            app.panel.as_ref().map(|(_, p)| p.kind()),
            Some(actions::Panel::Lens)
        );
        // the keys still work: T reads the text, A asks, L searches
        for (letter, kind) in [
            ('t', actions::Panel::Text),
            ('a', actions::Panel::Ask),
            ('l', actions::Panel::Lens),
        ] {
            app.panel = None;
            event(
                &mut app,
                overlay::Event::Key {
                    letter,
                    ctrl: false,
                },
            );
            assert_eq!(app.panel.as_ref().map(|(_, p)| p.kind()), Some(kind));
        }
    }

    /// Frames until the bar is where it goes (it glides there).
    pub(super) fn settle_bar(app: &mut App) {
        let start = Instant::now();
        for i in 0..100 {
            let _ = app.update(Message::Frame(start + Duration::from_millis(16 * i)));
        }
        assert!(!app.bar_gliding());
    }

    /// The bar's bounds, and whether its Edit slot (the last) is Edit, once settled.
    fn bar_box(app: &mut App) -> (Rectangle, bool) {
        settle_bar(app);
        let edit = widgets::shot::click(app.view(app.overlay), app.screen, bar_button(app, 2));
        let is_edit = |m: &Message| match m {
            Message::EditScreen => true,
            Message::Run(a) => a.id == actions::edit::ACTION.id,
            _ => false,
        };
        let edits = edit.iter().any(is_edit);
        (Rectangle::new(app.bar_at(), app.bar_size().1), edits)
    }

    /// The bottom bar is exactly the same size in every state (owner,
    /// 2026-10-25): no selection, a selection, after a result card closed.
    /// The selection's buttons are always there, dimmed until it exists.
    #[test]
    fn the_bottom_bar_never_changes_size() {
        let mut app = open_app(1280, 800, 1.0);
        let (before, edits) = bar_box(&mut app);
        assert!(edits);
        // nothing selected: Search in browser and Copy text are there but do nothing
        for k in 0..=1 {
            let at = bar_pill(&app, k);
            let got = widgets::shot::click(app.view(app.overlay), app.screen, at);
            assert!(
                !got.iter().any(|m| matches!(m, Message::Pill(_))),
                "{k}: {got:?}"
            );
        }
        // a selection (away from the bar): the same box, the same slots
        drag(
            &mut app,
            Point::new(200.0, 150.0),
            Point::new(500.0, 350.0),
            false,
        );
        app.panel = None; // the auto-search's card
        let (selected, edits) = bar_box(&mut app);
        assert_eq!(selected, before);
        assert!(edits);
        // Ask AI's card opens (the bar steps aside) and closes: the same again
        let _ = app.update(Message::AskButton);
        let (id, _) = app.panel.as_ref().expect("Ask AI opens");
        let id = *id;
        assert!(!app.bottom_bar_shown());
        let _ = app.update(Message::Panel(id, panel::Msg::Card(card::Event::Close)));
        assert!(app.bottom_bar_shown());
        let (after, edits) = bar_box(&mut app);
        assert_eq!(after, before);
        assert!(edits);
    }

    /// Edit turns the bar into the editing toolbar in its place (same anchor,
    /// a short morph between them); the toolbar drags and docks like the bar,
    /// remembered; Back turns it into the bar again where the toolbar was.
    #[test]
    fn the_editing_toolbar_takes_the_bars_place_and_docks_like_it() {
        let mut app = open_app(1280, 800, 1.0);
        app.work_inset = 40.0;
        // clear of both spots, even for a two-row toolbar
        drag(
            &mut app,
            Point::new(200.0, 300.0),
            Point::new(500.0, 450.0),
            false,
        );
        let (bar_box, _) = bar_box(&mut app);
        let spot = |app: &App, size| bar::anchor_at(app.bar_anchor, app.screen, size, 40.0);
        assert_eq!(bar_box.position(), spot(&app, bar_box.size()));
        let tool = Editing::toolbar_size(app.screen.width);
        let _ = app.update(Message::Run(&actions::edit::ACTION));
        assert!(app.editing());
        // on its way: one shape between the two sizes, at the same spot
        let start = Instant::now();
        let _ = app.update(Message::Frame(start));
        let _ = app.update(Message::Frame(start + Duration::from_millis(40)));
        let mid = app.dock_size();
        assert!(app.swapping(), "a morph, not a jump");
        assert!(mid.width > bar_box.width.min(tool.width));
        assert!(mid.width < bar_box.width.max(tool.width));
        assert_eq!(app.bar_at(), spot(&app, mid));
        // ...drawn at its whole size (inside it differs from just outside it)
        // (over a plain frame)
        app.frame = Some(image::Handle::from_rgba(4, 4, vec![9; 4 * 4 * 4]));
        let px = widgets::shot::save("bar-morph", app.view(app.overlay), app.screen);
        let at = app.bar_at();
        let y = (at.y + mid.height / 2.0) as usize;
        let pixel = |x: f32| {
            let i = (y * 1280 + x as usize) * 4;
            px[i..i + 4].to_vec()
        };
        assert_ne!(
            pixel(at.x + mid.width - 30.0),
            pixel(at.x + mid.width + 30.0),
            "the morph's shape is there"
        );
        // there: the toolbar at the bar's anchor (bottom edge, centre the same)
        settle_bar(&mut app);
        assert!(!app.swapping());
        let at = app.bar_at();
        assert_eq!(at, spot(&app, tool));
        assert_eq!(at.y + tool.height, bar_box.y + bar_box.height);
        assert_eq!(at.x + tool.width / 2.0, bar_box.center_x());
        // its first row (two rows where drag-out makes it too wide)
        let pad = widgets::SURFACE_PADDING;
        let row = pad + widgets::CONTROL / 2.0;
        let back = at + Vector::new(row, row);
        let hit = |app: &App, p| widgets::shot::click(app.view(app.overlay), app.screen, p);
        assert!(
            hit(&app, back)
                .iter()
                .any(|m| matches!(m, Message::Editor(editor::Message::Back))),
            "its Back arrow is there"
        );
        // dragged by its edge, it docks at the nearest spot, remembered
        let edge = at + Vector::new(pad / 2.0, row);
        assert!(
            hit(&app, edge)
                .iter()
                .any(|m| matches!(m, Message::BarGrab))
        );
        let _ = app.update(Message::BarGrab);
        let _ = app.update(Message::BarMoved(edge));
        let _ = app.update(Message::BarMoved(edge - Vector::new(0.0, 560.0)));
        let _ = app.update(Message::BarReleased);
        assert_eq!(app.bar_anchor, BarAnchor::Top);
        assert_eq!(config::current().bar.anchor, BarAnchor::Top, "remembered");
        settle_bar(&mut app);
        assert_eq!(app.bar_at(), spot(&app, tool));
        assert_eq!(app.bar_at().y, bar::TOP);
        // Back: the bar again, at the toolbar's spot
        let _ = app.update(Message::Editor(editor::Message::Back));
        assert!(!app.editing());
        // Esc did it: the same key's Escape doesn't close the overlay too
        let _ = app.update(Message::Escape);
        assert!(app.frame.is_some(), "still open, at the bar");
        settle_bar(&mut app);
        let (_, size) = app.bar_size();
        assert_eq!(app.bar_at(), spot(&app, size));
        assert_eq!(app.bar_at().y, bar::TOP);
        let edit = hit(&app, bar_button(&app, 2));
        assert!(
            edit.iter()
                .any(|m| matches!(m, Message::Run(a) if a.id == actions::edit::ACTION.id)),
            "{edit:?}"
        );
        let mut cfg = config::current();
        cfg.bar.anchor = BarAnchor::Bottom;
        config::set_current(cfg);
    }

    /// The bar: by default a clear gap above the taskbar; dragged by its
    /// handle it follows the pointer, and let go it glides to the nearest
    /// spot, remembered for the next run; a selection under it sends it to a
    /// free spot until the space is free again.
    #[test]
    fn the_bar_drags_by_its_handle_and_docks_at_the_nearest_spot() {
        let mut app = open_app(1280, 800, 1.0);
        app.work_inset = 40.0; // a taskbar
        let (_, size) = app.bar_size();
        assert_eq!(app.bar_at().y + size.height, 800.0 - 40.0 - bar::BOTTOM_GAP);
        let handle = app.bar_at()
            + Vector::new(
                widgets::SURFACE_PADDING + bar::HANDLE / 2.0,
                size.height / 2.0,
            );
        let grabbed = widgets::shot::click(app.view(app.overlay), app.screen, handle);
        assert!(grabbed.iter().any(|m| matches!(m, Message::BarGrab)));
        // the field and the buttons don't drag it
        let field = widgets::shot::click(app.view(app.overlay), app.screen, bar_field(&app));
        assert!(!field.iter().any(|m| matches!(m, Message::BarGrab)));
        // held, it follows the pointer
        let before = app.bar_at();
        let _ = app.update(Message::BarGrab);
        let _ = app.update(Message::BarMoved(handle));
        let _ = app.update(Message::BarMoved(handle - Vector::new(100.0, 560.0)));
        assert_eq!(app.bar_at(), before - Vector::new(100.0, 560.0));
        // let go near the top: it glides to the top spot, remembered
        let _ = app.update(Message::BarReleased);
        assert_eq!(app.bar_anchor, BarAnchor::Top);
        assert_eq!(config::current().bar.anchor, BarAnchor::Top, "remembered");
        let now = Instant::now();
        let _ = app.update(Message::Frame(now));
        let _ = app.update(Message::Frame(now + Duration::from_millis(16)));
        let step = app.bar_at();
        assert!(
            step != before - Vector::new(100.0, 560.0) && app.bar_gliding(),
            "gliding"
        );
        settle_bar(&mut app);
        assert_eq!(app.bar_at().y, bar::TOP);
        // the next opening finds it there
        let mut next = open_app(1280, 800, 1.0);
        next.bar_anchor = config::current().bar.anchor;
        assert_eq!(next.bar_at().y, bar::TOP);
        // a selection where the bar is: it moves to a free spot, and comes back
        let bar_box = Rectangle::new(next.bar_at(), size);
        drag(
            &mut next,
            bar_box.position() + Vector::new(50.0, -20.0),
            bar_box.position() + Vector::new(300.0, 120.0),
            false,
        );
        assert!(next.selection.is_some());
        settle_bar(&mut next);
        let sel = next.view.rect_to_screen(next.selection.unwrap().area);
        let (_, size) = next.bar_size();
        assert!(
            !Rectangle::new(next.bar_at(), size).intersects(&sel),
            "never covers it"
        );
        let _ = next.update(Message::Escape); // closes the overlay
        let mut back = open_app(1280, 800, 1.0);
        back.bar_anchor = BarAnchor::Top;
        settle_bar(&mut back);
        assert_eq!(back.bar_at().y, bar::TOP, "free again: home");
        // dropped far off screen: the nearest spot, inside the work area
        let _ = back.update(Message::BarGrab);
        let _ = back.update(Message::BarMoved(Point::new(10.0, 10.0)));
        let _ = back.update(Message::BarMoved(Point::new(5000.0, 5000.0)));
        let _ = back.update(Message::BarReleased);
        assert_eq!(back.bar_anchor, BarAnchor::BottomRight);
        let mut cfg = config::current();
        cfg.bar.anchor = BarAnchor::Bottom;
        config::set_current(cfg);
    }

    #[test]
    fn the_bottom_bar_has_every_main_action_on_any_screen() {
        for (w, h) in [(1280, 800), (800, 600), (640, 480)] {
            let app = open_app(w, h, 1.0);
            let _ = widgets::tests_support::shown(); // drop older records
            let click = |at| widgets::shot::click(app.view(app.overlay), app.screen, at);
            let translate = click(bar_pill(&app, 2));
            let messages: Vec<Vec<Message>> = (0..3).map(|k| click(bar_button(&app, k))).collect();
            let is = |k: usize, f: fn(&Message) -> bool| messages[k].iter().any(f);
            assert!(
                (translate.iter()).any(|m| matches!(m, Message::Pill(BarButton::Translate))),
                "{w}: Translate"
            );
            assert!(is(0, |m| matches!(m, Message::LiveToggle)), "{w}: Live");
            assert!(
                is(1, |m| matches!(m, Message::SongStart)),
                "{w}: music, key or not"
            );
            assert!(is(2, |m| matches!(m, Message::EditScreen)), "{w}: Edit");
            // each names itself (icons) and its shortcut in its tooltip
            let tips = widgets::tests_support::shown();
            for want in [
                "Live translate (Beta) · Ctrl+Shift+L",
                "Song search",
                "Edit the screen · E",
            ] {
                assert!(tips.iter().any(|t| t == want), "{want}: {tips:?}");
            }
            widgets::tests_support::assert_tips_add_something("bottom bar");
        }
    }

    #[test]
    fn the_bottom_bar_buttons_work_on_the_whole_screen() {
        // without a song key the music button opens its setup: nothing is recorded
        let mut app = open_app(800, 600, 1.0);
        let _ = app.song(false);
        assert!(
            app.song.is_none() && app.frame.is_none(),
            "closed, not listening"
        );
        assert!(app.windows.busy(), "Settings opened (Song search)");
        // Ask AI with an empty field: the panel with its presets, on the whole screen
        let mut app = open_app(800, 600, 1.0);
        let _ = app.update(Message::AskButton);
        assert_eq!(
            app.panel.as_ref().map(|(_, p)| p.kind()),
            Some(actions::Panel::Ask)
        );
        // the same keys with nothing selected: A asks, E edits, R translates, all the screen
        let mut app = open_app(800, 600, 1.0);
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'e',
                ctrl: false,
            },
        );
        assert!(app.edit.is_some(), "the whole screen in the editor");
        assert_eq!(
            app.selection.map(|s| s.area),
            Some(Rectangle::with_size(app.screen))
        );
        let mut app = open_app(800, 600, 1.0);
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'r',
                ctrl: false,
            },
        );
        assert!(app.translate.is_some());
        let mut app = open_app(800, 600, 1.0);
        event(
            &mut app,
            overlay::Event::Key {
                letter: 'a',
                ctrl: false,
            },
        );
        assert!(app.panel.is_some());
    }

    /// The opening as drawn `ms` after its first frame (`None`: the first frame).
    /// (A frame every 10 ms.)
    fn opening_at(ms: Option<u64>) -> App {
        let mut app = text_app();
        app.appear = None;
        if let Some(ms) = ms {
            let start = Instant::now();
            for at in (0..=ms).step_by(10).chain([ms]) {
                let _ = app.update(Message::Frame(start + Duration::from_millis(at)));
            }
        }
        app
    }

    #[test]
    fn the_opening_is_one_eased_animation_from_its_first_frame() {
        // frame 1 is t = 0, already dimmed: the window fades in as a whole
        // (`windows::fade`), the dim never fades over the whole screen
        let first = opening_at(None);
        assert_eq!(first.appeared(), 0.0);
        let shot = |app: &App| widgets::shot::render(app.view(app.overlay), 960, 420);
        let bright = |rgba: &[u8]| {
            let at = (300 * 960 + 700) * 4; // an empty part of the page
            u32::from(rgba[at]) + u32::from(rgba[at + 1]) + u32::from(rgba[at + 2])
        };
        let frame = match &first.frame {
            Some(image::Handle::Rgba { pixels, .. }) => pixels.to_vec(),
            _ => unreachable!(),
        };
        let dimmed = bright(&shot(&first));
        assert!(dimmed < bright(&frame), "dimmed on frame 1");
        // then it only ever moves one way, to rest by APPEAR; the dim stays
        let steps: Vec<(f32, u32)> = (0..=300)
            .step_by(25)
            .map(|ms| {
                let app = opening_at(Some(ms));
                (app.appeared(), bright(&shot(&app)))
            })
            .collect();
        for pair in steps.windows(2) {
            assert!(pair[1].0 >= pair[0].0 && pair[1].1 == dimmed, "{steps:?}");
        }
        assert!(
            steps[1].0 > 0.0 && steps[1].0 < 1.0,
            "eased in steps, no jump"
        );
        let done = opening_at(Some(overlay::APPEAR.as_millis() as u64));
        assert_eq!(done.appeared(), 1.0);
        assert!(!done.appearing(), "no more frames asked for");
        assert!(opening_at(Some(10)).appearing());
        // by the clock from the first frame: one slow frame (a busy machine)
        // lands where 60 fps would be, neither ahead nor behind
        let mut slow = opening_at(Some(0));
        let start = slow.appear.unwrap().0;
        let _ = slow.update(Message::Frame(start + Duration::from_millis(100)));
        let smooth = opening_at(Some(100));
        assert_eq!(slow.appeared(), smooth.appeared());
        assert!(
            slow.appeared() > 0.5 && slow.appearing(),
            "{}",
            slow.appeared()
        );
        // a new opening starts again at t = 0
        let mut again = done;
        let _ = again.open(Instant::now(), a_shot());
        assert_eq!(again.appeared(), 0.0);
    }

    /// `OPENCIRCLE_RENDER_DIR=<dir> cargo test opening_frames -- --ignored`:
    /// the overlay's first 300 ms, a PNG every 25 ms; and the time a full
    /// frame of the opening takes at 3440x1440 (the first frame: every pixel
    /// drawn, while the window is still clear).
    #[test]
    #[ignore = "writes PNGs and prints timings"]
    fn opening_frames() {
        for ms in (0..=300).step_by(25) {
            let app = opening_at(Some(ms));
            let name = format!("opening-{ms:03}ms");
            widgets::shot::save(&name, app.view(app.overlay), Size::new(960.0, 420.0));
        }
        let mut app = open_app(3440, 1440, 1.0);
        app.appear = None;
        app.appear = Some((Instant::now(), 0.4));
        let t = Instant::now();
        for _ in 0..5 {
            widgets::shot::render(app.view(app.overlay), 3440, 1440);
        }
        println!("3440x1440 opening frame: {:?}", t.elapsed() / 5);
        let t = Instant::now();
        for _ in 0..5 {
            widgets::shot::render(Element::<'_, Message>::from(space()), 3440, 1440);
        }
        println!("3440x1440 empty frame (the harness): {:?}", t.elapsed() / 5);
        let frame = app.frame.clone().unwrap();
        let t = Instant::now();
        for _ in 0..5 {
            let only = image(frame.clone())
                .width(Fill)
                .height(Fill)
                .filter_method(image::FilterMethod::Nearest);
            widgets::shot::render(Element::<'_, Message>::from(only), 3440, 1440);
        }
        println!("3440x1440 the frame alone: {:?}", t.elapsed() / 5);
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
        let field = bar_field(&app);
        let keys = key_after(&app, Task::none(), Some(field), 'a');
        assert!(!keys.iter().any(|m| is_key(m, 'a')));
        assert!(
            keys.iter()
                .any(|m| matches!(m, Message::AskInput(t) if t == "a"))
        );
    }

    // --- #101: one focus rule. A focused text field takes the typed letters; the
    // single-letter shortcuts run only when none has focus. ---

    use widgets::shot::{self, Clip, Focus};

    /// The shortcut letters that reached the overlay.
    fn shortcuts(messages: &[Message]) -> String {
        (messages.iter())
            .filter_map(|m| match m {
                Message::Overlay(overlay::Event::Key {
                    letter,
                    ctrl: false,
                }) => Some(*letter),
                _ => None,
            })
            .collect()
    }

    /// An open overlay with a selection and a ready Ask AI panel (browser provider).
    fn app_with_ask_panel() -> (App, u32) {
        let mut app = open_app(800, 600, 1.0);
        drag(
            &mut app,
            Point::new(50.0, 50.0),
            Point::new(150.0, 120.0),
            false,
        );
        let crop = app.crop().unwrap();
        let _ = app.open_panel(actions::Panel::Ask, crop.clone(), None);
        let (id, _) = app.panel.as_ref().map(|(id, p)| (*id, p.kind())).unwrap();
        let setup = actions::ask::setup(&crop).map(Arc::new);
        let _ = app.update(Message::Panel(id, panel::Msg::Ready(setup)));
        (app, id)
    }

    /// An open overlay with a selection and a Text panel that has read `text`.
    fn app_with_text_panel(text: &str) -> App {
        let mut app = open_app(800, 600, 1.0);
        drag(
            &mut app,
            Point::new(50.0, 50.0),
            Point::new(150.0, 120.0),
            false,
        );
        let crop = app.crop().unwrap();
        let _ = app.open_panel(actions::Panel::Text, crop, None);
        let id = app.panel.as_ref().map(|(id, _)| *id).unwrap();
        let _ = app.update(Message::Panel(id, panel::Msg::Read(Ok(text.into()))));
        app
    }

    fn feed(
        app: &App,
        focus: Focus,
        events: &[iced::Event],
    ) -> (Vec<Message>, Vec<iced::event::Status>) {
        shot::feed(
            app.view(app.overlay),
            app.screen,
            focus,
            events,
            &mut Clip::default(),
        )
    }

    #[test]
    fn typing_in_the_bars_field_runs_no_shortcut() {
        let app = open_app(800, 600, 1.0);
        let (messages, _) = feed(&app, Focus::Id(ASK_INPUT), &shot::typing("real estate"));
        assert_eq!(shortcuts(&messages), "", "{messages:?}");
        let typed = (messages.iter()).filter(|m| matches!(m, Message::AskInput(_)));
        assert_eq!(typed.count(), "real estate".len());
        assert!(!messages.iter().any(|m| matches!(
            m,
            Message::Overlay(overlay::Event::Space | overlay::Event::Enter) | Message::AskSubmit
        )));
        // The same letters with no field focused are the shortcuts.
        let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("aterl"));
        assert_eq!(shortcuts(&messages), "aterl");
    }

    #[test]
    fn typing_in_the_ask_panels_input_runs_no_shortcut() {
        let (app, _) = app_with_ask_panel();
        let (messages, _) = feed(&app, Focus::Id(panel::INPUT), &shot::typing("real estate"));
        assert_eq!(shortcuts(&messages), "", "{messages:?}");
        let typed =
            (messages.iter()).filter(|m| matches!(m, Message::Panel(_, panel::Msg::Input(_))));
        assert_eq!(typed.count(), "real estate".len());
        let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("aterl"));
        assert_eq!(
            shortcuts(&messages),
            "aterl",
            "the panel doesn't take keys by itself"
        );
    }

    #[test]
    fn typing_in_the_text_panels_editor_runs_no_shortcut() {
        let app = app_with_text_panel("hello");
        let (messages, _) = feed(&app, Focus::First, &shot::typing("real estate"));
        assert_eq!(shortcuts(&messages), "", "{messages:?}");
        let edits = (messages.iter())
            .filter(|m| matches!(m, Message::Panel(_, panel::Msg::Edit(_))))
            .count();
        assert!(edits >= "real estate".len(), "{messages:?}");
        let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("aterl"));
        assert_eq!(shortcuts(&messages), "aterl");
    }

    #[test]
    fn a_translation_has_no_field_to_take_the_letters() {
        let mut app = open_app(800, 600, 1.0);
        let frame = app.frame.clone().unwrap();
        let (session, _) =
            translate::Session::start(frame, [0, 0, 800, 600], 1.0, app.cache.clone(), false);
        app.translate = Some((7, session));
        assert!(!app.bottom_bar_shown(), "no bar, so no ask field");
        // Tab has no bar to move through; the card's E / A stay the card's.
        let _ = app.on_overlay(overlay::Event::Tab { back: false });
        assert_eq!(app.bar_focus, None);
        let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("ea"));
        assert_eq!(shortcuts(&messages), "ea");
    }

    #[test]
    fn esc_leaves_a_focused_field_and_the_second_closes() {
        let mut app = open_app(800, 600, 1.0);
        let esc = shot::named(iced::keyboard::key::Named::Escape, false);
        // bar field
        let (messages, status) = feed(&app, Focus::Id(ASK_INPUT), &esc);
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Field(keys::Keyed::Left)))
        );
        assert_eq!(
            status[1],
            iced::event::Status::Captured,
            "the overlay is told a widget took it"
        );
        // the subscription turns that into EscapeTaken: nothing closes
        let _ = app.update(Message::EscapeTaken);
        assert!(app.frame.is_some() && app.hiding == Hiding::No);
        // with the field let go, letters are shortcuts again
        let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("t"));
        assert_eq!(shortcuts(&messages), "t");
        // the second Esc reaches nobody: it closes as today
        let (_, status) = feed(&app, Focus::Nothing, &esc);
        assert_eq!(status[1], iced::event::Status::Ignored);
        let _ = app.update(Message::Escape);
        assert!(app.frame.is_none());
        // the Ask panel's input and the Text panel's editor (which lets Esc through)
        let (app, _) = app_with_ask_panel();
        let (messages, status) = feed(&app, Focus::Id(panel::INPUT), &esc);
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Panel(_, panel::Msg::Key(keys::Keyed::Left))))
        );
        assert_eq!(status[1], iced::event::Status::Captured);
        let app = app_with_text_panel("hello");
        let (messages, status) = feed(&app, Focus::First, &esc);
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Panel(_, panel::Msg::Key(keys::Keyed::Left))))
        );
        assert_eq!(
            status[1],
            iced::event::Status::Captured,
            "Esc must not also close the overlay"
        );
    }

    #[test]
    fn ctrl_c_in_a_field_copies_its_selection_else_the_capture() {
        let mut app = open_app(800, 600, 1.0);
        drag(
            &mut app,
            Point::new(100.0, 80.0),
            Point::new(400.0, 280.0),
            false,
        );
        app.ask = "hello".into();
        let run = |app: &App, events: Vec<iced::Event>| {
            let mut clip = Clip::default();
            let (messages, _) = shot::feed(
                app.view(app.overlay),
                app.screen,
                Focus::Id(ASK_INPUT),
                &events,
                &mut clip,
            );
            (messages, clip.0)
        };
        let copy = |m: &Message| {
            matches!(m, Message::Field(keys::Keyed::Copy))
                || matches!(
                    m,
                    Message::Overlay(overlay::Event::Key {
                        letter: 'c',
                        ctrl: true
                    })
                )
        };
        // text selected (Ctrl+A): the field's own copy, the capture is not copied
        let select_all = [shot::chord('a'), shot::chord('c')].concat();
        let (messages, clip) = run(&app, select_all);
        assert_eq!(clip, ["hello"]);
        assert!(!messages.iter().any(copy), "{messages:?}");
        // nothing selected: the overlay's copy of the capture
        let (messages, clip) = run(&app, shot::chord('c'));
        assert!(clip.is_empty());
        let copy_message = messages
            .into_iter()
            .find(copy)
            .expect("the capture is copied");
        let _ = copy::copied::image();
        let task = app.update(copy_message);
        run_task(&mut app, task);
        assert!(copy::copied::image().is_some(), "the capture was copied");
        // Ctrl+S saves even with the field focused (and text selected)
        // (the copy closed that overlay: a fresh one)
        let mut app = open_app(800, 600, 1.0);
        drag(
            &mut app,
            Point::new(100.0, 80.0),
            Point::new(400.0, 280.0),
            false,
        );
        app.ask = "hello".into();
        let (messages, _) = run(&app, shot::chord('s'));
        assert!(messages.iter().any(|m| matches!(
            m,
            Message::Overlay(overlay::Event::Key {
                letter: 's',
                ctrl: true
            })
        )));
    }

    #[test]
    fn tab_walks_the_bar_in_the_designs_order_and_enter_or_space_press() {
        let mut app = open_app(800, 600, 1.0);
        let tab = |app: &mut App, back| {
            let _ = app.on_overlay(overlay::Event::Tab { back });
            app.bar_focus
        };
        use bar::Stop::{Badge, Button, Field, Logo};
        let forward: Vec<_> = (0..8).map(|_| tab(&mut app, false)).collect();
        let order = [
            Logo,
            Field,
            Badge,
            Button(BarButton::Translate), // Search in browser and Copy text are dimmed
            Button(BarButton::Live),
            Button(BarButton::Song),
            Button(BarButton::Edit),
            Logo, // and round again
        ];
        assert_eq!(forward, order.map(Some));
        assert_eq!(tab(&mut app, true), Some(Button(BarButton::Edit)));
        // Tab events come from the key itself, Shift+Tab goes back
        let (messages, _) = feed(
            &app,
            Focus::Nothing,
            &shot::named(iced::keyboard::key::Named::Tab, true),
        );
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Overlay(overlay::Event::Tab { back: true })))
        );
        // a selection brings the two pills into the order, after the badge
        app.select(Rectangle::new(
            Point::new(100.0, 80.0),
            Size::new(300.0, 200.0),
        ));
        app.bar_focus = Some(Badge);
        assert_eq!(tab(&mut app, false), Some(Button(BarButton::Search)));
        assert_eq!(tab(&mut app, false), Some(Button(BarButton::CopyText)));
        // Enter and Space press the focused control
        let mut app = open_app(800, 600, 1.0);
        app.bar_focus = Some(Button(BarButton::Translate));
        let _ = app.on_overlay(overlay::Event::Space);
        assert_eq!(app.pill, BarButton::Translate);
        let mut app = open_app(800, 600, 1.0);
        app.bar_focus = Some(Logo);
        let _ = app.on_overlay(overlay::Event::Enter);
        assert!(app.panel.is_some(), "the logo opens the AI chat");
        // Space with nothing focused does nothing
        let mut app = open_app(800, 600, 1.0);
        let _ = app.on_overlay(overlay::Event::Space);
        assert!(app.panel.is_none() && app.bar_focus.is_none());
        // a click puts the focus back on the overlay
        app.bar_focus = Some(Badge);
        let _ = app.on_overlay(overlay::Event::Press {
            at: Point::new(5.0, 5.0),
            shift: false,
        });
        assert_eq!(app.bar_focus, None);
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
            text_reader: TextReader::Auto,
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
        // the defaults: snap, then wait for the user's choice (N-03)
        assert_eq!(
            config::Config::default().selection,
            behaviour(snap, After::Wait, SearchWith::Lens)
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
        app.purpose = Purpose::Snip(Box::new(Snip::new(Vec::new(), app.place(), false, None)));
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

    /// A snapped box on `text_app`'s page, the screen's text read.
    fn boxed_text(area: Rectangle) -> App {
        let mut app = read_app();
        app.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
        let (sel, stroke) = overlay::snap(area);
        (app.selection, app.stroke) = (Some(sel), stroke);
        app
    }

    fn rect4(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn a_snapped_box_resizes_by_its_edges_and_corners() {
        let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
        // the top edge, up 10: only that side moves (grabbed 1 px off it)
        drag(
            &mut app,
            Point::new(300.0, 21.0),
            Point::new(300.0, 11.0),
            false,
        );
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Snap);
        assert_eq!(app.area(), rect4(20.0, 10.0, 520.0, 180.0));
        // the right edge, in 100; the bottom-left bracket, out
        drag(
            &mut app,
            Point::new(541.0, 100.0),
            Point::new(441.0, 100.0),
            false,
        );
        assert_eq!(app.area(), rect4(20.0, 10.0, 420.0, 180.0));
        drag(
            &mut app,
            Point::new(17.0, 193.0),
            Point::new(7.0, 203.0),
            false,
        );
        assert_eq!(app.area(), rect4(10.0, 10.0, 430.0, 190.0));
        // the actions take the adjusted box, and it is remembered for the next time
        assert_eq!(app.regions.last().map(|r| r.1), Some(app.area()));
        // never off the screen (960x420), never under the minimum
        drag(
            &mut app,
            Point::new(441.0, 100.0),
            Point::new(5000.0, 100.0),
            false,
        );
        assert_eq!(app.area().x + app.area().width, 960.0);
        drag(
            &mut app,
            Point::new(961.0, 100.0),
            Point::new(-900.0, 100.0),
            false,
        );
        assert_eq!(app.area().width, overlay::MIN_SIDE);
    }

    #[test]
    fn a_snapped_box_moves_from_inside_but_text_inside_still_selects() {
        let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
        // the empty margin left of the text: move, stopped at the screen's edge
        drag(
            &mut app,
            Point::new(40.0, 100.0),
            Point::new(60.0, 110.0),
            false,
        );
        assert_eq!(app.area(), rect4(40.0, 30.0, 520.0, 170.0));
        let boxed = rect4(20.0, 20.0, 520.0, 170.0);
        app.selection = Some(overlay::snap(boxed).0);
        drag(
            &mut app,
            Point::new(40.0, 100.0),
            Point::new(-900.0, 900.0),
            false,
        );
        assert_eq!(app.area(), rect4(0.0, 250.0, 520.0, 170.0));
        // on a word: text is selected, the box stays where it was
        app.selection = Some(overlay::snap(rect4(20.0, 20.0, 520.0, 170.0)).0);
        stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
    }

    #[test]
    fn alt_drag_moves_the_box_even_over_text() {
        let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
        event(&mut app, overlay::Event::Alt(true));
        let on_text = caret(1, 7.0);
        drag(&mut app, on_text, on_text + Vector::new(20.0, 10.0), false);
        assert_eq!(app.area(), rect4(40.0, 30.0, 520.0, 170.0));
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Snap);
        // a bracket still resizes with Alt held
        drag(
            &mut app,
            Point::new(37.0, 33.0),
            Point::new(27.0, 23.0),
            false,
        );
        assert_eq!(app.area(), rect4(30.0, 20.0, 530.0, 180.0));
        // Alt let go: text selects again
        event(&mut app, overlay::Event::Alt(false));
        stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
        assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
    }

    fn arrow(app: &mut App, dx: i8, dy: i8, ctrl: bool, shift: bool) {
        event(
            app,
            overlay::Event::Arrow {
                dx,
                dy,
                ctrl,
                shift,
            },
        );
    }

    #[test]
    fn arrows_nudge_and_resize_the_box_within_the_screen() {
        let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
        arrow(&mut app, 1, 0, false, false);
        arrow(&mut app, 0, 1, false, true);
        assert_eq!(app.area(), rect4(21.0, 30.0, 520.0, 170.0));
        arrow(&mut app, 1, 1, true, false);
        arrow(&mut app, 1, 0, true, true);
        assert_eq!(app.area(), rect4(21.0, 30.0, 531.0, 171.0));
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Snap);
        // the actions take the adjusted box
        assert_eq!(app.regions.last().map(|r| r.1), Some(app.area()));
        // stops at the screen (960x420) and at the minimum size
        for _ in 0..30 {
            arrow(&mut app, -1, -1, false, true);
        }
        assert_eq!(app.area(), rect4(0.0, 0.0, 531.0, 171.0));
        for _ in 0..100 {
            arrow(&mut app, 1, 1, true, true);
        }
        assert_eq!((app.area().width, app.area().height), (960.0, 420.0));
        for _ in 0..100 {
            arrow(&mut app, -1, -1, true, true);
        }
        let min = overlay::MIN_SIDE;
        assert_eq!(app.area().size(), Size::new(min, min));
        for _ in 0..200 {
            arrow(&mut app, 1, 1, false, true);
        }
        let a = app.area();
        assert_eq!((a.x + a.width, a.y + a.height), (960.0, 420.0));
    }

    #[test]
    fn arrows_do_nothing_while_a_text_field_has_the_focus() {
        use iced::keyboard::key::{Named, NativeCode, Physical};
        use iced::keyboard::{Key, Location, Modifiers};
        let app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
        let key = Key::Named(Named::ArrowRight);
        let press = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        });
        let arrows = |m: &[Message]| {
            (m.iter())
                .filter(|m| matches!(m, Message::Overlay(overlay::Event::Arrow { .. })))
                .count()
        };
        let (messages, _) = feed(&app, Focus::Id(ASK_INPUT), std::slice::from_ref(&press));
        assert_eq!(arrows(&messages), 0, "{messages:?}");
        let (messages, _) = feed(&app, Focus::Nothing, &[press]);
        assert_eq!(arrows(&messages), 1, "{messages:?}");
    }

    /// Draw, release, snap, then act: during the press only the stroke is there,
    /// after the release the actions wait for the morph to end.
    #[test]
    fn the_selections_actions_wait_for_the_snap_to_finish() {
        let mut app = open_app(1280, 800, 1.0);
        app.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
        let pill = |app: &App| {
            let got = widgets::shot::click(app.view(app.overlay), app.screen, bar_pill(app, 0));
            got.iter().any(|m| matches!(m, Message::Pill(_)))
        };
        let ring = ring(300.0, 250.0, 80.0);
        event(
            &mut app,
            overlay::Event::Press {
                at: ring[0],
                shift: false,
            },
        );
        for &p in &ring[1..] {
            event(&mut app, overlay::Event::Move(p));
            // the press: the stroke and nothing else (no box, no snap, no actions)
            assert!(app.selection.is_none() && app.morph.is_none() && !app.settling());
            assert!(app.drawing && !pill(&app));
        }
        event(&mut app, overlay::Event::Release);
        assert!(app.selection.is_some() && app.settling(), "morphing");
        assert!(!pill(&app), "no actions mid-morph");
        // the morph ends: the actions are there
        let (from, _) = app.morph.take().unwrap();
        app.morph = Some((from, Instant::now() - overlay::MORPH * 2));
        assert!(!app.settling() && pill(&app));
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
        assert!(app.auto_actions().is_empty(), "not while it snaps");
        let (from, _) = app.morph.take().unwrap();
        app.morph = Some((from, Instant::now() - overlay::MORPH * 2));
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
        // press before "while", drag to after "words": two lines picked, the box stays
        drag(&mut app, caret(1, 7.0), caret(2, 16.0), false);
        let mode = app.text_mode.as_ref().unwrap();
        let picked = mode.picked.as_ref().unwrap();
        assert_eq!(picked.text(), "while you drag a finger\nacross the words");
        assert!(!mode.picking);
        assert_eq!(app.selection.unwrap().kind, overlay::Kind::Snap);
        // the bottom bar's text actions take the picked words; no pill, no second bar
        let texts = widgets::shot::texts(app.view(app.overlay));
        assert!(
            !texts.iter().any(|t| t == "Copy" || t == "Edit"),
            "{texts:?}"
        );
        let click = |k| widgets::shot::click(app.view(app.overlay), app.screen, bar_pill(&app, k));
        assert!(
            click(1)
                .iter()
                .any(|m| matches!(m, Message::Pill(BarButton::CopyText)))
        );
        // Ctrl+C copies the same picked characters, not the whole box's text
        let mut keyed = select_text_app();
        drag(&mut keyed, caret(1, 9.4), caret(2, 16.0), false);
        let _ = copy::copied::text();
        let ctrl_c = overlay::Event::Key {
            letter: 'c',
            ctrl: true,
        };
        let task = keyed.update(Message::Overlay(ctrl_c));
        run_task(&mut keyed, task);
        assert_eq!(keyed.windows.toasts(), [("Copied".to_string(), true)]);
        assert_eq!(
            copy::copied::text().as_deref(),
            Some("ile you drag a finger\nacross the words")
        );
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
    fn lens_in_the_browser_closes_the_overlay() {
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
        // Closed on every OS: nothing of ours stays on screen after the hand-off.
        assert!(app.frame.is_none());
        assert!(app.panel.is_none());
    }

    #[test]
    fn the_result_card_opens_off_the_selection_and_moves_by_its_grip() {
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
        // the grip: the card follows the pointer while held
        let id = app.panel.as_ref().unwrap().0;
        let grab = panel::Msg::Card(card::Event::Grab(card::Drag::Move));
        let _ = app.update(Message::Panel(id, grab));
        let _ = app.update(Message::CardMoved(Point::new(1700.0, 200.0)));
        let _ = app.update(Message::CardMoved(Point::new(1100.0, 200.0)));
        assert_eq!(
            app.card_at().position(),
            at.position() + Vector::new(-600.0, 0.0)
        );
        // let go: it stays there (nothing is saved), no more moves
        let _ = app.update(Message::CardReleased);
        let _ = app.update(Message::CardMoved(Point::new(0.0, 0.0)));
        assert_eq!(
            app.card_at().position(),
            at.position() + Vector::new(-600.0, 0.0)
        );
        // the default place again; a selection under it: this one goes left
        let _ = app.update(Message::Panel(id, panel::Msg::Card(card::Event::Reset)));
        assert_eq!(app.card_at(), at);
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
