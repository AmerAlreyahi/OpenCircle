//! A chip: a short label on a round tint (the kinds join with their users).
use iced::font::Weight;
use iced::widget::{container, text};
use iced::{Border, Element};

use super::text::mono;
use super::themes::c;
use super::tokens::radius;

/// Mono on `accent_soft` (the setup's version): 10.5 px / 500, 2x8 px, 20 px tall.
pub fn chip<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(
        text(label)
            .size(10.5)
            .font(mono(Weight::Medium))
            .color(c().accent),
    )
    .padding([2, 8])
    .height(20)
    .center_y(20)
    .style(|_| container::Style {
        background: Some(c().accent_soft.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    })
    .into()
}
