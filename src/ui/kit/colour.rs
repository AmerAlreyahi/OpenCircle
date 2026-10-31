//! The one colour control (owner rule: a colour button opening a picker, not
//! a row of swatches; the design has none): `swatch`, a 34x34 `.oc-btn` with
//! the colour as an 18 px dot (`accent` edge while its picker is open), and
//! `picker`, the colours in rows of 6 and a hex field, on the `dropdown`
//! material with a `rim` edge, radius 14. `recent`: the screen colour
//! picker's last colours, the picker's dots in one row.
use iced::widget::{Space, button, column, container, row};
use iced::{Border, Color, Element, Fill};

use super::field::input;
use super::themes::c;
use super::tokens::{radius, size, space};

const DOT: f32 = 18.0;
const CHIP: f32 = 22.0;
const PER_ROW: usize = 6;
const PAD: f32 = 10.0;
/// `picker_with`'s width: its rows of controls, and the padding.
const WIDE: f32 = 256.0;

/// A round dot of `colour` with a `rim` edge (white reads on a light page).
fn dot<'a, Message: 'a>(colour: Color, d: f32) -> Element<'a, Message> {
    container(Space::new().width(d).height(d))
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}

pub fn swatch<'a, Message: Clone + 'a>(
    colour: Color,
    open: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    // dimmed (no press) as the icon buttons are
    let colour = match on_press {
        Some(_) => colour,
        None => colour.scale_alpha(0.4),
    };
    button(container(dot(colour, DOT)).center(Fill))
        .width(size::ICON_BUTTON)
        .height(size::ICON_BUTTON)
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let fill = if hot {
                c().elevated_hover
            } else {
                c().elevated
            };
            let edge = if open {
                c().accent
            } else {
                c().elevated_border
            };
            button::Style {
                background: Some(fill.into()),
                border: Border::default()
                    .rounded(radius::CONTROL)
                    .width(1)
                    .color(edge),
                ..button::Style::default()
            }
        })
        .into()
}

/// `colours` to pick from (the chosen one ringed in `text`), and `hex` as typed.
pub fn picker<'a, Message: Clone + 'a>(
    colours: &[Color],
    current: Color,
    hex: &str,
    on_colour: impl Fn(Color) -> Message,
    on_hex: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    picker_with(colours, current, hex, on_colour, on_hex, None)
}

/// `picker` with more controls under the hex field (the editor's stroke
/// weight, eyedropper and presets).
pub fn picker_with<'a, Message: Clone + 'a>(
    colours: &[Color],
    current: Color,
    hex: &str,
    on_colour: impl Fn(Color) -> Message,
    on_hex: impl Fn(String) -> Message + 'a,
    more: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let chip = |&colour: &Color| chip(colour, colour == current, on_colour(colour));
    let rows =
        (colours.chunks(PER_ROW)).map(|r| row(r.iter().map(chip)).spacing(space::GAP).into());
    let hex = input("#RRGGBB", hex, true, false).on_input(on_hex);
    let mut body = column(rows).push(hex).spacing(space::GAP);
    // The hex field fills its row: with more controls the picker is as wide
    // as they need (a window floating it would otherwise stretch it).
    let wide = more.is_some();
    if let Some(more) = more {
        body = body.push(more);
    }
    let picker = container(body).padding(PAD).style(|_| container::Style {
        background: Some(c().dropdown.into()),
        border: Border::default()
            .rounded(radius::DROPDOWN)
            .width(1)
            .color(c().rim),
        ..container::Style::default()
    });
    match wide {
        true => picker.width(WIDE).into(),
        false => picker.into(),
    }
}

/// One colour to pick: a 22 px dot, ringed in `text` when `chosen`.
fn chip<'a, Message: Clone + 'a>(colour: Color, chosen: bool, on: Message) -> Element<'a, Message> {
    button(dot(colour, CHIP))
        .padding(2)
        .on_press(on)
        .style(move |_, status| button::Style {
            background: matches!(status, button::Status::Hovered).then(|| c().hover.into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(if chosen { 2 } else { 0 })
                .color(c().text),
            ..button::Style::default()
        })
        .into()
}

/// The colour picker's last colours, newest first, in one row: a click
/// sends `on` with its place in `colours`.
pub fn recent<'a, Message: Clone + 'a>(
    colours: &[[u8; 3]],
    on: impl Fn(usize) -> Message,
) -> Element<'a, Message> {
    let chip = |(i, &[r, g, b]): (usize, &[u8; 3])| chip(Color::from_rgb8(r, g, b), false, on(i));
    row(colours.iter().enumerate().map(chip))
        .spacing(space::TRACK_GAP)
        .into()
}
