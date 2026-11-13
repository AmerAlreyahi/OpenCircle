//! The annotation editor: ONE component behind both entry points, Edit in place
//! (the overlay's action bar turns into its toolbar) and Snip mode.
//!
//! Model: `Document` (image, items, selection, crop, undo stack), flattened by the
//! pure `render`. UI: `Annotator` = the image with a canvas over it (input + selection
//! chrome) and the floating toolbar. One file per tool.
//!
//! Smooth on big crops: the flattened document (`base`) is cached and repainted
//! once per change; while something is drawn, moved or typed, only its own
//! region is repainted (`live`) and shown over the base.
pub(crate) mod area;
mod arrow;
pub(crate) mod blur;
mod callout;
mod chrome;
mod crop;
mod document;
pub mod dragout;
mod highlighter;
mod input;
mod label;
mod legend;
mod lines;
mod magnifier;
mod pen;
mod pointer;
mod select;
mod shape;
mod spotlight;
mod step;
pub mod style;
/// Pasted text as a text item takes it: Windows line ends made `\n`, other
/// control characters (tabs too) dropped, trailing blank lines off, at most
/// `PASTE_MAX` characters.
fn clean_paste(t: &str) -> String {
    const PASTE_MAX: usize = 2000;
    let t: String = (t.replace("\r\n", "\n").chars())
        .filter(|c| *c == '\n' || !c.is_control())
        .take(PASTE_MAX)
        .collect();
    t.trim_end().to_string()
}

#[cfg(test)]
mod tests;
mod text;

pub use document::rounded as rounded_path;
pub use document::{
    Document, Edit, Image, Item, Shape, clean, hidden, hidden_in_clean, render, span,
};
pub use style::PRESETS;

/// Steps the Steps strip holds at most (open point 3: 12).
pub const STEPS_MAX: usize = 12;
pub use text::{Back as TextBack, Font as TextFont, Style as TextStyle, free_glyphs};

/// The numbered step tool's badge (a `colour` disc of `radius` with `number`
/// on it) drawn on `pm` centred at `at` (the Steps strip's Copy all image).
pub fn badge(
    pm: tiny_skia::Pixmap,
    at: Point,
    radius: f32,
    number: u32,
    colour: [u8; 4],
) -> tiny_skia::Pixmap {
    let mut layer = document::Layer {
        pm,
        origin: Point::ORIGIN,
    };
    step::paint(&mut layer, at, radius, number, colour);
    layer.pm
}

use crate::shared::app_language::S;
use iced::widget::image;
use iced::{Point, Rectangle, Size, Vector};

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::actions::Crop;
use crate::shared::config::{EditorConfig, Rgb};
use document::int_rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Select,
    Crop,
    Rect,
    Ellipse,
    Arrow,
    Line,
    /// An arrow with a label on its tail.
    ArrowLabel,
    Pen,
    Highlighter,
    Text,
    /// Text on a rounded fill.
    Label,
    Callout,
    Step,
    /// A ring with an arrow: a step without a number.
    Pointer,
    /// The soft blur brush.
    Blur,
    /// A solid box.
    Redact,
    Spotlight,
    Magnifier,
}

pub struct ToolInfo {
    pub tool: Tool,
    /// Lucide glyph (listed in `assets/lucide/subset.sh`).
    pub icon: char,
    /// Its name (the string table); the tooltip is `Name (K)`.
    pub name: S,
    /// Its key (no Ctrl) while the editor is open.
    pub key: char,
    /// The toolbar group (button) it is in.
    pub group: Group,
    /// The tool it is an option of (an arrow with a label: Arrow's "Label"),
    /// else itself (a tool of its group's panel).
    pub slot: Tool,
}

/// The grouped toolbar's tool buttons (owner approved, 2026-11-02): each
/// shows its last-used tool; a click on the active one opens its panel (the
/// group's tools, then the tool's options).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Select,
    Shapes,
    Draw,
    Text,
    Hide,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::Select,
        Group::Shapes,
        Group::Draw,
        Group::Text,
        Group::Hide,
    ];

    /// Each group's tool before one was used.
    const FIRST: [Tool; 5] = [Tool::Select, Tool::Arrow, Tool::Pen, Tool::Text, Tool::Blur];

    /// Its name (Select and Text are their tool's own: one tooltip, not two).
    pub fn name(self) -> S {
        [
            S::tl_select,
            S::grp_shapes,
            S::grp_draw,
            S::tl_text,
            S::grp_hide,
        ][self as usize]
    }

    /// The tools of its panel's top row (options such as Label are not
    /// tools there; Crop has no button: the area's handles crop).
    pub fn tools(self) -> impl Iterator<Item = &'static ToolInfo> {
        (TOOLS.iter()).filter(move |t| t.group == self && t.slot == t.tool && t.tool != Tool::Crop)
    }

    /// Its button opens a panel (Select has nothing to choose).
    pub fn has_panel(self) -> bool {
        self != Group::Select
    }

    /// `tools` is one tool of each group, in order (a remembered `group_tools`).
    pub fn valid(tools: &[Tool]) -> bool {
        tools.len() == Group::ALL.len()
            && (tools.iter().zip(Group::ALL)).all(|(&t, g)| t != Tool::Crop && info(t).group == g)
    }
}

/// `tool`'s shapes take a Fill (a tint inside): rectangles and ellipses.
pub fn fills_inside(tool: Tool) -> bool {
    matches!(tool, Tool::Rect | Tool::Ellipse)
}

/// Every tool, in toolbar order (its options right after it).
pub const TOOLS: &[ToolInfo] = &[
    tool(Tool::Select, '\u{e1c2}', S::tl_select, 'v', Group::Select),
    tool(Tool::Crop, '\u{e0af}', S::tl_crop, 'x', Group::Select),
    tool(Tool::Rect, '\u{e16a}', S::tl_rect, 'r', Group::Shapes),
    tool(Tool::Ellipse, '\u{e07a}', S::tl_ellipse, 'o', Group::Shapes),
    tool(Tool::Line, '\u{e521}', S::tl_line, 'l', Group::Shapes),
    tool(Tool::Arrow, '\u{e497}', S::tl_arrow, 'a', Group::Shapes),
    option(
        Tool::ArrowLabel,
        '\u{e544}',
        S::tl_arrow_label,
        'w',
        (Tool::Arrow, Group::Shapes),
    ),
    tool(Tool::Pen, '\u{e133}', S::tl_pen, 'p', Group::Draw),
    tool(
        Tool::Highlighter,
        '\u{e0f7}',
        S::tl_highlighter,
        'h',
        Group::Draw,
    ),
    tool(Tool::Text, '\u{e197}', S::tl_text, 't', Group::Text),
    option(
        Tool::Label,
        '\u{e182}',
        S::tl_label,
        'e',
        (Tool::Text, Group::Text),
    ),
    tool(Tool::Callout, '\u{e11a}', S::tl_callout, 'c', Group::Text),
    tool(Tool::Step, '\u{e1d0}', S::tl_step, 'n', Group::Text),
    option(
        Tool::Pointer,
        '\u{e123}',
        S::tl_pointer,
        'i',
        (Tool::Step, Group::Text),
    ),
    tool(Tool::Blur, '\u{e0b8}', S::tl_blur, 'b', Group::Hide),
    tool(Tool::Redact, '\u{e16b}', S::tl_redact, 'k', Group::Hide),
    tool(
        Tool::Spotlight,
        '\u{e29d}',
        S::tl_spotlight,
        's',
        Group::Hide,
    ),
    tool(Tool::Magnifier, '\u{e1b5}', S::tl_magnify, 'm', Group::Hide),
];

const fn tool(tool: Tool, icon: char, name: S, key: char, group: Group) -> ToolInfo {
    ToolInfo {
        tool,
        icon,
        name,
        key,
        group,
        slot: tool,
    }
}

/// A tool that is an option of `slot` (its panel's switch turns it on).
const fn option(
    tool: Tool,
    icon: char,
    name: S,
    key: char,
    (slot, group): (Tool, Group),
) -> ToolInfo {
    ToolInfo {
        tool,
        icon,
        name,
        key,
        group,
        slot,
    }
}

pub fn info(tool: Tool) -> &'static ToolInfo {
    TOOLS
        .iter()
        .find(|t| t.tool == tool)
        .expect("every tool is listed")
}

/// Stroke weight, in display px (as seen on screen, whatever the zoom).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    Thin,
    Medium,
    Thick,
}

impl Width {
    pub const ALL: [Width; 3] = [Width::Thin, Width::Medium, Width::Thick];
    pub(crate) fn stroke(self) -> f32 {
        [3.0, 5.0, 9.0][self as usize]
    }
    fn font(self) -> f32 {
        [18.0, 26.0, 38.0][self as usize]
    }
    fn badge(self) -> f32 {
        [12.0, 16.0, 22.0][self as usize]
    }
    /// The blur brush's height: Medium covers one line of normal UI text at
    /// 100 % (owner 2026-10-31: a smaller brush; display px, so DPI-scaled).
    fn brush(self) -> f32 {
        [14.0, 20.0, 32.0][self as usize]
    }
    /// The blur's mosaic cell (display px): fine, iTop's look with smaller
    /// cells (owner, 2026-10-31); no finer than 6 px: below that the text shows
    /// through (the candidate-scoring test, Windows CI 2026-11-02).
    pub(crate) fn cell(self) -> f32 {
        [6.0, 9.0, 12.0][self as usize]
    }
}

/// The panels by the toolbar (one at a time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    /// Colour, eyedropper, presets, copy / paste style.
    Colour,
    /// A group button's panel (a click on the active one): the group's
    /// tools, then the current tool's options.
    Options(Group),
    /// Undo's second click: Undo, Redo and where in the history.
    Undo,
    /// More: long capture, pin, share, the screen tools.
    More,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Pointer, image px (from the canvas).
    Press(Point),
    /// Ctrl+press: moves the item under it, whatever the tool (else a press).
    PressMove(Point),
    Move(Point),
    Release,
    /// The press was cancelled (a lost touch): nothing it began completes.
    Cancel,
    /// Shift is held (lines snap to 45° steps).
    Shift(bool),
    /// Typing into a text, label, callout, arrow label or step note.
    Type(String),
    /// Ctrl+V: the clipboard's text into the text being typed, else a new
    /// text at the pointer (image px; `None` = outside: the area's middle).
    Paste(Option<Point>),
    Backspace,
    /// Ends typing.
    Finish,
    Tool(Tool),
    /// A group button (it shows `Tool`, its group's last): picks that tool;
    /// on the active group, opens / closes its panel.
    Slot(Tool),
    /// Opens/closes the colour picker.
    Picker,
    /// Opens/closes a popover.
    Menu(Menu),
    Colour([u8; 4]),
    Width(Width),
    /// The line width of shapes, arrows, pen (display px; its slider).
    Stroke(u8),
    /// A shape's, arrow's or callout's outline (its halo) on / off.
    Outline(bool),
    /// A rectangle's or ellipse's tint inside on / off.
    Fill(bool),
    /// The blur brush's size: `None` = Auto (the text line it starts on).
    BlurSize(Option<Width>),
    /// The blur's pixel size (its mosaic cells).
    BlurCell(Width),
    /// The Text tool's options (its button's strip): font px on screen, and
    /// font, weight, backdrop.
    TextSize(f32),
    TextStyle(TextStyle),
    /// The picker's custom colour field.
    Hex(String),
    /// The next click on the image picks its colour.
    Eyedropper,
    /// A style preset (`PRESETS`).
    Preset(usize),
    CopyStyle,
    PasteStyle,
    /// Shows / hides the steps' legend.
    Legend,
    /// Line help on / off (a blur stroke keeps to the line it started on).
    Assist,
    /// Snip: the flattened image to the Steps strip, as its next step or in
    /// place of the one being edited (`Output::AddStep`).
    AddStep,
    /// Left button down on the drag handle: OS drag of the image.
    DragOut,
    Undo,
    Redo,
    /// The toolbar's Undo: undoes; right after an undo, opens its panel.
    UndoClick,
    Delete,
    Copy,
    /// More: Copy text, Ask AI, Translate on the area (`actions::snip_more`).
    CopyText,
    AskAi,
    Translate,
    /// The toolbar's Scrolling capture button.
    Scroll,
    /// Snip's toolbar: the colour picker or the ruler on the frozen screen.
    ScreenTool(ScreenTool),
    Save,
    /// Pin the flattened image to the screen.
    Pin,
    /// Snip's Live pin: the area's part of the window under it, kept updating.
    LivePin,
    /// Snip's Share: the flattened image to the OS share sheet.
    Share,
    /// The share sheet's user picked a target (the host's answer to
    /// `Output::Sharing`).
    Shared,
    Back,
    /// A press on the toolbar's grip: the host drags the toolbar (the editor
    /// ignores it).
    Grab,
    /// An arrow key on Snip's live area (`area::nudge`): direction, Ctrl,
    /// Shift, Alt.
    Nudge((i8, i8), (bool, bool, bool)),
    /// Snip's toolbar: Tab / Shift+Tab (`true`) walks its controls; Enter /
    /// Space presses the focused one (the host knows the toolbar's order).
    Focus(bool),
    /// The More menu's arrow keys: the next row (`true`: the one before).
    MenuStep(bool),
    /// Home / End in More: the first (`false`) or last (`true`) live row.
    MenuEdge(bool),
    Activate,
}

/// Snip's screen tools on the frozen screen (its toolbar's buttons; the
/// host runs them, `ui::screens::tools`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenTool {
    /// The colour picker (N-75).
    Picker,
    /// The screen ruler (N-76).
    Ruler,
}

/// What the editor asks of its host.
pub enum Output {
    /// Back to the action bar (edits are kept in the editor).
    Back,
    /// The flattened image, for the Copy / Save actions.
    Copy(Crop),
    Save(Crop),
    /// More's rows: read the clean capture's text and copy it; ask the AI
    /// about the annotated picture (this one); translate the clean capture.
    CopyText,
    AskAi(Crop),
    Translate,
    /// Scroll and stitch the area under the editor (Snip, in place only).
    Scroll,
    /// Leave for a screen tool (Snip; Esc there comes back to the area).
    ScreenTool(ScreenTool),
    /// A floating always-on-top copy of the image.
    Pin(Crop),
    /// A floating copy of the area that keeps following the window under it (Snip).
    LivePin,
    /// The flattened image for the Steps strip (`Annotator::updating`: in place
    /// of that step).
    AddStep(Crop),
    /// The share sheet is up: fires once the user picked a target (send
    /// `Message::Shared` then); cancelled if the sheet goes without one.
    Sharing(iced::futures::channel::oneshot::Receiver<()>),
    /// The image was dragged into another app (the overlay is already hidden),
    /// or a share target took it: close, as after Copy.
    Dragged,
}

/// An item being drawn, moved, resized or typed: shown live, committed as one edit.
struct Pending {
    /// The document item it replaces; `None` = new.
    index: Option<usize>,
    item: Item,
    drag: Drag,
    start: Point,
    /// The base is painted without the item (it changed since it was picked up).
    hidden: bool,
}

enum Drag {
    Draw,
    /// The item as picked up; `edits`: released without moving, it is typed into.
    Move {
        from: Item,
        edits: bool,
    },
    /// A handle of the item as picked up.
    Handle(select::Handle, Item),
    Type,
}

/// Handles react this close to the pointer (display px).
const REACH: f32 = 10.0;
/// A press this close to a stroke (display px) grabs it.
const GRAB: f32 = 4.0;
/// A press that moves less than this (display px) is a click.
const SLOP: f32 = 3.0;

/// Snip's live area being moved or resized: the grip, where the press was
/// (image px), the area and the items as they were then.
struct AreaDrag {
    grip: area::Grip,
    from: Point,
    start: Rectangle,
    items: Vec<Item>,
}

pub struct Annotator {
    pub doc: Document,
    pub tool: Tool,
    pub(crate) colour: [u8; 4],
    pub(crate) width: Width,
    /// The line width of shapes, arrows, pen (display px; `width` sets it).
    pub(crate) stroke: f32,
    /// Each group's last tool (`Group::ALL` order): its button shows it.
    pub(crate) last: [Tool; 5],
    /// The last change was an undo: Undo's next click opens its panel.
    pub(crate) undid: bool,
    /// The More menu's row the arrow keys are on.
    pub row: Option<usize>,
    /// What More's text rows show (the host keeps it current).
    pub more: crate::features::snip::snip_more::Info,
    /// New shapes, arrows and callouts get an outline / rectangles and
    /// ellipses a tint (both off by default, owner 2026-10-31).
    pub(crate) outline: bool,
    pub(crate) fill: bool,
    /// The Text tool's font size (display px; a weight sets it too) and style.
    pub(crate) text_size: f32,
    pub(crate) text_style: TextStyle,
    pub(crate) menu: Option<Menu>,
    pub(crate) hex: String,
    /// Display px per image px.
    pub(crate) zoom: f32,
    shift: bool,
    /// The next press picks a colour from the image.
    pub(crate) picking: bool,
    /// Copied style: colour, weight.
    style: Option<([u8; 4], Width)>,
    pub(crate) base: image::Handle,
    /// The legend as the base shows it (placed among the committed items).
    legend: Option<legend::Legend>,
    /// The pending item's region (image px) painted with it.
    pub(crate) live: Option<(Rectangle, image::Handle)>,
    pending: Option<Pending>,
    /// Crop drag: from, to.
    cropping: Option<(Point, Point)>,
    /// The PNG of the last drag-out (deleted with the editor).
    dragged: Option<dragout::DragFile>,
    /// The last share (its PNG and the sheet's handlers; go with the editor).
    shared: Option<crate::actions::share::Shared>,
    /// Line help: a blur stroke keeps to the line it started on (config
    /// `editor.line_assist`).
    pub(crate) assist: bool,
    /// Scrolling capture can run for this editor (the host sets it; the
    /// toolbar button is dimmed without).
    pub scroll: bool,
    /// A long capture of the area is running: the toolbar keeps only what
    /// finishes it (Copy, Save, Pin, Share, close); the rest is dimmed.
    pub capturing: bool,
    /// Window and control rectangles (image px) the live area's edges snap to
    /// while it is dragged (the host sets them; none on Wayland).
    pub edges: Vec<Rectangle>,
    /// The height (image px) the blur stroke being drawn is held at.
    lock: Option<f32>,
    /// The blur brush's size picked in its options; `None` = Auto: as tall
    /// as the text line under the stroke's start (owner, 2026-10-31).
    pub(crate) blur_size: Option<Width>,
    /// The blur's pixel size (its mosaic cells).
    pub(crate) blur_cell: Width,
    /// The image's text lines (Auto's), for the image they were found in.
    bands: Option<((u32, u32), Vec<lines::Band>)>,
    /// Snip's live area: the document is the whole frozen monitor (or every
    /// monitor joined) and its crop is the captured area, which moves and
    /// resizes (`area`). The host places its top-left pixel (`canvas::ScreenMap`).
    pub movable: bool,
    /// The live area is on a capture of its own (a stitched long capture, a
    /// recent one), not on the frozen frame: its pixels are always painted.
    pub own_image: bool,
    area_drag: Option<AreaDrag>,
    /// Snip's toolbar control Tab is on (its index in the toolbar's order);
    /// a press anywhere drops it.
    pub focus: Option<usize>,
    /// Snip's arrow-key hint shows (after the first arrow key).
    pub nudged: bool,
    /// The host drags the toolbar (its grip): it shows lifted.
    pub held: bool,
    /// Steps in the Steps strip (the host sets it; `None`: no strip): Copy's
    /// place holds "Add step N", Enter adds.
    pub steps: Option<usize>,
    /// The strip's step being edited (its index): "Update step N".
    pub updating: Option<usize>,
    /// The options as last remembered (`remember`).
    saved: EditorConfig,
}

impl Annotator {
    /// Opens `image`, shown at `zoom` display px per image px (e.g. 1 / the
    /// monitor's scale factor, so the crop shows at its on-screen size), with
    /// the user's last tool and options (owner, 2026-10-31).
    pub fn new(image: Image, zoom: f32) -> Self {
        Self::restored(image, zoom, remembered())
    }

    /// `new` with `saved`'s tool and options (a missing or bad one: its default).
    pub(crate) fn restored(image: Image, zoom: f32, saved: EditorConfig) -> Self {
        let colour = (saved.colour.as_deref())
            .and_then(parse_hex)
            .unwrap_or(style::ANNOTATION[0].into_rgba8());
        let width = saved.size.unwrap_or(Width::Medium);
        let tool = (saved.tool)
            .filter(|&t| t != Tool::Crop)
            .unwrap_or(Tool::Arrow);
        let mut last = Group::FIRST;
        if let Some(tools) = saved.group_tools.as_deref().filter(|t| Group::valid(t)) {
            last.copy_from_slice(tools);
        }
        last[info(tool).group as usize] = tool;
        let mut editor = Annotator {
            doc: Document::new(image),
            tool,
            colour,
            width,
            stroke: saved.stroke.map_or(width.stroke(), f32::from),
            last,
            undid: false,
            row: None,
            more: Default::default(),
            outline: saved.outline.unwrap_or(false),
            fill: saved.fill.unwrap_or(false),
            text_size: saved.text_size.map_or(width.font(), f32::from),
            text_style: {
                let d = TextStyle::default();
                TextStyle {
                    font: saved.text_font.unwrap_or(d.font),
                    bold: saved.text_bold.unwrap_or(d.bold),
                    back: saved.text_back.unwrap_or(d.back),
                }
            },
            menu: None,
            hex: hex(colour),
            zoom,
            shift: false,
            picking: false,
            style: None,
            base: image::Handle::from_rgba(1, 1, vec![0; 4]),
            legend: None,
            live: None,
            pending: None,
            cropping: None,
            dragged: None,
            shared: None,
            assist: saved.line_assist,
            scroll: false,
            capturing: false,
            lock: None,
            blur_size: saved.blur_size,
            blur_cell: saved.blur_cell.unwrap_or(Width::Thin),
            bands: None,
            movable: false,
            edges: Vec::new(),
            own_image: false,
            area_drag: None,
            focus: None,
            nudged: false,
            held: false,
            steps: None,
            updating: None,
            saved,
        };
        editor.refresh();
        editor
    }

    /// The tool and options to remember: `saved` with this editor's own.
    pub(crate) fn remembered(&self) -> EditorConfig {
        EditorConfig {
            line_assist: self.assist,
            // Crop is a step, not a tool to open on.
            tool: (self.tool != Tool::Crop)
                .then_some(self.tool)
                .or(self.saved.tool),
            colour: Some(hex(self.colour)),
            size: Some(self.width),
            text_size: Some(self.text_size.round().clamp(8.0, 96.0) as u8),
            outline: Some(self.outline),
            fill: Some(self.fill),
            blur_size: self.blur_size,
            blur_cell: Some(self.blur_cell),
            stroke: Some(self.stroke.round().clamp(1.0, 32.0) as u8),
            text_font: Some(self.text_style.font),
            text_bold: Some(self.text_style.bold),
            text_back: Some(self.text_style.back),
            group_tools: Some(self.last.to_vec()),
        }
    }

    pub fn update(&mut self, message: Message) -> Option<Output> {
        // Pointer messages never write (a press can pick up an item's options;
        // they are remembered with the next choice).
        let pointer = matches!(
            message,
            Message::Press(_)
                | Message::PressMove(_)
                | Message::Move(_)
                | Message::Release
                | Message::Cancel
        );
        let out = self.handle(message);
        if !pointer {
            let now = self.remembered();
            if now != self.saved {
                self.saved = now.clone();
                remember(now);
            }
        }
        out
    }

    fn handle(&mut self, message: Message) -> Option<Output> {
        // Undo's next click opens its panel only right after an undo.
        if !matches!(
            message,
            Message::Undo
                | Message::Redo
                | Message::UndoClick
                | Message::Menu(_)
                | Message::Move(_)
                | Message::Release
                | Message::Cancel
                | Message::Shift(_)
                | Message::Focus(_)
                | Message::MenuStep(_)
                | Message::MenuEdge(_)
                | Message::Activate
        ) {
            self.undid = false;
        }
        match message {
            Message::Press(p) => {
                (self.focus, self.nudged) = (None, false);
                // starting to draw closes a panel (not the colour's: its field)
                self.menu = self.menu.filter(|m| *m == Menu::Colour);
                self.press(p)
            }
            Message::PressMove(p) => {
                (self.focus, self.nudged) = (None, false);
                self.menu = self.menu.filter(|m| *m == Menu::Colour);
                self.commit();
                match select::hit(&self.doc.items, p, REACH / self.zoom) {
                    Some(i) => {
                        self.doc.selected = Some(i);
                        self.pick_up(i, false, p);
                    }
                    None => self.press(p),
                }
            }
            Message::Move(p) => self.drag_to(p),
            Message::Release => self.release(),
            Message::Cancel => self.cancel_press(),
            Message::Shift(on) => self.shift = on,
            Message::Type(s) => self.edit_text(|t| t.push_str(&s)),
            Message::Paste(at) => self.paste(at),
            Message::Backspace => self.edit_text(|t| {
                t.pop();
            }),
            Message::Finish => self.commit(),
            Message::Tool(tool) => self.pick_tool(tool),
            // First click: the group's tool; a click on the active one: its panel.
            Message::Slot(tool) if info(self.tool).group != info(tool).group => {
                self.pick_tool(tool)
            }
            Message::Slot(tool) => {
                let group = info(tool).group;
                let open = self.menu == Some(Menu::Options(group));
                self.menu = (group.has_panel() && !open).then_some(Menu::Options(group));
                self.picking = false;
            }
            Message::Picker => self.toggle(Menu::Colour),
            Message::Menu(menu) => self.toggle(menu),
            Message::Colour(c) => self.set_colour(c),
            Message::Width(w) => {
                self.set_width(w);
                self.restyle();
            }
            Message::Stroke(px) => {
                self.stroke = f32::from(px);
                let off = |w: &Width| (w.stroke() - self.stroke).abs();
                if let Some(&w) = Width::ALL.iter().min_by(|a, b| off(a).total_cmp(&off(b))) {
                    self.width = w;
                }
                self.restyle();
            }
            Message::BlurSize(size) => {
                self.blur_size = size;
                self.restyle();
            }
            Message::BlurCell(cell) => {
                self.blur_cell = cell;
                self.restyle();
            }
            Message::Outline(on) => {
                self.outline = on;
                self.restyle();
            }
            Message::Fill(on) => {
                self.fill = on;
                self.restyle();
            }
            Message::TextSize(size) => {
                self.text_size = size;
                self.restyle();
            }
            Message::TextStyle(style) => {
                self.text_style = style;
                self.restyle();
            }
            Message::Hex(s) => {
                if let Some(c) = parse_hex(&s) {
                    self.colour = c;
                    self.restyle();
                }
                self.hex = s;
            }
            Message::Eyedropper => {
                self.picking = !self.picking;
                self.menu = None;
            }
            Message::Preset(i) => self.preset(i),
            Message::CopyStyle => {
                let item = (self.pending.as_ref().map(|p| &p.item))
                    .or_else(|| self.doc.selected.and_then(|i| self.doc.items.get(i)));
                if let Some(item) = item {
                    self.style = Some((item.colour, style::level(item, self.zoom)));
                }
            }
            Message::PasteStyle => {
                if let Some((colour, width)) = self.style {
                    self.set_width(width);
                    self.set_colour(colour);
                }
            }
            Message::Assist => {
                self.assist = !self.assist;
            }
            Message::Legend => {
                self.commit();
                self.doc.legend = !self.doc.legend;
                self.refresh();
            }
            Message::AddStep if self.adds_step() => {
                self.commit();
                self.menu = None;
                return Some(Output::AddStep(render(&self.doc)));
            }
            Message::AddStep => {}
            Message::DragOut => {
                self.commit();
                let flat = render(&self.doc);
                // The previous drag's file goes first (one temp file at a time).
                self.dragged = None;
                match dragout::DragFile::write(&flat) {
                    Ok(mut file) => {
                        // The overlay hides for the drag; dropped = done, like Copy.
                        let dropped = dragout::start(&mut file, &flat, true);
                        self.dragged = Some(file);
                        match dropped {
                            Ok(true) => return Some(Output::Dragged),
                            Ok(false) => {}
                            Err(e) => crate::log!("[opencircle] {e}"),
                        }
                    }
                    Err(e) => crate::log!("[opencircle] drag-out: {e}"),
                }
            }
            Message::Share => {
                self.commit();
                let flat = render(&self.doc);
                self.shared = None; // one share at a time
                match crate::actions::share::start(&flat) {
                    Ok((shared, chosen)) => {
                        self.shared = Some(shared);
                        return Some(Output::Sharing(chosen));
                    }
                    Err(e) => crate::log!("[opencircle] {e}"),
                }
            }
            // A target took the image: done, like a drop (the file stays its grace).
            Message::Shared => {
                if let Some(shared) = &mut self.shared {
                    shared.chosen();
                    return Some(Output::Dragged);
                }
            }
            Message::UndoClick if self.undid => self.toggle(Menu::Undo),
            Message::UndoClick => return self.handle(Message::Undo),
            Message::Undo | Message::Redo => {
                // Ctrl+Z while typing ends the typing; that is the last thing
                // done, so a text, callout or label it made goes with it, and
                // an empty box just closes (the next Undo goes further back).
                let (typing, depth) = (self.typing(), self.doc.depth());
                self.commit();
                let undo = matches!(message, Message::Undo);
                let closed = typing && self.doc.depth() == depth;
                if !(closed && undo)
                    && if undo {
                        self.doc.undo()
                    } else {
                        self.doc.redo()
                    }
                {
                    self.refresh();
                }
                self.undid = true;
            }
            Message::Delete => {
                self.commit();
                if let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) {
                    let item = self.doc.items[i].clone();
                    self.doc.exec(Edit::Delete(i, item));
                    self.refresh();
                }
            }
            Message::ScreenTool(tool) => {
                self.commit();
                return Some(Output::ScreenTool(tool));
            }
            Message::Scroll => {
                if self.scroll {
                    self.commit();
                    return Some(Output::Scroll);
                }
            }
            // like Long capture: the area over the live screen (Snip in place)
            Message::LivePin if self.scroll => return Some(Output::LivePin),
            Message::LivePin => {}
            Message::Copy | Message::Save | Message::Pin => {
                self.commit();
                let flat = render(&self.doc);
                return Some(match message {
                    Message::Copy => Output::Copy(flat),
                    Message::Save => Output::Save(flat),
                    _ => Output::Pin(flat),
                });
            }
            Message::CopyText => {
                self.commit();
                self.menu = None;
                return Some(Output::CopyText);
            }
            Message::Translate => {
                self.commit();
                self.menu = None;
                return Some(Output::Translate);
            }
            Message::AskAi => {
                self.commit();
                self.menu = None;
                return Some(Output::AskAi(render(&self.doc)));
            }
            Message::Back => {
                self.commit();
                return Some(Output::Back);
            }
            Message::Grab
            | Message::Focus(_)
            | Message::MenuStep(_)
            | Message::MenuEdge(_)
            | Message::Activate => {}
            Message::Nudge(arrow, keys) if self.movable => {
                self.commit();
                self.nudged = true;
                let view = self.doc.view();
                let to = whole(area::nudge(
                    view,
                    arrow,
                    keys,
                    1.0 / self.zoom,
                    self.frame(),
                    area::MIN_SIDE,
                ));
                self.set_area(view, to, !keys.0);
            }
            Message::Nudge(..) => {}
        }
        None
    }

    /// The whole image (Snip: the frozen monitor), image px.
    fn frame(&self) -> Rectangle {
        let i = &self.doc.image;
        Rectangle::new(Point::ORIGIN, Size::new(i.width as f32, i.height as f32))
    }

    /// The live area goes from `from` to `to` as one undoable edit: a move
    /// takes every item with it; a resize leaves them on their pixels.
    fn set_area(&mut self, from: Rectangle, to: Rectangle, moves: bool) {
        if to == from {
            return;
        }
        let moved = match moves {
            true => to.position() - from.position(),
            false => Vector::ZERO,
        };
        self.doc.exec(Edit::Area(from, to, moved));
        self.refresh();
    }

    /// Snip's live area on the canvas while it is dragged by a handle: where
    /// that handle is (canvas px), for the host to keep the toolbar off it.
    pub fn area_handle(&self) -> Option<Point> {
        let drag = self
            .area_drag
            .as_ref()
            .filter(|d| d.grip != area::Grip::Move)?;
        Some(self.to_display_point(drag.grip.at(self.doc.view())))
    }

    /// Snip's live area is being moved or resized.
    pub fn area_held(&self) -> bool {
        self.area_drag.is_some()
    }

    /// Snip's live area: the handle held (`true`), else the one under `p`
    /// (image px), as its edges (`area::Grip::Edge`).
    pub fn area_hot(&self, p: Option<Point>) -> Option<((i8, i8), bool)> {
        let edges = |g| match g {
            area::Grip::Edge(x, y) => Some((x, y)),
            area::Grip::Move => None,
        };
        if let Some(drag) = &self.area_drag {
            return edges(drag.grip).map(|e| (e, true));
        }
        let grip = area::grip_at(self.doc.view(), p?, area::REACH / self.zoom)?;
        edges(grip).map(|e| (e, false))
    }

    /// The annotation colour as an iced colour.
    pub fn ink(&self) -> iced::Color {
        let [r, g, b, a] = self.colour;
        iced::Color::from_rgba8(r, g, b, a as f32 / 255.0)
    }

    /// `AddStep` does something: a step being edited, or room in the strip
    /// (`STEPS_MAX`; no strip yet = its first step).
    pub fn adds_step(&self) -> bool {
        self.updating.is_some() || self.steps.is_none_or(|n| n < STEPS_MAX)
    }

    /// The canvas size, display px (changes with the crop and the legend's band).
    pub fn size(&self) -> Size {
        let band = self.legend.as_ref().map_or(0.0, |l| l.band);
        (self.doc.view().size() + Size::new(0.0, band)) * self.zoom
    }

    /// Text or a callout is being typed into.
    fn typing(&self) -> bool {
        matches!(
            self.pending,
            Some(Pending {
                drag: Drag::Type,
                ..
            })
        )
    }

    fn toggle(&mut self, menu: Menu) {
        self.menu = (self.menu != Some(menu)).then_some(menu);
        self.picking = false;
        self.row = None;
    }

    /// Picks `tool`, its group's last from now on; its group's open panel
    /// stays (it shows this tool's options), any other closes.
    fn pick_tool(&mut self, tool: Tool) {
        self.commit();
        self.tool = tool;
        let group = info(tool).group;
        if tool != Tool::Crop {
            self.last[group as usize] = tool;
        }
        self.menu = self.menu.filter(|m| *m == Menu::Options(group));
        self.picking = false;
    }

    /// The weight; the line width and a text's size follow it.
    fn set_width(&mut self, w: Width) {
        self.width = w;
        self.stroke = w.stroke();
        self.text_size = w.font();
    }

    fn set_colour(&mut self, c: [u8; 4]) {
        self.colour = c;
        self.hex = hex(c);
        self.restyle();
    }

    /// A style preset: its colour and weight, for its tool (kept when the active
    /// one is one it suits). Redact on a selected box turns it into a solid box.
    fn preset(&mut self, i: usize) {
        let Some(p) = PRESETS.get(i) else {
            return;
        };
        self.menu = None;
        self.commit();
        let colour = style::ANNOTATION[p.colour].into_rgba8();
        if p.tool == Tool::Redact
            && let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len())
            && let Shape::Rect { a, b } | Shape::Ellipse { a, b } | Shape::Spotlight { a, b } =
                self.doc.items[i].shape
        {
            let before = self.doc.items[i].clone();
            let after = Item {
                shape: Shape::Redact { a, b },
                colour,
                ..before.clone()
            };
            self.doc.exec(Edit::Change(i, before, after));
            self.refresh();
        }
        if !p.keeps.contains(&self.tool) {
            self.pick_tool(p.tool);
            self.doc.selected = None; // the style is for the next item
        }
        self.set_width(p.width);
        self.set_colour(colour);
    }

    /// The image's text lines (found once per image).
    fn bands(&mut self) -> &[lines::Band] {
        let key = (self.doc.image.width, self.doc.image.height);
        if self.bands.as_ref().is_none_or(|(k, _)| *k != key) {
            self.bands = Some((key, lines::detect(&self.doc.image)));
        }
        self.bands.as_ref().map_or(&[], |(_, b)| b)
    }

    /// The blur brush's size at `p` (image px) and, for Auto on a line, the
    /// line's middle: the size picked, else Auto: the text line there (a line
    /// taller than three default brushes is a block, not a line), else the
    /// default.
    fn brush_size(&mut self, p: Point) -> (f32, Option<f32>) {
        let default = Width::Medium.brush() / self.zoom;
        if let Some(w) = self.blur_size {
            return (w.brush() / self.zoom, None);
        }
        let zoom = self.zoom;
        lines::under(self.bands(), p)
            .map(|b| b.stroke(zoom))
            .filter(|&(_, height)| height <= 3.0 * default)
            .map_or((default, None), |(middle, height)| (height, Some(middle)))
    }

    /// A blur stroke at `p`, sized (`brush_size`; Auto on a line: on its
    /// middle, so it covers that line and no other); with line help (and no
    /// Shift) held at its height.
    fn brush(&mut self, mut p: Point) -> Item {
        let (size, middle) = self.brush_size(p);
        if let Some(y) = middle.filter(|_| !self.shift) {
            p.y = y;
        }
        if self.assist && !self.shift {
            self.lock = Some(p.y);
        }
        let mut item = self.create(p);
        if let Shape::Blur { size: s, .. } = &mut item.shape {
            *s = size;
        }
        item
    }

    fn press(&mut self, p: Point) {
        if self.picking {
            self.picking = false;
            if let Some(c) = sample(&self.doc.image, p) {
                self.set_colour(c);
            }
            return;
        }
        let reach = REACH / self.zoom;
        self.commit();
        // Snip's live area: its handles and edges in every tool.
        if self.movable
            && let Some(grip) = area::grip_at(self.doc.view(), p, area::REACH / self.zoom)
        {
            self.grab_area(grip, p);
            return;
        }
        // The selected item's handles work in every tool.
        if let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) {
            let original = self.doc.items[i].clone();
            if let Some(handle) = select::handle_at(&original, p, reach) {
                let drag = Drag::Handle(handle, original.clone());
                self.begin(Some(i), original, drag, p);
                return;
            }
        }
        if self.tool == Tool::Select {
            let hit = select::hit(&self.doc.items, p, reach);
            self.doc.selected = hit;
            match hit {
                Some(i) => self.pick_up(i, false, p),
                // Nothing there: the press inside moves Snip's live area.
                None if self.movable && self.doc.view().contains(p) => {
                    self.grab_area(area::Grip::Move, p)
                }
                None => {}
            }
            return;
        }
        if let Some(i) = self.grabbed(p) {
            self.doc.selected = Some(i);
            self.pick_up(i, true, p);
            return;
        }
        match self.tool {
            Tool::Crop => self.cropping = Some((p, p)),
            Tool::Blur => {
                let item = self.brush(p);
                self.begin(None, item, Drag::Draw, p);
            }
            _ => {
                let item = self.create(p);
                self.begin(None, item, Drag::Draw, p);
            }
        }
    }

    /// The item a click at `p` types into instead of drawing: one of the
    /// tool's own text items (`edits`). Nothing else: a drawing tool never
    /// moves what is under it; only Ctrl+drag or Select move (owner, 2026-10-31).
    fn grabbed(&self, p: Point) -> Option<usize> {
        let i = select::grab(&self.doc.items, p, GRAB / self.zoom)?;
        edits(self.tool, &self.doc.items[i], p, REACH / self.zoom).then_some(i)
    }

    /// Starts moving item `i` (`edits`: a click without a move types into it).
    fn pick_up(&mut self, i: usize, edits: bool, p: Point) {
        let from = self.doc.items[i].clone();
        // A text's options show (and edit) its own.
        if let Shape::Text { size, style, .. } = &from.shape {
            (self.text_size, self.text_style) = (size * self.zoom, *style);
        }
        (self.outline, self.fill) = (from.outline, from.fill);
        self.begin(Some(i), from.clone(), Drag::Move { from, edits }, p);
    }

    fn begin(&mut self, index: Option<usize>, item: Item, drag: Drag, start: Point) {
        let bare = self.bare();
        self.pending = Some(Pending {
            index,
            item,
            drag,
            start,
            hidden: false,
        });
        match bare {
            true => self.refresh(), // the first item: the base is painted now
            false => self.update_live(),
        }
    }

    /// Repaints the base without the pending item (once, when it first changes).
    fn hide(&mut self) {
        if let Some(pending) = &mut self.pending
            && pending.index.is_some()
            && !pending.hidden
        {
            pending.hidden = true;
            self.refresh();
        }
    }

    fn grab_area(&mut self, grip: area::Grip, from: Point) {
        self.area_drag = Some(AreaDrag {
            grip,
            from,
            start: self.doc.view(),
            items: self.doc.items.clone(),
        });
    }

    /// Where the live area is dragged to now (whole image px).
    fn area_at(&self, drag: &AreaDrag, p: Point) -> Rectangle {
        let r = area::dragged(
            drag.grip,
            drag.start,
            p - drag.from,
            self.frame(),
            area::MIN_SIDE,
        );
        match self.shift {
            true => whole(r),
            false => whole(self.snapped(drag.grip, r, p)),
        }
    }

    /// `r` with the edges the grip moves pulled onto a window or control edge
    /// within the snap reach (as a first drag is; the same `snap`); Shift = off.
    fn snapped(&self, grip: area::Grip, r: Rectangle, at: Point) -> Rectangle {
        use crate::features::snip::{SNAP, snap};
        let reach = SNAP / self.zoom;
        // The edge value `v` snapped; probed along the area's side, middle and
        // far side and at the pointer, so a window's edge is found wherever the
        // area (or the pointer) meets it.
        let pull = |v: f32, x_axis: bool| {
            let (lo, len, near) = if x_axis {
                (r.y, r.height, at.y)
            } else {
                (r.x, r.width, at.x)
            };
            [lo, lo + len / 2.0, lo + len, near]
                .into_iter()
                .map(|o| {
                    let p = if x_axis {
                        Point::new(v, o)
                    } else {
                        Point::new(o, v)
                    };
                    let s = snap(p, &self.edges, reach);
                    if x_axis { s.x } else { s.y }
                })
                .filter(|s| *s != v) // (a probe that found no edge returns `v`)
                .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
                .unwrap_or(v)
        };
        let (x1, y1) = (r.x + r.width, r.y + r.height);
        let min = area::MIN_SIDE;
        let frame = self.frame();
        let axis = |s: i8, lo: f32, hi: f32, x_axis: bool| {
            let (a, b) = (pull(lo, x_axis), pull(hi, x_axis));
            match s {
                -1 => (a.min(hi - min), hi),
                1 => (lo, b.max(lo + min)),
                // A move: the edge that moved less (0 = none) brings the other.
                _ if grip == area::Grip::Move => {
                    let far = |d: f32| if d == 0.0 { f32::MAX } else { d.abs() };
                    let (da, db) = (a - lo, b - hi);
                    let by = if far(da) <= far(db) { da } else { db };
                    (lo + by, hi + by)
                }
                _ => (lo, hi),
            }
        };
        let (sx, sy) = match grip {
            area::Grip::Move => (0, 0),
            area::Grip::Edge(x, y) => (x, y),
        };
        let (nx0, nx1) = axis(sx, r.x, x1, true);
        let (ny0, ny1) = axis(sy, r.y, y1, false);
        let snapped = Rectangle::new(Point::new(nx0, ny0), Size::new(nx1 - nx0, ny1 - ny0));
        // never out of the frozen frame
        match frame.contains(snapped.position())
            && frame.contains(Point::new(nx1 - 0.01, ny1 - 0.01))
        {
            true => snapped,
            false => r,
        }
    }

    fn drag_to(&mut self, mut p: Point) {
        // The live area follows the pointer (and its items, for a move); one
        // undoable edit when let go.
        if let Some(drag) = &self.area_drag {
            let to = self.area_at(drag, p);
            if Some(to) != self.doc.crop {
                let moved = to.position() - drag.start.position();
                self.doc.items = drag.items.clone();
                if drag.grip == area::Grip::Move {
                    self.doc.shift(moved);
                }
                self.doc.crop = Some(to);
                self.refresh();
            }
            return;
        }
        if let Some((from, _)) = self.cropping {
            self.cropping = Some((from, p));
            return;
        }
        if let Some(held) = &mut self.lock
            && !self.shift
        {
            let brush = match self.pending.as_ref().map(|p| &p.item.shape) {
                Some(Shape::Blur { size, .. }) => *size,
                _ => Width::Medium.brush() / self.zoom,
            };
            p = blur::follow(held, p, brush);
        }
        // A press on a text item the tool types into that turns into a drag
        // draws a new item from there: a drawing tool never moves one.
        if let Some(Pending {
            drag: Drag::Move { edits: true, .. },
            start,
            ..
        }) = self.pending
            && p.distance(start) * self.zoom >= SLOP
        {
            self.pending = None;
            self.doc.selected = None;
            let item = self.create(start);
            self.begin(None, item, Drag::Draw, start);
        }
        let Some(pending) = &mut self.pending else {
            return;
        };
        pending.item = match &pending.drag {
            Drag::Draw => grow(&pending.item, pending.start, p, self.zoom, self.shift),
            // A click that wobbles a pixel is not a move.
            Drag::Move { from, .. } if p.distance(pending.start) * self.zoom < SLOP => from.clone(),
            Drag::Move { from, .. } => select::moved(from, p - pending.start),
            Drag::Handle(handle, original) => select::dragged(original, *handle, p),
            Drag::Type => return,
        };
        if pending.hidden || pending.index.is_none() {
            self.update_live();
        } else {
            self.hide();
        }
    }

    /// A cancelled press (a lost touch) ends like a release that completes nothing:
    /// the area and the pending item stay as they were before the press.
    fn cancel_press(&mut self) {
        self.lock = None;
        if let Some(drag) = self.area_drag.take() {
            (self.doc.crop, self.doc.items) = (Some(drag.start), drag.items);
            self.refresh();
        } else if self.cropping.take().is_none()
            && self
                .pending
                .as_ref()
                .is_some_and(|p| !matches!(p.drag, Drag::Type))
        {
            self.pending = None;
            self.refresh();
        }
    }

    fn release(&mut self) {
        self.lock = None;
        if let Some(drag) = self.area_drag.take() {
            let to = self.doc.view();
            (self.doc.crop, self.doc.items) = (Some(drag.start), drag.items);
            self.set_area(drag.start, to, drag.grip == area::Grip::Move);
            if to == drag.start {
                self.refresh();
            }
            return;
        }
        if let Some((from, to)) = self.cropping.take() {
            if let Some(area) = crop::area(from, to, self.doc.view()) {
                self.doc.exec(Edit::Crop(self.doc.crop, Some(area)));
                self.refresh();
            }
            return;
        }
        let labelled = self.tool == Tool::ArrowLabel;
        let Some(pending) = &self.pending else {
            return;
        };
        let typed = match (&pending.drag, &pending.item.shape) {
            (Drag::Type, _) => return,
            (
                Drag::Draw,
                Shape::Text { .. }
                | Shape::Callout { .. }
                | Shape::Label { .. }
                | Shape::Step { .. },
            ) => true,
            (Drag::Draw, Shape::Arrow { .. }) => labelled && !self.too_small(&pending.item),
            (Drag::Move { from, edits: true }, _) => pending.item == *from,
            _ => false,
        };
        let Some(pending) = &mut self.pending else {
            return;
        };
        if typed {
            pending.drag = Drag::Type;
            self.hide();
            self.update_live();
            return;
        }
        if let (Drag::Draw, Shape::Pen(points)) = (&pending.drag, &pending.item.shape)
            && let Some(snapped) = shape::snap(points)
        {
            pending.item.shape = snapped;
        }
        self.commit();
    }

    /// Ends the pending change: one undoable edit (or none if nothing changed).
    fn commit(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let item = pending.item;
        let empty = matches!(&item.shape,
            Shape::Text { text, .. } | Shape::Callout { text, .. } | Shape::Label { text, .. }
                if text.trim().is_empty());
        let edit = match pending.index {
            None if empty || self.too_small(&item) => None,
            None => Some(Edit::Insert(self.doc.items.len(), item)),
            Some(i) if empty => Some(Edit::Delete(i, self.doc.items[i].clone())),
            Some(i) if item != self.doc.items[i] => {
                Some(Edit::Change(i, self.doc.items[i].clone(), item))
            }
            Some(_) => None,
        };
        let repaint = edit.is_some() || pending.hidden;
        if let Some(edit) = edit {
            self.doc.exec(edit);
        }
        if repaint {
            self.refresh();
        } else {
            self.live = None;
        }
    }

    /// A drag that is really a click (shapes that need a size).
    fn too_small(&self, item: &Item) -> bool {
        let f = match &item.shape {
            // A labelled arrow's frame includes its label.
            Shape::Arrow { a, b, .. } => document::span(&[*a, *b]),
            _ => item.frame(),
        };
        let tiny = f.width.hypot(f.height) * self.zoom < 4.0;
        tiny && matches!(
            item.shape,
            Shape::Rect { .. }
                | Shape::Ellipse { .. }
                | Shape::Arrow { .. }
                | Shape::Redact { .. }
                | Shape::Spotlight { .. }
        )
    }

    /// Ctrl+V (owner, 2026-11-03): the clipboard's text where the typing is,
    /// else a new text (the last text style) at `at`, being typed into.
    /// A picture on the clipboard: nothing (pasting pictures is not built).
    fn paste(&mut self, at: Option<Point>) {
        let Some(text) = crate::actions::copy::read_text().map(|t| clean_paste(&t)) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        if self.typing() {
            return self.edit_text(|t| t.push_str(&text));
        }
        self.commit();
        let view = self.doc.view();
        let p = at.filter(|p| view.contains(*p)).unwrap_or(view.center());
        let tool = std::mem::replace(&mut self.tool, Tool::Text);
        let mut item = self.create(p);
        self.tool = tool;
        if let Some(t) = text_mut(&mut item.shape) {
            *t = text;
        }
        self.begin(None, item, Drag::Type, p);
    }

    fn edit_text(&mut self, f: impl FnOnce(&mut String)) {
        if let Some(Pending {
            item,
            drag: Drag::Type,
            ..
        }) = &mut self.pending
            && let Some(text) = text_mut(&mut item.shape)
        {
            f(text);
            self.update_live();
        }
    }

    /// A new item of the current tool at `p`, in the current colour and width.
    fn create(&self, p: Point) -> Item {
        let z = self.zoom;
        let shape = match self.tool {
            Tool::Rect => Shape::Rect { a: p, b: p },
            Tool::Ellipse => Shape::Ellipse { a: p, b: p },
            Tool::Arrow | Tool::Line | Tool::ArrowLabel => Shape::Arrow {
                a: p,
                b: p,
                bend: p,
                head: self.tool != Tool::Line,
                text: String::new(),
            },
            Tool::Pen | Tool::Select | Tool::Crop => Shape::Pen(vec![p]),
            Tool::Highlighter => Shape::Highlighter(vec![p]),
            Tool::Text => Shape::Text {
                at: p,
                text: String::new(),
                size: self.text_size / z,
                style: self.text_style,
            },
            Tool::Label => Shape::Label {
                at: p,
                text: String::new(),
                size: self.text_size * 0.75 / z,
            },
            Tool::Callout => Shape::Callout {
                tip: p,
                at: p + iced::Vector::new(90.0, -60.0) * (1.0 / z),
                text: String::new(),
                size: self.text_size * 0.75 / z,
            },
            Tool::Step => Shape::Step {
                at: p,
                radius: self.width.badge() / z,
                note: String::new(),
            },
            Tool::Pointer => Shape::Pointer {
                at: p,
                radius: self.width.badge() / z,
            },
            Tool::Blur => Shape::Blur {
                points: vec![p],
                size: self.blur_size.unwrap_or(Width::Medium).brush() / z,
                cell: self.blur_cell.cell() / z,
                seed: blur::seed(),
            },
            Tool::Redact => Shape::Redact { a: p, b: p },
            Tool::Spotlight => Shape::Spotlight { a: p, b: p },
            Tool::Magnifier => Shape::Magnifier {
                at: p,
                radius: 60.0 / z,
            },
        };
        let width = match self.tool {
            Tool::Highlighter => self.stroke * 4.0,
            _ => self.stroke,
        } / z;
        Item {
            shape,
            colour: self.colour,
            width,
            outline: self.outline,
            fill: self.fill,
        }
    }

    /// The colour/width picked while an item is selected (or typed) applies to it.
    fn restyle(&mut self) {
        let styled = |item: &Item, editor: &Annotator| {
            let mut new = editor.create(Point::ORIGIN);
            new.shape = item.shape.clone();
            let z = editor.zoom;
            match &mut new.shape {
                Shape::Text { size, style, .. } => {
                    *size = editor.text_size / z;
                    *style = editor.text_style;
                }
                Shape::Callout { size, .. } | Shape::Label { size, .. } => {
                    *size = editor.text_size * 0.75 / z
                }
                Shape::Step { radius, .. } => *radius = editor.width.badge() / z,
                Shape::Pointer { radius, .. } => {
                    *radius = editor.width.badge() / z;
                    new.width = editor.stroke / z;
                }
                Shape::Highlighter(_) => new.width = editor.stroke * 4.0 / z,
                // Auto keeps the size it was given (its line); a picked one applies.
                Shape::Blur { size, cell, .. } => {
                    *cell = editor.blur_cell.cell() / z;
                    new.width = item.width;
                    if let Some(w) = editor.blur_size {
                        *size = w.brush() / z;
                    }
                }
                _ => new.width = editor.stroke / z,
            }
            new
        };
        if let Some(pending) = &self.pending {
            let item = styled(&pending.item, self);
            if let Some(pending) = &mut self.pending {
                pending.item = item;
            }
            self.update_live();
        } else if let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) {
            let before = self.doc.items[i].clone();
            let after = styled(&before, self);
            if after != before {
                self.doc.exec(Edit::Change(i, before, after));
                self.refresh();
            }
        }
    }

    /// Repaints the base (all items but a hidden pending one, the legend), then
    /// the live layer.
    pub(crate) fn refresh(&mut self) {
        let hide = self
            .pending
            .as_ref()
            .and_then(|p| p.index.filter(|_| p.hidden));
        let layers: Vec<_> = (self.doc.items.iter().enumerate())
            .map(|(i, item)| (item, Some(i) != hide))
            .collect();
        self.legend = self.doc.legend_layout();
        if self.bare() {
            return; // nothing to paint: the frozen frame under it is the area
        }
        let pm = self.doc.flatten(&layers, self.legend.as_ref());
        self.base = handle(pm);
        self.update_live();
    }

    /// Snip's live area with nothing on it: the overlay's frozen frame under
    /// the area already shows its pixels, so no base is painted or drawn (a
    /// move or resize re-painted and re-uploaded the whole area every frame).
    pub(crate) fn bare(&self) -> bool {
        self.movable && !self.own_image && self.doc.items.is_empty() && self.pending.is_none()
    }

    /// Repaints the pending item's region with it in place.
    fn update_live(&mut self) {
        self.live = self.pending.as_ref().map(|pending| {
            let mut layers: Vec<_> = self.doc.items.iter().map(|item| (item, true)).collect();
            match pending.index {
                Some(i) => layers[i] = (&pending.item, true),
                None => layers.push((&pending.item, true)),
            }
            let view = self.doc.view();
            // A spotlight changes the dim everywhere.
            // ponytail: repaints the whole view per pointer move while a spotlight
            // is dragged; dim only its old/new boxes if that is slow on 4K+.
            let area = match pending.item.shape {
                Shape::Spotlight { .. } => view,
                _ => pending.item.bounds(),
            };
            let r = int_rect(area, view);
            let rect = Rectangle::new(
                Point::new(r.x() as f32, r.y() as f32),
                Size::new(r.width() as f32, r.height() as f32),
            );
            let pm = self.doc.paint(&layers, r, self.legend.as_ref());
            (rect, handle(pm))
        });
    }
}

/// The editor's options as last changed in this run (the next editor opens on
/// them even before the file is written), else the config's.
static LAST: Mutex<Option<EditorConfig>> = Mutex::new(None);
/// A write of `LAST` is waiting (`remember`).
static WRITE_DUE: AtomicBool = AtomicBool::new(false);
/// Changes within this long are written once.
const SAVE_AFTER: std::time::Duration = std::time::Duration::from_millis(500);

fn remembered() -> EditorConfig {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    last.unwrap_or_else(|| crate::shared::config::current().editor)
}

/// Remembers the editor's options (`[editor]`) for the next editor: at once in
/// memory; the file through the one writer, off the UI thread, `SAVE_AFTER`
/// the first of a burst of changes (one write for all of them).
fn remember(options: EditorConfig) {
    if cfg!(test) {
        return; // never the user's config file, nor another test's editor
    }
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(options);
    if WRITE_DUE.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(SAVE_AFTER);
        WRITE_DUE.store(false, Ordering::Release);
        // Read under the writer's lock: the last write always has the newest.
        let written = crate::shared::config::update(|c| {
            if let Some(last) = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                c.editor = last;
            }
        });
        if let Err(e) = written {
            crate::log!("[opencircle] config: {e}");
        }
    });
}

/// A click at `p` with `tool` edits this item's text: one of the tool's own
/// kind (an arrow: on its label or tail, elsewhere it draws a new one).
fn edits(tool: Tool, item: &Item, p: Point, reach: f32) -> bool {
    match (tool, &item.shape) {
        (Tool::ArrowLabel, Shape::Arrow { a, text, .. }) => {
            let size = label::arrow_size(item.width);
            a.distance(p) <= reach.max(size)
                || (!text.is_empty()
                    && label::bubble(label::centred(*a, text, size), text, size).contains(p))
        }
        (tool, shape) => matches!(
            (tool, shape),
            (Tool::Text, Shape::Text { .. })
                | (Tool::Label, Shape::Label { .. })
                | (Tool::Callout, Shape::Callout { .. })
                | (Tool::Step, Shape::Step { .. })
        ),
    }
}

/// The text a shape is typed into, if it has one.
fn text_mut(shape: &mut Shape) -> Option<&mut String> {
    match shape {
        Shape::Text { text, .. }
        | Shape::Label { text, .. }
        | Shape::Callout { text, .. }
        | Shape::Arrow { text, .. } => Some(text),
        Shape::Step { note, .. } => Some(note),
        _ => None,
    }
}

/// The image's colour at `p` (opaque), or `None` outside it.
fn sample(image: &Image, p: Point) -> Option<[u8; 4]> {
    let (x, y) = (p.x.floor(), p.y.floor());
    if x < 0.0 || y < 0.0 || x >= image.width as f32 || y >= image.height as f32 {
        return None;
    }
    let i = (y as usize * image.width as usize + x as usize) * 4;
    let px = image.rgba.get(i..i + 3)?;
    Some([px[0], px[1], px[2], 255])
}

/// The new item `item` (started at `start`) dragged to `p`; `shift` keeps lines
/// and arrows at 0/45/90°.
fn grow(item: &Item, start: Point, p: Point, zoom: f32, shift: bool) -> Item {
    let mut item = item.clone();
    match &mut item.shape {
        Shape::Rect { b, .. }
        | Shape::Ellipse { b, .. }
        | Shape::Redact { b, .. }
        | Shape::Spotlight { b, .. } => *b = p,
        Shape::Arrow { a, b, bend, .. } => {
            *b = if shift { arrow::snap_angle(*a, p) } else { p };
            *bend = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        }
        Shape::Blur { points, .. } => {
            // The span is press..pointer: sliding back un-blurs what it left.
            let dir = points.last().map_or(0.0, |l| l.x - start.x).signum();
            while points.len() > 1 && points.last().is_some_and(|l| (l.x - p.x) * dir > 0.0) {
                points.pop();
            }
            if points
                .last()
                .is_none_or(|last| last.distance(p) * zoom >= 1.0)
            {
                points.push(p);
            }
        }
        Shape::Pen(points) | Shape::Highlighter(points) => {
            if points
                .last()
                .is_none_or(|last| last.distance(p) * zoom >= 1.0)
            {
                points.push(p);
            }
        }
        Shape::Text { .. } | Shape::Label { .. } => {}
        Shape::Step { at, .. } | Shape::Callout { at, .. } | Shape::Pointer { at, .. } => *at = p,
        Shape::Magnifier { radius, .. } => *radius = start.distance(p).max(20.0 / zoom),
    }
    item
}

/// A painted pixmap as an iced image (premultiplied = straight: the image is opaque).
fn handle(pm: tiny_skia::Pixmap) -> image::Handle {
    image::Handle::from_rgba(pm.width(), pm.height(), pm.take())
}

fn hex([r, g, b, _]: [u8; 4]) -> String {
    Rgb([r, g, b]).hex(true)
}

/// `#RRGGBB` (the `#` optional) as an opaque colour.
pub(crate) fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let Rgb([r, g, b]) = Rgb::parse(&format!("#{}", s.trim().trim_start_matches('#')))?;
    Some([r, g, b, 255])
}

/// `r` on whole image px (the captured pixels are whole).
fn whole(r: Rectangle) -> Rectangle {
    let (x0, y0) = (r.x.round(), r.y.round());
    let (x1, y1) = ((r.x + r.width).round(), (r.y + r.height).round());
    Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
}
