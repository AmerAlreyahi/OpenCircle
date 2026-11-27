//! History (N-39, card 33): what OpenCircle keeps on this PC, one row per
//! kind (Recent captures, Chat history; song search joins when it ships) with
//! its switch, how many are kept and a Clear; then Clear all, which asks
//! first. The switches live only here: Snip and AI link to this page.
//! Counting and clearing run off the UI thread (`Outcome::History`).
use crate::t;
use iced::widget::{Space, column};
use iced::{Element, Fill};

use super::super::state::{Message, Section, State};
use crate::shared::history::Kind;
use crate::shared::kit::button::{Kind as Button, button, label};
use crate::shared::kit::section::section;
use crate::shared::kit::segmented::boolean;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::text::{error, sub};
use crate::shared::kit::tokens::{Role, space};

fn off_on() -> [&'static str; 2] {
    [t!(c_off), t!(c_on)]
}

/// What is kept on this PC: Recent captures and Chat history, each with its switch and count.
pub(super) fn kept(state: &State) -> Vec<Element<'_, Message>> {
    let d = &state.draft;
    let snip = d.snip.clone();
    let captures = row_of(
        state,
        Kind::Captures,
        t!(s_recent),
        t!(h_captures_desc),
        boolean(t!(s_recent), off_on(), d.snip.history, move |on| {
            Message::Snip(crate::shared::config::SnipConfig {
                history: on,
                ..snip.clone()
            })
        }),
    );
    let chats = row_of(
        state,
        Kind::Chats,
        t!(ai_hist),
        t!(h_chats_desc),
        boolean(
            t!(ai_hist),
            off_on(),
            d.chat.keep_history,
            Message::KeepChats,
        ),
    );
    vec![captures, chats]
}

/// The page as its own (the "Open history" links go here): what is kept, then Clear all.
pub fn view(state: &State) -> Element<'_, Message> {
    column![
        section(Some(t!(pv_kept)), None, || kept(state)),
        section(None, None, || vec![clear_all(state)]),
    ]
    .spacing(crate::shared::kit::tokens::settings::SECTION_GAP)
    .into()
}

/// A kind's row: its switch, and under it how many are kept and its Clear.
fn row_of<'a>(
    state: &'a State,
    kind: Kind,
    name: &'a str,
    desc: &'a str,
    switch: Element<'a, Message>,
) -> Element<'a, Message> {
    use crate::shared::app_language::{S, fmt};
    let rec = state.kept.map_or(0, |k| {
        if kind == Kind::Captures {
            k.recordings
        } else {
            0
        }
    });
    let n = state.kept.map(|k| shown(k, kind));
    let count = match (n, rec) {
        (None, _) => t!(h_counting).to_string(),
        (Some(0), _) => t!(h_none).to_string(),
        // Recent also holds unsaved recordings: Clear takes them too, so the count says so
        (Some(1), 1) => t!(h_rec1).to_string(),
        (Some(n), r) if r > 0 && n == r => fmt(S::h_recs, &[&r]),
        (Some(n), 1) => fmt(S::h_kept_rec1, &[&(n - 1)]),
        (Some(n), r) if r > 1 => fmt(S::h_kept_rec, &[&(n - r), &r]),
        (Some(1), _) => "1 kept".to_string(),
        (Some(n), _) => fmt(S::h_kept, &[&n]),
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

/// How many a row says it keeps (and its Clear takes): Recent's recordings are in Recent's row.
pub(in crate::features::settings) fn shown(k: crate::shared::history::Counts, kind: Kind) -> usize {
    k.of(kind)
        + if kind == Kind::Captures {
            k.recordings
        } else {
            0
        }
}

/// Clear all: a quiet button; pressed, the row asks to be sure.
pub(in crate::features::settings) fn clear_all(state: &State) -> Element<'_, Message> {
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
        .is_some_and(|k| Kind::ALL.iter().any(|&x| shown(k, x) > 0));
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
