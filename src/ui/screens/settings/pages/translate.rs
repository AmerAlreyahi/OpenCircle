//! Translate (`settings-window.js` "TRANSLATE"): the engine, the source and
//! target languages, and (Live translate on in Labs) the live refresh.
use crate::t;
use iced::Element;
use iced::widget::column;
use iced::widget::tooltip::Position;

use super::super::state::{Message, Named, Pick, SOURCES, State, TARGETS, issue};

use crate::config::Engine;
use crate::labs::Feature;
use crate::mt;
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
        &[(Engine::Ai, t!(tr_ai)), (Engine::Offline, t!(tr_local))],
        t.engine,
        from,
        glide,
        Message::Engine,
    );
    let mut rows = column![setting_row::row(
        title(t!(tr_engine)),
        match t.engine {
            Engine::Ai => t!(tr_engine_ai),
            Engine::Offline => t!(tr_engine_local),
        },
        engine,
    )];
    let source = SOURCES.iter().find(|l| l.0.code == t.source).copied();
    rows = rows.push(language_row(
        state,
        (t!(tr_source), t!(tr_source_desc)),
        dropdown(&SOURCES, source, &t.source, |l: Named| Message::Source(l.0)),
        "translate.source",
    ));
    // Offline can't do every pair: say so (AI is the other way).
    let target_desc = match t.engine == Engine::Offline && !mt::available(&t.source, &t.target) {
        true => {
            t!(tr_no_model)
        }
        false => t!(tr_target_desc),
    };
    let target = TARGETS.iter().find(|l| l.0.code == t.target).copied();
    rows = rows.push(language_row(
        state,
        (t!(tr_target), target_desc),
        dropdown(&TARGETS, target, &t.target, |l: Named| Message::Target(l.0)),
        "translate.target",
    ));
    // Live translate is a Labs feature (until Live translation): its row only while on.
    if state.draft.labs.has(Feature::Live) {
        let problem = issue(&state.problems, "translate.live_interval_ms");
        let field =
            input("1500", &state.interval, true, problem.is_some()).on_input(Message::LiveInterval);
        let mut control = column![tip(field, t!(tr_live_tip), Position::Top)].spacing(space::GAP);
        if let Some(e) = problem {
            control = control.push(error(Role::ROW_DESC, e));
        }
        rows = rows.push(setting_row::row(
            title(t!(tr_live)),
            t!(tr_live_desc),
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
