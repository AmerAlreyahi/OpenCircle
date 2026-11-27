//! Settings round 2: a page's header, a small accent tile with the page's icon, the title and ONE line
//! under it (no breadcrumb), with an optional part after the line ("See how it works").
use iced::widget::{Space, column, container};
use iced::{Border, Center, Element};

use super::button::{Kind, button};
use super::icon::{Icon, glyph as icon_glyph};
use super::text::{accent, sub, text};
use super::themes::c;
use super::tokens::settings::TILE;
use super::tokens::{Role, radius, size, space};

/// The tile's icon, and the gaps (tile to text, title to line).
const ICON: f32 = 20.0;
const GAP: f32 = 14.0;

pub fn page_head<'a, Message: 'a>(
    glyph: char,
    title: &'a str,
    line: &'a str,
    after: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let tile = container(icon_glyph(glyph, ICON).color(c().accent))
        .center(TILE)
        .style(|_| container::Style {
            background: Some(c().accent_soft.into()),
            border: Border::default().rounded(radius::DRAWER - 6.0),
            ..container::Style::default()
        });
    let mut second = super::dir::row([sub(Role::PAGE_SUB, line).into()])
        .spacing(10)
        .align_y(Center);
    if let Some(after) = after {
        second = second.push(after);
    }
    let body = column![text(Role::PAGE_HEAD, title), Space::new().height(2), second]
        .align_x(super::dir::start());
    super::dir::row([tile.into(), body.into()])
        .spacing(GAP)
        .into()
}

/// Accent-coloured link with a small chevron beside it (`"See how it works"` / `"Hide the demo"`).
pub fn link_chevron<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        super::dir::row([
            accent(Role::CONTROL, label).into(),
            icon_glyph(Icon::ChevronDown.glyph(), size::ICON_SMALL)
                .color(c().accent)
                .into(),
        ])
        .spacing(space::TRACK_GAP)
        .align_y(Center),
        Kind::Quiet,
        Some(on_press),
    )
}
