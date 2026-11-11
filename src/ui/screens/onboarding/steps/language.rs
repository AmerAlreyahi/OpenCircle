//! Your language (step 1, owner 2026-11-10): the language picker's list
//! (search, Suggested, A-Z; the OS language preselected; each language in
//! its own name, its name in the app's language beside it) beside what the
//! pick means: text is translated into it, and which local model the first
//! translation downloads (nothing is downloaded before that, nothing is
//! sent). The pick is the app language too when the app speaks it (picking
//! Arabic turns onboarding Arabic at once).
use iced::Element;
use iced::widget::{Space, column};

use super::super::flow::{Message, State};
use crate::app_language::{S, fmt, language_name};
use crate::mt;
use crate::t;
use crate::translate::lang::{self, AUTO};
use crate::ui::kit::card::card;
use crate::ui::kit::dir;
use crate::ui::kit::text::{paragraph, sub, text};
use crate::ui::kit::tokens::{Role, space};
use crate::ui::screens::model_download::mb;

pub fn view(state: &State) -> Element<'_, Message> {
    let target = &state.settings.draft().translate.target;
    let words = column![
        card(
            column![
                text(Role::ROW_TITLE, t!(o_lang)),
                paragraph(sub(Role::ROW_DESC, t!(o_lang_desc)).line_height(dir::line(1.5))),
            ]
            .spacing(space::GAP / 2.0)
            .align_x(dir::start())
        ),
        card(paragraph(
            sub(Role::ROW_DESC, model_note(target)).line_height(dir::line(1.5))
        )),
    ]
    .spacing(space::GAP)
    .width(iced::Fill);
    let mut parts = vec![words.into(), Space::new().width(space::GAP).into()];
    parts.extend(state.picker.list().map(|l| l.map(Message::Language)));
    dir::row(parts).width(iced::Fill).into()
}

/// Which local model translating into `target` downloads the first time,
/// and its size (English text: the most common source).
pub(in super::super) fn model_note(target: &str) -> String {
    let name = language_name(target);
    if lang::find(target).is_some_and(|l| l.code == "en") {
        return t!(o_model_en).into();
    }
    let label = |p: &mt::Pair| {
        fmt(
            S::o_model_pair,
            &[&language_name(p.source), &language_name(p.target)],
        )
    };
    match mt::next("en", target) {
        Some(p) if p.missing_bytes() == 0 => fmt(S::o_model_here, &[&label(p)]),
        Some(p) => fmt(S::o_model_dl, &[&label(p), &mb(p.missing_bytes())]),
        None if mt::available(AUTO, target) => fmt(S::o_model_into, &[&name]),
        None => fmt(S::o_model_none, &[&name]),
    }
}
