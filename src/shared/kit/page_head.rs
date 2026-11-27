//! Settings round 2: a page's header, a small accent tile with the page's icon, the title and ONE line
//! under it (no breadcrumb), with an optional part after the line ("See how it works").
use iced::widget::{Space, column, container};
use iced::{Border, Center, Element};

use super::icon::glyph as icon_glyph;
use super::text::{sub, text};
use super::themes::c;
use super::tokens::settings::TILE;
use super::tokens::{Role, radius};

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
