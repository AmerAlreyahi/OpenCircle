//! A window's side panel (Studio's Steps sidebar): its own
//! 40 px header (a name and the button that hides it), a hairline, and a body
//! that scrolls, on the `sidebar` fill. Also the parts its lists are made of:
//! a collapsible section header (30 px), a list row (a thumbnail row or an
//! object row, hover shade, `accent_soft` while selected) and the menu bar's
//! opener (28 px).
use iced::widget::{button, column, container};
use iced::{Border, Element, Fill, Length, Padding};

use super::themes::c;
use super::tokens::{Role, size};
use super::{divider, scroll};

const OPENER_H: f32 = 28.0;
const OPENER_R: f32 = 8.0;
const PAD: f32 = 12.0;

/// The panel: `head` (its name, and the buttons that pin or hide it, in one row), a hairline,
/// `body`.
pub fn panel<'a, Message: Clone + 'a>(
    width: f32,
    head: Element<'a, Message>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let head = container(head)
        .padding(Padding {
            left: PAD,
            right: PAD,
            ..Padding::ZERO
        })
        .height(size::HEADER)
        .center_y(size::HEADER);
    container(column![
        head,
        divider::divider(),
        scroll::scroll(body).height(Fill)
    ])
    .width(Length::Fixed(width))
    .height(Fill)
    .style(|_| container::Style {
        background: Some(c().sidebar.into()),
        ..container::Style::default()
    })
    .into()
}

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

/// A column inside a card, on its start side (the left, or the right right to left): `width`
/// wide, as tall as the card, recessed (a darker fill) with the card's own corners on its
/// outer side (`corner`: the card's radius less its edge). A dialog's section list.
pub fn inset<'a, Message: 'a>(
    content: Element<'a, Message>,
    width: f32,
    corner: f32,
) -> Element<'a, Message> {
    let outer = match super::dir::rtl() {
        true => [0.0, corner, corner, 0.0],
        false => [corner, 0.0, 0.0, corner],
    };
    container(content)
        .width(width)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(c().track_glass.into()),
            border: Border::default().rounded(iced::border::Radius {
                top_left: outer[0],
                top_right: outer[1],
                bottom_right: outer[2],
                bottom_left: outer[3],
            }),
            ..container::Style::default()
        })
        .into()
}

/// The page beside an `inset` list, inside the same card: on a light theme a grey pane (the
/// window's own fill) so white cards (`setting_row::card`) stand on it, its outer corners the
/// card's own; dark themes keep the card's surface.
pub fn pane<'a, Message: 'a>(content: Element<'a, Message>, corner: f32) -> Element<'a, Message> {
    let outer = match super::dir::rtl() {
        true => [corner, 0.0, 0.0, corner],
        false => [0.0, corner, corner, 0.0],
    };
    container(content)
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style {
            background: super::themes::current().light.then(|| c().bg.into()),
            border: Border::default().rounded(iced::border::Radius {
                top_left: outer[0],
                top_right: outer[1],
                bottom_right: outer[2],
                bottom_left: outer[3],
            }),
            ..container::Style::default()
        })
        .into()
}
