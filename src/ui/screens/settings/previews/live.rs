//! Live translate: it starts, a line of dialogue comes, and its translation
//! takes its place under the scene.
use iced::widget::{Space, column, container};
use iced::{Element, Fill};

use super::super::state::Message;
use crate::ui::kit::hero::{pill, screen, step};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::text::{accent, sub, text};
use crate::ui::kit::tokens::{Role, space};

pub const STEPS: [&str; 5] = [
    "Start live translation",
    "Watch for new dialogue",
    "Translate the next line",
    "Keep up with the scene",
    "Pause whenever you need",
];

const LINE: &str = "\"The shield barrier has been restored.\"";
const ARABIC: &str = "تمت استعادة الحاجز الواقي.";
/// The subtitle's place.
const SUBTITLE_W: f32 = 320.0;
const SUBTITLE_H: f32 = 34.0;

pub fn scene(at: f32) -> Element<'static, Message> {
    let s = step(at);
    let line: Element<'static, Message> = match s {
        0 => Space::new().into(),
        1 => sub(Role::CARD_DESC, "Listening for dialogue…").into(),
        2 => text(Role::META, LINE).into(),
        _ => accent(Role::META_STRONG, ARABIC).into(),
    };
    let subtitle = container(screen(container(line).center(Fill)))
        .width(SUBTITLE_W)
        .height(SUBTITLE_H);
    container(
        column![
            pill(Icon::Radio, "Live translate · English → Arabic", s >= 1),
            Space::new().height(Fill),
            subtitle,
        ]
        .spacing(space::GAP)
        .align_x(iced::Center),
    )
    .center(Fill)
    .into()
}
