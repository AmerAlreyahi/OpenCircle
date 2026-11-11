//! General (`settings-window.js` "GENERAL"): what Enter does, start with the
//! OS, the save folder, updates.
use crate::app_language::{S, count, fmt, tr};
use crate::t;
use iced::widget::tooltip::Position;
use iced::widget::{column, container};
use iced::{Center, Element, Fill};

use super::super::state::{Message, Pick, State, UpdateLine, issue};
use crate::config::DefaultAction;
use crate::ui::kit::button::{Kind, button, label, labelled, labelled_width};
use crate::ui::kit::field::field;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::segmented::{boolean, segmented};
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::error;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::tooltip::{Why, hint, tip};

fn off_on() -> [&'static str; 2] {
    [t!(c_off), t!(c_on)]
}

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let general = &d.general;
    let (from, glide) = state.thumb(general.default_action, |p| match p {
        Pick::Default(a) => Some(a),
        _ => None,
    });
    let actions = [
        (DefaultAction::Lens, t!(g_default_search)),
        (DefaultAction::Ai, t!(g_default_ai)),
        (DefaultAction::Text, t!(g_default_text)),
        (DefaultAction::Copy, t!(g_default_copy)),
    ];
    let mut rows = column![
        language_row(state),
        setting_row::row(
            title(t!(g_default)),
            t!(g_default_desc),
            segmented(
                &actions,
                general.default_action,
                from,
                glide,
                Message::DefaultAction
            ),
        )
    ];
    rows = rows.push(start_row(state));
    rows = rows.push(folder_row(state));
    rows = rows.push(update_row(state));
    rows = rows.push(updates_row(state));
    rows.into()
}

/// App language (owner, 2026-11-11): "Your language (…)" (follows the
/// onboarding pick; the default), then each language in its own name. A
/// pick speaks at once in every window (no restart).
fn language_row(state: &State) -> Element<'_, Message> {
    use crate::app_language::{CHOICES, Choice};
    let now = CHOICES
        .into_iter()
        .find(|c| c.0 == state.draft.general.app_language)
        .unwrap_or(Choice(""));
    setting_row::row(
        title(t!(g_lang)),
        t!(g_lang_desc),
        crate::ui::kit::dropdown::dropdown(&CHOICES, Some(now), "", Message::AppLanguage),
    )
}

/// Updates: what waits and when GitHub last answered; Check now, or the
/// waiting update's What's new and Restart now (Get it where the user installs it).
fn updates_row(state: &State) -> Element<'_, Message> {
    use crate::update::How;
    let waiting = (state.update_waiting.as_ref()).map(|(v, how)| (v.as_str(), *how));
    use crate::update::{Head, Say};
    let last = match &state.update_status {
        Some(UpdateLine::Answer(a)) => Some(a),
        _ => None,
    };
    let (head, line) = crate::update::status(
        waiting,
        env!("CARGO_PKG_VERSION"),
        state.update_checked,
        last,
        std::time::SystemTime::now(),
    );
    let head = match head {
        Head::Out(v) => fmt(S::up_out, &[&v]),
        Head::Ready(v) => fmt(S::up_ready, &[&v]),
        Head::Updates => t!(us_updates).to_string(),
    };
    let line = match (&state.update_status, line) {
        (Some(UpdateLine::Checking), _) => t!(key_checking).to_string(),
        (_, Say::GetIt) => t!(up_line_getit).to_string(),
        (_, Say::Have(v, when)) => fmt(S::us_have, &[&v, &when_words(when)]),
        (_, Say::Answer(a)) => answer_words(&a),
    };
    let control: Element<'_, Message> = match waiting {
        None => button(
            label(t!(g_updates_check)),
            Kind::Elevated,
            Some(Message::CheckUpdate),
        ),
        Some((_, how)) => {
            let (glyph, words) = match how {
                How::Restart => (Icon::Restart, t!(g_restart_now)),
                How::Setup => (Icon::Shield, t!(g_restart_now)),
                How::GetIt => (Icon::External, t!(g_get_it)),
            };
            let main = labelled(
                glyph.glyph(),
                words,
                labelled_width(words),
                Kind::Accent,
                false,
                Some(Message::RestartToUpdate),
            );
            let notes = button(label(t!(g_whats_new)), Kind::Quiet, Some(Message::WhatsNew));
            container(
                crate::dir_row![notes, main]
                    .spacing(space::GAP * 2.0)
                    .align_y(Center),
            )
            .align_right(Fill)
            .into()
        }
    };
    setting_row::row(title(head), line, control)
}

/// When the last check was, in the app's language.
fn when_words(when: crate::update::When) -> String {
    use crate::update::When;
    match when {
        When::Never => t!(us_never).into(),
        When::JustNow => t!(us_just).into(),
        When::Minutes(n) => fmt(S::us_min, &[&n]),
        When::Hours(n) => fmt(S::us_hours, &[&n]),
        When::Yesterday => t!(us_yesterday).into(),
        When::Days(n) => count(
            n as usize,
            [
                S::us_days_one,
                S::us_days_two,
                S::us_days_few,
                S::us_days_many,
            ],
        ),
    }
}

/// What a check found, in the app's language.
fn answer_words(a: &crate::update::Answer) -> String {
    use crate::update::Answer;
    match a {
        Answer::Wait(s) => fmt(S::ua_wait, &[&s.div_ceil(60)]),
        Answer::Unreachable => t!(ua_unreachable).into(),
        Answer::Out(v) => fmt(S::ua_out, &[v]),
        Answer::Ready(v) => fmt(S::ua_ready, &[v]),
        Answer::UpToDate(v) => fmt(S::ua_current, &[v]),
        Answer::DevBuild => t!(ua_dev).into(),
        Answer::NoFolder => t!(ua_nofolder).into(),
        Answer::Unverified(v) => fmt(S::ua_unverified, &[v]),
    }
}

fn switch<'a>(
    state: &'a State,
    on: bool,
    pick: fn(Pick) -> Option<bool>,
    m: fn(bool) -> Message,
) -> Element<'a, Message> {
    let (from, glide) = state.thumb(on, pick);
    boolean(off_on(), on, from, glide, m)
}

/// Start with the OS (Settings, and onboarding's last step).
pub fn start_row(state: &State) -> Element<'_, Message> {
    setting_row::row(
        title(tr(START_LABEL)),
        t!(g_start_desc),
        switch(
            state,
            state.draft.general.autostart,
            |p| match p {
                Pick::Autostart(v) => Some(v),
                _ => None,
            },
            Message::Autostart,
        ),
    )
}

/// Keep OpenCircle up to date (Settings, and onboarding's first step).
pub fn update_row(state: &State) -> Element<'_, Message> {
    setting_row::row(
        title(t!(g_update)),
        t!(g_update_desc),
        switch(
            state,
            state.draft.update.auto == Some(true),
            |p| match p {
                Pick::Update(v) => Some(v),
                _ => None,
            },
            Message::AutoUpdate,
        ),
    )
}

/// Where Save starts (General and Snip: the one folder).
pub(super) fn folder_row(state: &State) -> Element<'_, Message> {
    setting_row::row(title(t!(g_folder)), t!(g_folder_desc), folder(state))
}

/// The folder (cut in the middle when long: the tooltip shows it whole), and
/// Browse… / Reset under it at the right.
fn folder(state: &State) -> Element<'_, Message> {
    let folder = state.draft.general.save_folder.as_str();
    let shown = match folder.is_empty() {
        true => crate::actions::save::default_folder().map_or_else(
            || t!(g_folder_system).into(),
            |p| p.to_string_lossy().into_owned(),
        ),
        false => shorten(folder),
    };
    let value = container(field(
        &shown,
        None::<fn(String) -> Message>,
        Message::BrowseFolder,
    ))
    .width(Fill);
    let value = if folder.is_empty() || shorten(folder) == folder {
        tip(value, t!(g_folder), Position::Top)
    } else {
        hint(value, t!(g_folder), Why::Cut(folder), Position::Top)
    };
    let browse = button(
        label(t!(c_browse)),
        Kind::Elevated,
        Some(Message::BrowseFolder),
    );
    let reset = button(
        label(t!(c_reset)),
        Kind::Elevated,
        (!folder.is_empty()).then_some(Message::ClearFolder),
    );
    let mut col = column![setting_row::field_with(
        value,
        [
            tip(browse, t!(g_folder_tip_browse), Position::Bottom),
            tip(reset, t!(g_folder_tip_reset), Position::Bottom),
        ],
    )]
    .spacing(space::GAP);
    if let Some(e) = issue(&state.problems, "general.save_folder") {
        col = col.push(error(Role::ROW_DESC, e));
    }
    col.into()
}

/// A long folder path cut in the middle so it fits its field.
fn shorten(path: &str) -> String {
    // the field beside Browse… and Reset in the control column
    const MAX: usize = 20;
    let n = path.chars().count();
    if n <= MAX {
        return path.to_string();
    }
    let tail: String = path.chars().skip(n - (MAX - 1)).collect();
    format!("…{tail}")
}

/// The Start-with-the-OS switch's label (Settings and onboarding).
pub const START_LABEL: S = if cfg!(windows) {
    S::g_start_win
} else {
    S::g_start_linux
};
