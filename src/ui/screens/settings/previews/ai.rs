//! AI: the screen is attached, a question is typed, the logo turns while the
//! AI thinks, then the answer comes in.
use std::time::Duration;

use iced::widget::{Space, column, container, row};
use iced::{Center, Element, Fill};

use super::super::state::Message;
use crate::ui::kit::hero::{between, glyph, pill, screen, step};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::spinner::{thinking, turn};
use crate::ui::kit::text::{sub, text};
use crate::ui::kit::tokens::{Role, space};

pub const STEPS: [&str; 5] = [
    "Select part of your screen",
    "Attach it to a question",
    "Ask what you want to know",
    "Read the answer",
    "Keep the conversation going",
];

const QUESTION: &str = "Explain this chart";
const ANSWER: &str = "The blue line shows steady growth over time.";
/// The answer's place, kept while the AI thinks (the thinking row's height).
const ANSWER_H: f32 = 32.0;

/// The first `share` of `s`'s letters.
fn typed(s: &str, share: f32) -> &str {
    let n = (share * s.chars().count() as f32).ceil() as usize;
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

pub fn scene(at: f32) -> Element<'static, Message> {
    let mut field = row![].spacing(space::GAP).align_y(Center);
    if step(at) >= 1 {
        field = field.push(pill(Icon::Crop, "Screen attached", true));
    }
    let asked = typed(QUESTION, between(at, 1.6, 2.4));
    let field = field
        .push(text(Role::CARD_DESC, asked))
        .push(Space::new().width(Fill))
        .push(sub(Role::CARD_DESC, "Enter"))
        .push(glyph(Icon::Enter));
    let answer: Element<'static, Message> = match at {
        _ if at < 2.5 => Space::new().into(),
        _ if at < 3.2 => thinking(
            crate::ui::widgets::chrome::logo_handle(),
            turn(Duration::from_secs_f32(at)),
            false,
            "Thinking…",
        ),
        _ => screen(sub(Role::CARD_DESC, typed(ANSWER, between(at, 3.2, 4.2)))),
    };
    container(
        column![
            screen(field),
            container(answer).center_y(ANSWER_H).width(Fill)
        ]
        .spacing(space::GAP),
    )
    .center(Fill)
    .into()
}
