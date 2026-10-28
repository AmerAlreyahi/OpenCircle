//! Circle to Search: a stroke circles a picture on a frozen screen, then the
//! bar's Search in browser pill shows and lights.
use iced::widget::{Space, column, container, row, stack};
use iced::{Center, Element, Fill};

use super::super::state::Message;
use crate::ui::kit::hero::{bar, between, pill, ring, screen, step};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::tokens::space;

pub const STEPS: [&str; 5] = [
    "Freeze your screen",
    "Circle what you want",
    "Choose Search in browser",
    "Open the results",
    "Ready to explore",
];

/// The frozen screen (`w-44 h-12`, with the stroke's room) and the pill's place.
const SCREEN_W: f32 = 176.0;
const SCREEN_H: f32 = 64.0;
const PILL_W: f32 = 150.0;
/// The picture's lines.
const LINES: [(f32, bool); 3] = [(64.0, true), (40.0, false), (52.0, false)];

pub fn scene(at: f32) -> Element<'static, Message> {
    let picture = column(LINES.map(|(w, strong)| bar(w, strong))).spacing(space::GAP);
    let frozen = stack![
        screen(container(picture).center(Fill)),
        ring(between(at, 1.0, 1.9)),
    ]
    .width(SCREEN_W)
    .height(SCREEN_H);
    let search = (step(at) >= 2).then(|| pill(Icon::Search, "Search in browser", step(at) >= 3));
    let slot = container(row![].push(search)).width(PILL_W);
    container(
        row![frozen, Space::new().width(space::GAP), slot]
            .spacing(space::GAP)
            .align_y(Center),
    )
    .center(Fill)
    .into()
}
