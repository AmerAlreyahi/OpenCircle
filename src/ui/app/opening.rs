//! `Opening`: everything one capture holds, from `open` to `close`; `App::o`
//! holds it only while the overlay is open (`Open`). Closing takes it whole,
//! so no field (the frame, its tiles, the text read) outlives its opening.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Instant;

use iced::widget::image;
use iced::{Point, Rectangle, Size, Vector};

use crate::actions::{lens, qr};
use crate::config::{After, BarAnchor, SearchWith, SelectStyle, SelectionConfig, TextReader};
use crate::labs::{BarButton, Labs};
use crate::ocr;
use crate::ui::app::message::{Await, BarKeys, Grab, Purpose};
use crate::ui::app::word;
use crate::ui::app::{Reader, TextMode};
use crate::ui::edit::Editing;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame as card;
use crate::ui::screens::bar::geometry as bar;
use crate::ui::{overlay, panel, song, translate};

pub(in crate::ui) struct Opening {
    /// The frozen frame while the overlay is open; `None` = hidden, frame freed.
    /// Its RGBA bytes are also what actions crop (shared, never copied whole).
    pub(in crate::ui) frame: Option<image::Handle>,
    /// The frame cut into tiles for drawing (`overlay::tiles`), and its id.
    pub(in crate::ui) tiles: Option<(iced::advanced::image::Id, Vec<(Point, image::Handle)>)>,
    /// Physical px per logical px of `frame`.
    pub(in crate::ui) scale: f32,
    /// The overlay's logical size (the monitor's).
    pub(in crate::ui) screen: Size,
    /// The stroke being drawn or the selection's (image px; see `overlay::View`).
    pub(in crate::ui) stroke: Vec<Point>,
    pub(in crate::ui) drawing: bool,
    pub(in crate::ui) selection: Option<overlay::Selection>,
    /// Selections kept with Shift before the current one, with their strokes.
    pub(in crate::ui) others: Vec<(overlay::Selection, Vec<Point>)>,
    /// The stroke being drawn adds to `others` (Shift).
    pub(in crate::ui) adding: bool,
    /// Alt is held: a press inside the box moves it, even over text.
    pub(in crate::ui) alt: bool,
    /// Resizing or moving the selection.
    pub(in crate::ui) grab: Option<Grab>,
    /// The zoomed / panned part of the frame on screen; a pan's start.
    pub(in crate::ui) view: overlay::View,
    pub(in crate::ui) pan: Option<(Point, overlay::View)>,
    /// The tapped word(s) the selection is (the tap's number: `App::taps`).
    pub(in crate::ui) word: Option<word::Picked>,
    /// Main mode: the frame's text lines (image px), read on demand band by
    /// band (#123): never at open, the band the user touches first.
    pub(in crate::ui) screen_text: word::Screen,
    /// The band OCR reads now (one at a time: the helper serves one request
    /// at a time anyway).
    pub(in crate::ui) reading: Option<Rectangle>,
    /// A tap or Shift+click waiting for the band under it.
    pub(in crate::ui) awaits: Option<(Point, Await)>,
    /// Ctrl+A waits for the whole screen to be read.
    pub(in crate::ui) want_all: bool,
    /// A drag let go before the band under its press was read, and the
    /// overlay events after it: replayed once it is.
    pub(in crate::ui) held: Option<Vec<overlay::Event>>,
    /// Text was selected in this opening (or, off Windows, its helper
    /// started: `Warmed`): the rest of the screen is read in the background
    /// (gently), nearest the pointer first.
    pub(in crate::ui) text_used: bool,
    /// Where the pointer was last seen (image px).
    pub(in crate::ui) pointer: Point,
    /// The text reader's models are here (checked at open).
    pub(in crate::ui) ocr_ready: bool,
    /// How that read is going (the note over the bar).
    pub(in crate::ui) reader: Reader,
    /// Copy text asked for the OCR models: copy once they are downloaded.
    pub(in crate::ui) copy_after: bool,
    /// Who reads this opening's text (`ocr::route`), once its first read
    /// started: Windows OCR (the whole screen at once) or the helper (bands).
    pub(in crate::ui) text_by: Option<ocr::Source>,
    /// A press that began on text: its stroke so far (image px). It selects
    /// words unless it turns into a loop (`overlay::winds`).
    pub(in crate::ui) text_press: Option<Vec<Point>>,
    /// The opening animation (`overlay::APPEAR`): the latest frame's time and
    /// how far it is (0..1, linear); `None` until the first frame (t = 0).
    pub(in crate::ui) appear: Option<(Instant, f32)>,
    /// The bar's highlighted pill: Search in browser at every opening, then the
    /// one last pressed (the new design's active pill).
    pub(in crate::ui) pill: BarButton,
    /// The shortcuts the bottom bar's tooltips show (read at open).
    pub(in crate::ui) bar_keys: BarKeys,
    /// Which unfinished features are on (read at open; Settings changes apply to the next one).
    pub(in crate::ui) labs: Labs,
    /// The last press: when, where (image px), and how many in a row.
    pub(in crate::ui) clicks: Option<(Instant, Point, u8)>,
    /// The ask bar's text.
    pub(in crate::ui) ask: String,
    /// Shown but still fully clear (`show`): fades in once its first full
    /// frame is drawn (`first_draw` taken), or after `REVEAL_WAIT` anyway.
    pub(in crate::ui) reveal: Option<Instant>,
    /// The open result panel and its number.
    pub(in crate::ui) panel: Option<(u32, panel::Panel)>,
    /// Lens pages opened from this overlay, released when it closes (`finish`).
    pub(in crate::ui) lens_pages: Vec<lens::Held>,
    pub(in crate::ui) purpose: Purpose,
    /// The annotation editor on the selection; kept when going back to the bar.
    pub(in crate::ui) edit: Option<Editing>,
    /// The shown monitor's origin (logical px): the key of `App::regions`.
    pub(in crate::ui) origin: Point,
    /// A short status over the frame (copy text).
    pub(in crate::ui) toast: Option<&'static str>,
    /// What the current selection holds (QR code, protected capture); `None` until scanned.
    pub(in crate::ui) scan: Option<qr::Scan>,
    /// Translate in place on this overlay, and its number.
    pub(in crate::ui) translate: Option<(u32, translate::Session)>,
    /// One warm OCR / translation helper while the overlay is open (tap a
    /// word, Text, Translate), started by the first of them.
    /// (shared with the screen's text reads: one that starts after the
    /// overlay closed reads nothing, `read_band`)
    pub(in crate::ui) helper: Option<Arc<ocr::Hold>>,
    /// How selections behave (`[selection]`), read when the overlay opens.
    pub(in crate::ui) behaviour: SelectionConfig,
    /// A snapped selection's drawn stroke (image px) morphing into its box.
    pub(in crate::ui) morph: Option<(Vec<Point>, Instant)>,
    pub(in crate::ui) text_mode: Option<TextMode>,
    /// The bottom bar turning into the editing toolbar in its place: 0 = the
    /// bar, 1 = the toolbar (linear), and while it turns: since when, from
    /// and to where (`None`: at rest).
    pub(in crate::ui) swap: f32,
    pub(in crate::ui) swap_at: Option<(Instant, f32, f32)>,
    /// The editor just went back to the bar (until the next frame): an
    /// Escape now is the key that did it.
    pub(in crate::ui) esc_back: bool,
    /// Where Tab put the focus in the bottom bar (the field's own focus is the
    /// text input's); none = the overlay has it, so the letters are shortcuts.
    pub(in crate::ui) bar_focus: Option<bar::Stop>,
    /// The bar's spot (`config::BarConfig`); while its handle is held, the
    /// pointer's last position and where the bar follows it (top-left);
    /// where it is drawn now (gliding to its spot); how much of the screen's
    /// bottom the taskbar takes (logical px).
    pub(in crate::ui) bar_anchor: BarAnchor,
    pub(in crate::ui) bar_drag: Option<(Option<Point>, Point)>,
    pub(in crate::ui) bar_now: Option<Point>,
    /// Its glide to a new spot: since when, from and to where.
    pub(in crate::ui) bar_glide: Option<(Instant, Point, Point)>,
    /// While held: the last frame it was eased toward the pointer on.
    pub(in crate::ui) bar_eased: Option<Instant>,
    pub(in crate::ui) work_inset: f32,
    /// The card's grip or corner is held, and the pointer's last position.
    pub(in crate::ui) card_drag: Option<(card::Drag, Option<Point>)>,
    /// The result card opening (`motion::WIDGET`): which panel, its motion,
    /// and how open it was at the latest frame (it grows from `OPEN_SCALE`).
    pub(in crate::ui) card_open: Option<(u32, motion::Shown, f32)>,
    /// The pointer is on the card's resize edge (its grip shows).
    pub(in crate::ui) card_hover: bool,
    /// The AI chat card held away from its dock (logical px), and its glide
    /// back to a dock once let go: since when, from where, how far it is.
    pub(in crate::ui) chat_off: Vector,
    pub(in crate::ui) chat_glide: Option<(Instant, Rectangle, f32)>,
    /// Song search on this overlay, and its number.
    pub(in crate::ui) song: Option<(u32, song::Session)>,
}

impl Default for Opening {
    /// Nothing captured: what a closed overlay holds.
    fn default() -> Self {
        Opening {
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
            screen_text: word::Screen::default(),
            reading: None,
            awaits: None,
            want_all: false,
            held: None,
            text_used: false,
            pointer: Point::ORIGIN,
            ocr_ready: false,
            reader: Reader::Idle,
            copy_after: false,
            text_by: None,
            text_press: None,
            appear: None,
            pill: BarButton::Search,
            bar_keys: BarKeys::default(),
            labs: Labs::default(),
            clicks: None,
            ask: String::new(),
            reveal: None,
            panel: None,
            lens_pages: Vec::new(),
            purpose: Purpose::Search,
            edit: None,
            origin: Point::ORIGIN,
            toast: None,
            scan: None,
            translate: None,
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
            bar_anchor: BarAnchor::Bottom,
            bar_drag: None,
            bar_now: None,
            bar_eased: None,
            bar_glide: None,
            work_inset: 0.0,
            card_drag: None,
            card_open: None,
            card_hover: false,
            chat_off: Vector::ZERO,
            chat_glide: None,
            song: None,
        }
    }
}

impl Opening {
    /// Releases what dropping doesn't: the Lens pages. Everything else (panel,
    /// translate, song, helper, frame) stops or frees itself when dropped.
    pub(in crate::ui) fn finish(self) {
        self.lens_pages.into_iter().for_each(lens::Held::release);
    }
}

/// `App::o`: the open overlay's `Opening`, `None` while closed. `update` drops
/// every message that needs it while closed (`works_closed`), so handlers use
/// it straight (`self.o.frame`); read while closed it is an empty `Opening`.
#[derive(Default)]
pub(in crate::ui) struct Open(Option<Opening>);

thread_local! {
    /// What a closed overlay reads (nothing captured). Leaked once per thread
    /// (it is small and never changes) so `deref` can lend it.
    static CLOSED: &'static Opening = Box::leak(Box::default());
}

impl Open {
    pub(in crate::ui) fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub(in crate::ui) fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// A new opening (`App::open`).
    pub(in crate::ui) fn start(&mut self, opening: Opening) {
        self.0 = Some(opening);
    }

    /// Closing: the opening goes (`App::close`).
    pub(in crate::ui) fn take(&mut self) -> Option<Opening> {
        self.0.take()
    }
}

impl Deref for Open {
    type Target = Opening;

    fn deref(&self) -> &Opening {
        self.0.as_ref().unwrap_or_else(|| CLOSED.with(|c| *c))
    }
}

impl DerefMut for Open {
    /// Only while open (`works_closed` guards `update`). A path that slips
    /// through gets an empty opening (as before `Open`), never a crash; tests
    /// catch it.
    fn deref_mut(&mut self) -> &mut Opening {
        debug_assert!(self.0.is_some(), "the overlay's state changed while closed");
        self.0.get_or_insert_default()
    }
}
