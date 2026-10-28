//! Every message the overlay's `update` takes, and the small types they carry.

use std::time::Instant;

use iced::widget::image;
use iced::{Point, Rectangle, Size, window};

use crate::Trigger;
use crate::actions::{self, Action, lens, qr, text};
use crate::capture::CaptureError;
use crate::config::{self, Target};
use crate::editor;
use crate::labs::BarButton;
use crate::ocr::TextBlock;
use crate::ui::app::{keys, quick::Quick, word};
use crate::ui::screens::bar::geometry as bar;
use crate::ui::snip::{self, Snip};
use crate::ui::{live_layer, overlay, panel, song, translate, windows};

#[derive(Debug, Clone)]
pub(in crate::ui) enum Message {
    Trigger(Trigger),
    /// Capture number `.0`'s thread is done (`Instant` = trigger time, latency log).
    Captured(u32, Instant, Result<Shot, CaptureError>),
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
pub(in crate::ui) struct BarKeys {
    pub(in crate::ui) search: String,
    pub(in crate::ui) copy_text: String,
    pub(in crate::ui) translate: String,
    pub(in crate::ui) live: String,
    pub(in crate::ui) song: String,
    pub(in crate::ui) edit: String,
}

impl BarKeys {
    pub(in crate::ui) fn of(cfg: &config::Config) -> BarKeys {
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
pub(in crate::ui) enum PickAct {
    Copy,
    Translate,
    Search,
}

/// What waits for the band under it to be read (main mode).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::ui) enum Await {
    /// A tap: the word there (a triple tap: the line) becomes the selection.
    Tap(word::Unit),
    /// A Shift+click: the text selection stretches there.
    Extend,
}

/// What an OCR read for words was asked for, to place its answer.
#[derive(Debug, Clone, Copy)]
pub(in crate::ui) struct WordRead {
    /// The tap's number (a newer press makes older reads stale).
    pub(in crate::ui) id: u32,
    /// The tap, or where the drag ended (image px).
    pub(in crate::ui) at: Point,
    /// The crop's top-left (frame px) and extent (image px).
    pub(in crate::ui) origin: [u32; 2],
    pub(in crate::ui) region: Rectangle,
    /// Stretch the picked words to `at` (else: pick the word at `at`).
    pub(in crate::ui) extend: bool,
}

/// A press on the selection: resizing it by a corner or an edge, or moving it.
pub(in crate::ui) struct Grab {
    /// The corner or edge, or `None` = move.
    pub(in crate::ui) grip: Option<overlay::Grip>,
    /// Where the press was (image px).
    pub(in crate::ui) from: Point,
    /// The selection and its stroke when the press began.
    pub(in crate::ui) start: (overlay::Selection, Vec<Point>),
    /// It moved further than a tap.
    pub(in crate::ui) moved: bool,
}

/// What the open overlay is for.
pub(in crate::ui) enum Purpose {
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
pub(in crate::ui) enum Hiding {
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
pub(in crate::ui) struct Shot {
    pub(in crate::ui) image: image::Handle,
    pub(in crate::ui) scale: f32,
    /// Logical px: the monitor's origin and size.
    pub(in crate::ui) origin: Point,
    pub(in crate::ui) size: Size,
    /// Snip mode: the mode to start in, every window (physical desktop px,
    /// topmost first), the window the user was in (Windows), and every monitor
    /// with its frame (this one too).
    pub(in crate::ui) snip: Option<SnipShot>,
}

pub(in crate::ui) type SnipShot = (
    snip::Kind,
    Vec<snip::Pick>,
    Option<isize>,
    Vec<(snip::Place, image::Handle)>,
);
