//! Snip: an area is captured, an arrow points, a private number is hidden,
//! then the picture is copied.
use iced::widget::{Space, column, container, row};
use iced::{Center, Element, Fill};

use super::super::state::Message;
use crate::ui::kit::hero::{bar, frame, glyph, pill, redact, screen, step};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::text::{accent, sub};
use crate::ui::kit::tokens::{Role, space};

pub const STEPS: [&str; 5] = [
    "Choose a screen area",
    "Capture the picture",
    "Add an arrow",
    "Hide private details",
    "Copy or drag it into a chat",
];

/// The arrow's, the private number's and the pill's places.
const ARROW_W: f32 = 70.0;
const SECRET_W: f32 = 72.0;
const PILL_W: f32 = 90.0;

pub fn scene(at: f32) -> Element<'static, Message> {
    let s = step(at);
    let arrow: Element<'static, Message> = match s >= 2 {
        true => row![glyph(Icon::Arrow), accent(Role::CARD_DESC, "Inspect")]
            .spacing(space::TRACK_PAD)
            .align_y(Center)
            .into(),
        false => Space::new().into(),
    };
    let secret: Element<'static, Message> = match s >= 3 {
        true => redact(SECRET_W),
        false => sub(Role::CARD_DESC, "4921 7731").into(),
    };
    let window = row![
        column![bar(72.0, true), bar(56.0, false), bar(64.0, false)].spacing(space::GAP),
        container(arrow).width(ARROW_W),
        container(secret).width(SECRET_W),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    let picture = match s >= 1 {
        true => frame(window),
        false => screen(window),
    };
    let copied = (s >= 4).then(|| pill(Icon::Copy, "Copied", true));
    container(
        row![picture, container(row![].push(copied)).width(PILL_W)]
            .spacing(space::GAP * 2.0)
            .align_y(Center),
    )
    .center(Fill)
    .into()
}
