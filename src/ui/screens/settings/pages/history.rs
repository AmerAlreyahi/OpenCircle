//! History (N-39, card 33): what OpenCircle keeps on this PC, one row per
//! kind (Recent captures, Chat history; song search joins when it ships) with
//! its switch, how many are kept and a Clear; then Clear all, which asks
//! first. The switches live only here: Snip and AI link to this page.
//! Counting and clearing run off the UI thread (`Outcome::History`).
use iced::widget::{Space, column, row};
use iced::{Element, Fill};

use super::super::state::{Message, Pick, Section, State};
use crate::history::Kind;
use crate::ui::kit::button::{Kind as Button, button, label};
use crate::ui::kit::segmented::boolean;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::tokens::{Role, space};

const OFF_ON: [&str; 2] = ["Off", "On"];

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let snip = d.snip.clone();
    let captures = row_of(
        state,
        Kind::Captures,
        "Recent captures",
        "The last 20 snips, on this PC only. Off deletes them",
        boolean(OFF_ON, d.snip.history, d.snip.history, 1.0, move |on| {
            Message::Snip(crate::config::SnipConfig {
                history: on,
                ..snip.clone()
            })
        }),
    );
    let (from, t) = state.thumb(d.chat.keep_history, |p| match p {
        Pick::KeepChats(v) => Some(v),
        _ => None,
    });
    let chats = row_of(
        state,
        Kind::Chats,
        "Chat history",
        "AI chats, text only, on this PC. Off deletes them",
        boolean(OFF_ON, d.chat.keep_history, from, t, Message::KeepChats),
    );
    column![captures, chats, clear_all(state)].into()
}

/// A kind's row: its switch, and under it how many are kept and its Clear.
fn row_of<'a>(
    state: &'a State,
    kind: Kind,
    name: &'a str,
    desc: &'a str,
    switch: Element<'a, Message>,
) -> Element<'a, Message> {
    let n = state.kept.map(|k| k.of(kind));
    let count = match n {
        None => "Counting…".to_string(),
        Some(0) => "Nothing kept".to_string(),
        Some(1) => "1 kept".to_string(),
        Some(n) => format!("{n} kept"),
    };
    let clear = button(
        label("Clear"),
        Button::Remove,
        n.is_some_and(|n| n > 0)
            .then_some(Message::ClearHistory(kind)),
    );
    let under =
        row![sub(Role::ROW_DESC, count), Space::new().width(Fill), clear].align_y(iced::Center);
    let control = column![switch, under].spacing(space::GAP);
    setting_row::row(title(name), desc, control)
}

/// Clear all: a quiet button; pressed, the row asks to be sure.
fn clear_all(state: &State) -> Element<'_, Message> {
    if state.clearing_all {
        let buttons = row![
            Space::new().width(Fill),
            button(
                label("Cancel"),
                Button::Elevated,
                Some(Message::KeepHistory)
            ),
            // the elevated button, its words in the danger colour
            button(
                error(Role::CONTROL, "Delete all").line_height(1.5),
                Button::Elevated,
                Some(Message::ClearAll(true)),
            ),
        ]
        .spacing(space::GAP)
        .align_y(iced::Center);
        return setting_row::row(
            title("Clear all"),
            "Delete every kept capture and chat? This can't be undone",
            buttons,
        );
    }
    let any = state
        .kept
        .is_some_and(|k| Kind::ALL.iter().any(|&x| k.of(x) > 0));
    let ask = button(
        label("Clear all…"),
        Button::Remove,
        any.then_some(Message::ClearAll(false)),
    );
    setting_row::row(
        title("Clear all"),
        "Every kept capture and chat on this PC",
        row![Space::new().width(Fill), ask],
    )
}

/// A row on another page for a switch that lives here: what it is, and a
/// button that opens History (Snip's Recent captures, AI's Chat history).
pub fn link<'a>(name: &'a str, desc: &'a str) -> Element<'a, Message> {
    let open = button(
        label("Open History"),
        Button::Elevated,
        Some(Message::Section(Section::History)),
    );
    setting_row::row(title(name), desc, row![Space::new().width(Fill), open])
}
