//! A dialog's page list (`list`): items as an icon and a label, the open one a raised pill with a
//! thin accent bar at its start edge (the Studio Preferences dialog). Settings' own sidebar is the
//! `nav_card`.
use iced::widget::{Space, button, column, container};
use iced::{Border, Center, Element, Fill, Padding};

use super::themes::c;
use super::tokens::{Role, radius, size};

/// One page in the list: its name and an optional badge ("Exp").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item<'a> {
    pub label: &'a str,
    pub badge: Option<&'a str>,
    /// A lucide glyph before the label: the item is then drawn as the Settings shell's (the
    /// active one a raised pill with a thin accent bar at its start edge), see `list`.
    pub icon: Option<char>,
}

/// A dialog's section list: `items` (with icons) one under the other, `active` the open one.
pub fn list<'a, Message: Clone + 'a>(
    items: &[Item<'a>],
    active: usize,
    on: impl Fn(usize) -> Message,
) -> Element<'a, Message> {
    column(
        (items.iter())
            .enumerate()
            .map(|(i, item)| nav_item(*item, i == active, on(i))),
    )
    .spacing(GAP)
    .into()
}

/// The accent bar's size and the gap after it and the icon.
const BAR_W: f32 = 3.0;
const BAR_H: f32 = 18.0;
const ICON_GAP: f32 = 10.0;

/// An item with an icon: bar (the active one's), icon, label; the active one raised.
fn shell_item<'a, Message: Clone + 'a>(
    glyph: char,
    item: Item<'a>,
    active: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let bar = container(Space::new())
        .width(BAR_W)
        .height(BAR_H)
        .style(move |_| container::Style {
            background: active.then(|| c().accent.into()),
            border: Border::default().rounded(BAR_W),
            ..container::Style::default()
        });
    let glyph = super::icon::glyph(glyph, size::ICON).color(match active {
        true => c().text,
        false => c().text_sub,
    });
    let name = match active {
        true => super::text::text(Role::NAV, item.label),
        false => super::text::sub(Role::NAV, item.label),
    };
    let line = super::dir::row([
        bar.into(),
        glyph.into(),
        name.into(),
        Space::new().width(Fill).into(),
    ])
    .spacing(ICON_GAP)
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
                        .rounded(radius::CONTROL)
                        .width(if active { 1.0 } else { 0.0 })
                        .color(c().elevated_border),
                    ..button::Style::default()
                }
            }),
        radius::CONTROL,
        Some(on_press),
    )
    .into()
}

/// Between two items (`space-y-0.5`).
const GAP: f32 = 2.0;

fn nav_item<'a, Message: Clone + 'a>(
    item: Item<'a>,
    active: bool,
    on_press: Message,
) -> Element<'a, Message> {
    shell_item(item.icon.unwrap_or(' '), item, active, on_press)
}
