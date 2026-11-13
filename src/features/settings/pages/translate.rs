//! Translate (`settings-window.js` "TRANSLATE"): the engine, the source and
//! target languages, (Live translate on in Labs) the live refresh, the
//! offline models' update (while one still has the older v1 decoder) and
//! the OptimaMT credit.
use crate::shared::app_language::{S, fmt};
use crate::t;
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container};
use iced::{Element, Fill};

use super::super::state::{Message, Named, Pick, SOURCES, State, TARGETS, issue};

use crate::features::settings::labs::Feature;
use crate::features::text_reading::screens::model_download::mb;
use crate::mt;
use crate::shared::config::Engine;
use crate::shared::kit::button::{Kind, button, label, regular};
use crate::shared::kit::dropdown::dropdown;
use crate::shared::kit::field::input;
use crate::shared::kit::progress;
use crate::shared::kit::segmented::segmented;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::text::{error, paragraph, sub};
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::tooltip::tip;

/// The OptimaMT models (the lighter decoders) on Hugging Face.
pub const OPTIMA_MT: &str = "https://huggingface.co/OptimaMT";

/// The model row's state: the installed pairs and how many still have the
/// v1 decoder (with the bytes their update fetches), and the update.
#[derive(Debug, Clone, PartialEq)]
pub struct Models {
    pub(crate) old: usize,
    pub(crate) all: usize,
    pub(crate) bytes: u64,
    pub(crate) now: Now,
    /// The row's description and button, made from the above.
    desc: String,
    button: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Now {
    Idle,
    /// Update now runs: 0..=1.
    Updating(f32),
    /// It stopped (why, in the app's language).
    Failed(String),
}

impl Models {
    /// Counted from the disk (sizes only).
    pub fn count() -> Models {
        // (a pair whose v2 file is already here only waits for its swap)
        let (old, all) = mt::installed();
        let bytes: Vec<u64> = old
            .iter()
            .map(|p| p.update_bytes())
            .filter(|&b| b > 0)
            .collect();
        Models::of(bytes.len(), all, bytes.iter().sum(), Now::Idle)
    }

    pub(crate) fn of(old: usize, all: usize, bytes: u64, now: Now) -> Models {
        let mut m = Models {
            old,
            all,
            bytes,
            now,
            desc: String::new(),
            button: String::new(),
        };
        m.words();
        m
    }

    fn words(&mut self) {
        let size = format!("{} MB", mb(self.bytes));
        self.desc = match &self.now {
            Now::Idle => fmt(S::tr_models_old, &[&self.old, &self.all]),
            Now::Updating(share) => fmt(
                S::tr_models_updating,
                &[&format!("{:.0}%", share * 100.0), &size],
            ),
            Now::Failed(why) => why.clone(),
        };
        self.button = fmt(S::tr_models_update, &[&size]);
    }

    /// The page is visited: counted again, unless an update runs.
    pub(crate) fn recount(&mut self) {
        if !matches!(self.now, Now::Updating(_)) {
            *self = Models::count();
        }
    }

    /// Update now: `true` = start it (something to fetch, none running).
    pub(crate) fn start(&mut self) -> bool {
        if matches!(self.now, Now::Updating(_)) || self.bytes == 0 {
            return false;
        }
        self.now = Now::Updating(0.0);
        self.words();
        true
    }

    pub(crate) fn progress(&mut self, share: f32) {
        if let Now::Updating(at) = &mut self.now {
            *at = share;
            self.words();
        }
    }

    /// The update ended (a Cancel too, once its thread has stopped and
    /// swapped in what was complete): counted again.
    pub(crate) fn finished(&mut self, done: Result<(), String>) {
        if !matches!(self.now, Now::Updating(_)) {
            return;
        }
        *self = Models::count();
        if let Err(why) = done
            && self.bytes > 0
        {
            self.now = Now::Failed(why);
            self.words();
        }
    }

    /// The installed pairs all have the lighter decoder.
    fn current(&self) -> bool {
        self.all > 0 && self.bytes == 0 && !matches!(self.now, Now::Updating(_))
    }
}

/// The model row, while an installed pair still has the v1 decoder: how
/// many, and Update now (its progress and Cancel; Try again after a stop).
fn models_row(m: &Models) -> Option<Element<'_, Message>> {
    if m.current() || (m.bytes == 0 && m.now == Now::Idle) {
        return None;
    }
    let control: Element<'_, Message> = match m.now {
        Now::Idle => setting_row::action(&m.button, Kind::Elevated, Some(Message::UpdateModels)),
        Now::Updating(share) => column![
            progress::bar(share),
            crate::dir_row![
                Space::new().width(Fill),
                regular(t!(c_cancel), false, Some(Message::StopModels))
            ],
        ]
        .spacing(space::GAP)
        .into(),
        Now::Failed(_) => setting_row::action(
            t!(md_try_again),
            Kind::Elevated,
            Some(Message::UpdateModels),
        ),
    };
    Some(setting_row::row(title(t!(tr_models)), &m.desc, control))
}

/// Under the rows: offline translation runs on OptimaMT models (and, when
/// every installed one has the lighter decoder, that they are up to date),
/// with the link.
fn optima(m: &Models) -> Element<'_, Message> {
    let mut note = column![paragraph(sub(Role::ROW_DESC, t!(tr_optima)))];
    if m.current() {
        note = note.push(paragraph(sub(Role::ROW_DESC, t!(tr_models_current))));
    }
    let link = button(
        label(t!(tr_optima_link)),
        Kind::Quiet,
        Some(Message::OpenLink(OPTIMA_MT.into())),
    );
    container(
        note.push(link)
            .spacing(space::GAP)
            .align_x(crate::shared::kit::dir::start()),
    )
    .width(Fill)
    .padding([space::ROW_Y, 0.0])
    .into()
}

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
    if let Some(row) = models_row(&state.models) {
        rows = rows.push(row);
    }
    rows.push(optima(&state.models)).into()
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
