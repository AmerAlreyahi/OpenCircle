//! AI (`settings-window.js` "AI"): who answers, and for a provider of your
//! own its server, model and key (the design's "Use your own key" fields:
//! key + Save, model; mono where a value is code). The design's sign-in,
//! local-model and answer-style parts belong to 0.2.0 and are not drawn.
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container, row};
use iced::{Element, Fill};

use super::super::state::{Busy, KeyField, KeyMessage, KeyStatus, Message, PRESETS, State, issue};
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

/// A URL, model or key field (the design's `w-[260px]`).
pub(super) const FIELD_WIDTH: f32 = 260.0;
/// The key field with its buttons beside it.
const KEY_WIDTH: f32 = 340.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let ai = &state.draft.ai;
    let compat = ai.provider == Provider::OpenaiCompatible;
    let options = [
        (Choice::Browser, "Browser"),
        (Choice::Anthropic, "Anthropic"),
        (Choice::Compatible, "OpenAI-like"),
    ];
    // An unknown value from the file: none is chosen until a pick.
    let now = Choice::of(&ai.provider);
    let mut provider = column![segmented(&options, now, now, 1.0, |c: Choice| {
        Message::Provider(c.provider())
    })]
    .spacing(space::GAP)
    .align_x(iced::Right);
    if let Some(e) = issue(&state.problems, "ai.provider") {
        provider = provider.push(error(Role::ROW_DESC, e));
    }
    let mut rows = column![setting_row::wide(
        title("Provider"),
        match ai.provider {
            Provider::Browser => "Google AI Mode in your browser. No key",
            _ => "Who answers your questions",
        },
        provider,
    )];
    if matches!(
        ai.provider,
        Provider::Browser | Provider::Invalid(_) | Provider::Unreadable
    ) {
        return rows.into(); // no model, server or key: the browser, or nothing chosen yet
    }
    if compat {
        let preset = PRESETS.iter().find(|p| p.base_url == ai.base_url).copied();
        rows = rows.push(setting_row::row(
            title("Preset"),
            "Fills in the server URL",
            tip(
                dropdown(PRESETS, preset, "Choose a preset", Message::Preset),
                "Server preset",
                Position::Top,
            ),
        ));
        rows = rows.push(text_row(
            state,
            ("Server URL", "Your own server or any compatible API"),
            ("https://api.openai.com/v1", &ai.base_url, "ai.base_url"),
            Message::BaseUrl,
        ));
    }
    let example = match () {
        _ if compat && ai.base_url.contains("openrouter.ai") => {
            "e.g. anthropic/claude-haiku-5.5 (fast, paid)"
        }
        _ if compat => "e.g. gpt-4o-mini",
        _ => "claude-sonnet-5-5",
    };
    rows = rows.push(text_row(
        state,
        ("Model", "The model id sent to the provider"),
        (example, &ai.model, "ai.model"),
        Message::Model,
    ));
    let local = PRESETS.iter().any(|p| p.local && p.base_url == ai.base_url);
    rows = rows.push(setting_row::row(
        title("API key"),
        match local {
            true => "Not needed for a server on this PC",
            false => "Kept in your system keyring, never in a file",
        },
        state.key.view().map(Message::ApiKey),
    ));
    rows.into()
}

/// The provider segments (`Provider` holds data: no `Copy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Browser,
    Anthropic,
    Compatible,
    None,
}

impl Choice {
    fn of(p: &Provider) -> Self {
        match p {
            Provider::Browser => Choice::Browser,
            Provider::Anthropic => Choice::Anthropic,
            Provider::OpenaiCompatible => Choice::Compatible,
            Provider::Invalid(_) | Provider::Unreadable => Choice::None,
        }
    }

    fn provider(self) -> Provider {
        match self {
            Choice::Browser | Choice::None => Provider::Browser,
            Choice::Anthropic => Provider::Anthropic,
            Choice::Compatible => Provider::OpenaiCompatible,
        }
    }
}

/// A row with a mono text field (its problem, if any, under it).
fn text_row<'a>(
    state: &'a State,
    (name, desc): (&'a str, &'a str),
    (placeholder, value, key): (&'a str, &'a str, &str),
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    let problem = issue(&state.problems, key);
    let field = input(placeholder, value, true, problem.is_some())
        .on_input(on_input)
        .width(FIELD_WIDTH);
    let mut control = column![tip(field, name, Position::Top)]
        .spacing(space::GAP)
        .width(FIELD_WIDTH);
    if let Some(e) = problem {
        control = control.push(error(Role::ROW_DESC, e));
    }
    setting_row::row(title(name), desc, control)
}

impl KeyField {
    /// The write-only key: a secure field and Save; once saved, only "Saved in
    /// your keyring" with Replace / Remove (also in onboarding).
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
                    input("", "Saved in your keyring", false, false).into(),
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
        let mut col = column![line].spacing(space::GAP).width(KEY_WIDTH);
        if let Some(w) = wait {
            col = col.push(row![Space::new().width(Fill), sub(Role::ROW_DESC, w)]);
        }
        if let Some(e) = &self.error {
            col = col.push(error(Role::ROW_DESC, e.clone()));
        }
        col.into()
    }
}
