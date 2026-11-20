//! A slide-over: a panel from the window's end edge, full height, over the
//! window with a light scrim (a press on the scrim closes it, unless `busy`).
//! For big temporary tasks (Export walkthrough, Edit text, Cut out); small
//! glance-only things use `popover`.
use iced::widget::{column, container, mouse_area, opaque, stack};
use iced::{Center, Element, Fill, Length};

use super::icon_button::{self, State};
use super::surface::{Material, surface};
use super::text::text;
use super::tokens::{Role, space};
use super::{dir, divider, scroll};

/// The panel's width.
pub const WIDTH: f32 = 380.0;

/// `base` with the panel: `title`, a close button (`×`), then `body`.
pub fn over<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    title: &'a str,
    body: Element<'a, Message>,
    close: Message,
    busy: bool,
) -> Element<'a, Message> {
    let x = icon_button::square(
        super::icon::Icon::Close.glyph(),
        State::default(),
        (!busy).then(|| close.clone()),
    );
    let head =
        container(crate::dir_row![text(Role::ROW_TITLE, title).width(Fill), x].align_y(Center))
            .padding([space::GAP, space::PAGE[1]]);
    let panel = surface(
        Material::Widget,
        0.0,
        column![
            head,
            divider::divider(),
            scroll::scroll(container(body).padding(space::PAGE)).height(Fill)
        ],
    )
    .width(Length::Fixed(WIDTH))
    .height(Fill);
    let scrim = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill),
    )
    .on_press(close);
    let scrim: Element<'a, Message> = match busy {
        true => opaque(scrim),
        false => scrim.into(),
    };
    let placed = container(opaque(panel))
        .width(Fill)
        .height(Fill)
        .align_x(dir::end());
    stack![base, scrim, placed].into()
}
