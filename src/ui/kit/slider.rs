//! A whole number dragged along a rail (the custom look's glow and dim; the
//! design has no slider): a 4 px rail, `accent` up to the value and `track`
//! after it, a 14 px round `text` handle (`accent` while held or under the
//! cursor), the value in mono `text_sub` at the right. As wide as a settings row's control column.
use std::ops::RangeInclusive;

use iced::widget::slider::{self as rail, Handle, HandleShape, Rail};
use iced::widget::{container, text};
use iced::{Border, Center, Color, Element};

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
    let role = Role::KEYCAP;
    crate::dir_row![
        line(range, value, false, on_change),
        text(format!("{value}{unit}"))
            .size(super::text::size(role))
            .font(font(role))
            .color(c().text_sub)
            .width(VALUE)
            .align_x(super::dir::end()),
    ]
    .spacing(space::GAP)
    .width(size::CONTROL_COLUMN)
    .align_y(Center)
    .into()
}

/// The rail alone, `width` across (Snip's toolbar panels: the value shows
/// in the panel's row); `dim`: faded (Auto decides; dragging it still works).
pub fn bare<'a, Message: Clone + 'a>(
    range: RangeInclusive<u8>,
    value: u8,
    width: f32,
    dim: bool,
    on_change: impl Fn(u8) -> Message + 'a,
) -> Element<'a, Message> {
    container(line(range, value, dim, on_change))
        .width(width)
        .into()
}

fn line<'a, Message: Clone + 'a>(
    range: RangeInclusive<u8>,
    value: u8,
    dim: bool,
    on_change: impl Fn(u8) -> Message + 'a,
) -> Element<'a, Message> {
    let alpha = if dim { 0.45 } else { 1.0 };
    let fade = move |colour: Color| super::button::faded(colour, alpha);
    // right to left: the least at the right, filled from the right (the
    // rail runs mirrored: its value is the distance from the other end)
    let rtl = super::dir::rtl();
    let (lo, hi) = (*range.start(), *range.end());
    let flip = move |v: u8| match rtl {
        true => hi - (v - lo),
        false => v,
    };
    let filled = (fade(c().accent).into(), c().track.into());
    let backgrounds = match rtl {
        true => (filled.1, filled.0),
        false => filled,
    };
    let on_change = std::rc::Rc::new(on_change);
    let step = on_change.clone();
    let rail = rail::Slider::new(range, flip(value), move |v| on_change(flip(v))).style(
        move |_, status| rail::Style {
            rail: Rail {
                backgrounds,

                width: RAIL,
                border: Border::default().rounded(radius::FULL),
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: HANDLE },
                background: fade(match status {
                    rail::Status::Active => c().text,
                    _ => c().accent,
                })
                .into(),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        },
    );
    // the arrows: one more / less (left is more right to left, as the rail runs)
    super::focus::stop(rail, radius::FULL, None)
        .up_is_on()
        .steps(move |s| {
            let to = match s {
                super::focus::Step::On => value.checked_add(1).filter(|v| *v <= hi)?,
                super::focus::Step::Back => value.checked_sub(1).filter(|v| *v >= lo)?,
            };
            Some(step(to))
        })
        .into()
}
