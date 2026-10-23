//! The floating editing toolbar: back · tools (grouped) · colour · undo/redo ·
//! Copy, Save. Built only from the shared widgets.
use iced::widget::{Space, column, row, tooltip};
use iced::{Color, Element};

use super::{Editor, Message, TOOLS, Width};
use crate::ui::theme;
use crate::ui::widgets::{self, CONTROL, SURFACE_PADDING};

const SPACING: f32 = 4.0;
const BACK: char = '\u{e04c}'; // lucide "arrow-left"
const UNDO: char = '\u{e2a0}'; // lucide "undo-2"
const REDO: char = '\u{e29f}'; // lucide "redo-2"
const COPY: char = '\u{e0a2}'; // lucide "copy"
const SAVE: char = '\u{e0b6}'; // lucide "download"
const PIN: char = '\u{e258}'; // lucide "pin"

pub fn view(e: &Editor) -> Element<'_, Message> {
    let side = tooltip::Position::Bottom;
    let icon =
        |glyph: char, active: bool, message: Message, tip: &'static str| -> Element<'_, Message> {
            widgets::with_tip(widgets::icon_button(glyph, active, message), tip, side).into()
        };
    let mut bar: Vec<Element<'_, Message>> = Vec::new();
    // Left edge of the next control, for placing the colour popover under its button.
    let mut x = SURFACE_PADDING;
    let mut step = |width: f32| x += width + SPACING;

    bar.push(icon(
        BACK,
        false,
        Message::Back,
        "Back to the actions (Esc)",
    ));
    step(CONTROL);
    let mut group = None;
    for t in TOOLS {
        if group != Some(t.group) {
            bar.push(widgets::toolbar_divider());
            step(1.0);
            group = Some(t.group);
        }
        bar.push(icon(t.icon, e.tool == t.tool, Message::Tool(t.tool), t.tip));
        step(CONTROL);
    }
    bar.push(widgets::toolbar_divider());
    step(1.0);
    let colour = Color::from_rgba8(
        e.colour[0],
        e.colour[1],
        e.colour[2],
        e.colour[3] as f32 / 255.0,
    );
    let colour_at = x;
    bar.push(
        widgets::with_tip(
            widgets::colour_button(colour, e.picker, Message::Picker),
            "Colour and stroke",
            side,
        )
        .into(),
    );
    bar.push(widgets::toolbar_divider());
    bar.push(icon(UNDO, false, Message::Undo, "Undo (Ctrl+Z)"));
    bar.push(icon(REDO, false, Message::Redo, "Redo (Ctrl+Shift+Z)"));
    bar.push(widgets::toolbar_divider());
    bar.push(icon(
        PIN,
        false,
        Message::Pin,
        "Pin to screen: a floating copy (Ctrl+P)",
    ));
    bar.push(widgets::text_button(COPY, "Copy", "Ctrl C".into(), true, Message::Copy).into());
    bar.push(widgets::text_button(SAVE, "Save", "Ctrl S".into(), false, Message::Save).into());

    let bar = widgets::surface(row(bar).spacing(SPACING).align_y(iced::Center));
    if !e.picker {
        return bar.into();
    }
    let popover = widgets::colour_popover(
        &theme::ANNOTATION,
        colour,
        e.width as usize,
        &e.hex,
        |c| Message::Colour(c.into_rgba8()),
        |i| Message::Width(Width::ALL[i]),
        Message::Hex,
    );
    column![
        bar,
        row![Space::new().width(colour_at - SURFACE_PADDING), popover]
    ]
    .spacing(8)
    .into()
}
