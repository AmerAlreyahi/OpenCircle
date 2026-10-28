//! The floating editing toolbar (the design's `editor-toolbar.js`), composed
//! from kit parts only: grip · back · tools in groups (a tool with variants opens them
//! in a strip under it) · colour · board · scrolling · undo, redo · pin · copy,
//! save · drag. The open popover shows under its button.
#[cfg(test)]
use iced::widget::Space;
use iced::widget::{column, container, row, tooltip};
use iced::{Element, Point, Rectangle, Size, Vector};

use crate::editor::{
    Editor, Menu, Message, PRESETS, TOOLS, TextBack, TextFont, Tool, ToolInfo, Width, dragout, info,
};
use crate::ui::kit::icon_button::{self, State};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{self as kit_text, text as caption};
use crate::ui::kit::tokens::{Role, radius, size};
use crate::ui::kit::{chip, colour, divider, grip, segmented};
use crate::ui::screens::bar::geometry as bar;
use crate::ui::widgets::{self, Why};

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
const ADD: char = '\u{e1f6}'; // lucide "image-plus"
const STACK: char = '\u{e43d}'; // lucide "rows-2"
const SIDE: char = '\u{e09c}'; // lucide "columns-2"
const NUMBERS: char = '\u{e0f2}'; // lucide "hash"
const LEGEND: char = '\u{e10b}'; // lucide "list"
const SCROLL: char = '\u{e4d6}'; // lucide "gallery-vertical-end"
const ASSIST: char = '\u{e2b4}'; // lucide "magnet"
const PIPETTE: char = '\u{e13e}'; // lucide "pipette"

/// The glyphs used here (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[
    BACK, UNDO, REDO, COPY, SAVE, PIN, BOARD, ADD, STACK, SIDE, NUMBERS, LEGEND, ASSIST, SCROLL,
    PIPETTE,
];

/// The tool buttons: one per slot (variants live in its strip).
fn slots() -> impl Iterator<Item = &'static ToolInfo> {
    TOOLS.iter().filter(|t| t.slot == t.tool)
}

/// Pins are a Labs feature: the toolbar has the pin button only while it is on.
fn pins() -> bool {
    crate::labs::has(crate::labs::Feature::Pins)
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
    let mut rest = vec![CONTROL; 5 + usize::from(pins())];
    rest.extend([bar::DIVIDER, CONTROL, CONTROL]);
    if dragout::SUPPORTED {
        rest.extend([bar::DIVIDER, DRAG_WIDTH]);
    }
    (tools, rest)
}

/// The toolbar's size within `max_width` (as `view` lays it out without a
/// popover): one row, or two (tools, then the rest) when one does not fit.
pub fn size(max_width: f32) -> Size {
    let (tools, rest) = parts();
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
    let (bar, popover) = build(e, max_width);
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
pub fn view_at(
    e: &Editor,
    max_width: f32,
    bar: Rectangle,
) -> (Element<'_, Message>, Option<Element<'_, Message>>) {
    let (toolbar, popover) = build(e, max_width);
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

/// A button, named (and given its key) in a tooltip under it.
fn hinted<'a>(button: Element<'a, Message>, label: &'a str, why: Why<'a>) -> Element<'a, Message> {
    widgets::hint(button, label, why, tooltip::Position::Bottom)
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
) -> (Element<'_, Message>, Option<(Element<'_, Message>, f32)>) {
    let wrap = size(max_width).height > bar::HEIGHT;
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
        at.push((Menu::Variants(t.tool), x));
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
            colour::swatch(e.ink(), e.menu == Some(Menu::Colour), Message::Picker),
            "Colour, stroke, eyedropper, presets",
            Why::Icon,
        ),
        CONTROL,
    );
    at.push((Menu::Colour, x));
    let x = rest.push(
        hinted(
            icon_button::square(BOARD, open(Menu::Board), Some(Message::Menu(Menu::Board))),
            "Board: add captures, layout, legend",
            Why::Icon,
        ),
        CONTROL,
    );
    at.push((Menu::Board, x));
    let scrolling = if e.scroll {
        "Scrolling capture: scrolls the page under this area and stitches it"
    } else {
        "Scrolling capture: only when snipping a window"
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
    if pins() {
        rest.push(
            icon(
                PIN,
                false,
                Message::Pin,
                "Pin to screen: a floating copy (Ctrl+P)",
            ),
            CONTROL,
        );
    }
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
        Menu::Variants(slot) => variants_popover(e, slot),
    };
    let left = at.iter().find(|(m, _)| *m == menu).map_or(0.0, |(_, x)| *x);
    (toolbar.into(), Some((popover, left - bar::PAD)))
}

/// A popover's body: the toolbar's surface round `content`.
fn popover<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    surface(Material::Widget, radius::WINDOW, content)
        .padding(bar::PAD)
        .into()
}

/// A tool's variants (and the blur brush's line assist; the Text tool's options
/// under them).
fn variants_popover(e: &Editor, slot: Tool) -> Element<'_, Message> {
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
            "Line assist: the brush keeps to the text line (Shift: free)",
        ));
    }
    let tools = row(tools).spacing(SPACING).align_y(iced::Center);
    if slot == Tool::Text {
        popover(column![tools, text_options(e)].spacing(SPACING * 2.0))
    } else {
        popover(tools)
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

/// Add capture, then (with two or more) the layout and numbers, then the legend.
fn board_popover(e: &Editor) -> Element<'_, Message> {
    let board = e.doc.board;
    let mut items: Vec<Element<'_, Message>> = vec![icon(
        ADD,
        false,
        Message::AddCapture,
        "Add capture: pick another area to put beside this one",
    )];
    if board.count > 1 {
        items.push(divide());
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
    }
    items.push(divide());
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

    /// The toolbar (Arrow active; the Text tool's options; the colour
    /// popover) in Sapphire and Daylight.
    #[test]
    fn the_toolbar_matches_its_golden_images() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let mut text = editor();
            text.update(Message::Slot(Tool::Text));
            let mut colour = editor();
            colour.update(Message::Picker);
            let wide = size(f32::INFINITY).width;
            for (name, e, height) in [
                ("rest", editor(), 60.0),
                ("text", text, 280.0),
                ("colour", colour, 440.0),
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
