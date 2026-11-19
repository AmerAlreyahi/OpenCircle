//! An options bar's controls (Studio v3, board 10): ONE control height, 32 = a
//! group container (`track` fill, radius 9, 2 px padding) holding 28 px cells
//! (the pills' radius, 2 px apart); groups are split by a 1 x 20 hairline.
//! Colours, size dots, toggles, values and buttons are all cells; only the
//! Done group is filled with the accent.
use iced::widget::{Space, button, container, text};
use iced::{Border, Center, Color, Element, Fill, Length};

use super::dir;
use super::themes::{c, on};
use super::tokens::{Role, radius};

/// The control height (a group) and its cells' (the group less its padding).
pub const HEIGHT: f32 = 32.0;
pub const CELL: f32 = 28.0;
const PAD: f32 = 2.0;
const GAP: f32 = 2.0;
/// The gap between groups.
pub const GROUP_GAP: f32 = 8.0;
const HAIR_H: f32 = 20.0;
/// A swatch dot, and the size dots' diameters (small, medium, large).
const SWATCH: f32 = 16.0;
pub const DOTS: [f32; 3] = [4.0, 8.0, 12.0];
/// A cell's padding when it is as wide as its words.
const WORDS_PAD: f32 = 10.0;

/// `cells` in a group: 32 tall, whatever they hold. `accent`: the Done group.
pub fn group<'a, Message: 'a>(
    cells: Vec<Element<'a, Message>>,
    accent: bool,
) -> Element<'a, Message> {
    container(dir::row(cells).spacing(GAP).align_y(Center))
        .padding(PAD)
        .height(HEIGHT)
        .center_y(HEIGHT)
        .style(move |_| container::Style {
            background: Some(if accent { c().accent } else { c().track }.into()),
            border: Border::default().rounded(radius::CONTROL),
            ..container::Style::default()
        })
        .into()
}

/// The hairline between two groups.
pub fn hairline<'a, Message: 'a>() -> Element<'a, Message> {
    container(Space::new().width(1).height(HAIR_H))
        .style(|_| container::Style {
            background: Some(c().divider.into()),
            ..container::Style::default()
        })
        .into()
}

/// A 28 px cell around `content`, raised while `on`, shaded under the pointer;
/// no `press` = dimmed to 38 %. `width` None = as wide as its content.
fn cell<'a, Message: Clone + 'a>(
    content: Element<'a, Message>,
    width: Option<f32>,
    on_: bool,
    in_accent: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    let live = press.is_some();
    let pad = if width.is_some() { 0.0 } else { WORDS_PAD };
    let body = container(content).center_y(Fill).padding([0.0, pad]);
    let b = button(body)
        .height(CELL)
        .width(width.map_or(Length::Shrink, Length::Fixed))
        .padding(0)
        .style(move |_, status| {
            let hot = live && matches!(status, button::Status::Hovered | button::Status::Pressed);
            let (fill, edge) = match () {
                _ if on_ => (Some(c().elevated), c().elevated_border),
                _ if hot && !in_accent => (Some(c().hover), Color::TRANSPARENT),
                _ => (None, Color::TRANSPARENT),
            };
            let ink = match () {
                _ if in_accent => on(c().accent),
                _ if on_ || hot => c().text,
                _ => c().text_sub,
            };
            let ink = if live {
                ink
            } else {
                Color {
                    a: ink.a * 0.38,
                    ..ink
                }
            };
            button::Style {
                background: fill.map(Into::into),
                text_color: ink,
                border: Border::default().rounded(radius::PILL).width(1).color(edge),
                ..button::Style::default()
            }
        });
    super::focus::press(b, radius::PILL, press).into()
}

fn label<'a, Message: 'a>(words: &'a str, strong: bool) -> Element<'a, Message> {
    let role = if strong { Role::CTA } else { Role::CONTROL };
    text(words)
        .size(12)
        .font(super::text::font(role))
        .wrapping(text::Wrapping::None)
        .into()
}

/// A text cell (Outline, Fill, Bold ...): `on` raises it.
pub fn text_cell<'a, Message: Clone + 'a>(
    words: &'a str,
    on: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    cell(label(words, false), None, on, false, press)
}

/// A square cell around a glyph of the icon font.
pub fn glyph_cell<'a, Message: Clone + 'a>(
    glyph: char,
    on: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    let g = text(glyph).font(super::icon::FONT).size(14);
    cell(
        container(g).center_x(CELL).into(),
        Some(CELL),
        on,
        false,
        press,
    )
}

/// The Done group's cell: words on the accent fill.
pub fn accent_cell<'a, Message: Clone + 'a>(
    words: &'a str,
    press: Option<Message>,
) -> Element<'a, Message> {
    cell(label(words, true), None, false, true, press)
}

/// A colour: a 16 px dot, ringed and raised while picked.
pub fn swatch<'a, Message: Clone + 'a>(
    colour: Color,
    picked: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    let dot =
        container(Space::new().width(SWATCH).height(SWATCH)).style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(if picked { 2.0 } else { 1.5 })
                .color(if picked {
                    c().text
                } else {
                    Color {
                        a: 0.4,
                        ..c().text_sub
                    }
                }),
            ..container::Style::default()
        });
    cell(
        container(dot).center_x(CELL).into(),
        Some(CELL),
        picked,
        false,
        press,
    )
}

/// A size dot (`DOTS[i]` px across), raised while picked.
pub fn dot<'a, Message: Clone + 'a>(
    i: usize,
    picked: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    let d = DOTS[i.min(2)];
    let dot = container(Space::new().width(d).height(d)).style(move |_| container::Style {
        background: Some(if picked { c().text } else { c().text_sub }.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    });
    cell(
        container(dot).center_x(CELL).into(),
        Some(CELL),
        picked,
        false,
        press,
    )
}

/// A value read out (mono 11.5): not pressable.
pub fn value<'a, Message: 'a>(words: String) -> Element<'a, Message> {
    let t = text(words)
        .size(11.5)
        .font(super::text::mono(iced::font::Weight::Medium))
        .color(c().text);
    container(t).center_y(CELL).padding([0.0, 8.0]).into()
}

/// A name inside a group ("Size"): muted, not pressable.
pub fn caption<'a, Message: 'a>(words: &'a str) -> Element<'a, Message> {
    let t = text(words)
        .size(12)
        .font(super::text::font(Role::CONTROL))
        .color(c().text_sub);
    container(t).center_y(CELL).padding([0.0, 6.0]).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tall(e: &Element<'_, ()>) -> Length {
        iced::advanced::Widget::size(e.as_widget()).height
    }

    /// STUDIO_V3 S4 (board 10): every control of the bar is 32 high; the cells
    /// are 28 and, with the group's padding, make 32.
    #[test]
    fn every_control_in_an_options_bar_is_32_high() {
        assert_eq!(CELL + 2.0 * PAD, HEIGHT);
        let cells = vec![
            swatch(Color::WHITE, true, Some(())),
            dot(1, false, Some(())),
            text_cell("Outline", false, Some(())),
            glyph_cell('\u{e140}', false, None),
            value("5 px".into()),
            caption("Size"),
        ];
        assert_eq!(tall(&group(cells, false)), Length::Fixed(HEIGHT));
        let done = group(vec![accent_cell("Done", Some(()))], true);
        assert_eq!(tall(&done), Length::Fixed(HEIGHT));
    }
}
