//! The annotation editor: ONE component behind both entry points, Edit in place
//! (the overlay's action bar turns into its toolbar) and Snip mode.
//!
//! Model: `Document` (image, items, selection, crop, undo stack), flattened by the
//! pure `render`. UI: `Editor` = the image with a canvas over it (input + selection
//! chrome) and the floating toolbar. One file per tool.
//!
//! Smooth on big crops: the flattened document (`base`) is cached and repainted
//! once per change; while something is drawn, moved or typed, only its own
//! region is repainted (`live`) and shown over the base.
pub(crate) mod area;
mod arrow;
pub(crate) mod blur;
pub mod board;
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
#[cfg(test)]
mod tests;
mod text;

pub use document::{Document, Edit, Image, Item, Shape, render, span};
pub use style::PRESETS;
pub use text::{Back as TextBack, Font as TextFont, Style as TextStyle, free_glyphs};

use iced::widget::image;
use iced::{Point, Rectangle, Size, Vector};

use crate::actions::Crop;
use document::int_rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Tooltip = the button's description, with its key.
    pub tip: &'static str,
    /// Its key (no Ctrl) while the editor is open.
    pub key: char,
    /// Toolbar group: a divider goes between groups.
    pub group: u8,
    /// The toolbar button it is under (itself, or the tool whose variant it is:
    /// the variants show in a strip under that button).
    pub slot: Tool,
}

/// Every tool, in toolbar order (variants right after their button's tool).
pub const TOOLS: &[ToolInfo] = &[
    tool(Tool::Select, '\u{e1c2}', "Select, move, resize (V)", 'v', 0),
    tool(Tool::Crop, '\u{e0af}', "Crop (X)", 'x', 0),
    tool(Tool::Rect, '\u{e16a}', "Rectangle (R)", 'r', 1),
    tool(Tool::Ellipse, '\u{e07a}', "Ellipse (O)", 'o', 1),
    tool(
        Tool::Arrow,
        '\u{e497}',
        "Arrow: drag the middle to curve it; click again for line and labelled arrow (A)",
        'a',
        1,
    ),
    variant(
        Tool::Line,
        '\u{e521}',
        "Line: Shift keeps it at 0, 45 or 90° (L)",
        'l',
        (Tool::Arrow, 1),
    ),
    variant(
        Tool::ArrowLabel,
        '\u{e544}',
        "Arrow with a label: draw, then type (W)",
        'w',
        (Tool::Arrow, 1),
    ),
    tool(
        Tool::Pen,
        '\u{e133}',
        "Pen: rough boxes, circles, lines and arrows snap clean (P)",
        'p',
        1,
    ),
    tool(Tool::Highlighter, '\u{e0f7}', "Highlighter (H)", 'h', 1),
    tool(
        Tool::Text,
        '\u{e197}',
        "Text; click again for a label (T)",
        't',
        2,
    ),
    variant(
        Tool::Label,
        '\u{e182}',
        "Label: text on a colour (E)",
        'e',
        (Tool::Text, 2),
    ),
    tool(Tool::Callout, '\u{e11a}', "Callout (C)", 'c', 2),
    tool(
        Tool::Step,
        '\u{e1d0}',
        "Numbered step: click, then type its note for the legend; click again for a pointer (N)",
        'n',
        2,
    ),
    variant(
        Tool::Pointer,
        '\u{e123}',
        "Pointer: a ring and arrow, no number (I)",
        'i',
        (Tool::Step, 2),
    ),
    tool(
        Tool::Blur,
        '\u{e0b8}',
        "Blur brush: stays on the text line (Shift: free); click again for redact and line assist (B)",
        'b',
        3,
    ),
    variant(
        Tool::Redact,
        '\u{e16b}',
        "Redact: a solid box (K)",
        'k',
        (Tool::Blur, 3),
    ),
    tool(Tool::Spotlight, '\u{e29d}', "Spotlight (S)", 's', 3),
    tool(Tool::Magnifier, '\u{e1b5}', "Magnifier (M)", 'm', 3),
];

const fn tool(tool: Tool, icon: char, tip: &'static str, key: char, group: u8) -> ToolInfo {
    ToolInfo {
        tool,
        icon,
        tip,
        key,
        group,
        slot: tool,
    }
}

/// A tool shown in the strip under `slot`'s button (in `slot`'s `group`).
const fn variant(
    tool: Tool,
    icon: char,
    tip: &'static str,
    key: char,
    (slot, group): (Tool, u8),
) -> ToolInfo {
    ToolInfo {
        tool,
        icon,
        tip,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Thin,
    Medium,
    Thick,
}

impl Width {
    pub const ALL: [Width; 3] = [Width::Thin, Width::Medium, Width::Thick];
    fn stroke(self) -> f32 {
        [3.0, 5.0, 9.0][self as usize]
    }
    fn font(self) -> f32 {
        [18.0, 26.0, 38.0][self as usize]
    }
    fn badge(self) -> f32 {
        [12.0, 16.0, 22.0][self as usize]
    }
    /// The blur brush's height where no text line is under it.
    fn brush(self) -> f32 {
        [16.0, 28.0, 44.0][self as usize]
    }
}

/// The popovers under the toolbar (one at a time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    /// Colour, weight, eyedropper, presets, copy / paste style.
    Colour,
    /// Add capture, board layout, numbers, legend.
    Board,
    /// The variants of a tool button (`slot`).
    Variants(Tool),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Pointer, image px (from the canvas).
    Press(Point),
    Move(Point),
    Release,
    /// Shift is held (lines snap to 45° steps).
    Shift(bool),
    /// Typing into a text, label, callout, arrow label or step note.
    Type(String),
    Backspace,
    /// Ends typing.
    Finish,
    Tool(Tool),
    /// A toolbar tool button: picks its tool, or opens / closes its variants.
    Slot(Tool),
    /// Opens/closes the colour picker.
    Picker,
    /// Opens/closes a popover.
    Menu(Menu),
    Colour([u8; 4]),
    Width(Width),
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
    /// Line assist on / off (the blur brush keeps to text lines).
    Assist,
    /// Asks the host for another capture (`Output::AddCapture`). No button
    /// sends it (owner, 2026-10-31: the Steps board replaces it; hidden, kept).
    #[allow(dead_code)]
    AddCapture,
    Layout(board::Layout),
    /// Number labels on the board's captures.
    Numbers,
    /// Left button down on the drag handle: OS drag of the image.
    DragOut,
    Undo,
    Redo,
    Delete,
    Copy,
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
    Activate,
    /// Snip's toolbar ×: the snip closes, nothing copied.
    Close,
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
    /// Scroll and stitch the area under the editor (Snip, in place only).
    Scroll,
    /// Leave for a screen tool (Snip; Esc there comes back to the area).
    ScreenTool(ScreenTool),
    /// A floating always-on-top copy of the image.
    Pin(Crop),
    /// A floating copy of the area that keeps following the window under it (Snip).
    LivePin,
    /// Let the user pick another area; give it to `Editor::append`.
    AddCapture,
    /// The share sheet is up: fires once the user picked a target (send
    /// `Message::Shared` then); cancelled if the sheet goes without one.
    Sharing(iced::futures::channel::oneshot::Receiver<()>),
    /// The image was dragged into another app (the overlay is already hidden),
    /// or a share target took it: close, as after Copy.
    Dragged,
    /// Close (Snip's ×).
    Close,
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
    Resize(usize, Item),
    Bend(Item),
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

pub struct Editor {
    pub doc: Document,
    pub tool: Tool,
    pub(crate) colour: [u8; 4],
    pub(crate) width: Width,
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
    /// The blur brush keeps to text lines (config `editor.line_assist`).
    pub(crate) assist: bool,
    /// Scrolling capture can run for this editor (the host sets it; the
    /// toolbar button is dimmed without).
    pub scroll: bool,
    /// The image's text lines, for the image they were found in.
    bands: Option<((u32, u32, board::Board), Vec<lines::Band>)>,
    /// The text line the blur stroke being drawn is held on.
    lock: Option<lines::Lock>,
    /// Snip's live area: the document is the whole frozen monitor (or every
    /// monitor joined) and its crop is the captured area, which moves and
    /// resizes (`area`). The host places its top-left pixel (`canvas::Lens`).
    pub movable: bool,
    area_drag: Option<AreaDrag>,
    /// Snip's toolbar control Tab is on (its index in the toolbar's order);
    /// a press anywhere drops it.
    pub focus: Option<usize>,
    /// Snip's arrow-key hint shows (after the first arrow key).
    pub nudged: bool,
}

impl Editor {
    /// Opens `image`, shown at `zoom` display px per image px (e.g. 1 / the
    /// monitor's scale factor, so the crop shows at its on-screen size).
    pub fn new(image: Image, zoom: f32) -> Self {
        let colour = style::ANNOTATION[0].into_rgba8();
        let mut editor = Editor {
            doc: Document::new(image),
            tool: Tool::Arrow,
            colour,
            width: Width::Medium,
            text_size: Width::Medium.font(),
            text_style: TextStyle::default(),
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
            assist: crate::config::current().editor.line_assist,
            scroll: false,
            bands: None,
            lock: None,
            movable: false,
            area_drag: None,
            focus: None,
            nudged: false,
        };
        editor.refresh();
        editor
    }

    /// Adds `image` (another capture) to the board.
    pub fn append(&mut self, image: Image) {
        self.commit();
        if std::mem::take(&mut self.movable) {
            self.doc.bake(); // a board of captures as they are
        }
        self.doc.append(image);
        self.refresh();
    }

    /// Shows the document at `zoom` display px per image px (e.g. after the board
    /// grew past the screen).
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
    }

    pub fn update(&mut self, message: Message) -> Option<Output> {
        match message {
            Message::Press(p) => {
                (self.focus, self.nudged) = (None, false);
                if matches!(self.menu, Some(Menu::Board | Menu::Variants(_))) {
                    self.menu = None;
                }
                self.press(p)
            }
            Message::Move(p) => self.drag_to(p),
            Message::Release => self.release(),
            Message::Shift(on) => self.shift = on,
            Message::Type(s) => self.edit_text(|t| t.push_str(&s)),
            Message::Backspace => self.edit_text(|t| {
                t.pop();
            }),
            Message::Finish => self.commit(),
            Message::Tool(tool) => self.pick_tool(tool),
            Message::Slot(slot) => {
                let has_variants = TOOLS.iter().any(|t| t.slot == slot && t.tool != slot);
                let open = self.menu == Some(Menu::Variants(slot));
                if info(self.tool).slot != slot {
                    self.pick_tool(slot);
                }
                self.menu = (has_variants && !open).then_some(Menu::Variants(slot));
            }
            Message::Picker => self.toggle(Menu::Colour),
            Message::Menu(menu) => self.toggle(menu),
            Message::Colour(c) => self.set_colour(c),
            Message::Width(w) => {
                self.set_width(w);
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
                save_assist(self.assist);
            }
            Message::Legend => {
                self.commit();
                self.doc.legend = !self.doc.legend;
                self.refresh();
            }
            Message::AddCapture => {
                self.commit();
                self.menu = None;
                return Some(Output::AddCapture);
            }
            Message::Layout(layout) => self.rearrange(board::Board {
                layout,
                ..self.doc.board
            }),
            Message::Numbers => self.rearrange(board::Board {
                numbers: !self.doc.board.numbers,
                ..self.doc.board
            }),
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
            Message::Undo | Message::Redo => {
                self.commit();
                let undo = matches!(message, Message::Undo);
                if if undo {
                    self.doc.undo()
                } else {
                    self.doc.redo()
                } {
                    self.refresh();
                }
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
            Message::Back => {
                self.commit();
                return Some(Output::Back);
            }
            Message::Grab | Message::Focus(_) | Message::Activate => {}
            Message::Close => return Some(Output::Close),
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

    /// The canvas size, display px (changes with the crop, the board and the
    /// legend's band).
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
    }

    fn pick_tool(&mut self, tool: Tool) {
        self.commit();
        self.tool = tool;
        self.menu = None;
        self.picking = false;
    }

    /// The weight; a text's size follows it.
    fn set_width(&mut self, w: Width) {
        self.width = w;
        self.text_size = w.font();
    }

    fn set_colour(&mut self, c: [u8; 4]) {
        self.colour = c;
        self.hex = hex(c);
        self.restyle();
    }

    fn rearrange(&mut self, board: board::Board) {
        self.commit();
        self.doc.rearrange(board);
        self.refresh();
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

    /// The image's text lines (detected once per board layout).
    fn bands(&mut self) -> &[lines::Band] {
        let key = (self.doc.image.width, self.doc.image.height, self.doc.board);
        if self.bands.as_ref().is_none_or(|(k, _)| *k != key) {
            self.bands = Some((key, lines::detect(&self.doc.image)));
        }
        self.bands.as_ref().map_or(&[], |(_, b)| b)
    }

    /// A blur stroke at `p`: as tall as the text line there; with line assist
    /// (and no Shift) held on that line.
    fn brush(&mut self, p: Point) -> Item {
        let band = lines::under(self.bands(), p);
        let mut item = self.create(p);
        if let (Some(band), Shape::Blur { points, size, .. }) = (band, &mut item.shape) {
            *size = band.brush();
            if self.assist && !self.shift {
                points[0] = Point::new(p.x, band.centre());
                self.lock = Some(lines::Lock::new(band));
            }
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
                let drag = match handle {
                    select::Handle::Corner(c) => Drag::Resize(c, original.clone()),
                    select::Handle::Bend => Drag::Bend(original.clone()),
                };
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
            let edits = edits(self.tool, &self.doc.items[i], p, reach);
            self.pick_up(i, edits, p);
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

    /// The item a press at `p` moves instead of drawing: every drawing tool moves
    /// what is painted under it (a click on one of its own text items types into
    /// it), the pen and highlighter their own strokes too (owner: drag a drawing
    /// by holding it). The blur brush paints over its own strokes (a second
    /// pass); crop always crops.
    fn grabbed(&self, p: Point) -> Option<usize> {
        if self.tool == Tool::Crop {
            return None;
        }
        let i = select::grab(&self.doc.items, p, GRAB / self.zoom)?;
        let second_pass =
            self.tool == Tool::Blur && matches!(self.doc.items[i].shape, Shape::Blur { .. });
        (!second_pass).then_some(i)
    }

    /// Starts moving item `i` (`edits`: a click without a move types into it).
    fn pick_up(&mut self, i: usize, edits: bool, p: Point) {
        let from = self.doc.items[i].clone();
        // A text's options show (and edit) its own.
        if let Shape::Text { size, style, .. } = &from.shape {
            (self.text_size, self.text_style) = (size * self.zoom, *style);
        }
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
        whole(r)
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
        if let Some(lock) = &mut self.lock
            && !self.shift
        {
            let bands = self.bands.as_ref().map_or(&[][..], |(_, b)| b);
            match lock.follow(bands, p, std::time::Instant::now()) {
                lines::Follow::Stay(on_line) => p = on_line,
                // On to another line: this stroke ends, a new one starts there.
                lines::Follow::Switch(band) => {
                    self.commit();
                    let at = Point::new(p.x, band.centre());
                    let mut item = self.create(at);
                    if let Shape::Blur { size, .. } = &mut item.shape {
                        *size = band.brush();
                    }
                    self.begin(None, item, Drag::Draw, at);
                    return;
                }
            }
        }
        let Some(pending) = &mut self.pending else {
            return;
        };
        pending.item = match &pending.drag {
            Drag::Draw => grow(&pending.item, pending.start, p, self.zoom, self.shift),
            // A click that wobbles a pixel is not a move.
            Drag::Move { from, .. } if p.distance(pending.start) * self.zoom < SLOP => from.clone(),
            Drag::Move { from, .. } => select::moved(from, p - pending.start),
            Drag::Resize(corner, original) => select::resized(original, *corner, p),
            Drag::Bend(original) => select::bent(original, p),
            Drag::Type => return,
        };
        if pending.hidden || pending.index.is_none() {
            self.update_live();
        } else {
            self.hide();
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
                size: self.width.font() * 0.75 / z,
            },
            Tool::Callout => Shape::Callout {
                tip: p,
                at: p + iced::Vector::new(90.0, -60.0) * (1.0 / z),
                text: String::new(),
                size: self.width.font() * 0.75 / z,
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
                size: self.width.brush() / z,
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
            Tool::Highlighter => self.width.stroke() * 4.0,
            _ => self.width.stroke(),
        } / z;
        Item {
            shape,
            colour: self.colour,
            width,
        }
    }

    /// The colour/width picked while an item is selected (or typed) applies to it.
    fn restyle(&mut self) {
        let styled = |item: &Item, editor: &Editor| {
            let mut new = editor.create(Point::ORIGIN);
            new.shape = item.shape.clone();
            let z = editor.zoom;
            match &mut new.shape {
                Shape::Text { size, style, .. } => {
                    *size = editor.text_size / z;
                    *style = editor.text_style;
                }
                Shape::Callout { size, .. } | Shape::Label { size, .. } => {
                    *size = editor.width.font() * 0.75 / z
                }
                Shape::Step { radius, .. } => *radius = editor.width.badge() / z,
                Shape::Pointer { radius, .. } => {
                    *radius = editor.width.badge() / z;
                    new.width = editor.width.stroke() / z;
                }
                Shape::Highlighter(_) => new.width = editor.width.stroke() * 4.0 / z,
                // A brush keeps its size (fit to its text line).
                Shape::Blur { .. } => new.width = item.width,
                _ => new.width = editor.width.stroke() / z,
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
        self.movable && self.doc.items.is_empty() && self.pending.is_none()
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

/// Remembers the line assist choice (`editor.line_assist`) for the next editor.
fn save_assist(on: bool) {
    if cfg!(test) {
        return; // never the user's config file
    }
    std::thread::spawn(move || {
        if let Err(e) = crate::config::update(|c| c.editor.line_assist = on) {
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
        Shape::Pen(points) | Shape::Highlighter(points) | Shape::Blur { points, .. } => {
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

fn hex(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// `#RRGGBB` (the `#` optional) as an opaque colour.
fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, 255])
}

/// `r` on whole image px (the captured pixels are whole).
fn whole(r: Rectangle) -> Rectangle {
    let (x0, y0) = (r.x.round(), r.y.round());
    let (x1, y1) = ((r.x + r.width).round(), (r.y + r.height).round());
    Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
}
