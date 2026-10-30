//! `icon_button(glyph, state, on_press)`: `.oc-icon-btn` round, 34x34, a
//! 16 px glyph in `text_sub`; under the cursor `text` on `hover`; `active`
//! (the one chosen) raised: `elevated` with its edge, `text`. Pressed draws a
//! 2 px inset edge (no scale on the CPU renderer); Tab's focus an
//! `accent_soft` fill (no ring); no `on_press` = dimmed,
//! no hover. Every state keeps the same layout.
use iced::widget::{button, container, text};
use iced::{Border, Element, Fill};

use super::button::faded;
use super::icon::FONT;
use super::themes::c;
use super::tokens::{radius, size};

/// What the button shows besides hover and press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    /// The one chosen (a tool, Live translate running).
    pub active: bool,
    /// Tab put the keyboard focus on it.
    pub focused: bool,
}

pub fn icon_button<'a, Message: Clone + 'a>(
    glyph: char,
    state: State,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    made(glyph_text(glyph), size::ICON_BUTTON / 2.0, state, on_press)
}

/// The same button with the design's square corners (r9): the editing
/// toolbar's tools, undo and redo, copy and save.
pub fn square<'a, Message: Clone + 'a>(
    glyph: char,
    state: State,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    made(glyph_text(glyph), radius::CONTROL, state, on_press)
}

/// A square button around any `content` (a letter, a sample): the text of
/// `content` takes the button's colours.
pub fn option<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    state: State,
    on_press: Message,
) -> Element<'a, Message> {
    made(content, radius::CONTROL, state, Some(on_press))
}

fn glyph_text<'a>(glyph: char) -> iced::widget::Text<'a> {
    text(glyph).font(FONT).size(size::ICON)
}

fn made<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    corner: f32,
    state: State,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use button::Status;
    let live = on_press.is_some();
    button(container(content).center(Fill))
        .width(size::ICON_BUTTON)
        .height(size::ICON_BUTTON)
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hot = live && matches!(status, Status::Hovered | Status::Pressed);
            let fill = match () {
                _ if state.active => Some(c().elevated),
                _ if state.focused => Some(c().accent_soft),
                _ if hot => Some(c().hover),
                _ => None,
            };
            let text_color = match () {
                _ if !live => faded(c().text_muted, 0.6),
                _ if state.active || hot || state.focused => c().text,
                _ => c().text_sub,
            };
            let (width, edge) = match () {
                _ if live && status == Status::Pressed => (2.0, c().widget),
                _ if state.active => (1.0, c().elevated_border),
                _ => (0.0, c().widget),
            };
            button::Style {
                background: fill.map(Into::into),
                text_color,
                border: Border::default().rounded(corner).width(width).color(edge),
                ..button::Style::default()
            }
        })
        .into()
}
