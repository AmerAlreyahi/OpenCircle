//! AI (`settings-window.js` "AI"): who answers (Browser | AI provider), and
//! for an AI provider its name (one dropdown of real services; the API each
//! speaks is chosen inside, never shown: owner, 2026-10-28), a Custom
//! server's URL, the model and the key (key + Save; mono where a value is
//! code). The design's sign-in, local-model and answer-style parts belong to
//! 0.2.0 and are not drawn.
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container, row};
use iced::{Element, Fill};

use super::super::state::{
    Busy, KEY_MASK, KeyField, KeyMessage, KeyStatus, Message, PRESETS, Preset, State, issue,
};
use crate::config::Provider;
use crate::secrets::Secret;
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::dropdown::dropdown;
use crate::ui::kit::field::input;
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::tokens::{Role, space};
use crate::ui::widgets::tip;

pub fn view(state: &State) -> Element<'_, Message> {
    let ai = &state.draft.ai;
    let mut rows = column![provider_row(state)];
    let Some(preset) = Preset::of(ai).filter(|_| keyed(state)) else {
        return rows.into(); // no model, server or key: the browser, or nothing chosen yet
    };
    rows = rows.push(preset_row(state)).push(url_row(state));
    let example = match preset.name {
        "OpenRouter" => "e.g. anthropic/claude-haiku-5.5 (fast, paid)".to_string(),
        _ if preset.custom() => "e.g. gpt-4o-mini".to_string(),
        _ => format!("e.g. {}", preset.model),
    };
    rows = rows.push(text_row(
        state,
        ("Model", "The model id sent to the provider"),
        (&example, &ai.model, "ai.model"),
        Message::Model,
    ));
    rows.push(key_row(state)).into()
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
    let options = [(Choice::Browser, "Browser"), (Choice::Ai, "AI provider")];
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
        title("Answers from"),
        match ai.provider {
            Provider::Browser => "Google AI Mode in your browser. No key",
            _ => "Who answers your questions",
        },
        provider,
    )
}

/// The AI provider, by name (Settings, and onboarding's AI step).
pub fn preset_row(state: &State) -> Element<'_, Message> {
    let preset = Preset::of(&state.draft.ai);
    setting_row::row(
        title("Provider"),
        "The service that answers, with your key",
        tip(
            dropdown(PRESETS, preset, "Choose a provider", Message::Preset),
            "AI provider",
            Position::Top,
        ),
    )
}

/// A Custom server's URL; nothing for any other provider (Settings, and
/// onboarding's AI step).
pub fn url_row(state: &State) -> Option<Element<'_, Message>> {
    let ai = &state.draft.ai;
    Preset::of(ai).filter(Preset::custom)?;
    Some(text_row(
        state,
        ("Server URL", "Your own server or any compatible API"),
        ("https://my-server.example/v1", &ai.base_url, "ai.base_url"),
        Message::BaseUrl,
    ))
}

/// The write-only key (Settings, and onboarding's AI step).
pub fn key_row(state: &State) -> Element<'_, Message> {
    let local = Preset::of(&state.draft.ai).is_some_and(|p| p.local);
    setting_row::row(
        title("API key"),
        match local {
            true => "Not needed for a server on this PC",
            false => "Kept in your system keyring, never in a file",
        },
        state.key.view().map(Message::ApiKey),
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
                    tip(input("", KEY_MASK, false, false), l.field, Position::Top),
                    vec![
                        tip(
                            b("Replace", KeyMessage::Replace),
                            l.replace,
                            Position::Bottom,
                        ),
                        tip(b("Remove", KeyMessage::Remove), l.remove, Position::Bottom),
                    ],
                ),
                _ => {
                    let mut field = input(l.placeholder, self.input.expose(), false, false)
                        .on_input(|s| KeyMessage::Input(Secret::new(s)))
                        .secure(true);
                    if wait.is_none() {
                        field = field.on_submit(KeyMessage::Save);
                    }
                    let save = b("Save key", KeyMessage::Save);
                    let mut buttons = vec![tip(save, l.save, Position::Bottom)];
                    if self.replacing {
                        let keep = b("Cancel", KeyMessage::Cancel);
                        buttons.push(tip(keep, l.keep, Position::Bottom));
                    }
                    (tip(field, l.field, Position::Top), buttons)
                }
            };
        let mut line = row![container(field).width(Fill)]
            .spacing(space::GAP)
            .align_y(iced::Center);
        if wait.is_none() {
            line = line.extend(buttons);
        }
        let mut col = column![line].spacing(space::GAP);
        if let Some(w) = wait {
            col = col.push(row![Space::new().width(Fill), sub(Role::ROW_DESC, w)]);
        }
        if let Some(e) = &self.error {
            col = col.push(error(Role::ROW_DESC, e.clone()));
        }
        col.into()
    }
}
