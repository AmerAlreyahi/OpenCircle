//! `.oc-keycap`: one key of a shortcut, 24 px tall, radius 6, `keycap` with a
//! 1 px `keycap_border` edge and a 2 px one under it, mono 11 / 500.
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

/// A key of a row that is off: the cap at 38 %.
pub fn dimmed<'a, Message: 'a>(label: impl Into<String>) -> Element<'a, Message> {
    cap(label.into(), 0.38)
}

fn cap<'a, Message: 'a>(label: String, alpha: f32) -> Element<'a, Message> {
    let fade = move |colour: Color| faded(colour, alpha);
    // the edge is the outer box showing round the face: 1 px, 2 px below
    let face = container(
        text(label)
            .size(super::text::size(Role::KEYCAP))
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

/// `field` with `keys` drawn at its end while `shown` (a search field's "Ctrl F"): one small cap per
/// key, over the field, never taking a press from it.
pub fn at_end<'a, Message: 'a>(
    field: impl Into<Element<'a, Message>>,
    keys: &[&str],
    shown: bool,
) -> Element<'a, Message> {
    let field = field.into();
    if !shown {
        return field;
    }
    // (a plain row: keys read in their own order, "Ctrl F", in every language)
    let caps = iced::widget::Row::with_children(
        keys.iter()
            .map(|k| {
                let width = 12.0 + 7.0 * k.chars().count() as f32;
                container(keycap(*k)).width(width).into()
            })
            .collect::<Vec<Element<'a, Message>>>(),
    )
    .spacing(2.0)
    .align_y(iced::Center);
    let over = container(caps)
        .padding([0.0, 6.0])
        .center_y(Fill)
        .align_x(super::dir::end())
        .width(Fill);
    iced::widget::stack![field, over].into()
}
