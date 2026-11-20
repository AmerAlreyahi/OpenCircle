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

/// The dialog's top line: `title` (and a grey one-line `subtitle`) and the X.
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
    crate::dir_row![container(words).width(Fill), x]
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

/// The body of a small confirmation: `body` under the header, `buttons` (right) below it.
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
    let placed = container(opaque(card.into()))
        .width(Fill)
        .height(Fill)
        .center(Fill);
    stack![base, dim, placed].into()
}
