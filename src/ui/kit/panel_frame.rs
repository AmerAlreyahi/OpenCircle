//! A floating card (the design's `.as-card` on `.oc-widget`: the update card,
//! later the model download and error cards): a heading (an accent icon and
//! a 16 px title; pressing it drags the window), a body that scrolls when it
//! is long, and an actions row. Fills its window, padding 22.
use iced::widget::{column, container, mouse_area, row, scrollable};
use iced::{Border, Center, Element, Fill};

use super::icon::{Icon, icon};
use super::surface::{Material, surface};
use super::text::text;
use super::themes::c;
use super::tokens::Role;

const PAD: f32 = 22.0;
/// Under the heading, above the actions (`.as-heading` 14, `.as-actions` 18).
const UNDER_HEADING: f32 = 14.0;
const ABOVE_ACTIONS: f32 = 18.0;
const ICON: f32 = 20.0;
const ICON_GAP: f32 = 10.0;
/// The scroller: 4 px, no rail.
const SCROLLER: f32 = 4.0;

/// The card with corners of `radius` (the window's cut).
pub fn frame<'a, Message: Clone + 'a>(
    glyph: Icon,
    title: &'a str,
    body: impl Into<Element<'a, Message>>,
    actions: impl Into<Element<'a, Message>>,
    radius: f32,
    on_drag: Message,
) -> Element<'a, Message> {
    let heading = row![
        icon(glyph, ICON).color(c().accent),
        text(Role::PAGE_TITLE, title)
    ]
    .spacing(ICON_GAP)
    .align_y(Center);
    let heading = mouse_area(container(heading).width(Fill).padding(iced::Padding {
        bottom: UNDER_HEADING,
        ..iced::Padding::ZERO
    }))
    .on_press(on_drag);
    let bar = scrollable::Scrollbar::new()
        .width(SCROLLER)
        .scroller_width(SCROLLER);
    let body = scrollable(body)
        .direction(scrollable::Direction::Vertical(bar))
        .height(Fill)
        .style(|theme, status| {
            let mut style = scrollable::default(theme, status);
            style.vertical_rail = scrollable::Rail {
                background: None,
                border: Border::default(),
                scroller: scrollable::Scroller {
                    background: c().rim.into(),
                    border: Border::default().rounded(SCROLLER / 2.0),
                },
            };
            style
        });
    let actions = container(actions).padding(iced::Padding {
        top: ABOVE_ACTIONS,
        ..iced::Padding::ZERO
    });
    surface(Material::Widget, radius, column![heading, body, actions])
        .padding(PAD)
        .width(Fill)
        .height(Fill)
        .into()
}
