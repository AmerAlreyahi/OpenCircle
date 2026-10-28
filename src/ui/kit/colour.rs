//! The one colour control (owner rule: a colour button opening a picker, not
//! a row of swatches; the design has none): `swatch`, a 34x34 `.oc-btn` with
//! the colour as an 18 px dot (`accent` edge while its picker is open), and
//! `picker`, the colours in rows of 6 and a hex field, on the `dropdown`
//! material with a `rim` edge, radius 14.
use iced::widget::{Space, button, column, container, row};
use iced::{Border, Color, Element, Fill};

use super::field::input;
use super::themes::c;
use super::tokens::{radius, size, space};

const DOT: f32 = 18.0;
const CHIP: f32 = 22.0;
const PER_ROW: usize = 6;
const PAD: f32 = 10.0;

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
    on_press: Message,
) -> Element<'a, Message> {
    button(container(dot(colour, DOT)).center(Fill))
        .width(size::ICON_BUTTON)
        .height(size::ICON_BUTTON)
        .padding(0)
        .on_press(on_press)
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
    let chip = |&colour: &Color| -> Element<'a, Message> {
        let chosen = colour == current;
        button(dot(colour, CHIP))
            .padding(2)
            .on_press(on_colour(colour))
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered).then(|| c().hover.into()),
                border: Border::default()
                    .rounded(radius::FULL)
                    .width(if chosen { 2 } else { 0 })
                    .color(c().text),
                ..button::Style::default()
            })
            .into()
    };
    let rows =
        (colours.chunks(PER_ROW)).map(|r| row(r.iter().map(chip)).spacing(space::GAP).into());
    let hex = input("#RRGGBB", hex, true, false).on_input(on_hex);
    let mut body = column(rows).push(hex).spacing(space::GAP);
    if let Some(more) = more {
        body = body.push(more);
    }
    container(body)
        .padding(PAD)
        .style(|_| container::Style {
            background: Some(c().dropdown.into()),
            border: Border::default()
                .rounded(radius::DROPDOWN)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}
