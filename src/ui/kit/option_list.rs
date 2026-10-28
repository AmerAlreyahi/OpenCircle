//! A list of options and its opener (the design's `.oc-dropdown-item` rows
//! and the live controller's language buttons): `opener` shows the current
//! choice with a ▾ (`accent` = the target: `accent_soft` with `accent` text),
//! `option` is one row of the open list: 12 px / 500 `text`, 6.5x10 px in,
//! radius 8, `hover` under the cursor, `active` (the current one) raised on
//! `elevated` with its edge; a `badge` at its end. States change colour only.
//! A menu (`menu`, `item`, `separator`: the tray menu) is the same kind of
//! list: plain rows with a shortcut hint.
use iced::widget::{Space, button, column, container, row, text};
use iced::{Border, Center, Element, Fill, Padding};

use super::button::faded;
use super::chip::badge as tag;
use super::divider::divider;
use super::icon::{Icon, icon};
use super::surface::{Material, surface};
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

/// A menu (the design's tray menu): `MENU_WIDTH` wide, its `items` and
/// `separator`s on the widget surface with `radius::MENU` corners, 4 px in
/// (`p-1`, inside the 1 px edge). As tall as `menu_height` says.
pub fn menu<'a, Message: 'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    surface(Material::Widget, radius::MENU, column(items))
        .padding(MENU_PAD)
        .width(MENU_WIDTH)
        .into()
}

/// The menu's width, a row's height, the room around the rows (edge + `p-1`)
/// and a separator's height (1 px, `my-1`).
pub const MENU_WIDTH: f32 = 240.0;
pub const ITEM_H: f32 = 32.0;
const MENU_PAD: f32 = 5.0;
const SEPARATOR_PAD: [f32; 2] = [4.0, 8.0];
const SEPARATOR_H: f32 = 9.0;
/// The room before the check mark (`ml-1`), before the hint (`pl-2`) and in a
/// row (`px-2`); the check's size (`w-3`).
const CHECK_GAP: f32 = 4.0;
const HINT_GAP: f32 = 8.0;
const ITEM_PAD: f32 = 8.0;
const CHECK: f32 = 12.0;

/// A menu of `items` rows and `separators` lines: its height.
pub fn menu_height(items: usize, separators: usize) -> f32 {
    2.0 * MENU_PAD + items as f32 * ITEM_H + separators as f32 * SEPARATOR_H
}

/// A menu's hairline between groups (`h-px my-1 mx-2`).
pub fn separator<'a, Message: 'a>() -> Element<'a, Message> {
    container(divider()).padding(SEPARATOR_PAD).into()
}

/// A menu row: `label` (13 px `text`), a muted ✓ after it when `checked`
/// (a toggle that is on), the muted `hint` (its shortcut) at the end; the
/// whole menu wide, `ITEM_H` tall, `hover` under the cursor or when
/// `focused` (the keyboard's row). Quit is no different (not red).
pub fn item<'a, Message: Clone + 'a>(
    label: &'a str,
    checked: bool,
    hint: Option<String>,
    focused: bool,
    on_press: Message,
) -> Element<'a, Message> {
    use button::Status;
    let (role, small) = (Role::MENU, Role::MENU_HINT);
    let check = checked.then(|| {
        container(icon(Icon::Check, CHECK).color(c().text_muted)).padding(Padding {
            left: CHECK_GAP,
            ..Padding::ZERO
        })
    });
    let hint = hint.map(|h| {
        container(
            text(h)
                .size(small.size)
                .font(font(small))
                .color(c().text_muted),
        )
        .padding(Padding {
            left: HINT_GAP,
            ..Padding::ZERO
        })
    });
    let content = row![text(label).size(role.size).font(font(role))]
        .push(check)
        .push(Space::new().width(Fill))
        .push(hint)
        .align_y(Center);
    button(container(content).center_y(Fill))
        .width(Fill)
        .height(ITEM_H)
        .padding([0.0, ITEM_PAD])
        .on_press(on_press)
        .style(move |_, status| {
            let hot = focused || matches!(status, Status::Hovered | Status::Pressed);
            button::Style {
                background: hot.then(|| c().hover.into()),
                text_color: c().text,
                border: Border::default().rounded(radius::MENU_ITEM),
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
