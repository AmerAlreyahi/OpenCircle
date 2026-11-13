//! The one hairline: 1 px of `--app-divider`, across (`divider`) or down
//! (`vertical`): both are `line`, so every separator looks the same.
use iced::widget::{Space, container};
use iced::{Element, Fill, Length};

use super::themes::c;

/// A hairline's thickness.
pub const WIDTH: f32 = 1.0;

fn line<'a, Message: 'a>(width: Length, height: Length) -> Element<'a, Message> {
    container(Space::new().width(width).height(height))
        .style(|_| container::Style::default().background(c().divider))
        .into()
}

pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    line(Fill, WIDTH.into())
}

/// A hairline down, `height` tall (the sidebar's edge, a toolbar's groups).
pub fn vertical<'a, Message: 'a>(height: impl Into<Length>) -> Element<'a, Message> {
    line(WIDTH.into(), height.into())
}
