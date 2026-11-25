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
mod eraser;
pub use eraser::SIZES as ERASER_SIZES;
mod highlighter;
mod input;
mod inside;
mod label;
mod legend;
mod lines;
mod magnifier;
mod objects;
mod pen;
mod pointer;
mod select;
mod shape;
mod spotlight;
pub(crate) mod step;
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
mod blur_look_tests;
#[cfg(test)]
mod blur_shape_tests;
#[cfg(test)]
mod blur_speed_tests;
#[cfg(test)]
mod blur_strength_tests;
#[cfg(test)]
mod blur_tests;
#[cfg(test)]
mod inside_tests;
#[cfg(test)]
mod objects_api_tests;
#[cfg(test)]
mod objects_grow_tests;
#[cfg(test)]
mod objects_more_tests;
#[cfg(test)]
mod objects_settings_tests;
#[cfg(test)]
mod objects_tests;
#[cfg(test)]
mod tests;
mod text;
mod wheel;
pub use wheel::WHEEL_PX;
#[cfg(test)]
mod w111_tests;
#[cfg(test)]
mod wheel_tests;

pub use document::rounded as rounded_path;
pub use document::{
    Document, Edit, Image, Item, Shape, base_of, clean, hidden, hidden_in_clean, render, span,
};
pub use objects::{GrowRefusal, Order, clear_clipboard};
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

use crate::shared::config::{EditorConfig, Rgb};

use crate::shared::config::{RECENT_COLOURS as RECENT, remember_recent};
use crate::shared::picture::Crop;
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
    /// Takes drawings off, never the picture (`eraser`).
    Eraser,
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
    // no key ('\0'): X is the Eraser's; the area's handles crop (Studio: its rail)
    tool(Tool::Crop, '\u{e0af}', S::tl_crop, '\0', Group::Select),
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
    // last (Studio round 2): in the Draw group's panel after the highlighter
    tool(Tool::Eraser, '\u{e28e}', S::tl_eraser, 'x', Group::Draw),
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

/// How much bigger than a 1080p snip a picture whose short edge is `short` image px is
/// (1 up to 1080 px): what makes a size that is fixed in image px (the legend's text) look
/// the same on an 8K photo as on a 1080p snip. Every other default size (stroke, text, step
/// marker, blur cell, handle reach) is in display px, divided by the zoom, so it follows
/// the picture by itself.
pub(crate) fn picture_scale(short: f32) -> f32 {
    (short / 1080.0).max(1.0)
}

/// The reference text height Hide's strength is a fraction of (image px at DPI
/// scale 1).
pub const STRENGTH_TEXT: f32 = 16.0;
/// The Studio's custom strength: its range, in the same px.
pub const STRENGTH_RANGE: std::ops::RangeInclusive<u8> = 2..=32;

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
    /// Hide's strength: Light, Medium, Strong = 0.25, 0.5, 1.0 of the
    /// reference text height (16 px at the capture's DPI scale; owner,
    /// 2026-11-24): the texture scale of Hide and Pixelate, the sigma of the
    /// looks. Image px at scale 1; the capture's scale is applied by the editor.
    pub(crate) fn cell(self) -> f32 {
        STRENGTH_TEXT * [0.25, 0.5, 1.0][self as usize]
    }
}

/// The panels by the toolbar (one at a time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    /// Colour, eyedropper, presets, copy / paste style.
    Colour,
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
    /// A shape's, arrow's or callout's outline (its halo) on / off.
    Outline(bool),
    /// A rectangle's or ellipse's tint inside on / off.
    Fill(bool),
    /// The blur brush's size: `None` = Auto (the text line it starts on).
    BlurSize(Option<Width>),
    /// The blur's pixel size (its mosaic cells).
    BlurCell(Width),
    /// The Studio's custom strength (`None`: back to the presets).
    BlurCustom(Option<u8>),
    /// The next style in the list for the NEXT Hide (Shift+B).
    BlurCycle,
    /// Hide's shape for the next one drawn.
    BlurShape(blur::Kind),
    /// Hide's look: for the next one drawn, and the selected one.
    BlurStyle(blur::Style),
    /// Hide the selected annotation: a rectangle, ellipse, pen or arrow gets
    /// a Hide over its area (`true`: only along its outline).
    HideShape(bool),
    /// The eraser's brush (display px, `ERASER_SIZES`) and "Whole drawing".
    EraserSize(f32),
    EraserWhole(bool),
    /// The Text tool's options (its button's strip): font px on screen, and
    /// font, weight, backdrop.
    TextSize(f32),
    TextStyle(TextStyle),
    /// The picker's custom colour field.
    Hex(String),
    /// The picker's square or hue slider is being dragged (not written, not in the recent
    /// colours yet), and the drag ended; a key moved it (a finished change).
    /// `[hue, saturation, brightness]` and the RGB colour it is (the picker works it out).
    Hsv([f32; 3], [u8; 3]),
    HsvDone,
    HsvSet([f32; 3], [u8; 3]),
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
    /// Studio (S1, S7): select all, deselect, Ctrl+C / Ctrl+X on the selected
    /// objects (nothing selected: Ctrl+C = the picture), Ctrl+D, arrows
    /// (direction, Shift = 10 px), Ctrl+] / Ctrl+[, Alt+press (a copy is
    /// what moves), "Merge into picture".
    SelectAll,
    Deselect,
    CopySelection,
    CutSelection,
    Duplicate,
    Shove((i8, i8), bool),
    Order(Order),
    PressCopy(Point),
    Merge,
    /// Studio (right-click, Library rows): select what is at a point / at an
    /// index in `doc.items` (`true`: add to the selection), hide / show an
    /// item, move item `from` to place `to`.
    /// Ctrl+Enter in the Studio: the picture goes to the Steps (`Output::AddStep`).
    AddToSteps,
    SelectAt(Point, bool),
    /// Ctrl+wheel at a point (image px): resizes the selection when over it.
    WheelResize(Point, f32),
    // (the Objects list is gone in v3.1: the editor's API keeps these, its tests use them)
    #[allow(dead_code)]
    SelectIndex(usize, bool),
    #[allow(dead_code)]
    ToggleHidden(usize),
    #[allow(dead_code)]
    Reorder {
        from: usize,
        to: usize,
    },
    /// Studio's Properties: the selection's box (image px), a pasted
    /// picture's opacity (0-255) and ratio lock.
    Bounds(Rectangle),
    Opacity(u8),
    LockRatio(bool),
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
    /// More: this picture, its drawings still editable, into OpenCircle Studio.
    OpenStudio,
    /// The share sheet's user picked a target (the host's answer to
    /// `Output::Sharing`).
    Shared,
    Back,
    /// The toolbar's quiet x (C6): the same as Esc, Snip ends.
    Close,
    /// A press on the toolbar's grip: the host drags the toolbar (the editor
    /// ignores it).
    Grab,
    /// An arrow key on Snip's live area (`area::nudge`): direction, Ctrl,
    /// Shift, Alt.
    Nudge((i8, i8), (bool, bool, bool)),
    /// Snip's toolbar: Tab / Shift+Tab (`true`) walks its controls; Enter /
    /// Space presses the focused one (the host knows the toolbar's order).
    Focus(bool),
    /// An arrow key while the focus is in the options row (dx, dy): the toolbar turns it into a Tab step.
    RowStep(i8, i8),
    /// The More menu's arrow keys: the next row (`true`: the one before).
    MenuStep(bool),
    /// Home / End in More: the first (`false`) or last (`true`) live row.
    MenuEdge(bool),
    Activate,
}

/// Snip's screen tools on the frozen screen (its toolbar's buttons; the
/// host runs them, `features::snip::screens::tools`).
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
    /// The toolbar's x: Snip ends, as with Esc (C6).
    Close,
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
    /// The Studio's picture for the Steps, not yet flattened (a copy of the document: flattening an
    /// 8K picture takes 130-180 ms, so the host does it on a thread).
    AddStepDoc(Box<Document>),
    /// The share sheet is up: fires once the user picked a target (send
    /// `Message::Shared` then); cancelled if the sheet goes without one.
    Sharing(iced::futures::channel::oneshot::Receiver<()>),
    /// The image was dragged into another app (the overlay is already hidden),
    /// or a share target took it: close, as after Copy.
    Dragged,
    /// More's Open in Studio: the host moves this editor into the Studio.
    OpenStudio,
    /// Ctrl+V in Snip: the host reads the clipboard's picture on a thread and answers with
    /// `Annotator::paste_picture_or_text`.
    Paste(Option<Point>),
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

/// The style cards' pictures and what they were made for (place, strength).
type Thumbs = (ThumbKey, Vec<image::Handle>);
/// (place, strength, the drawing's history state, the item being drawn's frame): a picture of
/// the area is made again when any of them changes, so a card never shows bare pixels under a Hide.
type ThumbKey = (i32, i32, u32, u64, [u32; 4]);

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
    /// The picker's square and slider: the colour with its hue (a grey has none in RGB).
    hsv: Option<([f32; 3], [u8; 3])>,
    /// The colour before the picker's last change (its preview's "old" half).
    picked_from: [u8; 4],
    /// A drag on the picker is on.
    hsv_drag: bool,
    /// Snip: the selected item as it was when the picker's drag began (one drag = one undo step).
    drag_item: Option<(usize, Item)>,
    /// The Studio's selection as it was when the picker's drag began (the same, for `set_items`).
    drag_items: Option<Vec<Item>>,
    /// The colours the user picked lately, newest first (remembered, `RECENT`).
    pub(crate) recent: Vec<[u8; 3]>,
    /// Display px per image px.
    pub(crate) zoom: f32,
    shift: bool,
    /// The next press picks a colour from the image.
    pub(crate) picking: bool,
    /// Copied style: colour, weight.
    style: Option<([u8; 4], Width)>,
    pub(crate) base: image::Handle,
    /// The Studio's copy of `base` scaled to about what the stage shows (a big picture at a small zoom:
    /// the CPU renderer redraws a downscaled 8K picture from its full pixels every frame, 25 ms, and
    /// 1 ms from this); `None`: draw `base`. Cleared whenever `base` is repainted.
    pub(crate) display: Option<image::Handle>,
    /// Draw the picture with the renderer's pixel-for-pixel fast path (nearest, clipped rows): the
    /// Studio sets it when the picture lands 1:1 or from a copy of about its shown size, where it
    /// looks the same and costs a copy of the visible rows (a linear draw of an 8K picture: 40 ms).
    pub(crate) crisp: bool,
    /// The legend as the base shows it (placed among the committed items).
    legend: Option<legend::Legend>,
    /// The pending item's region (image px) painted with it.
    pub(crate) live: Option<(Rectangle, image::Handle)>,
    /// Hides committed since the picture was last painted whole: their region
    /// of it, drawn over `base` (see `patch_over`; see-through where unpainted).
    pub(crate) patch: Vec<(Rectangle, image::Handle)>,
    pending: Option<Pending>,
    /// Crop drag: from, to.
    cropping: Option<(Point, Point)>,
    /// The PNG of the last drag-out (deleted with the editor).
    dragged: Option<dragout::DragFile>,
    /// The last share (its PNG and the sheet's handlers; go with the editor).
    shared: Option<crate::features::circle::actions::share::Shared>,
    /// Line help: a blur stroke keeps to the line it started on (config
    /// `editor.line_assist`).
    pub(crate) assist: bool,
    /// The Studio's magnet (Settings > Studio > Snap): moving and drawing land
    /// on the picture's edges and other objects' edges, within `SNAP`.
    pub snap: bool,
    /// A menu, sheet or narrow overlay of the Studio window is up: the canvas
    /// takes no key at all (the window has them).
    pub keys_blocked: bool,
    /// Studio settings (`[studio]`): a pasted picture stays a movable object;
    /// a paste or move past the edge grows the canvas; the file a drag-out
    /// writes is a JPEG.
    pub paste_movable: bool,
    pub grow_canvas: bool,
    pub drag_jpeg: bool,
    /// Why the last "grow the canvas" was refused (`take_grow_refusal`).
    grow_refused: Option<GrowRefusal>,
    /// The growth budgets (`objects::GROW_PIXELS`, `GROW_HISTORY`; tests make them small).
    pub(super) grow_pixels: f64,
    pub(super) grow_history: usize,
    /// The part of the picture the stage shows (display px from its top-left),
    /// so the pixel grid draws only that; `None` = all of it.
    pub viewport: Option<Rectangle>,
    /// The Studio's pixel grid (drawn by the canvas at 800 % and over).
    pub grid: bool,
    /// Scrolling capture can run for this editor (the host sets it; the
    /// toolbar button is dimmed without).
    pub scroll: bool,
    /// Snip's first run ever: the toolbar shows its one-time "Edit more in OpenCircle Studio" hint.
    pub studio_hint: bool,
    /// A drag began on a shape with a drawing tool (it draws; Ctrl+drag moves): the host
    /// says "Ctrl+drag moves" once ever (`take_ctrl_hint`).
    ctrl_hint: bool,
    /// A press landed on a shape with a drawing tool: its first move is the drag that asks.
    ctrl_armed: bool,
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
    /// The Studio's custom strength (px at scale 1); `None`: the preset.
    pub(crate) blur_custom: Option<u8>,
    /// Where the pointer last was over the picture (image px): the style cards
    /// show what each style does to the area there.
    hover: Option<Point>,
    /// The style cards' pictures, made for (place, strength, style count).
    thumbs: std::cell::RefCell<Option<Thumbs>>,
    /// Hide's look (Frost, or the Pixelate mosaic).
    pub(crate) blur_style: blur::Style,
    /// Hide's shape: the free brush, a rectangle or an ellipse.
    pub(crate) blur_shape: blur::Kind,
    /// Hide's look was just picked: a restyle gives it to the selected Hides.
    style_picked: bool,
    /// Image px per display px of the capture (its monitor's DPI scale, or what
    /// the picture's size says): Hide's texture scale is this times its base size,
    /// never divided by the zoom.
    pub(crate) capture_scale: f32,
    /// The eraser's brush (display px) and "Whole drawing".
    pub(crate) eraser: f32,
    pub(crate) eraser_whole: bool,
    /// An eraser stroke: the items before it and the pointer's last point.
    erasing: Option<(Vec<Item>, Point)>,
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
    /// Where the options row's stops begin in the Tab ring (the toolbar's own come first): while the
    /// focus is in the row the arrow keys walk it. 0 = not known yet.
    pub row_at: usize,
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
    /// The Studio's editor: multi-selection, the object clipboard, options
    /// that follow the selection (Snip's quick widget keeps today's rules).
    pub studio: bool,
    /// Studio: Ctrl+C with nothing selected copies the picture (the
    /// `studio` setting, default on).
    pub copy_picture_when_empty: bool,
    group: Option<objects::GroupDrag>,
    band: Option<(Point, Point)>,
    /// Ctrl+wheel resizing in progress (`wheel.rs`).
    wheel: Option<wheel::Burst>,
    /// The next drawing's style while a selection shows its own values.
    stash: Option<objects::DrawStyle>,
    /// What the options message being handled changes (`objects::touch`).
    touch: u8,
}

impl Annotator {
    /// Opens `image`, shown at `zoom` display px per image px (e.g. 1 / the
    /// monitor's scale factor, so the crop shows at its on-screen size), with
    /// the user's last tool and options (owner, 2026-10-31).
    pub fn new(image: Image, zoom: f32) -> Self {
        Self::restored(image, zoom, remembered())
    }

    /// `new` with the base already painted (`document::base_of`, made off the UI thread: the
    /// flatten of an 8K picture is 130-180 ms).
    pub fn new_with_base(image: Image, zoom: f32, base: image::Handle) -> Self {
        Self::restored_with(image, zoom, remembered(), Some(base))
    }

    /// `new` with `saved`'s tool and options (a missing or bad one: its default).
    pub(crate) fn restored(image: Image, zoom: f32, saved: EditorConfig) -> Self {
        Self::restored_with(image, zoom, saved, None)
    }

    fn restored_with(
        image: Image,
        zoom: f32,
        saved: EditorConfig,
        base: Option<image::Handle>,
    ) -> Self {
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
        let capture_scale = (1.0 / zoom).max(picture_scale(image.width.min(image.height) as f32));
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
            hsv: None,
            picked_from: colour,
            hsv_drag: false,
            drag_item: None,
            drag_items: None,
            recent: (saved.recent.iter().flatten())
                .filter_map(|s| parse_hex(s))
                .map(|[r, g, b, _]| [r, g, b])
                .take(RECENT)
                .collect(),
            zoom,
            shift: false,
            picking: false,
            style: None,
            base: image::Handle::from_rgba(1, 1, vec![0; 4]),
            display: None,
            crisp: false,
            legend: None,
            live: None,
            patch: Vec::new(),
            pending: None,
            cropping: None,
            dragged: None,
            shared: None,
            assist: saved.line_assist,
            snap: false,
            keys_blocked: false,
            paste_movable: true,
            grow_canvas: false,
            drag_jpeg: false,
            grow_refused: None,
            grow_pixels: objects::GROW_PIXELS,
            grow_history: objects::GROW_HISTORY,
            viewport: None,
            grid: false,
            scroll: false,
            studio_hint: false,
            ctrl_hint: false,
            ctrl_armed: false,
            capturing: false,
            lock: None,
            blur_size: saved.blur_size,
            blur_cell: saved.blur_cell.unwrap_or(Width::Medium),
            blur_custom: saved.blur_custom,
            hover: None,
            thumbs: std::cell::RefCell::new(None),
            blur_style: saved.blur_style.unwrap_or_default(),
            blur_shape: saved.blur_shape.unwrap_or_default(),
            style_picked: false,
            capture_scale,
            eraser: saved.eraser.map_or(ERASER_SIZES[1], f32::from),
            eraser_whole: saved.eraser_whole.unwrap_or(false),
            erasing: None,
            bands: None,
            movable: false,
            edges: Vec::new(),
            own_image: false,
            area_drag: None,
            focus: None,
            row_at: 0,
            nudged: false,
            held: false,
            steps: None,
            updating: None,
            studio: false,
            copy_picture_when_empty: true,
            group: None,
            band: None,
            wheel: None,
            stash: None,
            touch: 0,
            saved,
        };
        match base {
            Some(base) => editor.base = base,
            None => editor.refresh(),
        }
        editor
    }

    /// Settings > Studio > "Reset tool styles": this editor and the next ones take
    /// `saved`'s tool and options (the config's defaults).
    pub(crate) fn reset_options(&mut self, saved: EditorConfig) {
        let fresh = Self::restored(
            Image {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
            1.0,
            saved,
        );
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = None;
        (self.tool, self.colour, self.width, self.stroke) =
            (fresh.tool, fresh.colour, fresh.width, fresh.stroke);
        (self.last, self.outline, self.fill) = (fresh.last, fresh.outline, fresh.fill);
        (self.text_size, self.text_style, self.hex) =
            (fresh.text_size, fresh.text_style, fresh.hex);
        (self.assist, self.blur_size, self.blur_cell) =
            (fresh.assist, fresh.blur_size, fresh.blur_cell);
        (self.blur_style, self.blur_shape, self.blur_custom) =
            (fresh.blur_style, fresh.blur_shape, fresh.blur_custom);
        (self.eraser, self.eraser_whole, self.saved) =
            (fresh.eraser, fresh.eraser_whole, fresh.saved);
    }

    /// The tool and options to remember: `saved` with this editor's own.
    pub(crate) fn remembered(&self) -> EditorConfig {
        let d = self.drawing_style();
        EditorConfig {
            line_assist: self.assist,
            // Crop is a step, not a tool to open on.
            tool: (self.tool != Tool::Crop)
                .then_some(self.tool)
                .or(self.saved.tool),
            colour: Some(hex(d.colour)),
            recent: (!self.recent.is_empty()).then(|| {
                (self.recent.iter())
                    .map(|&[r, g, b]| hex([r, g, b, 255]))
                    .collect()
            }),
            size: Some(d.width),
            text_size: Some(d.text_size.round().clamp(8.0, 96.0) as u8),
            outline: Some(d.outline),
            fill: Some(d.fill),
            blur_size: d.blur_size,
            blur_cell: Some(d.blur_cell),
            stroke: Some(d.stroke.round().clamp(1.0, 32.0) as u8),
            text_font: Some(d.text_style.font),
            text_bold: Some(d.text_style.bold),
            text_back: Some(d.text_style.back),
            group_tools: Some(self.last.to_vec()),
            eraser: Some(self.eraser.round().clamp(1.0, 255.0) as u8),
            eraser_whole: Some(self.eraser_whole),
            blur_style: Some(self.blur_style),
            blur_shape: Some(self.blur_shape),
            blur_custom: self.blur_custom,
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
        let picked = self
            .studio
            .then(|| (self.doc.selected, self.doc.extra.clone()));
        self.touch = objects::touch::of(&message);
        let out = self.handle(message);
        self.touch = 0;
        if let Some(picked) = picked
            && picked != (self.doc.selected, self.doc.extra.clone())
        {
            self.sync_style();
        }
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
                | Message::RowStep(..)
                | Message::MenuStep(_)
                | Message::MenuEdge(_)
                | Message::Activate
        ) {
            self.undid = false;
        }
        match message {
            Message::Press(p) => {
                self.ctrl_armed = false;
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
                    Some(i)
                        if self.studio
                            && self.doc.selection().len() > 1
                            && self.doc.selection().contains(&i) =>
                    {
                        self.start_group(p)
                    }
                    Some(i) => {
                        self.doc.select_one(Some(i));
                        self.pick_up(i, false, p);
                    }
                    None => self.press(p),
                }
            }
            Message::PressCopy(p) => {
                (self.focus, self.nudged) = (None, false);
                self.press_copy(p)
            }
            Message::Move(p) => {
                self.ctrl_hint |= std::mem::take(&mut self.ctrl_armed);
                self.hover = Some(p);
                self.drag_to(p)
            }
            Message::Release => {
                self.ctrl_armed = false;
                self.release()
            }
            Message::Cancel => self.cancel_press(),
            Message::Shift(on) => self.shift = on,
            Message::Type(s) => self.edit_text(|t| t.push_str(&s)),
            // typing, or the Studio (it read the picture itself, off the UI thread): the
            // text; Snip: the host reads the clipboard's picture off the UI thread
            Message::Paste(at) if self.typing() || self.studio => self.paste_text(at),
            Message::Paste(at) => return Some(Output::Paste(at)),
            Message::Backspace => self.edit_text(|t| {
                t.pop();
            }),
            Message::Finish => self.commit(),
            Message::Tool(tool) => self.pick_tool(tool),
            // The group's tool; on the active group nothing more: its choices and options are
            // in the row under the toolbar already (W107), no panel, no second click.
            Message::Slot(tool) if info(self.tool).group != info(tool).group => {
                self.pick_tool(tool)
            }
            Message::Slot(_) => {
                self.menu = None;
                self.picking = false;
            }
            Message::Picker => self.toggle(Menu::Colour),
            Message::Menu(menu) => self.toggle(menu),
            Message::Colour(c) => self.set_colour(c),
            Message::Width(w) => {
                self.set_width(w);
                self.restyle();
            }
            Message::BlurSize(size) => {
                self.blur_size = size;
                self.restyle();
            }
            Message::BlurCell(cell) => {
                self.blur_cell = cell;
                self.blur_custom = None;
                self.restyle();
            }
            Message::BlurCustom(px) => {
                self.blur_custom = px;
                self.restyle();
            }
            Message::BlurCycle => {
                let all = blur_styles();
                let at = all.iter().position(|(s, _)| *s == self.blur_style);
                // the next Hide only: an existing one is restyled by a click on a style
                // card, never silently (a secure Hide must not turn into a look)
                self.blur_style = all[at.map_or(0, |i| (i + 1) % all.len())].0;
            }
            Message::BlurShape(kind) => self.blur_shape = kind,
            Message::BlurStyle(style) => {
                self.blur_style = style;
                self.style_picked = true;
                self.restyle();
                self.style_picked = false;
            }
            Message::HideShape(outline) => self.hide_shape(outline),
            Message::EraserSize(px) => self.eraser = px,
            Message::EraserWhole(on) => self.eraser_whole = on,
            Message::OpenStudio if self.capturing => {}
            Message::OpenStudio => {
                self.commit();
                self.menu = None;
                return Some(Output::OpenStudio);
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
                    self.picked_from = self.colour;
                    self.colour = c;
                    self.hsv = None;
                    self.restyle();
                    self.remember_colour();
                }
                self.hex = s;
            }
            Message::Hsv(hsv, rgb) => self.set_hsv(hsv, rgb),
            Message::HsvDone => self.end_hsv_drag(),
            Message::HsvSet(hsv, rgb) => {
                self.set_hsv(hsv, rgb);
                self.end_hsv_drag();
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
                    self.style = Some((item.colour, style::level(item, self.size_zoom())));
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
            Message::AddToSteps => {
                self.commit();
                self.menu = None;
                return Some(match self.studio {
                    true => Output::AddStepDoc(Box::new(self.doc.clone())),
                    false => Output::AddStep(render(&self.doc)),
                });
            }
            Message::DragOut => {
                self.commit();
                let flat = render(&self.doc);
                // The previous drag's file goes first (one temp file at a time).
                self.dragged = None;
                match dragout::DragFile::write_as(&flat, self.drag_jpeg) {
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
                match crate::features::circle::actions::share::start(&flat) {
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
            Message::Delete if self.studio => {
                self.commit();
                self.delete_selection();
            }
            Message::SelectAll => self.select_all(),
            Message::Deselect => {
                self.commit();
                self.doc.deselect();
            }
            Message::CopySelection | Message::CutSelection => {
                return self.copy_selection(matches!(message, Message::CutSelection));
            }
            Message::Duplicate => self.duplicate(),
            Message::Shove(arrow, far) => self.shove(arrow, far),
            Message::Order(how) => self.reorder(how),
            Message::Merge => self.merge(),
            Message::SelectAt(p, add) => self.select_at(p, add),
            Message::WheelResize(p, n) => {
                self.wheel_resize(p, n, std::time::Instant::now());
            }
            Message::SelectIndex(i, add) => self.select_index(i, add),
            Message::ToggleHidden(i) => self.toggle_hidden(i),
            Message::Reorder { from, to } => self.reorder_to(from, to),
            Message::Bounds(r) => self.set_selection_bounds(r),
            Message::Opacity(o) => self.set_picture(Some(o), None),
            Message::LockRatio(on) => self.set_picture(None, Some(on)),
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
            Message::Close => {
                self.commit();
                return Some(Output::Close);
            }
            Message::Grab
            | Message::Focus(_)
            | Message::RowStep(..)
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
    pub(crate) fn typing(&self) -> bool {
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

    /// Picks `tool`, its group's last from now on; an open panel closes (the colour's too).
    fn pick_tool(&mut self, tool: Tool) {
        self.commit();
        self.tool = tool;
        let group = info(tool).group;
        if tool != Tool::Crop {
            self.last[group as usize] = tool;
        }
        self.menu = None;
        self.picking = false;
    }

    /// The weight; the line width and a text's size follow it.
    fn set_width(&mut self, w: Width) {
        self.width = w;
        self.stroke = w.stroke();
        self.text_size = w.font();
    }

    fn set_colour(&mut self, c: [u8; 4]) {
        if c != self.colour {
            self.picked_from = self.colour;
        }
        self.colour = c;
        self.hsv = None;
        self.hex = hex(c);
        self.restyle();
    }

    /// The picker's square or slider moved the colour to `hsv` (its `rgb`; the alpha stays).
    fn set_hsv(&mut self, hsv: [f32; 3], [r, g, b]: [u8; 3]) {
        if !self.hsv_drag {
            (self.hsv_drag, self.picked_from) = (true, self.colour);
        }
        self.hsv = Some((hsv, [r, g, b]));
        self.colour = [r, g, b, self.colour[3]];
        self.hex = hex(self.colour);
        self.restyle();
    }

    /// The picker's drag (or key) is over: the colour is remembered, and a selected item's change
    /// of the whole drag is ONE undo step.
    fn end_hsv_drag(&mut self) {
        self.hsv_drag = false;
        if let Some(before) = self.drag_items.take() {
            let after = std::mem::replace(&mut self.doc.items, before.clone());
            self.doc.exec(Edit::Items(before, after));
            self.refresh();
        }
        if let Some((i, before)) = self.drag_item.take()
            && i < self.doc.items.len()
        {
            let after = std::mem::replace(&mut self.doc.items[i], before.clone());
            self.doc.exec(Edit::Change(i, before, after));
            self.refresh();
        }
        self.remember_colour();
    }

    /// The colour now goes first in the recent colours.
    fn remember_colour(&mut self) {
        let [r, g, b, _] = self.colour;
        remember_recent(&mut self.recent, [r, g, b]);
    }

    /// The picker's `[hue, saturation, brightness]` when the colour is the one it set last (a grey
    /// or a black has no hue in RGB); `None`: the colour came another way (the picker works it out).
    pub(crate) fn hsv(&self) -> Option<[f32; 3]> {
        let [r, g, b, _] = self.colour;
        self.hsv
            .filter(|(_, rgb)| *rgb == [r, g, b])
            .map(|(h, _)| h)
    }

    /// The colour before the picker's last change.
    pub(crate) fn picked_from(&self) -> iced::Color {
        let [r, g, b, _] = self.picked_from;
        iced::Color::from_rgb8(r, g, b)
    }

    /// "Hide this shape": a Hide over the selected rectangle, ellipse, pen
    /// stroke or arrow (`outline`: a ring along its edge, not its inside),
    /// selected, on top (one undo step).
    fn hide_shape(&mut self, outline: bool) {
        // (an edit in progress ends first: it may change the items and the selection)
        self.commit();
        let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) else {
            return;
        };
        let item = &self.doc.items[i];
        let cell = self.blur_strength();
        let over = item.width + 4.0;
        // (what it covers: its points, brush size or box, whether a ring; an arrow's
        // label frame is a second Hide)
        let mut covers: Vec<(Vec<Point>, f32, blur::Kind, bool)> = Vec::new();
        match &item.shape {
            Shape::Rect { a, b } => covers.push((vec![*a, *b], over, blur::Kind::Rect, outline)),
            Shape::Ellipse { a, b } => {
                covers.push((vec![*a, *b], over, blur::Kind::Ellipse, outline))
            }
            Shape::Pen(points) | Shape::Highlighter(points) => {
                covers.push((points.clone(), over, blur::Kind::Brush, false))
            }
            Shape::Arrow {
                a,
                b,
                bend,
                head,
                text,
            } => {
                // along the curve as it is drawn (a quadratic through its bend),
                // wide enough for its head
                let curve = (0..=24)
                    .map(|k| {
                        let t = k as f32 / 24.0;
                        let (u, v, w) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t, t * t);
                        Point::new(
                            u * a.x + v * bend.x + w * b.x,
                            u * a.y + v * bend.y + w * b.y,
                        )
                    })
                    .collect();
                let wide = match head {
                    true => over.max(arrow::head_length(item.width) * 1.3),
                    false => over,
                };
                covers.push((curve, wide, blur::Kind::Brush, false));
                if !text.is_empty() {
                    let size = label::arrow_size(item.width);
                    let frame = label::bubble(label::centred(*a, text, size), text, size);
                    let r = frame.expand(2.0);
                    covers.push((
                        vec![
                            r.position(),
                            r.position() + iced::Vector::new(r.width, r.height),
                        ],
                        over,
                        blur::Kind::Rect,
                        false,
                    ));
                }
            }
            _ => return,
        }
        let (colour, width) = (self.colour, item.width);
        let at = self.doc.items.len();
        let inserts: Vec<Edit> = covers
            .into_iter()
            .enumerate()
            .map(|(n, (points, size, kind, ring))| {
                Edit::Insert(
                    at + n,
                    Item {
                        shape: Shape::Blur {
                            points,
                            size,
                            cell,
                            style: self.blur_style,
                            kind,
                            outline: if ring { over } else { 0.0 },
                            ramp: Vec::new(),
                        },
                        colour,
                        width,
                        outline: false,
                        fill: false,
                        hidden: false,
                    },
                )
            })
            .collect();
        match inserts.len() {
            1 => self.doc.exec(inserts.into_iter().next().unwrap()),
            _ => self.doc.exec(Edit::Seq(inserts)),
        }
        self.doc.select_one(Some(at));
        self.refresh();
    }

    /// One picture per style in `blur_styles()`: the area under the pointer
    /// (else the picture's middle), its left half as it is and its right half
    /// hidden in that style, at the strength now. Made again when the pointer
    /// moves on by 8 px or the strength changes.
    pub fn style_thumbs(&self) -> Vec<image::Handle> {
        let view = self.doc.view();
        let at = self.hover.unwrap_or_else(|| view.center());
        let strength = self.blur_strength();
        let drawn = (self.pending.as_ref()).map_or([0; 4], |p| {
            let f = p.item.frame();
            [f.x, f.y, f.width, f.height].map(f32::to_bits)
        });
        let key = (
            (at.x / 8.0) as i32,
            (at.y / 8.0) as i32,
            strength.to_bits(),
            self.doc.state(),
            drawn,
        );
        if let Some((k, all)) = &*self.thumbs.borrow()
            && *k == key
        {
            return all.clone();
        }
        let all = blur_styles()
            .iter()
            .map(|&(style, _)| self.style_thumb(style, at, strength))
            .collect::<Vec<_>>();
        *self.thumbs.borrow_mut() = Some((key, all.clone()));
        all
    }

    fn style_thumb(&self, style: blur::Style, at: Point, strength: f32) -> image::Handle {
        let k = self.capture_scale;
        let (w, h) = ((72.0 * k).ceil(), (44.0 * k).ceil());
        let view = self.doc.view();
        let from = Point::new(
            (at.x - w / 2.0).clamp(view.x, (view.x + view.width - w).max(view.x)),
            (at.y - h / 2.0).clamp(view.y, (view.y + view.height - h).max(view.y)),
        );
        let region = document::int_rect(Rectangle::new(from, iced::Size::new(w, h)), view);
        // the picture as the editor shows it (every item, as flatten draws it): the
        // half "as it is" never shows the bare pixels under an existing Hide
        // (the item being drawn too: a Hide under the pointer is already a Hide)
        let mut items = self.doc.items.clone();
        if let Some(p) = &self.pending {
            match p.index.filter(|i| *i < items.len()) {
                Some(i) => items[i] = p.item.clone(),
                None => items.push(p.item.clone()),
            }
        }
        let layers: Vec<_> = items.iter().map(|i| (i, true)).collect();
        let pm = self.doc.paint(&layers, region, None);
        let mut layer = document::Layer {
            pm,
            origin: Point::new(region.x() as f32, region.y() as f32),
        };
        let (x0, y0) = (region.x() as f32, region.y() as f32);
        let (rw, rh) = (region.width() as f32, region.height() as f32);
        let corners = [Point::new(x0 + rw / 2.0, y0), Point::new(x0 + rw, y0 + rh)];
        let ramp = [
            Point::new(x0 + rw / 2.0, y0 + rh / 2.0),
            Point::new(x0 + rw, y0 + rh / 2.0),
        ];
        let fp = blur::Footprint {
            points: &corners,
            size: 0.0,
            kind: blur::Kind::Rect,
            outline: 0.0,
            ramp: &ramp,
            // the picture as captured: Frost and Pixelate never read the annotations (W111)
            source: Some(&self.doc.image),
        };
        blur::paint(&mut layer, &fp, strength, style, self.doc.seed);
        handle(layer.pm)
    }

    /// Hide's strength now, image px: the preset's (or the custom slider's) times
    /// the capture's scale: never divided by the zoom.
    pub(crate) fn blur_strength(&self) -> f32 {
        self.blur_custom.map_or(self.blur_cell.cell(), f32::from) * self.capture_scale
    }

    /// What "Hide this shape" can cover now: the selection is a rectangle,
    /// ellipse (both: inside or outline), pen stroke, highlighter or arrow.
    pub fn hideable(&self) -> Option<bool> {
        let shape = &self.doc.items.get(self.doc.selected?)?.shape;
        match shape {
            Shape::Rect { .. } | Shape::Ellipse { .. } => Some(true),
            Shape::Pen(_) | Shape::Highlighter(_) | Shape::Arrow { .. } => Some(false),
            _ => None,
        }
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
                shape: Shape::Redact {
                    a,
                    b,
                    oval: matches!(before.shape, Shape::Ellipse { .. }),
                },
                colour,
                ..before.clone()
            };
            self.doc.exec(Edit::Change(i, before, after));
            self.refresh();
        }
        if !p.keeps.contains(&self.tool) {
            self.pick_tool(p.tool);
            self.doc.deselect(); // the style is for the next item
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
        let default = Width::Medium.brush() / self.size_zoom();
        if let Some(w) = self.blur_size {
            return (w.brush() / self.size_zoom(), None);
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
        if self.blur_shape != blur::Kind::Brush || self.blur_style == blur::Style::Progressive {
            return self.create(p);
        }
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
                self.remember_colour();
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
        if self.tool == Tool::Eraser {
            self.erasing = Some((self.doc.items.clone(), p));
            self.erase_to(p);
            return;
        }
        // The selected item's handles work in every tool (one selected item).
        if let Some(i) = self
            .doc
            .selected
            .filter(|&i| i < self.doc.items.len() && self.doc.extra.is_empty())
        {
            let original = self.doc.items[i].clone();
            if let Some(handle) = select::handle_at(&original, p, reach) {
                let drag = Drag::Handle(handle, original.clone());
                self.begin(Some(i), original, drag, p);
                return;
            }
        }
        if self.tool == Tool::Select && self.studio {
            let hit = select::hit(&self.doc.items, p, reach);
            match hit {
                Some(i) if self.shift => self.toggle_selected(i),
                Some(i) if self.doc.selection().len() > 1 && self.doc.selection().contains(&i) => {
                    self.start_group(p)
                }
                Some(i) => {
                    self.doc.select_one(Some(i));
                    self.pick_up(i, false, p);
                }
                None => {
                    self.doc.deselect();
                    self.band = Some((p, p));
                }
            }
            return;
        }
        if self.tool == Tool::Select {
            let hit = select::hit(&self.doc.items, p, reach);
            self.doc.select_one(hit);
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
            self.doc.select_one(Some(i));
            self.pick_up(i, true, p);
            return;
        }
        // a drawing tool draws over a shape; Ctrl+drag moves it (C5: said once)
        if self.tool != Tool::Crop && select::hit(&self.doc.items, p, reach).is_some() {
            self.ctrl_armed = true; // the hint waits for the drag (a plain click is no drag)
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
            (self.text_size, self.text_style) = (size * self.size_zoom(), *style);
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
        if self.group.is_some() {
            return self.drag_group(p);
        }
        if let Some((from, _)) = self.band {
            self.band = Some((from, p));
            return;
        }
        if self.erasing.is_some() {
            self.erase_to(p);
            return;
        }
        let size_zoom = self.size_zoom();
        if let Some(held) = &mut self.lock
            && !self.shift
        {
            let brush = match self.pending.as_ref().map(|p| &p.item.shape) {
                Some(Shape::Blur { size, .. }) => *size,
                _ => Width::Medium.brush() / size_zoom,
            };
            let before = *held;
            p = blur::follow(held, p, brush);
            // Once the stroke leaves its line it is a free brush (a ring, a
            // loop): the drag-back shrink belongs to a held line only.
            if *held != before {
                self.lock = None;
            }
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
            self.doc.deselect();
            let item = self.create(start);
            self.begin(None, item, Drag::Draw, start);
        }
        // the magnet: a drawn corner or a moved item's edges land on the picture's
        // and the other items' edges
        let (drawn_to, moved_by) = match (&self.pending, self.snap) {
            (Some(pd), true) => match &pd.drag {
                Drag::Draw => (self.snap_point(p, pd.index), None),
                Drag::Move { from, .. } => (p, Some(self.snap_move(from, p - pd.start, pd.index))),
                _ => (p, None),
            },
            _ => (p, None),
        };
        let (inside, view) = (self.keeps_inside(), self.doc.view());
        let Some(pending) = &mut self.pending else {
            return;
        };
        pending.item = match &pending.drag {
            Drag::Draw => grow(
                &pending.item,
                pending.start,
                drawn_to,
                self.zoom,
                self.shift,
                self.lock.is_some(),
            ),
            // A click that wobbles a pixel is not a move.
            Drag::Move { from, .. } if p.distance(pending.start) * self.zoom < SLOP => from.clone(),
            Drag::Move { from, .. } => select::moved(from, moved_by.unwrap_or(p - pending.start)),
            Drag::Handle(handle, original) => select::dragged(original, *handle, p),
            Drag::Type => return,
        };
        // where the picture cannot grow, a drawing stops at its last pixels
        if inside && matches!(pending.drag, Drag::Draw) {
            inside::keep_inside(&mut pending.item, view);
        }
        inside::fit_wrap(&mut pending.item, view, inside);
        if pending.hidden || pending.index.is_none() {
            self.update_live();
        } else {
            self.hide();
        }
    }

    /// What a snap lands on: the picture and every item but `skip`.
    fn snap_targets(&self, skip: Option<usize>) -> Vec<Rectangle> {
        let mut all = vec![self.doc.view()];
        all.extend(
            (self.doc.items.iter().enumerate())
                .filter(|(i, _)| Some(*i) != skip)
                .map(|(_, it)| it.frame()),
        );
        all
    }

    /// `p` on the nearest target edge within the snap reach.
    fn snap_point(&self, p: Point, skip: Option<usize>) -> Point {
        let reach = crate::features::snip::SNAP / self.zoom.max(0.01);
        crate::features::snip::snap(p, &self.snap_targets(skip), reach)
    }

    /// `by` (a moved item's offset) nudged so the item's left / right and top /
    /// bottom edges land on a target edge within the reach.
    fn snap_move(&self, from: &Item, by: Vector, skip: Option<usize>) -> Vector {
        let reach = crate::features::snip::SNAP / self.zoom.max(0.01);
        let f = from.frame();
        let targets = self.snap_targets(skip);
        let best = |mine: [f32; 2], theirs: Vec<f32>| {
            let mut best: Option<f32> = None;
            for m in mine {
                for t in &theirs {
                    let d = t - m;
                    if d.abs() <= reach && best.is_none_or(|b| d.abs() < b.abs()) {
                        best = Some(d);
                    }
                }
            }
            best.unwrap_or(0.0)
        };
        let xs = targets.iter().flat_map(|r| [r.x, r.x + r.width]).collect();
        let ys = targets.iter().flat_map(|r| [r.y, r.y + r.height]).collect();
        let dx = best([f.x + by.x, f.x + f.width + by.x], xs);
        let dy = best([f.y + by.y, f.y + f.height + by.y], ys);
        Vector::new(by.x + dx, by.y + dy)
    }

    /// A cancelled press (a lost touch) ends like a release that completes nothing:
    /// the area and the pending item stay as they were before the press.
    fn cancel_press(&mut self) {
        self.lock = None;
        self.band = None;
        if self.group.is_some() {
            self.cancel_group();
        } else if let Some((before, _)) = self.erasing.take() {
            self.doc.items = before;
            self.refresh();
        } else if let Some(drag) = self.area_drag.take() {
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
        if self.group.is_some() {
            return self.end_group();
        }
        if self.band.is_some() {
            return self.end_band();
        }
        if let Some(drag) = self.area_drag.take() {
            let to = self.doc.view();
            (self.doc.crop, self.doc.items) = (Some(drag.start), drag.items);
            self.set_area(drag.start, to, drag.grip == area::Grip::Move);
            if to == drag.start {
                self.refresh();
            }
            return;
        }
        if let Some((before, _)) = self.erasing.take() {
            // one undo brings back all the stroke erased
            if before != self.doc.items {
                let after = std::mem::replace(&mut self.doc.items, before.clone());
                self.doc.exec(Edit::Items(before, after));
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

    /// The eraser stroke on to `p` (image px): what its brush passed over goes.
    fn erase_to(&mut self, p: Point) {
        let Some((_, last)) = &mut self.erasing else {
            return;
        };
        let from = std::mem::replace(last, p);
        let r = self.eraser / 2.0 / self.zoom;
        if eraser::erase(&mut self.doc.items, from, p, r, self.eraser_whole) {
            self.doc.deselect();
            self.refresh();
        }
    }

    /// Ends the pending change: one undoable edit (or none if nothing changed).
    fn commit(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let mut item = pending.item;
        inside::fit_wrap(&mut item, self.doc.view(), self.keeps_inside());
        let empty = matches!(&item.shape,
            Shape::Text { text, .. } | Shape::Callout { text, .. } | Shape::Label { text, .. }
                if text.trim().is_empty());
        let unmoved_copy = matches!(&pending.drag, Drag::Move { from, .. } if *from == item);
        let edit = match pending.index {
            None if empty || unmoved_copy || self.too_small(&item) => None,
            None => Some(Edit::Insert(self.doc.items.len(), item)),
            Some(i) if empty => Some(Edit::Delete(i, self.doc.items[i].clone())),
            Some(i) if item != self.doc.items[i] => {
                Some(Edit::Change(i, self.doc.items[i].clone(), item))
            }
            Some(_) => None,
        };
        let repaint = edit.is_some() || pending.hidden;
        // A new Hide only paints over its own region, which the live layer has
        // just painted: patch the picture with it instead of flattening it all
        // (an 8K picture: ~150 ms).
        let mut patch = pending.index.is_none()
            && !pending.hidden
            && matches!(
                &edit,
                Some(Edit::Insert(
                    _,
                    Item {
                        shape: Shape::Blur { .. },
                        ..
                    }
                ))
            );
        if let Some(edit) = edit {
            let grown = match &edit {
                Edit::Insert(_, item) | Edit::Change(_, _, item) => self.grow_for(&[item.frame()]),
                _ => Ok(None),
            };
            patch &= matches!(grown, Ok(None));
            match grown {
                Ok(Some(grow)) => self.doc.exec(Edit::Seq(vec![edit, grow])),
                Ok(None) => self.doc.exec(edit),
                // refused: the item stays where it was put, clipped by the picture (a typed
                // text is never lost); the reason is kept for the window's pill
                Err(()) => self.doc.exec(edit),
            }
        }
        if patch && self.patch_over() {
            self.live = None;
        } else if repaint {
            self.refresh();
        } else {
            self.live = None;
        }
    }

    /// Keeps the live layer (it holds every item over its region: what a full
    /// repaint gives there) as one more local patch over the picture, instead of
    /// flattening the whole picture again; the next repaint folds them in. Each
    /// patch is only its own region, so two Hides far apart never allocate the
    /// space between them. `false`: it can't (no live layer, no base shown, a
    /// legend, or `MAX_PATCHES` already), then repaint.
    fn patch_over(&mut self) -> bool {
        let (Some(live), false, None) = (&self.live, self.bare(), &self.legend) else {
            return false;
        };
        if self.patch.len() >= MAX_PATCHES {
            return false;
        }
        self.patch.push(live.clone());
        true
    }

    /// The painted picture's bytes (the tests).
    #[cfg(test)]
    fn base_pixels(&self) -> Vec<u8> {
        match &self.base {
            image::Handle::Rgba { pixels, .. } => pixels.to_vec(),
            _ => Vec::new(),
        }
    }

    /// The committed Hides' patches: each one's place (image px) and bytes (the tests).
    #[cfg(test)]
    fn patch_pixels(&self) -> Vec<(Rectangle, Vec<u8>)> {
        (self.patch.iter())
            .filter_map(|(r, h)| match h {
                image::Handle::Rgba { pixels, .. } => Some((*r, pixels.to_vec())),
                _ => None,
            })
            .collect()
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
                | Shape::Blur {
                    kind: blur::Kind::Rect | blur::Kind::Ellipse,
                    ..
                }
        )
    }

    /// Ctrl+V found `clip` (a picture) on the clipboard in Snip: an image object; none: the
    /// clipboard's text (F2: the same rule as the Studio).
    pub(crate) fn paste_picture_or_text(&mut self, clip: Option<Crop>, at: Option<Point>) {
        if !self.paste_clipboard(clip) {
            self.paste_text(at);
        }
    }

    /// Ctrl+V (owner, 2026-11-03): the clipboard's text where the typing is,
    /// else a new text (the last text style) at `at`, being typed into.
    pub(crate) fn paste_text(&mut self, at: Option<Point>) {
        let Some(text) =
            crate::features::circle::actions::copy::read_text().map(|t| clean_paste(&t))
        else {
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
            self.wrap_typed();
            self.update_live();
        }
    }

    /// The zoom the default sizes (stroke, text, markers, blur cell) are divided by: the
    /// view's own while a snip or a 1080p picture is shown, so they hold their size on
    /// screen. A picture bigger than that never gets a default thinner than its fit-view
    /// one, in IMAGE px (the Photoshop / Paint.NET way: a brush size is in image px), so at
    /// 100 % on an 8K photo a default line is as bold relative to the picture as a 1080p
    /// snip's. At the zoom that fits the picture it is the view's own (no change there).
    pub(crate) fn size_zoom(&self) -> f32 {
        let v = self.doc.view().size();
        match picture_scale(v.width.min(v.height)) {
            s if s > 1.0 => self.zoom.min(1.0 / s),
            _ => self.zoom,
        }
    }

    /// A new item of the current tool at `p`, in the current colour and width.
    fn create(&self, p: Point) -> Item {
        let z = self.size_zoom();
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
            Tool::Pen | Tool::Select | Tool::Crop | Tool::Eraser => Shape::Pen(vec![p]),
            Tool::Highlighter => Shape::Highlighter(vec![p]),
            Tool::Text => Shape::Text {
                at: p,
                text: String::new(),
                size: self.text_size / z,
                style: self.text_style,
                wrap: None,
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
                cell: self.blur_strength(),
                style: self.blur_style,
                // (a progressive look is a region with a ramp across it)
                kind: match self.blur_style {
                    blur::Style::Progressive => blur::Kind::Rect,
                    _ => self.blur_shape,
                },
                outline: 0.0,
                ramp: Vec::new(),
            },
            Tool::Redact => Shape::Redact {
                a: p,
                b: p,
                oval: false,
            },
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
            hidden: false,
        }
    }

    /// The colour/width picked while an item is selected (or typed) applies to it.
    fn restyle(&mut self) {
        let styled = |item: &Item, editor: &Annotator| {
            let mut new = editor.create(Point::ORIGIN);
            new.shape = item.shape.clone();
            let z = editor.size_zoom();
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
                Shape::Blur {
                    size, cell, style, ..
                } => {
                    *cell = editor.blur_strength();
                    if editor.style_picked {
                        *style = editor.blur_style;
                    }
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
        } else if self.studio {
            self.restyle_selection(&styled);
        } else if let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) {
            let before = self.doc.items[i].clone();
            let after = styled(&before, self);
            if after != before && self.hsv_drag {
                // the picker's drag: shown live, made one undo step at its end
                if self.drag_item.is_none() {
                    self.drag_item = Some((i, before));
                }
                self.doc.items[i] = after;
                self.refresh();
            } else if after != before {
                self.doc.exec(Edit::Change(i, before, after));
                self.refresh();
            }
        }
    }

    /// Repaints the base (all items but a hidden pending one, the legend), then
    /// the live layer.
    pub(crate) fn refresh(&mut self) {
        *self.thumbs.borrow_mut() = None; // (the style pictures follow the drawing)
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
        self.patch.clear();
        self.display = None;
        self.crisp = false;
        self.update_live();
    }

    /// The pending item's painted layer (the tests).
    #[cfg(test)]
    fn live_layer(&self) -> Option<(Rectangle, image::Handle)> {
        self.live.clone()
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

/// Hide's styles and their words, in the order of the options row (Snip's
/// panel and the Studio's bar both show this one list): the two that never
/// read the text, then the looks.
pub fn blur_styles() -> [(blur::Style, &'static str); 5] {
    use crate::t;
    [
        (blur::Style::Frost, t!(sn_style_hide)),
        (blur::Style::Pixelate, t!(sn_style_pixelate)),
        (blur::Style::Soft, t!(sn_style_soft)),
        (blur::Style::Frosted, t!(sn_style_frosted)),
        (blur::Style::Progressive, t!(sn_style_progressive)),
    ]
}

/// Committed Hides kept as patches before the picture is painted whole again.
const MAX_PATCHES: usize = 64;

/// Hide's strengths and their words (Light / Medium / Strong), for Snip's panel and
/// the Studio's bar.
pub fn blur_strengths() -> [(Width, &'static str); 3] {
    use crate::t;
    [
        (Width::Thin, t!(sn_light)),
        (Width::Medium, t!(sn_medium)),
        (Width::Thick, t!(sn_strong)),
    ]
}

/// Hide's shapes and their words, for Snip's panel and the Studio's bar.
pub fn blur_shapes() -> [(blur::Kind, &'static str); 3] {
    use crate::t;
    [
        (blur::Kind::Brush, t!(sn_brush)),
        (blur::Kind::Rect, t!(tl_rect)),
        (blur::Kind::Ellipse, t!(tl_ellipse)),
    ]
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
fn grow(item: &Item, start: Point, p: Point, zoom: f32, shift: bool, line_help: bool) -> Item {
    let mut item = item.clone();
    match &mut item.shape {
        Shape::Rect { b, .. }
        | Shape::Ellipse { b, .. }
        | Shape::Redact { b, .. }
        | Shape::Spotlight { b, .. }
        | Shape::Picture { b, .. } => *b = p,
        Shape::Arrow { a, b, bend, .. } => {
            *b = if shift { arrow::snap_angle(*a, p) } else { p };
            *bend = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        }
        Shape::Blur {
            points,
            kind: blur::Kind::Rect | blur::Kind::Ellipse,
            ramp,
            style,
            ..
        } => {
            // Shift: a square or a circle.
            let d = p - start;
            let side = d.x.abs().max(d.y.abs());
            let end = match shift {
                true => start + iced::Vector::new(side.copysign(d.x), side.copysign(d.y)),
                false => p,
            };
            *points = vec![start, end];
            if *style == blur::Style::Progressive {
                // Strongest along the bottom, gone at the top (Figma's default).
                let r = document::span(points);
                let mid = r.x + r.width / 2.0;
                *ramp = vec![Point::new(mid, r.y + r.height), Point::new(mid, r.y)];
            }
        }
        Shape::Blur { points, .. } => {
            // Line help holds the stroke on one line: sliding back along it
            // un-blurs what it left (#154). A free brush never loses a point.
            let dir = points.last().map_or(0.0, |l| l.x - start.x).signum();
            while line_help
                && points.len() > 1
                && points.last().is_some_and(|l| (l.x - p.x) * dir > 0.0)
            {
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
