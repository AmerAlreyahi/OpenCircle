//! `surface(Material, radius, content)`: what every panel and window body sits
//! on, solid (glass joins with the GPU renderer, 0.1.3).
use iced::widget::{Container, container};
use iced::{Border, Element};

use super::themes::c;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// A window's body (`--app-bg-solid`, rim edge).
    Window,
    /// A recessed panel laid over a window (`--app-track-glass`, divider edge).
    Recessed,
    /// A floating widget or card (`.oc-widget`: `--app-widget-solid`, rim edge).
    Widget,
    /// A floating widget while it is held and dragged (`.oc-dragging`): the
    /// widget with an `accent` edge.
    Held,
}

/// `content` on `material` with corners of `radius` and a 1 px edge. The
/// caller sizes and pads it.
pub fn surface<'a, Message: 'a>(
    material: Material,
    radius: f32,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content).style(move |_| {
        let (fill, edge) = match material {
            Material::Window => (c().bg, c().rim),
            Material::Recessed => (c().track_glass, c().divider),
            Material::Widget => (c().widget, c().rim),
            Material::Held => (c().widget, c().accent),
        };
        container::Style {
            background: Some(fill.into()),
            border: Border::default().rounded(radius).width(1).color(edge),
            ..container::Style::default()
        }
    })
}
