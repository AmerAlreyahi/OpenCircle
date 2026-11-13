//! `Opening`: everything one capture holds, from `open` to `close`; `App::o`
//! holds it only while the overlay is open (`Open`). Closing takes it whole,
//! so no field (the frame, the text read) outlives its opening.

use crate::shared::app_language::Said;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Instant;

use iced::widget::image;
use iced::{Point, Rectangle, Size, Vector};

use crate::features::circle::actions::qr;
use crate::features::circle::screens::bar::geometry as bar;
use crate::features::settings::labs::{BarButton, Labs};
use crate::features::snip::screens::snip_toolbar::SnipToolbar;
use crate::features::text_reading as ocr;
use crate::features::text_reading::app::word;
use crate::shared::config::{BarAnchor, SelectStyle, SelectionConfig, TextReader};
use crate::shared::kit::motion;
use crate::shared::kit::panel_frame as card;
use crate::ui::app::message::{Await, BarKeys, Grab, Purpose};
use crate::ui::app::screens::Screen;
use crate::ui::app::{Reader, TextMode};
use crate::{
    features::circle::screens::overlay,
    ui::{ai_chat, song, translate},
};

pub(crate) struct Opening {
    /// The frozen frame while the overlay is open; `None` = hidden, frame freed.
    /// Its RGBA bytes are also what actions crop (shared, never copied whole).
    pub(crate) frame: Option<image::Handle>,
    /// Physical px per logical px of `frame`.
    pub(crate) scale: f32,
    /// The overlay's logical size (the monitor's).
    pub(crate) screen: Size,
    /// The stroke being drawn or the selection's (image px; see `overlay::View`).
    pub(crate) stroke: Vec<Point>,
    pub(crate) drawing: bool,
    pub(crate) selection: Option<overlay::Selection>,
    /// Selections kept with Shift before the current one, with their strokes.
    pub(crate) others: Vec<(overlay::Selection, Vec<Point>)>,
    /// The stroke being drawn adds to `others` (Shift).
    pub(crate) adding: bool,
    /// Alt is held: a press inside the box moves it, even over text.
    pub(crate) alt: bool,
    /// Resizing or moving the selection.
    pub(crate) grab: Option<Grab>,
    /// The zoomed / panned part of the frame on screen; a pan's start.
    pub(crate) view: overlay::View,
    pub(crate) pan: Option<(Point, overlay::View)>,
    /// The wheel zoom easing `view` to its target (`zoom.rs`).
    pub(crate) zoom: Option<crate::features::circle::app::zoom::ZoomMotion>,
    /// The tapped word(s) the selection is (the tap's number: `App::taps`).
    pub(crate) word: Option<word::Picked>,
    /// Main mode: the frame's text lines (image px), read on demand band by
    /// band (#123): never at open, the band the user touches first.
    pub(crate) screen_text: word::Screen,
    /// The band OCR reads now (one at a time: the helper serves one request
    /// at a time anyway).
    pub(crate) reading: Option<Rectangle>,
    /// The second reader (other scripts than Windows OCR's) was started,
    /// and the band it reads now.
    pub(crate) other_read: bool,
    pub(crate) other_reading: Option<Rectangle>,
    /// Windows OCR's lines the second reader leaves alone (image px x scale:
    /// frame px), once judged (`ocr::system::read_well`).
    pub(crate) other_known: Option<std::sync::Arc<Vec<[f32; 4]>>>,
    /// The whole frame went to the helper for the second reader
    /// (`ocr::read_screen`): later parts send only its key.
    pub(crate) frame_sent: bool,
    /// The reader offered for the script found (#169, sheet b7): the card
    /// asks for it, once per opening.
    pub(crate) script_reader: Option<&'static crate::features::text_reading::models::Recognizer>,
    pub(crate) script_offered: bool,
    /// Lines the Latin reader read as noise with no other reader on disk.
    pub(crate) noise: usize,
    /// Copy text's spinner while the screen's text is read: since when, and
    /// its turn (`kit::spinner`).
    pub(crate) text_spin: Option<(Instant, f32)>,
    /// A tap or Shift+click waiting for the band under it.
    pub(crate) awaits: Option<(Point, Await)>,
    /// Ctrl+A waits for the whole screen to be read.
    pub(crate) want_all: bool,
    /// A drag let go before the band under its press was read, and the
    /// overlay events after it: replayed once it is.
    pub(crate) held: Option<Vec<overlay::Event>>,
    /// Text was selected in this opening (or, off Windows, its helper
    /// started: `Warmed`): the rest of the screen is read in the background
    /// (gently), nearest the pointer first.
    pub(crate) text_used: bool,
    /// Where the pointer was last seen (image px).
    pub(crate) pointer: Point,
    /// The text reader's models are here (checked at open).
    pub(crate) ocr_ready: bool,
    /// How that read is going (the note over the bar).
    pub(crate) reader: Reader,
    /// Copy text asked for the OCR models: copy once they are downloaded.
    pub(crate) copy_after: bool,
    /// Who reads this opening's text (`ocr::route`), once its first read
    /// started: Windows OCR (the whole screen at once) or the helper (bands).
    pub(crate) text_by: Option<ocr::Source>,
    /// A press that began on text: its stroke so far (image px). It selects
    /// words unless it turns into a loop (`overlay::winds`).
    pub(crate) text_press: Option<Vec<Point>>,
    /// The opening animation (`overlay::APPEAR`): the latest frame's time and
    /// how far it is (0..1, linear); `None` until the first frame (t = 0).
    pub(crate) appear: Option<(Instant, f32)>,
    /// The bar's highlighted pill: Search in browser at every opening, then the
    /// one last pressed (the new design's active pill).
    pub(crate) pill: BarButton,
    /// The shortcuts the bottom bar's tooltips show (read at open).
    pub(crate) bar_keys: BarKeys,
    /// Which unfinished features are on (read at open; Settings changes apply to the next one).
    pub(crate) labs: Labs,
    /// The last press: when, where (image px), and how many in a row.
    pub(crate) clicks: Option<(Instant, Point, u8)>,
    /// The ask bar's text.
    pub(crate) ask: String,
    /// Include screen (the AI field's switch and the chat's, one state): a
    /// question goes with the selection or the screen; off, the question only.
    pub(crate) include: bool,
    /// Shown but still fully clear (`show`): fades in once its first full
    /// frame is drawn (`first_draw` taken), or after `REVEAL_WAIT` anyway.
    pub(crate) reveal: Option<Instant>,
    /// The open AI chat and its number.
    pub(crate) ai_chat: Option<(u32, ai_chat::AiChat)>,
    pub(crate) purpose: Purpose,
    /// The annotation editor on the selection; kept when going back to the bar.
    pub(crate) edit: Option<SnipToolbar>,
    /// The shown monitor's origin (logical px): the key of `App::regions`.
    pub(crate) origin: Point,
    /// A short status over the frame (copy text).
    pub(crate) toast: Option<Said>,
    /// Wayland: the Copy / Save pill over the frame (its text, done or not,
    /// where), held a moment before the overlay closes (`App::toast_for`).
    pub(crate) result: Option<(String, bool, Point)>,
    /// What the current selection holds (QR code, protected capture); `None` until scanned.
    pub(crate) scan: Option<qr::Scan>,
    /// Translate in place on this overlay, and its number.
    pub(crate) translate: Option<(u32, translate::Session)>,
    /// One warm OCR / translation helper while the overlay is open (tap a
    /// word, Text, Translate), started by the first of them.
    /// (shared with the screen's text reads: one that starts after the
    /// overlay closed reads nothing, `read_band`)
    pub(crate) helper: Option<Arc<ocr::Hold>>,
    /// How selections behave (`[selection]`), read when the overlay opens.
    pub(crate) behaviour: SelectionConfig,
    /// A snapped selection's drawn stroke (image px) morphing into its box.
    pub(crate) morph: Option<(Vec<Point>, Instant)>,
    pub(crate) text_mode: Option<TextMode>,
    /// The bottom bar turning into the editing toolbar in its place: 0 = the
    /// bar, 1 = the toolbar (linear), and while it turns: since when, from
    /// and to where (`None`: at rest).
    pub(crate) swap: f32,
    pub(crate) swap_at: Option<(Instant, f32, f32)>,
    /// The editor just went back to the bar (until the next frame): an
    /// Escape now is the key that did it.
    pub(crate) esc_back: bool,
    /// Where Tab put the focus in the bottom bar (the field's own focus is the
    /// text input's); none = the overlay has it, so the letters are shortcuts.
    pub(crate) bar_focus: Option<bar::Stop>,
    /// The bar's spot (`config::BarConfig`); while its handle is held, the
    /// pointer's last position and where the bar follows it (top-left);
    /// where it is drawn now (gliding to its spot); how much of the screen's
    /// bottom the taskbar takes (logical px).
    pub(crate) bar_anchor: BarAnchor,
    pub(crate) bar_drag: Option<(Option<Point>, Point)>,
    /// The other screen (index in `screens`) the bar was dropped on; `None`:
    /// the starting one.
    pub(crate) bar_on: Option<usize>,
    /// Snip's toolbar dropped here by the user: it stays (no docking to the
    /// area) until a new area is picked.
    pub(crate) bar_left: Option<Point>,
    /// Snip's toolbar picked up: where it was left before (Esc puts it back).
    pub(crate) bar_was: Option<Option<Point>>,
    pub(crate) bar_now: Option<Point>,
    /// Its glide to a new spot: since when, from and to where.
    pub(crate) bar_glide: Option<(Instant, Point, Point)>,
    /// While held: the last frame it was eased toward the pointer on.
    pub(crate) bar_eased: Option<Instant>,
    pub(crate) work_inset: f32,
    /// The card's grip or corner is held, and the pointer's last position.
    pub(crate) chat_drag: Option<(card::Drag, Option<Point>)>,
    /// The card held is the AI chat (it docks when let go).
    pub(crate) chat_docks: bool,
    /// Where the pinned chat's own window was when this frame was captured
    /// (logical px): it is kept out of captures, so the frame may be black there.
    pub(crate) chat_hole: Option<Rectangle>,
    /// The pinned chat's own window where this overlay found it (logical px):
    /// it stays put (#170 C4) until it is picked up (`App::adopt_chat`).
    pub(crate) chat_fixed: Option<Rectangle>,
    /// The chat was pinned before this overlay opened: a question sent into
    /// it closes the overlay (#170 C4c).
    pub(crate) chat_came_pinned: bool,
    /// The result card opening (`motion::WIDGET`): which panel, its motion,
    /// and how open it was at the latest frame (it grows from `OPEN_SCALE`).
    pub(crate) chat_open: Option<(u32, motion::Shown, f32)>,
    /// The pointer is on the card's resize edge (its grip shows).
    pub(crate) chat_hover: bool,
    /// The AI chat card held away from its dock (logical px), and its glide
    /// back to a dock once let go: since when, from where, how far it is.
    pub(crate) chat_off: Vector,
    /// The other screen (index in `screens`) the AI chat is docked on;
    /// `None`: the starting one.
    pub(crate) chat_on: Option<usize>,
    pub(crate) chat_glide: Option<(Instant, Rectangle, f32)>,
    /// Song search on this overlay, and its number.
    pub(crate) song: Option<(u32, song::Session)>,
    /// The capture this opening shows (its other monitors' frames carry it).
    pub(crate) capture: u32,
    /// Circle to Search: every other monitor, each in its own window, as its
    /// frame lands (`app::screens`); freed with the opening.
    pub(crate) screens: Vec<Screen>,
    /// Their frames that landed during the opening animation (shown after it).
    pub(crate) waiting: Vec<(crate::features::snip::screens::snip::Place, image::Handle)>,
    /// Where the edge glow is: the screen the mouse is on (`screens::Glow`).
    pub(crate) glow: crate::ui::app::screens::Glow,
    /// Main mode: the desktop's windows at the freeze (`Shot::windows`), for
    /// the bar's Snip (a late list drifts from the frozen frames).
    pub(crate) windows: Vec<crate::features::snip::screens::snip::Pick>,
    /// Snip's More menu: the area's text read, the translation on it.
    pub(crate) more: crate::features::snip::app::snip_more::More,
}

impl Default for Opening {
    /// Nothing captured: what a closed overlay holds.
    fn default() -> Self {
        Opening {
            frame: None,
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
            zoom: None,
            word: None,
            screen_text: word::Screen::default(),
            reading: None,
            other_read: false,
            other_reading: None,
            other_known: None,
            frame_sent: false,
            script_reader: None,
            script_offered: false,
            noise: 0,
            text_spin: None,
            awaits: None,
            want_all: false,
            held: None,
            text_used: false,
            pointer: Point::ORIGIN,
            ocr_ready: false,
            reader: Reader::Idle,
            more: Default::default(),
            copy_after: false,
            text_by: None,
            text_press: None,
            appear: None,
            pill: BarButton::Search,
            bar_keys: BarKeys::default(),
            labs: Labs::default(),
            clicks: None,
            ask: String::new(),
            include: true,
            reveal: None,
            ai_chat: None,
            purpose: Purpose::Search,
            edit: None,
            origin: Point::ORIGIN,
            toast: None,
            result: None,
            scan: None,
            translate: None,
            helper: None,
            // As before the setting existed; `open` reads the config.
            behaviour: SelectionConfig {
                style: SelectStyle::Shape,
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
            bar_on: None,
            bar_left: None,
            bar_was: None,
            bar_now: None,
            bar_eased: None,
            bar_glide: None,
            work_inset: 0.0,
            chat_drag: None,
            chat_docks: false,
            chat_hole: None,
            chat_fixed: None,
            chat_came_pinned: false,
            chat_open: None,
            chat_hover: false,
            chat_off: Vector::ZERO,
            chat_on: None,
            chat_glide: None,
            song: None,
            capture: 0,
            screens: Vec::new(),
            waiting: Vec::new(),
            glow: Default::default(),
            windows: Vec::new(),
        }
    }
}

/// `App::o`: the open overlay's `Opening`, `None` while closed. `update` drops
/// every message that needs it while closed (`works_closed`), so handlers use
/// it straight (`self.o.frame`); read while closed it is an empty `Opening`.
#[derive(Default)]
pub(crate) struct Open(Option<Opening>);

thread_local! {
    /// What a closed overlay reads (nothing captured). Leaked once per thread
    /// (it is small and never changes) so `deref` can lend it.
    static CLOSED: &'static Opening = Box::leak(Box::default());
}

impl Open {
    pub(crate) fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// A new opening (`App::open`).
    pub(crate) fn start(&mut self, opening: Opening) {
        self.0 = Some(opening);
    }

    /// Closing: the opening goes (`App::close`).
    pub(crate) fn take(&mut self) -> Option<Opening> {
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
