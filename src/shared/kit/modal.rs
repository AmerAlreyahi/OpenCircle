//! A modal dialog, the familiar way: a solid card (never see-through) centred over the
//! window, which is dimmed behind it; an X at the card's top end; a press outside the
//! card closes it, and so does Esc (the host sends the same `close`). One behaviour for
//! every dialog and sheet of a window (the Studio's question, shortcuts, restore sheet,
//! preferences ...); small glance-only things are a `popover`, big temporary tasks a
//! `slide_over`.
use iced::widget::{column, container, mouse_area, opaque, stack};
use iced::{Center, Element, Fill, Length};

use super::icon::Icon;
use super::icon_button::{self, State};
use super::surface::{Material, surface};
use super::text::{sub, text};
use super::tokens::{Role, fixed, radius, space};

/// The dialog's top line (it acts as a title bar): `title` (and a grey one-line `subtitle`) and the X.
pub fn header<'a, Message: Clone + 'a>(
    title: &'a str,
    subtitle: Option<&'a str>,
    close: Message,
) -> Element<'a, Message> {
    let words = match subtitle {
        Some(sub_line) => column![
            text(Role::CARD_HEADING, title),
            sub(Role::ROW_DESC, sub_line)
        ]
        .into(),
        None => Element::from(text(Role::CARD_HEADING, title)),
    };
    let x = icon_button::square(Icon::Close.glyph(), State::default(), Some(close));
    // (never mirrored, like a title bar: the title at the left, the x at the right, in every language)
    iced::widget::row![container(words).width(Fill), x]
        .spacing(space::GAP)
        .align_y(Center)
        .into()
}

/// `base` with `content` as a solid card, `width` wide (at most the window's), centred
/// over a dimmed window; a press on the dim sends `close`.
pub fn over<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: impl Into<Element<'a, Message>>,
    width: f32,
    close: Message,
) -> Element<'a, Message> {
    let card = surface(Material::Widget, radius::CARD, content)
        .width(Length::Fixed(width))
        .max_width(width);
    over_sized(base, card, close)
}

/// `over`, rising (see `over_rising`).
pub fn over_lifted<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: impl Into<Element<'a, Message>>,
    width: f32,
    close: Message,
    rise: f32,
) -> Element<'a, Message> {
    let card = surface(Material::Widget, radius::CARD, content)
        .width(Length::Fixed(width))
        .max_width(width);
    over_rising(base, card, close, rise)
}

/// A dialog's option buttons, centred across the card (owner, 2026-11-27): the one row every
/// dialog uses, in reading order (from the right in RTL).
pub fn buttons<'a, Message: 'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(super::dir::row(items).spacing(space::GAP).align_y(Center))
        .center_x(Fill)
        .into()
}

/// The body of a small confirmation: `body` under the header, `buttons` below it.
pub fn pad<'a, Message: 'a>(parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(column(parts).spacing(space::PAGE[1]))
        .padding(space::PAGE)
        .into()
}

/// `over` for content that is its own solid card and sizes itself (a sheet, a dialog):
/// the dim, the centring and the outside press are the same.
pub fn over_sized<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    card: impl Into<Element<'a, Message>>,
    close: Message,
) -> Element<'a, Message> {
    over_rising(base, card, close, 0.0)
}

/// `over_sized`, the card `rise` px lower than it will rest (it opens: the host lowers `rise` to
/// 0 over `motion::OPEN`; the dim is static, only the card's place redraws).
pub fn over_rising<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    card: impl Into<Element<'a, Message>>,
    close: Message,
    rise: f32,
) -> Element<'a, Message> {
    let dim = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(fixed::MODAL_DIM.into()),
                ..container::Style::default()
            }),
    )
    .on_press(close); // mouse only: a press outside the card closes it (Esc is the host's)
    // (centred, then lowered: twice the rise as top padding moves the centre by the rise)
    let placed = container(opaque(card.into()))
        .padding(iced::Padding {
            top: 2.0 * rise,
            ..iced::Padding::ZERO
        })
        .width(Fill)
        .height(Fill)
        .center(Fill);
    stack![base, dim, placed].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::button::{self, Kind};
    use crate::shared::kit::testing::shot;
    use iced::Size;

    /// Owner 2026-11-27: a dialog's option buttons sit in the middle of the card, not at its end.
    #[test]
    fn dialog_buttons_are_centred_across_the_card() {
        let size = Size::new(460.0, 60.0);
        let labels = ["Don't save", "Cancel", "Save"];
        let w = button::dialog_width(&labels);
        let row: Element<'_, ()> = buttons(vec![
            button::dialog(labels[0], w, Kind::Elevated, ()),
            button::dialog(labels[1], w, Kind::Elevated, ()),
            button::dialog(labels[2], w, Kind::Accent, ()),
        ]);
        let texts = shot::texts_at(row, size);
        assert_eq!(texts.len(), 3, "{texts:?}");
        // (the labels differ in width: compare the middles of the first and last button's words)
        let mid = |i: usize| texts[i].0.x + texts[i].0.width / 2.0;
        let centre = (mid(0) + mid(2)) / 2.0;
        assert!(
            (centre - size.width / 2.0).abs() < 2.0,
            "buttons off centre: {centre} of {}",
            size.width
        );
    }
}
