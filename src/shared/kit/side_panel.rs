//! The parts a window's side lists are made of:
//! a collapsible section header (30 px), a list row (a thumbnail row or an
//! object row, hover shade, `accent_soft` while selected) and the menu bar's
//! opener (28 px).
use iced::widget::{button, container};
use iced::{Border, Element, Fill};

use super::themes::c;
use super::tokens::Role;

const OPENER_H: f32 = 28.0;
const OPENER_R: f32 = 8.0;

/// A menu bar's opener: 28 high, radius 8, raised while its menu is `open`.
pub fn menu_opener<'a, Message: Clone + 'a>(
    words: &'a str,
    open: bool,
    press: Message,
    hover: Option<Message>,
) -> Element<'a, Message> {
    let label = super::text::text(Role::NAV, words);
    let b = button(container(label).center_y(Fill).padding([0.0, 11.0]))
        .height(OPENER_H)
        .padding(0)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let (fill, edge) = match () {
                _ if open => (Some(c().elevated), c().elevated_border),
                _ if hot => (Some(c().hover), iced::Color::TRANSPARENT),
                _ => (None, iced::Color::TRANSPARENT),
            };
            button::Style {
                background: fill.map(Into::into),
                text_color: if open || hot { c().text } else { c().text_sub },
                border: Border::default().rounded(OPENER_R).width(1).color(edge),
                ..button::Style::default()
            }
        });
    let opener = super::focus::press(b, OPENER_R, Some(press));
    // (menu mode: the pointer coming over another title switches to its menu)
    match hover {
        Some(on_enter) => iced::widget::mouse_area(opener).on_enter(on_enter).into(),
        None => opener.into(),
    }
}
