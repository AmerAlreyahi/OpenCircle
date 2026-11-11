//! `.oc-chat-bubble`: the user's own message in a chat, 13 px `text` on
//! `elevated` with its edge, 10x14 px in, radius 16 with a 4 px corner at the
//! bottom right (the side it comes from), laid at the right of its row; right
//! to left, at the left of it with the corner at the bottom left. Its text has
//! the direction of its own content (first strong letter).
use iced::border::Radius;
use iced::widget::container;
use iced::{Border, Element, Fill};

use super::dir;
use super::text::text;
use super::themes::c;
use super::tokens::Role;

const RADIUS: f32 = 16.0;
const TAIL: f32 = 4.0;

pub fn bubble<'a, Message: 'a>(line: &'a str) -> Element<'a, Message> {
    let corners = match dir::rtl() {
        true => Radius::new(RADIUS).bottom_left(TAIL),
        false => Radius::new(RADIUS).bottom_right(TAIL),
    };
    let line = text(Role::CHAT, line)
        .line_height(1.45)
        .align_x(dir::content_start(line));
    let bubble = container(line)
        .padding([10, 14])
        .style(move |_| container::Style {
            background: Some(c().elevated.into()),
            border: Border {
                color: c().elevated_border,
                width: 1.0,
                radius: corners,
            },
            ..container::Style::default()
        });
    container(bubble).width(Fill).align_x(dir::end()).into()
}
