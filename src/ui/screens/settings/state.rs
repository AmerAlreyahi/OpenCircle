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
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use iced::{Color, Size, Subscription, event, keyboard};

use crate::config::{
    self, After, BarStyle, BracketColour, BracketStyle, ColourSet, Config, DefaultAction, EdgeMode,
    EdgeStyle, Engine, Look, LookConfig, LookTheme, Provider, Rgb, SearchWith, SelectStyle,
    SnipConfig, SongSource, Target, TextReader, Theme, parse_shortcut,
};
use crate::hotkey::{DesktopKey, managed};
use crate::labs::Feature;
use crate::music::{self, SERVICE, Service};
use crate::secrets::{KeyStore, Keyring, Secret, SecretsError};
use crate::translate::lang::{self, LANGUAGES, Language};
use crate::ui::kit::header::Caption;
use crate::ui::kit::hero;
use crate::ui::kit::motion::{self, Curve};
use crate::ui::kit::themes::THEMES;
use crate::ui::kit::tokens::{size, space};
use crate::ui::theme;

use super::pages::text_reading::open_language_settings;
use super::pages::{self, PAGES};
use super::previews;

/// The size the Settings window prefers (logical px), the design's 820 x 560.
/// Screenshots and tests render at this size.
pub const WINDOW_SIZE: Size = Size::new(820.0, 560.0);

/// The sidebar's footer under its list: a 2 px gap, the hairline, 8 px and a
/// 10.5 px line (`kit::sidebar`).
const SIDEBAR_FOOTER: f32 = 2.0 + 1.0 + 8.0 + 16.0;

/// The height that shows the header and every sidebar page (Labs on).
pub const FIT_HEIGHT: f32 =
    size::HEADER + 2.0 * space::SIDEBAR_PAD + PAGES.len() as f32 * (size::NAV_ITEM + 2.0) - 2.0
        + SIDEBAR_FOOTER;
/// The narrowest layout that keeps the label column readable beside a control.
pub(super) const MIN_WIDTH: f32 = 760.0;

/// The opening size and the smallest size for a screen whose usable area is `work`
/// (logical px): the preferred size, kept clear of the screen's edges and the taskbar,
/// never below what fits the sidebar unless the screen itself is smaller (the sidebar
/// then scrolls).
pub fn window_limits(work: Size) -> (Size, Size) {
    let room = Size::new(work.width * 0.92, work.height * 0.96);
    let size = Size::new(
        WINDOW_SIZE.width.min(room.width),
        WINDOW_SIZE.height.min(room.height),
    );
    let min = Size::new(MIN_WIDTH.min(size.width), FIT_HEIGHT.min(size.height));
    (size, min)
}

/// A key store bound to one owner (a provider or a service), so the field and
/// its worker-thread jobs need not know which kind it is.
pub trait Bound: Send + Sync {
    fn is_set(&self) -> Result<bool, SecretsError>;
    fn set(&self, key: &Secret) -> Result<(), SecretsError>;
    fn delete(&self) -> Result<(), SecretsError>;
}

pub(super) struct Of<K>(Arc<dyn KeyStore<K>>, K);

impl<K: Clone + Send + Sync> Bound for Of<K> {
    fn is_set(&self) -> Result<bool, SecretsError> {
        self.0.is_set(self.1.clone())
    }
    fn set(&self, key: &Secret) -> Result<(), SecretsError> {
        self.0.set(self.1.clone(), key)
    }
    fn delete(&self) -> Result<(), SecretsError> {
        self.0.delete(self.1.clone())
    }
}

pub fn bind<K: Clone + Send + Sync + 'static>(
    store: &Arc<dyn KeyStore<K>>,
    owner: K,
) -> Arc<dyn Bound> {
    Arc::new(Of(store.clone(), owner))
}

/// Song search's key store: tests get an in-memory one (never the real keyring).
pub(super) fn song_keyring() -> Arc<dyn KeyStore<Service>> {
    #[cfg(test)]
    return Arc::<crate::secrets::MemoryStore>::default();
    #[cfg(not(test))]
    Arc::new(Keyring)
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
    /// A `KeyJob`'s answer (tagged with the job's number; an old one is ignored).
    Done(u32, KeyDone),
}

/// What a keyring call answered: the status now, or why it failed. Never the key.
#[derive(Debug, Clone)]
pub enum KeyDone {
    Status(KeyStatus),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Busy {
    Checking,
    Saving,
    Removing,
}

impl Busy {
    pub(super) fn label(self) -> &'static str {
        match self {
            Busy::Checking => "Checking…",
            Busy::Saving => "Saving…",
            Busy::Removing => "Removing…",
        }
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum Work {
    Check,
    Save(Secret),
    Remove,
}

/// A keyring call for a worker thread: `run` it there and send the answer back
/// as `KeyMessage::Done`, so the UI never waits on the keyring.
#[derive(Clone)]
pub struct KeyJob {
    slot: Arc<dyn Bound>,
    work: Work,
    id: u32,
}

impl KeyJob {
    pub fn run(self) -> KeyMessage {
        let done = match self.work {
            Work::Check => KeyDone::Status(match self.slot.is_set() {
                Ok(true) => KeyStatus::Saved,
                Ok(false) => KeyStatus::NotSet,
                Err(e) => KeyStatus::Unavailable(e.to_string()),
            }),
            Work::Save(key) => match self.slot.set(&key) {
                Ok(()) => KeyDone::Status(KeyStatus::Saved),
                Err(e) => KeyDone::Failed(e.to_string()),
            },
            Work::Remove => match self.slot.delete() {
                Ok(()) => KeyDone::Status(KeyStatus::NotSet),
                Err(e) => KeyDone::Failed(e.to_string()),
            },
        };
        KeyMessage::Done(self.id, done)
    }
}

impl fmt::Debug for KeyJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyJob({})", self.id)
    }
}

#[derive(Debug)]
pub struct KeyField {
    pub(super) status: KeyStatus,
    pub(super) input: Secret,
    pub(super) replacing: bool,
    pub(super) error: Option<String>,
    pub(super) labels: KeyLabels,
    /// The keyring call in flight, and the number of the latest one.
    pub(super) busy: Option<Busy>,
    pub(super) job: u32,
}

/// A saved key as the field shows it (owner, 2026-10-28): dots only, the
/// same for every key. Nothing of the key is read to draw it (only that one
/// exists: `Bound::is_set`), and there is no way to reveal it.
pub const KEY_MASK: &str = "••••••••••••";

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

pub(super) const API_KEY: KeyLabels = KeyLabels {
    placeholder: "Paste your API key",
    field: "API key",
    save: "Save the API key",
    replace: "Replace the API key",
    remove: "Remove the API key",
    keep: "Keep the saved API key",
};

pub(super) const SONG_KEY: KeyLabels = KeyLabels {
    placeholder: "Paste your AudD API token",
    field: "AudD API token",
    save: "Save the AudD token",
    replace: "Replace the AudD token",
    remove: "Remove the AudD token",
    keep: "Keep the saved AudD token",
};

impl KeyField {
    /// A field whose status is still being asked: run the job.
    pub fn pending(slot: Arc<dyn Bound>) -> (Self, KeyJob) {
        let mut f = KeyField {
            status: KeyStatus::NotSet,
            input: Secret::new(""),
            replacing: false,
            error: None,
            labels: API_KEY,
            busy: None,
            job: 0,
        };
        let job = f.refresh(slot);
        (f, job)
    }

    /// Asks the keyring on this thread (a window opening); `update` never does.
    pub fn new(slot: Arc<dyn Bound>) -> Self {
        let (mut f, job) = Self::pending(slot.clone());
        f.update(slot, job.run());
        f
    }

    pub fn labelled(mut self, labels: KeyLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Ask the keyring again (after the provider changed).
    pub fn refresh(&mut self, slot: Arc<dyn Bound>) -> KeyJob {
        self.input = Secret::new("");
        self.replacing = false;
        self.error = None;
        self.start(slot, Busy::Checking, Work::Check)
    }

    pub(super) fn start(&mut self, slot: Arc<dyn Bound>, busy: Busy, work: Work) -> KeyJob {
        self.job += 1;
        self.busy = Some(busy);
        KeyJob {
            slot,
            work,
            id: self.job,
        }
    }

    #[cfg(test)]
    pub fn status(&self) -> &KeyStatus {
        &self.status
    }

    /// True while the keyring is being asked (the status is not known yet).
    pub fn busy(&self) -> bool {
        self.busy.is_some()
    }

    /// Save / Remove start a job for the caller to run off the UI thread.
    pub fn update(&mut self, slot: Arc<dyn Bound>, message: KeyMessage) -> Option<KeyJob> {
        self.error = None;
        match message {
            KeyMessage::Input(key) => self.input = key,
            KeyMessage::Replace => self.replacing = true,
            KeyMessage::Cancel => {
                self.replacing = false;
                self.input = Secret::new("");
            }
            KeyMessage::Save | KeyMessage::Remove if self.busy.is_some() => {}
            KeyMessage::Save if self.input.is_empty() => {
                self.error = Some("Paste your key first".into())
            }
            KeyMessage::Save => {
                let work = Work::Save(self.input.clone());
                return Some(self.start(slot, Busy::Saving, work));
            }
            KeyMessage::Remove => return Some(self.start(slot, Busy::Removing, Work::Remove)),
            KeyMessage::Done(id, done) if id == self.job => {
                let was = self.busy.take();
                match done {
                    KeyDone::Status(status) => {
                        if was == Some(Busy::Saving) {
                            self.replacing = false;
                            self.input = Secret::new("");
                        }
                        self.status = status;
                    }
                    KeyDone::Failed(e) => self.error = Some(e),
                }
            }
            KeyMessage::Done(..) => {} // the provider changed meanwhile
        }
        None
    }
}

// ---- data: presets, languages, action names ----

/// The API a provider speaks: chosen here, never shown (owner, 2026-10-28:
/// protocol names mislead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    /// The Anthropic Messages API (`provider = "anthropic"`).
    Anthropic,
    /// The OpenAI-compatible client (`provider = "openai_compatible"`).
    OpenAi,
}

/// An AI provider in the Provider dropdown, by its real name: what it writes
/// to `[ai]` (the config format is unchanged: `provider` and `base_url`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub api: Api,
    /// "" for Anthropic (its default) and for Custom server (typed by the user).
    pub base_url: &'static str,
    /// Suggested model, used only while the model field is empty.
    pub model: &'static str,
    /// Runs on this machine: no API key needed.
    pub local: bool,
}

impl Preset {
    /// The provider this preset writes.
    pub fn provider(&self) -> Provider {
        match self.api {
            Api::Anthropic => Provider::Anthropic,
            Api::OpenAi => Provider::OpenaiCompatible,
        }
    }

    /// Custom server: the one that shows the Server URL row.
    pub fn custom(&self) -> bool {
        *self == CUSTOM
    }

    /// The preset a stored `[ai]` shows as (`None`: the browser, or a value
    /// the file can't give): Anthropic; an OpenAI-compatible server by its
    /// URL (a trailing `/` or the letter case aside); any other URL is a
    /// Custom server.
    pub fn of(ai: &crate::config::AiConfig) -> Option<Preset> {
        let same = |a: &str, b: &str| a.trim_end_matches('/').eq_ignore_ascii_case(b);
        match ai.provider {
            Provider::Anthropic => Some(ANTHROPIC),
            Provider::OpenaiCompatible => Some(
                (PRESETS.iter())
                    .find(|p| p.api == Api::OpenAi && !p.custom() && same(&ai.base_url, p.base_url))
                    .copied()
                    .unwrap_or(CUSTOM),
            ),
            _ => None,
        }
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const fn openai(name: &'static str, base_url: &'static str, model: &'static str) -> Preset {
    Preset {
        name,
        api: Api::OpenAi,
        base_url,
        model,
        local: false,
    }
}

const fn local(name: &'static str, base_url: &'static str, model: &'static str) -> Preset {
    Preset {
        local: true,
        ..openai(name, base_url, model)
    }
}

pub const ANTHROPIC: Preset = Preset {
    name: "Anthropic",
    api: Api::Anthropic,
    base_url: "",
    model: crate::ai::DEFAULT_ANTHROPIC_MODEL,
    local: false,
};

pub const CUSTOM: Preset = openai("Custom server", "", "");

/// The Provider dropdown, in the owner's order (2026-10-28).
pub const PRESETS: &[Preset] = &[
    // Free, correct Arabic, ~3 s when measured. Fast paid option: anthropic/claude-haiku-5.5
    // (shown as the placeholder while the model field is empty).
    openai(
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        "nvidia/nemotron-3-ultra-550b-a55b:free",
    ),
    openai("OpenAI", "https://api.openai.com/v1", "gpt-4o-mini"),
    ANTHROPIC,
    openai(
        "Google Gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "gemini-2.0-flash",
    ),
    openai(
        "Groq",
        "https://api.groq.com/openai/v1",
        "meta-llama/llama-4-scout-17b-16e-instruct",
    ),
    openai(
        "Mistral",
        "https://api.mistral.ai/v1",
        "mistral-small-latest",
    ),
    openai("DeepSeek", "https://api.deepseek.com/v1", "deepseek-chat"),
    openai("xAI", "https://api.x.ai/v1", "grok-4"),
    local(
        "Ollama (local)",
        "http://localhost:11434/v1",
        "llama3.2-vision",
    ),
    local(
        "LM Studio (local)",
        "http://localhost:1234/v1",
        "local-model",
    ),
    CUSTOM,
];

/// The Source dropdown: "Detect automatically", then what OCR can read.
pub(super) static SOURCES: LazyLock<Vec<Language>> = LazyLock::new(|| {
    let auto = Language {
        code: lang::AUTO,
        name: "Detect automatically",
        ..LANGUAGES[1]
    };
    let readable = LANGUAGES.iter().filter(|l| l.ocr.is_some()).copied();
    std::iter::once(auto).chain(readable).collect()
});

pub(super) fn action_name(id: &str, live: bool, target: Target) -> String {
    match (id, live) {
        ("snip", _) => match target {
            Target::Window => "Snip a window".into(),
            Target::FullScreen => "Snip the full screen".into(),
            _ => "Snip a region".into(),
        },
        ("translate", false) => "Translate".into(),
        ("translate", true) => "Translate (live, Beta)".into(),
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

pub(super) fn target_name(t: Target) -> &'static str {
    match t {
        Target::FullScreen => "Whole screen",
        Target::Circle => "The area you circle",
        Target::LastRegion => "The last region",
        Target::Window => "The window you click",
    }
}

/// The hint under a quick action's name: snip shows what the mode does.
pub(super) fn quick_hint(q: &crate::config::QuickAction) -> &'static str {
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
    pub(super) fn from_iced(
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

    pub(super) fn bare(&self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.logo)
    }

    /// "Ctrl+Shift+T" for `parse_shortcut`; `None` for a lone modifier key.
    pub(super) fn shortcut(&self) -> Option<String> {
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
    TextReading,
    Translate,
    Song,
    Privacy,
    Labs,
    About,
}

/// What the update switch says it does (Settings and onboarding).
pub const UPDATE_PRIVACY: &str = "Asks GitHub for the latest version twice a day and installs it when \
     OpenCircle is idle. Sends only that request: nothing about you or your screen";

/// A shortcut field: the capture shortcut or quick action `n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Capture,
    Quick(usize),
}

pub(super) struct SlotText {
    pub(super) label: String,
    pub(super) hint: String,
    pub(super) tip: String,
}

pub struct State {
    pub(super) section: Section,
    pub(super) draft: Config,
    pub(super) saved: Config,
    /// `None` = the real config path.
    pub(super) path: Option<PathBuf>,
    pub(super) store: Arc<dyn KeyStore>,
    pub(super) recording: Option<Slot>,
    pub(super) shortcut_error: Option<(Slot, String)>,
    pub(super) key: KeyField,
    /// Song search's key: the store, and its field once the section was opened
    /// (the keyring is asked only then).
    pub(super) song_store: Arc<dyn KeyStore<Service>>,
    pub(super) song_key: Option<KeyField>,
    /// Keyring calls started by the last message; the shell runs them off the UI
    /// thread (`take_key_jobs`) and sends each answer back as a `KeyMessage::Done`.
    pub(super) key_jobs: Vec<(bool, KeyJob)>,
    pub(super) quick_text: Vec<SlotText>,
    /// What the live-interval field shows (kept as typed).
    pub(super) interval: String,
    /// "key: reason" lines from `Config`'s own validation of the draft.
    pub(super) problems: Vec<String>,
    pub(super) notice: Option<String>,
    /// The desktop's own say on a running key (`hotkey::desktop_key`; tests swap it).
    pub(super) desktop: fn(&str) -> Option<DesktopKey>,
    /// Look & behaviour: whose screen edge the custom look edits, which of its
    /// two colours has the picker open (and the hex typed there).
    pub(super) edge_mode: EdgeMode,
    pub(super) picker: Option<usize>,
    pub(super) hex: String,
    /// The last Check now's result (`None` = not checked in this window).
    pub(super) update_status: Option<String>,
    /// Windows OCR's recognizers and the Windows display language, asked when
    /// the window opens (#63; empty off Windows).
    pub(super) recognizers: Vec<String>,
    pub(super) profile: Option<String>,
    /// The window was asked to close with unsaved changes: the footer asks
    /// Save / Discard / Keep editing (#136).
    pub(super) asking_close: bool,
    /// Save or Discard answered: the shell closes the window now.
    pub(super) leaving: bool,
    /// Every valid change is written at once (the real window; `State::new`
    /// keeps it off so tests drive `Message::Save` themselves).
    pub(super) auto: bool,
    /// When the last auto-save happened: the pill says "Saved" for `SAVED_FOR`.
    pub(super) saved_at: Option<Instant>,
    /// A write is on its way (one at a time): later saves wait for its answer.
    pub(super) writing: bool,
    /// Labs switches flipped while a write was on its way, written in turn.
    pub(super) queued_labs: Vec<(Feature, bool)>,
    /// Close was answered with Save: the shell closes once the write lands.
    pub(super) closing: bool,
    /// The last page change (the page it left, and when): the sidebar's
    /// highlight glides and the page rises.
    pub(super) moved: Option<(Section, Instant)>,
    /// The last pick in a ported segmented control (its old value, and when):
    /// its thumb glides.
    pub(super) picked: Option<(Pick, Instant)>,
    /// When the page's feature preview began playing (`previews`); `None` =
    /// at rest on its last frame.
    pub(super) played: Option<Instant>,
}

/// A segmented control's value before a pick (`State::thumb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Default(DefaultAction),
    Autostart(bool),
    History(bool),
    Update(bool),
    Lab(Feature, bool),
    Engine(Engine),
    SongSource(SongSource),
    SongHistory(bool),
    Select(SelectStyle),
    After(After),
    SearchWith(SearchWith),
    Reader(TextReader),
}

/// How long the footer pill says "Saved" after a change (the design's 1.8 s).
pub(super) const SAVED_FOR: Duration = Duration::from_millis(1800);

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
    /// The header: drag, minimise, maximise, close (the shell does them).
    Caption(Caption),
    DefaultAction(DefaultAction),
    SearchHistory(bool),
    Autostart(bool),
    /// The Browse button: the shell opens the folder picker.
    BrowseFolder,
    /// The picker's answer (`None` = cancelled).
    Folder(Option<PathBuf>),
    ClearFolder,
    /// The Project button.
    OpenLink(String),
    /// About: put the diagnostics on the clipboard / open the pre-filled issue.
    CopyDiagnostics,
    ReportProblem,
    /// The OS mode: System / Light / Dark.
    Theme(Theme),
    /// A theme card (a `kit::themes` id): its mode's pick, shown at once.
    ThemeCard(&'static str),
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
    /// A Labs switch: saved and applied at once.
    Labs(Feature, bool),
    Source(Language),
    Target(Language),
    Engine(Engine),
    /// What was typed in the live-interval field.
    LiveInterval(String),
    SelectStyle(SelectStyle),
    After(After),
    SearchWith(SearchWith),
    TextReader(TextReader),
    /// The text reader row's button: Windows' language settings.
    OpenLanguageSettings,
    Look(LookTheme),
    /// The custom look's controls.
    Palette(PaletteChoice),
    Glow(u8),
    Dim(u8),
    Brackets(BracketColour),
    BracketStyle(BracketStyle),
    Bar(BarChoice),
    EdgeMode(EdgeMode),
    EdgeStyle(EdgeStyle),
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
    /// A write (`Outcome::Write`) finished, on its thread.
    Written(Box<Done>),
    /// The "Saved" pill's clock.
    Tick,
    /// A feature preview's Hide (`false`) / Show (`true`), by its
    /// `hidden_previews` name; and Replay.
    Preview(&'static str, bool),
    Replay,
    /// The footer's question when the window is closed with unsaved changes (#136).
    CloseSave,
    CloseDiscard,
    CloseKeep,
}

/// A colour set in the custom look's dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteChoice(pub(super) ColourSet);

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

pub(super) const PALETTES: [PaletteChoice; 7] = [
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
pub struct BarChoice(pub(super) BarStyle);

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

pub(super) const BARS: [BarChoice; 5] = [
    BarChoice(BarStyle::Glass),
    BarChoice(BarStyle::GlassDark),
    BarChoice(BarStyle::GlassLight),
    BarChoice(BarStyle::Solid),
    BarChoice(BarStyle::Outline),
];

// One short-lived value per message: a boxed Config would buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    None,
    /// Written to disk: re-bind the shortcuts, re-read the theme.
    Saved(Config),
    /// Write this on a worker thread (`Write::run`) and send the answer back as
    /// `Message::Written`: the UI never waits on the disk.
    Write(Write),
    /// Show the folder picker; the answer comes back as `Message::Folder`.
    Browse,
    /// Open this link in the browser (the user clicked it on the About page).
    OpenLink(String),
    /// Copy the diagnostics report (the shell does it and shows the pill).
    CopyDiagnostics,
    /// Check for an update now; the answer comes back as `Message::UpdateStatus`.
    CheckUpdate,
}

/// A config write for a worker thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Write {
    path: Option<PathBuf>,
    edit: Edit,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Edit {
    /// The draft's edits (`saved` -> `draft`), over the config as it is now.
    Save {
        saved: Box<Config>,
        draft: Box<Config>,
    },
    /// One Labs switch.
    Lab(Feature, bool),
}

/// What a write answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    edit: Edit,
    /// The config the app was running before the write.
    before: Config,
    result: Result<Config, String>,
}

impl Done {
    /// A write that landed: (the config before, the config now), for the app-level
    /// effects (shortcuts, theme, tray). They run even when the window is gone.
    pub fn landed(&self) -> Option<(&Config, &Config)> {
        self.result.as_ref().ok().map(|now| (&self.before, now))
    }
}

impl Write {
    /// Blocks on the disk (fsync): call it off the UI thread.
    pub fn run(self) -> Box<Done> {
        let path = self.path.as_deref();
        let before = match path {
            Some(p) => Config::load_from(p).0,
            None => config::current(),
        };
        let result = match &self.edit {
            Edit::Save { saved, draft } => config::update_at(path, |c| {
                *c = config::rebase(c, saved, draft);
            }),
            Edit::Lab(feature, on) => config::update_at(path, |c| c.labs.set(*feature, *on)),
        };
        Box::new(Done {
            edit: self.edit,
            before,
            result: result.map_err(|e| e.to_string()),
        })
    }
}

impl State {
    /// The real config file and the real keyring.
    pub fn open() -> Self {
        let (config, warnings) = Config::load();
        let mut s = Self::new(config, warnings, None, Box::new(Keyring));
        s.auto = true;
        s
    }

    /// The keyring calls the last message started (`true` = song search's key).
    pub fn take_key_jobs(&mut self) -> Vec<(bool, KeyJob)> {
        std::mem::take(&mut self.key_jobs)
    }

    /// `path: None` = `Config::path()`. `warnings` come from `Config::load`.
    pub fn new(
        config: Config,
        warnings: Vec<String>,
        path: Option<PathBuf>,
        store: Box<dyn KeyStore>,
    ) -> Self {
        theme::apply(config.theme);
        let store: Arc<dyn KeyStore> = store.into();
        let key = KeyField::new(bind(&store, config.ai.provider.clone()));
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
        let (recognizers, profile) = crate::ocr::system::windows_languages();
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
            key_jobs: Vec::new(),
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
            update_status: None,
            recognizers,
            profile,
            asking_close: false,
            leaving: false,
            auto: false,
            saved_at: None,
            writing: false,
            queued_labs: Vec::new(),
            closing: false,
            moved: None,
            picked: None,
            played: None,
        };
        s.revalidate();
        s
    }

    /// What look `theme` is (Custom: the draft's own).
    pub(super) fn look(&self, theme: LookTheme) -> Look {
        theme.look(&self.draft.look.custom)
    }

    pub(super) fn dirty(&self) -> bool {
        self.draft != self.saved
    }

    /// The window is being closed: `true` = nothing is lost, close it. With
    /// unsaved changes it asks in the footer instead and returns `false`.
    pub fn close_requested(&mut self) -> bool {
        self.asking_close = self.dirty();
        !self.asking_close
    }

    /// The footer's question was answered with Save (done) or Discard.
    pub fn leaving(&self) -> bool {
        self.leaving
    }

    /// The edited config (onboarding writes it on Finish).
    pub fn draft(&self) -> &Config {
        &self.draft
    }

    /// What is wrong with the draft, as "key: reason" lines.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Puts `ai` back (onboarding's Skip), its key looked up again.
    pub fn set_ai(&mut self, ai: config::AiConfig) {
        if ai.provider != self.draft.ai.provider {
            let job = self.key.refresh(bind(&self.store, ai.provider.clone()));
            self.key_jobs.push((false, job));
        }
        self.draft.ai = ai;
        self.revalidate();
    }

    /// A shortcut field is recording (the global shortcuts are held off meanwhile).
    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    pub(super) fn revalidate(&mut self) {
        self.problems = problems(&self.draft, self.key.status == KeyStatus::Saved);
    }

    pub(super) fn label(&self, slot: Slot) -> &str {
        match slot {
            Slot::Capture => "Open OpenCircle",
            Slot::Quick(i) => &self.quick_text[i].label,
        }
    }

    pub(super) fn shortcut(&self, slot: Slot) -> &str {
        match slot {
            Slot::Capture => &self.draft.shortcut,
            Slot::Quick(i) => &self.draft.quick_actions[i].shortcut,
        }
    }

    /// Wayland: what the desktop says about `slot`'s key when it isn't that key
    /// (`None` also while the slot has unsaved changes: the desktop has the saved one).
    pub(super) fn desktop_note(&self, slot: Slot) -> Option<String> {
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

    pub(super) fn set_shortcut(&mut self, slot: Slot, value: String) {
        match slot {
            Slot::Capture => self.draft.shortcut = value,
            Slot::Quick(i) => self.draft.quick_actions[i].shortcut = value,
        }
    }

    /// The shortcut fields: Open OpenCircle, then every quick action whose Labs
    /// feature is on (the others keep their entry in the file, unseen).
    pub(super) fn slots(&self) -> std::vec::IntoIter<Slot> {
        let labs = &self.draft.labs;
        let quick = self.draft.quick_actions.iter().enumerate();
        let shown = quick.filter(|(_, q)| crate::labs::quick_on(labs, q));
        let mut all = vec![Slot::Capture];
        all.extend(shown.map(|(i, _)| Slot::Quick(i)));
        all.into_iter()
    }

    /// Why `chord` cannot be `slot`'s shortcut, or the shortcut string to store.
    pub(super) fn check_chord(&self, slot: Slot, chord: &Chord) -> Result<String, String> {
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

    pub(super) fn on_chord(&mut self, slot: Slot, chord: Chord) {
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
pub(super) fn problems(cfg: &Config, key_saved: bool) -> Vec<String> {
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
pub(super) fn issue(problems: &[String], key: &str) -> Option<String> {
    problems
        .iter()
        .find_map(|p| p.strip_prefix(key)?.strip_prefix(": "))
        .map(|s| {
            let mut c = s.chars();
            c.next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
}

/// Applies `message`; in the real window (`auto`) a change that leaves the draft
/// valid is written at once, so nothing is lost on close (#136). A draft with
/// problems is held back (the footer says why) and closing asks about it.
/// The write itself runs off the UI thread (`Outcome::Write`).
pub fn update(state: &mut State, message: Message) -> Outcome {
    let change = !matches!(
        message,
        Message::Save
            | Message::Written(_)
            | Message::Tick
            | Message::Replay
            | Message::Caption(_)
            | Message::CloseSave
            | Message::CloseDiscard
            | Message::CloseKeep
    );
    let out = apply(state, message);
    if state.auto && change && out == Outcome::None && state.dirty() && state.problems.is_empty() {
        return save(state);
    }
    out
}

pub(super) fn apply(state: &mut State, message: Message) -> Outcome {
    state.notice = None;
    if !matches!(message, Message::CloseSave | Message::CloseDiscard) {
        state.asking_close = false; // any other action is "keep editing"
    }
    let picked = match &message {
        Message::DefaultAction(_) => Some(Pick::Default(state.draft.general.default_action)),
        Message::Autostart(_) => Some(Pick::Autostart(state.draft.general.autostart)),
        Message::SearchHistory(_) => Some(Pick::History(state.draft.general.search_history)),
        Message::AutoUpdate(_) => Some(Pick::Update(state.draft.update.auto == Some(true))),
        Message::Labs(f, _) => Some(Pick::Lab(*f, state.draft.labs.has(*f))),
        Message::Engine(_) => Some(Pick::Engine(state.draft.translate.engine)),
        Message::SongSource(_) => Some(Pick::SongSource(state.draft.song.source)),
        Message::SongHistory(_) => Some(Pick::SongHistory(state.draft.song.history)),
        Message::SelectStyle(_) => Some(Pick::Select(state.draft.selection.style)),
        Message::After(_) => Some(Pick::After(state.draft.selection.after)),
        Message::SearchWith(_) => Some(Pick::SearchWith(state.draft.selection.search_with)),
        Message::TextReader(_) => Some(Pick::Reader(state.draft.selection.text_reader)),
        _ => None,
    };
    if let Some(p) = picked {
        state.picked = Some((p, Instant::now()));
    }
    match message {
        Message::Caption(_) => {}
        Message::Section(s) => {
            if s != state.section {
                state.moved = Some((state.section, Instant::now()));
            }
            state.section = s;
            state.play();
            state.recording = None;
            state.shortcut_error = None;
            if s == Section::Song && state.song_key.is_none() {
                let (field, job) = KeyField::pending(bind(&state.song_store, SERVICE));
                state.song_key = Some(field.labelled(SONG_KEY));
                state.key_jobs.push((true, job));
            }
        }
        Message::SongKey(k) => {
            if let Some(field) = &mut state.song_key {
                let answer = matches!(k, KeyMessage::Done(..));
                let slot = bind(&state.song_store, SERVICE);
                state
                    .key_jobs
                    .extend(field.update(slot, k).map(|j| (true, j)));
                // The overlay's music button shows from now on (or hides).
                if answer || !field.busy() {
                    music::remember_key(field.status == KeyStatus::Saved);
                }
            }
        }
        Message::SongSource(source) => state.draft.song.source = source,
        Message::SongHistory(on) => state.draft.song.history = on,
        Message::Labs(feature, on) => return set_lab(state, feature, on),
        Message::Theme(t) => {
            state.draft.theme = t;
            theme::preview(t, &state.draft.appearance);
        }
        Message::ThemeCard(id) => pick_theme(state, id),
        Message::Record(slot) => {
            state.recording = (state.recording != Some(slot)).then_some(slot);
            state.shortcut_error = None;
        }
        Message::Key(chord) => {
            if let Some(slot) = state.recording {
                state.on_chord(slot, chord);
            }
        }
        Message::Provider(p) => set_provider(state, p),
        Message::Preset(p) => pick_preset(state, p),
        Message::BaseUrl(s) => state.draft.ai.base_url = s.trim().to_string(),
        Message::Model(s) => state.draft.ai.model = s.trim().to_string(),
        Message::ApiKey(k) => {
            let provider = state.draft.ai.provider.clone();
            let slot = bind(&state.store, provider);
            state
                .key_jobs
                .extend(state.key.update(slot, k).map(|j| (false, j)));
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
        Message::SearchHistory(on) => {
            state.draft.general.search_history = on;
            if !on {
                crate::history::search::clear_default(); // turning it off deletes it
            }
        }
        Message::Autostart(on) => state.draft.general.autostart = on,
        Message::SelectStyle(v) => state.draft.selection.style = v,
        Message::After(v) => state.draft.selection.after = v,
        Message::SearchWith(v) => state.draft.selection.search_with = v,
        Message::TextReader(v) => state.draft.selection.text_reader = v,
        Message::OpenLanguageSettings => open_language_settings(),
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
        }
        Message::EdgeMode(m) => state.edge_mode = m,
        Message::Picker(i) => {
            state.picker = (state.picker != Some(i)).then_some(i);
            state.hex = state.draft.look.custom.colours[i.min(1)].hex();
        }
        Message::Hex(hex) => {
            if let (Some(i), Some(c)) = (state.picker, Rgb::parse(&hex)) {
                state.draft.look.custom.colours[i] = c;
            }
            state.hex = hex;
        }
        Message::PickColour(c) => {
            if let Some(i) = state.picker {
                let [r, g, b, _] = c.into_rgba8();
                state.draft.look.custom.colours[i] = Rgb([r, g, b]);
                state.hex = Rgb([r, g, b]).hex();
            }
        }
        Message::Palette(_)
        | Message::Glow(_)
        | Message::Dim(_)
        | Message::Brackets(_)
        | Message::BracketStyle(_)
        | Message::Bar(_)
        | Message::EdgeStyle(_) => {
            let mode = state.edge_mode;
            let custom = &mut state.draft.look.custom;
            match message {
                Message::Palette(p) => custom.palette = p.0,
                Message::Glow(v) => custom.glow = v,
                Message::Dim(v) => custom.dim = v,
                Message::Brackets(v) => custom.brackets = v,
                Message::BracketStyle(v) => custom.bracket_style = v,
                Message::Bar(v) => custom.bar = v.0,
                Message::EdgeStyle(v) => state.draft.look.edge_mut(mode).style = v,
                _ => {}
            }
        }
        Message::Snip(snip) => state.draft.snip = snip,
        Message::BrowseFolder => return Outcome::Browse,
        Message::Folder(Some(dir)) => {
            state.draft.general.save_folder = dir.to_string_lossy().into_owned();
        }
        Message::Folder(None) => {}
        Message::ClearFolder => state.draft.general.save_folder.clear(),
        Message::OpenLink(url) => return Outcome::OpenLink(url),
        Message::CopyDiagnostics => return Outcome::CopyDiagnostics,
        Message::ReportProblem => return Outcome::OpenLink(crate::diagnostics::issue_url()),
        Message::AutoUpdate(on) => state.draft.update.auto = Some(on),
        Message::CheckUpdate => {
            state.update_status = Some("Checking…".into());
            return Outcome::CheckUpdate;
        }
        Message::UpdateStatus(status) => state.update_status = Some(status),
        Message::Save => return save(state),
        Message::Written(done) => return written(state, done),
        Message::Preview(key, show) => {
            let hidden = &mut state.draft.appearance.hidden_previews;
            hidden.retain(|k| k != key);
            if !show {
                hidden.push(key.to_string());
            }
            // a hidden preview stops; shown again, it plays from the start
            if show || !previews::plays(state, state.section) {
                state.play();
            }
        }
        Message::Replay => state.play(),
        Message::Tick => {
            if state.saved_at.is_some_and(|t| t.elapsed() >= SAVED_FOR) {
                state.saved_at = None;
            }
            if state.played.is_some_and(|t| t.elapsed() >= hero::LENGTH) {
                state.played = None;
            }
            // the longest of them: the glide (the page rises in `MORPH`)
            if state
                .moved
                .is_some_and(|(_, t)| t.elapsed() >= motion::GLIDE)
            {
                state.moved = None;
            }
            if state
                .picked
                .is_some_and(|(_, t)| t.elapsed() >= motion::GLIDE)
            {
                state.picked = None;
            }
        }
        Message::CloseSave => {
            let saved = save(state);
            if state.writing {
                state.closing = true; // closes when the write lands
            } else {
                state.leaving = !state.dirty(); // a refused save (problems) stays open
            }
            return saved;
        }
        Message::CloseDiscard => state.leaving = true,
        Message::CloseKeep => {}
    }
    state.revalidate();
    Outcome::None
}

/// The provider (the browser's segment, or a Provider pick's API): hidden
/// fields stay valid, and the key field asks the keyring again.
fn set_provider(state: &mut State, p: Provider) {
    if p == state.draft.ai.provider {
        return;
    }
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
    let job = state.key.refresh(bind(&state.store, p));
    state.key_jobs.push((false, job));
}

/// A Provider dropdown pick: its API's provider, its server (Custom server
/// keeps a URL of your own) and its model while the field is empty or holds
/// a suggestion.
fn pick_preset(state: &mut State, p: Preset) {
    set_provider(state, p.provider());
    let ai = &mut state.draft.ai;
    if !p.custom() {
        ai.base_url = p.base_url.to_string();
    } else if Preset::of(ai) != Some(CUSTOM) {
        ai.base_url.clear();
    }
    if ai.model.is_empty() || PRESETS.iter().any(|x| x.model == ai.model) {
        ai.model = p.model.to_string();
    }
}

/// A theme card: that theme, and its mode (owner, 2026-10-28: the cards
/// decide; a config that never picked one keeps "system", following the OS).
fn pick_theme(state: &mut State, id: &str) {
    let Some(t) = THEMES.into_iter().find(|t| t.id == id) else {
        return;
    };
    let d = &mut state.draft;
    match t.light {
        true => d.appearance.light = t.id.into(),
        false => d.appearance.dark = t.id.into(),
    }
    d.theme = if t.light { Theme::Light } else { Theme::Dark };
    theme::preview(d.theme, &d.appearance);
}

/// A Labs switch writes at once, and only itself: the saved config with this one
/// switch changed (other edits in the draft stay unsaved). A failed write changes nothing.
pub(super) fn set_lab(state: &mut State, feature: Feature, on: bool) -> Outcome {
    if state.writing {
        state.queued_labs.push((feature, on));
        return Outcome::None;
    }
    state.writing = true;
    Outcome::Write(Write {
        path: state.path.clone(),
        edit: Edit::Lab(feature, on),
    })
}

pub(super) fn save(state: &mut State) -> Outcome {
    if !state.problems.is_empty() {
        state.notice = Some("Fix the problems above first".into());
        return Outcome::None;
    }
    // Never a dead button: with nothing to save it says so.
    if !state.dirty() {
        state.notice = Some("Nothing to save".into());
        return Outcome::None;
    }
    // A write is on its way: `written` saves again if the draft still differs.
    if state.writing {
        return Outcome::None;
    }
    // Only what was edited here is written, over the config as it is now.
    state.writing = true;
    Outcome::Write(Write {
        path: state.path.clone(),
        edit: Edit::Save {
            saved: Box::new(state.saved.clone()),
            draft: Box::new(state.draft.clone()),
        },
    })
}

/// A write landed: the saved config is what it returned, the draft keeps edits
/// made since; then the next waiting write, or the answer for the shell.
pub(super) fn written(state: &mut State, done: Box<Done>) -> Outcome {
    state.writing = false;
    let Done { edit, result, .. } = *done;
    let cfg = match result {
        Ok(cfg) => cfg,
        Err(e) => {
            // The failed edit stays in the draft (auto-save retries it); the switches
            // flipped meanwhile are still written.
            state.closing = false;
            state.notice = Some(format!("Could not save: {e}"));
            if state.queued_labs.is_empty() {
                return Outcome::None;
            }
            let (feature, on) = state.queued_labs.remove(0);
            return set_lab(state, feature, on);
        }
    };
    match edit {
        Edit::Save { draft, .. } => {
            state.draft = config::rebase(&cfg, &draft, &state.draft);
            state.saved_at = Some(Instant::now());
        }
        Edit::Lab(..) => {
            // The draft keeps its unsaved edits, on top of what is saved now.
            state.draft = config::rebase(&cfg, &state.saved, &state.draft);
            state.saved_at = Some(Instant::now());
            // its page goes with a Labs feature: back to Labs
            if pages::of(state.section)
                .lab
                .is_some_and(|f| !cfg.labs.has(f))
            {
                state.section = Section::Labs;
            }
        }
    }
    state.saved = cfg.clone();
    if !state.queued_labs.is_empty() {
        let (feature, on) = state.queued_labs.remove(0);
        return set_lab(state, feature, on);
    }
    if state.auto && state.dirty() && state.problems.is_empty() {
        return save(state); // edited while writing
    }
    state.revalidate();
    if state.closing && !state.dirty() {
        state.leaving = true;
    }
    Outcome::Saved(cfg)
}

/// The frame clock while something moves or the pill says "Saved"; key
/// presses only while a shortcut field is recording.
pub fn subscription(state: &State) -> Subscription<Message> {
    let clock = match state.animating() {
        true => motion::frames().map(|_| Message::Tick),
        false => Subscription::none(),
    };
    if state.recording.is_none() {
        return clock;
    }
    let keys = event::listen_with(|event, _status, _window| match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            physical_key,
            modifiers,
            ..
        }) => Chord::from_iced(physical_key, modifiers).map(Message::Key),
        _ => None,
    });
    Subscription::batch([keys, clock])
}

impl State {
    /// Something moves by the clock: a glide, the "Saved" pill, a preview.
    pub fn animating(&self) -> bool {
        self.saved_at.is_some()
            || self.moved.is_some()
            || self.picked.is_some()
            || self.played.is_some()
    }

    /// The page's previews play from the start (none shown, or reduced
    /// motion: they rest on their last frame, and the clock stays off).
    fn play(&mut self) {
        let plays = !motion::reduced() && previews::plays(self, self.section);
        self.played = plays.then(Instant::now);
    }

    /// Seconds into the page's previews (`hero::LENGTH` = the last frame).
    pub(super) fn preview_at(&self) -> f32 {
        let length = hero::LENGTH.as_secs_f32();
        match self.played {
            Some(t) if !motion::reduced() => t.elapsed().as_secs_f32().min(length),
            _ => length,
        }
    }

    /// A ported segmented control's thumb: (where it glides from, how far),
    /// `old` reading the control's own pick out of the last one.
    pub(super) fn thumb<T: Copy>(&self, now: T, old: impl Fn(Pick) -> Option<T>) -> (T, f32) {
        match self.picked.and_then(|(p, t)| Some((old(p)?, t))) {
            Some((from, t)) => (
                from,
                motion::progress(t, Instant::now(), motion::GLIDE, Curve::Standard),
            ),
            None => (now, 1.0),
        }
    }

    /// The page change under way: (the page it left, the glide's and the
    /// rise's progress); `None` at rest.
    pub(super) fn moving(&self) -> Option<(Section, f32, f32)> {
        let (from, t) = self.moved?;
        let now = Instant::now();
        Some((
            from,
            motion::progress(t, now, motion::GLIDE, Curve::Standard),
            motion::progress(t, now, motion::MORPH, Curve::Out),
        ))
    }
}
