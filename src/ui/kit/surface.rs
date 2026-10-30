//! `surface(Material, radius, content)`: what every panel and window body sits
//! on, solid (glass joins with the GPU renderer, 0.1.4).
use iced::widget::{Container, container, text::IntoFragment};
use iced::{Border, Element};

use super::themes::c;
use super::tokens::{Role, radius};

/// A hint's padding (the toast pill's, design 11.2).
const HINT_PAD: [f32; 2] = [8.0, 14.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// A window's body (`--app-bg-solid`, rim edge).
    Window,
    /// A recessed panel laid over a window (`--app-track-glass`, divider edge).
    Recessed,
    /// A floating widget or card (`.oc-widget`: `--app-widget-solid`, rim edge).
    Widget,
    /// A tile on a window (the setup's logo emblem): `hover`, rim edge.
    Tile,
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
            Material::Tile => (c().hover, c().rim),
            Material::Held => (c().widget, c().accent),
        };
        container::Style {
            background: Some(fill.into()),
            border: Border::default().rounded(radius).width(1).color(edge),
            ..container::Style::default()
        }
    })
}

/// A one-line hint over the screen ("Circle, drag or tap anything", the
/// editor's area picker, a protected capture): a solid pill on the bar's
/// surface, in its text, in every theme.
pub fn hint<'a, Message: 'a>(label: impl IntoFragment<'a>) -> Container<'a, Message> {
    let line = super::text::text(Role::NOTE, label);
    surface(Material::Widget, radius::FULL, line).padding(HINT_PAD)
}
