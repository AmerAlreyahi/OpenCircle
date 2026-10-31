//! The floating editing toolbar (the design's `editor-toolbar.js`), composed
//! from kit parts only: grip · back · tools in groups (a click on the active tool
//! opens its options under it: variants, size, outline, fill) · colour · board · scrolling · undo, redo · pin · copy,
//! save · drag. The open popover shows under its button.
#[cfg(test)]
use iced::widget::Space;
use iced::widget::{column, container, row, tooltip};
use iced::{Element, Point, Rectangle, Size, Vector};

use crate::actions::share;
use crate::editor::{
    Editor, Menu, Message, PRESETS, ScreenTool, TOOLS, TextBack, TextFont, Tool, ToolInfo, Width,
    dragout, info,
};
use crate::ui::kit::icon_button::{self, State};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{self as kit_text, text as caption};
use crate::ui::kit::tokens::{Role, radius, size};
use crate::ui::kit::tooltip::{Why, hint};
use crate::ui::kit::{button, chip, colour, divider, grip, segmented};
use crate::ui::screens::bar::geometry as bar;

const SPACING: f32 = bar::SPACING;
/// A button's side.
const CONTROL: f32 = size::ICON_BUTTON;
/// The Drag chip's width.
const DRAG_WIDTH: f32 = 63.0;
const BACK: char = '\u{e04c}'; // lucide "arrow-left"
const UNDO: char = '\u{e2a0}'; // lucide "undo-2"
const REDO: char = '\u{e29f}'; // lucide "redo-2"
const COPY: char = '\u{e0a2}'; // lucide "copy"
const SAVE: char = '\u{e0b6}'; // lucide "download"
const PIN: char = '\u{e258}'; // lucide "pin"
const BOARD: char = '\u{e104}'; // lucide "layout-grid"
#[allow(dead_code)] // Add capture's, hidden (`board_popover`)
const ADD: char = '\u{e1f6}'; // lucide "image-plus"
const STACK: char = '\u{e43d}'; // lucide "rows-2"
const SIDE: char = '\u{e09c}'; // lucide "columns-2"
const NUMBERS: char = '\u{e0f2}'; // lucide "hash"
const LEGEND: char = '\u{e10b}'; // lucide "list"
const SCROLL: char = '\u{e4d6}'; // lucide "gallery-vertical-end"
const ASSIST: char = '\u{e2b4}'; // lucide "magnet"
const PIPETTE: char = '\u{e13e}'; // lucide "pipette"
const SHARE: char = '\u{e158}'; // lucide "share"
const LIVE_PIN: char = '\u{e3b2}'; // lucide "picture-in-picture-2"
/// The screen tools' buttons: glyph, tooltip.
const SCREEN_TOOLS: [(ScreenTool, char, &str); 2] = [
    (
        ScreenTool::Picker,
        crate::ui::screens::tools::picker::ICON,
        "Colour picker: click copies HEX, Shift+click RGB",
    ),
    (
        ScreenTool::Ruler,
        crate::ui::screens::tools::ruler::ICON,
        "Ruler: measure in pixels",
    ),
];

/// The glyphs used here (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[
    BACK,
    UNDO,
    REDO,
    COPY,
    SAVE,
    PIN,
    BOARD,
    ADD,
    STACK,
    SIDE,
    NUMBERS,
    LEGEND,
    ASSIST,
    SCROLL,
    PIPETTE,
    SHARE,
    crate::ui::screens::tools::ruler::ICON,
    LIVE_PIN,
];

/// The tool buttons: one per slot (variants live in its strip).
fn slots() -> impl Iterator<Item = &'static ToolInfo> {
    TOOLS.iter().filter(|t| t.slot == t.tool)
}

/// Snip's Live pin (Windows; Labs Pins); it works only over the live screen
/// (`Editor::scroll`). Still pins left Labs (owner, 2026-10-31): Pin is always there.
fn live_pin() -> bool {
    crate::labs::has(crate::labs::Feature::Pins) && crate::capture::live::SUPPORTED
}

/// The widths of a line's items, side by side.
fn line_width(items: &[f32]) -> f32 {
    items.iter().sum::<f32>() + SPACING * items.len().saturating_sub(1) as f32
}

/// The widths of the toolbar's two parts: the grip, back and the tools, then
/// the rest (colour, board, scrolling, undo, redo, pin, a divider, copy, save,
/// and where supported a divider and the drag chip).
fn parts() -> (Vec<f32>, Vec<f32>) {
    let mut tools = vec![bar::HANDLE, CONTROL];
    let mut group = None;
    for t in slots() {
        if group != Some(t.group) {
            tools.push(bar::DIVIDER);
            group = Some(t.group);
        }
        tools.push(CONTROL);
    }
    let mut rest = vec![CONTROL; 6];
    rest.extend([bar::DIVIDER, CONTROL, CONTROL]);
    if dragout::SUPPORTED {
        rest.extend([bar::DIVIDER, DRAG_WIDTH]);
    }
    (tools, rest)
}

/// Snip's toolbar (the design's capture flow, N-73): no Back (no bar to go
/// back to), no Crop (the area's handles are the crop), no group dividers
/// between the tools; the size readout after the grip; Long capture, the
/// colour picker and the ruler, Pin, Live pin (Labs), Share (where there is a share
/// sheet) after Undo / Redo; Copy and Save labelled; ×.
fn snip_parts() -> (Vec<f32>, Vec<f32>) {
    let mut tools = vec![bar::HANDLE, READOUT_W, bar::DIVIDER];
    tools.extend(snip_slots().map(|_| CONTROL));
    let mut rest = vec![CONTROL; 4];
    rest.extend([bar::DIVIDER, CONTROL]);
    rest.extend(SCREEN_TOOLS.map(|_| CONTROL));
    rest.push(CONTROL); // Pin
    if live_pin() {
        rest.push(CONTROL);
    }
    if share::SUPPORTED {
        rest.push(CONTROL);
    }
    rest.extend([bar::DIVIDER, COPY_W, SAVE_W, CONTROL]);
    (tools, rest)
}

/// Snip's tool slots (no Crop).
fn snip_slots() -> impl Iterator<Item = &'static ToolInfo> {
    slots().filter(|t| t.tool != Tool::Crop)
}

/// Snip's size readout's width (fits `9999 × 9999`).
const READOUT_W: f32 = 96.0;
/// Snip's Copy and Save (the design's widths).
const COPY_W: f32 = 78.0;
const SAVE_W: f32 = 74.0;
const CLOSE: char = '\u{e1b1}'; // lucide "x"

/// What Tab walks on Snip's toolbar, in its order (left to right), and what
/// Enter / Space on each sends.
pub fn stops(e: &Editor) -> Vec<Message> {
    let mut stops: Vec<Message> = snip_slots().map(|t| Message::Slot(t.tool)).collect();
    stops.extend([
        Message::Picker,
        Message::Menu(Menu::Board),
        Message::Undo,
        Message::Redo,
    ]);
    if e.scroll {
        stops.push(Message::Scroll);
    }
    stops.extend(SCREEN_TOOLS.map(|(t, ..)| Message::ScreenTool(t)));
    stops.push(Message::Pin);
    if live_pin() && e.scroll {
        stops.push(Message::LivePin);
    }
    if share::SUPPORTED {
        stops.push(Message::Share);
    }
    stops.extend([Message::Copy, Message::Save, Message::Close]);
    stops
}

/// The toolbar's size within `max_width` (as `view` lays it out without a
/// popover): one row, or two (tools, then the rest) when one does not fit;
/// `snip`: Snip's toolbar.
pub fn size(max_width: f32, snip: bool) -> Size {
    let (tools, rest) = if snip { snip_parts() } else { parts() };
    let (tools, rest) = (line_width(&tools), line_width(&rest));
    let pad = 2.0 * bar::PAD;
    let one = tools + SPACING + rest + pad;
    if one <= max_width {
        Size::new(one, bar::HEIGHT)
    } else {
        Size::new(tools.max(rest) + pad, 2.0 * CONTROL + SPACING + pad)
    }
}

/// The toolbar with its open popover under it (where nothing limits the room).
#[cfg(test)]
pub fn view(e: &Editor, max_width: f32) -> Element<'_, Message> {
    let (bar, popover) = build(e, max_width, tooltip::Position::Bottom);
    match popover {
        None => bar,
        Some((popover, left)) => column![bar, row![Space::new().width(left), popover]]
            .spacing(POPOVER_GAP)
            .into(),
    }
}

/// The toolbar shown at `bar` (screen px) and its open popover as a floating
/// layer for the host to put over the whole screen at (0, 0): laid out there at
/// its full size, then moved by `place_popover`.
/// `side`: where the tooltips go (away from Snip's area).
pub fn view_at(
    e: &Editor,
    max_width: f32,
    bar: Rectangle,
    side: tooltip::Position,
) -> (Element<'_, Message>, Option<Element<'_, Message>>) {
    let (toolbar, popover) = build(e, max_width, side);
    let popover = popover.map(|(popover, left)| {
        iced::widget::float(popover)
            .translate(move |laid, screen| {
                let under = Rectangle::new(
                    Point::new(bar.x + left, bar.y + bar.height + POPOVER_GAP),
                    laid.size(),
                );
                under.position() - laid.position() + place_popover(under, bar.height, screen)
            })
            .into()
    });
    (toolbar, popover)
}

thread_local! {
    /// Where tooltips go (the toolbar's side away from the area), for this build.
    static TIP_SIDE: std::cell::Cell<tooltip::Position> =
        const { std::cell::Cell::new(tooltip::Position::Bottom) };
}

/// A button, named (and given its key) in a tooltip beside it.
fn hinted<'a>(button: Element<'a, Message>, label: &'a str, why: Why<'a>) -> Element<'a, Message> {
    hint(button, label, why, TIP_SIDE.get())
}

/// A square icon button with its tooltip.
fn icon<'a>(glyph: char, on: bool, message: Message, tip: &'a str) -> Element<'a, Message> {
    let state = State {
        active: on,
        focused: false,
    };
    hinted(
        icon_button::square(glyph, state, Some(message)),
        tip,
        Why::Icon,
    )
}

/// A divider between groups of buttons.
fn divide<'a>() -> Element<'a, Message> {
    container(divider::vertical(bar::DIVIDER_H))
        .center_x(bar::DIVIDER)
        .into()
}

/// The items of a line and where the next one starts (from the toolbar's left
/// edge), for placing a popover under its button.
struct Line<'a> {
    items: Vec<Element<'a, Message>>,
    x: f32,
}

impl<'a> Line<'a> {
    /// Adds `item`, `width` wide; its left edge.
    fn push(&mut self, item: Element<'a, Message>, width: f32) -> f32 {
        let at = self.x;
        self.items.push(item);
        self.x += width + SPACING;
        at
    }

    fn row(self) -> Element<'a, Message> {
        row(self.items)
            .spacing(SPACING)
            .align_y(iced::Center)
            .into()
    }
}

/// The toolbar, and the open popover with its left edge (from the toolbar's).
#[allow(clippy::type_complexity)]
fn build(
    e: &Editor,
    max_width: f32,
    side: tooltip::Position,
) -> (Element<'_, Message>, Option<(Element<'_, Message>, f32)>) {
    TIP_SIDE.set(side);
    if e.movable {
        return build_snip(e, max_width);
    }
    let wrap = size(max_width, false).height > bar::HEIGHT;
    let mut tools = Line {
        items: Vec::new(),
        x: bar::PAD,
    };
    let mut at: Vec<(Menu, f32)> = Vec::new();

    tools.push(grip::grip(Message::Grab), bar::HANDLE);
    tools.push(
        icon(BACK, false, Message::Back, "Back to the actions (Esc)"),
        CONTROL,
    );
    let current = info(e.tool);
    let mut group = None;
    for t in slots() {
        if group != Some(t.group) {
            tools.push(divide(), bar::DIVIDER);
            group = Some(t.group);
        }
        // The button shows the variant in use.
        let shown = if current.slot == t.tool { current } else { t };
        let x = tools.push(
            icon(
                shown.icon,
                current.slot == t.tool,
                Message::Slot(t.tool),
                shown.tip,
            ),
            CONTROL,
        );
        at.push((Menu::Options(t.tool), x));
    }
    let mut rest = Line {
        items: Vec::new(),
        x: if wrap { bar::PAD } else { tools.x },
    };
    let open = |menu| State {
        active: e.menu == Some(menu),
        focused: false,
    };
    let x = rest.push(
        hinted(
            colour::swatch(e.ink(), e.menu == Some(Menu::Colour), Some(Message::Picker)),
            "Colour, stroke, eyedropper, presets",
            Why::Icon,
        ),
        CONTROL,
    );
    at.push((Menu::Colour, x));
    let x = rest.push(
        hinted(
            icon_button::square(BOARD, open(Menu::Board), Some(Message::Menu(Menu::Board))),
            "Board: layout, legend",
            Why::Icon,
        ),
        CONTROL,
    );
    at.push((Menu::Board, x));
    let scrolling = if e.scroll {
        "Long capture: scroll the page under this area yourself; it is stitched"
    } else {
        "Long capture: only when snipping"
    };
    rest.push(
        hinted(
            icon_button::square(
                SCROLL,
                State::default(),
                e.scroll.then_some(Message::Scroll),
            ),
            scrolling,
            Why::Icon,
        ),
        CONTROL,
    );
    rest.push(icon(UNDO, false, Message::Undo, "Undo (Ctrl+Z)"), CONTROL);
    rest.push(
        icon(REDO, false, Message::Redo, "Redo (Ctrl+Shift+Z)"),
        CONTROL,
    );
    rest.push(
        icon(
            PIN,
            false,
            Message::Pin,
            "Pin to screen: a floating copy (Ctrl+P)",
        ),
        CONTROL,
    );
    rest.push(divide(), bar::DIVIDER);
    let copy = State {
        active: true,
        focused: false,
    };
    rest.push(
        hinted(
            icon_button::square(COPY, copy, Some(Message::Copy)),
            "Copy",
            Why::Keys("Ctrl C"),
        ),
        CONTROL,
    );
    rest.push(
        hinted(
            icon_button::square(SAVE, State::default(), Some(Message::Save)),
            "Save",
            Why::Keys("Ctrl S"),
        ),
        CONTROL,
    );
    if dragout::SUPPORTED {
        rest.push(divide(), bar::DIVIDER);
        // its word "Drag" says it: no tooltip (owner rule: only when it adds)
        rest.push(chip::drag_out(Message::DragOut), DRAG_WIDTH);
    }

    finish(e, tools, rest, wrap, at)
}

/// The two lines in the toolbar's surface, and the open popover under its button.
#[allow(clippy::type_complexity)]
fn finish<'a>(
    e: &'a Editor,
    mut tools: Line<'a>,
    rest: Line<'a>,
    wrap: bool,
    at: Vec<(Menu, f32)>,
) -> (Element<'a, Message>, Option<(Element<'a, Message>, f32)>) {
    let content: Element<'_, Message> = if wrap {
        column![tools.row(), rest.row()].spacing(SPACING).into()
    } else {
        tools.items.extend(rest.items);
        tools.row()
    };
    let toolbar = surface(Material::Widget, radius::WINDOW, content).padding(bar::PAD);
    let Some(menu) = e.menu else {
        return (toolbar.into(), None);
    };
    let popover = match menu {
        Menu::Colour => colour_popover(e),
        Menu::Board => board_popover(e),
        Menu::Options(slot) => options_popover(e, slot),
    };
    let left = at.iter().find(|(m, _)| *m == menu).map_or(0.0, |(_, x)| *x);
    (toolbar.into(), Some((popover, left - bar::PAD)))
}

/// Snip's toolbar (`snip_parts`): Tab's focus shows on `e.focus` (`stops`).
#[allow(clippy::type_complexity)]
fn build_snip(
    e: &Editor,
    max_width: f32,
) -> (Element<'_, Message>, Option<(Element<'_, Message>, f32)>) {
    let wrap = size(max_width, true).height > bar::HEIGHT;
    let stops = stops(e);
    let focused = |m: &Message| e.focus.and_then(|i| stops.get(i)).is_some_and(|s| s == m);
    let state = |active: bool, m: &Message| State {
        active,
        focused: focused(m),
    };
    // a long capture running: only what finishes it stays live
    let idle = !e.capturing;
    let live = |m: Message| idle.then_some(m);
    let mut tools = Line {
        items: Vec::new(),
        x: bar::PAD,
    };
    let mut at: Vec<(Menu, f32)> = Vec::new();
    tools.push(grip::grip(Message::Grab), bar::HANDLE);
    let px = e.doc.view().size();
    let readout = chip::readout(
        format!("{} × {}", px.width.round(), px.height.round()),
        e.area_held(),
        None,
    );
    tools.push(container(readout).center_x(READOUT_W).into(), READOUT_W);
    tools.push(divide(), bar::DIVIDER);
    let current = info(e.tool);
    for t in snip_slots() {
        let shown = if current.slot == t.tool { current } else { t };
        let m = Message::Slot(t.tool);
        let s = state(idle && current.slot == t.tool, &m);
        let x = tools.push(
            hinted(
                icon_button::square(shown.icon, s, live(m)),
                shown.tip,
                Why::Icon,
            ),
            CONTROL,
        );
        at.push((Menu::Options(t.tool), x));
    }
    let mut rest = Line {
        items: Vec::new(),
        x: if wrap { bar::PAD } else { tools.x },
    };
    let x = rest.push(
        hinted(
            colour::swatch(e.ink(), e.menu == Some(Menu::Colour), live(Message::Picker)),
            "Colour, stroke, eyedropper, presets",
            Why::Icon,
        ),
        CONTROL,
    );
    at.push((Menu::Colour, x));
    let board = Message::Menu(Menu::Board);
    let s = state(e.menu == Some(Menu::Board), &board);
    let x = rest.push(
        hinted(
            icon_button::square(BOARD, s, live(board)),
            "Board: layout, legend",
            Why::Icon,
        ),
        CONTROL,
    );
    at.push((Menu::Board, x));
    for (glyph, m, tip) in [
        (UNDO, Message::Undo, "Undo (Ctrl+Z)"),
        (REDO, Message::Redo, "Redo (Ctrl+Shift+Z)"),
    ] {
        let s = state(false, &m);
        rest.push(
            hinted(icon_button::square(glyph, s, live(m)), tip, Why::Icon),
            CONTROL,
        );
    }
    rest.push(divide(), bar::DIVIDER);
    let long = match (e.capturing, e.scroll) {
        (true, _) => "Long capture: scroll the page; Copy or Save finishes it",
        (false, true) => "Long capture: scroll the page under the area yourself; it is stitched",
        (false, false) => "Long capture: only when snipping",
    };
    let s = state(e.capturing, &Message::Scroll);
    rest.push(
        hinted(
            icon_button::square(SCROLL, s, e.scroll.then_some(Message::Scroll)),
            long,
            Why::Icon,
        ),
        CONTROL,
    );
    for (tool, glyph, tip) in SCREEN_TOOLS {
        let m = Message::ScreenTool(tool);
        let s = state(false, &m);
        rest.push(
            hinted(icon_button::square(glyph, s, live(m)), tip, Why::Icon),
            CONTROL,
        );
    }
    let s = state(false, &Message::Pin);
    rest.push(
        hinted(
            icon_button::square(PIN, s, Some(Message::Pin)),
            "Pin to screen: a floating copy (Ctrl+P)",
            Why::Icon,
        ),
        CONTROL,
    );
    if live_pin() {
        let s = state(false, &Message::LivePin);
        let tip = match e.scroll {
            true => "Live pin: this part of the window, kept up to date",
            false => "Live pin: only when snipping",
        };
        rest.push(
            hinted(
                icon_button::square(LIVE_PIN, s, (e.scroll && idle).then_some(Message::LivePin)),
                tip,
                Why::Icon,
            ),
            CONTROL,
        );
    }
    if share::SUPPORTED {
        let s = state(false, &Message::Share);
        rest.push(
            hinted(
                icon_button::square(SHARE, s, Some(Message::Share)),
                "Share: the system's share sheet",
                Why::Icon,
            ),
            CONTROL,
        );
    }
    rest.push(divide(), bar::DIVIDER);
    let copy = button::tool(
        COPY,
        "Copy",
        button::Kind::Accent,
        COPY_W,
        focused(&Message::Copy),
        Some(Message::Copy),
    );
    rest.push(
        hint(copy, "Copy", Why::Keys("Ctrl C"), TIP_SIDE.get()),
        COPY_W,
    );
    let save = button::tool(
        SAVE,
        "Save",
        button::Kind::Elevated,
        SAVE_W,
        focused(&Message::Save),
        Some(Message::Save),
    );
    rest.push(
        hint(save, "Save", Why::Keys("Ctrl S"), TIP_SIDE.get()),
        SAVE_W,
    );
    let s = state(false, &Message::Close);
    rest.push(
        hint(
            icon_button::square(CLOSE, s, Some(Message::Close)),
            "Close",
            Why::Keys("Esc"),
            TIP_SIDE.get(),
        ),
        CONTROL,
    );
    finish(e, tools, rest, wrap, at)
}

/// A popover's body: the toolbar's surface round `content`.
fn popover<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    surface(Material::Widget, radius::WINDOW, content)
        .padding(bar::PAD)
        .into()
}

/// A tool's options (a click on the active tool; ONE design for every tool):
/// its variants (the blur brush: and line help), then its own sections:
/// the Text tool's style; else its size, and where it applies the outline
/// and the fill. They apply to the next item and to the selected one.
fn options_popover(e: &Editor, slot: Tool) -> Element<'_, Message> {
    let mut tools: Vec<Element<'_, Message>> = (TOOLS.iter())
        .filter(|t| t.slot == slot)
        .map(|t| icon(t.icon, e.tool == t.tool, Message::Tool(t.tool), t.tip))
        .collect();
    if slot == Tool::Blur {
        tools.push(divide());
        tools.push(icon(
            ASSIST,
            e.assist,
            Message::Assist,
            "Line help: a stroke stays on its line unless you clearly move up or down (Shift: free)",
        ));
    }
    let mut parts: Vec<Element<'_, Message>> = Vec::new();
    if tools.len() > 1 {
        parts.push(row(tools).spacing(SPACING).align_y(iced::Center).into());
    }
    if slot == Tool::Text {
        parts.push(text_options(e));
        return popover(column(parts).spacing(SPACING * 2.0));
    }
    let weights = [
        (Width::Thin, "Thin"),
        (Width::Medium, "Medium"),
        (Width::Thick, "Thick"),
    ];
    let (outline, fill) = styles(slot);
    let size: Element<'_, Message> = match slot {
        Tool::Blur => {
            let sizes = [
                (None, "Auto"),
                (Some(Width::Thin), "Small"),
                (Some(Width::Medium), "Medium"),
                (Some(Width::Thick), "Large"),
            ];
            let on = e.blur_size;
            segmented::inline(&sizes, on, on, 1.0, Message::BlurSize)
        }
        _ => segmented::inline(&weights, e.width, e.width, 1.0, Message::Width),
    };
    let label = if slot == Tool::Blur { "Brush" } else { "Size" };
    let mut options: Vec<Element<'_, Message>> = vec![caption(Role::SECTION, label).into(), size];
    if outline {
        options.push(caption(Role::SECTION, "Outline").into());
        let on = segmented::inline_boolean(
            ["None", "Outline"],
            e.outline,
            e.outline,
            1.0,
            Message::Outline,
        );
        options.push(on);
    }
    if fill {
        options.push(caption(Role::SECTION, "Fill").into());
        let on = segmented::inline_boolean(["None", "Tint"], e.fill, e.fill, 1.0, Message::Fill);
        options.push(on);
    }
    parts.push(column(options).spacing(SPACING).into());
    popover(column(parts).spacing(SPACING * 2.0))
}

/// What a tool button's options offer besides the size: (outline, fill).
pub fn styles(slot: Tool) -> (bool, bool) {
    match slot {
        Tool::Rect | Tool::Ellipse => (true, true),
        Tool::Arrow | Tool::Callout => (true, false),
        _ => (false, false),
    }
}

/// Between the toolbar and its popover.
const POPOVER_GAP: f32 = 8.0;

/// THE placement of every toolbar popover: `under` = under its button,
/// `bar_height` = the toolbar's. Above the toolbar when there is no room below;
/// always kept inside `screen`. Returns the shift from `under`.
pub fn place_popover(under: Rectangle, bar_height: f32, screen: Rectangle) -> Vector {
    let mut at = under.position();
    if at.y + under.height > screen.y + screen.height {
        at.y -= under.height + bar_height + 2.0 * POPOVER_GAP;
    }
    let clamp = |v: f32, lo: f32, hi: f32| v.min(hi).max(lo);
    at.x = clamp(at.x, screen.x, screen.x + screen.width - under.width);
    at.y = clamp(at.y, screen.y, screen.y + screen.height - under.height);
    at - under.position()
}

/// The kit's colour picker with the stroke weight, the eyedropper, the presets
/// (keys 1-9) and copy / paste style under its hex field.
fn colour_popover(e: &Editor) -> Element<'_, Message> {
    let weights = [
        (Width::Thin, "Thin"),
        (Width::Medium, "Medium"),
        (Width::Thick, "Thick"),
    ];
    let weight = segmented::inline(&weights, e.width, e.width, 1.0, Message::Width);
    let eyedropper = hinted(
        icon_button::square(PIPETTE, State::default(), Some(Message::Eyedropper)),
        "Pick a colour from the image",
        Why::Keys("D"),
    );
    let style = row![
        eyedropper,
        chip::action("Copy style".into(), Message::CopyStyle),
        chip::action("Paste style".into(), Message::PasteStyle),
    ]
    .spacing(SPACING)
    .align_y(iced::Center);
    let preset = |i: usize| -> Element<'_, Message> {
        let p = &PRESETS[i];
        hinted(
            chip::action(p.short.into(), Message::Preset(i)),
            p.name,
            Why::Keys(p.key),
        )
    };
    let pairs = (0..PRESETS.len())
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|pair| row(pair.iter().map(|&i| preset(i))).spacing(SPACING).into())
        .collect::<Vec<Element<'_, Message>>>();
    let more = column![
        weight,
        style,
        caption(Role::SECTION, "Presets"),
        column(pairs).spacing(SPACING)
    ]
    .spacing(SPACING);
    colour::picker_with(
        &crate::editor::style::ANNOTATION,
        e.ink(),
        &e.hex,
        |c| Message::Colour(c.into_rgba8()),
        Message::Hex,
        Some(more.into()),
    )
}

/// (With two or more captures) the layout and numbers, then the legend.
/// No Add capture (owner, 2026-10-31): the new Steps board replaces it; its
/// message and `Output::AddCapture` stay, unreachable (Hide, don't delete).
fn board_popover(e: &Editor) -> Element<'_, Message> {
    let board = e.doc.board;
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    if board.count > 1 {
        for (glyph, layout, tip) in [
            (STACK, crate::editor::board::Layout::Stack, "Stacked"),
            (SIDE, crate::editor::board::Layout::Side, "Side by side"),
            (BOARD, crate::editor::board::Layout::Grid, "Grid"),
        ] {
            items.push(icon(
                glyph,
                board.layout == layout,
                Message::Layout(layout),
                tip,
            ));
        }
        items.push(icon(
            NUMBERS,
            board.numbers,
            Message::Numbers,
            "Number each capture",
        ));
        items.push(divide());
    }
    items.push(icon(
        LEGEND,
        e.doc.legend,
        Message::Legend,
        "Legend: the steps' notes in a box",
    ));
    popover(row(items).spacing(SPACING).align_y(iced::Center))
}

/// The Text tool's sizes (display px) and their tooltips.
pub const TEXT_SIZES: [(f32, &str); 5] = [
    (14.0, "Size 14"),
    (18.0, "Size 18"),
    (26.0, "Size 26"),
    (38.0, "Size 38"),
    (52.0, "Size 52"),
];

/// The Text tool's options: size, font, then bold and the backdrop. They apply
/// to the next text and to the selected one.
fn text_options(e: &Editor) -> Element<'_, Message> {
    use iced::widget::text;
    let style = e.text_style;
    let option = |content: Element<'static, Message>, on, message, tip| {
        let state = State {
            active: on,
            focused: false,
        };
        hinted(icon_button::option(content, state, message), tip, Why::Icon)
    };
    let letter = |label: &'static str, font: iced::Font, px: f32| -> Element<'static, Message> {
        text(label).font(font).size(px).into()
    };
    let sizes = TEXT_SIZES.iter().enumerate().map(|(i, &(px, tip))| {
        let on = (e.text_size - px).abs() < 0.5;
        let glyph = letter("A", kit_text::ui_font(), 11.0 + 2.5 * i as f32);
        option(glyph, on, Message::TextSize(px), tip)
    });
    let fonts = TextFont::ALL.iter().map(|&font| {
        let (family, tip) = match font {
            TextFont::Sans => (iced::font::Family::SansSerif, "Sans-serif"),
            TextFont::Serif => (iced::font::Family::Serif, "Serif"),
            TextFont::Mono => (iced::font::Family::Monospace, "Monospace"),
        };
        let face = iced::Font {
            family,
            ..kit_text::ui_font()
        };
        let message = Message::TextStyle(crate::editor::TextStyle { font, ..style });
        option(letter("Aa", face, 15.0), style.font == font, message, tip)
    });
    let bold = {
        let message = Message::TextStyle(crate::editor::TextStyle {
            bold: !style.bold,
            ..style
        });
        option(
            letter("B", kit_text::bold(), 16.0),
            style.bold,
            message,
            "Bold",
        )
    };
    let backs = TextBack::ALL.iter().map(|&back| {
        let (ring, fill, tip) = match back {
            TextBack::None => (false, false, "No backdrop"),
            TextBack::Outline => (true, false, "Outline: a halo that reads on any background"),
            TextBack::Box => (false, true, "Box: on a solid backdrop"),
        };
        let message = Message::TextStyle(crate::editor::TextStyle { back, ..style });
        option(chip::sample(ring, fill), style.back == back, message, tip)
    });
    let line =
        |items: Vec<Element<'static, Message>>| row(items).spacing(SPACING).align_y(iced::Center);
    let mut last = vec![bold, divide()];
    last.extend(backs);
    column![
        caption(Role::SECTION, "Size"),
        line(sizes.collect()),
        caption(Role::SECTION, "Font"),
        line(fonts.collect()),
        caption(Role::SECTION, "Style"),
        line(last),
    ]
    .spacing(SPACING)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Image;
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    fn editor() -> Editor {
        let image = Image {
            width: 4,
            height: 4,
            rgba: vec![255; 64],
        };
        Editor::new(image, 1.0)
    }

    /// The toolbar (Arrow active; the Text, Rectangle, Arrow and Blur tools'
    /// options; the colour popover) in Sapphire and Daylight.
    #[test]
    fn the_toolbar_matches_its_golden_images() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let mut text = editor();
            text.update(Message::Slot(Tool::Text));
            text.update(Message::Slot(Tool::Text));
            let mut colour = editor();
            colour.update(Message::Picker);
            // a click on the active tool: its options (one design for all)
            let options = |tool| {
                let mut e = editor();
                e.update(Message::Tool(tool));
                e.update(Message::Slot(tool));
                e
            };
            let wide = size(f32::INFINITY, false).width;
            for (name, e, height) in [
                ("rest", editor(), 60.0),
                ("text", text, 280.0),
                ("colour", colour, 440.0),
                ("options-rect", options(Tool::Rect), 260.0),
                ("options-arrow", options(Tool::Arrow), 260.0),
                ("options-blur", options(Tool::Blur), 200.0),
            ] {
                let file = format!("editor-toolbar-{name}-{}", theme.id);
                let at = Size::new(wide, height);
                failed.extend(golden::check("screens", &file, at, &[1.0], || {
                    view(&e, f32::INFINITY)
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
