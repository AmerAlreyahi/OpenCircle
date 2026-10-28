//! `.oc-card` as an info card (`!cursor-default !p-4`): 16 px in, radius 12,
//! `card_idle` with a divider edge. (The picking cards, theme and look, join
//! with Appearance.)
use iced::widget::container;
use iced::{Border, Element, Fill};

use super::themes::c;
use super::tokens::radius;

const PAD: f32 = 16.0;

pub fn card<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().card_idle.into()),
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}
