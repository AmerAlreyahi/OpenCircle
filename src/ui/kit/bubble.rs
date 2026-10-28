//! `.oc-chat-bubble`: the user's own message in a chat, 13 px `text` on
//! `elevated` with its edge, 10x14 px in, radius 16 with a 4 px corner at the
//! bottom right (the side it comes from), laid at the right of its row.
use iced::border::Radius;
use iced::widget::container;
use iced::{Border, Element, Fill};

use super::text::text;
use super::themes::c;
use super::tokens::Role;

const RADIUS: f32 = 16.0;
const TAIL: f32 = 4.0;

pub fn bubble<'a, Message: 'a>(line: &'a str) -> Element<'a, Message> {
    let bubble = container(text(Role::CHAT, line).line_height(1.45))
        .padding([10, 14])
        .style(|_| container::Style {
            background: Some(c().elevated.into()),
            border: Border {
                color: c().elevated_border,
                width: 1.0,
                radius: Radius::new(RADIUS).bottom_right(TAIL),
            },
            ..container::Style::default()
        });
    container(bubble).align_right(Fill).into()
}
