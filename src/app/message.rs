//! Every message the overlay's `update` takes, and the small types they carry.

use std::time::Instant;

use iced::widget::image;
use iced::{Point, Rectangle, Size, window};

use crate::Trigger;
use crate::features::circle::screens::bar::geometry as bar;
use crate::features::settings::labs::BarButton;
use crate::features::snip::annotate;
use crate::features::snip::screens::pick::{self, Snip};
use crate::features::text_reading::TextBlock;
use crate::features::{
    circle::actions::{Action, qr, search_in_browser},
    text_reading::text,
};
use crate::shared::app_language::{S, Said};
use crate::shared::config::{self, Target};
use crate::shared::platform::capture::CaptureError;
use crate::{
    app::keys,
    features::{circle::app::quick::Quick, text_reading::app::word},
};
use crate::{
    app::windows,
    features::{
        ask_ai::screens::ai_chat, circle::screens::overlay, live_translate::screens::live_layer,
        song::screens::listen, translate::screens::pill,
    },
};

/// Where the overlay was when a Copy / Save ran: its origin, size and scale
/// (logical px), and what its pill goes beside (overlay px; `actions::pill_at`).
pub(crate) type PillPlace = (Point, Size, f32, Option<Rectangle>);

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Trigger(Trigger),
    /// Capture number `.0`'s thread is done (`Instant` = trigger time, latency log).
    Captured(u32, Instant, Result<Shot, CaptureError>),
    /// Capture number `.0`'s grab is over (every monitor's).
    Grabbed(u32),
    /// Capture number `.0` has waited `CAPTURE_TIMEOUT`.
    CaptureTimedOut(u32),
    /// Capture number `.0` has waited `CAPTURE_SLOW`: the desktop asks (its dialog).
    #[cfg(target_os = "linux")]
    CaptureSlow(u32),
    /// Snip capture number `.0` will open on this monitor (its grab runs).
    Chosen(u32, pick::Place),
    Overlay(overlay::Event),
    /// Another monitor's frame of capture number `.0` landed (`app::screens`).
    Screen(u32, pick::Place, image::Handle),
    /// From another monitor's overlay window (its logical px).
    ScreenEvent(window::Id, overlay::Event),
    /// The pointer came into one of our windows: the edge glow follows it
    /// onto that screen (`screens::Glow`).
    PointerOn(window::Id),
    /// Esc, wherever the focus is.
    Escape,
    /// Esc that a widget took (a field let go of the focus; the editor): it
    /// only closes what the widget does not own.
    EscapeTaken,
    /// A focused field gave a key back (`keys`).
    Field(keys::Keyed),
    /// From the panel with this number (stale numbers are ignored).
    AiChat(u32, ai_chat::Msg),
    /// The pinned chat's own window moved (dragged) to here; it stopped
    /// moving after this many moves (it docks); a frame of its spinner.
    ChatMoved(iced::window::Id, iced::Point),
    ChatSettle(u32),
    /// The pinned chat's own window is up over the overlay: the overlay can go.
    ChatHandedOver,
    /// The same with its window's scale: dock on the monitor it is on.
    ChatDocked(u32, f32),
    ChatFrame(std::time::Instant),
    /// Run an action on the selection (action bar button or its key).
    Run(&'static Action),
    TestShot(window::Screenshot),
    /// While closing: a frame was drawn / the window was resized (see `Hiding`).
    Drawn,
    Resized(Size),
    /// Settings / onboarding windows and the OS theme (`windows.rs`).
    Windows(windows::Msg),
    /// From the Snip annotation editor.
    Annotate(annotate::Message),
    Snip(pick::Msg),
    /// The copy-text quick action read the text and copied it (or why not).
    TextRead(Result<(), String>),
    /// Snip's More read the area's text (opening, which read; its lines).
    MoreRead(
        u32,
        u32,
        Result<Vec<crate::features::text_reading::TextBlock>, String>,
    ),
    /// A Copy / Save run off the UI thread is done: its pill, where the
    /// overlay was (`PillPlace`).
    Confirm(PillPlace, Result<String, String>),
    /// A link opened (or text copied) off the UI thread: the overlay closes,
    /// after this pill if any.
    ThenClose(Option<Said>),
    /// Lens is open in the browser (the page is held), or why it could not.
    SearchOpened(Result<(), String>),
    /// The toast has been shown long enough.
    ToastDone,
    /// The selection with this area was scanned (a stale area is ignored).
    Scanned(Rectangle, qr::Scan),
    /// The QR chip's Copy / Open link.
    Qr(qr::Act),
    /// The selection's link or email chip was clicked (the only thing that opens it).
    OpenEntity(crate::features::circle::actions::entities::Entity),
    /// From the translate session with this number (stale numbers are ignored).
    Translate(u32, pill::Msg),
    /// Alt went down / up (peek at the original under a translation).
    Peek(bool),
    Live(live_layer::Msg),
    /// OCR for a tap or a text drag read these lines.
    Tapped(WordRead, Result<Vec<TextBlock>, String>),
    /// The bottom bar: its ask field's text, Enter in it, its Translate button.
    AskInput(String),
    AskSubmit,
    /// The AI field's Include screen switch.
    IncludeScreen,
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
    /// The second reader's lines (other scripts) in this band of this
    /// opening's frame.
    ScreenOther(u32, Rectangle, Result<Vec<word::Line>, String>),
    /// Every line the helper found on this opening's frame (image px), the
    /// known ones aside: those not read yet are being read (#169).
    ScreenFound(u32, Vec<Rectangle>),
    /// The second reader read all there is on this opening's frame.
    ScreenOtherDone(u32),
    /// The helper read this many lines as noise with no reader for their
    /// script on disk (#169: the reader for the user's language is offered).
    ScreenNoise(u32, usize),
    /// Another screen's text (`Screen::lines`): the opening, its window, the read.
    ScreenLines(u32, window::Id, Result<Vec<word::Line>, String>),
    /// The text reader's helper of this opening has started (after its
    /// opening animation).
    Warmed(u32),
    /// The note over the bar's Download (the text reader's models), and how
    /// that download for this opening goes (`Reading` = done).
    ReaderDownload,
    /// The download card's Not now / Cancel: no note for this opening.
    ReaderDismiss,
    Reader(u32, Result<text::Progress, S>),
    /// The "Select text" chip under a snapped box, and the lines OCR read in it.
    SelectText,
    TextLines(Rectangle, Result<Vec<word::Line>, String>),
    /// The bottom bar's handle went down / the pointer moved / it was let go.
    BarGrab,
    BarMoved(Point),
    /// The pointer moved in that window (its px), while the bar may be held.
    BarMovedOn(window::Id, Point),
    BarReleased,
    /// While the result card is dragged: the pointer moved / was let go.
    CardMoved(Point),
    /// The same, in the px of the window the pointer is over.
    CardMovedOn(window::Id, Point),
    CardReleased,
    /// The music button (or the song_search quick action).
    SongStart,
    /// From the song search with this number (stale numbers are ignored).
    Song(u32, listen::Msg),
    /// Whether a song-search key is saved (checked once, off the UI thread).
    SongKey,
    /// Try the waiting update again (it waits while OpenCircle is in use).
    UpdateTick,
    /// A snapped stroke finished morphing into its box: the selection's actions show.
    Settled,
}

/// The shortcuts of the bottom bar's buttons, as the tooltips show them ("" = none).
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct BarKeys {
    pub(crate) search: String,
    pub(crate) copy_text: String,
    pub(crate) translate: String,
    pub(crate) live: String,
    pub(crate) song: String,
    pub(crate) edit: String,
}

impl BarKeys {
    pub(crate) fn of(cfg: &config::Config) -> BarKeys {
        let quick = |id: &str, live: bool| {
            let bound = (cfg.quick_actions.iter())
                .find(|q| q.action == id && q.live == live && !q.shortcut.is_empty());
            bound.map(|q| q.shortcut.clone()).unwrap_or_default()
        };
        let key = |a: &Action| bar::key_hint(a.key);
        BarKeys {
            search: key(&search_in_browser::ACTION),
            copy_text: quick("copy_text", false),
            translate: quick(crate::features::translate::action::ACTION.id, false),
            live: quick(crate::features::translate::action::ACTION.id, true),
            song: quick("song_search", false),
            edit: key(&crate::features::snip::action::ACTION),
        }
    }
}

/// What the select-text popup does with the picked words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickAct {
    Copy,
    Translate,
    Search,
}

/// What waits for the band under it to be read (main mode).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Await {
    /// A tap: the word there (a triple tap: the line) becomes the selection.
    Tap(word::Unit),
    /// A Shift+click: the text selection stretches there.
    Extend,
}

/// What an OCR read for words was asked for, to place its answer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WordRead {
    /// The tap's number (a newer press makes older reads stale).
    pub(crate) id: u32,
    /// The tap, or where the drag ended (image px).
    pub(crate) at: Point,
    /// The crop's top-left (frame px) and extent (image px).
    pub(crate) origin: [u32; 2],
    pub(crate) region: Rectangle,
    /// Stretch the picked words to `at` (else: pick the word at `at`).
    pub(crate) extend: bool,
}

/// A press on the selection: resizing it by a corner or an edge, or moving it.
pub(crate) struct Grab {
    /// The corner or edge, or `None` = move.
    pub(crate) grip: Option<overlay::Grip>,
    /// Where the press was (image px).
    pub(crate) from: Point,
    /// The selection and its stroke when the press began.
    pub(crate) start: (overlay::Selection, Vec<Point>),
    /// It moved further than a tap.
    pub(crate) moved: bool,
}

/// What the open overlay is for.
pub(crate) enum Purpose {
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
pub(crate) enum Hiding {
    No,
    /// Wait for one empty full-size frame (drops the cached frame).
    Empty,
    /// Left full-screen and asked for 1x1; wait for the resize to land (frames so far).
    Shrinking(u8),
    /// Wait for one frame at the small size (shrinks the buffers), then hide.
    Shrunk,
}

/// A captured monitor, ready to show.
#[derive(Debug, Clone)]
pub(crate) struct Shot {
    pub(crate) image: image::Handle,
    pub(crate) scale: f32,
    /// Logical px: the monitor's origin and size.
    pub(crate) origin: Point,
    pub(crate) size: Size,
    /// Snip mode: the mode to start in, every window (physical desktop px,
    /// topmost first), the window the user was in (Windows), and every monitor
    /// with its frame (this one too).
    pub(crate) snip: Option<SnipShot>,
    /// Main mode: every window at the freeze (physical desktop px, topmost
    /// first), for the bar's Snip; Snip's own are in `snip`.
    pub(crate) windows: Vec<pick::Pick>,
}

pub(crate) type SnipShot = (
    pick::Kind,
    Vec<pick::Pick>,
    Option<isize>,
    Vec<(pick::Place, image::Handle)>,
);
