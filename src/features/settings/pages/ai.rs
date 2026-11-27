//! AI (`settings-window.js` "AI"): who answers (Browser | AI provider), and
//! for an AI provider its name (one dropdown of real services; the API each
//! speaks is chosen inside, never shown: owner, 2026-10-28), a Custom
//! server's URL, the model and the key (key + Save; mono where a value is
//! code). The design's sign-in, local-model and answer-style parts belong to
//! AI online and are not drawn.
use crate::shared::app_language::tr;
use crate::shared::kit::icon::Icon;
use crate::t;
use iced::widget::tooltip::Position;
use iced::widget::{Space, column};
use iced::{Element, Fill};

use super::super::state::{
    Busy, ChatZoom, KEY_MASK, KeyField, KeyMessage, KeyStatus, Message, PRESETS, Pick, Preset,
    State, issue,
};
use crate::features::ask_ai::screens::model_list;
use crate::shared::config::{Provider, StaysOpen};
use crate::shared::kit::button::{Kind, button, label};
use crate::shared::kit::choice_card::{Choice as Card, Mark, choice_cards};
use crate::shared::kit::dropdown::dropdown;
use crate::shared::kit::field::input;
use crate::shared::kit::section::section;
use crate::shared::kit::segmented::{boolean, segmented};
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::text::{error, sub};
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::tooltip::tip;
use crate::shared::secrets::Secret;

pub fn view(state: &State) -> Element<'_, Message> {
    column![
        section(Some(t!(sec_answers)), None, || answer_rows(state)),
        section(Some(t!(sec_chat)), None, || chat_rows(state)),
    ]
    .spacing(crate::shared::kit::tokens::settings::SECTION_GAP)
    .into()
}

/// The AI chat's rows, for every provider (the chat's drawer has the first too).
fn chat_rows(state: &State) -> Vec<Element<'_, Message>> {
    let chat = &state.draft.chat;
    let (from, t) = state.thumb(chat.stays_open, |p| match p {
        Pick::ChatStays(v) => Some(v),
        _ => None,
    });
    let stays = setting_row::row(
        title(t!(ai_stays)),
        t!(ai_stays_desc),
        segmented(
            &[
                (StaysOpen::FollowUp, t!(ai_stays_follow)),
                (StaysOpen::Always, t!(ai_stays_always)),
            ],
            chat.stays_open,
            from,
            t,
            Message::ChatStays,
        ),
    );
    // its switch is on History (the one place for what is kept)
    let keep = super::history::link(
        t!(ai_hist),
        match chat.keep_history {
            true => t!(ai_hist_on),
            false => t!(ai_hist_off),
        },
    );
    // an AI provider only (the browser has no tools); not asked yet = Off
    let on = state.draft.ai.web_search == Some(true);
    let web = setting_row::row(
        title(t!(ai_web)),
        t!(ai_web_desc),
        boolean(t!(ai_web), [t!(c_off), t!(c_on)], on, Message::WebSearch),
    );
    // the chat's text zoom (also Ctrl+= / Ctrl+- / Ctrl+0 in the chat)
    let zoom = setting_row::row(
        title(t!(ai_zoom)),
        t!(ai_zoom_desc),
        dropdown(
            &ChatZoom::ALL,
            Some(ChatZoom(state.draft.ai.chat_zoom)),
            "",
            Message::ChatZoom,
        ),
    );
    match keyed(state) {
        true => vec![stays, zoom, web, keep],
        false => vec![stays, keep],
    }
}

/// Which of the three setup paths the config is on (`None`: an unreadable value, none chosen).
fn path(ai: &crate::shared::config::AiConfig) -> Option<usize> {
    match (&ai.provider, Preset::of(ai)) {
        (Provider::Browser, _) => Some(0),
        (Provider::Invalid(_) | Provider::Unreadable, _) => None,
        (_, Some(p)) if p.signs_in() => Some(1),
        _ => Some(2),
    }
}

/// The presets "Your own provider" offers: every one but OpenRouter (its own card).
static OWN: std::sync::LazyLock<Vec<Preset>> =
    std::sync::LazyLock::new(|| PRESETS.iter().filter(|p| !p.signs_in()).copied().collect());

/// Who answers: the three setup cards (Browser / OpenRouter account / Your own provider and key), then
/// the chosen path's rows.
fn answer_rows(state: &State) -> Vec<Element<'_, Message>> {
    let ai = &state.draft.ai;
    let now = path(ai);
    let cards = [
        Card {
            mark: Mark::Glyph(Icon::Search.glyph()),
            title: t!(ai_browser),
            desc: t!(ai_c_browser_d),
        },
        Card {
            mark: Mark::Logo,
            title: t!(ai_c_or),
            desc: t!(ai_c_or_d),
        },
        Card {
            mark: Mark::Glyph(Icon::Chat.glyph()),
            title: t!(ai_c_own),
            desc: t!(ai_c_own_d),
        },
    ];
    let current = Message::Provider(ai.provider.clone());
    let mut picker = column![choice_cards(&cards, now.unwrap_or(usize::MAX), move |i| {
        match (i, now) {
            (0, _) => Message::Provider(Provider::Browser),
            _ if now == Some(i) => current.clone(),
            (1, _) => Message::Preset(PRESETS[0]),
            _ => Message::Preset(OWN[0]),
        }
    })]
    .spacing(space::GAP);
    if let Some(e) = issue(&state.problems, "ai.provider") {
        picker = picker.push(error(Role::ROW_DESC, e));
    }
    let desc = match ai.provider {
        Provider::Browser => t!(ai_from_browser),
        _ => t!(ai_from_desc),
    };
    let mut rows = vec![setting_row::block(title(t!(ai_from)), desc, picker)];
    let Some(preset) = Preset::of(ai).filter(|_| keyed(state)) else {
        return rows; // no model, server or key: the browser, or nothing chosen yet
    };
    let saved = state.key.status == KeyStatus::Saved;
    match now {
        Some(1) => {
            rows.push(openrouter_card(state, saved));
            // Model row appears only once connected (key is never shown for OpenRouter).
            if saved && state.sign_in.is_none() {
                rows.push(model_row(state, preset));
            }
        }
        _ => {
            rows.push(own_preset_row(state));
            rows.extend(url_row(state));
            // a free tier's key page (A 1.7), while no key is saved
            let free = preset.free_key_page().filter(|_| !saved).map(|url| {
                crate::dir_row![
                    Space::new().width(Fill),
                    button(
                        label(t!(ai_key_free)),
                        Kind::Quiet,
                        Some(Message::OpenLink(url.into()))
                    )
                ]
                .into()
            });
            rows.push(model_row(state, preset));
            rows.push(key_row_with(state, free));
        }
    }
    rows
}

/// "Your own provider": the provider by name, without OpenRouter (its own card).
fn own_preset_row(state: &State) -> Element<'_, Message> {
    let preset = Preset::of(&state.draft.ai);
    setting_row::row(
        title(t!(ai_prov)),
        t!(ai_prov_desc),
        tip(
            dropdown(&OWN, preset, t!(ai_prov_choose), Message::Preset),
            t!(ai_provider),
            Position::Top,
        ),
    )
}

/// Formats the remaining OpenRouter browser sign-in wait (WAIT - elapsed) as M:SS.
pub(crate) fn sign_in_countdown(elapsed: std::time::Duration) -> String {
    let left = crate::features::ask_ai::sign_in::WAIT
        .saturating_sub(elapsed)
        .as_secs();
    format!("{}:{:02}", left / 60, left % 60)
}

/// Renders the 4-state OpenRouter connection card (not connected, waiting, connected, failed).
fn openrouter_card(state: &State, saved: bool) -> Element<'_, Message> {
    use crate::shared::kit::card::ConnectionState;
    use crate::shared::kit::card::connection_card;
    use crate::shared::kit::motion;
    use crate::shared::kit::spinner;
    let key_unavailable = match &state.key.status {
        KeyStatus::Unavailable(why) => Some(why.as_str()),
        _ => None,
    };
    let card_state = if state.sign_in.is_some() {
        let elapsed = state
            .sign_in_started
            .map_or(std::time::Duration::ZERO, |t| t.elapsed());
        ConnectionState::Waiting {
            title: t!(or_wait_title),
            desc: t!(or_wait_desc),
            status: t!(or_wait_status),
            countdown: sign_in_countdown(elapsed),
            turn: spinner::turn(elapsed),
            still: motion::reduced(),
            cancel: t!(c_cancel),
            on_cancel: Message::SignInCancel,
            reopen: t!(or_wait_again),
            on_reopen: Message::SignIn,
        }
    } else if saved {
        ConnectionState::Connected {
            title: t!(or_ok_title),
            saved: t!(or_ok_saved),
            desc: t!(or_ok_desc),
            error: state.key.problem(),
            test: t!(or_test),
            on_test: Message::LoadModels,
            disconnect: t!(or_disconnect),
            on_disconnect: Message::ApiKey(KeyMessage::Remove),
        }
    } else if let Some(detail) = state
        .sign_in_error
        .as_deref()
        .or(state.key.error.as_deref())
    {
        ConnectionState::Failed {
            title: t!(or_err_title),
            desc: t!(or_err_desc),
            detail,
            retry: t!(md_try_again),
            on_retry: Message::SignIn,
            own_link: t!(or_err_own),
            on_own: Message::Preset(OWN[0]),
        }
    } else {
        ConnectionState::NotConnected {
            title: t!(or_off_title),
            desc: t!(or_off_desc),
            cta: t!(or_off_cta),
            on_sign_in: key_unavailable.is_none().then_some(Message::SignIn),
            error: key_unavailable,
        }
    };
    connection_card(card_state)
}

/// The model: a list of the provider's models with a search; a typed id
/// that is in no list is offered as is (every provider, Custom server too).
fn model_row(state: &State, preset: Preset) -> Element<'_, Message> {
    let ai = &state.draft.ai;
    let example = match preset.model {
        "" => t!(ai_model_type).to_string(),
        m => crate::shared::app_language::fmt(crate::shared::app_language::S::ai_model_eg, &[&m]),
    };
    let on = model_list::On {
        opened: Message::ModelsOpened,
        query: Message::ModelQuery,
        first: Message::ModelFirst,
        pick: Message::ModelPick,
        load: Message::LoadModels,
    };
    let field = state.model_list.view(&ai.model, &example, on);
    let mut control = column![tip(field, t!(ai_model), Position::Top)].spacing(space::GAP);
    if let Some(e) = issue(&state.problems, "ai.model") {
        control = control.push(error(Role::ROW_DESC, e));
    }
    setting_row::row(title(t!(ai_model)), t!(ai_model_desc), control)
}

/// An AI provider with a model, a server and a key (not the browser, not an
/// unreadable value from the file).
pub fn keyed(state: &State) -> bool {
    !matches!(
        state.draft.ai.provider,
        Provider::Browser | Provider::Invalid(_) | Provider::Unreadable
    )
}

/// Browser | AI provider (Settings, and onboarding's AI step).
pub fn provider_row(state: &State) -> Element<'_, Message> {
    let ai = &state.draft.ai;
    let options = [
        (Choice::Browser, t!(ai_browser)),
        (Choice::Ai, t!(ai_provider)),
    ];
    // An unknown value from the file: none is chosen until a pick.
    let now = match ai.provider {
        Provider::Browser => Choice::Browser,
        Provider::Anthropic | Provider::OpenaiCompatible => Choice::Ai,
        Provider::Invalid(_) | Provider::Unreadable => Choice::None,
    };
    // AI provider from the browser: the first in the list; once chosen, a
    // press keeps the one picked
    let ai_pick = match now {
        Choice::Ai => Message::Provider(ai.provider.clone()),
        _ => Message::Preset(PRESETS[0]),
    };
    let mut provider = column![segmented(&options, now, now, 1.0, move |c: Choice| {
        match c {
            Choice::Ai => ai_pick.clone(),
            Choice::Browser | Choice::None => Message::Provider(Provider::Browser),
        }
    })]
    .spacing(space::GAP);
    if let Some(e) = issue(&state.problems, "ai.provider") {
        provider = provider.push(error(Role::ROW_DESC, e));
    }
    setting_row::row(
        title(t!(ai_from)),
        match ai.provider {
            Provider::Browser => t!(ai_from_browser),
            _ => t!(ai_from_desc),
        },
        provider,
    )
}

/// The AI provider, by name (Settings, and onboarding's AI step).
pub fn preset_row(state: &State) -> Element<'_, Message> {
    let preset = Preset::of(&state.draft.ai);
    setting_row::row(
        title(t!(ai_prov)),
        t!(ai_prov_desc),
        tip(
            dropdown(PRESETS, preset, t!(ai_prov_choose), Message::Preset),
            t!(ai_provider),
            Position::Top,
        ),
    )
}

/// A Custom server's URL; nothing for any other provider (Settings, and
/// onboarding's AI step).
pub fn url_row(state: &State) -> Option<Element<'_, Message>> {
    let ai = &state.draft.ai;
    Preset::of(ai).filter(Preset::custom)?;
    // #56: plain http on the local network works, and says what it costs
    let desc = match crate::features::ask_ai::lan_http(&ai.base_url) {
        true => t!(ai_url_lan),
        false => t!(ai_url_desc),
    };
    Some(text_row(
        state,
        (t!(ai_url), desc),
        (t!(ai_url_ph), &ai.base_url, "ai.base_url"),
        Message::BaseUrl,
    ))
}

/// The write-only key (Settings, and onboarding's AI step).
pub fn key_row(state: &State) -> Element<'_, Message> {
    key_row_with(state, None)
}

/// The key row with `extra` under the field (Settings: the free-key link).
fn key_row_with<'a>(state: &'a State, extra: Option<Element<'a, Message>>) -> Element<'a, Message> {
    let local = Preset::of(&state.draft.ai).is_some_and(|p| p.local);
    setting_row::row(
        title(t!(ai_key)),
        match local {
            true => t!(ai_key_local),
            false => t!(ai_key_desc),
        },
        column![state.key.view().map(Message::ApiKey)]
            .push(extra)
            .spacing(space::GAP),
    )
}

/// The Answers from segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Browser,
    Ai,
    None,
}

/// A row with a mono text field (its problem, if any, under it).
fn text_row<'a>(
    state: &'a State,
    (name, desc): (&'a str, &'a str),
    (placeholder, value, key): (&str, &'a str, &str),
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    let problem = issue(&state.problems, key);
    let field = input(placeholder, value, true, problem.is_some()).on_input(on_input);
    let mut control = column![tip(field, name, Position::Top)].spacing(space::GAP);
    if let Some(e) = problem {
        control = control.push(error(Role::ROW_DESC, e));
    }
    setting_row::row(title(name), desc, control)
}

impl KeyField {
    /// Shared keyring problem text: unavailable keyring or last save/remove error.
    pub(super) fn problem(&self) -> Option<&str> {
        if let Some(e) = &self.error {
            return Some(e.as_str());
        }
        match &self.status {
            KeyStatus::Unavailable(why) => Some(why.as_str()),
            _ => None,
        }
    }

    /// The write-only key: a secure field and Save (its hint "Paste your API
    /// key"); once saved, only dots (`KEY_MASK`) with Replace / Remove, never
    /// a character of the key (also in onboarding).
    pub fn view(&self) -> Element<'_, KeyMessage> {
        let l = self.labels;
        let wait = self.busy.map(Busy::label);
        let b = |s, m| button(label(s), Kind::Elevated, Some(m));
        let (field, buttons): (Element<'_, KeyMessage>, Vec<_>) =
            match (&self.status, self.replacing) {
                (KeyStatus::Unavailable(why), _) => {
                    (error(Role::ROW_DESC, why.clone()).into(), vec![])
                }
                (KeyStatus::Saved, false) => (
                    tip(
                        input("", KEY_MASK, false, false),
                        tr(l.field),
                        Position::Top,
                    ),
                    vec![
                        tip(
                            b(t!(ai_key_replace), KeyMessage::Replace),
                            tr(l.replace),
                            Position::Bottom,
                        ),
                        tip(
                            b(t!(ai_key_remove), KeyMessage::Remove),
                            tr(l.remove),
                            Position::Bottom,
                        ),
                    ],
                ),
                _ => {
                    let mut field = input(tr(l.placeholder), self.input.expose(), false, false)
                        .on_input(|s| KeyMessage::Input(Secret::new(s)))
                        .secure(true);
                    if wait.is_none() {
                        field = field.on_submit(KeyMessage::Save);
                    }
                    let save = b(t!(ai_key_save), KeyMessage::Save);
                    let mut buttons = vec![tip(save, tr(l.save), Position::Bottom)];
                    if self.replacing {
                        let keep = b(t!(c_cancel), KeyMessage::Cancel);
                        buttons.push(tip(keep, tr(l.keep), Position::Bottom));
                    }
                    (tip(field, tr(l.field), Position::Top), buttons)
                }
            };
        let buttons = if wait.is_none() { buttons } else { Vec::new() };
        let mut col = column![setting_row::field_with(field, buttons)].spacing(space::GAP);
        if let Some(w) = wait {
            col = col.push(crate::dir_row![
                Space::new().width(Fill),
                sub(Role::ROW_DESC, w)
            ]);
        }
        if let Some(e) = &self.error {
            col = col.push(error(Role::ROW_DESC, e.clone()));
        }
        col.into()
    }
}
