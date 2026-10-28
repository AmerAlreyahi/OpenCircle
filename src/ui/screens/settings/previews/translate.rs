//! Translate: a line on the screen is read, cleared, and turns into Arabic in
//! its place, word by word.
use iced::widget::{Space, column, container, row};
use iced::{Center, Element, Fill, Right};

use super::super::state::Message;
use crate::ui::kit::hero::{between, glyph, screen, step};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::text::{accent, sub, text};
use crate::ui::kit::tokens::{Role, space};

pub const STEPS: [&str; 5] = [
    "Choose a language",
    "Read the screen",
    "Clear the original text",
    "Place the translation",
    "Read it in place",
];

const ORIGINAL: &str = "Translate the text on your screen, in place.";
const ARABIC: &str = "ترجم النص على شاشتك في مكانه.";
/// The original's place, kept as it turns (one line).
const LINE_H: f32 = 18.0;

pub fn scene(at: f32) -> Element<'static, Message> {
    let s = step(at);
    let mut head = row![
        glyph(Icon::Translate),
        sub(Role::CARD_DESC, "Translate in place"),
        Space::new().width(Fill),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    if s >= 3 {
        head = head.push(accent(Role::CARD_DESC, "Translation ready"));
    }
    let line: Element<'static, Message> = match s {
        0 | 1 => text(Role::META, ORIGINAL).into(),
        2 => sub(Role::META, ORIGINAL).into(),
        _ => {
            // the words land one after the other (right to left)
            let words: Vec<_> = ARABIC.split(' ').collect();
            let n = (between(at, 3.0, 3.8) * words.len() as f32).ceil() as usize;
            let shown = words[..n.max(1)].join(" ");
            container(accent(Role::META_STRONG, shown))
                .width(Fill)
                .align_x(Right)
                .into()
        }
    };
    container(screen(
        column![head, container(line).center_y(LINE_H)].spacing(space::GAP),
    ))
    .center(Fill)
    .into()
}
