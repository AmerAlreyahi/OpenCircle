//! Translate (`settings-window.js` "TRANSLATE"): the engine, the source and
//! target languages, (Live translate on in Labs) the live refresh, the
//! offline models list (one row per installed pair: status, Update, progress,
//! Cancel, Add a language pair, OptimaMT link), and the Game glossaries row
//! with its one-line note.
use crate::shared::app_language::{S, fmt, language_name};
use crate::shared::kit::icon::{Icon, glyph};
use crate::t;
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container};
use iced::{Center, Element, Fill};

use super::super::state::{Message, Named, SOURCES, State, TARGETS, issue};

use crate::features::settings::labs::Feature;
use crate::features::text_reading::screens::model_download::mb;
use crate::features::translate::mt;
use crate::shared::config::Engine;
use crate::shared::kit::button::{Kind, button, label, regular};
use crate::shared::kit::card::{icon_tile, quiet_note};
use crate::shared::kit::chip::{Tone, status};
use crate::shared::kit::choice_card::{Choice as Card, Mark, choice_cards};
use crate::shared::kit::dropdown::dropdown;
use crate::shared::kit::field::input;
use crate::shared::kit::progress;
use crate::shared::kit::section::section;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::text::{accent, error, paragraph, sub, text};
use crate::shared::kit::tokens::{Role, radius, settings, size, space};
use crate::shared::kit::tooltip::tip;

/// The OptimaMT models (the lighter decoders) on Hugging Face.
pub const OPTIMA_MT: &str = "https://huggingface.co/OptimaMT";
/// Labs Live's "Live refresh" row (hidden: see where it is drawn).
const LIVE_REFRESH_ROW: bool = false;

/// One installed language pair shown in the Offline models list.
#[derive(Debug, Clone, PartialEq)]
pub struct PairRow {
    pub name: String,
    pub meta: String,
    pub outdated: bool,
    pub update_bytes: u64,
}

impl PairRow {
    pub fn from_codes(
        source: &str,
        target: &str,
        total_bytes: u64,
        outdated: bool,
        update_bytes: u64,
    ) -> Self {
        let name = format!(
            "{} \u{2192} {}",
            language_name(source),
            language_name(target)
        );
        let meta = fmt(S::tr_pair_meta, &[&source, &target, &mb(total_bytes)]);
        Self {
            name,
            meta,
            outdated,
            update_bytes,
        }
    }
}

/// The model section's state: the installed pairs and how many still have the
/// v1 decoder (with the bytes their update fetches), and the update.
#[derive(Debug, Clone, PartialEq)]
pub struct Models {
    pub(crate) old: usize,
    pub(crate) all: usize,
    pub(crate) bytes: u64,
    pub(crate) now: Now,
    pub(crate) pairs: Vec<PairRow>,
    /// The header's description and button, made from the above.
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
        let installed = mt::installed_pairs();
        let pairs: Vec<PairRow> = installed
            .iter()
            .map(|p| {
                PairRow::from_codes(
                    p.pair.source,
                    p.pair.target,
                    p.total_bytes,
                    p.outdated,
                    p.update_bytes,
                )
            })
            .collect();
        let update_bytes: Vec<u64> = installed
            .iter()
            .filter(|p| p.outdated && p.update_bytes > 0)
            .map(|p| p.update_bytes)
            .collect();
        let mut m = Models::of(
            update_bytes.len(),
            installed.len(),
            update_bytes.iter().sum(),
            Now::Idle,
        );
        m.pairs = pairs;
        m
    }

    pub(crate) fn of(old: usize, all: usize, bytes: u64, now: Now) -> Models {
        let mut m = Models {
            old,
            all,
            bytes,
            now,
            pairs: Vec::new(),
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

/// Top header of the Offline models section (`.mhead`): title + OptimaMT description, and the
/// 300 px "Update now (about N MB)" action when any installed pair is still on v1.
fn models_header(m: &Models) -> Element<'_, Message> {
    let mut left = column![
        title(t!(tr_models)),
        paragraph(sub(Role::ROW_DESC, t!(tr_optima)))
    ]
    .spacing(space::TRACK_GAP)
    .width(Fill)
    .align_x(crate::shared::kit::dir::start());
    if m.current() {
        left = left.push(paragraph(sub(Role::ROW_DESC, t!(tr_models_current))));
    } else if m.bytes > 0 && m.pairs.is_empty() {
        left = left.push(paragraph(sub(Role::ROW_DESC, &m.desc)));
    }
    let right: Option<Element<'_, Message>> = if m.current() || (m.bytes == 0 && m.now == Now::Idle)
    {
        None
    } else {
        let ctl: Element<'_, Message> = match &m.now {
            Now::Idle => {
                setting_row::action(&m.button, Kind::Elevated, Some(Message::UpdateModels))
            }
            Now::Updating(share) => crate::dir_row![
                container(progress::bar(*share)).width(Fill),
                regular(t!(c_cancel), false, Some(Message::StopModels)),
            ]
            .spacing(space::GAP)
            .align_y(Center)
            .into(),
            Now::Failed(_) => setting_row::action(
                t!(md_try_again),
                Kind::Elevated,
                Some(Message::UpdateModels),
            ),
        };
        Some(container(ctl).width(size::CONTROL_COLUMN).into())
    };
    let mut row = crate::dir_row![left]
        .spacing(settings::CARD_GAP)
        .align_y(Center);
    if let Some(r) = right {
        row = row.push(r);
    }
    container(row)
        .width(Fill)
        .padding([settings::ROW_PAD_Y, 0.0])
        .into()
}

/// One installed language pair row (`.mrow2`): translate icon tile, pair name + code/size,
/// and status chip / Update / progress + Cancel.
fn pair_row<'a>(pair: &'a PairRow, now: &'a Now, desc: &'a str) -> Element<'a, Message> {
    let tile = icon_tile(
        Icon::Translate.glyph(),
        size::ICON_SMALL,
        settings::MODEL_TILE,
        radius::CONTROL,
    );
    let info = column![
        text(Role::ROW_TITLE, &pair.name),
        sub(Role::ROW_DESC, &pair.meta),
    ]
    .spacing(space::TRACK_GAP)
    .width(Fill)
    .align_x(crate::shared::kit::dir::start());
    let right: Element<'a, Message> = match (pair.outdated, now) {
        (true, Now::Updating(share)) => crate::dir_row![
            container(progress::bar(*share)).width(settings::MODEL_BAR_W),
            sub(Role::ROW_DESC, desc),
            button(label(t!(c_cancel)), Kind::Quiet, Some(Message::StopModels)),
        ]
        .spacing(space::ROW_Y)
        .align_y(Center)
        .into(),
        (true, Now::Failed(_)) => crate::dir_row![
            status(t!(tr_m_stopped), Tone::Error),
            button(
                label(t!(md_try_again)),
                Kind::Elevated,
                Some(Message::UpdateModels)
            ),
        ]
        .spacing(space::ROW_Y)
        .align_y(Center)
        .into(),
        (true, Now::Idle) => crate::dir_row![
            status(t!(tr_m_old), Tone::Warn),
            button(
                label(t!(tr_m_update)),
                Kind::Elevated,
                Some(Message::UpdateModels)
            ),
        ]
        .spacing(space::ROW_Y)
        .align_y(Center)
        .into(),
        (false, _) => status(t!(tr_m_ok), Tone::Ok),
    };
    container(
        crate::dir_row![tile, info, right]
            .spacing(settings::CARD_GAP)
            .align_y(Center),
    )
    .width(Fill)
    .padding([space::ROW_Y, 0.0])
    .into()
}

/// Footer of the Offline models section (`.full.add`): "+ Add a language pair" on the start side,
/// "OptimaMT on Hugging Face" external link on the end side.
fn models_footer<'a>() -> Element<'a, Message> {
    let add = tip(
        button(
            label(format!("+ {}", t!(tr_m_add))),
            Kind::Quiet,
            Some(Message::Section(super::super::state::Section::Translate)),
        ),
        t!(tr_m_add_hint),
        Position::Top,
    );
    let link = button(
        crate::dir_row![
            accent(Role::CONTROL, t!(tr_optima_link)),
            glyph(Icon::External.glyph(), size::ICON_SMALL),
        ]
        .spacing(space::GAP)
        .align_y(Center),
        Kind::Quiet,
        Some(Message::OpenLink(OPTIMA_MT.into())),
    );
    container(
        crate::dir_row![add, Space::new().width(Fill), link]
            .spacing(space::GAP)
            .align_y(Center),
    )
    .width(Fill)
    .padding([space::ROW_Y, 0.0])
    .into()
}

pub fn view(state: &State) -> Element<'_, Message> {
    // the glossary editor is a sub-page of this page
    if state.glossaries.is_editing() {
        return super::glossaries::editor(state);
    }
    let page = column![
        section(Some(t!(sec_languages)), None, || language_rows(state)),
        section(Some(t!(sec_models)), None, || {
            let mut rows: Vec<Element<'_, Message>> = vec![models_header(&state.models)];
            if state.models.pairs.is_empty() && state.models.all == 0 {
                rows.push(
                    container(sub(Role::ROW_DESC, t!(tr_m_none)))
                        .padding([space::ROW_Y, 0.0])
                        .into(),
                );
            } else {
                for p in &state.models.pairs {
                    rows.push(pair_row(p, &state.models.now, &state.models.desc));
                }
            }
            rows.push(models_footer());
            rows
        }),
        section(Some(t!(sec_games)), None, || {
            vec![
                super::glossaries::row(state),
                quiet_note(Icon::Glossary.glyph(), t!(gl_note)),
            ]
        }),
    ]
    .spacing(settings::SECTION_GAP);
    page.into()
}

/// Engine (three cards, the design's order), source and target language, the Live refresh row.
fn language_rows(state: &State) -> Vec<Element<'_, Message>> {
    let t = &state.draft.translate;
    let engines = [
        (
            Engine::Offline,
            Mark::Glyph(Icon::Shield.glyph()),
            t!(tr_local),
            t!(tr_c_local_d),
        ),
        (Engine::Ai, Mark::Logo, t!(tr_ai), t!(tr_c_ai_d)),
        (
            Engine::Online,
            Mark::Glyph(Icon::Search.glyph()),
            t!(tr_online),
            t!(tr_c_online_d),
        ),
    ];
    let cards: Vec<_> = (engines.iter())
        .map(|&(_, mark, title_, desc)| Card {
            mark,
            title: title_,
            desc,
        })
        .collect();
    let at = engines.iter().position(|e| e.0 == t.engine).unwrap_or(0);
    let mut rows = vec![setting_row::block(
        title(t!(tr_engine)),
        match t.engine {
            Engine::Ai => t!(tr_engine_ai),
            Engine::Offline => t!(tr_engine_local),
            Engine::Online => t!(tr_engine_online),
        },
        choice_cards(&cards, at, move |i| Message::Engine(engines[i].0)),
    )];
    let source = SOURCES.iter().find(|l| l.0.code == t.source).copied();
    rows.push(language_row(
        state,
        (t!(tr_source), t!(tr_source_desc)),
        dropdown(&SOURCES, source, &t.source, |l: Named| Message::Source(l.0)),
        "translate.source",
    ));
    // Offline can't do every pair: say so (AI is the other way).
    let target_desc = match t.engine == Engine::Offline && !mt::available(&t.source, &t.target) {
        true => t!(tr_no_model),
        false => t!(tr_target_desc),
    };
    let target = TARGETS.iter().find(|l| l.0.code == t.target).copied();
    rows.push(language_row(
        state,
        (t!(tr_target), target_desc),
        dropdown(&TARGETS, target, &t.target, |l: Named| Message::Target(l.0)),
        "translate.target",
    ));
    // Live translate is a Labs feature (until Live translation): its row only while on.
    // Hidden (hide, don't delete): live translate reads every frame since 0.3.0,
    // so the refresh beat (`translate.live_interval_ms`, still read) does nothing.
    if LIVE_REFRESH_ROW && state.draft.labs.has(Feature::Live) {
        let problem = issue(&state.problems, "translate.live_interval_ms");
        let field =
            input("1500", &state.interval, true, problem.is_some()).on_input(Message::LiveInterval);
        let mut control = column![tip(field, t!(tr_live_tip), Position::Top)].spacing(space::GAP);
        if let Some(e) = problem {
            control = control.push(error(Role::ROW_DESC, e));
        }
        rows.push(setting_row::row(
            title(t!(tr_live)),
            t!(tr_live_desc),
            control,
        ));
    }
    rows
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
