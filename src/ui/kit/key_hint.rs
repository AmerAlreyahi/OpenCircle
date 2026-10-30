//! A hint pill with keys (Snip's "Click a window · Drag an area · [Enter]
//! Full screen · [Esc] Cancel"): groups of an optional keycap and a label,
//! a 3 px dot between them, on the widget surface, 36 px tall, round. `rtl`
//! runs the groups right to left (Arabic); keycaps stay Latin.
use iced::widget::{container, row};
use iced::{Center, Element};

use super::chip::dot;
use super::surface::{Material, surface};
use super::text::{self as kit_text, font};
use super::themes::c;
use super::tokens::{Role, radius};

/// The pill's height.
pub const HEIGHT: f32 = 36.0;

/// One group: its key (if any) and its label.
pub type Group<'a> = (Option<&'a str>, &'a str);

pub fn key_hint<'a, Message: 'a>(groups: &[Group<'a>], rtl: bool) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    let mut ordered: Vec<&Group<'a>> = groups.iter().collect();
    if rtl {
        ordered.reverse();
    }
    for (i, (key, label)) in ordered.into_iter().enumerate() {
        if i > 0 {
            items.push(container(dot(c().text_muted)).center_y(HEIGHT).into());
        }
        let label = kit_text::text(Role::CONTROL, *label).color(c().text_sub);
        let group: Element<'a, Message> = match key {
            Some(key) => {
                let cap = cap(key);
                match rtl {
                    true => row![label, cap],
                    false => row![cap, label],
                }
                .spacing(6)
                .align_y(Center)
                .into()
            }
            None => label.into(),
        };
        items.push(group);
    }
    surface(
        Material::Widget,
        radius::FULL,
        row(items).spacing(8).align_y(Center),
    )
    .padding([0, 12])
    .height(HEIGHT)
    .center_y(HEIGHT)
    .into()
}

/// A keycap as wide as its key (the kit keycap fills its cell).
fn cap<'a, Message: 'a>(key: &str) -> Element<'a, Message> {
    let w = kit_text::width(key, Role::KEYCAP.size, font(Role::KEYCAP)) + 12.0;
    container(super::keycap::keycap(key.to_string()))
        .width(w.max(22.0))
        .into()
}
