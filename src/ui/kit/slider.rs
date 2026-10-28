//! A whole number dragged along a rail (the custom look's glow and dim; the
//! design has no slider): a 4 px rail, `accent` up to the value and `track`
//! after it, a 14 px round `text` handle (`accent` while held or under the
//! cursor), the value in mono `text_sub` at the right. As wide as a settings row's control column.
use std::ops::RangeInclusive;

use iced::widget::slider::{self as rail, Handle, HandleShape, Rail};
use iced::widget::{row, text};
use iced::{Border, Center, Color, Element, alignment};

use super::text::font;
use super::themes::c;
use super::tokens::{Role, radius, size, space};

const RAIL: f32 = 4.0;
const HANDLE: f32 = 7.0;
/// The value's column ("100 %").
const VALUE: f32 = 44.0;

pub fn slider<'a, Message: Clone + 'a>(
    range: RangeInclusive<u8>,
    value: u8,
    unit: &'a str,
    on_change: impl Fn(u8) -> Message + 'a,
) -> Element<'a, Message> {
    let bar = rail::Slider::new(range, value, on_change).style(|_, status| rail::Style {
        rail: Rail {
            backgrounds: (c().accent.into(), c().track.into()),
            width: RAIL,
            border: Border::default().rounded(radius::FULL),
        },
        handle: Handle {
            shape: HandleShape::Circle { radius: HANDLE },
            background: match status {
                rail::Status::Active => c().text,
                _ => c().accent,
            }
            .into(),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    });
    let role = Role::KEYCAP;
    row![
        bar,
        text(format!("{value}{unit}"))
            .size(role.size)
            .font(font(role))
            .color(c().text_sub)
            .width(VALUE)
            .align_x(alignment::Horizontal::Right),
    ]
    .spacing(space::GAP)
    .width(size::CONTROL_COLUMN)
    .align_y(Center)
    .into()
}
