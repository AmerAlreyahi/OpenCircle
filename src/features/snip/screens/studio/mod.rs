//! Snip Studio (v0.4.0, STUDIO_V2_SPEC): a normal window for longer work on a
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
mod view;

use std::path::PathBuf;

use iced::widget::image;
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Point, Size, Vector};

use super::steps::{self, Released, Reorder};
use crate::features::snip::annotate::{self, Annotator, Image, Output};
use crate::features::snip::export;
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
/// Room kept around a fitted picture.
const STAGE_PAD: f32 = 24.0;
/// One zoom step (Ctrl+= / Ctrl+-) and the zoom's range, as a share of 100 %.
const ZOOM_STEP: f32 = 1.25;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.05..=8.0;
/// The canvas's scroll area (`operation::scroll_to`).
pub const CANVAS: &str = "studio-canvas";

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
        editor.own_image = false;
        (editor.scroll, editor.capturing, editor.held) = (false, false, false);
        (editor.steps, editor.updating, editor.focus, editor.menu) = (None, None, None, None);
        editor.edges.clear();
        // what came from Snip with no drawing is nothing to lose
        self.saved = (editor.doc.items.is_empty()).then(|| editor.doc.state());
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
            )
        {
            return Outcome::None;
        }
        match msg {
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
            Msg::DontSave => self.proceed(),
            Msg::Cancel => {
                self.asking = None;
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
            + (self.panels.options as u8 as f32) * (PANEL + 1.0);
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
