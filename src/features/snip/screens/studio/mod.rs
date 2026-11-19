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
pub(crate) mod editing;
mod edit_text;
mod view;

use std::path::PathBuf;

use iced::widget::image;
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Point, Size, Vector};

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
#[cfg(test)]
pub use view::GLYPHS;
pub use view::view;

/// The window's opening and smallest size (logical px; spec §2).
pub const SIZE: Size = Size::new(1280.0, 800.0);
pub const MIN: Size = Size::new(960.0, 600.0);

// The regions around the canvas (logical px): the stage is what they leave.
const HEADER: f32 = 41.0; // 40 + the hairline
const RAIL: f32 = 52.0;
const OPTIONS: f32 = 76.0;
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

/// One step's row in the Steps list: its thumbnail and the gap under it.
const STEP_PITCH: f32 =
    crate::shared::kit::thumb::WELL.height + crate::shared::kit::tokens::space::GAP;

/// The library's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Recent,
    Steps,
}

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
    Recent(Vec<(PathBuf, image::Handle)>),
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
    Tab(Tab),
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
    /// The library's tab the user picked (`None`: Steps while there are some).
    pub(super) tab: Option<Tab>,
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
            tab: None,
            pick: Reorder::default(),
            step_hover: None,
            export: export::Picture::default(),
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
        if self.dirty() {
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
        self.zoom = None;
        self.editor = Some(editor);
        self.fit();
        if let Some(e) = &mut self.editor {
            e.refresh();
        }
    }

    /// Unsaved drawings: closing or opening another picture asks first.
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
        match self.dirty() {
            true => {
                self.asking = Some(After::Close);
                Outcome::None
            }
            false => Outcome::Close,
        }
    }

    /// The app is quitting: unsaved drawings ask first (`Outcome::Quit` after).
    pub fn quit_requested(&mut self) -> Outcome {
        match self.dirty() {
            true => {
                self.asking = Some(After::Quit);
                Outcome::None
            }
            false => Outcome::Quit,
        }
    }

    /// The last Save landed: what is shown now is saved.
    pub fn saved(&mut self) {
        if let Some(e) = &self.editor {
            self.saved = Some(e.doc.state());
        }
    }

    /// After the question's Save landed (or Don't save): what waited goes on.
    pub fn proceed(&mut self) -> Outcome {
        match self.asking.take() {
            Some(After::Close) => Outcome::Close,
            Some(After::Quit) => {
                self.saved = self.editor.as_ref().map(|e| e.doc.state()); // nothing left to ask
                Outcome::Quit
            }
            Some(After::Open(picture)) => {
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
                Outcome::Read(path)
            }
            Msg::OpenRecent(path) => Outcome::Read(path),
            Msg::Recent(recent) => {
                self.recent = recent;
                Outcome::None
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
                // Esc on the unsaved question closes only the question
                if self.asking.take().is_none() {
                    self.edit = None;
                }
                Outcome::None
            }
            Msg::SaveFirst => match &mut self.editor {
                Some(e) => Outcome::SaveThen(annotate::render(&e.doc)),
                None => self.proceed(),
            },
            Msg::Tab(tab) => {
                self.tab = Some(tab);
                Outcome::None
            }
            Msg::StepPress(i) => {
                self.pick.press(i);
                Outcome::None
            }
            Msg::StepMove(at, n) => {
                self.pick.moved(at, 0.0, STEP_PITCH, n);
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
        match out {
            Some(Output::Copy(crop)) => Outcome::Copy(crop),
            Some(Output::Save(crop)) => Outcome::Save(crop),
            Some(Output::Pin(crop)) => Outcome::Pin(crop),
            // Esc never closes the Studio (Alt+F4 / the x does); nothing else
            // the widget's toolbar asks for is on the Studio
            _ => Outcome::None,
        }
    }

    /// The stage: the window less the header, the rail, the bars and the shown panels.
    pub(super) fn stage(&self) -> Size {
        // each region with its 1 px hairline, and the window's 1 px rim all round
        let side = (self.panels.library as u8 as f32) * (LIBRARY + 1.0)
            + (self.panels.options as u8 as f32) * (panel_width() + 1.0);
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

#[cfg(test)]
mod tests;
