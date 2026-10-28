//! A 1 px `--app-divider` hairline across.
use iced::widget::{Space, container};
use iced::{Element, Fill};

use super::themes::c;

pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .style(|_| container::Style::default().background(c().divider))
        .into()
}

/// A 1 px `--app-divider` line `height` tall (between a toolbar's groups).
pub fn vertical<'a, Message: 'a>(height: f32) -> Element<'a, Message> {
    container(Space::new().width(1).height(height))
        .style(|_| container::Style::default().background(c().divider))
        .into()
}
