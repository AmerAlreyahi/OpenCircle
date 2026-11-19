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
mod cut;
mod edit_text;
pub(crate) mod editing;
pub(crate) mod keys;
mod library;
mod menu_bar;
mod options_bar;
mod properties;
pub mod recovery;
mod sheets;
mod status_bar;
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
#[cfg(test)]
pub use view::GLYPHS;
pub use view::view;

/// The window's opening and smallest size (logical px; spec §2).
pub const SIZE: Size = Size::new(1280.0, 800.0);
pub const MIN: Size = Size::new(960.0, 600.0);

// The regions around the canvas (logical px): the stage is what they leave.
const HEADER: f32 = 41.0; // 40 + the hairline
const RAIL: f32 = 52.0;
const OPTIONS: f32 = 48.0;
/// Below this width both panels fold and the Library opens over the stage.
const NARROW: f32 = 1100.0;
const STATUS: f32 = 44.0;
const LIBRARY: f32 = 240.0;
const PANEL: f32 = 260.0;
/// Arabic's picture panel is wider (its names are longer; spec §2).
const PANEL_AR: f32 = 288.0;

/// The picture panel's width in the app's language.
fn panel_width() -> f32 {
    match crate::shared::app_language::rtl() {
        true => PANEL_AR,
        false => PANEL,
    }
}
/// Room kept around a fitted picture.
const STAGE_PAD: f32 = 24.0;
/// One zoom step (Ctrl+= / Ctrl+-) and the zoom's range, as a share of 100 %.
const ZOOM_STEP: f32 = 1.25;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.05..=8.0;
/// The canvas's scroll area (`operation::scroll_to`).
pub const CANVAS: &str = "studio-canvas";
/// Edit text's inline field.
pub const FIELD: &str = "studio-edit-field";

/// The Steps grid: four to a row, the cell and the gap round it.
const STEP_COLS: usize = 4;
const STEP_CELL: Size = crate::shared::kit::thumb::MINI;
const STEP_GAP: f32 = 8.0;

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

#[derive(Debug, Clone)]
pub enum Msg {
    Editor(annotate::Message),
    Caption(Caption),
    /// The side panels' toggles (Ctrl+[ / Ctrl+] / Ctrl+\).
    Library,
    Panel,
    Both,
    ZoomIn,
    ZoomOut,
    /// Ctrl+wheel over the stage: `notches` steps (up = in) round the pointer
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
    OpenRecent(PathBuf),
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
    /// A menu opens (`Some`) or closes; a row picked; the keyboard on the bar.
    Menu(Option<Menu>),
    MenuPick(Menu, usize),
    MenuKey(Nav),
    /// A Library section folds / unfolds (index in `library`).
    Section(usize),
    /// The Library over the stage (narrow windows).
    Overlay,
    /// The shortcuts sheet.
    Shortcuts(bool),
    /// The shortcuts sheet's Edit shortcuts… (Settings); the restore sheet's Later.
    EditShortcuts,
    Later,
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
    /// The Objects list: a press on a row (display slot, top = front), the
    /// pointer over the list (with the rows' count), the press ends, the eye.
    ObjPress(usize),
    ObjMove(Point, usize),
    ObjRelease,
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
    /// File > New snip, File > Settings.
    NewSnip,
    Settings,
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
    /// Read this file (a dropped or recent one) and `open` it.
    Read(PathBuf),
    /// Ctrl+V: a picture on the clipboard opens; else the editor pastes its text
    /// at that point (`paste_text`).
    Paste(Option<Point>),
    Copy(Crop),
    /// Save; on success the host calls `saved` (and, after the question, `proceed`).
    Save(Crop),
    SaveThen(Crop),
    Pin(Crop),
    /// The panels changed: remember them.
    Panels(StudioConfig),
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
    /// Write the unsaved copy (the host encodes and writes it off the UI thread).
    KeepCopy(Box<KeepCopy>),
    /// File > New snip: the Snip capture starts.
    NewSnip,
    /// Settings > Studio changed from the View menu.
    Prefs(crate::shared::config::StudioPrefs),
    /// Recent > Clear.
    ClearRecent,
    /// File > Settings…
    Settings,
}

pub struct Studio {
    pub(super) editor: Option<Annotator>,
    /// The zoom the user picked (display px per image px); `None` = fit.
    zoom: Option<f32>,
    scale: f32,
    window: Size,
    pub(super) panels: StudioConfig,
    /// The history's state (`Document::state`) when last saved: other = unsaved;
    /// `None` = never saved with what it holds.
    saved: Option<u64>,
    asking: Option<After>,
    pub(super) recent: Vec<(PathBuf, image::Handle)>,
    pub(super) hovered: bool,
    /// The stage's scroll offset, and Ctrl held (see `Msg::Scrolled`).
    offset: Vector,
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
    pub(super) shut: [bool; 3],
    pub(super) overlay: bool,
    pub(super) shortcuts: bool,
    /// The right-click menu, and the pointer's last place on the stage.
    pub(super) context: Option<Context>,
    pub(super) pointer: PointerCell,
    /// Alt went down with nothing since: its release opens the menu bar.
    alt_clean: bool,
    /// A kept copy of this picture is on disk (a clean state removes it).
    copy_on_disk: bool,
    /// A restore that waits for the unsaved question (Cancel gives the sheet back).
    restoring: Option<Restorable>,
    /// Properties' X / Y / W / H while typed (committed on Enter or the next action).
    pub(super) bounds_draft: Option<[String; 4]>,
    /// A picture was opened (Reopen last waits for none).
    opened: bool,
    pub(super) lock: bool,
    /// Space is held (the stage pans), the pointer where a pan last was, the
    /// open second menu, the document's name and the name of the next picture.
    pub(super) space: bool,
    pub(super) shift: bool,
    pub(super) obj_pick: Reorder,
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
    pub fn new(panels: StudioConfig) -> Self {
        Studio {
            editor: None,
            zoom: None,
            scale: 1.0,
            window: SIZE,
            panels,
            saved: Some(0),
            asking: None,
            recent: Vec::new(),
            hovered: false,
            offset: Vector::ZERO,
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
            shut: [false; 3],
            overlay: false,
            shortcuts: false,
            context: None,
            pointer: PointerCell::default(),
            alt_clean: false,
            copy_on_disk: false,
            restoring: None,
            bounds_draft: None,
            opened: false,
            lock: true,
            space: false,
            shift: false,
            obj_pick: Reorder::default(),
            quiet: recovery::Debounce::default(),
            pan: None,
            sub: None,
            snap: crate::shared::config::current().studio.snap,
            grid: crate::shared::config::current().studio.pixel_grid,
            name: None,
            next_name: None,
            title: title_of(None),
            recent_info: Vec::new(),
            restorable: pending_copy(),
            later: false,
            pick: Reorder::default(),
            step_hover: None,
            export: export_defaults(),
            exporting: false,
            walk: Walk {
                edits: false,
                tenths: 20,
                how: export::Walkthrough {
                    movie: export::Movie::from_index(panels.walk_format),
                    ..export::Walkthrough::default()
                },
                busy: None,
            },
            walk_tab: false,
        }
    }

    /// Opens `picture` (fitted); with unsaved drawings it asks first.
    pub fn open(&mut self, picture: Picture) {
        if self.asks() {
            self.asking = Some(After::Open(picture));
            return;
        }
        let mut editor = match picture {
            Picture::Crop(crop) => Annotator::new(
                Image {
                    width: crop.width,
                    height: crop.height,
                    rgba: crop.rgba,
                },
                1.0,
            ),
            Picture::Editor(editor) => *editor,
        };
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
        self.transparent = cut::has_alpha(&editor.doc.image.rgba);
        // (one counter for every Studio: an answer for a closed one never fits a new one)
        self.epoch = NEXT_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let prefs = crate::shared::config::current().studio;
        editor.copy_picture_when_empty = prefs.ctrl_c_picture;
        (self.snap, self.grid) = (prefs.snap, prefs.pixel_grid);
        (editor.snap, editor.grid) = (self.snap, self.grid);
        self.zoom = match prefs.zoom_on_open {
            crate::shared::config::StudioZoom::Actual => Some(1.0 / self.scale),
            crate::shared::config::StudioZoom::Fit => None,
        };
        self.name = self.next_name.take();
        self.title = title_of(self.name.as_deref());
        self.editor = Some(editor);
        self.fit();
        self.opened = true;
        if let Some(e) = &mut self.editor {
            if let Some(r) = self.restoring.take() {
                e.restore_annotations(&r.notes);
                self.copy_on_disk = false;
                recovery::discard();
            }
            e.refresh();
        }
    }

    /// The next picture opened is this file's (its name shows in the title).
    pub fn next_name(&mut self, path: &std::path::Path) {
        self.next_name = file_name(path);
    }

    /// Unsaved drawings: closing or opening another picture asks first.
    /// Settings > Studio > "Ask before closing with unsaved changes".
    fn asks(&self) -> bool {
        self.dirty() && crate::shared::config::current().studio.ask_unsaved
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
    pub fn set_cut(&mut self, epoch: u32, done: cut::Done) -> Outcome {
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
    pub fn close_requested(&mut self) -> Outcome {
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
    pub fn quit_requested(&mut self) -> Outcome {
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
        if let Some(e) = &self.editor {
            self.saved = Some(e.doc.state());
        }
    }

    /// After the question's Save landed (or Don't save): what waited goes on.
    pub fn proceed(&mut self) -> Outcome {
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
        self.fit();
        Outcome::None
    }

    /// Ctrl+V found no picture: the editor's own paste (text at `at`).
    pub fn paste_text(&mut self, at: Option<Point>) -> Outcome {
        self.on_editor(annotate::Message::Paste(at))
    }

    pub fn update(&mut self, msg: Msg) -> Outcome {
        let quiet_msg = matches!(
            msg,
            Msg::AltDown
                | Msg::AltUp
                | Msg::Pointer(_)
                | Msg::Mods(..)
                | Msg::Tick
                | Msg::Focus(_)
                | Msg::Scrolled(_)
                | Msg::Resized(_)
                | Msg::Scale(_)
        );
        if !quiet_msg {
            self.alt_clean = false;
        }
        // a typed X / Y / W / H is one edit, made when the user moves on
        if !matches!(msg, Msg::BoundsEdit(..) | Msg::BoundsCommit) && !quiet_msg {
            let out = self.commit_bounds();
            if !matches!(out, Outcome::None) {
                return out;
            }
        }
        // the question blocks the picture: nothing changes it until answered
        if self.asking()
            && matches!(
                msg,
                Msg::Editor(_)
                    | Msg::Open
                    | Msg::Paste
                    | Msg::Dropped(_)
                    | Msg::OpenRecent(_)
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
            Msg::Editor(annotate::Message::Paste(at))
                if self.editor.as_ref().is_some_and(|e| !e.typing()) =>
            {
                Outcome::Paste(at)
            }
            Msg::Editor(m) => self.on_editor(m),
            Msg::Caption(Caption::Close) => self.close_requested(),
            Msg::Caption(c) => Outcome::Caption(c),
            // narrow: the Library is an overlay, the panels' choices stay as saved
            Msg::Library | Msg::Panel | Msg::Both if self.narrow() => {
                self.overlay = !self.overlay;
                Outcome::None
            }
            Msg::Library | Msg::Panel | Msg::Both => {
                let p = &mut self.panels;
                match msg {
                    Msg::Library => p.library = !p.library,
                    Msg::Panel => p.options = !p.options,
                    // both: hidden if either shows, else both shown
                    _ => {
                        let show = !(p.library || p.options);
                        (p.library, p.options) = (show, show);
                    }
                }
                self.fit();
                Outcome::Panels(self.panels)
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
                let Some(e) = &self.editor else {
                    return Outcome::None;
                };
                self.zoom_to(e.zoom * ZOOM_STEP.powf(notches), Some(at))
            }
            Msg::Scrolled(offset) => {
                self.offset = offset;
                Outcome::None
            }
            Msg::Ctrl(down) => {
                self.ctrl = down;
                Outcome::None
            }
            Msg::Fit => {
                self.zoom = None;
                self.fit();
                Outcome::None
            }
            Msg::Open => Outcome::PickFile,
            Msg::Paste => Outcome::Paste(None),
            Msg::Hovered(on) => {
                self.hovered = on;
                Outcome::None
            }
            Msg::Dropped(path) => {
                self.hovered = false;
                self.next_name = file_name(&path);
                Outcome::Read(path)
            }
            Msg::OpenRecent(path) => {
                self.next_name = file_name(&path);
                Outcome::Read(path)
            }
            Msg::Recent(recent, info) => {
                self.recent_info = info;
                self.recent = recent;
                // Settings > Studio > Reopen the last picture: the newest, when none is open
                let first = self.recent.first().map(|(p, _)| p.clone());
                match (first, crate::shared::config::current().studio.reopen_last) {
                    (Some(p), true) if self.editor.is_none() && !self.opened => {
                        self.next_name = file_name(&p);
                        Outcome::Read(p)
                    }
                    _ => Outcome::None,
                }
            }
            Msg::Scale(s) => {
                self.scale = s.max(0.1);
                self.fit();
                Outcome::None
            }
            Msg::Resized(size) => {
                self.window = size;
                self.fit();
                Outcome::None
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
            Msg::Cancel => {
                // Esc ladder: menu, sheet, overlay, the question, Edit text
                let closed = self.sub.take().is_some()
                    || self.context.take().is_some()
                    || self.menu.take().is_some()
                    || std::mem::take(&mut self.shortcuts)
                    || std::mem::take(&mut self.overlay);
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
                self.pan = Some(p);
                let off = Vector::new(
                    (self.offset.x - (p.x - last.x)).max(0.0),
                    (self.offset.y - (p.y - last.y)).max(0.0),
                );
                self.offset = off;
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
            Msg::ObjPress(slot) => {
                self.obj_pick.press(slot);
                Outcome::None
            }
            Msg::ObjMove(at, n) => {
                self.obj_pick.moved(at, 0.0, library::OBJECT_ROW, n);
                Outcome::None
            }
            Msg::ObjRelease => {
                let n = self.editor.as_ref().map_or(0, |e| e.doc.items.len());
                // the list shows the front first: slot k is item n - 1 - k
                match self.obj_pick.release() {
                    Some(Released::Click(slot)) if slot < n => self.update(Msg::Editor(
                        annotate::Message::SelectIndex(n - 1 - slot, self.shift),
                    )),
                    Some(Released::Moved(from, to)) if from < n && to < n => {
                        self.update(Msg::Editor(annotate::Message::Reorder {
                            from: n - 1 - from,
                            to: n - 1 - to,
                        }))
                    }
                    _ => Outcome::None,
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
                // the stage's left edge: mirrored, the properties panel sits there
                let left = match crate::shared::app_language::rtl() {
                    true => (self.panel_shown() as u8 as f32) * (panel_width() + 1.0),
                    false => (self.library_shown() as u8 as f32) * (LIBRARY + 1.0) + RAIL + 2.0,
                };
                let origin = Point::new(left, HEADER + OPTIONS + 1.0);
                let seps = entries.iter().filter(|r| r.divider).count();
                let menu = Size::new(
                    crate::shared::kit::option_list::MENU_WIDTH,
                    crate::shared::kit::option_list::menu_height(entries.len(), seps),
                );
                let p = self.pointer.get();
                let at = Point::new(
                    (origin.x + p.x)
                        .min(self.window.width - menu.width - EDGE)
                        .max(0.0),
                    (origin.y + p.y)
                        .min(self.window.height - menu.height - EDGE)
                        .max(0.0),
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
                    recovery::discard();
                    return Outcome::Say(crate::t!(st_unreadable).to_string());
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
                self.copy_on_disk = false;
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
            Msg::MenuKey(key) => {
                let (menu, pick) = menu_bar::nav(self, key);
                self.menu = menu;
                match pick {
                    Some(m) => self.update(m),
                    None => Outcome::None,
                }
            }
            Msg::Section(i) => {
                if let Some(x) = self.shut.get_mut(i) {
                    *x = !*x;
                }
                Outcome::None
            }
            Msg::Overlay => {
                self.overlay = !self.overlay;
                Outcome::None
            }
            Msg::Shortcuts(on) => {
                self.shortcuts = on;
                Outcome::None
            }
            Msg::EditShortcuts => {
                self.shortcuts = false;
                Outcome::Settings
            }
            Msg::Later => {
                self.later = true;
                Outcome::None
            }
            Msg::NewSnip => Outcome::NewSnip,
            Msg::Settings => Outcome::Settings,
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
                Outcome::Panels(self.panels)
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
        let history = matches!(m, annotate::Message::Undo | annotate::Message::Redo);
        let out = e.update(m);
        if history {
            self.transparent = cut::has_alpha(&e.doc.image.rgba);
        }
        self.fit(); // a crop changes what is shown
        self.keep_unsaved();
        match out {
            Some(Output::Copy(crop)) => Outcome::Copy(crop),
            Some(Output::Save(crop)) => Outcome::Save(crop),
            Some(Output::Pin(crop)) => Outcome::Pin(crop),
            // Esc never closes the Studio (Alt+F4 / the x does); nothing else
            // the widget's toolbar asks for is on the Studio
            _ => Outcome::None,
        }
    }

    /// Narrow windows fold both panels (the Library opens over the stage).
    pub(super) fn narrow(&self) -> bool {
        self.window.width < NARROW
    }

    pub(super) fn library_shown(&self) -> bool {
        self.panels.library && !self.narrow()
    }

    pub(super) fn panel_shown(&self) -> bool {
        self.panels.options && !self.narrow()
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
        self.copy_on_disk = true;
        Outcome::KeepCopy(Box::new(KeepCopy {
            generation: recovery::generation(),
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
        self.copy_on_disk = false;
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

    /// The picture point under `at` (a point of the stage): the scroll and the
    /// centring undone, the zoom divided out, the view's corner added.
    pub(super) fn image_point(&self, at: Point) -> Point {
        let Some(e) = &self.editor else {
            return at;
        };
        let (room, shown) = (self.stage(), e.size());
        let corner = Vector::new(
            ((room.width - 2.0).max(shown.width) - shown.width) / 2.0,
            ((room.height - 2.0).max(shown.height) - shown.height) / 2.0,
        );
        let d = (Vector::new(at.x, at.y) + self.offset - corner) * (1.0 / e.zoom);
        let origin = e.doc.view().position();
        Point::new(origin.x + d.x, origin.y + d.y)
    }

    /// The stage: the window less the header, the rail, the bars and the shown panels.
    pub(super) fn stage(&self) -> Size {
        // each region with its 1 px hairline, and the window's 1 px rim all round
        let side = (self.library_shown() as u8 as f32) * (LIBRARY + 1.0)
            + (self.panel_shown() as u8 as f32) * (panel_width() + 1.0);
        Size::new(
            (self.window.width - 2.0 - (RAIL + 1.0) - side).max(1.0),
            (self.window.height - 2.0 - HEADER - (OPTIONS + 1.0) - (STATUS + 1.0)).max(1.0),
        )
    }

    /// The editor's zoom: the user's, else the picture fitted to the stage,
    /// never above 100 % (one picture px per screen px).
    fn fit(&mut self) {
        let (stage, scale, zoom) = (self.stage(), self.scale, self.zoom);
        if let Some(e) = &mut self.editor {
            e.zoom = zoom.unwrap_or_else(|| fit_zoom(e.doc.view().size(), stage, scale));
        }
    }

    /// The one zoom: `zoom` (display px per image px) kept in the range; with
    /// `round` (a point in the stage) the picture point under it stays there.
    fn zoom_to(&mut self, zoom: f32, round: Option<Point>) -> Outcome {
        let actual = 1.0 / self.scale;
        let (lo, hi) = (ZOOM_RANGE.start() * actual, ZOOM_RANGE.end() * actual);
        let Some(old) = self.editor.as_ref().map(|e| (e.zoom, e.size())) else {
            return Outcome::None;
        };
        self.zoom = Some(zoom.clamp(lo, hi));
        self.fit();
        let (Some(at), Some(e)) = (round, &self.editor) else {
            return Outcome::None;
        };
        // the picture's corner in the scrolled content: centred when it is smaller
        let room = self.stage();
        let corner = |shown: Size| {
            Vector::new(
                ((room.width - 2.0).max(shown.width) - shown.width) / 2.0,
                ((room.height - 2.0).max(shown.height) - shown.height) / 2.0,
            )
        };
        let ratio = e.zoom / old.0;
        let offset = (Vector::new(at.x, at.y) + self.offset - corner(old.1)) * ratio
            + corner(e.size())
            - Vector::new(at.x, at.y);
        let offset = Vector::new(offset.x.max(0.0), offset.y.max(0.0));
        self.offset = offset;
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

/// The unsaved copy a Tick hands the host.
pub struct KeepCopy {
    /// `recovery::generation()` when it was made: a later discard voids the write.
    pub generation: u32,
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
    format!("{size} · {when}")
}

#[cfg(test)]
mod tests;
