//! The floating editing toolbar: back · tools (grouped; a tool with variants
//! opens them in a strip under it) · colour · board · undo/redo · drag, pin ·
//! Copy, Save. The open popover shows under its button. Built only from the
//! shared widgets.
#[cfg(test)]
use iced::widget::Space;
use iced::widget::{column, row, tooltip};
use iced::{Color, Element, Point, Rectangle, Size, Vector};

use super::board::Layout;
use super::{Editor, Menu, Message, PRESETS, TOOLS, Tool, Width, dragout, info};
use crate::ui::theme;
use crate::ui::widgets::{self, CONTROL, SURFACE_PADDING, TEXT_BUTTON_WIDTH};

const SPACING: f32 = 4.0;
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

/// The glyphs used here (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[
    BACK, UNDO, REDO, COPY, SAVE, PIN, BOARD, ADD, STACK, SIDE, NUMBERS, LEGEND, ASSIST, SCROLL,
];

/// The tool buttons: one per slot (variants live in its strip).
fn slots() -> impl Iterator<Item = &'static super::ToolInfo> {
    TOOLS.iter().filter(|t| t.slot == t.tool)
}

/// Widths (no padding) of the toolbar's two parts: back and the tools (a
/// divider before each group), and the rest (a divider, colour and board, a
/// divider, undo/redo, a divider, drag where supported, pin, Copy and Save).
fn parts() -> (f32, f32) {
    let buttons = slots().count();
    let groups = 1 + TOOLS
        .windows(2)
        .filter(|w| w[0].group != w[1].group)
        .count();
    let tools = (1 + buttons) as f32 * CONTROL + groups as f32;
    let tools = tools + (buttons + groups) as f32 * SPACING;
    let icons = 5 + usize::from(pins()) + usize::from(dragout::SUPPORTED);
    let rest = icons as f32 * CONTROL + 3.0 + 2.0 * TEXT_BUTTON_WIDTH;
    let rest = rest + (icons + 3 + 2 - 1) as f32 * SPACING;
    (tools, rest)
}

/// Pins are a Labs feature: the toolbar has the pin button only while it is on.
fn pins() -> bool {
    crate::labs::has(crate::labs::Feature::Pins)
}

/// The toolbar's size within `max_width` (as `view` lays it out without a
/// popover): one row, or two (tools, then the rest) when one does not fit.
pub fn size(max_width: f32) -> Size {
    let (tools, rest) = parts();
    let one = tools + SPACING + rest + 2.0 * SURFACE_PADDING;
    if one <= max_width {
        Size::new(one, CONTROL + 2.0 * SURFACE_PADDING)
    } else {
        // The second row starts without its divider.
        let width = tools.max(rest - 1.0 - SPACING) + 2.0 * SURFACE_PADDING;
        Size::new(width, 2.0 * CONTROL + SPACING + 2.0 * SURFACE_PADDING)
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

/// The toolbar, and the open popover with its left edge (from the toolbar's).
#[allow(clippy::type_complexity)]
fn build(
    e: &Editor,
    max_width: f32,
) -> (Element<'_, Message>, Option<(Element<'_, Message>, f32)>) {
    let side = tooltip::Position::Bottom;
    let icon =
        |glyph: char, active: bool, message: Message, tip: &'static str| -> Element<'_, Message> {
            widgets::hint(
                widgets::icon_button(glyph, active, message),
                tip,
                widgets::Why::Icon,
                side,
            )
        };
    let wrap = size(max_width).height > CONTROL + 2.0 * SURFACE_PADDING;
    let mut tools: Vec<Element<'_, Message>> = Vec::new();
    // Left edge of the next control, for placing a popover under its button.
    let mut x = SURFACE_PADDING;
    let mut at: Vec<(Menu, f32)> = Vec::new();
    let step = |width: f32, x: &mut f32| *x += width + SPACING;

    tools.push(icon(
        BACK,
        false,
        Message::Back,
        "Back to the actions (Esc)",
    ));
    step(CONTROL, &mut x);
    let mut group = None;
    let current = info(e.tool);
    for t in slots() {
        if group != Some(t.group) {
            tools.push(widgets::toolbar_divider());
            step(1.0, &mut x);
            group = Some(t.group);
        }
        // The button shows the variant in use.
        let shown = if current.slot == t.tool { current } else { t };
        at.push((Menu::Variants(t.tool), x));
        tools.push(icon(
            shown.icon,
            current.slot == t.tool,
            Message::Slot(t.tool),
            shown.tip,
        ));
        step(CONTROL, &mut x);
    }
    let mut rest: Vec<Element<'_, Message>> = Vec::new();
    if wrap {
        x = SURFACE_PADDING; // the second row
    } else {
        rest.push(widgets::toolbar_divider());
        step(1.0, &mut x);
    }
    let colour = Color::from_rgba8(
        e.colour[0],
        e.colour[1],
        e.colour[2],
        e.colour[3] as f32 / 255.0,
    );
    at.push((Menu::Colour, x));
    rest.push(widgets::hint(
        widgets::colour_button(colour, e.menu == Some(Menu::Colour), Message::Picker),
        "Colour, stroke, eyedropper, presets",
        widgets::Why::Icon,
        side,
    ));
    step(CONTROL, &mut x);
    at.push((Menu::Board, x));
    rest.push(icon(
        BOARD,
        e.menu == Some(Menu::Board),
        Message::Menu(Menu::Board),
        "Board: add captures, layout, legend",
    ));
    rest.push(widgets::hint(
        widgets::icon_button_maybe(SCROLL, false, e.scroll.then_some(Message::Scroll)),
        if e.scroll {
            "Scrolling capture: scrolls the page under this area and stitches it"
        } else {
            "Scrolling capture: only when snipping a window"
        },
        widgets::Why::Icon,
        side,
    ));
    rest.push(widgets::toolbar_divider());
    rest.push(icon(UNDO, false, Message::Undo, "Undo (Ctrl+Z)"));
    rest.push(icon(REDO, false, Message::Redo, "Redo (Ctrl+Shift+Z)"));
    rest.push(widgets::toolbar_divider());
    if dragout::SUPPORTED {
        rest.push(widgets::hint(
            widgets::drag_handle(widgets::DRAG, Message::DragOut),
            "Drag the image into another app (an AI chat, a document)",
            widgets::Why::Icon,
            side,
        ));
    }
    if pins() {
        rest.push(icon(
            PIN,
            false,
            Message::Pin,
            "Pin to screen: a floating copy (Ctrl+P)",
        ));
    }
    rest.push(widgets::text_button(COPY, "Copy", "Ctrl C".into(), true, Message::Copy).into());
    rest.push(widgets::text_button(SAVE, "Save", "Ctrl S".into(), false, Message::Save).into());

    fn line(items: Vec<Element<'_, Message>>) -> iced::widget::Row<'_, Message> {
        row(items).spacing(SPACING).align_y(iced::Center)
    }
    let bar = if wrap {
        widgets::surface(column![line(tools), line(rest)].spacing(SPACING))
    } else {
        tools.extend(rest);
        widgets::surface(line(tools))
    };
    let Some(menu) = e.menu else {
        return (bar.into(), None);
    };
    let popover = match menu {
        Menu::Colour => colour_popover(e, colour),
        Menu::Board => board_popover(e),
        Menu::Variants(slot) => {
            let mut tools: Vec<Element<'_, Message>> = (TOOLS.iter())
                .filter(|t| t.slot == slot)
                .map(|t| {
                    widgets::hint(
                        widgets::icon_button(t.icon, e.tool == t.tool, Message::Tool(t.tool)),
                        t.tip,
                        widgets::Why::Icon,
                        side,
                    )
                })
                .collect();
            // The blur brush's option.
            if slot == Tool::Blur {
                tools.push(widgets::toolbar_divider());
                tools.push(icon(
                    ASSIST,
                    e.assist,
                    Message::Assist,
                    "Line assist: the brush keeps to the text line (Shift: free)",
                ));
            }
            let tools = row(tools).spacing(SPACING).align_y(iced::Center);
            // The text's options (size, font, weight, backdrop) under its variants.
            if slot == Tool::Text {
                widgets::surface(column![tools, text_options(e)].spacing(2.0 * SPACING)).into()
            } else {
                widgets::surface(tools).into()
            }
        }
    };
    let left = at.iter().find(|(m, _)| *m == menu).map_or(0.0, |(_, x)| *x);
    (bar.into(), Some((popover, left - SURFACE_PADDING)))
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

/// Colour, stroke, hex + eyedropper, the presets (keys 1–9), copy / paste style.
fn colour_popover(e: &Editor, colour: Color) -> Element<'_, Message> {
    let preset = |i: usize| -> Element<'_, Message> {
        let p = &PRESETS[i];
        widgets::hint(
            widgets::wide_chip(p.short, Message::Preset(i)),
            p.name,
            widgets::Why::Keys(p.key),
            tooltip::Position::Bottom,
        )
    };
    let pairs = (0..PRESETS.len())
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|pair| row(pair.iter().map(|&i| preset(i))).spacing(6).into())
        .collect::<Vec<Element<'_, Message>>>();
    let style = row![
        widgets::wide_chip("Copy style", Message::CopyStyle),
        widgets::wide_chip("Paste style", Message::PasteStyle),
    ]
    .spacing(6);
    widgets::colour_popover(
        &super::style::ANNOTATION,
        colour,
        e.width as usize,
        &e.hex,
        |c| Message::Colour(c.into_rgba8()),
        |i| Message::Width(Width::ALL[i]),
        Message::Hex,
        Message::Eyedropper,
        column![widgets::caption("Presets"), column(pairs).spacing(6), style]
            .spacing(6)
            .into(),
    )
    .into()
}

/// Add capture, then (with two or more) the layout and numbers, then the legend.
fn board_popover(e: &Editor) -> Element<'_, Message> {
    let side = tooltip::Position::Bottom;
    let icon = |glyph, active, message, tip| -> Element<'_, Message> {
        widgets::hint(
            widgets::icon_button(glyph, active, message),
            tip,
            widgets::Why::Icon,
            side,
        )
    };
    let board = e.doc.board;
    let mut items: Vec<Element<'_, Message>> = vec![icon(
        ADD,
        false,
        Message::AddCapture,
        "Add capture: pick another area to put beside this one",
    )];
    if board.count > 1 {
        items.push(widgets::toolbar_divider());
        for (glyph, layout, tip) in [
            (STACK, Layout::Stack, "Stacked"),
            (SIDE, Layout::Side, "Side by side"),
            (BOARD, Layout::Grid, "Grid"),
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
    items.push(widgets::toolbar_divider());
    items.push(icon(
        LEGEND,
        e.doc.legend,
        Message::Legend,
        "Legend: the steps' notes in a box",
    ));
    widgets::surface(row(items).spacing(SPACING).align_y(iced::Center)).into()
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
    use super::text::{Back, Font};
    use iced::widget::{container, text};
    let side = tooltip::Position::Bottom;
    let style = e.text_style;
    let fg = |on: bool| {
        if on {
            theme::ON_ACCENT
        } else {
            theme::floating().text
        }
    };
    let option = |content: Element<'static, Message>, on, message, tip| {
        widgets::hint(
            widgets::option_button(content, on, message),
            tip,
            widgets::Why::Icon,
            side,
        )
    };
    let letter = |label: &'static str, font: iced::Font, size: f32, on: bool| {
        Element::from(text(label).font(font).size(size).color(fg(on)))
    };
    let sizes = TEXT_SIZES.iter().enumerate().map(|(i, &(px, tip))| {
        let on = (e.text_size - px).abs() < 0.5;
        let glyph = letter("A", iced::Font::DEFAULT, 11.0 + 2.5 * i as f32, on);
        option(glyph, on, Message::TextSize(px), tip)
    });
    let fonts = Font::ALL.iter().map(|&font| {
        let (family, tip) = match font {
            Font::Sans => (iced::font::Family::SansSerif, "Sans-serif"),
            Font::Serif => (iced::font::Family::Serif, "Serif"),
            Font::Mono => (iced::font::Family::Monospace, "Monospace"),
        };
        let on = style.font == font;
        let face = iced::Font {
            family,
            ..iced::Font::DEFAULT
        };
        let message = Message::TextStyle(super::TextStyle { font, ..style });
        option(letter("Aa", face, 15.0, on), on, message, tip)
    });
    let bold = {
        let face = iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        };
        let message = Message::TextStyle(super::TextStyle {
            bold: !style.bold,
            ..style
        });
        option(
            letter("B", face, 16.0, style.bold),
            style.bold,
            message,
            "Bold",
        )
    };
    let backs = Back::ALL.iter().map(|&back| {
        let on = style.back == back;
        let (ring, fill, tip) = match back {
            Back::None => (0.0, false, "No backdrop"),
            Back::Outline => (1.5, false, "Outline: a halo that reads on any background"),
            Back::Box => (0.0, true, "Box: on a solid backdrop"),
        };
        // A small preview: the letter plain, ringed, or on a fill.
        let ink = if fill { fg(!on) } else { fg(on) };
        let preview = container(text("A").size(14).color(ink))
            .padding([0, 4])
            .style(move |_| container::Style {
                background: fill.then(|| fg(on).into()),
                border: iced::Border {
                    color: fg(on),
                    width: ring,
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            });
        let message = Message::TextStyle(super::TextStyle { back, ..style });
        option(preview.into(), on, message, tip)
    });
    let line =
        |items: Vec<Element<'static, Message>>| row(items).spacing(SPACING).align_y(iced::Center);
    let mut last = vec![bold, widgets::toolbar_divider()];
    last.extend(backs);
    column![
        widgets::caption("Size"),
        line(sizes.collect()),
        widgets::caption("Font"),
        line(fonts.collect()),
        widgets::caption("Style"),
        line(last),
    ]
    .spacing(SPACING)
    .into()
}
