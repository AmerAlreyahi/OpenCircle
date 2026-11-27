//! OpenCircle Studio (v0.4.0, STUDIO_V2_SPEC): a normal window for longer work on a
//! picture. The SAME editor as the Snip widget (`annotate::Annotator`), shown
//! another way: every tool on a rail, the tool's options always visible in a bar
//! over the canvas, the canvas with zoom, Save / Copy in the status bar. Side
//! panels: the library (Recent captures) and the picture panel, each hidden by
//! its toggle or Ctrl+[ / Ctrl+] (Ctrl+\ both), remembered in `[snip.studio]`.
//! The library's Steps tab (v0.4.2) shows the Steps strip's own steps (one
//! store, the host's): click = open on the canvas, drag = reorder, x = delete,
//! Copy all, Export (Markdown / HTML / images). Export picture: PNG / JPG /
//! WebP / PDF at a size (`features::snip::export`).
//! One Studio at a time: the host (`app::windows`) brings it forward and
//! `open`s the next picture in it; unsaved drawings ask first.
mod corners;
mod cut;
mod edit_text;
pub(crate) mod editing;
pub(crate) mod keys;
mod menu_bar;
pub(crate) mod options_bar;
mod popovers;
pub(crate) use popovers::tool_of;
mod pills;
pub mod preferences;
pub mod recovery;
mod sheets;
mod sidebar;
mod start;
mod text_select;
mod view;

use std::path::PathBuf;

use iced::widget::image;
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Point, Rectangle, Size, Vector};

use super::steps::{self, Released, Reorder};
use crate::features::snip::annotate::{self, Annotator, Image, Output};
use crate::features::snip::export;
use crate::features::snip::snip_more::{self as more, Row};
pub use crate::features::text_reading::model_download::Fetching;
use crate::features::translate::{self, Line};
use crate::shared::config::StudioConfig;
use crate::shared::kit::header::Caption;
use crate::shared::picture::Crop;

pub use cut::{Done as CutDone, Phase as CutPhase};
pub use menu_bar::{Menu, Nav, Sub};
pub use sheets::Restorable;
pub use text_select::{Work as TextWork, lines_of as text_lines};
#[cfg(test)]
pub use view::GLYPHS;
pub use view::view;

/// The window's opening and smallest size (logical px; spec §2).
pub const SIZE: Size = Size::new(1280.0, 800.0);
pub const MIN: Size = Size::new(960.0, 600.0);

// The regions around the canvas (logical px): the stage is what they leave.
const HEADER: f32 = 41.0; // 40 + the hairline
const RAIL: f32 = 60.0;
const OPTIONS: f32 = 48.0;
// === Studio round 6 ===
/// A hairline's width (the gap that replaced a divider).
const HAIR: f32 = 1.0;
/// The floating pills: their gap to the stage's edge, their height, and the stage's bottom band
/// they cover (a fitted, centred picture stays above it; `stage()` is the room above the band).
const PILL_EDGE: f32 = 12.0;
const PILL_ROOM: f32 = 56.0;
const BAND: f32 = PILL_EDGE + PILL_ROOM;
/// The floating Steps card's margins (design r6): top and window edge, and the room left under it
/// so the floating pills stay visible.
pub(super) const SLIDER_TOP: f32 = 12.0;
pub(super) const SLIDER_EDGE: f32 = 8.0;
pub(super) const SLIDER_BOTTOM: f32 = 74.0;
/// The pills' Save / Copy buttons (build.py `.tr .tb2`): 32 px tall, 12 px each side, pill corners.
pub(super) const PILL_BTN_H: f32 = 32.0;
pub(super) const PILL_BTN_SIDE: f32 = 12.0;
pub(super) const PILL_BTN_RADIUS: f32 = 16.0;
/// The gap between the rail's tiles.
const TILE_GAP: f32 = 8.0;
// === end Studio round 6 ===
/// The Steps sidebar's width.
const SIDEBAR: f32 = 320.0;
/// Room kept around a fitted picture.
const STAGE_PAD: f32 = 24.0;
/// One zoom step (Ctrl+= / Ctrl+-) and the zoom's range, as a share of 100 %.
const ZOOM_STEP: f32 = 1.25;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.05..=8.0;
/// The canvas's scroll area (`operation::scroll_to`).
pub const CANVAS: &str = "studio-canvas";
/// Edit text's inline field.
pub const FIELD: &str = "studio-edit-field";

/// Files of one drop arrive one by one: the drop settles after this.
pub const DROP_SETTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// The Steps grid: four to a row, the cell and the gap round it.
/// The sidebar's steps: four to a row (3 x 10 + 4 x 65 = 290 px of its 292), room for the number badge.
const STEP_COLS: usize = 4;
const STEP_CELL: Size = Size::new(65.0, 44.0);
const STEP_GAP: f32 = 10.0;

/// What the Studio opens.
pub enum Picture {
    /// A file, the clipboard or a recent capture.
    Crop(Crop),
    /// The Snip widget's editor, its drawings still editable and its undo kept.
    Editor(Box<Annotator>),
}

/// Edit text's read counter, shared like the epoch: each read is its own.
static NEXT_READ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The picture counter (`Studio::epoch`), shared by every Studio of the run.
static NEXT_EPOCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// What waits on the unsaved-changes question.
enum After {
    Close,
    /// Quit (the tray, `opencircle quit`): the app goes after it.
    Quit,
    Open(Picture),
}

/// A picture of at least this many pixels (1 MP) is prepared on a thread.
const BIG_PICTURE: u32 = 1_000_000;

/// What the thread made of a big picture.
struct Prepared {
    image: Image,
    base: image::Handle,
    alpha: bool,
}

type OpenSlot = std::sync::Arc<std::sync::Mutex<Option<Prepared>>>;

/// The Steps sidebar's pages: the list, and (drilled into) the export choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Steps,
    Export,
}

/// The sidebar's page change: when it began and which way (`true`: into Export).
#[derive(Debug, Clone, Copy)]
pub(super) struct Slide {
    pub start: std::time::Instant,
    pub into: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The Steps sidebar goes to a page (a slide), and the frame clock steps it.
    SidebarPage(Page),
    Frame(std::time::Instant),
    Editor(annotate::Message),
    Caption(Caption),
    /// The Steps panel's toggle (its handle, header arrow, View menu, Ctrl+B).
    Library,
    /// The Steps panel's pin: docked (the canvas makes room) or sliding over it.
    PinSteps,
    /// A press outside the open Steps panel.
    StepsAway,
    /// A popover opens (`Some`) or closes (v3.1).
    Pop(Option<popovers::Pop>),
    /// Steps: the file dialog (several pictures), the files it gave or that were
    /// dropped together, Paste as step.
    PickFiles,
    AddFiles(Vec<PathBuf>),
    PasteStep,
    /// The middle button pans (drag start / end).
    Middle(bool),
    /// Ctrl+= / Ctrl+- / Ctrl+0 scale the Studio's own UI (steps; 0 = reset).
    UiScale(i8),
    ZoomIn,
    ZoomOut,
    /// The wheel over the stage (it zooms; Ctrl+wheel always does): `notches` steps (up = in) round the pointer
    /// `at` (in the stage).
    Wheel {
        at: Point,
        notches: f32,
    },
    /// The stage's scroll offset (kept to zoom round the pointer) and whether
    /// Ctrl is down (the stage then zooms instead of scrolling).
    Scrolled(Vector),
    Ctrl(bool),
    /// Ctrl+0: fit the stage; Ctrl+1: 100 %.
    Fit,
    Actual,
    /// Ctrl+O, Ctrl+V (a picture on the clipboard).
    Open,
    Paste,
    /// A file is dragged over the window / dropped on it.
    Hovered(bool),
    Dropped(PathBuf),
    /// The drop's files have all arrived (`n` of them: a later one voids this).
    DropSettle(usize),
    /// The pointer enters / leaves the Steps sidebar.
    OverSidebar(bool),
    /// The Open dialog's file.
    Picked(PathBuf),
    /// Studio preferences (Ctrl+, / File > Preferences): open, the dialog's own messages,
    /// the folder picker's answer.
    OpenPrefs,
    Prefs(preferences::Msg),
    OpenRecent(PathBuf),
    /// The start screen's "Show recordings": the folder of unsaved recordings in the file manager.
    ShowRecordings,
    Recent(Vec<(PathBuf, image::Handle)>, Vec<String>),
    /// The window's px per logical px, and its size.
    Scale(f32),
    Resized(Size),
    /// The unsaved-changes question's buttons.
    DontSave,
    Cancel,
    SaveFirst,
    /// A picture-panel row (Copy text, Translate, Ask AI).
    Act(Row),
    /// The download card's buttons (Download / Try again, Not now / Cancel,
    /// Use an AI provider instead).
    Download,
    DismissCard,
    Provider,
    /// A failed download card's Show / Hide details.
    CardDetails,
    /// The window has / lost the keyboard (the quick-action keys act on it).
    Focus(bool),
    /// The panel's Edit text row; a line picked on the picture; its new text;
    /// Done (Enter). Cancel (Esc) leaves the picture as it was.
    EditText,
    PickLine(usize),
    Draft(String),
    EditDone,
    /// The panel's Cut out row; the options bar's Before (`true`) / After.
    CutOut,
    CutView(bool),
    /// The Cut out panel's close: the cut stays, the compare view goes.
    CutClose,
    /// A menu opens (`Some`) or closes; a row picked; the keyboard on the bar.
    Menu(Option<Menu>),
    MenuPick(Menu, usize),
    MenuKey(Nav),
    /// The shortcuts sheet.
    Shortcuts(bool),
    /// The shortcuts sheet's Edit shortcuts… (Settings); the restore sheet's Later.
    EditShortcuts,
    Later,
    /// Selectable text: start / stop, the overlay's pointer, the menu (`text_select`).
    Text(text_select::Ev),
    /// Right-click on the stage; the menu's row; the pointer over the stage.
    ContextOpen,
    ContextPick(usize),
    Pointer(Point),
    /// Properties: the ratio lock; a typed number that isn't one.
    Lock(bool),
    /// Space held: the stage pans on a drag; the drag starts / ends.
    Space(bool),
    /// Ctrl and Shift as the keyboard holds them; the recovery timer.
    Mods(bool, bool),
    Tick,
    /// Alt down / up (the menu bar opens on the release, Windows' way).
    AltDown,
    AltUp,
    /// Another key went down: Alt's release is no longer a menu key.
    AltBreak,
    /// Properties' box fields: a typed value (axis, text), Enter.
    BoundsEdit(usize, String),
    BoundsCommit,
    PanStart,
    PanEnd,
    /// A second menu's row; View > Snap / Pixel grid; Recent > Clear.
    SubPick(Sub, usize),
    Snap,
    Grid,
    ClearRecent,
    /// Board 20: bring the kept copy back / throw it away.
    Restore,
    DiscardCopy,
    /// File > New snip.
    NewSnip,
    /// The Steps list: a press on a step, the pointer over the list (list px,
    /// with the steps' count), the press ends, the pointer on / off a step.
    StepPress(usize),
    StepMove(Point, usize),
    StepRelease,
    StepHover(usize, bool),
    StepDelete(usize),
    CopyAllSteps,
    ExportSteps(export::Steps),
    /// The Export sheet's Walkthrough tab (the Steps menu's first row), its
    /// choices, and the encoder's progress (done, total) and end.
    ExportWalkthrough,
    /// The sheet's tab: the picture's (`false`) or the walkthrough's.
    ExportTab(bool),
    WalkFormat(export::Movie),
    WalkProgress(usize, usize),
    /// The encoder ended (`true`: the file is written: the sheet closes).
    WalkEnded(bool),
    WalkEdits(bool),
    WalkSize(u32),
    WalkTime(u8),
    WalkNumbered(bool),
    WalkLoop(bool),
    /// The walkthrough's one transition between steps, and its length (ms).
    WalkTransition(crate::features::snip::transition::Transition),
    WalkTransitionMs(u32),
    WalkGo,
    /// The sheet's Cancel: stops the encoder when one runs, else closes.
    WalkCancel,
    /// The Export sheet: open (Picture tab), its choices, Export…, Cancel.
    Export,
    ExportFormat(export::Format),
    ExportQuality(u8),
    ExportSize(u32),
    ExportGo,
}

/// Where a walkthrough's frames come from.
pub enum Frames {
    /// The Steps strip's steps.
    Steps,
    /// The picture's edits: a copy of the document, replayed by the host's
    /// worker (one picture, not a copy of every frame).
    Edits(Box<annotate::Document>),
}

/// What the host does after a message.
pub enum Outcome {
    None,
    /// The window goes (asked first when there were unsaved drawings).
    Close,
    /// Quit went through the question: the app may go.
    Quit,
    Caption(Caption),
    /// The open-file dialog, then `Msg::Dropped` with the file.
    PickFile,
    /// The preferences dialog changed something: write it (`Studio::apply_prefs`).
    ApplyPrefs,
    /// Open Settings > General (the Save folder).
    PrefsOpenGeneral,
    /// The open-file dialog for several pictures, then `Msg::AddFiles`.
    PickFiles,
    /// Read these files and add each as a step.
    AddPictures(Vec<PathBuf>),
    /// Several files dropped on an empty Studio: the first opens AND all are steps.
    OpenAndAdd(Vec<PathBuf>),
    /// One file dropped on an open picture: placed on it as a movable picture.
    Place(PathBuf),
    /// A drop's files are still arriving: settle after `DROP_SETTLE`.
    SettleDrop(usize),
    /// The clipboard's picture becomes a step.
    PasteStep,

    /// Read this file (a dropped or recent one) and `open` it.
    Read(PathBuf),
    /// Open this folder in the file manager.
    ShowFolder,
    /// Ctrl+V: a picture on the clipboard opens; else the editor pastes its text
    /// at that point (`paste_text`).
    Paste(Option<Point>),
    Copy(Crop),
    /// Save; on success the host calls `saved` (and, after the question, `proceed`).
    Save(Crop),
    SaveThen(Crop),
    Pin(Crop),
    /// The panels changed: remember them.
    /// (what it was, what it is now): the host writes only the keys that differ.
    Panels(StudioConfig, StudioConfig),
    /// Scroll the stage there (a zoom round the pointer).
    ScrollTo(AbsoluteOffset),
    /// A picture action: the host (the App) runs main mode's own work on the
    /// pictures `read_picture` / `annotated` / `translate_frame` give.
    Act(Row),
    /// The card's Download: the host fetches what the row needs.
    Download,
    /// The card was dismissed: the host drops what waited on it.
    Dismissed,
    /// The card's AI link: dismissed, and Settings opens.
    Provider,
    /// Edit text: the host reads the picture's lines (`read_picture`) and
    /// answers with `set_lines`.
    ReadLines,
    /// Selectable text: the host reads the picture's words once, or runs a menu row on them.
    Text(text_select::Work),
    /// The inline field takes the keyboard (`FIELD`).
    FocusField,
    /// A pill with this (an edit that could not be made).
    Say(String),
    /// Cut out: the host cuts `cut_source`'s picture and answers `set_cut`.
    CutOut,
    /// For the Steps strip (delete, reorder, Copy all): the steps are its.
    Steps(steps::Msg),
    /// Open the `i`th step on the canvas.
    OpenStep(usize),
    /// Export the steps (the host asks for a folder).
    ExportSteps(export::Steps),
    /// Export a walkthrough (the host asks where, then draws the frames and
    /// encodes them off the UI thread).
    ExportWalkthrough(export::Walkthrough, Frames),
    /// Stop the encoder that runs.
    CancelWalkthrough,
    /// Export the picture so (the host asks where).
    ExportPicture(Crop, export::Picture),
    /// Add to steps: the picture as drawn joins the Steps strip.
    AddStep(Crop),
    /// The picture to add as a step, flattened by the host on a thread.
    AddStepDoc(Box<annotate::Document>),
    /// Write the unsaved copy (the host encodes and writes it off the UI thread).
    KeepCopy(Box<KeepCopy>),
    /// File > New snip: the Snip capture starts.
    NewSnip,
    /// Settings > Studio changed from the View menu.
    Prefs(crate::shared::config::StudioPrefs),
    /// Recent > Clear.
    ClearRecent,
}

/// A Steps panel slide's length, the way it goes.
fn steps_slide_len(opening: bool) -> std::time::Duration {
    match opening {
        true => crate::shared::kit::motion::PANEL_OPEN,
        false => crate::shared::kit::motion::PANEL_CLOSE,
    }
}

pub struct Studio {
    pub(super) editor: Option<Annotator>,
    scale: f32,
    window: Size,
    pub(super) panels: StudioConfig,
    /// The history's state (`Document::state`) when last saved: other = unsaved;
    /// `None` = never saved with what it holds.
    saved: Option<u64>,
    asking: Option<After>,
    pub(super) recent: Vec<(PathBuf, image::Handle)>,
    /// Recent holds unsaved recordings: the start screen's "Show recordings" row.
    pub(super) recordings: bool,
    pub(super) hovered: bool,
    /// The stage's scroll offset, and Ctrl held (see `Msg::Scrolled`).
    offset: Vector,
    /// Scroll offset queued by a stage-size change (panel pin/unpin/toggle, resize, Fit)
    /// for the host to apply to `CANVAS` alongside any other `Outcome`.
    pending_scroll: Option<AbsoluteOffset>,
    pub(super) ctrl: bool,
    /// The picture as it was before Translate (Show original), and the
    /// editor's `own_image` before it.
    original: Option<(Vec<u8>, bool)>,
    /// Counts the pictures opened: a translation that answers for an older one
    /// is dropped.
    epoch: u32,
    focused: bool,
    /// Ask AI opens the browser (no AI key): the host says (`set_browser`).
    browser: bool,
    /// The row that is working (its icon spins), and the download card up.
    busy: Option<Row>,
    card: Option<Fetching>,
    /// Edit text is on (S8).
    edit: Option<edit_text::Session>,
    /// Cut out (S9): the work's phase, the compare view, and whether the
    /// picture has see-through pixels (it sits on a checkerboard then).
    cutting: cut::Phase,
    cut: Option<cut::Cut>,
    transparent: bool,
    /// The open menu and its keyboard row; the Library's folded sections
    /// (Recent, Objects, Steps); the narrow Library overlay; the shortcuts sheet.
    pub(super) menu: Option<(Menu, usize)>,
    pub(super) popover: Option<popovers::Pop>,
    /// Where each popover's opener was last drawn.
    pub(super) anchors: popovers::Anchors,
    /// The picture's smaller copies (halves) for the stage, built when a big picture is shown
    /// small: the base's handle id they were made from, and (width, pixels) of each.
    mips: Option<Mips>,
    /// A middle-button pan is on.
    middle: bool,
    /// The Steps sidebar's page and its slide (the frame clock runs only while it moves).
    pub(super) page: Page,
    /// Tests: draw a rising menu where it lands (goldens are things at rest); the rise test turns it off.
    pub(super) at_rest: bool,
    /// When a menu, popover or dialog began to rise (the frame clock runs until it has).
    pub(super) rise_from: Option<std::time::Instant>,
    /// Steps being made on a thread (flatten, PNG, thumbnail): the sidebar shows a place for each.
    pub(super) steps_pending: usize,
    /// The sidebar was closed at opening for want of steps: only the first step opens it.
    closed_for_empty: bool,
    /// A big picture is being prepared on a thread: when it began, where the thread leaves it.
    opening: Option<(std::time::Instant, OpenSlot)>,
    pub(super) slide: Option<Slide>,
    /// The Steps panel sliding over the canvas: when it began and whether it opens (the frame
    /// clock runs only while it moves; a pinned panel never slides).
    /// (a slide that is reversed starts from where it was: the third is its start position, 0..1)
    steps_slide: Option<(std::time::Instant, bool, f32)>,
    pub(super) now: std::time::Instant,
    /// The Studio's UI scale, percent (Ctrl+= / Ctrl+- / Ctrl+0).
    pub(super) ui_zoom: u16,
    /// When the last file was dropped (files dropped together are steps).
    drops: Vec<PathBuf>,
    /// "Reset tool styles" was pressed: the open editor takes the defaults when written.
    reset_styles: bool,
    /// The preferences dialog, over the window while open.
    pub(super) prefs: Option<preferences::Dialog>,
    /// The pointer is over the Steps sidebar (a drop there is steps).
    over_sidebar: bool,
    pub(super) shortcuts: bool,
    /// The right-click menu, and the pointer's last place on the stage.
    pub(super) context: Option<Context>,
    pub(super) pointer: PointerCell,
    /// Selectable text (R-16): the mode, the words read, the selection, its menu.
    pub(super) text: text_select::State,
    /// Alt went down with nothing since: its release opens the menu bar.
    alt_clean: bool,
    /// A restore that waits for the unsaved question (Cancel gives the sheet back).
    restoring: Option<Restorable>,
    /// Properties' X / Y / W / H while typed (committed on Enter or the next action).
    pub(super) bounds_draft: Option<[String; 4]>,
    /// A picture was asked for (a file from the command line or "Open with", the
    /// Open button, a drop, a recent file, a paste) or opened: Reopen last stands down.
    asked: bool,
    pub(super) lock: bool,
    /// Space is held (the stage pans), the pointer where a pan last was, the
    /// open second menu, the document's name and the name of the next picture.
    pub(super) space: bool,
    pub(super) shift: bool,
    quiet: recovery::Debounce,
    pan: Option<Point>,
    pub(super) sub: Option<Sub>,
    /// Snap and the pixel grid (Settings > Studio; the View menu flips them here).
    pub(super) snap: bool,
    pub(super) grid: bool,
    name: Option<String>,
    next_name: Option<String>,
    /// The window's title: "<name> - OpenCircle Studio".
    pub(super) title: String,
    /// The Recent rows' second line (size, when), made when the list arrives.
    pub(super) recent_info: Vec<String>,
    /// A copy kept after a crash, waiting for Restore / Discard (board 20).
    pub(super) restorable: Option<Restorable>,
    /// Later: the restore sheet is put away for now (the copy stays).
    pub(super) later: bool,
    /// The copy could not be restored (the sheet says so; the copy is kept).
    pub(super) restore_failed: bool,
    /// When the unsaved copy was last written (`recovery`).
    /// The Steps list: a press or drag, and the step under the pointer.
    pub(super) pick: Reorder,
    pub(super) step_hover: Option<usize>,
    /// The export bar's choices, and whether it shows.
    pub(super) export: export::Picture,
    pub(super) exporting: bool,
    /// The walkthrough tab's choices.
    pub(super) walk: Walk,
    /// The sheet shows the Walkthrough tab.
    pub(super) walk_tab: bool,
}

/// The walkthrough dialog's choices (STUDIO_V2_SPEC "Export dialog").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Walk {
    /// From the picture's edits, not the Steps.
    pub edits: bool,
    /// Frame time in tenths of a second.
    pub tenths: u8,
    pub how: export::Walkthrough,
    /// The encoder runs: frames done and all (0 of 0 while the file is asked for).
    pub busy: Option<(usize, usize)>,
}

impl Walk {
    fn how(self) -> export::Walkthrough {
        export::Walkthrough {
            frame_ms: u32::from(self.tenths) * 100,
            ..self.how
        }
    }
}

impl Studio {
    /// The empty Studio with the user's panels.
    pub fn new(mut panels: StudioConfig) -> Self {
        // Settings > Studio: the Steps panel on opening (else as it was left: closed at first)
        panels.library |= crate::shared::config::current().studio.steps_on_start;
        Studio {
            editor: None,
            scale: 1.0,
            window: SIZE,
            panels,
            saved: Some(0),
            asking: None,
            recent: Vec::new(),
            recordings: false,
            hovered: false,
            offset: Vector::ZERO,
            pending_scroll: None,
            ctrl: false,
            original: None,
            epoch: 0,
            focused: false,
            browser: false,
            busy: None,
            card: None,
            edit: None,
            cutting: cut::Phase::Idle,
            cut: None,
            transparent: false,
            menu: None,
            popover: None,
            anchors: popovers::Anchors::default(),
            middle: false,
            ui_zoom: panels.ui_zoom,
            drops: Vec::new(),
            prefs: None,
            reset_styles: false,
            over_sidebar: false,
            shortcuts: false,
            context: None,
            pointer: PointerCell::default(),
            text: text_select::State::default(),
            alt_clean: false,
            restoring: None,
            bounds_draft: None,
            asked: false,
            lock: true,
            space: false,
            shift: false,
            quiet: recovery::Debounce::default(),
            pan: None,
            page: Page::Steps,
            opening: None,
            steps_pending: 0,
            closed_for_empty: false,
            rise_from: None,
            at_rest: cfg!(test),
            mips: None,
            slide: None,
            steps_slide: None,
            now: std::time::Instant::now(),
            sub: None,
            snap: crate::shared::config::current().studio.snap,
            grid: crate::shared::config::current().studio.pixel_grid,
            name: None,
            next_name: None,
            title: title_of(None),
            recent_info: Vec::new(),
            restorable: pending_copy(),
            later: false,
            restore_failed: false,
            pick: Reorder::default(),
            step_hover: None,
            export: export_defaults(),
            exporting: false,
            walk: Walk {
                edits: false,
                tenths: 20,
                how: export::Walkthrough {
                    movie: export::Movie::from_index(panels.walk_format),
                    transition: crate::features::snip::transition::Transition::from_index(
                        panels.walk_transition,
                    ),
                    transition_ms: panels.walk_ms,
                    ..export::Walkthrough::default()
                },
                busy: None,
            },
            walk_tab: false,
        }
    }

    /// Sets the initial window size before opening a picture so the opening fit uses it.
    pub fn set_window_size(&mut self, size: Size) {
        self.window = size;
    }

    /// Any scroll offset queued by a view-preserving update (panel pin/unpin, resize, Fit).
    pub fn take_scroll(&mut self) -> Option<AbsoluteOffset> {
        self.pending_scroll.take()
    }

    /// Opens `picture` (fitted); with unsaved drawings it asks first. A big picture is prepared
    /// on a thread (its base painted, its see-through pixels looked for: 130-220 ms at 8K) while
    /// the window shows a spinner: the UI thread only starts it (`poll_open` finishes it).
    pub fn open(&mut self, picture: Picture) {
        if self.asks() {
            self.asking = Some(After::Open(picture));
            return;
        }
        match picture {
            Picture::Crop(crop) if crop.width * crop.height >= BIG_PICTURE => {
                let slot: OpenSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
                let out = slot.clone();
                std::thread::spawn(move || {
                    let image = Image {
                        width: crop.width,
                        height: crop.height,
                        rgba: crop.rgba,
                    };
                    let (base, alpha) = annotate::base_of(&image);
                    if let Ok(mut out) = out.lock() {
                        *out = Some(Prepared { image, base, alpha });
                    }
                });
                self.now = std::time::Instant::now();
                self.opening = Some((self.now, slot));
            }
            Picture::Crop(crop) => {
                let editor = Annotator::new(
                    Image {
                        width: crop.width,
                        height: crop.height,
                        rgba: crop.rgba,
                    },
                    1.0,
                );
                self.finish_open(editor, None);
            }
            Picture::Editor(editor) => self.finish_open(*editor, None),
        }
    }

    /// The big picture's thread is done: open it (a frame tick calls this).
    fn poll_open(&mut self) {
        let Some((_, slot)) = &self.opening else {
            return;
        };
        let ready = slot.lock().ok().and_then(|mut s| s.take());
        if let Some(p) = ready {
            self.opening = None;
            let editor = Annotator::new_with_base(p.image, 1.0, p.base);
            self.finish_open(editor, Some(p.alpha));
        }
    }

    /// A big picture is being prepared (the window shows a spinner): when it began.
    pub(super) fn opening_since(&self) -> Option<std::time::Instant> {
        self.opening.as_ref().map(|(t, _)| *t)
    }

    /// Tests: the old way, all on the calling thread (what `open` was before the thread).
    #[cfg(test)]
    pub fn open_in_place(&mut self, crop: Crop) {
        let editor = Annotator::new(
            Image {
                width: crop.width,
                height: crop.height,
                rgba: crop.rgba,
            },
            1.0,
        );
        self.finish_open(editor, None);
    }

    /// Tests: `poll_open` once; how long it took when it opened the picture (ms).
    #[cfg(test)]
    pub fn poll_open_timed(&mut self) -> Option<f64> {
        let t = std::time::Instant::now();
        let was = self.opening.is_some();
        self.poll_open();
        (was && self.opening.is_none()).then(|| t.elapsed().as_secs_f64() * 1e3)
    }

    /// Tests: wait for the big picture's thread and open it.
    #[cfg(test)]
    pub fn settle_open(&mut self) {
        for _ in 0..2000 {
            if self.opening.is_none() {
                return;
            }
            self.poll_open();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Takes `editor` as the open picture.
    /// `alpha`: whether it has see-through pixels, when that is known already (its base is then
    /// painted too).
    fn finish_open(&mut self, mut editor: Annotator, alpha: Option<bool>) {
        // the widget's live area becomes a plain picture: the crop shows alone
        editor.movable = false;
        editor.studio = true;
        editor.own_image = false;
        (editor.scroll, editor.capturing, editor.held) = (false, false, false);
        (editor.steps, editor.updating, editor.focus, editor.menu) = (None, None, None, None);
        editor.edges.clear();
        // what came from Snip with no drawing is nothing to lose
        self.saved = (editor.doc.items.is_empty()).then(|| editor.doc.state());
        (self.original, self.busy, self.card, self.edit) = (None, None, None, None);
        (self.cutting, self.cut) = (cut::Phase::Idle, None);
        self.transparent = alpha.unwrap_or_else(|| cut::has_alpha(&editor.doc.image.rgba));
        // (one counter for every Studio: an answer for a closed one never fits a new one)
        self.epoch = NEXT_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let prefs = crate::shared::config::current().studio;
        self.prefs_into(&mut editor);
        self.name = self.next_name.take();
        self.title = title_of(self.name.as_deref());
        self.editor = Some(editor);
        match prefs.zoom_on_open {
            crate::shared::config::StudioZoom::Actual => {
                let z = 1.0 / self.scale;
                if let Some(e) = &mut self.editor {
                    e.zoom = z;
                }
                self.offset = Vector::ZERO;
                self.pending_scroll = Some(AbsoluteOffset { x: 0.0, y: 0.0 });
                self.sync_stage();
            }
            crate::shared::config::StudioZoom::Fit => self.fit_to_stage(),
        }
        self.asked = true;
        self.sync_blocked();
        if let Some(e) = &mut self.editor {
            if let Some(r) = self.restoring.take() {
                match e.restore_annotations(&r.notes) {
                    // the document is what it was: unsaved (it asks before it is lost), and a
                    // new copy is written in time; the old one goes only when it is saved or
                    // thrown away
                    true => {
                        self.saved = None;
                        self.restore_failed = false;
                        self.quiet.changed(std::time::Instant::now());
                    }
                    // the drawings can't be read: the copy is kept and the sheet says so
                    false => {
                        self.restorable = Some(r);
                        self.restore_failed = true;
                    }
                }
                e.refresh();
            } else if alpha.is_none() {
                e.refresh(); // (a prepared picture has its base already)
            }
        }
        self.sync_stage(); // (the repaint cleared the stage copy)
    }

    /// The picture Reopen last brings back (`on`: the setting): the newest recent
    /// file, but never when any picture was asked for or is open: a requested
    /// file always wins.
    fn reopen(&self, on: bool) -> Option<PathBuf> {
        let first = self.recent.first().map(|(p, _)| p.clone());
        first.filter(|_| on && self.editor.is_none() && !self.asked)
    }

    /// The next picture opened is this file's (its name shows in the title).
    pub fn next_name(&mut self, path: &std::path::Path) {
        self.asked = true; // (the request is here before its picture is)
        self.next_name = file_name(path);
    }

    /// Unsaved drawings: closing or opening another picture asks first.
    /// Settings > Studio > "Ask before closing with unsaved changes".
    fn asks(&self) -> bool {
        self.dirty() && crate::shared::config::current().studio.ask_unsaved
    }

    /// The sidebar's slide, 0..1 of the way (1 when none runs or motion is reduced).
    pub(super) fn slide_at(&self) -> f32 {
        match self.slide {
            Some(s) => crate::shared::kit::motion::progress(
                s.start,
                self.now,
                crate::shared::kit::motion::GLIDE,
                crate::shared::kit::motion::Curve::Standard,
            ),
            None => 1.0,
        }
    }

    fn slide_done(&self, s: Slide) -> bool {
        crate::shared::kit::motion::done(s.start, self.now, crate::shared::kit::motion::GLIDE)
            >= 1.0
    }

    /// The frame clock should run: a slide is moving.
    /// A step is being made (+1) or has arrived or failed (-1).
    pub fn step_pending(&mut self, delta: isize) {
        self.steps_pending = self.steps_pending.saturating_add_signed(delta);
        // the first step after an empty start opens the sidebar that was closed for want of
        // steps (F8); later steps leave it as the user has it (Ctrl+B, the setting)
        if delta > 0 && std::mem::take(&mut self.closed_for_empty) {
            let old = self.stage();
            let shown = self.shown_size();
            self.panels.library = true;
            self.keep_centre(old, shown);
        }
    }

    /// The Studio opened with `n` steps: with none, the Steps sidebar starts closed (owner
    /// rule: no empty panels, F8); it opens with the first step or by hand (Ctrl+B).
    pub fn steps_at_open(&mut self, n: usize) {
        if n == 0 && self.panels.library {
            self.panels.library = false;
            // (a panel that slides over the canvas is not reopened by itself)
            self.closed_for_empty = self.panels.pinned;
        }
    }

    pub fn moving(&self) -> bool {
        self.slide.is_some()
            || self.steps_slide.is_some()
            || self.rise_from.is_some()
            || self.opening.is_some()
            || self.mips.as_ref().is_some_and(|m| m.pending.is_some())
    }

    pub fn dirty(&self) -> bool {
        (self.editor.as_ref()).is_some_and(|e| Some(e.doc.state()) != self.saved)
    }

    /// The shown picture's size, and whether Ctrl is held (the host's tests).
    #[cfg(test)]
    pub fn probe(&self) -> (Option<Size>, bool) {
        (self.editor.as_ref().map(|e| e.doc.view().size()), self.ctrl)
    }

    /// What the picture panel's rows show: the user's own keys, where Ask AI
    /// goes, whether the translation is on.
    pub(super) fn info(&self) -> more::Info {
        more::Info {
            caps: more::caps_from(&crate::shared::config::current()),
            browser: self.browser,
            translated: self.original.is_some() && self.card.is_none(),
            ..more::Info::default()
        }
    }

    /// The window has the keyboard (and a picture is open): the quick-action
    /// keys of Copy text / Ask AI / Translate act here.
    pub fn takes_quick_keys(&self) -> bool {
        self.focused && self.editor.is_some() && !self.asking()
    }

    /// The row that is reading or translating (`None`: none).
    pub fn set_busy(&mut self, row: Option<Row>) {
        self.busy = row;
    }

    pub fn busy(&self) -> Option<Row> {
        self.busy
    }

    /// The download card up (`None`: gone).
    pub fn set_card(&mut self, card: Option<Fetching>) {
        self.card = card;
        self.sync_blocked();
    }

    pub fn card(&self) -> Option<&Fetching> {
        self.card.as_ref()
    }

    pub fn set_browser(&mut self, on: bool) {
        self.browser = on;
    }

    /// Which picture this is (`open` counts them).
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// The clean capture (no drawing on it) with every hidden part painted
    /// out, and those parts: what Copy text reads. A blurred secret never
    /// reaches the reader.
    pub fn read_picture(&self) -> Option<(Crop, more::Hidden)> {
        let doc = &self.editor.as_ref()?.doc;
        let hidden = annotate::hidden_in_clean(doc);
        let mut crop = annotate::clean(doc);
        more::mask(&mut crop, &hidden);
        Some((crop, hidden))
    }

    /// Cut out starts: the picture's own pixels (drawings are items over them,
    /// they stay), its size and which picture this is.
    pub fn cut_source(&mut self) -> Option<(Vec<u8>, u32, u32, u32)> {
        let e = self.editor.as_ref()?;
        self.cutting = cut::Phase::Working;
        self.busy = None;
        Some((
            e.doc.image.rgba.clone(),
            e.doc.image.width,
            e.doc.image.height,
            self.epoch,
        ))
    }

    pub fn cut_phase(&self) -> cut::Phase {
        self.cutting
    }

    /// The cut's answer for the picture `epoch`: applied as one undoable edit
    /// (the compare view opens), or the model's card, or a pill.
    /// (the host's answer can raise a card: the keys are the card's)
    pub fn set_cut(&mut self, epoch: u32, done: cut::Done) -> Outcome {
        let out = self.set_cut_inner(epoch, done);
        self.sync_blocked();
        out
    }

    fn set_cut_inner(&mut self, epoch: u32, done: cut::Done) -> Outcome {
        if epoch != self.epoch || self.cutting != cut::Phase::Working {
            return Outcome::None;
        }
        self.cutting = cut::Phase::Idle;
        let alpha = match done {
            cut::Done::NeedsModel => {
                self.cutting = cut::Phase::Asking;
                self.card = Some(Fetching::cutout());
                return Outcome::None;
            }
            cut::Done::Failed(why) => return Outcome::Say(why),
            cut::Done::Alpha(alpha) => alpha,
        };
        let Some(e) = self.editor.as_mut() else {
            return Outcome::None;
        };
        let before = e.doc.image.rgba.clone();
        let mut after = before.clone();
        crate::features::cutout::apply(&mut after, &alpha);
        if !e.doc.set_rows(0, &after) {
            return Outcome::Say(crate::t!(st_cut_nothing).to_string());
        }
        e.own_image = true; // its pixels are no longer the opened picture's
        e.refresh();
        self.transparent = true;
        self.cut = Some(cut::Cut {
            before,
            after,
            state: e.doc.state(),
            showing_before: false,
        });
        Outcome::None
    }

    /// The compare view is up (a cut made and nothing since).
    pub(super) fn comparing(&self) -> Option<bool> {
        let (c, e) = (self.cut.as_ref()?, self.editor.as_ref()?);
        (c.state == e.doc.state()).then_some(c.showing_before)
    }

    /// Before / After: the picture's pixels shown, not an edit.
    fn show_cut(&mut self, before: bool) {
        let (Some(c), Some(e)) = (self.cut.as_mut(), self.editor.as_mut()) else {
            return;
        };
        if c.showing_before == before || c.state != e.doc.state() {
            return;
        }
        c.showing_before = before;
        let shown = if before { &c.before } else { &c.after };
        e.doc.image.rgba.copy_from_slice(shown);
        e.refresh();
    }

    /// Anything that works on the picture sees the cut, not the "Before" view.
    fn settle_cut(&mut self) {
        self.show_cut(false);
    }

    /// Edit text stops (the reader's models are missing: the card asks).
    pub fn cancel_edit(&mut self) {
        self.edit = None;
    }

    /// The read the running Edit text session waits for (0: none).
    pub fn edit_token(&self) -> u32 {
        self.edit.as_ref().map_or(0, |s| s.token)
    }

    /// The lines read for session `token`, as the reader gave them in
    /// `read_picture`'s px; or why none (a pill). An answer for a cancelled
    /// session is dropped.
    pub fn set_lines(&mut self, token: u32, read: Result<Vec<Line>, String>) -> Outcome {
        let (Some(e), Some(session)) = (self.editor.as_ref(), self.edit.as_mut()) else {
            return Outcome::None;
        };
        if token != session.token {
            return Outcome::None;
        }
        // read from the clean view: back to image px
        let view = e.doc.view();
        let (dx, dy) = (view.x.floor(), view.y.floor());
        match read {
            Ok(lines) if !lines.is_empty() => {
                session.lines = Some(
                    lines
                        .into_iter()
                        .map(|mut l| {
                            (l.bounds[0], l.bounds[1]) = (l.bounds[0] + dx, l.bounds[1] + dy);
                            l
                        })
                        .collect(),
                );
                Outcome::None
            }
            Ok(_) => {
                self.edit = None;
                Outcome::Say(crate::t!(sn_no_text).to_string())
            }
            Err(why) => {
                self.edit = None;
                Outcome::Say(why)
            }
        }
    }

    /// Done: the picked line says the typed text in the pixels, one undoable edit.
    fn finish_edit(&mut self) -> Outcome {
        let (Some(e), Some(session)) = (self.editor.as_mut(), self.edit.as_ref()) else {
            return Outcome::None;
        };
        let Some((line, text)) = session.target() else {
            return Outcome::None;
        };
        if text.is_empty() || text == line.text.trim() {
            self.edit = None; // nothing to change
            return Outcome::None;
        }
        let (w, d) = (e.doc.image.width, &e.doc.image);
        let (top, mut pixels, moved) = edit_text::band(&d.rgba, w, d.height, line);
        let rows = (pixels.len() / (w as usize * 4)) as u32;
        let drawn = translate::replace_line(&mut pixels, w, rows, &moved, text, session.tweak());
        if drawn.drawn == 0 {
            // (stays on the line: the text can be changed)
            return Outcome::Say(crate::t!(st_edit_failed).to_string());
        }
        e.doc.set_rows(top, &pixels);
        e.own_image = true; // its pixels are no longer the opened picture's
        e.refresh();
        self.edit = None;
        Outcome::None
    }

    /// The picture as shown, drawings on it: what Ask AI gets.
    pub fn annotated(&self) -> Option<Crop> {
        Some(annotate::render(&self.editor.as_ref()?.doc))
    }

    /// Translate starts: the whole picture, hidden parts painted out, and the
    /// area shown (image px); the picture as it was is kept for Show original.
    pub fn translate_frame(&mut self) -> Option<(image::Handle, [u32; 4])> {
        let e = self.editor.as_mut()?;
        let doc = &e.doc;
        let (w, h) = (doc.image.width, doc.image.height);
        let view = doc.view();
        let edge = |v: f32, max: u32| (v.max(0.0) as u32).min(max);
        let area = [
            edge(view.x.floor(), w),
            edge(view.y.floor(), h),
            edge((view.x + view.width).ceil(), w),
            edge((view.y + view.height).ceil(), h),
        ];
        let original = doc.image.rgba.clone();
        let mut frame = Crop {
            width: w,
            height: h,
            rgba: original.clone(),
        };
        more::mask(&mut frame, &annotate::hidden(doc));
        self.original = Some((original, e.own_image));
        Some((image::Handle::from_rgba(w, h, frame.rgba), area))
    }

    /// The translation as far as it is: pasted into the picture under the
    /// drawings, the hidden parts as they were.
    pub fn show_translation(&mut self, from: &image::Handle, area: [u32; 4], whole: bool) {
        let (Some((original, _)), Some(e)) = (&self.original, self.editor.as_mut()) else {
            return;
        };
        let image::Handle::Rgba {
            width, pixels: src, ..
        } = from
        else {
            return;
        };
        let hidden = annotate::hidden(&e.doc);
        let dst_w = e.doc.image.width as usize;
        let area = area.map(|v| v as usize);
        let dst = &mut e.doc.image.rgba;
        more::paste_translation(dst, dst_w, src, *width as usize, area, whole);
        more::keep_hidden(dst, original, dst_w, area, &hidden);
        e.own_image = true; // its pixels are no longer the opened picture's
        e.refresh();
    }

    /// Show original: the picture as it was before Translate.
    pub fn show_original(&mut self) {
        if let (Some((original, was_own)), Some(e)) = (self.original.take(), self.editor.as_mut()) {
            if e.doc.image.rgba.len() == original.len() {
                e.doc.image.rgba.copy_from_slice(&original);
            }
            e.own_image = was_own;
            e.refresh();
        }
    }

    /// The translation is on the picture (or being made).
    pub fn translated(&self) -> bool {
        self.original.is_some()
    }

    /// A picture is open.
    pub fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    /// The unsaved-changes question is up.
    pub fn asking(&self) -> bool {
        self.asking.is_some()
    }

    /// The OS (or the header's x) asked to close.
    /// (the question it raises has the keys)
    pub fn close_requested(&mut self) -> Outcome {
        let out = self.close_requested_inner();
        self.sync_blocked();
        out
    }

    fn close_requested_inner(&mut self) -> Outcome {
        match self.asks() {
            true => {
                self.asking = Some(After::Close);
                Outcome::None
            }
            false => {
                self.drop_copy();
                Outcome::Close
            }
        }
    }

    /// The app is quitting: unsaved drawings ask first (`Outcome::Quit` after).
    /// (the question it raises has the keys)
    pub fn quit_requested(&mut self) -> Outcome {
        let out = self.quit_requested_inner();
        self.sync_blocked();
        out
    }

    fn quit_requested_inner(&mut self) -> Outcome {
        match self.asks() {
            true => {
                self.asking = Some(After::Quit);
                Outcome::None
            }
            false => {
                self.drop_copy();
                Outcome::Quit
            }
        }
    }

    /// The last Save landed: what is shown now is saved.
    pub fn saved(&mut self) {
        self.drop_copy();
        self.sync_blocked();
        if let Some(e) = &self.editor {
            self.saved = Some(e.doc.state());
        }
    }

    /// After the question's Save landed (or Don't save): what waited goes on.
    /// (the question is answered: the keys are the canvas's again)
    pub fn proceed(&mut self) -> Outcome {
        let out = self.proceed_inner();
        self.sync_blocked();
        out
    }

    fn proceed_inner(&mut self) -> Outcome {
        match self.asking.take() {
            Some(After::Close) => {
                self.drop_copy();
                Outcome::Close
            }
            Some(After::Quit) => {
                self.drop_copy();
                self.saved = self.editor.as_ref().map(|e| e.doc.state()); // nothing left to ask
                Outcome::Quit
            }
            Some(After::Open(picture)) => {
                self.drop_copy();
                self.saved = self.editor.as_ref().map(|e| e.doc.state());
                self.open(picture);
                Outcome::None
            }
            None => Outcome::None,
        }
    }

    /// Ctrl+V found a picture on the clipboard while one is open: the editor
    /// pastes our copied objects or a movable pasted picture (S1).
    pub fn paste_objects(&mut self, clip: Crop) -> Outcome {
        if let Some(e) = &mut self.editor {
            e.paste_clipboard(Some(clip));
        }
        self.sync_stage();
        Outcome::None
    }

    /// Ctrl+V found no picture: the editor's own paste (text at `at`).
    pub fn paste_text(&mut self, at: Option<Point>) -> Outcome {
        self.on_editor(annotate::Message::Paste(at))
    }

    pub fn update(&mut self, msg: Msg) -> Outcome {
        let before = self.layers_up();
        let out = self.update_inner(msg);
        self.sync_blocked();
        // a menu, popover or dialog that just opened rises to its place (motion off: none)
        let after = self.layers_up();
        let animate = !crate::shared::kit::motion::reduced() && !self.at_rest;
        if (0..after.len()).any(|i| after[i] && !before[i]) && animate {
            let now = std::time::Instant::now();
            (self.now, self.rise_from) = (now, Some(now));
        }
        out
    }

    /// Which of menu, popover, preferences, shortcuts, question are up.
    fn layers_up(&self) -> [bool; 5] {
        [
            self.menu.is_some(),
            self.popover.is_some(),
            self.prefs.is_some(),
            self.shortcuts,
            self.asking(),
        ]
    }

    /// How far below its place a menu, popover or dialog is now (px): `motion::RISE` as it opens,
    /// 0 when it has risen (or motion is off).
    pub(super) fn rise(&self) -> f32 {
        match self.rise_from {
            Some(t) => {
                let k = crate::shared::kit::motion::progress(
                    t,
                    self.now,
                    crate::shared::kit::motion::OPEN,
                    crate::shared::kit::motion::Curve::Out,
                );
                crate::shared::kit::motion::RISE * (1.0 - k)
            }
            None => 0.0,
        }
    }

    /// A menu, the right-click menu, a sheet, the narrow overlay or a dialog is up:
    /// it has the keys, the canvas takes none (Esc closes it first, the arrows walk it).
    pub(super) fn blocked(&self) -> bool {
        self.menu.is_some()
            || self.context.is_some()
            || self.text.menu_open()
            || self.shortcuts
            || self.prefs.is_some()
            || self.asking()
            || self.exporting
            || self.card.is_some()
            || self.restorable.as_ref().is_some_and(|_| !self.later)
    }

    fn sync_blocked(&mut self) {
        // (the unsaved question takes over: files that were still arriving are dropped)
        if self.asking() {
            self.drops.clear();
        }
        let blocked = self.blocked();
        if let Some(e) = &mut self.editor {
            e.keys_blocked = blocked;
        }
    }

    fn update_inner(&mut self, msg: Msg) -> Outcome {
        let old_panels = self.panels;
        let quiet_msg = matches!(
            msg,
            Msg::AltDown
                | Msg::AltUp
                | Msg::Pointer(_)
                | Msg::Mods(..)
                | Msg::Tick
                | Msg::Frame(_)
                | Msg::Focus(_)
                | Msg::Scrolled(_)
                | Msg::Resized(_)
                | Msg::Scale(_)
        );
        if !quiet_msg {
            self.alt_clean = false;
        }
        // a typed X / Y / W / H is one edit, made on Enter or when the user moves on
        // to something that is not typing: a key the field took (`AltBreak`, sent for
        // every key) is typing, not moving on
        if !matches!(
            msg,
            Msg::BoundsEdit(..) | Msg::BoundsCommit | Msg::AltBreak | Msg::Space(false)
        ) && !quiet_msg
        {
            let out = self.commit_bounds();
            if !matches!(out, Outcome::None) {
                return out;
            }
        }
        // a pick in a popover (an Actions row, the order buttons, Merge) closes it: its
        // catch layer must never stay over the picture (Edit text's first pick)
        if matches!(
            msg,
            Msg::Act(_)
                | Msg::EditText
                | Msg::CutOut
                | Msg::Editor(
                    annotate::Message::Order(_)
                        | annotate::Message::Merge
                        | annotate::Message::HideShape(_)
                        | annotate::Message::Eyedropper
                )
        ) {
            self.popover = None;
        }
        // (these change the pixels: the words read are stale)
        if matches!(msg, Msg::EditText | Msg::CutOut | Msg::Act(Row::Translate)) {
            self.text.reset();
        }
        // the question blocks the picture: nothing changes it until answered
        if self.asking()
            && matches!(
                msg,
                Msg::Editor(_)
                    | Msg::Open
                    | Msg::Paste
                    | Msg::Dropped(_)
                    | Msg::DropSettle(_)
                    | Msg::Picked(_)
                    | Msg::OpenRecent(_)
                    | Msg::ShowRecordings
                    | Msg::Wheel { .. }
                    | Msg::Act(_)
                    | Msg::EditText
                    | Msg::CutOut
                    | Msg::Download
            )
        {
            return Outcome::None;
        }
        if matches!(
            msg,
            Msg::Editor(_) | Msg::Act(_) | Msg::EditText | Msg::CutOut | Msg::SaveFirst
        ) {
            self.settle_cut();
        }
        match msg {
            // while a line is edited the picture's tools rest; the bar's size,
            // colour and Bold are the line's own
            Msg::Editor(m) if self.edit.is_some() => {
                if let Some(s) = &mut self.edit {
                    s.set(m);
                }
                Outcome::None
            }
            Msg::Act(_) if self.edit.is_some() => Outcome::None,
            Msg::CutView(before) => {
                self.show_cut(before);
                Outcome::None
            }
            Msg::CutClose => {
                self.show_cut(false);
                self.cut = None;
                Outcome::None
            }
            Msg::CutOut
                if self.editor.is_none()
                    || self.card.is_some()
                    || self.original.is_some()
                    || self.edit.is_some()
                    || self.cutting != cut::Phase::Idle =>
            {
                Outcome::None
            }
            Msg::CutOut => Outcome::CutOut,
            Msg::EditText
                if self.editor.is_none()
                    || self.card.is_some()
                    || self.original.is_some()
                    || self.edit.is_some() =>
            {
                Outcome::None
            }
            Msg::EditText => {
                let token = NEXT_READ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                self.edit = Some(edit_text::Session::new(token));
                Outcome::ReadLines
            }
            Msg::PickLine(i) => {
                let (Some(s), Some(e)) = (&mut self.edit, &self.editor) else {
                    return Outcome::None;
                };
                let Some(line) = s.lines.as_ref().and_then(|l| l.get(i)) else {
                    return Outcome::None;
                };
                let d = &e.doc.image;
                let (_, rows, moved) = edit_text::band(&d.rgba, d.width, d.height, line);
                let m = translate::matched(
                    &rows,
                    d.width,
                    (rows.len() / (d.width as usize * 4)) as u32,
                    &moved,
                );
                s.pick(i, m);
                Outcome::FocusField
            }
            Msg::Draft(text) => {
                if let Some(s) = &mut self.edit {
                    s.draft = text;
                }
                Outcome::None
            }
            Msg::EditDone => self.finish_edit(),
            Msg::Text(ev) => self.on_text(ev),
            Msg::Editor(m) if self.text_takes(&m) => self.text_key(&m),
            Msg::Editor(annotate::Message::Paste(at))
                if self.editor.as_ref().is_some_and(|e| !e.typing()) =>
            {
                Outcome::Paste(at)
            }
            Msg::Editor(m) => self.on_editor(m),
            Msg::Caption(Caption::Close) => self.close_requested(),
            Msg::Caption(c) => Outcome::Caption(c),
            Msg::Library => self.open_steps(!self.panels.library),
            Msg::PinSteps => {
                let old = self.stage();
                let shown = self.shown_size();
                self.panels.pinned = !self.panels.pinned;
                self.steps_slide = None; // (docked or not, it simply stays where it is)
                self.keep_centre(old, shown);
                Outcome::Panels(old_panels, self.panels)
            }
            // a press outside the open panel: it slides away, unless it is pinned or a step
            // is being dragged
            Msg::StepsAway => match self.pick.dragging() {
                false => self.open_steps(false),
                true => Outcome::None,
            },
            Msg::Pop(p) => {
                self.popover = p.filter(|p| self.popover != Some(*p));
                Outcome::None
            }
            Msg::PickFiles => {
                self.asked = true;
                Outcome::PickFiles
            }
            Msg::AddFiles(files) => match files.is_empty() {
                true => Outcome::None,
                false => Outcome::AddPictures(files),
            },
            Msg::PasteStep => Outcome::PasteStep,
            Msg::UiScale(by) => {
                self.ui_zoom = crate::shared::config::zoom_step(self.ui_zoom, by);
                // remembered, like the AI chat's text zoom
                self.panels.ui_zoom = self.ui_zoom;
                Outcome::Panels(old_panels, self.panels)
            }
            Msg::OpenPrefs => {
                self.popover = None;
                self.menu = None;
                self.prefs = Some(preferences::Dialog::open());
                Outcome::None
            }
            Msg::Prefs(m) => {
                let Some(d) = &mut self.prefs else {
                    return Outcome::None;
                };
                if let preferences::Msg::UiZoom(z) = m {
                    // the Studio's own size, kept like the View menu's (C3)
                    d.ui_zoom = z;
                    (self.ui_zoom, self.panels.ui_zoom) = (z, z);
                    return Outcome::Panels(old_panels, self.panels);
                }
                if matches!(m, preferences::Msg::ResetToolStyles) {
                    self.reset_styles = true;
                }
                match d.update(m) {
                    preferences::Out::None => Outcome::None,
                    preferences::Out::Changed => Outcome::ApplyPrefs,
                    preferences::Out::OpenGeneral => Outcome::PrefsOpenGeneral,
                    preferences::Out::Close => {
                        self.prefs = None;
                        Outcome::None
                    }
                }
            }
            Msg::OverSidebar(on) => {
                self.over_sidebar = on;
                Outcome::None
            }
            Msg::Picked(path) => {
                self.asked = true;
                self.next_name = file_name(&path);
                Outcome::Read(path)
            }
            Msg::Middle(on) => {
                self.middle = on;
                self.pan = on.then(|| self.pointer.get());
                Outcome::None
            }
            Msg::ZoomIn | Msg::ZoomOut | Msg::Actual => {
                let Some(e) = &self.editor else {
                    return Outcome::None;
                };
                let zoom = match msg {
                    Msg::ZoomIn => e.zoom * ZOOM_STEP,
                    Msg::ZoomOut => e.zoom / ZOOM_STEP,
                    _ => 1.0 / self.scale,
                };
                self.zoom_to(zoom, None)
            }
            Msg::Wheel { at, notches } => {
                // Ctrl+wheel over the selection resizes it (annotate::wheel); elsewhere it zooms
                let over = self.image_point(at);
                if self.ctrl
                    && let Some(e) = &mut self.editor
                    && e.wheel_resize(over, notches, std::time::Instant::now())
                {
                    return Outcome::None;
                }
                let Some(e) = &self.editor else {
                    return Outcome::None;
                };
                self.zoom_to(e.zoom * ZOOM_STEP.powf(notches), Some(at))
            }
            Msg::Scrolled(offset) => {
                self.offset = offset;
                self.pending_scroll = None;
                self.sync_view();
                Outcome::None
            }
            Msg::Ctrl(down) => {
                self.ctrl = down;
                Outcome::None
            }
            Msg::Fit => {
                self.fit_to_stage();
                self.take_scroll().map_or(Outcome::None, Outcome::ScrollTo)
            }
            Msg::Open => {
                self.asked = true;
                Outcome::PickFile
            }
            Msg::Paste => {
                self.asked = true;
                Outcome::Paste(None)
            }
            Msg::Hovered(on) => {
                self.hovered = on;
                Outcome::None
            }
            Msg::Dropped(path) => {
                self.asked = true;
                self.hovered = false;
                self.drops.push(path);
                Outcome::SettleDrop(self.drops.len())
            }
            // what was dropped: several files = all steps (an empty Studio also opens
            // the first); one on the Steps sidebar = a step; one on an open picture =
            // placed on it; one on the empty Studio opens
            Msg::DropSettle(n) if n == self.drops.len() && n > 0 => {
                let files = std::mem::take(&mut self.drops);
                match (files.len(), self.editor.is_some()) {
                    (1, _) if self.over_sidebar => Outcome::AddPictures(files),
                    (1, true) => Outcome::Place(files[0].clone()),
                    (1, false) => {
                        self.next_name = file_name(&files[0]);
                        Outcome::Read(files[0].clone())
                    }
                    (_, true) => Outcome::AddPictures(files),
                    (_, false) => {
                        self.next_name = file_name(&files[0]);
                        Outcome::OpenAndAdd(files)
                    }
                }
            }
            Msg::DropSettle(_) => Outcome::None,
            Msg::OpenRecent(path) => {
                self.asked = true;
                self.next_name = file_name(&path);
                Outcome::Read(path)
            }
            Msg::ShowRecordings => Outcome::ShowFolder,
            Msg::Recent(recent, info) => {
                self.recent_info = info;
                self.recent = recent;
                // Settings > Studio > Reopen the last picture: the newest, when none is open
                match self.reopen(crate::shared::config::current().studio.reopen_last) {
                    Some(p) => {
                        self.asked = true;
                        self.next_name = file_name(&p);
                        Outcome::Read(p)
                    }
                    None => Outcome::None,
                }
            }
            Msg::Scale(s) => {
                let old_stage = self.stage();
                let old_shown = self.shown_size();
                let old_scale = self.scale;
                self.scale = s.max(0.1);
                if let Some(e) = &mut self.editor {
                    let actual = 1.0 / self.scale;
                    e.zoom = (e.zoom * old_scale / self.scale)
                        .clamp(ZOOM_RANGE.start() * actual, ZOOM_RANGE.end() * actual);
                }
                self.keep_centre(old_stage, old_shown);
                self.take_scroll().map_or(Outcome::None, Outcome::ScrollTo)
            }
            Msg::Resized(size) => {
                let old_stage = self.stage();
                let old_shown = self.shown_size();
                self.window = size;
                self.keep_centre(old_stage, old_shown);
                self.take_scroll().map_or(Outcome::None, Outcome::ScrollTo)
            }
            Msg::Act(_) if self.editor.is_none() || self.card.is_some() => Outcome::None,
            // a second Copy text while it reads would only read twice
            Msg::Act(Row::CopyText) if self.busy == Some(Row::CopyText) => Outcome::None,
            Msg::Download if self.card.is_some() => {
                if let Some(c) = &mut self.card {
                    (c.share, c.failed) = (Some(0.0), None);
                }
                Outcome::Download
            }
            Msg::Download => Outcome::None,
            Msg::CardDetails => {
                if let Some(card) = &mut self.card {
                    card.details = !card.details;
                }
                Outcome::None
            }
            Msg::DismissCard | Msg::Provider => {
                self.card = None;
                self.busy = None;
                self.cutting = cut::Phase::Idle;
                match msg {
                    Msg::Provider => Outcome::Provider,
                    _ => Outcome::Dismissed,
                }
            }
            Msg::Act(row) => Outcome::Act(row),
            Msg::Focus(on) => {
                self.focused = on;
                Outcome::None
            }
            Msg::DontSave => self.proceed(),
            Msg::SidebarPage(page) => {
                if self.page != page {
                    let now = std::time::Instant::now();
                    self.page = page;
                    self.now = now;
                    // (motion off: the page is simply there, no clock runs)
                    self.slide = (!crate::shared::kit::motion::reduced()).then_some(Slide {
                        start: now,
                        into: page == Page::Export,
                    });
                }
                Outcome::None
            }
            Msg::Frame(now) => {
                self.now = now;
                self.poll_open();
                if self.rise_from.is_some_and(|t| {
                    crate::shared::kit::motion::done(t, now, crate::shared::kit::motion::OPEN)
                        >= 1.0
                }) {
                    self.rise_from = None;
                }
                if self.mips.as_ref().is_some_and(|m| m.pending.is_some()) {
                    self.sync_stage(); // (the stage copy, once its thread is done)
                }
                if self.slide.is_some_and(|s| self.slide_done(s)) {
                    self.slide = None;
                }
                if let Some((start, opening, _)) = self.steps_slide
                    && crate::shared::kit::motion::done(start, now, steps_slide_len(opening)) >= 1.0
                {
                    self.steps_slide = None;
                }
                Outcome::None
            }
            Msg::Cancel => {
                // Esc ladder: a key being recorded, menu, sheet, overlay, the question, Edit text
                if let Some(d) = self.prefs.as_mut().filter(|d| d.recording()) {
                    d.stop_recording();
                    return Outcome::None;
                }
                // the sidebar's Export page goes back to the Steps (after the layers over it)
                if self.page == Page::Export
                    && self.menu.is_none()
                    && self.popover.is_none()
                    && self.prefs.is_none()
                    && !self.shortcuts
                    && !self.exporting
                    && self.context.is_none()
                    && !self.asking()
                {
                    return self.update(Msg::SidebarPage(Page::Steps));
                }
                let closed = self.sub.take().is_some()
                    || self.context.take().is_some()
                    || self.menu.take().is_some()
                    || std::mem::take(&mut self.shortcuts)
                    || self.popover.take().is_some()
                    || self.prefs.take().is_some()
                    || self.text.escape(self.epoch);
                // the export sheet (a running encoder stops), then the restore sheet (Later)
                if !closed && self.exporting {
                    return self.update(Msg::WalkCancel);
                }
                if !closed && self.restorable.is_some() && !self.later {
                    self.later = true;
                    return Outcome::None;
                }
                // last: Esc closes the open Steps panel (the Export page went back first)
                // (a pinned panel is the user's own layout: only its arrow, handle or Ctrl+B close it)
                if !closed
                    && self.asking.is_none()
                    && self.edit.is_none()
                    && self.panels.library
                    && !self.panels.pinned
                {
                    return self.open_steps(false);
                }
                if !closed {
                    match self.asking.take() {
                        // the restore that waited on the question: the copy stays
                        Some(After::Open(_)) => {
                            if let Some(r) = self.restoring.take() {
                                self.restorable = Some(r);
                            }
                        }
                        Some(_) => {}
                        None => self.edit = None,
                    }
                }
                Outcome::None
            }
            Msg::SaveFirst => match &mut self.editor {
                Some(e) => Outcome::SaveThen(annotate::render(&e.doc)),
                None => self.proceed(),
            },
            Msg::Lock(on) => {
                self.lock = on;
                self.update(Msg::Editor(annotate::Message::LockRatio(on)))
            }
            Msg::AltBreak => Outcome::None,
            Msg::AltDown => {
                self.alt_clean = true;
                Outcome::None
            }
            Msg::AltUp => match std::mem::take(&mut self.alt_clean) {
                true => self.update(Msg::MenuKey(Nav::Alt)),
                false => Outcome::None,
            },
            Msg::BoundsEdit(axis, text) => {
                if let Some(f) = self.bounds_fields() {
                    let mut draft = self.bounds_draft.take().unwrap_or(f);
                    draft[axis.min(3)] = text;
                    self.bounds_draft = Some(draft);
                }
                Outcome::None
            }
            Msg::BoundsCommit => self.commit_bounds(),
            Msg::Pointer(p) => {
                self.pointer.set(p);
                let Some(last) = self.pan else {
                    return Outcome::None;
                };
                self.sync_stage(); // (the stage copy, once its thread is done)
                self.pan = Some(p);
                let off = Vector::new(
                    (self.offset.x - (p.x - last.x)).max(0.0),
                    (self.offset.y - (p.y - last.y)).max(0.0),
                );
                self.offset = off;
                self.pending_scroll = None;
                Outcome::ScrollTo(AbsoluteOffset { x: off.x, y: off.y })
            }
            Msg::Mods(ctrl, shift) => {
                (self.ctrl, self.shift) = (ctrl, shift);
                Outcome::None
            }
            Msg::Tick => {
                // the debounce: the copy is written once the picture has been quiet
                match self.quiet.due(std::time::Instant::now()) {
                    true => self.copy_job(),
                    false => Outcome::None,
                }
            }
            Msg::Space(on) => {
                // (typing a text on the picture keeps its spaces)
                self.space = on && !self.editor.as_ref().is_some_and(|e| e.typing());
                self.pan = self.pan.filter(|_| self.space);
                Outcome::None
            }
            Msg::PanStart => {
                self.pan = Some(self.pointer.get());
                Outcome::None
            }
            Msg::PanEnd => {
                self.pan = None;
                Outcome::None
            }
            Msg::Snap | Msg::Grid => {
                let mut p = crate::shared::config::current().studio;
                match msg {
                    Msg::Snap => self.snap = !self.snap,
                    _ => self.grid = !self.grid,
                }
                (p.snap, p.pixel_grid) = (self.snap, self.grid);
                if let Some(e) = &mut self.editor {
                    (e.snap, e.grid) = (self.snap, self.grid);
                }
                self.menu = None;
                self.sub = None;
                Outcome::Prefs(p)
            }
            Msg::ClearRecent => {
                self.recent.clear();
                self.recent_info.clear();
                Outcome::ClearRecent
            }
            Msg::SubPick(sub, i) => {
                self.menu = None;
                self.sub = None;
                match menu_bar::sub_items(sub, self)
                    .into_iter()
                    .nth(i)
                    .map(|r| r.press)
                {
                    Some(press) => self.update(press),
                    None => Outcome::None,
                }
            }
            Msg::ContextOpen => {
                let at = self.image_point(self.pointer.get());
                let _ = self.on_editor(annotate::Message::SelectAt(at, false));
                let Some(e) = &self.editor else {
                    return Outcome::None;
                };
                let entries = match e.selection().is_empty() {
                    true => editing::picture_menu(e),
                    false => editing::object_menu(e),
                };
                // the stage's left edge: mirrored, the rail's column is at the other edge
                let rail_left =
                    view::rail_on_left(self.sidebar_at_start(), crate::shared::app_language::rtl());
                let left = match rail_left {
                    true => RAIL + 2.0,
                    false => 0.0,
                };
                let origin = Point::new(left, HEADER + OPTIONS + 1.0);
                let seps = entries.iter().filter(|r| r.divider).count();
                let menu = Size::new(
                    crate::shared::kit::option_list::MENU_WIDTH,
                    crate::shared::kit::option_list::menu_height(entries.len(), seps),
                );
                let p = self.pointer.get();
                let at = crate::shared::kit::option_list::place(
                    Point::new(origin.x + p.x, origin.y + p.y),
                    menu,
                    self.window,
                    EDGE,
                );
                self.context = Some(Context { at, entries });
                Outcome::None
            }
            Msg::ContextPick(i) => {
                let Some(c) = self.context.take() else {
                    return Outcome::None;
                };
                match c.entries.get(i).filter(|r| r.enabled).map(|r| r.action) {
                    Some(a) => self.update(menu_bar::action_msg(a)),
                    None => Outcome::None,
                }
            }
            Msg::Restore => {
                let Some(r) = self.restorable.take() else {
                    return Outcome::None;
                };
                let Ok(img) = ::image::load_from_memory(&r.png) else {
                    // (the copy stays: it may be readable by a later version)
                    self.restorable = Some(r);
                    self.restore_failed = true;
                    return Outcome::None;
                };
                let img = img.into_rgba8();
                // the picture and its drawings come back in `open`; with unsaved
                // work on screen the question comes first (Cancel keeps the copy)
                self.restoring = Some(r);
                self.open(Picture::Crop(Crop {
                    width: img.width(),
                    height: img.height(),
                    rgba: img.into_raw(),
                }));
                Outcome::None
            }
            Msg::DiscardCopy => {
                self.restorable = None;
                recovery::discard();
                Outcome::None
            }
            Msg::Menu(m) => {
                self.sub = None;
                self.menu = m.map(|m| (m, 0));
                Outcome::None
            }
            Msg::MenuPick(menu, i) => {
                let row = menu_bar::items(menu, self)
                    .into_iter()
                    .nth(i)
                    .map(|r| (r.press, r.sub));
                if let Some(sub) = row.as_ref().and_then(|r| r.1) {
                    self.sub = Some(sub);
                    return Outcome::None;
                }
                self.menu = None;
                self.sub = None;
                match row {
                    Some((press, _)) => self.update(press),
                    None => Outcome::None,
                }
            }
            Msg::MenuKey(Nav::Enter) if self.asking() => self.update(Msg::SaveFirst),
            Msg::MenuKey(key) => {
                let (menu, pick) = menu_bar::nav(self, key);
                self.menu = menu;
                match pick {
                    Some(m) => self.update(m),
                    None => Outcome::None,
                }
            }
            Msg::Shortcuts(on) => {
                self.shortcuts = on;
                Outcome::None
            }
            // the shortcuts sheet's Edit: the keys are Studio preferences' own page
            Msg::EditShortcuts => {
                self.shortcuts = false;
                let _ = self.update(Msg::OpenPrefs);
                if let Some(d) = &mut self.prefs {
                    d.show_shortcuts();
                }
                Outcome::None
            }
            Msg::Later => {
                self.later = true;
                Outcome::None
            }
            Msg::NewSnip => Outcome::NewSnip,
            Msg::StepPress(i) => {
                self.pick.press(i);
                Outcome::None
            }
            Msg::StepMove(at, n) => {
                let cell = iced::Size::new(STEP_CELL.width + STEP_GAP, STEP_CELL.height + STEP_GAP);
                self.pick
                    .moved_grid(at, cell, STEP_COLS, n, crate::shared::app_language::rtl());
                Outcome::None
            }
            Msg::StepRelease => match self.pick.release() {
                Some(Released::Moved(from, to)) => Outcome::Steps(steps::Msg::Reorder(from, to)),
                Some(Released::Click(i)) => Outcome::OpenStep(i),
                None => Outcome::None,
            },
            Msg::StepHover(i, on) => {
                self.step_hover = match on {
                    true => Some(i),
                    false => self.step_hover.filter(|&h| h != i),
                };
                Outcome::None
            }
            Msg::StepDelete(i) => {
                self.pick.cancel();
                self.step_hover = None;
                Outcome::Steps(steps::Msg::Delete(i))
            }
            Msg::CopyAllSteps => Outcome::Steps(steps::Msg::CopyAll),
            Msg::ExportSteps(how) => Outcome::ExportSteps(how),
            Msg::ExportWalkthrough => {
                self.exporting = true;
                self.walk_tab = true;
                self.walk.edits = false;
                Outcome::None
            }
            Msg::ExportTab(walk) => {
                self.walk_tab = walk;
                Outcome::None
            }
            Msg::WalkFormat(movie) => {
                self.walk.how.movie = movie;
                self.panels.walk_format = movie.index();
                Outcome::Panels(old_panels, self.panels)
            }
            Msg::WalkProgress(done, total) => {
                self.walk.busy = self.walk.busy.map(|_| (done, total));
                Outcome::None
            }
            Msg::WalkEnded(done) => {
                self.walk.busy = None;
                self.exporting = self.exporting && !done;
                Outcome::None
            }
            Msg::WalkEdits(on) => self.walk_set(|w| w.edits = on),
            Msg::WalkSize(px) => self.walk_set(|w| w.how.max_edge = px),
            Msg::WalkTime(t) => self.walk_set(|w| w.tenths = t),
            Msg::WalkNumbered(on) => self.walk_set(|w| w.how.numbered = on),
            Msg::WalkLoop(on) => self.walk_set(|w| w.how.repeat = on),
            Msg::WalkTransition(kind) => {
                self.walk.how.transition = kind;
                self.panels.walk_transition = kind.index();
                Outcome::Panels(old_panels, self.panels)
            }
            Msg::WalkTransitionMs(ms) => {
                self.walk.how.transition_ms = ms;
                self.panels.walk_ms = ms;
                Outcome::Panels(old_panels, self.panels)
            }
            Msg::WalkCancel if self.walk.busy.is_some() => Outcome::CancelWalkthrough,
            Msg::WalkCancel => {
                self.exporting = false;
                Outcome::None
            }
            Msg::WalkGo if self.walk.busy.is_some() => Outcome::None,
            Msg::WalkGo => {
                let w = self.walk;
                match (w.edits, &mut self.editor) {
                    (true, Some(e)) => {
                        self.walk.busy = Some((0, 0));
                        Outcome::ExportWalkthrough(w.how(), Frames::Edits(Box::new(e.doc.clone())))
                    }
                    (true, None) => Outcome::None,
                    (false, _) => {
                        self.walk.busy = Some((0, 0));
                        Outcome::ExportWalkthrough(w.how(), Frames::Steps)
                    }
                }
            }
            Msg::Export => {
                self.exporting = true;
                self.walk_tab = false;
                Outcome::None
            }
            Msg::ExportFormat(format) => {
                self.export.format = format;
                Outcome::None
            }
            Msg::ExportQuality(quality) => {
                self.export.quality = quality;
                Outcome::None
            }
            Msg::ExportSize(max_edge) => {
                self.export.max_edge = max_edge;
                Outcome::None
            }
            Msg::ExportGo => {
                self.exporting = false;
                match &self.editor {
                    Some(e) => Outcome::ExportPicture(annotate::render(&e.doc), self.export),
                    None => Outcome::None,
                }
            }
        }
    }

    fn walk_set(&mut self, f: impl FnOnce(&mut Walk)) -> Outcome {
        if self.walk.busy.is_none() {
            f(&mut self.walk);
        }
        Outcome::None
    }

    fn on_editor(&mut self, m: annotate::Message) -> Outcome {
        let Some(e) = &mut self.editor else {
            return Outcome::None;
        };
        let img_gen = e.doc.image_gen;
        let out = e.update(m);
        // Only rescan image alpha when the picture pixels or canvas dimensions actually changed.
        if img_gen != e.doc.image_gen {
            self.transparent = cut::has_alpha(&e.doc.image.rgba);
        }
        self.sync_stage(); // a crop changes what is shown
        self.keep_unsaved();
        let refusal = self.editor.as_mut().and_then(|e| e.take_grow_refusal());
        let hint = self.editor.as_mut().is_some_and(|e| e.take_ctrl_hint());
        match out {
            Some(Output::Copy(crop)) => Outcome::Copy(crop),
            Some(Output::Save(crop)) => Outcome::Save(crop),
            Some(Output::Pin(crop)) => Outcome::Pin(crop),
            Some(Output::Paste(at)) => Outcome::Paste(at),
            Some(Output::AddStep(crop)) => Outcome::AddStep(crop),
            Some(Output::AddStepDoc(doc)) => Outcome::AddStepDoc(doc),
            // "Grow the canvas past the edge" said no: a plain pill says why
            None if refusal.is_some() => Outcome::Say(grow_words(refusal)),
            // C5: a drag began on a shape; with a drawing tool it draws, Ctrl+drag moves
            None if hint => Outcome::Say(crate::t!(pill_ctrl_moves).to_string()),
            // Esc never closes the Studio (Alt+F4 / the x does); nothing else
            // the widget's toolbar asks for is on the Studio
            _ => Outcome::None,
        }
    }

    /// Settings > Studio's switches onto `editor` (as the picture opens, and when the
    /// dialog changes one while a picture is open).
    fn prefs_into(&mut self, editor: &mut Annotator) {
        let prefs = crate::shared::config::current().studio;
        editor.copy_picture_when_empty = prefs.ctrl_c_picture;
        editor.paste_movable = prefs.paste_movable;
        editor.grow_canvas = prefs.grow_canvas;
        editor.drag_jpeg = prefs.drag_type == crate::shared::config::Format::Jpeg;
        (self.snap, self.grid) = (prefs.snap, prefs.pixel_grid);
        (editor.snap, editor.grid) = (self.snap, self.grid);
    }

    /// The dialog's changes were written: the open picture follows them now (snap,
    /// pixel grid, Ctrl+C, paste, grow, drag type), and "Reset tool styles" resets
    /// the editor's own options too.
    pub fn prefs_applied(&mut self) {
        if let Some(mut e) = self.editor.take() {
            self.prefs_into(&mut e);
            if std::mem::take(&mut self.reset_styles) {
                e.reset_options(crate::shared::config::current().editor);
            }
            self.editor = Some(e);
        }
        // (the Save as sheet starts on the format, quality and size the dialog now says)
        self.export = export_defaults();
    }

    /// The preferences changed elsewhere (Settings): the dialog and the Studio read them again.
    pub fn prefs_reread(&mut self) {
        if let Some(d) = &mut self.prefs {
            d.reread();
        }
        self.prefs_applied();
    }

    /// The dialog's changes onto `c` (the host calls this inside `config::update`).
    pub fn apply_prefs(&mut self, c: &mut crate::shared::config::Config) {
        if let Some(d) = &mut self.prefs {
            d.apply(c);
        }
    }

    /// Settings > Studio > Steps sidebar side: at the reading start (left; right in
    /// Arabic), the rail on the other edge.
    pub(super) fn sidebar_at_start(&self) -> bool {
        crate::shared::config::current().studio.steps_side
            == crate::shared::config::StudioSide::Start
    }

    /// Studio preferences wait for a new key: the host sends the next chord.
    pub fn recording(&self) -> bool {
        self.prefs.as_ref().is_some_and(|d| d.recording())
    }

    /// The window less the Steps sidebar: a popover hangs inside it, never over the sidebar
    /// (F15: the Actions card covered the sidebar's buttons and text).
    pub(super) fn popover_room(&self) -> iced::Rectangle {
        let side = match self.sidebar_shown() {
            true => SIDEBAR + SLIDER_EDGE,
            false => 0.0,
        };
        let sidebar_left =
            !view::rail_on_left(self.sidebar_at_start(), crate::shared::kit::dir::rtl());
        let left = if sidebar_left { side } else { 0.0 };
        iced::Rectangle::new(
            Point::new(left, 0.0),
            Size::new(self.window.width - side, self.window.height),
        )
    }

    /// The Steps panel is open (docked or over the canvas).
    pub(super) fn sidebar_shown(&self) -> bool {
        self.panels.library
    }

    /// The Steps card is pinned open (it floats over the canvas either way; only an outside click differs).
    pub(super) fn sidebar_docked(&self) -> bool {
        self.panels.library && self.panels.pinned
    }

    /// The Steps card is drawn over the canvas: open (pinned or not), or still sliding.
    pub(super) fn steps_overlay(&self) -> bool {
        self.panels.library || self.steps_slide.is_some()
    }

    /// How far in the sliding panel is (0..1): the slide's own time and curve, else its rest.
    pub(super) fn steps_at(&self) -> f32 {
        use crate::shared::kit::motion::{Curve, progress};
        match self.steps_slide {
            Some((start, true, from)) => {
                let t = progress(
                    start,
                    self.now,
                    crate::shared::kit::motion::PANEL_OPEN,
                    Curve::Out,
                );
                from + (1.0 - from) * t
            }
            Some((start, false, from)) => {
                let t = progress(
                    start,
                    self.now,
                    crate::shared::kit::motion::PANEL_CLOSE,
                    Curve::In,
                );
                from * (1.0 - t)
            }
            None => f32::from(u8::from(self.panels.library)),
        }
    }

    /// The Steps panel opens or closes (the handle, the toggle, Ctrl+B, Esc, a press outside):
    /// it slides over the canvas, or at once when pinned or motion is reduced.
    fn open_steps(&mut self, open: bool) -> Outcome {
        if self.panels.library == open {
            return Outcome::None;
        }
        // (a slide reversed half way goes on from where the panel is, not from the edge)
        let from = self.steps_at();
        let old_stage = self.stage();
        let shown = self.shown_size();
        let old = self.panels;
        self.panels.library = open;
        self.closed_for_empty = false; // the user's own say now
        self.popover = None;
        self.steps_slide = None;
        if !self.panels.pinned && !crate::shared::kit::motion::reduced() {
            self.now = std::time::Instant::now();
            self.steps_slide = Some((self.now, open, from));
        }
        self.keep_centre(old_stage, shown);
        Outcome::Panels(old, self.panels)
    }

    /// Settings > Studio > "Keep unsaved work": every edit restarts the quiet
    /// timer (`recovery::Debounce`); `Msg::Tick` hands the host the copy to write
    /// once it ran out. Back to a clean state (undo, save) the copy is removed.
    fn keep_unsaved(&mut self) {
        if !crate::shared::config::current().studio.keep_unsaved {
            return;
        }
        match self.dirty() {
            true => self.quiet.changed(std::time::Instant::now()),
            false => self.drop_copy(),
        }
    }

    /// The unsaved copy to write: the picture's own pixels and the drawings as
    /// JSON (the host encodes and writes it off the UI thread).
    fn copy_job(&mut self) -> Outcome {
        let on = crate::shared::config::current().studio.keep_unsaved;
        if !on || !self.dirty() || self.restorable.is_some() {
            return Outcome::None;
        }
        let Some(e) = &self.editor else {
            return Outcome::None;
        };
        Outcome::KeepCopy(Box::new(KeepCopy {
            ticket: recovery::ticket(),
            width: e.doc.image.width,
            height: e.doc.image.height,
            rgba: e.doc.image.rgba.clone(),
            notes: e.annotations_json(),
        }))
    }

    /// The kept copy goes (saved, discarded, closed or back to clean). A copy
    /// waiting for Restore is not ours to remove.
    fn drop_copy(&mut self) {
        if self.restorable.is_none() && self.restoring.is_none() {
            recovery::discard();
        }
        self.quiet = recovery::Debounce::default();
    }

    /// The clock only runs while an edit waits for its copy.
    pub fn waiting(&self) -> bool {
        self.quiet.waiting()
    }

    /// The selection's box as typed fields (image px), when one is selected.
    pub(super) fn bounds_fields(&self) -> Option<[String; 4]> {
        let r = self.editor.as_ref()?.selection_bounds()?;
        Some([r.x, r.y, r.width, r.height].map(|v| (v.round() as i32).to_string()))
    }

    /// The typed box becomes one edit (a field left empty or wrong keeps its value).
    fn commit_bounds(&mut self) -> Outcome {
        let (Some(draft), Some(now)) = (
            self.bounds_draft.take(),
            self.editor.as_ref().and_then(|e| e.selection_bounds()),
        ) else {
            return Outcome::None;
        };
        let was = [now.x, now.y, now.width, now.height];
        let mut v = was;
        for (i, text) in draft.iter().enumerate() {
            if let Ok(n) = text.trim().parse::<f32>() {
                v[i] = if i >= 2 { n.max(1.0) } else { n };
            }
        }
        if v == was.map(f32::round) || v == was {
            return Outcome::None;
        }
        self.on_editor(annotate::Message::Bounds(Rectangle::new(
            Point::new(v[0], v[1]),
            Size::new(v[2], v[3]),
        )))
    }

    /// The picture's currently displayed size (logical px), if one is open.
    fn shown_size(&self) -> Option<Size> {
        self.editor.as_ref().map(Annotator::size)
    }

    /// The picture point under `at` (a point of the stage): the scroll and the
    /// centring undone, the zoom divided out, the view's corner added.
    pub(super) fn image_point(&self, at: Point) -> Point {
        let Some(e) = &self.editor else {
            return at;
        };
        let corner = stage_corner(self.stage(), e.size());
        let d = (Vector::new(at.x, at.y) + self.offset - corner) * (1.0 / e.zoom);
        let origin = e.doc.view().position();
        Point::new(origin.x + d.x, origin.y + d.y)
    }

    /// The stage: the window less the header, the rail, the bars, the shown panels and the
    /// pills' band: the room a fitted picture is centred in (the canvas itself runs under the pills).
    pub(super) fn stage(&self) -> Size {
        // each region with its 1 px hairline, and the window's 1 px rim all round
        Size::new(
            (self.window.width - 2.0 - (RAIL + 1.0)).max(1.0),
            (self.window.height - 2.0 - HEADER - (OPTIONS + 1.0) - BAND).max(1.0),
        )
    }

    /// Fits the picture to the current stage (called ONLY on initial open and deliberate `Msg::Fit`)
    /// and locks the resulting zoom so later panel changes and resizes never re-fit it.
    fn fit_to_stage(&mut self) {
        let (stage, scale) = (self.stage(), self.scale);
        if let Some(e) = &mut self.editor {
            let z = fit_zoom(e.doc.view().size(), stage, scale);
            e.zoom = z;
        }
        self.offset = Vector::ZERO;
        self.pending_scroll = Some(AbsoluteOffset { x: 0.0, y: 0.0 });
        self.sync_stage();
    }

    /// Keeps the picture point at the centre of the visible stage steady across a stage or scale
    /// change, leaving the zoom untouched.
    fn keep_centre(&mut self, old_stage: Size, old_shown: Option<Size>) {
        let new_stage = self.stage();
        if let (Some(e), Some(old_shown)) = (&self.editor, old_shown) {
            let new_shown = e.size();
            let same_stage = (old_stage.width - new_stage.width).abs() < 0.01
                && (old_stage.height - new_stage.height).abs() < 0.01;
            let same_shown = (old_shown.width - new_shown.width).abs() < 0.01
                && (old_shown.height - new_shown.height).abs() < 0.01;
            if !same_stage || !same_shown {
                let old_corner = stage_corner(old_stage, old_shown);
                let new_corner = stage_corner(new_stage, new_shown);
                let ratio_x = new_shown.width / old_shown.width.max(1.0);
                let ratio_y = new_shown.height / old_shown.height.max(1.0);
                let old_pic = Vector::new(
                    self.offset.x + old_stage.width / 2.0 - old_corner.x,
                    self.offset.y + old_stage.height / 2.0 - old_corner.y,
                );
                let new_pic = Vector::new(old_pic.x * ratio_x, old_pic.y * ratio_y);
                let max_x =
                    ((new_stage.width - 2.0).max(new_shown.width) - new_stage.width).max(0.0);
                let max_y =
                    ((new_stage.height - 2.0).max(new_shown.height) - new_stage.height).max(0.0);
                let next = Vector::new(
                    (new_pic.x + new_corner.x - new_stage.width / 2.0).clamp(0.0, max_x),
                    (new_pic.y + new_corner.y - new_stage.height / 2.0).clamp(0.0, max_y),
                );
                if (next.x - self.offset.x).abs() > 0.01 || (next.y - self.offset.y).abs() > 0.01 {
                    self.offset = next;
                    self.pending_scroll = Some(AbsoluteOffset {
                        x: next.x,
                        y: next.y,
                    });
                }
            }
        }
        self.sync_stage();
    }

    /// Refreshes the editor viewport and stage's scaled display copy without changing zoom or scroll.
    fn sync_stage(&mut self) {
        self.sync_view();
        let scale = self.scale;
        if let Some(e) = &mut self.editor {
            stage_copy(e, scale, &mut self.mips);
        }
    }

    /// Tells the editor which part of the picture the stage shows (its pixel
    /// grid draws only that).
    fn sync_view(&mut self) {
        let (stage, off) = (self.stage(), self.offset);
        if let Some(e) = &mut self.editor {
            let pad = STAGE_PAD + 8.0;
            e.viewport = Some(iced::Rectangle::new(
                Point::new(off.x - pad, off.y - pad),
                Size::new(stage.width + 2.0 * pad, stage.height + BAND + 2.0 * pad),
            ));
        }
    }

    /// The one zoom: `zoom` (display px per image px) kept in the range; with
    /// `round` (a point in the stage, else the stage's centre) the picture point under it stays.
    fn zoom_to(&mut self, zoom: f32, round: Option<Point>) -> Outcome {
        let actual = 1.0 / self.scale;
        let (lo, hi) = (ZOOM_RANGE.start() * actual, ZOOM_RANGE.end() * actual);
        let Some(old) = self.editor.as_ref().map(|e| (e.zoom, e.size())) else {
            return Outcome::None;
        };
        let z = zoom.clamp(lo, hi);
        if let Some(e) = &mut self.editor {
            e.zoom = z;
        }
        self.sync_stage();
        let Some(e) = &self.editor else {
            return Outcome::None;
        };
        let room = self.stage();
        let at = round.unwrap_or(Point::new(room.width / 2.0, room.height / 2.0));
        let ratio = e.zoom / old.0;
        let shown = e.size();
        let max_x = ((room.width - 2.0).max(shown.width) - room.width).max(0.0);
        let max_y = ((room.height - 2.0).max(shown.height) - room.height).max(0.0);
        let offset = (Vector::new(at.x, at.y) + self.offset - stage_corner(room, old.1)) * ratio
            + stage_corner(room, shown)
            - Vector::new(at.x, at.y);
        let offset = Vector::new(offset.x.clamp(0.0, max_x), offset.y.clamp(0.0, max_y));
        self.offset = offset;
        self.pending_scroll = None;
        self.sync_stage();
        Outcome::ScrollTo(AbsoluteOffset {
            x: offset.x,
            y: offset.y,
        })
    }

    /// The zoom as a percentage of 100 %.
    pub(super) fn percent(&self) -> Option<u32> {
        let e = self.editor.as_ref()?;
        Some((e.zoom * self.scale * 100.0).round() as u32)
    }
}

/// The picture's top-left corner inside the scrollable content (centred when smaller than stage).
fn stage_corner(room: Size, shown: Size) -> Vector {
    Vector::new(
        ((room.width - 2.0).max(shown.width) - shown.width) / 2.0,
        ((room.height - 2.0).max(shown.height) - shown.height) / 2.0,
    )
}

/// The zoom that shows `picture` (image px) whole on `stage` (logical px) with
/// room around it, at most 100 % on a screen of `scale`.
fn fit_zoom(picture: Size, stage: Size, scale: f32) -> f32 {
    let room = Size::new(
        (stage.width - 2.0 * STAGE_PAD).max(1.0),
        (stage.height - 2.0 * STAGE_PAD).max(1.0),
    );
    (room.width / picture.width.max(1.0))
        .min(room.height / picture.height.max(1.0))
        .min(1.0 / scale.max(0.1))
}

/// A picture over this many pixels is drawn from a copy of its shown size when shown small. 1 Mpx, not
/// 3 (W107-4): a 1640 x 1270 picture, the size of a typical snip, drew whole through the linear pipeline
/// on every crop frame (release, drag mean 6.1 ms, worst 13.7, against 1.0 with the copy; 4K already had it).
const MIP_MIN_PIXELS: u32 = 1_000_000;

/// The copy of one base picture at about the size the stage shows it, made on a thread.
/// Keyed by (base handle id, baked_gen) so any thread result from before an edit/undo is dropped.
pub(super) struct Mips {
    key: (iced::advanced::image::Id, u64),
    /// The copy and its width, once made.
    ready: Option<(u32, image::Handle)>,
    /// The thread making one: the width and baked_gen it was built for, and its output slot.
    pub(super) pending: Option<(u32, u64, MipSlot)>,
}

/// Where the thread leaves the copy.
type MipSlot = std::sync::Arc<std::sync::Mutex<Option<image::Handle>>>;

/// Sets e.display and e.crisp for the stage's zoom.
/// - Shown at 100 % or >= 200 %: base itself, pixel-for-pixel (Nearest, no copy).
/// - Shown smaller (< 100 %, including Fit and 80-99 % such as 90 %): full box-filtered copy
///   at shown size (Nearest).
/// - Shown between 100 % and 200 %: origin/main smooth Linear path on base (no viewport tile
///   seam during pan).
fn stage_copy(e: &mut Annotator, scale: f32, mips: &mut Option<Mips>) {
    e.display_scale = scale;
    let image::Handle::Rgba {
        id,
        width,
        height,
        pixels,
    } = &e.base
    else {
        (e.display, e.crisp) = (None, false);
        *mips = None;
        return;
    };
    let (id, bw, bh) = (*id, *width, *height);
    let z = e.zoom * scale;
    let want = (bw as f32 * z).round().max(1.0);
    let actual = (want - bw as f32).abs() < 1.0;

    if bw * bh < MIP_MIN_PIXELS || actual || want >= bw as f32 {
        let crisp = actual || z >= 2.0;
        if !actual && !e.base_deltas.is_empty() {
            // Flush deltas for every non-100 % direct-base zoom (150 %, 244 %, 305 %, small
            // pictures) so separate delta textures never land on fractional screen pixels.
            e.flush_base_deltas();
        }
        (e.display, e.crisp) = (None, crisp);
        *mips = None;
        return;
    }

    let want = want as u32;
    let near = |w: u32| w.abs_diff(want) * 50 <= want; // within 2 %
    let key = (id, e.baked_gen);
    if mips.as_ref().is_none_or(|m| m.key != key) {
        // Key changed (edit, undo, redo, or new picture): drop any stale in-flight mip thread.
        let ready = match &e.display {
            Some(h @ image::Handle::Rgba { width: dw, .. }) if near(*dw) => Some((*dw, h.clone())),
            _ => None,
        };
        *mips = Some(Mips {
            key,
            ready,
            pending: None,
        });
    }
    let Some(m) = mips else { return };
    if let Some((pw, pgen, slot)) = &m.pending
        && let Some(done) = slot.lock().ok().and_then(|mut s| s.take())
    {
        if *pgen == e.baked_gen {
            m.ready = Some((*pw, done));
        }
        m.pending = None;
    }
    if m.ready.as_ref().is_none_or(|(w, _)| !near(*w)) && m.pending.is_none() {
        let slot: MipSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
        let (out, px) = (slot.clone(), pixels.clone());
        let deltas = e.base_delta_slices();
        std::thread::spawn(move || {
            let th = ((bh as f32 * want as f32 / bw as f32).round() as u32).max(1);
            let small = annotate::downsample_with_deltas(px.as_ref(), bw, bh, &deltas, want, th);
            if let Ok(mut out) = out.lock() {
                *out = Some(image::Handle::from_rgba(want, th, small));
            }
        });
        m.pending = Some((want, e.baked_gen, slot));
    }
    e.display = m.ready.as_ref().map(|(_, h)| h.clone());
    e.crisp = e.display.is_some();
}

/// The unsaved copy a Tick hands the host.
pub struct KeepCopy {
    /// Taken when it was made: a later discard, or a newer copy, voids the write.
    pub ticket: recovery::Ticket,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub notes: String,
}

/// The pointer's last place over the stage, kept by the stage's own widget with
/// no message per move (`Msg::Pointer` is for a pan only).
#[derive(Clone, Default)]
pub struct PointerCell(std::sync::Arc<std::sync::Mutex<Point>>);

impl PointerCell {
    pub fn get(&self) -> Point {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set(&self, p: Point) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = p;
    }
}

/// The right-click menu keeps this far from the window's edge.
const EDGE: f32 = 8.0;

/// The right-click menu: where (window px) and its rows.
pub(super) struct Context {
    pub at: Point,
    pub entries: Vec<editing::Entry>,
}

/// The export choices Settings > Studio starts the Export sheet with.
fn export_defaults() -> export::Picture {
    let prefs = crate::shared::config::current();
    export::Picture {
        format: match prefs.studio.format {
            crate::shared::config::Format::Png => export::Format::Png,
            crate::shared::config::Format::Jpeg => export::Format::Jpg,
        },
        quality: prefs.studio.quality,
        max_edge: prefs.snip.max_edge,
    }
}

/// A copy kept after a crash (only with the switch on), for the restore sheet.
fn pending_copy() -> Option<Restorable> {
    if !crate::shared::config::current().studio.keep_unsaved {
        return None;
    }
    let p = recovery::pending()?;
    let size = ::image::load_from_memory(&p.png)
        .ok()
        .map(|i| (i.width(), i.height()))?;
    Some(Restorable {
        name: crate::t!(st_unsaved_picture).to_string(),
        size,
        png: p.png,
        notes: p.notes,
    })
}

/// What the pill says when the canvas would not grow.
fn grow_words(why: Option<annotate::GrowRefusal>) -> String {
    match why {
        Some(annotate::GrowRefusal::TooLarge) => crate::t!(st_grow_far),
        _ => crate::t!(st_grow_big),
    }
    .to_string()
}

/// The window's title: the document's name, else "Untitled", then the app's.
fn title_of(name: Option<&str>) -> String {
    format!(
        "{} - {}",
        name.unwrap_or(crate::t!(st_untitled)),
        crate::t!(st_title)
    )
}

fn file_name(path: &std::path::Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// "2.1 MB · 3 min ago" for a recent file (blank where the OS says nothing).
pub fn recent_line(path: &std::path::Path) -> String {
    use crate::shared::app_language::{S, fmt, tr};
    let Ok(meta) = std::fs::metadata(path) else {
        return String::new();
    };
    let size = match meta.len() {
        n if n >= 1_048_576 => format!("{:.1} MB", n as f64 / 1_048_576.0),
        n => format!("{} KB", n.div_ceil(1024)),
    };
    let secs = meta
        .modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map_or(0, |d| d.as_secs());
    let when = match secs {
        0..=59 => tr(S::st_ago_now).to_string(),
        60..=3599 => fmt(S::st_ago_min, &[&(secs / 60)]),
        3600..=86399 => fmt(S::st_ago_h, &[&(secs / 3600)]),
        _ => fmt(S::st_ago_d, &[&(secs / 86400)]),
    };
    [size, when].join(" · ")
}

#[cfg(test)]
mod tests;

impl Studio {
    /// Recent holds unsaved recordings (read once when the Studio opens).
    pub fn set_recordings(&mut self, there: bool) {
        self.recordings = there;
    }
}
