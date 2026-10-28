//! Text reading: a drag selects words on the screen to the letter, then
//! "Copied" shows.
use iced::widget::{column, container, row, stack};
use iced::{Center, Element, Fill, Right, Top};

use super::super::state::Message;
use crate::ui::kit::hero::{between, mark, pill, step};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::text::{MONO, sub, text};
use crate::ui::kit::tokens::{Role, space};

pub const STEPS: [&str; 5] = [
    "Find text on your screen",
    "Drag across the words",
    "Select exactly what you need",
    "Copy the text",
    "Ready to paste",
];

const PICKED: &str = "these words";

pub fn scene(at: f32) -> Element<'static, Message> {
    let line = |s: &'static str| sub(Role::CARD_DESC, s).font(MONO);
    // the drag reaches one more letter at a time
    let n = (between(at, 1.0, 2.6) * PICKED.len() as f32).ceil() as usize;
    let (picked, rest) = PICKED.split_at(n);
    let mut middle = row![line("Select ")].align_y(Center);
    if n > 0 {
        let word = text(Role::CARD_DESC, picked).font(MONO);
        middle = middle.push(mark(word.into(), step(at) < 3));
    }
    let middle = middle
        .push(text(Role::CARD_DESC, rest).font(MONO))
        .push(line(" and copy them."));
    let lines = column![
        line("Your screen is full of useful words."),
        middle,
        line("Paste them into any app."),
    ]
    .spacing(space::GAP);
    let copied = (step(at) >= 3).then(|| pill(Icon::Copy, "Copied (Ctrl+C)", true));
    stack![
        container(lines).center(Fill),
        container(row![].push(copied))
            .width(Fill)
            .height(Fill)
            .align_x(Right)
            .align_y(Top),
    ]
    .into()
}
