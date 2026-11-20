//! A popover: a small card anchored to a corner of `base`, over it; a press
//! anywhere outside the card closes it (the press is taken: nothing under it
//! acts). Esc is the host's (it sends the same `close`). For glance-only
//! things (picture info, Position, Actions); big temporary tasks use
//! `slide_over`.
use iced::widget::{container, mouse_area, opaque, stack};
use iced::{Element, Fill, Padding};

use super::dir;
use super::surface::{Material, surface};
use super::tokens::{radius, space};

/// Which corner of the base the card hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// Under the end edge (right; left in right-to-left).
    TopEnd,
    /// Over the start edge, at the bottom.
    BottomStart,
}

/// `base` with `content` in a card at `corner`, `gap` px in from the edges
/// on the side it hangs from (`top`: from the base's top).
pub fn over<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    corner: Corner,
    offset: Padding,
    close: Message,
) -> Element<'a, Message> {
    let card = opaque(
        surface(
            Material::Widget,
            radius::MENU,
            container(content).padding(space::GAP + 4.0),
        )
        .padding(0),
    );
    let placed = container(card)
        .padding(offset)
        .width(Fill)
        .height(Fill)
        .align_x(match corner {
            Corner::TopEnd => dir::end(),
            Corner::BottomStart => dir::start(),
        })
        .align_y(match corner {
            Corner::TopEnd => iced::alignment::Vertical::Top,
            Corner::BottomStart => iced::alignment::Vertical::Bottom,
        });
    let catch = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill),
    )
    .on_press(close.clone())
    .on_right_press(close);
    stack![base, catch, placed].into()
}
