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
use super::tokens::{radius, size, widget_e};

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
/// `content` takes the button's colours; no `on_press` = dimmed.
pub fn option<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    state: State,
    on_press: impl Into<Option<Message>>,
) -> Element<'a, Message> {
    made(content, radius::CONTROL, state, on_press.into())
}

/// Style E floating-widget icon button (`.ib`): 34x34, 10 px corners,
/// `accent_soft` + `accent` when active (Translate Original/Glossary, bar Live,
/// Chat Pin), `hover` under the cursor, dimmed to 38 % when disabled.
pub fn pill_icon<'a, Message: Clone + 'a>(
    glyph: char,
    state: State,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    made_e(
        glyph_text(glyph),
        widget_e::ICON_RADIUS,
        true,
        state,
        on_press,
    )
}

/// Style E toolbar tool button (`.ib` with `.on`): 34x34, 10 px corners,
/// raised `elevated` with its border when active (Snip tools), `hover` under
/// the cursor, dimmed to 38 % when disabled.
pub fn tool_icon<'a, Message: Clone + 'a>(
    glyph: char,
    state: State,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    made_e(
        glyph_text(glyph),
        widget_e::ICON_RADIUS,
        false,
        state,
        on_press,
    )
}

/// Style E toolbar tool button around custom content (e.g. the OpenCircle logo).
pub fn tool_option<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    state: State,
    on_press: impl Into<Option<Message>>,
) -> Element<'a, Message> {
    made_e(
        content,
        widget_e::ICON_RADIUS,
        false,
        state,
        on_press.into(),
    )
}

/// Style E quiet close button (`.qx`): 28x28 round, `text_muted` at rest,
/// `hover` + `text` under the cursor, `accent_soft` when keyboard-focused.
pub fn quiet_close<'a, Message: Clone + 'a>(
    focused: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use button::Status;
    let live = on_press.is_some();
    let corner = radius::FULL;
    super::focus::press(
        button(container(glyph_text(super::icon::Icon::Close.glyph())).center(Fill))
            .width(widget_e::QUIET_X)
            .height(widget_e::QUIET_X)
            .padding(0)
            .style(move |_, status| {
                let hot = live && matches!(status, Status::Hovered | Status::Pressed);
                let fill = match () {
                    _ if focused => Some(c().accent_soft),
                    _ if hot => Some(c().hover),
                    _ => None,
                };
                let text_color = match () {
                    _ if !live => faded(c().text_muted, 0.38),
                    _ if hot || focused => c().text,
                    _ => c().text_muted,
                };
                button::Style {
                    background: fill.map(Into::into),
                    text_color,
                    border: Border::default().rounded(corner),
                    ..button::Style::default()
                }
            }),
        corner,
        on_press,
    )
    .into()
}

fn made_e<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    corner: f32,
    accent_active: bool,
    state: State,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use button::Status;
    let live = on_press.is_some();
    super::focus::press(
        button(container(content).center(Fill))
            .width(size::ICON_BUTTON)
            .height(size::ICON_BUTTON)
            .padding(0)
            .style(move |_, status| {
                let hot = live && matches!(status, Status::Hovered | Status::Pressed);
                let fill = match () {
                    _ if state.active && accent_active => Some(c().accent_soft),
                    _ if state.active => Some(c().elevated),
                    _ if state.focused => Some(c().accent_soft),
                    _ if hot => Some(c().hover),
                    _ => None,
                };
                let text_color = match () {
                    _ if !live => faded(c().text_sub, 0.38),
                    _ if state.active && accent_active => c().accent,
                    _ if state.active || hot || state.focused => c().text,
                    _ => c().text_sub,
                };
                let (width, edge) = match () {
                    _ if live && status == Status::Pressed => (2.0, c().widget),
                    _ if state.active && !accent_active => (1.0, c().elevated_border),
                    _ => (0.0, c().widget),
                };
                button::Style {
                    background: fill.map(Into::into),
                    text_color,
                    border: Border::default().rounded(corner).width(width).color(edge),
                    ..button::Style::default()
                }
            }),
        corner,
        on_press,
    )
    .into()
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
    super::focus::press(
        button(container(content).center(Fill))
            .width(size::ICON_BUTTON)
            .height(size::ICON_BUTTON)
            .padding(0)
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
            }),
        corner,
        on_press,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::shot;
    use crate::shared::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    #[test]
    fn style_e_icon_buttons_render_and_fire() {
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let close = quiet_close(false, Some(1u8));
            assert_eq!(
                shot::click(
                    close,
                    iced::Size::new(28.0, 28.0),
                    iced::Point::new(14.0, 14.0)
                ),
                vec![1]
            );
            let active = pill_icon(
                super::super::icon::Icon::Eye.glyph(),
                State {
                    active: true,
                    focused: false,
                },
                Some(2u8),
            );
            assert_eq!(
                shot::click(
                    active,
                    iced::Size::new(34.0, 34.0),
                    iced::Point::new(17.0, 17.0)
                ),
                vec![2]
            );
            let tool = tool_icon(
                super::super::icon::Icon::Pin.glyph(),
                State {
                    active: true,
                    focused: false,
                },
                Some(3u8),
            );
            assert_eq!(
                shot::click(
                    tool,
                    iced::Size::new(34.0, 34.0),
                    iced::Point::new(17.0, 17.0)
                ),
                vec![3]
            );
        }
        themes::set(&SAPPHIRE);
    }
}
