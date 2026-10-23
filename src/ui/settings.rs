//! The Settings window as a self-contained component (its own `State`,
//! `Message`, `update`, `view`, `subscription`); the UI shell opens it in a window.
//!
//! ```ignore
//! let mut state = settings::State::open();            // loads the real config + keyring
//! // window size: settings::WINDOW_SIZE
//! // update: if let settings::Outcome::Saved(cfg) = settings::update(&mut state, msg) { ...rebind shortcuts... }
//! // view: settings::view(&state);  subscription: settings::subscription(&state)
//! ```
//!
//! Rules kept here: the draft is validated by `Config`'s own reader (so Save can
//! never write what `load` would reject), shortcuts are recorded by key press and
//! checked with `config::parse_shortcut`, and the API key is write-only: it goes
//! to the OS keyring through `KeyStore`, is never shown, and is never part of any
//! `Debug` output (`Secret` is redacted).

use std::fmt;
use std::path::PathBuf;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, row, space, text};
use iced::{Element, Length, Size, Subscription, event, keyboard};

use crate::config::{Config, Engine, Provider, Target, Theme, parse_shortcut};
use crate::secrets::{self, Secret, SecretsError};
use crate::ui::theme;
use crate::ui::widgets::{
    self, CONTROL, FORM_WIDTH, FieldState, caption, divider, error_text, form_row, label_button,
    nav_item, segmented, shortcut_field, text_field, tip, title, vertical_divider,
};

/// Logical size of the Settings window.
pub const WINDOW_SIZE: Size = Size::new(820.0, 600.0);

// ---- the OS keyring, behind a trait so tests never touch the real one ----

pub trait KeyStore {
    fn is_set(&self, provider: Provider) -> Result<bool, SecretsError>;
    fn set(&self, provider: Provider, key: &Secret) -> Result<(), SecretsError>;
    fn delete(&self, provider: Provider) -> Result<(), SecretsError>;
}

/// The real thing: `secrets` (Windows Credential Manager / Secret Service).
pub struct Keyring;

impl KeyStore for Keyring {
    fn is_set(&self, provider: Provider) -> Result<bool, SecretsError> {
        secrets::get(provider).map(|k| k.is_some())
    }
    fn set(&self, provider: Provider, key: &Secret) -> Result<(), SecretsError> {
        secrets::set(provider, key)
    }
    fn delete(&self, provider: Provider) -> Result<(), SecretsError> {
        secrets::delete(provider)
    }
}

// ---- the write-only API key control (also used by onboarding) ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStatus {
    Saved,
    NotSet,
    /// No usable keyring: the message says what to install or unlock.
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub enum KeyMessage {
    /// What the user typed (a `Secret`, so no `Debug` can print it).
    Input(Secret),
    Save,
    Replace,
    Cancel,
    Remove,
}

#[derive(Debug)]
pub struct KeyField {
    status: KeyStatus,
    input: Secret,
    replacing: bool,
    error: Option<String>,
}

impl KeyField {
    pub fn new(store: &dyn KeyStore, provider: Provider) -> Self {
        let mut f = KeyField {
            status: KeyStatus::NotSet,
            input: Secret::new(""),
            replacing: false,
            error: None,
        };
        f.refresh(store, provider);
        f
    }

    /// Ask the keyring again (after the provider changed).
    pub fn refresh(&mut self, store: &dyn KeyStore, provider: Provider) {
        self.input = Secret::new("");
        self.replacing = false;
        self.error = None;
        self.status = match store.is_set(provider) {
            Ok(true) => KeyStatus::Saved,
            Ok(false) => KeyStatus::NotSet,
            Err(e) => KeyStatus::Unavailable(e.to_string()),
        };
    }

    pub fn status(&self) -> &KeyStatus {
        &self.status
    }

    pub fn update(&mut self, store: &dyn KeyStore, provider: Provider, message: KeyMessage) {
        self.error = None;
        match message {
            KeyMessage::Input(key) => self.input = key,
            KeyMessage::Replace => self.replacing = true,
            KeyMessage::Cancel => {
                self.replacing = false;
                self.input = Secret::new("");
            }
            KeyMessage::Save if self.input.is_empty() => {}
            KeyMessage::Save => match store.set(provider, &self.input) {
                Ok(()) => {
                    self.status = KeyStatus::Saved;
                    self.replacing = false;
                    self.input = Secret::new("");
                }
                Err(e) => self.error = Some(e.to_string()),
            },
            KeyMessage::Remove => match store.delete(provider) {
                Ok(()) => self.status = KeyStatus::NotSet,
                Err(e) => self.error = Some(e.to_string()),
            },
        }
    }

    pub fn view(&self) -> Element<'_, KeyMessage> {
        let palette = theme::current();
        let mut col = column![].spacing(8).width(FORM_WIDTH);
        match (&self.status, self.replacing) {
            (KeyStatus::Unavailable(why), _) => col = col.push(error_text(why.as_str())),
            (KeyStatus::Saved, false) => {
                let saved = container(text("Saved in your keyring").size(14).color(palette.muted))
                    .padding([0, 14])
                    .center_y(CONTROL)
                    .width(FORM_WIDTH)
                    .style(|_| container::Style {
                        background: Some(theme::current().field.into()),
                        border: iced::Border::default().rounded(14),
                        ..container::Style::default()
                    });
                let replace = label_button("Replace", false, Some(KeyMessage::Replace));
                let remove = label_button("Remove", false, Some(KeyMessage::Remove));
                col = col.push(saved).push(
                    row![
                        tip(replace, "Replace the API key", Position::Bottom),
                        tip(remove, "Remove the API key", Position::Bottom),
                    ]
                    .spacing(8),
                );
            }
            _ => {
                let field = text_field("Paste your API key", self.input.expose(), false, |s| {
                    KeyMessage::Input(Secret::new(s))
                })
                .secure(true)
                .on_submit(KeyMessage::Save);
                let save = label_button(
                    "Save key",
                    true,
                    (!self.input.is_empty()).then_some(KeyMessage::Save),
                );
                let mut buttons = row![tip(save, "Save the API key", Position::Bottom)].spacing(8);
                if self.replacing {
                    let cancel = label_button("Cancel", false, Some(KeyMessage::Cancel));
                    buttons = buttons.push(tip(cancel, "Keep the saved API key", Position::Bottom));
                }
                col = col.push(tip(field, "API key", Position::Top)).push(buttons);
            }
        }
        if let Some(e) = &self.error {
            col = col.push(error_text(e.as_str()));
        }
        col.into()
    }
}

// ---- data: presets, languages, action names ----

/// A ready-made OpenAI-compatible server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub base_url: &'static str,
    /// Suggested model, used only while the model field is empty.
    pub model: &'static str,
    /// Runs on this machine: no API key needed.
    pub local: bool,
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        model: "gpt-4o-mini",
        local: false,
    },
    Preset {
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        model: "openrouter/auto",
        local: false,
    },
    Preset {
        name: "Gemini (OpenAI endpoint)",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-2.0-flash",
        local: false,
    },
    Preset {
        name: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        model: "llama3.2-vision",
        local: true,
    },
    Preset {
        name: "LM Studio (local)",
        base_url: "http://localhost:1234/v1",
        model: "local-model",
        local: true,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const fn lang(code: &'static str, name: &'static str) -> Language {
    Language { code, name }
}

pub const LANGUAGES: &[Language] = &[
    lang("ar", "Arabic"),
    lang("en", "English"),
    lang("fr", "French"),
    lang("de", "German"),
    lang("es", "Spanish"),
    lang("pt", "Portuguese"),
    lang("pt-BR", "Portuguese (Brazil)"),
    lang("it", "Italian"),
    lang("nl", "Dutch"),
    lang("ru", "Russian"),
    lang("uk", "Ukrainian"),
    lang("pl", "Polish"),
    lang("tr", "Turkish"),
    lang("fa", "Persian"),
    lang("ur", "Urdu"),
    lang("he", "Hebrew"),
    lang("hi", "Hindi"),
    lang("id", "Indonesian"),
    lang("vi", "Vietnamese"),
    lang("th", "Thai"),
    lang("ja", "Japanese"),
    lang("ko", "Korean"),
    lang("zh", "Chinese (Simplified)"),
    lang("zh-Hant", "Chinese (Traditional)"),
];

fn action_name(id: &str, live: bool) -> String {
    match (id, live) {
        ("translate", false) => "Translate".into(),
        ("translate", true) => "Translate (live)".into(),
        ("ai_explain", _) => "Explain with AI".into(),
        ("copy_text", _) => "Copy text".into(),
        ("lens", _) => "Search with Lens".into(),
        (other, _) => {
            let spaced = other.replace('_', " ");
            let mut chars = spaced.chars();
            chars
                .next()
                .map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
        }
    }
}

fn target_name(t: Target) -> &'static str {
    match t {
        Target::FullScreen => "Whole screen",
        Target::Circle => "The area you circle",
        Target::LastRegion => "The last region",
    }
}

// ---- shortcut capture ----

/// What a key press looked like (built from the iced event; plain data for tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
    /// Physical key name, e.g. "KeyT", "Space", "F9", "ControlLeft".
    pub key: String,
}

impl Chord {
    fn from_iced(
        physical: keyboard::key::Physical,
        modifiers: keyboard::Modifiers,
    ) -> Option<Self> {
        let keyboard::key::Physical::Code(code) = physical else {
            return None;
        };
        Some(Chord {
            ctrl: modifiers.control(),
            alt: modifiers.alt(),
            shift: modifiers.shift(),
            logo: modifiers.logo(),
            key: format!("{code:?}"),
        })
    }

    fn bare(&self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.logo)
    }

    /// "Ctrl+Shift+T" for `parse_shortcut`; `None` for a lone modifier key.
    fn shortcut(&self) -> Option<String> {
        let key = self.key.as_str();
        let modifier = ["Control", "Shift", "Alt", "Meta", "Super", "Fn", "CapsLock"]
            .iter()
            .any(|m| key.starts_with(m));
        if modifier {
            return None;
        }
        let single = |prefix: &str| key.strip_prefix(prefix).filter(|r| r.len() == 1);
        let key = single("Key").or(single("Digit")).unwrap_or(key);
        let mut parts = Vec::new();
        for (on, name) in [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.logo, "Super"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(key);
        Some(parts.join("+"))
    }
}

/// One keycap label per key of a shortcut string ("Ctrl+Shift+T" -> Ctrl, Shift, T).
pub fn key_labels(shortcut: &str) -> Vec<String> {
    shortcut
        .split('+')
        .filter(|k| !k.is_empty())
        .map(|k| {
            let single = |prefix: &str| k.strip_prefix(prefix).filter(|r| r.len() == 1);
            match k {
                "PrintScreen" => "PrtSc".into(),
                "Escape" => "Esc".into(),
                _ => {
                    let k = single("Key").or(single("Digit")).unwrap_or(k);
                    let mut c = k.chars();
                    c.next()
                        .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
                }
            }
        })
        .collect()
}

// ---- state ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Shortcuts,
    Ai,
    Translate,
}

const SECTIONS: [(Section, &str, &str); 4] = [
    (Section::Appearance, "Appearance", "Appearance settings"),
    (Section::Shortcuts, "Shortcuts", "Shortcut settings"),
    (Section::Ai, "AI", "AI settings"),
    (Section::Translate, "Translate", "Translate settings"),
];

/// A shortcut field: the capture shortcut or quick action `n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Capture,
    Quick(usize),
}

struct SlotText {
    label: String,
    hint: String,
    tip: String,
}

pub struct State {
    section: Section,
    draft: Config,
    saved: Config,
    /// `None` = the real config path.
    path: Option<PathBuf>,
    store: Box<dyn KeyStore>,
    recording: Option<Slot>,
    shortcut_error: Option<(Slot, String)>,
    key: KeyField,
    quick_text: Vec<SlotText>,
    /// "key: reason" lines from `Config`'s own validation of the draft.
    problems: Vec<String>,
    notice: Option<String>,
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("settings::State")
            .field("section", &self.section)
            .field("draft", &self.draft)
            .field("recording", &self.recording)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Section(Section),
    Theme(Theme),
    Record(Slot),
    Key(Chord),
    Provider(Provider),
    Preset(Preset),
    BaseUrl(String),
    Model(String),
    ApiKey(KeyMessage),
    Language(Language),
    Engine(Engine),
    Save,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    None,
    /// Written to disk: re-bind the shortcuts, re-read the theme.
    Saved(Config),
}

impl State {
    /// The real config file and the real keyring.
    pub fn open() -> Self {
        let (config, warnings) = Config::load();
        Self::new(config, warnings, None, Box::new(Keyring))
    }

    /// `path: None` = `Config::path()`. `warnings` come from `Config::load`.
    pub fn new(
        config: Config,
        warnings: Vec<String>,
        path: Option<PathBuf>,
        store: Box<dyn KeyStore>,
    ) -> Self {
        theme::apply(config.theme);
        let key = KeyField::new(&*store, config.ai.provider);
        let quick_text = config
            .quick_actions
            .iter()
            .map(|q| {
                let label = action_name(&q.action, q.live);
                SlotText {
                    hint: target_name(q.target).to_string(),
                    tip: format!("{label} shortcut"),
                    label,
                }
            })
            .collect();
        let mut s = State {
            section: Section::Appearance,
            saved: config.clone(),
            draft: config,
            path,
            store,
            recording: None,
            shortcut_error: None,
            key,
            quick_text,
            problems: Vec::new(),
            notice: warnings
                .first()
                .map(|w| format!("Some saved settings were invalid and were reset ({w})")),
        };
        s.revalidate();
        s
    }

    fn dirty(&self) -> bool {
        self.draft != self.saved
    }

    fn revalidate(&mut self) {
        self.problems = problems(&self.draft, self.key.status == KeyStatus::Saved);
    }

    fn label(&self, slot: Slot) -> &str {
        match slot {
            Slot::Capture => "Open OpenCircle",
            Slot::Quick(i) => &self.quick_text[i].label,
        }
    }

    fn shortcut(&self, slot: Slot) -> &str {
        match slot {
            Slot::Capture => &self.draft.shortcut,
            Slot::Quick(i) => &self.draft.quick_actions[i].shortcut,
        }
    }

    fn set_shortcut(&mut self, slot: Slot, value: String) {
        match slot {
            Slot::Capture => self.draft.shortcut = value,
            Slot::Quick(i) => self.draft.quick_actions[i].shortcut = value,
        }
    }

    fn slots(&self) -> impl Iterator<Item = Slot> + use<> {
        std::iter::once(Slot::Capture).chain((0..self.draft.quick_actions.len()).map(Slot::Quick))
    }

    /// Why `chord` cannot be `slot`'s shortcut, or the shortcut string to store.
    fn check_chord(&self, slot: Slot, chord: &Chord) -> Result<String, String> {
        let Some(shortcut) = chord.shortcut() else {
            return Err(String::new()); // lone modifier: keep waiting
        };
        let hotkey = parse_shortcut(&shortcut)?;
        for other in self.slots().filter(|&o| o != slot) {
            if parse_shortcut(self.shortcut(other)).is_ok_and(|h| h == hotkey) {
                return Err(format!("Already used by {}", self.label(other)));
            }
        }
        Ok(shortcut)
    }

    fn on_chord(&mut self, slot: Slot, chord: Chord) {
        if chord.bare() && chord.key == "Escape" {
            self.recording = None;
            self.shortcut_error = None;
            return;
        }
        if chord.bare() && matches!(chord.key.as_str(), "Delete" | "Backspace") {
            if matches!(slot, Slot::Quick(_)) {
                self.set_shortcut(slot, String::new());
                self.recording = None;
                self.shortcut_error = None;
            }
            return;
        }
        match self.check_chord(slot, &chord) {
            Ok(shortcut) => {
                self.set_shortcut(slot, shortcut);
                self.recording = None;
                self.shortcut_error = None;
            }
            Err(why) if why.is_empty() => {}
            Err(why) => self.shortcut_error = Some((slot, why)),
        }
    }
}

/// Everything wrong with `cfg`, as "key: reason" lines. The shared validator is
/// `Config::parse`, run over exactly what `save` would write.
fn problems(cfg: &Config, key_saved: bool) -> Vec<String> {
    let mut out = match toml::to_string_pretty(cfg) {
        Ok(text) => Config::parse(&text)
            .1
            .into_iter()
            .map(|w| w.split("; ").next().unwrap_or_default().to_string())
            .collect(),
        Err(e) => vec![format!("config: {e}")],
    };
    if cfg.ai.model.is_empty() {
        out.push("ai.model: choose a model".into());
    }
    if cfg.ai.provider == Provider::OpenaiCompatible {
        if cfg.ai.base_url.is_empty() {
            out.push("ai.base_url: enter your server's URL".into());
        } else if let Err(crate::ai::AiError::NotConfigured(why)) =
            crate::ai::check_endpoint(&cfg.ai.base_url, key_saved)
            && !out.iter().any(|p| p.starts_with("ai.base_url"))
        {
            out.push(format!("ai.base_url: {why}"));
        }
    }
    out
}

/// The reason for `key` from a list of "key: reason" problems.
fn issue(problems: &[String], key: &str) -> Option<String> {
    problems
        .iter()
        .find_map(|p| p.strip_prefix(key)?.strip_prefix(": "))
        .map(|s| {
            let mut c = s.chars();
            c.next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
}

pub fn update(state: &mut State, message: Message) -> Outcome {
    state.notice = None;
    match message {
        Message::Section(s) => {
            state.section = s;
            state.recording = None;
            state.shortcut_error = None;
        }
        Message::Theme(t) => {
            state.draft.theme = t;
            theme::apply(t);
        }
        Message::Record(slot) => {
            state.recording = (state.recording != Some(slot)).then_some(slot);
            state.shortcut_error = None;
        }
        Message::Key(chord) => {
            if let Some(slot) = state.recording {
                state.on_chord(slot, chord);
            }
        }
        Message::Provider(p) if p != state.draft.ai.provider => {
            let ai = &mut state.draft.ai;
            let default_model = crate::ai::DEFAULT_ANTHROPIC_MODEL;
            ai.provider = p;
            match p {
                Provider::Anthropic => {
                    ai.base_url.clear();
                    if ai.model.is_empty() || PRESETS.iter().any(|x| x.model == ai.model) {
                        ai.model = default_model.to_string();
                    }
                }
                Provider::OpenaiCompatible if ai.model == default_model => ai.model.clear(),
                Provider::OpenaiCompatible => {}
            }
            state.key.refresh(&*state.store, p);
        }
        Message::Provider(_) => {}
        Message::Preset(p) => {
            state.draft.ai.base_url = p.base_url.to_string();
            if state.draft.ai.model.is_empty() {
                state.draft.ai.model = p.model.to_string();
            }
        }
        Message::BaseUrl(s) => state.draft.ai.base_url = s.trim().to_string(),
        Message::Model(s) => state.draft.ai.model = s.trim().to_string(),
        Message::ApiKey(k) => {
            let provider = state.draft.ai.provider;
            state.key.update(&*state.store, provider, k);
        }
        Message::Language(l) => state.draft.translate.target = l.code.to_string(),
        Message::Engine(e) => state.draft.translate.engine = e,
        Message::Save => return save(state),
    }
    state.revalidate();
    Outcome::None
}

fn save(state: &mut State) -> Outcome {
    if !state.problems.is_empty() {
        state.notice = Some("Fix the problems above first".into());
        return Outcome::None;
    }
    let result = match &state.path {
        Some(p) => state.draft.save_to(p),
        None => state.draft.save(),
    };
    match result {
        Ok(()) => {
            state.saved = state.draft.clone();
            state.notice = Some("Saved".into());
            Outcome::Saved(state.saved.clone())
        }
        Err(e) => {
            state.notice = Some(format!("Could not save: {e}"));
            Outcome::None
        }
    }
}

/// Key presses, only while a shortcut field is recording.
pub fn subscription(state: &State) -> Subscription<Message> {
    if state.recording.is_none() {
        return Subscription::none();
    }
    event::listen_with(|event, _status, _window| match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            physical_key,
            modifiers,
            ..
        }) => Chord::from_iced(physical_key, modifiers).map(Message::Key),
        _ => None,
    })
}

// ---- view ----

pub fn view(state: &State) -> Element<'_, Message> {
    let sidebar = column(SECTIONS.iter().map(|&(section, label, hint)| {
        tip(
            nav_item(label, section == state.section, Message::Section(section)),
            hint,
            Position::Right,
        )
    }))
    .spacing(4)
    .padding(16)
    .width(200);

    let main = column![
        container(content(state))
            .padding([24, 32])
            .width(Length::Fill)
            .height(Length::Fill),
        divider(),
        footer(state),
    ];
    widgets::window(row![sidebar, vertical_divider(), main]).into()
}

fn content(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let (heading, rows): (&str, Vec<Element<'_, Message>>) = match state.section {
        Section::Appearance => (
            "Appearance",
            vec![form_row(
                "Theme",
                "System follows your OS",
                segmented(
                    &[
                        (Theme::System, "System", "Follow the system theme"),
                        (Theme::Light, "Light", "Use the light theme"),
                        (Theme::Dark, "Dark", "Use the dark theme"),
                    ],
                    d.theme,
                    Message::Theme,
                ),
                None,
            )],
        ),
        Section::Shortcuts => ("Shortcuts", shortcut_rows(state)),
        Section::Ai => ("AI", ai_rows(state)),
        Section::Translate => (
            "Translate",
            vec![
                form_row(
                    "Target language",
                    "What the text is translated into",
                    tip(
                        widgets::dropdown(
                            LANGUAGES,
                            LANGUAGES
                                .iter()
                                .find(|l| l.code == d.translate.target)
                                .copied(),
                            &d.translate.target,
                            Message::Language,
                        ),
                        "Target language",
                        Position::Top,
                    ),
                    issue(&state.problems, "translate.target"),
                ),
                form_row(
                    "Engine",
                    "AI uses your provider; Offline stays on this PC",
                    segmented(
                        &[
                            (Engine::Ai, "AI", "Translate with your AI provider"),
                            (Engine::Offline, "Offline", "Translate on this device only"),
                        ],
                        d.translate.engine,
                        Message::Engine,
                    ),
                    None,
                ),
            ],
        ),
    };
    let mut col = column![title(heading), space::vertical().height(8)];
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            col = col.push(divider());
        }
        col = col.push(r);
    }
    col.into()
}

fn shortcut_rows(state: &State) -> Vec<Element<'_, Message>> {
    // ponytail: no scrolling; ~7 quick actions fit the window. Add a scrollable
    // (themed in widgets.rs) when users add more.
    state
        .slots()
        .map(|slot| {
            let (label, hint, tip_text) = match slot {
                Slot::Capture => (
                    "Open OpenCircle",
                    "Starts a capture",
                    "Open OpenCircle shortcut",
                ),
                Slot::Quick(i) => {
                    let t = &state.quick_text[i];
                    (t.label.as_str(), t.hint.as_str(), t.tip.as_str())
                }
            };
            let error = state
                .shortcut_error
                .as_ref()
                .filter(|(s, _)| *s == slot)
                .map(|(_, e)| e.clone());
            let recording = state.recording == Some(slot);
            let field_state = match (recording, &error) {
                (_, Some(_)) => FieldState::Invalid,
                (true, None) => FieldState::Recording,
                _ => FieldState::Idle,
            };
            let hint = if recording {
                match slot {
                    Slot::Capture => "Esc cancels",
                    Slot::Quick(_) => "Esc cancels · Delete clears",
                }
            } else {
                hint
            };
            let field = shortcut_field(
                key_labels(state.shortcut(slot)),
                field_state,
                "Press keys…",
                Message::Record(slot),
            );
            form_row(label, hint, tip(field, tip_text, Position::Top), error)
        })
        .collect()
}

fn ai_rows(state: &State) -> Vec<Element<'_, Message>> {
    let ai = &state.draft.ai;
    let compat = ai.provider == Provider::OpenaiCompatible;
    let mut rows = vec![form_row(
        "Provider",
        "Who answers your questions",
        segmented(
            &[
                (Provider::Anthropic, "Anthropic", "Use Anthropic (Claude)"),
                (
                    Provider::OpenaiCompatible,
                    "OpenAI-compatible",
                    "Use an OpenAI-compatible server",
                ),
            ],
            ai.provider,
            Message::Provider,
        ),
        None,
    )];
    if compat {
        rows.push(form_row(
            "Preset",
            "Fills in the server URL",
            tip(
                widgets::dropdown(
                    PRESETS,
                    PRESETS.iter().find(|p| p.base_url == ai.base_url).copied(),
                    "Choose a preset",
                    Message::Preset,
                ),
                "Server preset",
                Position::Top,
            ),
            None,
        ));
        let error = issue(&state.problems, "ai.base_url");
        rows.push(form_row(
            "Server URL",
            "Your own server or any compatible API",
            tip(
                text_field(
                    "https://api.openai.com/v1",
                    &ai.base_url,
                    error.is_some(),
                    Message::BaseUrl,
                ),
                "Server URL",
                Position::Top,
            ),
            error,
        ));
    }
    let error = issue(&state.problems, "ai.model");
    rows.push(form_row(
        "Model",
        "The model id sent to the provider",
        tip(
            text_field(
                if compat {
                    "e.g. gpt-4o-mini"
                } else {
                    "claude-sonnet-5-5"
                },
                &ai.model,
                error.is_some(),
                Message::Model,
            ),
            "Model name",
            Position::Top,
        ),
        error,
    ));
    let local = PRESETS.iter().any(|p| p.local && p.base_url == ai.base_url);
    rows.push(form_row(
        "API key",
        if local {
            "Not needed for a server on this PC"
        } else {
            "Kept in your system keyring, never in a file"
        },
        state.key.view().map(Message::ApiKey),
        None,
    ));
    rows
}

fn footer(state: &State) -> Element<'_, Message> {
    let n = state.problems.len();
    let status: Element<'_, Message> = if n > 0 {
        error_text(format!(
            "{n} {} to fix before saving",
            if n == 1 { "problem" } else { "problems" }
        ))
        .size(13)
        .into()
    } else if let Some(notice) = &state.notice {
        caption(notice.as_str()).size(13).into()
    } else if state.dirty() {
        caption("Unsaved changes").size(13).into()
    } else {
        space::horizontal().into()
    };
    let can_save = n == 0 && state.dirty();
    let save = label_button("Save", true, can_save.then_some(Message::Save));
    row![
        status,
        space::horizontal(),
        tip(save, "Save settings", Position::Top)
    ]
    .align_y(iced::Center)
    .padding([14, 32])
    .into()
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::ui::widgets::{TIPS, shot};

    /// In-memory keyring.
    #[derive(Default)]
    pub struct MemoryStore(pub RefCell<HashMap<&'static str, String>>);

    impl KeyStore for MemoryStore {
        fn is_set(&self, p: Provider) -> Result<bool, SecretsError> {
            Ok(self.0.borrow().contains_key(p.id()))
        }
        fn set(&self, p: Provider, key: &Secret) -> Result<(), SecretsError> {
            self.0.borrow_mut().insert(p.id(), key.expose().to_string());
            Ok(())
        }
        fn delete(&self, p: Provider) -> Result<(), SecretsError> {
            self.0.borrow_mut().remove(p.id());
            Ok(())
        }
    }

    /// Shares a `MemoryStore` between the test and the state.
    struct Shared(std::rc::Rc<MemoryStore>);
    impl KeyStore for Shared {
        fn is_set(&self, p: Provider) -> Result<bool, SecretsError> {
            self.0.is_set(p)
        }
        fn set(&self, p: Provider, k: &Secret) -> Result<(), SecretsError> {
            self.0.set(p, k)
        }
        fn delete(&self, p: Provider) -> Result<(), SecretsError> {
            self.0.delete(p)
        }
    }

    pub fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oc-settings-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("config.toml")
    }

    fn state(path: Option<PathBuf>) -> (State, std::rc::Rc<MemoryStore>) {
        let store = std::rc::Rc::new(MemoryStore::default());
        let s = State::new(
            Config::default(),
            vec![],
            path,
            Box::new(Shared(store.clone())),
        );
        (s, store)
    }

    fn chord(ctrl: bool, shift: bool, key: &str) -> Chord {
        Chord {
            ctrl,
            alt: false,
            shift,
            logo: false,
            key: key.into(),
        }
    }

    fn press(s: &mut State, c: Chord) {
        update(s, Message::Key(c));
    }

    #[test]
    fn recording_validates_and_assigns() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Record(Slot::Quick(2)));
        assert_eq!(s.recording, Some(Slot::Quick(2)));
        // A lone modifier keeps waiting; nothing changes.
        press(&mut s, chord(true, false, "ControlLeft"));
        assert_eq!(s.recording, Some(Slot::Quick(2)));
        assert!(s.shortcut_error.is_none());
        press(&mut s, chord(true, true, "KeyK"));
        assert_eq!(s.draft.quick_actions[2].shortcut, "Ctrl+Shift+K");
        assert_eq!(s.recording, None);
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        // Digits and F-keys.
        update(&mut s, Message::Record(Slot::Capture));
        press(&mut s, chord(true, false, "Digit1"));
        assert_eq!(s.draft.shortcut, "Ctrl+1");
        update(&mut s, Message::Record(Slot::Quick(4)));
        press(&mut s, chord(false, false, "F9"));
        assert_eq!(s.draft.quick_actions[4].shortcut, "F9");
    }

    #[test]
    fn invalid_keys_are_rejected_inline_and_escape_cancels() {
        let (mut s, _) = state(None);
        let before = s.draft.clone();
        update(&mut s, Message::Record(Slot::Capture));
        press(&mut s, chord(false, false, "KeyA"));
        let (slot, why) = s.shortcut_error.clone().unwrap();
        assert_eq!(slot, Slot::Capture);
        assert!(why.contains("modifier"), "{why}");
        assert_eq!(s.recording, Some(Slot::Capture), "stays open to retry");
        assert_eq!(s.draft, before);
        press(&mut s, chord(false, false, "Escape"));
        assert_eq!(s.recording, None);
        assert!(s.shortcut_error.is_none());
        assert_eq!(s.draft, before);
        // A key the parser does not know.
        update(&mut s, Message::Record(Slot::Capture));
        press(&mut s, chord(true, false, "NotARealKey"));
        assert!(s.shortcut_error.as_ref().unwrap().1.contains("unknown key"));
    }

    #[test]
    fn conflicts_show_inline_and_do_not_assign() {
        let (mut s, _) = state(None);
        // Quick action 0 (Ctrl+Shift+T) vs the capture shortcut.
        update(&mut s, Message::Record(Slot::Quick(0)));
        press(&mut s, chord(true, true, "Space"));
        assert_eq!(
            s.shortcut_error,
            Some((Slot::Quick(0), "Already used by Open OpenCircle".into()))
        );
        assert_eq!(s.draft.quick_actions[0].shortcut, "Ctrl+Shift+T");
        // Quick vs quick: "Translate (live)" owns Ctrl+Shift+L.
        press(&mut s, chord(true, true, "KeyL"));
        assert_eq!(
            s.shortcut_error.as_ref().unwrap().1,
            "Already used by Translate (live)"
        );
        // Re-pressing its own combination is fine.
        press(&mut s, chord(true, true, "KeyT"));
        assert_eq!(s.recording, None);
    }

    #[test]
    fn delete_unbinds_quick_actions_only() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Record(Slot::Quick(3)));
        press(&mut s, chord(false, false, "Delete"));
        assert_eq!(s.draft.quick_actions[3].shortcut, "");
        assert_eq!(s.recording, None);
        update(&mut s, Message::Record(Slot::Capture));
        press(&mut s, chord(false, false, "Delete"));
        assert_eq!(s.draft.shortcut, "Ctrl+Shift+Space");
        assert_eq!(s.recording, Some(Slot::Capture));
    }

    #[test]
    fn keycaps_one_per_key() {
        assert_eq!(key_labels("Ctrl+Shift+Space"), ["Ctrl", "Shift", "Space"]);
        assert_eq!(key_labels("ctrl+shift+t"), ["Ctrl", "Shift", "T"]);
        assert_eq!(key_labels("Alt+PrintScreen"), ["Alt", "PrtSc"]);
        assert_eq!(key_labels("Super+KeyA"), ["Super", "A"]);
        assert!(key_labels("").is_empty());
    }

    #[test]
    fn save_round_trips_through_a_temp_config() {
        let path = tmp("roundtrip");
        let (mut s, _) = state(Some(path.clone()));
        update(&mut s, Message::Theme(Theme::Light));
        update(&mut s, Message::Provider(Provider::OpenaiCompatible));
        update(&mut s, Message::Preset(PRESETS[3]));
        update(&mut s, Message::Model("llama3:8b".into()));
        update(&mut s, Message::Language(LANGUAGES[1]));
        update(&mut s, Message::Engine(Engine::Offline));
        update(&mut s, Message::Record(Slot::Quick(4)));
        press(&mut s, chord(true, true, "KeyY"));
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
            panic!("not saved: {:?}", s.notice)
        };
        let (back, warnings) = Config::load_from(&path);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back, cfg);
        assert_eq!(back.theme, Theme::Light);
        assert_eq!(back.ai.base_url, "http://localhost:11434/v1");
        assert_eq!(back.ai.model, "llama3:8b");
        assert_eq!(back.translate.target, "en");
        assert_eq!(back.translate.engine, Engine::Offline);
        assert_eq!(back.quick_actions[4].shortcut, "Ctrl+Shift+Y");
        assert!(!s.dirty());
        theme::apply(Theme::Dark);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn invalid_values_block_save_with_a_reason() {
        let path = tmp("invalid");
        let (mut s, _) = state(Some(path.clone()));
        // OpenAI-compatible with nothing filled in.
        update(&mut s, Message::Provider(Provider::OpenaiCompatible));
        assert_eq!(
            s.draft.ai.model, "",
            "the Claude model id is not carried over"
        );
        assert!(issue(&s.problems, "ai.base_url").unwrap().contains("URL"));
        assert!(issue(&s.problems, "ai.model").is_some());
        assert_eq!(update(&mut s, Message::Save), Outcome::None);
        assert!(!path.exists());
        // Bad scheme: the config validator's own reason.
        update(&mut s, Message::BaseUrl("ftp://x".into()));
        update(&mut s, Message::Model("m".into()));
        assert!(
            issue(&s.problems, "ai.base_url")
                .unwrap()
                .contains("http(s)")
        );
        // Plain http to a remote host while a key is saved: the ai module's rule.
        update(&mut s, Message::BaseUrl("http://example.com/v1".into()));
        assert!(s.problems.is_empty(), "no key saved: {:?}", s.problems);
        update(&mut s, Message::ApiKey(KeyMessage::Input(Secret::new("k"))));
        update(&mut s, Message::ApiKey(KeyMessage::Save));
        assert!(
            issue(&s.problems, "ai.base_url")
                .unwrap()
                .contains("https://")
        );
        assert_eq!(update(&mut s, Message::Save), Outcome::None);
        assert!(!path.exists());
        update(&mut s, Message::BaseUrl("https://example.com/v1".into()));
        assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
        assert!(path.exists());
        theme::apply(Theme::Dark);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn switching_provider_keeps_the_model_sensible() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Provider(Provider::OpenaiCompatible));
        update(&mut s, Message::Preset(PRESETS[0]));
        assert_eq!(
            s.draft.ai.model, "gpt-4o-mini",
            "preset fills an empty model"
        );
        update(&mut s, Message::Provider(Provider::Anthropic));
        assert_eq!(s.draft.ai.model, crate::ai::DEFAULT_ANTHROPIC_MODEL);
        assert_eq!(s.draft.ai.base_url, "", "Anthropic uses its own endpoint");
    }

    const FAKE_KEY: &str = "sk-ant-FAKE-0123456789";

    #[test]
    fn the_api_key_is_write_only() {
        let (mut s, store) = state(None);
        let typing = Message::ApiKey(KeyMessage::Input(Secret::new(FAKE_KEY)));
        assert!(!format!("{typing:?}").contains("FAKE"), "message Debug");
        update(&mut s, typing);
        assert!(
            !format!("{s:?}").contains("FAKE"),
            "state Debug while typing"
        );
        update(&mut s, Message::Section(Section::Ai));
        update(&mut s, Message::ApiKey(KeyMessage::Save));
        assert_eq!(
            store.0.borrow()["anthropic"],
            FAKE_KEY,
            "went to the keyring"
        );
        assert_eq!(*s.key.status(), KeyStatus::Saved);
        // After saving no view text and no Debug output holds the key.
        let texts = shot::texts(view(&s));
        assert!(
            texts.iter().any(|t| t == "Saved in your keyring"),
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t.contains("FAKE")), "{texts:?}");
        assert!(!format!("{s:?}").contains("FAKE"));
        assert!(!format!("{:?}", s.key).contains("FAKE"));
        // Replace opens an empty field (never prefilled); Remove deletes.
        update(&mut s, Message::ApiKey(KeyMessage::Replace));
        let texts = shot::texts(view(&s));
        assert!(!texts.iter().any(|t| t.contains("FAKE")), "{texts:?}");
        update(&mut s, Message::ApiKey(KeyMessage::Cancel));
        update(&mut s, Message::ApiKey(KeyMessage::Remove));
        assert!(store.0.borrow().is_empty());
        assert_eq!(*s.key.status(), KeyStatus::NotSet);
        // The key never reaches the config file either.
        let path = tmp("nokey");
        update(&mut s, Message::Theme(Theme::Dark));
        update(&mut s, Message::Theme(Theme::Light));
        s.path = Some(path.clone());
        update(&mut s, Message::Save);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("FAKE") && !text.contains("api_key"),
            "{text}"
        );
        theme::apply(Theme::Dark);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn every_control_has_its_own_label() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Provider(Provider::OpenaiCompatible));
        let mut all = Vec::new();
        for section in [
            Section::Appearance,
            Section::Shortcuts,
            Section::Ai,
            Section::Translate,
        ] {
            update(&mut s, Message::Section(section));
            TIPS.with(|t| t.borrow_mut().clear());
            let _ = view(&s);
            all.extend(TIPS.with(|t| t.borrow().clone()));
        }
        // Remove the sidebar labels counted once per section.
        all.sort();
        let sidebar = [
            "Appearance settings",
            "Shortcut settings",
            "AI settings",
            "Translate settings",
        ];
        for name in sidebar {
            assert_eq!(all.iter().filter(|t| *t == name).count(), 4, "{name}");
        }
        all.retain(|t| !sidebar.contains(&t.as_str()) && t != "Save settings");
        let mut unique = all.clone();
        unique.dedup();
        assert_eq!(all, unique, "duplicate control labels");
        assert!(all.len() >= 18, "{all:?}");
    }

    #[test]
    fn renders_every_section_in_both_themes() {
        for (name, theme_choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
            let (mut s, _) = state(None);
            update(&mut s, Message::Theme(theme_choice));
            update(&mut s, Message::Provider(Provider::OpenaiCompatible));
            update(&mut s, Message::Preset(PRESETS[0]));
            for (section, label, _) in SECTIONS {
                update(&mut s, Message::Section(section));
                if section == Section::Shortcuts {
                    update(&mut s, Message::Record(Slot::Quick(1)));
                    shot::save(
                        &format!("settings-shortcuts-recording-{name}"),
                        view(&s),
                        WINDOW_SIZE,
                    );
                    press(&mut s, chord(true, true, "Space"));
                }
                let file = format!("settings-{}-{name}", label.to_lowercase());
                let px = shot::save(&file, view(&s), WINDOW_SIZE);
                let window = theme::current().window;
                // Not blank: more than the window colour is on screen.
                let bg = [window.r, window.g, window.b].map(|c| (c * 255.0).round() as u8);
                let other = px
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[..3] != bg)
                    .count();
                assert!(other > 5_000, "{file} looks empty ({other})");
            }
            // The error state of the shortcut field (conflict) and an invalid URL.
            update(&mut s, Message::Section(Section::Ai));
            update(&mut s, Message::BaseUrl("ftp://nope".into()));
            shot::save(
                &format!("settings-ai-invalid-{name}"),
                view(&s),
                WINDOW_SIZE,
            );
            update(&mut s, Message::Section(Section::Ai));
            update(&mut s, Message::Provider(Provider::Anthropic));
            update(&mut s, Message::ApiKey(KeyMessage::Input(Secret::new("k"))));
            update(&mut s, Message::ApiKey(KeyMessage::Save));
            shot::save(&format!("settings-ai-claude-{name}"), view(&s), WINDOW_SIZE);
        }
        theme::apply(Theme::Dark);
    }
}
