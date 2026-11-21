//! A slide-over: a panel from the window's end edge, full height, over the
//! window. For big temporary tasks (Export walkthrough, Edit text, Cut out);
//! small glance-only things use `popover`.
use iced::widget::{column, container, mouse_area, opaque, stack};
use iced::{Center, Element, Fill, Length};

use super::icon::Icon;
use super::icon_button::{self, State};
use super::surface::{Material, surface};
use super::text::text;
use super::tokens::{Role, space};
use super::{dir, divider, scroll};

/// The panel's width.
pub const WIDTH: f32 = 380.0;

/// What is behind the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scrim {
    /// The window stays usable beside the panel (Edit text works on the picture).
    None,
    /// A press outside closes it.
    Closes,
    /// A press outside does nothing and the panel can't be closed (a job runs).
    Holds,
}

/// `base` with the panel: `title`, a close button, then `body`. `top`: the
/// window's header height, which stays free (its caption buttons keep working).
pub fn over<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    title: &'a str,
    body: Element<'a, Message>,
    close: Message,
    scrim: Scrim,
    top: f32,
) -> Element<'a, Message> {
    over_wide(base, title, body, close, scrim, top, WIDTH)
}

/// `over` with a panel `width` wide (a roomy task: Export), never wider than the window's
/// two thirds.
pub fn over_wide<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    title: &'a str,
    body: Element<'a, Message>,
    close: Message,
    scrim: Scrim,
    top: f32,
    width: f32,
) -> Element<'a, Message> {
    let x = icon_button::square(
        Icon::Close.glyph(),
        State::default(),
        (scrim != Scrim::Holds).then(|| close.clone()),
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
    .width(Length::Fixed(width))
    .max_width(width)
    .height(Fill);
    let placed = container(opaque(panel))
        .padding(iced::Padding {
            top,
            ..iced::Padding::ZERO
        })
        .width(Fill)
        .height(Fill)
        .align_x(dir::end());
    // the scrim leaves the header free too
    let blocker = |press: Option<Message>| {
        let area = mouse_area(
            container(iced::widget::Space::new())
                .width(Fill)
                .height(Fill),
        );
        let area = match press {
            Some(m) => area.on_press(m), // mouse only: a press outside closes it (Esc is the host's)
            None => area,
        };
        column![iced::widget::Space::new().height(top), area]
    };
    match scrim {
        Scrim::None => stack![base, placed].into(),
        Scrim::Closes => stack![base, blocker(Some(close)), placed].into(),
        Scrim::Holds => stack![base, opaque(blocker(None)), placed].into(),
    }
}
