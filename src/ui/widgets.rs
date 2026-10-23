//! Shared floating components. Every floating widget is built here, styled only
//! from `theme.rs`; screens never style a container themselves.
//!
//! Glass later: blur the frozen frame once per overlay and paint the blurred crop
//! behind each surface (no nested/live blur).
use iced::widget::{Container, container, text};
use iced::{Border, Shadow, Vector};

use super::theme;

/// Fully rounded glass pill with one line of text (e.g. the overlay hint).
pub fn pill<'a, Message: 'a>(label: &'a str) -> Container<'a, Message> {
    let palette = theme::current();
    container(text(label).size(14).color(palette.text))
        .padding([8, 18])
        .style(|_| glass(999.0))
}

/// The one glass style: panel colour, 1 px border, soft shadow.
// ponytail: only the pill uses it so far; add `surface()` (20 px radius) with the
// first panel, and one shared button helper with the first button.
fn glass(radius: f32) -> container::Style {
    let palette = theme::current();
    container::Style {
        background: Some(palette.panel.into()),
        border: Border {
            color: palette.border,
            width: 1.0,
            radius: radius.into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}
