//! Translate (`settings-window.js` "TRANSLATE"): the engine, the source and
//! target languages, and (Live translate on in Labs) the live refresh.
use iced::Element;
use iced::widget::column;
use iced::widget::tooltip::Position;

use super::super::state::{Message, Pick, SOURCES, State, issue};
use crate::config::Engine;
use crate::labs::Feature;
use crate::mt;
use crate::translate::lang::LANGUAGES;
use crate::ui::kit::dropdown::dropdown;
use crate::ui::kit::field::input;
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::error;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::tooltip::tip;

pub fn view(state: &State) -> Element<'_, Message> {
    let t = &state.draft.translate;
    let (from, glide) = state.thumb(t.engine, |p| match p {
        Pick::Engine(e) => Some(e),
        _ => None,
    });
    let engine = segmented(
        &[(Engine::Ai, "AI"), (Engine::Offline, "Offline")],
        t.engine,
        from,
        glide,
        Message::Engine,
    );
    let mut rows = column![setting_row::row(
        title("Engine"),
        match t.engine {
            Engine::Ai => "Your AI provider; offline when it isn't set up",
            Engine::Offline => "No key, nothing leaves your PC; more literal than AI",
        },
        engine,
    )];
    let source = SOURCES.iter().find(|l| l.code == t.source).copied();
    rows = rows.push(language_row(
        state,
        (
            "Source language",
            "What the text on screen is in; picks the text recognizer",
        ),
        dropdown(&SOURCES, source, &t.source, Message::Source),
        "translate.source",
    ));
    // Offline can't do every pair: say so (AI is the other way).
    let target_desc = match t.engine == Engine::Offline && !mt::available(&t.source, &t.target) {
        true => "No offline model for this pair: choose AI to translate it",
        false => "What the text is translated into",
    };
    let target = LANGUAGES.iter().find(|l| l.code == t.target).copied();
    rows = rows.push(language_row(
        state,
        ("Target language", target_desc),
        dropdown(LANGUAGES, target, &t.target, Message::Target),
        "translate.target",
    ));
    // Live translate is a Labs feature (until 0.3.0): its row only while on.
    if state.draft.labs.has(Feature::Live) {
        let problem = issue(&state.problems, "translate.live_interval_ms");
        let field =
            input("1500", &state.interval, true, problem.is_some()).on_input(Message::LiveInterval);
        let mut control = column![tip(field, "Live translate refresh interval", Position::Top)]
            .spacing(space::GAP);
        if let Some(e) = problem {
            control = control.push(error(Role::ROW_DESC, e));
        }
        rows = rows.push(setting_row::row(
            title("Live refresh"),
            "Milliseconds between updates in live translate",
            control,
        ));
    }
    rows.into()
}

fn language_row<'a>(
    state: &State,
    (name, desc): (&'a str, &'a str),
    picker: Element<'a, Message>,
    key: &str,
) -> Element<'a, Message> {
    let mut control = column![tip(picker, name, Position::Top)].spacing(space::GAP);
    if let Some(e) = issue(&state.problems, key) {
        control = control.push(error(Role::ROW_DESC, e));
    }
    setting_row::row(title(name), desc, control)
}
