//! Settings round 2: the floating card both the sidebar and the page sit on, over the window's calm
//! pane (`themes::Colours::sidebar`): the `widget` fill, 18 px corners, a 1 px edge (the divider on a
//! light theme, the rim on a dark one) and no drawn shadow (`arch_tests::no_drawn_shadows`: the edge alone lifts it).
use iced::widget::{Container, container};
use iced::{Border, Element};

use super::themes::{c, current};
use super::tokens::settings::CARD_RADIUS;

/// `content` on the floating card; the caller sizes and pads it.
pub fn floating_card<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content).style(|_| {
        let light = current().light;
        container::Style {
            background: Some(c().widget.into()),
            border: Border::default()
                .rounded(CARD_RADIUS)
                .width(1)
                .color(if light { c().divider } else { c().rim }),
            ..container::Style::default()
        }
    })
}

/// The calm pane the cards float on: a window's body (or a dialog's, `radius` its corners) in `sidebar`, with the 1 px rim.
pub fn pane<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    radius: f32,
) -> Container<'a, Message> {
    container(content).style(move |_| container::Style {
        background: Some(c().sidebar.into()),
        border: Border::default().rounded(radius).width(1).color(c().rim),
        ..container::Style::default()
    })
}
