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
use std::sync::LazyLock;

use iced::widget::image::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{column, container, row, space};
use iced::{Color, Element, Length, Size, Subscription, event, keyboard};

use crate::config::{
    After, BarStyle, BracketColour, BracketStyle, CardSide, ColourSet, Config, DefaultAction,
    EdgeMode, EdgeStyle, Engine, Format, Look, LookConfig, LookTheme, Provider, Rgb, SearchWith,
    SelectStyle, SnipAfter, SnipConfig, SongSource, Target, Theme, WidgetTheme, parse_shortcut,
};
use crate::hotkey::{DesktopKey, managed};
use crate::mt;
use crate::music::{self, SERVICE, Service};
use crate::secrets::{self, Secret, SecretsError};
use crate::translate::lang::{self, LANGUAGES, Language};
use crate::ui::about;
use crate::ui::theme;
use crate::ui::widgets::{
    self, FORM_WIDTH, FieldState, button_row, caption, colour_swatch, divider, error_text,
    form_button, form_row, label_button, look_tile, nav_item, segmented, shortcut_field, slider,
    text_field, tip, title, value_field, vertical_divider,
};

/// Logical size of the Settings window.
pub const WINDOW_SIZE: Size = Size::new(820.0, 600.0);

// ---- the OS keyring, behind a trait so tests never touch the real one ----

/// `K` = whose key: an AI provider, or a song-search service.
pub trait KeyStore<K = Provider> {
    fn is_set(&self, provider: K) -> Result<bool, SecretsError>;
    fn set(&self, provider: K, key: &Secret) -> Result<(), SecretsError>;
    fn delete(&self, provider: K) -> Result<(), SecretsError>;
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

impl KeyStore<Service> for Keyring {
    /// Finds the entry without reading the key (no unlock prompt).
    fn is_set(&self, service: Service) -> Result<bool, SecretsError> {
        secrets::by_name::exists(service.key_entry())
    }
    fn set(&self, service: Service, key: &Secret) -> Result<(), SecretsError> {
        secrets::by_name::set(service.key_entry(), key)
    }
    fn delete(&self, service: Service) -> Result<(), SecretsError> {
        secrets::by_name::delete(service.key_entry())
    }
}

/// Song search's key store: tests get an in-memory one (never the real keyring).
fn song_keyring() -> Box<dyn KeyStore<Service>> {
    #[cfg(test)]
    return Box::<tests::MemoryStore>::default();
    #[cfg(not(test))]
    Box::new(Keyring)
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
    labels: KeyLabels,
}

/// What a key field calls its key (tooltips are the controls' names).
#[derive(Debug, Clone, Copy)]
pub struct KeyLabels {
    pub placeholder: &'static str,
    pub field: &'static str,
    pub save: &'static str,
    pub replace: &'static str,
    pub remove: &'static str,
    pub keep: &'static str,
}

const API_KEY: KeyLabels = KeyLabels {
    placeholder: "Paste your API key",
    field: "API key",
    save: "Save the API key",
    replace: "Replace the API key",
    remove: "Remove the API key",
    keep: "Keep the saved API key",
};

const SONG_KEY: KeyLabels = KeyLabels {
    placeholder: "Paste your AudD API token",
    field: "AudD API token",
    save: "Save the AudD token",
    replace: "Replace the AudD token",
    remove: "Remove the AudD token",
    keep: "Keep the saved AudD token",
};

impl KeyField {
    pub fn new<K>(store: &dyn KeyStore<K>, provider: K) -> Self {
        let mut f = KeyField {
            status: KeyStatus::NotSet,
            input: Secret::new(""),
            replacing: false,
            error: None,
            labels: API_KEY,
        };
        f.refresh(store, provider);
        f
    }

    pub fn labelled(mut self, labels: KeyLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Ask the keyring again (after the provider changed).
    pub fn refresh<K>(&mut self, store: &dyn KeyStore<K>, provider: K) {
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

    pub fn update<K>(&mut self, store: &dyn KeyStore<K>, provider: K, message: KeyMessage) {
        self.error = None;
        match message {
            KeyMessage::Input(key) => self.input = key,
            KeyMessage::Replace => self.replacing = true,
            KeyMessage::Cancel => {
                self.replacing = false;
                self.input = Secret::new("");
            }
            KeyMessage::Save if self.input.is_empty() => {
                self.error = Some("Paste your key first".into())
            }
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
        let l = self.labels;
        let mut col = column![].spacing(8).width(FORM_WIDTH);
        match (&self.status, self.replacing) {
            (KeyStatus::Unavailable(why), _) => col = col.push(error_text(why.as_str())),
            (KeyStatus::Saved, false) => {
                let saved = value_field("Saved in your keyring", true);
                let replace = form_button("Replace", false, Some(KeyMessage::Replace));
                let remove = form_button("Remove", false, Some(KeyMessage::Remove));
                col = col.push(saved).push(button_row([
                    tip(replace, l.replace, Position::Bottom),
                    tip(remove, l.remove, Position::Bottom),
                ]));
            }
            _ => {
                let field = text_field(l.placeholder, self.input.expose(), false, |s| {
                    KeyMessage::Input(Secret::new(s))
                })
                .secure(true)
                .on_submit(KeyMessage::Save);
                let save = form_button("Save key", true, Some(KeyMessage::Save));
                let mut buttons = vec![tip(save, l.save, Position::Bottom)];
                if self.replacing {
                    let cancel = form_button("Cancel", false, Some(KeyMessage::Cancel));
                    buttons.push(tip(cancel, l.keep, Position::Bottom));
                }
                col = col
                    .push(tip(field, l.field, Position::Top))
                    .push(button_row(buttons));
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
        // Free, correct Arabic, ~3 s when measured. Fast paid option: anthropic/claude-haiku-5.5
        // (shown as the placeholder while the model field is empty).
        model: "nvidia/nemotron-3-ultra-550b-a55b:free",
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

/// The Source dropdown: "Detect automatically", then what OCR can read.
static SOURCES: LazyLock<Vec<Language>> = LazyLock::new(|| {
    let auto = Language {
        code: lang::AUTO,
        name: "Detect automatically",
        ..LANGUAGES[1]
    };
    let readable = LANGUAGES.iter().filter(|l| l.ocr.is_some()).copied();
    std::iter::once(auto).chain(readable).collect()
});

fn action_name(id: &str, live: bool, target: Target) -> String {
    match (id, live) {
        ("snip", _) => match target {
            Target::Window => "Snip a window".into(),
            Target::FullScreen => "Snip the full screen".into(),
            _ => "Snip a region".into(),
        },
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
        Target::Window => "The window you click",
    }
}

/// The hint under a quick action's name: snip shows what the mode does.
fn quick_hint(q: &crate::config::QuickAction) -> &'static str {
    match (q.action.as_str(), q.target) {
        ("snip", Target::Window) => "Click a window, then edit it",
        ("snip", Target::FullScreen) => "The whole screen, then edit it",
        ("snip", _) => "Drag or circle an area, then edit it",
        ("song_search", _) => "Listens and names the song (needs a key)",
        (_, t) => target_name(t),
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
    General,
    Appearance,
    Look,
    Shortcuts,
    Snip,
    Ai,
    Translate,
    Song,
    About,
}

const SECTIONS: [(Section, &str, &str); 9] = [
    (Section::General, "General", "General settings"),
    (Section::Appearance, "Appearance", "Appearance settings"),
    (
        Section::Look,
        "Look & behaviour",
        "Look and behaviour settings",
    ),
    (Section::Shortcuts, "Shortcuts", "Shortcut settings"),
    (Section::Snip, "Snip", "Snip settings"),
    (Section::Ai, "AI", "AI settings"),
    (Section::Translate, "Translate", "Translate settings"),
    (Section::Song, "Song search", "Song search settings"),
    (Section::About, "About", "About OpenCircle"),
];

/// What the update switch says it does (Settings and onboarding).
pub const UPDATE_PRIVACY: &str = "Asks GitHub for the latest version twice a day and installs it when \
     OpenCircle is idle. Sends only that request: nothing about you or your screen";

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
    /// Song search's key: the store, and its field once the section was opened
    /// (the keyring is asked only then).
    song_store: Box<dyn KeyStore<Service>>,
    song_key: Option<KeyField>,
    quick_text: Vec<SlotText>,
    /// What the live-interval field shows (kept as typed).
    interval: String,
    /// "key: reason" lines from `Config`'s own validation of the draft.
    problems: Vec<String>,
    notice: Option<String>,
    /// The desktop's own say on a running key (`hotkey::desktop_key`; tests swap it).
    desktop: fn(&str) -> Option<DesktopKey>,
    /// Look & behaviour: whose screen edge the custom look edits, which of its
    /// two colours has the picker open (and the hex typed there), and the
    /// preview of each look (presets, then custom).
    edge_mode: EdgeMode,
    picker: Option<usize>,
    hex: String,
    previews: Vec<Handle>,
    /// The last Check now's result (`None` = not checked in this window).
    update_status: Option<String>,
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
    DefaultAction(DefaultAction),
    CardSide(CardSide),
    SearchHistory(bool),
    Autostart(bool),
    /// The Browse button: the shell opens the folder picker.
    BrowseFolder,
    /// The picker's answer (`None` = cancelled).
    Folder(Option<PathBuf>),
    ClearFolder,
    /// The Project button.
    OpenLink(String),
    Theme(Theme),
    WidgetTheme(WidgetTheme),
    GlassEffect(bool),
    Record(Slot),
    Key(Chord),
    Provider(Provider),
    Preset(Preset),
    BaseUrl(String),
    Model(String),
    ApiKey(KeyMessage),
    SongKey(KeyMessage),
    SongSource(SongSource),
    SongHistory(bool),
    Source(Language),
    Target(Language),
    Engine(Engine),
    /// What was typed in the live-interval field.
    LiveInterval(String),
    SelectStyle(SelectStyle),
    After(After),
    SearchWith(SearchWith),
    Look(LookTheme),
    /// The custom look's controls.
    Palette(PaletteChoice),
    Glow(u8),
    Dim(u8),
    Activation(bool),
    Brackets(BracketColour),
    BracketStyle(BracketStyle),
    Bar(BarChoice),
    EdgeMode(EdgeMode),
    EdgeStyle(EdgeStyle),
    Thickness(u8),
    Animate(bool),
    /// Open / close the picker for custom colour 0 (from) or 1 (to).
    Picker(usize),
    PickColour(Color),
    Hex(String),
    /// The Snip section's choices.
    Snip(SnipConfig),
    /// About: keep OpenCircle up to date, Check now, and that check's result.
    AutoUpdate(bool),
    CheckUpdate,
    UpdateStatus(String),
    Save,
}

/// A colour set in the custom look's dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteChoice(ColourSet);

impl fmt::Display for PaletteChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            ColourSet::Spectrum => "Spectrum",
            ColourSet::Aurora => "Aurora",
            ColourSet::Sunset => "Sunset",
            ColourSet::Ocean => "Ocean",
            ColourSet::Neon => "Neon",
            ColourSet::Mono => "Mono",
            ColourSet::TwoColour => "Two colours of my own",
        })
    }
}

const PALETTES: [PaletteChoice; 7] = [
    PaletteChoice(ColourSet::Spectrum),
    PaletteChoice(ColourSet::Aurora),
    PaletteChoice(ColourSet::Sunset),
    PaletteChoice(ColourSet::Ocean),
    PaletteChoice(ColourSet::Neon),
    PaletteChoice(ColourSet::Mono),
    PaletteChoice(ColourSet::TwoColour),
];

/// A bar style in the custom look's dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarChoice(BarStyle);

impl fmt::Display for BarChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            BarStyle::Glass => "Glass (app theme)",
            BarStyle::GlassDark => "Dark glass",
            BarStyle::GlassLight => "Light glass",
            BarStyle::Solid => "Solid",
            BarStyle::Outline => "Outline",
        })
    }
}

const BARS: [BarChoice; 5] = [
    BarChoice(BarStyle::Glass),
    BarChoice(BarStyle::GlassDark),
    BarChoice(BarStyle::GlassLight),
    BarChoice(BarStyle::Solid),
    BarChoice(BarStyle::Outline),
];

/// The looks in the grid: the presets, then Custom.
const LOOKS: [(LookTheme, &str, &str); 8] = [
    (
        LookTheme::Spectrum,
        "Spectrum",
        "Spectrum look: soft glow in many colours",
    ),
    (
        LookTheme::Aurora,
        "Aurora",
        "Aurora look: thin violet to cyan line",
    ),
    (
        LookTheme::Sunset,
        "Sunset",
        "Sunset look: orange to pink glow",
    ),
    (LookTheme::Ocean, "Ocean", "Ocean look: teal to blue glow"),
    (
        LookTheme::Neon,
        "Neon",
        "Neon look: strong cyan and pink glow",
    ),
    (
        LookTheme::Mono,
        "Mono",
        "Mono look: white and grey, minimal",
    ),
    (
        LookTheme::Calm,
        "Calm",
        "Calm look: no glow, thin line, light dim",
    ),
    (LookTheme::Custom, "Custom", "Custom look: your own choices"),
];

// One short-lived value per message: a boxed Config would buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    None,
    /// Written to disk: re-bind the shortcuts, re-read the theme.
    Saved(Config),
    /// Show the folder picker; the answer comes back as `Message::Folder`.
    Browse,
    /// Open this link in the browser (the user clicked it on the About page).
    OpenLink(String),
    /// Check for an update now; the answer comes back as `Message::UpdateStatus`.
    CheckUpdate,
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
        let key = KeyField::new(&*store, config.ai.provider.clone());
        let quick_text = config
            .quick_actions
            .iter()
            .map(|q| {
                let label = action_name(&q.action, q.live, q.target);
                SlotText {
                    hint: quick_hint(q).to_string(),
                    tip: format!("{label} shortcut"),
                    label,
                }
            })
            .collect();
        let interval = config.translate.live_interval_ms.to_string();
        let mut s = State {
            section: Section::General,
            saved: config.clone(),
            draft: config,
            path,
            store,
            recording: None,
            shortcut_error: None,
            key,
            song_store: song_keyring(),
            song_key: None,
            quick_text,
            interval,
            problems: Vec::new(),
            notice: warnings
                .first()
                // Not "reset": an unknown or unread provider is kept off, not defaulted.
                .map(|w| format!("Some saved settings were invalid ({w})")),
            desktop: crate::hotkey::desktop_key,
            edge_mode: EdgeMode::Overlay,
            picker: None,
            hex: String::new(),
            previews: Vec::new(),
            update_status: None,
        };
        s.previews = LOOKS.iter().map(|(t, ..)| preview(&s.look(*t))).collect();
        s.revalidate();
        s
    }

    /// What look `theme` is (Custom: the draft's own).
    fn look(&self, theme: LookTheme) -> Look {
        theme.look(&self.draft.look.custom)
    }

    fn dirty(&self) -> bool {
        self.draft != self.saved
    }

    /// The custom look changed: its preview again.
    fn refresh_custom(&mut self) {
        let custom = self.look(LookTheme::Custom);
        if let Some(last) = self.previews.last_mut() {
            *last = preview(&custom);
        }
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

    /// Wayland: what the desktop says about `slot`'s key when it isn't that key
    /// (`None` also while the slot has unsaved changes: the desktop has the saved one).
    fn desktop_note(&self, slot: Slot) -> Option<String> {
        let saved = match slot {
            Slot::Capture => Some(&self.saved.shortcut),
            Slot::Quick(i) => self.saved.quick_actions.get(i).map(|q| &q.shortcut),
        };
        let shortcut = self.shortcut(slot);
        if saved.map(String::as_str) != Some(shortcut) {
            return None;
        }
        Some(match (self.desktop)(shortcut)? {
            DesktopKey::Other(keys) => format!("In use: {keys}. {}", managed()),
            DesktopKey::Unassigned => format!("No key in use. {}", managed()),
        })
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
    if cfg.ai.provider == Provider::Unreadable {
        // It cannot be written: report it, and check the rest as if one were picked.
        let mut rest = cfg.clone();
        rest.ai.provider = Provider::Browser;
        let mut out = problems(&rest, key_saved);
        out.insert(
            0,
            "ai.provider: Choose a provider (the settings file couldn't be read)".into(),
        );
        return out;
    }
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
            if s == Section::Song && state.song_key.is_none() {
                let field = KeyField::new(&*state.song_store, SERVICE).labelled(SONG_KEY);
                music::remember_key(field.status == KeyStatus::Saved);
                state.song_key = Some(field);
            }
        }
        Message::SongKey(k) => {
            if let Some(field) = &mut state.song_key {
                field.update(&*state.song_store, SERVICE, k);
                // The overlay's music button shows from now on (or hides).
                music::remember_key(field.status == KeyStatus::Saved);
            }
        }
        Message::SongSource(source) => state.draft.song.source = source,
        Message::SongHistory(on) => state.draft.song.history = on,
        Message::Theme(t) => {
            state.draft.theme = t;
            theme::apply(t);
        }
        Message::WidgetTheme(t) => state.draft.widget_theme = t,
        Message::GlassEffect(on) => state.draft.glass_effect = on,
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
            ai.provider = p.clone();
            match p {
                // Hidden fields must stay valid: nothing half-typed blocks Save.
                Provider::Browser => {
                    ai.base_url.clear();
                    if ai.model.is_empty() {
                        ai.model = default_model.to_string();
                    }
                }
                Provider::Anthropic => {
                    ai.base_url.clear();
                    if ai.model.is_empty() || PRESETS.iter().any(|x| x.model == ai.model) {
                        ai.model = default_model.to_string();
                    }
                }
                Provider::OpenaiCompatible if ai.model == default_model => ai.model.clear(),
                Provider::OpenaiCompatible => {}
                Provider::Invalid(_) | Provider::Unreadable => {} // never offered: only a bad file has it
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
            let provider = state.draft.ai.provider.clone();
            state.key.update(&*state.store, provider, k);
        }
        Message::Source(l) => state.draft.translate.source = l.code.to_string(),
        Message::Target(l) => state.draft.translate.target = l.code.to_string(),
        Message::Engine(e) => state.draft.translate.engine = e,
        Message::LiveInterval(s) => {
            // Not a number = 0, which the range check reports under the field.
            state.draft.translate.live_interval_ms = s.trim().parse().unwrap_or(0);
            state.interval = s;
        }
        Message::DefaultAction(a) => state.draft.general.default_action = a,
        Message::CardSide(side) => state.draft.results.side = side,
        Message::SearchHistory(on) => {
            state.draft.general.search_history = on;
            if !on {
                crate::sheet::history::clear_default(); // turning it off deletes it
            }
        }
        Message::Autostart(on) => state.draft.general.autostart = on,
        Message::SelectStyle(v) => state.draft.selection.style = v,
        Message::After(v) => state.draft.selection.after = v,
        Message::SearchWith(v) => state.draft.selection.search_with = v,
        Message::Look(t) => {
            // The first time Custom is picked it starts as the look in use.
            let look = &mut state.draft.look;
            if t == LookTheme::Custom
                && look.theme != t
                && look.custom == LookConfig::default().custom
            {
                look.custom = look.active();
            }
            look.theme = t;
            state.picker = None;
            state.refresh_custom();
        }
        Message::EdgeMode(m) => state.edge_mode = m,
        Message::Picker(i) => {
            state.picker = (state.picker != Some(i)).then_some(i);
            state.hex = state.draft.look.custom.colours[i.min(1)].hex();
        }
        Message::Hex(hex) => {
            if let (Some(i), Some(c)) = (state.picker, Rgb::parse(&hex)) {
                state.draft.look.custom.colours[i] = c;
                state.refresh_custom();
            }
            state.hex = hex;
        }
        Message::PickColour(c) => {
            if let Some(i) = state.picker {
                let [r, g, b, _] = c.into_rgba8();
                state.draft.look.custom.colours[i] = Rgb([r, g, b]);
                state.hex = Rgb([r, g, b]).hex();
                state.refresh_custom();
            }
        }
        Message::Palette(_)
        | Message::Glow(_)
        | Message::Dim(_)
        | Message::Activation(_)
        | Message::Brackets(_)
        | Message::BracketStyle(_)
        | Message::Bar(_)
        | Message::EdgeStyle(_)
        | Message::Thickness(_)
        | Message::Animate(_) => {
            let mode = state.edge_mode;
            let custom = &mut state.draft.look.custom;
            match message {
                Message::Palette(p) => custom.palette = p.0,
                Message::Glow(v) => custom.glow = v,
                Message::Dim(v) => custom.dim = v,
                Message::Activation(v) => custom.activation_glow = v,
                Message::Brackets(v) => custom.brackets = v,
                Message::BracketStyle(v) => custom.bracket_style = v,
                Message::Bar(v) => custom.bar = v.0,
                Message::EdgeStyle(v) => state.draft.look.edge_mut(mode).style = v,
                Message::Thickness(v) => state.draft.look.edge_mut(mode).thickness = v,
                Message::Animate(v) => state.draft.look.edge_mut(mode).animate = v,
                _ => {}
            }
            state.refresh_custom();
        }
        Message::Snip(snip) => state.draft.snip = snip,
        Message::BrowseFolder => return Outcome::Browse,
        Message::Folder(Some(dir)) => {
            state.draft.general.save_folder = dir.to_string_lossy().into_owned();
        }
        Message::Folder(None) => {}
        Message::ClearFolder => state.draft.general.save_folder.clear(),
        Message::OpenLink(url) => return Outcome::OpenLink(url),
        Message::AutoUpdate(on) => state.draft.update.auto = Some(on),
        Message::CheckUpdate => {
            state.update_status = Some("Checking…".into());
            return Outcome::CheckUpdate;
        }
        Message::UpdateStatus(status) => state.update_status = Some(status),
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
    // Never a dead button: with nothing to save it says so.
    if !state.dirty() {
        state.notice = Some("Nothing to save".into());
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
        Section::General => ("General", general_rows(state)),
        Section::Look => ("Look & behaviour", look_rows(state)),
        Section::About => ("About", about_rows()),
        Section::Appearance => (
            "Appearance",
            vec![
                form_row(
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
                ),
                form_row(
                    "Floating widgets",
                    "The bar, cards and toolbar",
                    segmented(
                        &[
                            (WidgetTheme::Default, "Default", "Follow the app theme"),
                            (WidgetTheme::Frost, "Frost", "Frosted glass over the screen"),
                            (WidgetTheme::Graphite, "Graphite", "Calm dark graphite"),
                        ],
                        d.widget_theme,
                        Message::WidgetTheme,
                    ),
                    None,
                ),
                form_row(
                    "Glass effect",
                    "Frost and Graphite blur the screen behind them",
                    segmented(
                        &[
                            (true, "On", "Blur the screen behind the widgets"),
                            (false, "Off", "A plain tinted widget"),
                        ],
                        d.glass_effect,
                        Message::GlassEffect,
                    ),
                    None,
                ),
            ],
        ),
        Section::Shortcuts => ("Shortcuts", shortcut_rows(state)),
        Section::Snip => ("Snip", snip_rows(&state.draft.snip)),
        Section::Ai => ("AI", ai_rows(state)),
        Section::Song => ("Song search", song_rows(state)),
        Section::Translate => (
            "Translate",
            vec![
                form_row(
                    "Engine",
                    match d.translate.engine {
                        Engine::Ai => "Your AI provider; offline when it isn't set up",
                        Engine::Offline => "No key, nothing leaves your PC; more literal than AI",
                    },
                    segmented(
                        &[
                            (
                                Engine::Ai,
                                "AI",
                                "Translate with your AI provider (Settings > AI)",
                            ),
                            (
                                Engine::Offline,
                                "Offline",
                                "Translate on this PC (English to and from some languages)",
                            ),
                        ],
                        d.translate.engine,
                        Message::Engine,
                    ),
                    None,
                ),
                form_row(
                    "Source language",
                    "What the text on screen is in; picks the text recognizer",
                    tip(
                        widgets::dropdown(
                            &SOURCES,
                            SOURCES
                                .iter()
                                .find(|l| l.code == d.translate.source)
                                .copied(),
                            &d.translate.source,
                            Message::Source,
                        ),
                        "Source language",
                        Position::Top,
                    ),
                    issue(&state.problems, "translate.source"),
                ),
                form_row(
                    "Target language",
                    // Offline can't do every pair: say so (AI is the other way).
                    if d.translate.engine == Engine::Offline
                        && !mt::available(&d.translate.source, &d.translate.target)
                    {
                        "No offline model for this pair: choose AI to translate it"
                    } else {
                        "What the text is translated into"
                    },
                    tip(
                        widgets::dropdown(
                            LANGUAGES,
                            LANGUAGES
                                .iter()
                                .find(|l| l.code == d.translate.target)
                                .copied(),
                            &d.translate.target,
                            Message::Target,
                        ),
                        "Target language",
                        Position::Top,
                    ),
                    issue(&state.problems, "translate.target"),
                ),
                {
                    let error = issue(&state.problems, "translate.live_interval_ms");
                    form_row(
                        "Live refresh",
                        "Milliseconds between updates in live translate",
                        tip(
                            text_field(
                                "1000",
                                &state.interval,
                                error.is_some(),
                                Message::LiveInterval,
                            ),
                            "Live translate refresh interval",
                            Position::Top,
                        ),
                        error,
                    )
                },
            ],
        ),
    };
    // The rows scroll under the title: the Shortcuts list outgrows the window.
    let mut list = column![].padding(iced::Padding::ZERO.right(12));
    // About is a stack of cards; the other pages are rows split by rules.
    let cards = state.section == Section::About;
    if cards {
        list = list.spacing(16);
    }
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 && !cards {
            list = list.push(divider());
        }
        list = list.push(r);
    }
    column![
        title(heading),
        space::vertical().height(8),
        widgets::scroll(list).height(Length::Fill)
    ]
    .height(Length::Fill)
    .into()
}

/// A long folder path cut in the middle so it fits its field.
fn shorten(path: &str) -> String {
    const MAX: usize = 38;
    let n = path.chars().count();
    if n <= MAX {
        return path.to_string();
    }
    let tail: String = path.chars().skip(n - (MAX - 1)).collect();
    format!("…{tail}")
}

/// The one line under "Results card": where it sits, or why it can't open.
fn results_hint() -> &'static str {
    crate::sheet::unavailable()
        .unwrap_or("Lens and AI Mode results. Drag its header to the other side")
}

/// The Start-with-the-OS switch's label (Settings and onboarding).
pub const START_LABEL: &str = if cfg!(windows) {
    "Start OpenCircle when Windows starts"
} else {
    "Start OpenCircle when you log in"
};

fn general_rows(state: &State) -> Vec<Element<'_, Message>> {
    let folder = state.draft.general.save_folder.as_str();
    let shown = if folder.is_empty() {
        value_field("Chosen by your system", true)
    } else {
        value_field(shorten(folder), false)
    };
    let browse = form_button("Browse…", false, Some(Message::BrowseFolder));
    let reset = form_button(
        "Reset",
        false,
        (!folder.is_empty()).then_some(Message::ClearFolder),
    );
    let folder_control = column![
        if shorten(folder) == folder {
            tip(shown, "Save folder", Position::Top)
        } else {
            // Cut in the middle: the tooltip is the whole path.
            widgets::hint(
                shown,
                "Save folder",
                widgets::Why::Cut(folder),
                Position::Top,
            )
        },
        button_row([
            tip(browse, "Choose the save folder", Position::Bottom),
            tip(reset, "Use the system's folder", Position::Bottom),
        ]),
    ]
    .spacing(8)
    .width(FORM_WIDTH);
    let error = issue(&state.problems, "general.save_folder");
    vec![
        form_row(
            "Default action",
            "What Enter does after you select something",
            segmented(
                &[
                    (
                        DefaultAction::Lens,
                        "Search",
                        "Enter searches with Google Lens",
                    ),
                    (DefaultAction::Ai, "Ask AI", "Enter asks the AI about it"),
                    (DefaultAction::Text, "Text", "Enter reads the text"),
                    (DefaultAction::Copy, "Copy", "Enter copies the image"),
                ],
                state.draft.general.default_action,
                Message::DefaultAction,
            ),
            None,
        ),
        form_row(
            "Results card",
            results_hint(),
            segmented(
                &[
                    (CardSide::Left, "Left", "The card sits on the left"),
                    (CardSide::Right, "Right", "The card sits on the right"),
                ],
                state.draft.results.side,
                Message::CardSide,
            ),
            None,
        ),
        form_row(
            START_LABEL,
            "OpenCircle waits in the tray, with no window",
            segmented(
                &[
                    (false, "Off", "Don't start OpenCircle by itself"),
                    (true, "On", "Start OpenCircle by itself"),
                ],
                state.draft.general.autostart,
                Message::Autostart,
            ),
            None,
        ),
        form_row(
            "Search history",
            "Only on this PC: what you searched, a small picture, the time",
            segmented(
                &[
                    (
                        false,
                        "Off",
                        "Keep no searches (turning it off deletes them)",
                    ),
                    (true, "On", "The card's History button lists your searches"),
                ],
                state.draft.general.search_history,
                Message::SearchHistory,
            ),
            None,
        ),
        form_row(
            "Save folder",
            "Where Save starts. Files are OpenCircle-date-time.png",
            folder_control,
            error,
        ),
        form_row(
            "Keep OpenCircle up to date",
            UPDATE_PRIVACY,
            segmented(
                &[
                    (false, "Off", "Don't check for updates"),
                    (true, "On", "Check for updates and install them"),
                ],
                state.draft.update.auto == Some(true),
                Message::AutoUpdate,
            ),
            None,
        ),
        form_row(
            "Updates",
            state
                .update_status
                .as_deref()
                .unwrap_or("Look for a new version now"),
            button_row([form_button("Check now", false, Some(Message::CheckUpdate)).into()]),
            None,
        ),
    ]
}

fn look_rows(state: &State) -> Vec<Element<'_, Message>> {
    let sel = state.draft.selection;
    let look = &state.draft.look;
    let mut rows = vec![
        form_row(
            "Selection",
            "Snap: a rounded box with corners to drag",
            segmented(
                &[
                    (
                        SelectStyle::Snap,
                        "Snap to box",
                        "Selections snap to a rounded box",
                    ),
                    (
                        SelectStyle::Shape,
                        "As drawn",
                        "Selections keep the drawn shape",
                    ),
                ],
                sel.style,
                Message::SelectStyle,
            ),
            None,
        ),
        form_row(
            "After selecting",
            match sel.after {
                After::Search => "Searches at once; More shows every action",
                After::Wait => "Shows the action bar",
            },
            segmented(
                &[
                    (After::Search, "Search", "Search the selection at once"),
                    (After::Wait, "Show actions", "Show the action bar and wait"),
                ],
                sel.after,
                Message::After,
            ),
            None,
        ),
        form_row(
            "Search with",
            "What searching a selection opens",
            segmented(
                &[
                    (
                        SearchWith::Lens,
                        "Lens",
                        "Search with Google Lens in your browser",
                    ),
                    (SearchWith::Ai, "Ask AI", "Ask the AI about the selection"),
                    (SearchWith::Both, "Both", "Open Lens and ask the AI"),
                ],
                sel.search_with,
                Message::SearchWith,
            ),
            None,
        ),
    ];
    // The looks, as a grid of previews (4 per row).
    let tiles: Vec<Element<'_, Message>> = (LOOKS.iter().zip(&state.previews))
        .map(|(&(theme, label, hint), preview)| {
            let tile = look_tile(
                preview.clone(),
                label,
                look.theme == theme,
                Message::Look(theme),
            );
            tip(tile, hint, Position::Top)
        })
        .collect();
    let mut grid = column![].spacing(8);
    let mut tiles = tiles.into_iter();
    loop {
        let line: Vec<_> = tiles.by_ref().take(4).collect();
        if line.is_empty() {
            break;
        }
        grid = grid.push(row(line).spacing(8));
    }
    let hint = match look.theme {
        LookTheme::Custom => "Your own look: change it below",
        _ => "How the screen looks while OpenCircle is open. Custom changes every part",
    };
    rows.push(
        column![
            column![
                iced::widget::text("Active look")
                    .size(14)
                    .color(theme::current().text),
                caption(hint)
            ]
            .spacing(2),
            grid
        ]
        .spacing(12)
        .padding([12, 0])
        .into(),
    );
    if look.theme == LookTheme::Custom {
        rows.extend(custom_rows(state));
    }
    rows
}

/// The custom look's controls.
fn custom_rows(state: &State) -> Vec<Element<'_, Message>> {
    let c = &state.draft.look.custom;
    let edge = c.edge(state.edge_mode);
    let mut rows = vec![form_row(
        "Colours",
        "The screen edge, selection and buttons",
        tip(
            widgets::dropdown(
                &PALETTES,
                Some(PaletteChoice(c.palette)),
                "Choose colours",
                Message::Palette,
            ),
            "Look colours",
            Position::Top,
        ),
        None,
    )];
    if c.palette == ColourSet::TwoColour {
        let rgb = |i: usize| {
            let [r, g, b] = c.colours[i].0;
            Color::from_rgb8(r, g, b)
        };
        let buttons = button_row([
            widgets::hint(
                colour_swatch(rgb(0), state.picker == Some(0), Message::Picker(0)),
                "First colour",
                widgets::Why::Icon,
                Position::Top,
            ),
            widgets::hint(
                colour_swatch(rgb(1), state.picker == Some(1), Message::Picker(1)),
                "Second colour",
                widgets::Why::Icon,
                Position::Top,
            ),
        ]);
        let mut control = column![buttons].spacing(8);
        if let Some(i) = state.picker {
            control = control.push(widgets::colour_picker(
                &theme::ANNOTATION,
                rgb(i),
                &state.hex,
                Message::PickColour,
                Message::Hex,
            ));
        }
        rows.push(form_row(
            "Your two colours",
            "From the top-left corner round to the opposite one",
            control,
            None,
        ));
    }
    rows.extend([
        form_row(
            "Glow strength",
            "How bright the edge glow is",
            tip(
                slider(crate::config::GLOW, c.glow, " %", Message::Glow),
                "Glow strength",
                Position::Top,
            ),
            None,
        ),
        form_row(
            "Dim",
            "How much the frozen screen darkens",
            tip(
                slider(crate::config::DIM, c.dim, " %", Message::Dim),
                "Dim strength",
                Position::Top,
            ),
            None,
        ),
        form_row(
            "Activation glow",
            "A wide glow when the screen freezes",
            segmented(
                &[
                    (true, "On", "Glow when the screen freezes"),
                    (false, "Off", "No glow when the screen freezes"),
                ],
                c.activation_glow,
                Message::Activation,
            ),
            None,
        ),
        form_row(
            "Selection corners",
            "The brackets on a snapped box",
            segmented(
                &[
                    (BracketColour::White, "White", "White corners"),
                    (
                        BracketColour::Accent,
                        "Accent",
                        "Corners in the look's colour",
                    ),
                ],
                c.brackets,
                Message::Brackets,
            ),
            None,
        ),
        form_row(
            "Corner style",
            "Four corners, or a frame all round",
            segmented(
                &[
                    (BracketStyle::Corners, "Corners", "Four corner brackets"),
                    (BracketStyle::Frame, "Frame", "A thin frame all round"),
                ],
                c.bracket_style,
                Message::BracketStyle,
            ),
            None,
        ),
        form_row(
            "Bars and pills",
            "The action bar, chips and panels",
            tip(
                widgets::dropdown(
                    &BARS,
                    Some(BarChoice(c.bar)),
                    "Choose a style",
                    Message::Bar,
                ),
                "Bar style",
                Position::Top,
            ),
            None,
        ),
        form_row(
            "Screen edge for",
            "Each mode has its own edge",
            segmented(
                &[
                    (EdgeMode::Overlay, "Circle", "The edge while circling"),
                    (EdgeMode::Live, "Live", "The edge of live translate"),
                    (EdgeMode::Snip, "Snip", "The edge while snipping"),
                ],
                state.edge_mode,
                Message::EdgeMode,
            ),
            None,
        ),
        form_row(
            "Edge style",
            "A soft glow, a thin line, or none",
            segmented(
                &[
                    (EdgeStyle::Glow, "Glow", "A soft glow fading inward"),
                    (EdgeStyle::Thin, "Thin", "A thin line"),
                    (EdgeStyle::None, "None", "No screen edge"),
                ],
                edge.style,
                Message::EdgeStyle,
            ),
            None,
        ),
        form_row(
            "Edge thickness",
            "A glow reaches 8 times as far",
            tip(
                slider(
                    crate::config::EDGE_THICKNESS,
                    edge.thickness,
                    " px",
                    Message::Thickness,
                ),
                "Edge thickness",
                Position::Top,
            ),
            None,
        ),
        form_row(
            "Edge motion",
            match state.edge_mode {
                EdgeMode::Live => "Moves while live translate runs",
                _ => "Once when it opens",
            },
            segmented(
                &[
                    (true, "Animate", "The edge moves"),
                    (false, "Still", "The edge stays still"),
                ],
                edge.animate,
                Message::Animate,
            ),
            None,
        ),
    ]);
    rows
}

/// A small picture of `look` over a sample screen: the dim, a snapped box
/// with its corners, the screen edge and a pill in the bar style.
fn preview(look: &Look) -> Handle {
    let (w, h) = (
        widgets::TILE.width as u32 * 2,
        widgets::TILE.height as u32 * 2,
    );
    // a sample screen: a dark page with lines of "text" (ours, made up)
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let line = (y % 22) < 6 && y > 20 && x > 30 && x < w - 30 - (y * 7 % 60);
            let c = if line { [120, 128, 150] } else { [34, 38, 52] };
            rgba.extend([c[0], c[1], c[2], 255]);
        }
    }
    // dim outside a snapped box, its corners (or frame) 3 px outside it
    let (_, outside) = theme::dims(look);
    let corner = theme::bracket(look).into_rgba8();
    let (bx, by) = (80..160u32, 40..96u32);
    let frame = look.bracket_style == BracketStyle::Frame;
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if bx.contains(&x) && by.contains(&y) {
                continue;
            }
            let ring = (77..163).contains(&x) && (37..99).contains(&y);
            let near_corner = !(94..=145).contains(&x) && !(52..=83).contains(&y);
            if ring && (frame || near_corner) {
                rgba[i..i + 4].copy_from_slice(&corner);
            } else {
                for c in &mut rgba[i..i + 3] {
                    *c = (f32::from(*c) * (1.0 - outside.a)) as u8;
                }
            }
        }
    }
    crate::ui::border::paint(&mut rgba, w, h, 0.3, look, EdgeMode::Overlay);
    // a pill in the bar style at the bottom
    let (start, _) = theme::accent_of(look);
    let fill = match look.bar {
        BarStyle::GlassLight => Color::from_rgb8(0xf2, 0xf2, 0xf6),
        BarStyle::Outline => Color::from_rgb8(0x22, 0x24, 0x30),
        _ => Color::from_rgb8(0x1c, 0x1d, 0x2a),
    }
    .into_rgba8();
    let edge = start.into_rgba8();
    let (px0, px1, py0, py1) = (w / 2 - 40, w / 2 + 40, h - 26, h - 12);
    for y in py0..py1 {
        for x in px0..px1 {
            let border = x == px0 || x == px1 - 1 || y == py0 || y == py1 - 1;
            let i = ((y * w + x) * 4) as usize;
            let button = x > px0 + 6 && x < px0 + 30 && y > py0 + 4 && y < py1 - 4;
            let c = if (border && look.bar == BarStyle::Outline) || button {
                edge // the outline, and the accent button
            } else {
                fill
            };
            rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    Handle::from_rgba(w, h, rgba)
}

/// Snip: what a capture does, the size and file it is copied and saved as,
/// the pointer, and Recent (off = nothing kept on disk).
fn snip_rows(d: &SnipConfig) -> Vec<Element<'_, Message>> {
    // The draft's snip choices with one changed.
    let edit = |change: &dyn Fn(&mut SnipConfig)| {
        let mut next = d.clone();
        change(&mut next);
        Message::Snip(next)
    };
    vec![
        form_row(
            "After a capture",
            "Copy: ready to paste into an AI chat, Edit a click away",
            segmented(
                &[
                    (SnipAfter::Copy, "Copy", "Copy it; the toast offers Edit"),
                    (SnipAfter::Editor, "Editor", "Open the editor on it"),
                    (SnipAfter::Both, "Both", "Copy it and open the editor"),
                ],
                d.after,
                move |after| edit(&|s| s.after = after),
            ),
            None,
        ),
        form_row(
            "Image size",
            "Long edge for Copy and Save; AI chats shrink bigger ones anyway",
            segmented(
                &[
                    (1024, "1024", "Scale the long edge to 1024 px"),
                    (1568, "1568", "Scale the long edge to 1568 px"),
                    (2048, "2048", "Scale the long edge to 2048 px"),
                    (0, "Original", "Keep the original size"),
                ],
                d.max_edge,
                move |max_edge| edit(&|s| s.max_edge = max_edge),
            ),
            None,
        ),
        form_row(
            "Save as",
            "The file Save writes",
            segmented(
                &[
                    (Format::Png, "PNG", "Save PNG files (sharp text)"),
                    (Format::Jpeg, "JPEG", "Save JPEG files (smaller)"),
                ],
                d.format,
                move |format| edit(&|s| s.format = format),
            ),
            None,
        ),
        form_row(
            "Save size",
            "Copy always uses the image size",
            segmented(
                &[
                    (false, "Image size", "Save at the image size"),
                    (true, "Original", "Save at the original size"),
                ],
                d.save_original,
                move |original| edit(&|s| s.save_original = original),
            ),
            None,
        ),
        form_row(
            "Mouse pointer",
            "Draw the pointer into the capture",
            segmented(
                &[
                    (false, "Off", "Leave the pointer out"),
                    (true, "On", "Draw the pointer in"),
                ],
                d.cursor,
                move |cursor| edit(&|s| s.cursor = cursor),
            ),
            None,
        ),
        form_row(
            "Recent captures",
            "The last 20, on this PC only. Off deletes them",
            segmented(
                &[
                    (false, "Off", "Keep no captures"),
                    (true, "On", "Keep the last 20 captures"),
                ],
                d.history,
                move |history| edit(&|s| s.history = history),
            ),
            None,
        ),
    ]
}

fn about_rows<'a>() -> Vec<Element<'a, Message>> {
    vec![
        about::app(Message::OpenLink),
        about::developer(Message::OpenLink),
        about::notices(Message::OpenLink),
        form_row(
            "Telemetry",
            "A capture leaves this PC only when you choose an action",
            value_field("None", false),
            None,
        ),
    ]
}

fn shortcut_rows(state: &State) -> Vec<Element<'_, Message>> {
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
            let recording = state.recording == Some(slot);
            let error = state
                .shortcut_error
                .as_ref()
                .filter(|(s, _)| *s == slot)
                .map(|(_, e)| e.clone());
            // Never claim a key the desktop doesn't use.
            let note = (!recording).then(|| state.desktop_note(slot)).flatten();
            let error = error.or(note);
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
        if ai.provider == Provider::Browser {
            "Google AI Mode in OpenCircle's results card. No key"
        } else {
            "Who answers your questions"
        },
        segmented(
            &[
                (
                    Provider::Browser,
                    "Browser",
                    "Google AI Mode in the results card, no key",
                ),
                (Provider::Anthropic, "Anthropic", "Use Anthropic (Claude)"),
                (
                    Provider::OpenaiCompatible,
                    "OpenAI-like",
                    "Use an OpenAI-compatible server",
                ),
            ],
            ai.provider.clone(),
            Message::Provider,
        ),
        // An unknown value from the file: none is selected, Save waits for a pick.
        issue(&state.problems, "ai.provider"),
    )];
    if matches!(
        ai.provider,
        Provider::Browser | Provider::Invalid(_) | Provider::Unreadable
    ) {
        return rows; // no model, server or key: the browser, or nothing chosen yet
    }
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
                if compat && ai.base_url.contains("openrouter.ai") {
                    "e.g. anthropic/claude-haiku-5.5 (fast, paid)"
                } else if compat {
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

/// What song search sends, and when (also in docs/privacy.md).
pub const SONG_PRIVACY: &str = "When you press the music button, a ~10-second clip of the audio you choose is sent to AudD. Nothing is recorded at any other time.";

fn song_rows(state: &State) -> Vec<Element<'_, Message>> {
    let song = &state.draft.song;
    let mut rows = vec![form_row(
        "Service",
        "Free fingerprint lookups can't name a short clip, so this needs your own key",
        value_field(SERVICE.name(), false),
        None,
    )];
    if let Some(field) = &state.song_key {
        rows.push(form_row(
            "API token",
            "Sign up at dashboard.audd.io and paste your token. Kept in your system keyring",
            field.view().map(Message::SongKey),
            None,
        ));
    }
    rows.push(form_row(
        "Listen to",
        match song.source {
            SongSource::System => "What this PC plays: a video, a game, a stream",
            SongSource::Microphone => "Your microphone: music playing near you",
        },
        segmented(
            &[
                (
                    SongSource::System,
                    "This PC",
                    "Listen to what this PC plays",
                ),
                (
                    SongSource::Microphone,
                    "Microphone",
                    "Listen with the microphone",
                ),
            ],
            song.source,
            Message::SongSource,
        ),
        None,
    ));
    rows.push(form_row(
        "History",
        "Recent songs, kept only on this PC. Off deletes them",
        segmented(
            &[
                (false, "Off", "Keep no song history"),
                (true, "On", "Keep recent songs on this PC"),
            ],
            song.history,
            Message::SongHistory,
        ),
        None,
    ));
    rows.push(form_row(
        "Privacy",
        "",
        widgets::caption(SONG_PRIVACY),
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
    let save = label_button("Save", true, Some(Message::Save));
    row![
        status,
        space::horizontal(),
        tip(save, "Save settings", Position::Top)
    ]
    .align_y(iced::Center)
    .padding([16, 32])
    .into()
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::ui::widgets::tests_support::{assert_tips_add_something, shown};
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

    impl KeyStore<Service> for MemoryStore {
        fn is_set(&self, s: Service) -> Result<bool, SecretsError> {
            Ok(self.0.borrow().contains_key(s.key_entry()))
        }
        fn set(&self, s: Service, key: &Secret) -> Result<(), SecretsError> {
            self.0
                .borrow_mut()
                .insert(s.key_entry(), key.expose().to_string());
            Ok(())
        }
        fn delete(&self, s: Service) -> Result<(), SecretsError> {
            self.0.borrow_mut().remove(s.key_entry());
            Ok(())
        }
    }

    #[test]
    fn general_has_the_start_with_the_os_switch_on_by_default() {
        let path = tmp("autostart");
        let (mut s, _) = state(Some(path.clone()));
        assert!(s.draft.general.autostart);
        let texts = shot::texts(view(&s));
        assert!(texts.iter().any(|t| t == START_LABEL), "{texts:?}");
        update(&mut s, Message::Autostart(false));
        let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
            panic!("not saved: {:?}", s.problems);
        };
        assert!(!cfg.general.autostart);
        assert!(!Config::load_from(&path).0.general.autostart);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_browser_provider_says_it_opens_in_the_results_card() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Section(Section::Ai));
        update(&mut s, Message::Provider(Provider::Browser));
        let texts = shot::texts(view(&s));
        assert!(
            texts.iter().any(|t| t.contains("results card")),
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t.contains("in your browser")));
    }

    #[test]
    fn song_search_section_saves_a_write_only_token_and_its_choices() {
        let path = tmp("song");
        let (mut s, _) = state(Some(path.clone()));
        assert!(
            s.song_key.is_none(),
            "no keyring call before the section opens"
        );
        update(&mut s, Message::Section(Section::Song));
        let texts = shot::texts(view(&s));
        assert!(texts.iter().any(|t| t == SONG_PRIVACY), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("dashboard.audd.io")));
        // Write-only: once saved, the token is never shown again.
        update(
            &mut s,
            Message::SongKey(KeyMessage::Input(Secret::new("FAKE-audd-1"))),
        );
        update(&mut s, Message::SongKey(KeyMessage::Save));
        assert_eq!(s.song_key.as_ref().unwrap().status, KeyStatus::Saved);
        assert_eq!(music::key_known(), Some(true), "the music button shows");
        let texts = shot::texts(view(&s));
        assert!(!texts.iter().any(|t| t.contains("FAKE")), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Saved in your keyring"));
        shot::save("settings-song-saved", view(&s), WINDOW_SIZE);
        update(&mut s, Message::SongKey(KeyMessage::Remove));
        assert_eq!(music::key_known(), Some(false), "and hides again");
        update(&mut s, Message::SongSource(SongSource::Microphone));
        update(&mut s, Message::SongHistory(true));
        let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
            panic!("not saved: {:?}", s.problems);
        };
        assert_eq!(cfg.song.source, SongSource::Microphone);
        assert!(cfg.song.history);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
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
        update(&mut s, Message::Target(LANGUAGES[1]));
        update(&mut s, Message::Source(SOURCES[2]));
        update(&mut s, Message::DefaultAction(DefaultAction::Copy));
        update(&mut s, Message::Folder(Some(std::env::temp_dir())));
        update(&mut s, Message::LiveInterval("2500".into()));
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
        assert_eq!(back.translate.source, SOURCES[2].code);
        assert_eq!(back.translate.engine, Engine::Offline);
        assert_eq!(back.general.default_action, DefaultAction::Copy);
        assert_eq!(
            back.general.save_folder,
            std::env::temp_dir().to_string_lossy()
        );
        assert_eq!(back.translate.live_interval_ms, 2500);
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
    fn the_browser_provider_needs_no_key_model_or_server() {
        let (mut s, store) = state(None);
        update(&mut s, Message::Section(Section::Ai));
        assert_eq!(s.draft.ai.provider, Provider::Browser, "the default");
        let texts = shot::texts(view(&s));
        assert!(texts.iter().any(|t| t == "Provider"), "{texts:?}");
        for hidden in ["API key", "Model", "Server URL", "Preset"] {
            assert!(!texts.iter().any(|t| t == hidden), "{hidden}: {texts:?}");
        }
        // Back from a half-filled server form: hidden fields do not block Save.
        update(&mut s, Message::Provider(Provider::OpenaiCompatible));
        update(&mut s, Message::Provider(Provider::Browser));
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        assert!(store.0.borrow().is_empty(), "no keyring entry for it");
    }

    #[test]
    fn an_unknown_provider_blocks_save_until_one_is_picked() {
        let path = tmp("bad-provider");
        let (config, warnings) = Config::parse("[ai]\nprovider = \"antropic\"\n");
        let store = Box::new(MemoryStore::default());
        let mut s = State::new(config, warnings, Some(path.clone()), store);
        update(&mut s, Message::Section(Section::Ai));
        let texts = shot::texts(view(&s));
        assert!(
            texts
                .iter()
                .any(|t| t.contains("\"antropic\" is not browser")),
            "{texts:?}"
        );
        for hidden in ["API key", "Model", "Server URL"] {
            assert!(!texts.iter().any(|t| t == hidden), "{hidden}: {texts:?}");
        }
        // Save never writes "browser" (or anything) over it on its own.
        update(&mut s, Message::Save);
        assert!(!path.exists(), "saved with an unknown provider");
        update(&mut s, Message::Provider(Provider::Anthropic));
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("provider = \"anthropic\""), "{text}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_unreadable_file_keeps_ai_off_until_a_provider_is_picked() {
        let path = tmp("unreadable");
        let (config, warnings) = Config::parse("theme = = =");
        let store = Box::new(MemoryStore::default());
        let mut s = State::new(config, warnings, Some(path.clone()), store);
        let notice = s.notice.clone().unwrap();
        assert!(
            !notice.contains("reset") && notice.contains("AI is off"),
            "{notice}"
        );
        update(&mut s, Message::Section(Section::Ai));
        let texts = shot::texts(view(&s));
        assert!(
            texts.iter().any(|t| t.contains("couldn't be read")),
            "{texts:?}"
        );
        assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
        update(&mut s, Message::Save);
        assert!(!path.exists(), "saved a guessed provider");
        update(&mut s, Message::Provider(Provider::Browser));
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("provider = \"browser\""), "{text}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_api_key_is_write_only() {
        let (mut s, store) = state(None);
        update(&mut s, Message::Provider(Provider::Anthropic));
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
        for (section, ..) in SECTIONS {
            update(&mut s, Message::Section(section));
            TIPS.with(|t| t.borrow_mut().clear());
            let _ = view(&s);
            all.extend(TIPS.with(|t| t.borrow().clone()));
        }
        // Remove the sidebar labels counted once per section.
        all.sort();
        let sidebar = SECTIONS.map(|(_, _, hint)| hint);
        for name in sidebar {
            assert_eq!(
                all.iter().filter(|t| *t == name).count(),
                SECTIONS.len(),
                "{name}"
            );
        }
        all.retain(|t| !sidebar.contains(&t.as_str()) && t != "Save settings");
        let mut unique = all.clone();
        unique.dedup();
        assert_eq!(all, unique, "duplicate control labels");
        assert!(all.len() >= 24, "{all:?}");
    }

    #[test]
    fn no_visible_tooltip_repeats_its_control() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Provider(Provider::OpenaiCompatible));
        for (section, name, _) in SECTIONS {
            update(&mut s, Message::Section(section));
            let _ = view(&s);
            assert_tips_add_something(name);
        }
        // Text that says what it is shows no tooltip: Settings has none by default.
        update(&mut s, Message::Section(Section::General));
        let _ = view(&s);
        assert_eq!(shown().len(), 0, "{:?}", shown());
        // A save folder cut short does: the whole path.
        let long = "/a".repeat(40);
        update(&mut s, Message::Folder(Some(PathBuf::from(&long))));
        let _ = view(&s);
        assert_eq!(shown(), [long]);
        assert_tips_add_something("a cut folder");
    }

    #[test]
    fn general_section_edits_default_action_and_save_folder() {
        let path = tmp("general");
        let (mut s, _) = state(Some(path.clone()));
        assert_eq!(s.section, Section::General, "the first section opens");
        assert_eq!(s.draft.general.default_action, DefaultAction::Lens);
        update(&mut s, Message::DefaultAction(DefaultAction::Text));
        // Browse asks the shell for the picker; nothing changes until it answers.
        assert_eq!(update(&mut s, Message::BrowseFolder), Outcome::Browse);
        assert_eq!(s.draft.general.save_folder, "");
        update(&mut s, Message::Folder(None));
        assert_eq!(s.draft.general.save_folder, "", "cancelled");
        let dir = std::env::temp_dir();
        update(&mut s, Message::Folder(Some(dir.clone())));
        assert_eq!(s.draft.general.save_folder, dir.to_string_lossy());
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
            panic!("not saved: {:?}", s.notice)
        };
        assert_eq!(cfg.general.default_action, DefaultAction::Text);
        assert_eq!(
            Config::load_from(&path).0.general.save_folder,
            dir.to_string_lossy()
        );
        update(&mut s, Message::ClearFolder);
        assert_eq!(s.draft.general.save_folder, "");
        // The control shows the choice (a long path is cut in the middle).
        update(
            &mut s,
            Message::Folder(Some(PathBuf::from("/a".repeat(40)))),
        );
        let texts = shot::texts(view(&s));
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with('…') && t.chars().count() == 38),
            "{texts:?}"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn live_interval_is_checked_against_its_range() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Section(Section::Translate));
        assert_eq!(s.interval, "1000");
        for ok in ["500", "1000", "5000", " 2500 "] {
            update(&mut s, Message::LiveInterval(ok.into()));
            assert!(s.problems.is_empty(), "{ok}: {:?}", s.problems);
        }
        for bad in ["499", "5001", "", "fast", "-3"] {
            update(&mut s, Message::LiveInterval(bad.into()));
            let why = issue(&s.problems, "translate.live_interval_ms");
            assert!(why.is_some_and(|w| w.contains("500 to 5000")), "{bad}");
            assert_eq!(s.interval, bad, "the field keeps what was typed");
            assert_eq!(update(&mut s, Message::Save), Outcome::None, "{bad}");
        }
        assert_eq!(*crate::config::LIVE_INTERVAL_MS.start(), 500);
    }

    #[test]
    fn engine_choice_says_what_offline_means() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Section(Section::Translate));
        let texts = shot::texts(view(&s));
        for want in [
            "Engine",
            "AI",
            "Offline",
            "Your AI provider; offline when it isn't set up",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        update(&mut s, Message::Engine(Engine::Offline));
        let texts = shot::texts(view(&s));
        let honest = "No key, nothing leaves your PC; more literal than AI";
        assert!(texts.iter().any(|t| t == honest), "{texts:?}");
        // Source and Target; a pair with no offline model says so
        for want in ["Source language", "Target language"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let gap = "No offline model for this pair: choose AI to translate it";
        assert!(!texts.iter().any(|t| t == gap));
        update(&mut s, Message::Source(lang::find("fr").copied().unwrap()));
        let texts = shot::texts(view(&s));
        assert!(
            texts.iter().any(|t| t == gap),
            "fr -> ar offline: {texts:?}"
        );
        shot::save("settings-translate", view(&s), Size::new(860.0, 600.0));
        // a saved "offline" is read as is, without a warning
        let (cfg, warnings) = Config::parse("[translate]\nengine = \"offline\"\n");
        let s = State::new(cfg, warnings, None, Box::new(MemoryStore::default()));
        assert_eq!(s.draft.translate.engine, Engine::Offline);
        assert!(s.notice.is_none(), "{:?}", s.notice);
    }

    #[test]
    fn snip_section_edits_the_snip_choices() {
        let dir = tmp("snip-section");
        let path = dir.join("config.toml");
        let (mut s, _) = state(Some(path.clone()));
        update(&mut s, Message::Section(Section::Snip));
        let texts = shot::texts(view(&s));
        for label in [
            "After a capture",
            "Image size",
            "Save as",
            "Recent captures",
        ] {
            assert!(texts.iter().any(|t| t == label), "{label}: {texts:?}");
        }
        let mut snip = s.draft.snip.clone();
        assert!(!snip.history, "off until the user turns it on");
        snip.after = SnipAfter::Both;
        snip.max_edge = 0;
        snip.format = Format::Jpeg;
        snip.history = true;
        update(&mut s, Message::Snip(snip.clone()));
        assert_eq!(s.draft.snip, snip);
        assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
        let (back, w) = Config::load_from(&path);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back.snip, snip);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn general_switches_updates_and_checks_now() {
        let path = tmp("update");
        let (mut s, _) = state(Some(path.clone()));
        update(&mut s, Message::Section(Section::General));
        assert_eq!(update(&mut s, Message::CheckUpdate), Outcome::CheckUpdate);
        assert!(shot::texts(view(&s)).iter().any(|t| t == "Checking…"));
        update(
            &mut s,
            Message::UpdateStatus("OpenCircle 1.0.0 is up to date".into()),
        );
        assert!(
            shot::texts(view(&s))
                .iter()
                .any(|t| t == "OpenCircle 1.0.0 is up to date")
        );
        update(&mut s, Message::AutoUpdate(false));
        let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
            panic!("saved");
        };
        assert_eq!(cfg.update.auto, Some(false));
        assert_eq!(Config::load_from(&path).0.update.auto, Some(false));
    }

    #[test]
    fn about_shows_version_license_project_and_no_telemetry() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Section(Section::About));
        let texts = shot::texts(view(&s));
        for want in ["MIT licence", "Telemetry", "None", "Amer Alreyahi"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
        assert!(texts.contains(&version), "{texts:?}");
        // Nothing opens until a link is clicked; the click asks the shell to open it.
        let url = about::PROJECT.to_string();
        assert_eq!(
            update(&mut s, Message::OpenLink(url.clone())),
            Outcome::OpenLink(url)
        );
        assert!(!s.dirty(), "About changes nothing");
    }

    #[test]
    fn shortcuts_show_the_key_the_desktop_actually_uses() {
        let (mut s, _) = state(None);
        s.desktop = |keys| match keys {
            "Ctrl+Shift+Space" => Some(DesktopKey::Other("Super+O".into())),
            "Ctrl+Shift+E" => Some(DesktopKey::Unassigned),
            _ => None,
        };
        update(&mut s, Message::Section(Section::Shortcuts));
        let notes = |s: &State| -> Vec<String> {
            let texts = shot::texts(view(s));
            texts
                .into_iter()
                .filter(|t| t.ends_with(managed()))
                .collect()
        };
        assert_eq!(
            notes(&s),
            [
                format!("In use: Super+O. {}", managed()),
                format!("No key in use. {}", managed()),
            ]
        );
        // A key changed but not saved yet: the desktop only knows the saved one.
        update(&mut s, Message::Record(Slot::Capture));
        assert_eq!(notes(&s).len(), 1, "no note while recording");
        press(&mut s, chord(true, true, "F7"));
        assert_eq!(s.draft.shortcut, "Ctrl+Shift+F7");
        assert_eq!(notes(&s), [format!("No key in use. {}", managed())]);
        // Nothing to say (not Wayland, or the desktop uses our keys): no note at all.
        s.desktop = |_| None;
        assert!(notes(&s).is_empty());
    }

    #[test]
    fn snip_shortcuts_are_listed_and_editable() {
        let (mut s, _) = state(None);
        update(&mut s, Message::Section(Section::Shortcuts));
        let texts = shot::texts(view(&s));
        for want in ["Snip a region", "Snip a window", "Snip the full screen"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let names: Vec<_> = s.quick_text.iter().map(|t| t.label.as_str()).collect();
        for want in ["Snip a region", "Snip a window", "Snip the full screen"] {
            assert!(names.contains(&want), "{names:?}");
        }
        let i = s
            .draft
            .quick_actions
            .iter()
            .position(|q| q.shortcut == "Alt+PrintScreen")
            .unwrap();
        update(&mut s, Message::Record(Slot::Quick(i)));
        press(&mut s, chord(false, false, "F8"));
        assert_eq!(s.draft.quick_actions[i].shortcut, "F8");
        // Print Screen is a valid lone key but each key still does one job.
        update(&mut s, Message::Record(Slot::Quick(i)));
        let key = Chord {
            ctrl: false,
            alt: false,
            shift: false,
            logo: false,
            key: "PrintScreen".into(),
        };
        press(&mut s, key);
        assert!(
            s.shortcut_error
                .as_ref()
                .unwrap()
                .1
                .starts_with("Already used")
        );
    }

    #[test]
    fn look_and_behaviour_edits_the_selection_and_the_custom_look() {
        let path = tmp("look");
        let (mut s, _) = state(Some(path.clone()));
        update(&mut s, Message::Section(Section::Look));
        let texts = shot::texts(view(&s));
        for want in [
            "Active look",
            "Spectrum",
            "Neon",
            "Calm",
            "Custom",
            "Snap to box",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        assert!(
            !texts.iter().any(|t| t == "Edge style"),
            "a preset has no custom rows"
        );
        assert_eq!(s.previews.len(), LOOKS.len());
        // every preset's preview differs from the others
        for (i, a) in s.previews.iter().enumerate().take(7) {
            for b in &s.previews[i + 1..7] {
                assert_ne!(a.id(), b.id());
                let (Handle::Rgba { pixels: a, .. }, Handle::Rgba { pixels: b, .. }) = (a, b)
                else {
                    panic!("rgba previews")
                };
                assert_ne!(a, b);
            }
        }
        update(&mut s, Message::SelectStyle(SelectStyle::Shape));
        update(&mut s, Message::After(After::Wait));
        update(&mut s, Message::SearchWith(SearchWith::Both));
        // Custom starts as the look in use (Neon here), then changes on its own
        update(&mut s, Message::Look(LookTheme::Neon));
        update(&mut s, Message::Look(LookTheme::Custom));
        assert_eq!(
            s.draft.look.custom,
            LookTheme::Neon.look(&s.draft.look.custom)
        );
        let before = s.previews.last().unwrap().id();
        update(&mut s, Message::Glow(55));
        update(&mut s, Message::Dim(30));
        update(&mut s, Message::EdgeMode(EdgeMode::Live));
        update(&mut s, Message::EdgeStyle(EdgeStyle::Thin));
        update(&mut s, Message::Thickness(9));
        update(&mut s, Message::Animate(false));
        update(&mut s, Message::Bar(BarChoice(BarStyle::Solid)));
        update(
            &mut s,
            Message::Palette(PaletteChoice(ColourSet::TwoColour)),
        );
        update(&mut s, Message::Picker(1));
        update(&mut s, Message::Hex("#12345".into())); // not a colour yet: kept as typed
        update(&mut s, Message::Hex("#123456".into()));
        update(&mut s, Message::Picker(0));
        update(&mut s, Message::PickColour(Color::from_rgb8(255, 0, 0)));
        assert_ne!(
            s.previews.last().unwrap().id(),
            before,
            "the preview follows"
        );
        let texts = shot::texts(view(&s));
        for want in [
            "Edge style",
            "Glow strength",
            "Your two colours",
            "Bars and pills",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
            panic!("not saved: {:?}", s.notice)
        };
        let (back, w) = Config::load_from(&path);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back, cfg);
        let look = back.look.active();
        assert_eq!((look.glow, look.dim, look.bar), (55, 30, BarStyle::Solid));
        assert_eq!(look.live.style, EdgeStyle::Thin);
        assert_eq!((look.live.thickness, look.live.animate), (9, false));
        assert_eq!(
            look.overlay,
            LookTheme::Neon.look(&look).overlay,
            "other modes kept"
        );
        assert_eq!(look.colours, [Rgb([255, 0, 0]), Rgb([0x12, 0x34, 0x56])]);
        assert_eq!(back.selection.search_with, SearchWith::Both);
        // the page in both themes, a preset and the custom look
        for (name, theme_choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
            update(&mut s, Message::Theme(theme_choice));
            assert_eq!(s.picker, Some(0), "the picker is open");
            shot::save(
                &format!("settings-look-custom-{name}"),
                view(&s),
                Size::new(820.0, 1640.0),
            );
        }
        theme::apply(Theme::Dark);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn renders_about_in_default_and_frost() {
        for widgets_theme in [WidgetTheme::Default, WidgetTheme::Frost] {
            for (name, theme_choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
                let (mut s, _) = state(None);
                update(&mut s, Message::Theme(theme_choice));
                update(&mut s, Message::WidgetTheme(widgets_theme));
                update(&mut s, Message::Section(Section::About));
                let w = format!("{widgets_theme:?}").to_lowercase();
                shot::save(
                    &format!("about-{w}-{name}"),
                    view(&s),
                    Size::new(820.0, 1450.0),
                );
            }
        }
        theme::apply(Theme::Dark);
    }

    /// Every control is FORM_WIDTH wide and every button in a group fills an equal
    /// share, so no row can drift from the column (owner, 2026-10-25).
    #[test]
    fn controls_and_button_rows_share_the_one_column_width() {
        use crate::ui::widgets::{button_row, colour_swatch, form_button};
        let width = |e: &Element<'_, Message>| e.as_widget().size().width;
        let col = Length::Fixed(FORM_WIDTH);
        let b = |l| form_button(l, false, Some(Message::Save)).into();
        let two = button_row([b("Browse…"), b("Reset")]);
        let one = button_row([b("Check now")]);
        assert_eq!(width(&two), col);
        assert_eq!(width(&one), col);
        for e in [
            b("x"),
            colour_swatch(Color::WHITE, false, Message::Save).into(),
        ] {
            assert_eq!(width(&e), Length::Fill, "a group button fills its share");
        }
        let seg: Element<'_, Message> = segmented(&[(1, "a", "a")], 1, |_| Message::Save);
        assert_eq!(width(&seg), col);
        let val: Element<'_, Message> = value_field("v", false).into();
        assert_eq!(width(&val), col);
        let txt: Element<'_, Message> = text_field("p", "", false, |_| Message::Save).into();
        assert_eq!(width(&txt), col);
        let sl: Element<'_, Message> = slider(0..=10, 1, "", |_| Message::Save);
        assert_eq!(width(&sl), col);
        let (s, _) = state(None);
        assert_eq!(width(&s.key.view().map(Message::ApiKey)), col);
        // No page builds its own fixed-width button: only the footer's Save.
        let src = include_str!("settings.rs");
        let body = &src[..src.find("\n#[cfg(test)]").unwrap()];
        assert_eq!(body.matches("label_button(").count(), 1);
    }

    #[test]
    fn save_is_never_a_dead_button() {
        let (mut s, _) = state(None);
        assert_eq!(update(&mut s, Message::Save), Outcome::None);
        assert_eq!(s.notice.as_deref(), Some("Nothing to save"));
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
                let slug = label.to_lowercase().replace(" & ", "-");
                let file = format!("settings-{slug}-{name}");
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
            // General in full: Save folder and Check now sit below the fold.
            update(&mut s, Message::Section(Section::General));
            shot::save(
                &format!("settings-general-tall-{name}"),
                view(&s),
                Size::new(820.0, 900.0),
            );
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
