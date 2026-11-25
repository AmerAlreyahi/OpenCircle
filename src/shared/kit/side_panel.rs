//! A window's side panel (Studio's Steps sidebar): its own
//! 40 px header (a name and the button that hides it), a hairline, and a body
//! that scrolls, on the `sidebar` fill. Also the parts its lists are made of:
//! a collapsible section header (30 px), a list row (a thumbnail row or an
//! object row, hover shade, `accent_soft` while selected) and the menu bar's
//! opener (28 px).
use iced::widget::{button, column, container};
use iced::{Border, Center, Element, Fill, Length, Padding};

use super::themes::c;
use super::tokens::{Role, size};
use super::{divider, scroll};

const OPENER_H: f32 = 28.0;
const OPENER_R: f32 = 8.0;
const PAD: f32 = 12.0;

/// The panel: `title`, a hairline, `body`. (Its show / hide button is the window's:
/// the same place whether the panel is open or closed.)
pub fn panel<'a, Message: Clone + 'a>(
    width: f32,
    title: &'a str,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let head = container(
        crate::dir_row![super::text::text(Role::CONTROL, title).width(Fill)].align_y(Center),
    )
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
    super::focus::press(b, OPENER_R, Some(press)).into()
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

/// Between two `section` items.
pub const SECTION_GAP: f32 = 2.0;
const BAR_W: f32 = 3.0;
const BAR_H: f32 = 18.0;
const SECTION_ICON_GAP: f32 = 10.0;

/// One section in a dialog's list (the Settings shell's sidebar item): its icon (a lucide glyph)
/// and name; the active one is a raised pill with a thin accent bar at its start edge.
pub fn section<'a, Message: Clone + 'a>(
    glyph: char,
    label: &'a str,
    active: bool,
    on_press: Message,
) -> Element<'a, Message> {
    use iced::widget::Space;
    let bar = container(Space::new())
        .width(BAR_W)
        .height(BAR_H)
        .style(move |_| container::Style {
            background: active.then(|| c().accent.into()),
            border: Border::default().rounded(BAR_W),
            ..container::Style::default()
        });
    let glyph =
        super::icon::glyph(glyph, size::ICON).color(if active { c().text } else { c().text_sub });
    let name = match active {
        true => super::text::text(Role::NAV, label),
        false => super::text::sub(Role::NAV, label),
    };
    let line = super::dir::row([
        bar.into(),
        glyph.into(),
        name.into(),
        Space::new().width(Fill).into(),
    ])
    .spacing(SECTION_ICON_GAP)
    .height(Fill)
    .align_y(Center);
    super::focus::press(
        button(line)
            .width(Fill)
            .height(size::NAV_ITEM)
            .padding(super::dir::pad(Padding {
                left: 6.0,
                right: 10.0,
                ..Padding::ZERO
            }))
            .style(move |_, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: match (active, hot) {
                        (true, _) => Some(c().elevated.into()),
                        (false, true) => Some(c().hover.into()),
                        _ => None,
                    },
                    border: Border::default()
                        .rounded(super::tokens::radius::CONTROL)
                        .width(if active { 1.0 } else { 0.0 })
                        .color(c().elevated_border),
                    ..button::Style::default()
                }
            }),
        super::tokens::radius::CONTROL,
        Some(on_press),
    )
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
