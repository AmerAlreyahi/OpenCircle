//! OpenCircle Studio (v0.4.0, STUDIO_V2_SPEC): a normal window for longer work on a
//! picture. The SAME editor as the Snip widget (`annotate::Annotator`), shown
//! another way: every tool on a rail, the tool's options always visible in a bar
//! over the canvas, the canvas with zoom, Save / Copy in the status bar. Side
//! panels: the library (Recent captures) and the picture panel, each hidden by
//! its toggle or Ctrl+[ / Ctrl+] (Ctrl+\ both), remembered in `[snip.studio]`.
//! One Studio at a time: the host (`app::windows`) brings it forward and
//! `open`s the next picture in it; unsaved drawings ask first.
mod edit_text;
mod view;

use std::path::PathBuf;

use iced::widget::image;
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Point, Size, Vector};

use crate::features::snip::annotate::{self, Annotator, Image, Output};
use crate::features::snip::snip_more::{self as more, Row};
pub use crate::features::text_reading::model_download::Fetching;
use crate::features::translate::{self, Line};
use crate::shared::config::StudioConfig;
use crate::shared::kit::header::Caption;
use crate::shared::picture::Crop;

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

/// What the Studio opens.
pub enum Picture {
    /// A file, the clipboard or a recent capture.
    Crop(Crop),
    /// The Snip widget's editor, its drawings still editable and its undo kept.
    Editor(Box<Annotator>),
}

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
        editor.own_image = false;
        (editor.scroll, editor.capturing, editor.held) = (false, false, false);
        (editor.steps, editor.updating, editor.focus, editor.menu) = (None, None, None, None);
        editor.edges.clear();
        // what came from Snip with no drawing is nothing to lose
        self.saved = (editor.doc.items.is_empty()).then(|| editor.doc.state());
        (self.original, self.busy, self.card, self.edit) = (None, None, None, None);
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

    /// Edit text stops (the reader's models are missing: the card asks).
    pub fn cancel_edit(&mut self) {
        self.edit = None;
    }

    /// The lines Edit text read (for the picture `epoch`), as the reader gave
    /// them in `read_picture`'s px; or why none (a pill).
    pub fn set_lines(&mut self, epoch: u32, read: Result<Vec<Line>, String>) -> Outcome {
        let (Some(e), Some(session)) = (self.editor.as_ref(), self.edit.as_mut()) else {
            return Outcome::None;
        };
        if epoch != self.epoch {
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
        let (w, h) = (e.doc.image.width, e.doc.image.height);
        let mut pixels = e.doc.image.rgba.clone();
        let drawn = translate::replace_line(&mut pixels, w, h, line, text, session.tweak());
        if drawn.drawn == 0 {
            // (stays on the line: the text can be changed)
            return Outcome::Say(crate::t!(st_edit_failed).to_string());
        }
        e.doc.set_pixels(&pixels);
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
                    | Msg::Download
            )
        {
            return Outcome::None;
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
            Msg::EditText
                if self.editor.is_none()
                    || self.card.is_some()
                    || self.original.is_some()
                    || self.edit.is_some() =>
            {
                Outcome::None
            }
            Msg::EditText => {
                self.edit = Some(edit_text::Session::new());
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
                let m = translate::matched(&d.rgba, d.width, d.height, line);
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
                self.asking = None;
                self.edit = None;
                Outcome::None
            }
            Msg::SaveFirst => match &mut self.editor {
                Some(e) => Outcome::SaveThen(annotate::render(&e.doc)),
                None => self.proceed(),
            },
        }
    }

    fn on_editor(&mut self, m: annotate::Message) -> Outcome {
        let Some(e) = &mut self.editor else {
            return Outcome::None;
        };
        let out = e.update(m);
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
