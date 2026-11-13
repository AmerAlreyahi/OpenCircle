//! History (N-39, card 33): what OpenCircle keeps on this PC, one row per
//! kind (Recent captures, Chat history; song search joins when it ships) with
//! its switch, how many are kept and a Clear; then Clear all, which asks
//! first. The switches live only here: Snip and AI link to this page.
//! Counting and clearing run off the UI thread (`Outcome::History`).
use crate::t;
use iced::widget::{Space, column};
use iced::{Element, Fill};

use super::super::state::{Message, Pick, Section, State};
use crate::history::Kind;
use crate::shared::kit::button::{Kind as Button, button, label};
use crate::shared::kit::segmented::boolean;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::text::{error, sub};
use crate::shared::kit::tokens::{Role, space};

fn off_on() -> [&'static str; 2] {
    [t!(c_off), t!(c_on)]
}

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let snip = d.snip.clone();
    let captures = row_of(
        state,
        Kind::Captures,
        t!(s_recent),
        t!(h_captures_desc),
        boolean(off_on(), d.snip.history, d.snip.history, 1.0, move |on| {
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
        t!(ai_hist),
        t!(h_chats_desc),
        boolean(off_on(), d.chat.keep_history, from, t, Message::KeepChats),
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
        None => t!(h_counting).to_string(),
        Some(0) => t!(h_none).to_string(),
        Some(1) => "1 kept".to_string(),
        Some(n) => crate::app_language::fmt(crate::app_language::S::h_kept, &[&n]),
    };
    let clear = button(
        label(t!(h_clear)),
        Button::Remove,
        n.is_some_and(|n| n > 0)
            .then_some(Message::ClearHistory(kind)),
    );
    let under = crate::dir_row![sub(Role::ROW_DESC, count), Space::new().width(Fill), clear]
        .align_y(iced::Center);
    let control = column![switch, under].spacing(space::GAP);
    setting_row::row(title(name), desc, control)
}

/// Clear all: a quiet button; pressed, the row asks to be sure.
fn clear_all(state: &State) -> Element<'_, Message> {
    if state.clearing_all {
        let buttons = crate::dir_row![
            setting_row::cancel(t!(c_cancel), Message::KeepHistory),
            // the elevated button, its words in the danger colour
            setting_row::action_with(
                error(Role::CONTROL, t!(h_delete_all)).line_height(1.5),
                Button::Elevated,
                Some(Message::ClearAll(true)),
            ),
        ]
        .spacing(space::GAP);
        return setting_row::row(title(t!(h_all)), t!(h_delete_ask), buttons);
    }
    let any = state
        .kept
        .is_some_and(|k| Kind::ALL.iter().any(|&x| k.of(x) > 0));
    let ask = setting_row::action(
        t!(h_all_btn),
        Button::Remove,
        any.then_some(Message::ClearAll(false)),
    );
    setting_row::row(title(t!(h_all)), t!(h_all_desc), ask)
}

/// A row on another page for a switch that lives here: what it is, and a
/// button that opens History (Snip's Recent captures, AI's Chat history).
pub fn link<'a>(name: &'a str, desc: &'a str) -> Element<'a, Message> {
    let open = setting_row::action(
        t!(c_open_history),
        Button::Elevated,
        Some(Message::Section(Section::History)),
    );
    setting_row::row(title(name), desc, open)
}
