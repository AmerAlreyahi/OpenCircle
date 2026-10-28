//! `.oc-keycap`: one key of a shortcut, 24 px tall, radius 6, `keycap` with a
//! 1 px `keycap_border` edge and a 2 px one under it, mono 11 / 500. `empty`
//! is the design's dimmed "—" cap (opacity .25) that pads a shortcut box.
use iced::widget::{container, text};
use iced::{Border, Color, Element, Fill, Padding};

use super::button::faded;
use super::text::font;
use super::themes::c;
use super::tokens::{Role, radius, size};

/// The key's label, filling its cell (`width: 100%`).
pub fn keycap<'a, Message: 'a>(label: impl Into<String>) -> Element<'a, Message> {
    cap(label.into(), 1.0)
}

/// An unused cell: "—" on a cap at 25 %.
pub fn empty<'a, Message: 'a>() -> Element<'a, Message> {
    cap("—".into(), 0.25)
}

fn cap<'a, Message: 'a>(label: String, alpha: f32) -> Element<'a, Message> {
    let fade = move |colour: Color| faded(colour, alpha);
    // the edge is the outer box showing round the face: 1 px, 2 px below
    let face = container(
        text(label)
            .size(Role::KEYCAP.size)
            .font(font(Role::KEYCAP))
            .color(fade(c().text)),
    )
    .center(Fill)
    .style(move |_| container::Style {
        background: Some(fade(c().keycap).into()),
        border: Border::default().rounded(radius::KEYCAP - 1.0),
        ..container::Style::default()
    });
    container(face)
        .padding(Padding::new(1.0).bottom(2.0))
        .width(Fill)
        .height(size::KEYCAP_H)
        .style(move |_| container::Style {
            background: Some(fade(c().keycap_border).into()),
            border: Border::default().rounded(radius::KEYCAP),
            ..container::Style::default()
        })
        .into()
}
