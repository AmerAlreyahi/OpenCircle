//! The one colour control (owner rule: a colour button opening a picker, not
//! a row of swatches; the design has none): `swatch`, a 34x34 `.oc-btn` with
//! the colour as an 18 px dot (`accent` edge while its picker is open), and
//! the picker itself is `colour_picker`. `recent`: the screen colour
//! picker's last colours, the picker's dots in one row.
use iced::widget::{Space, button, container, row};
use iced::{Border, Color, Element, Fill};

use super::themes::c;
use super::tokens::{radius, size, space};

const DOT: f32 = 18.0;
const DOT_E: f32 = 20.0;
const CHIP: f32 = 22.0;
/// The colour picker's width (`colour_picker::WIDTH`): the popovers that hold it are this wide.
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
fn mark<'a, Message: 'a>(colour: Color, side: f32) -> Element<'a, Message> {
    container(Space::new().width(side).height(side))
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
    swatch_face(false, colour, open, on_press)
}

/// Style E toolbar colour button (`.ib.cl2` + `.sw2`): the swatch with a 20 px
/// mark, 10 px corners, no fill at rest and a `rim` edge (`accent` when open).
pub fn swatch_e<'a, Message: Clone + 'a>(
    colour: Color,
    open: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    swatch_face(true, colour, open, on_press)
}

/// The swatch button; `floating` = style E (Snip's toolbar), else Settings' and the Studio's.
fn swatch_face<'a, Message: Clone + 'a>(
    floating: bool,
    colour: Color,
    open: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    // dimmed (no press) as the icon buttons are
    let colour = match on_press {
        Some(_) => colour,
        None => colour.scale_alpha(0.4),
    };
    let corner = if floating {
        radius::TRACK
    } else {
        radius::CONTROL
    };
    super::focus::press(
        button(container(mark(colour, if floating { DOT_E } else { DOT })).center(Fill))
            .width(size::ICON_BUTTON)
            .height(size::ICON_BUTTON)
            .padding(0)
            .style(move |_, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let (fill, rest, edge) = if floating {
                    (c().hover, None, c().rim)
                } else {
                    (c().elevated_hover, Some(c().elevated), c().elevated_border)
                };
                let edge = if open { c().accent } else { edge };
                button::Style {
                    background: if hot { Some(fill) } else { rest }.map(Into::into),
                    border: Border::default().rounded(corner).width(1).color(edge),
                    ..button::Style::default()
                }
            }),
        if floating {
            radius::TRACK
        } else {
            radius::FULL
        },
        on_press,
    )
    .into()
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
