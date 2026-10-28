//! A list of options and its opener (the design's `.oc-dropdown-item` rows
//! and the live controller's language buttons): `opener` shows the current
//! choice with a ▾ (`accent` = the target: `accent_soft` with `accent` text),
//! `option` is one row of the open list: 12 px / 500 `text`, 6.5x10 px in,
//! radius 8, `hover` under the cursor, `active` (the current one) raised on
//! `elevated` with its edge; a `badge` at its end. States change colour only.
use iced::widget::{button, container, row, text};
use iced::{Border, Center, Element, Fill};

use super::button::faded;
use super::chip::badge as tag;
use super::text::{font, mono};
use super::themes::c;
use super::tokens::{Role, radius, space};

/// A row's corners (`rounded-lg`) and its room.
const ROW_RADIUS: f32 = 8.0;
const ROW_PAD: [f32; 2] = [6.5, 10.0];
/// The opener's room (`px-2.5`).
const OPENER_PAD: f32 = 10.0;

/// The opener of a list: `label` and a ▾, `width` x `height`, the label
/// left; `accent` = the target's look. Lighter under the cursor.
pub fn opener<'a, Message: Clone + 'a>(
    label: String,
    accent: bool,
    width: f32,
    height: f32,
    on_press: Message,
) -> Element<'a, Message> {
    use button::Status;
    let role = match accent {
        true => Role {
            weight: iced::font::Weight::Semibold,
            ..Role::CONTROL
        },
        false => Role::CONTROL,
    };
    let content = row![
        container(text(label).size(role.size).font(font(role))).width(Fill),
        text("▾").size(12).font(mono(iced::font::Weight::Normal))
    ]
    .spacing(space::GAP)
    .align_y(Center);
    button(container(content).center_y(Fill))
        .width(width)
        .height(height)
        .padding([0.0, OPENER_PAD])
        .on_press(on_press)
        .style(move |_, status| {
            let hot = matches!(status, Status::Hovered | Status::Pressed);
            let (fill, colour) = match (accent, hot) {
                (true, false) => (c().accent_soft, c().accent),
                (true, true) => (faded(c().accent, 0.24), c().accent),
                (false, false) => (c().hover, c().text),
                (false, true) => (c().elevated_hover, c().text),
            };
            button::Style {
                background: Some(fill.into()),
                text_color: colour,
                border: Border::default()
                    .rounded(radius::CARD)
                    .width(1)
                    .color(c().divider),
                ..button::Style::default()
            }
        })
        .into()
}

/// One option: `label`, a `badge` at its end, `width` x `height`.
pub fn option<'a, Message: Clone + 'a>(
    label: String,
    badge: Option<&'a str>,
    active: bool,
    (width, height): (f32, f32),
    on_press: Message,
) -> Element<'a, Message> {
    use button::Status;
    let role = Role::CONTROL;
    let content = row![container(text(label).size(role.size).font(font(role))).width(Fill)]
        .push(badge.map(tag))
        .spacing(space::GAP)
        .align_y(Center);
    button(container(content).center_y(Fill))
        .width(width)
        .height(height)
        .padding(ROW_PAD)
        .on_press(on_press)
        .style(move |_, status| {
            let hot = matches!(status, Status::Hovered | Status::Pressed);
            let (fill, edge) = match () {
                _ if active => (Some(c().elevated), c().elevated_border),
                _ if hot => (Some(c().hover), iced::Color::TRANSPARENT),
                _ => (None, iced::Color::TRANSPARENT),
            };
            button::Style {
                background: fill.map(Into::into),
                text_color: c().text,
                border: Border::default().rounded(ROW_RADIUS).width(1).color(edge),
                ..button::Style::default()
            }
        })
        .into()
}
