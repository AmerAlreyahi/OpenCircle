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
pub const PICKER_W: f32 = super::colour_picker::WIDTH;

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

/// The swatch's mark: the colour as a filled rounded square (a round dot read as a record
fn mark<'a, Message: 'a>(colour: Color) -> Element<'a, Message> {
    container(Space::new().width(DOT).height(DOT))
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border::default()
                .rounded(radius::MENU_ITEM)
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
    super::focus::press(
        button(container(mark(colour)).center(Fill))
            .width(size::ICON_BUTTON)
            .height(size::ICON_BUTTON)
            .padding(0)
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
            }),
        radius::FULL,
        on_press,
    )
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
        true => picker.width(PICKER_W).into(),
        false => picker.into(),
    }
}

/// One colour to pick: a 22 px dot, ringed in `text` when `chosen`.
fn chip<'a, Message: Clone + 'a>(colour: Color, chosen: bool, on: Message) -> Element<'a, Message> {
    super::focus::press(
        button(dot(colour, CHIP))
            .padding(2)
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered).then(|| c().hover.into()),
                border: Border::default()
                    .rounded(radius::FULL)
                    .width(if chosen { 2 } else { 0 })
                    .color(c().text),
                ..button::Style::default()
            }),
        radius::FULL,
        Some(on),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::shot;

    /// F15 (#220): the colour button is a filled rounded square (a round dot read as a record
    /// button): the colour fills the middle AND the corner region a round dot would leave bare.
    #[test]
    fn f15_the_swatch_is_a_filled_rounded_square() {
        let red = Color::from_rgb8(220, 30, 30);
        let side = size::ICON_BUTTON;
        let px = shot::render_with(
            swatch(red, false, Some(())),
            iced::Size::new(side, side),
            None,
            &[],
        );
        let at = |x: f32, y: f32| {
            let i = ((y as usize) * (side as usize) + x as usize) * 4;
            [px[i], px[i + 1], px[i + 2]]
        };
        let mid = side / 2.0;
        assert_eq!(at(mid, mid), [220, 30, 30], "the colour in the middle");
        // 6 px right and down: inside a square of 18, outside a circle of 18
        // near the corner, where a round dot leaves the button bare, the square is still colour
        let near = at(mid + 6.0, mid + 5.0);
        assert!(
            near[0] > 190 && near[1] < 70,
            "a square, not a dot: {near:?}"
        );
        // and the corner itself is rounded off
        assert_ne!(at(mid + 8.5, mid + 8.5), [220, 30, 30]);
    }
}
