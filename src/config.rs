//! The one config reader/validator: `<user config dir>/opencircle/config.toml`.
//!
//! Schema (version 1). Every key is optional; a missing file means defaults.
//!
//! ```toml
//! version = 1                       # schema version, written by save()
//! theme = "system"                  # system | light | dark
//! shortcut = "Ctrl+Shift+Space"     # global capture shortcut
//!
//! [general]
//! default_action = "lens"           # Enter on the action bar: lens | ai | text | copy
//! save_folder = ""                  # Save dialog starts here; "" = the system's choice
//!
//! [ai]                              # the API key is NOT here: OS keyring only
//! provider = "anthropic"            # anthropic | openai_compatible
//! base_url = ""                     # "" = provider default (Anthropic only)
//! model = "claude-sonnet-5-5"       # model id sent to the provider
//!
//! [translate]
//! target = "ar"                     # language code to translate into
//! engine = "ai"                     # ai (the only engine; anything else warns and uses ai)
//! live_interval_ms = 1000           # live translate repeats every N ms (500-5000)
//!
//! [[quick_actions]]                 # one direct shortcut per action
//! action = "translate"              # id from the action registry
//! shortcut = "Ctrl+Shift+T"         # "" = not bound
//! target = "full_screen"            # full_screen | circle | last_region | window
//! live = false                      # optional: repeat while shown
//!                                   # (snip: circle = region, window, full_screen)
//! ```
//!
//! Rules: an invalid value gives a warning plus that field's default, never a
//! crash and never a rewrite of the file (only `save` writes). Unknown keys
//! are ignored. Once released, this format is frozen: migrate, never break.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use global_hotkey::hotkey::{Code, HotKey, HotKeyParseError, Modifiers};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

/// Schema version this build reads and writes.
pub const VERSION: u32 = 1;
const DEFAULT_CAPTURE_SHORTCUT: &str = "Ctrl+Shift+Space";
const DEFAULT_TRANSLATE_TARGET: &str = "ar";
/// Live translate repeats this often (ms); the allowed range is `LIVE_INTERVAL_MS`.
pub const DEFAULT_LIVE_INTERVAL_MS: u32 = 1000;
pub const LIVE_INTERVAL_MS: std::ops::RangeInclusive<u32> = 500..=5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Anthropic,
    OpenaiCompatible,
}

impl Provider {
    /// Stable id, used for the keyring entry name.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenaiCompatible => "openai_compatible",
        }
    }
}

/// How text is translated. Only the AI engine exists; an "offline" engine may come
/// later, and a config that names it (or anything else) is read as `Ai` with a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    #[default]
    Ai,
}

/// What Enter does on the action bar. The serialized names are action-registry ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultAction {
    #[default]
    Lens,
    Ai,
    Text,
    Copy,
}

impl DefaultAction {
    pub const ALL: [DefaultAction; 4] = [Self::Lens, Self::Ai, Self::Text, Self::Copy];

    /// The id in `actions::ACTIONS`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Lens => "lens",
            Self::Ai => "ai",
            Self::Text => "text",
            Self::Copy => "copy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    #[default]
    FullScreen,
    Circle,
    LastRegion,
    /// Snip only: click a window. Other actions treat it like `circle`.
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuickAction {
    pub action: String,
    /// "" = not bound.
    pub shortcut: String,
    pub target: Target,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiConfig {
    pub provider: Provider,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranslateConfig {
    pub target: String,
    pub engine: Engine,
    /// Live translate repeats every this many ms (`LIVE_INTERVAL_MS`).
    pub live_interval_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct GeneralConfig {
    pub default_action: DefaultAction,
    /// Where the Save dialog starts; "" = the system's choice.
    pub save_folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Config {
    pub version: u32,
    pub theme: Theme,
    pub shortcut: String,
    pub general: GeneralConfig,
    pub ai: AiConfig,
    pub translate: TranslateConfig,
    pub quick_actions: Vec<QuickAction>,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: Provider::Anthropic,
            base_url: String::new(),
            model: crate::ai::DEFAULT_ANTHROPIC_MODEL.to_string(),
        }
    }
}

impl Default for TranslateConfig {
    fn default() -> Self {
        TranslateConfig {
            target: DEFAULT_TRANSLATE_TARGET.to_string(),
            engine: Engine::Ai,
            live_interval_ms: DEFAULT_LIVE_INTERVAL_MS,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let qa = |action: &str, shortcut: &str, target, live| QuickAction {
            action: action.to_string(),
            shortcut: shortcut.to_string(),
            target,
            live,
        };
        Config {
            version: VERSION,
            theme: Theme::System,
            shortcut: DEFAULT_CAPTURE_SHORTCUT.to_string(),
            general: GeneralConfig::default(),
            ai: AiConfig::default(),
            translate: TranslateConfig::default(),
            // DESIGN.md "Quick actions".
            quick_actions: vec![
                qa("translate", "Ctrl+Shift+T", Target::FullScreen, false),
                qa("translate", "Ctrl+Shift+L", Target::FullScreen, true),
                qa("ai_explain", "Ctrl+Shift+E", Target::Circle, false),
                qa("copy_text", "Ctrl+Shift+X", Target::Circle, false),
                qa("lens", "", Target::Circle, false),
                // Snip mode: region, window, full screen (DESIGN.md "Shortcuts").
                qa("snip", "PrintScreen", Target::Circle, false),
                qa("snip", "Alt+PrintScreen", Target::Window, false),
                qa("snip", "Ctrl+PrintScreen", Target::FullScreen, false),
            ],
        }
    }
}

/// Parse a shortcut string ("Ctrl+Shift+Space") with `global-hotkey`'s parser,
/// plus one rule of ours: a bare typing key (letter, digit, Space, ...) cannot
/// be global; only F-keys, PrintScreen, Pause and ScrollLock may stand alone.
pub fn parse_shortcut(s: &str) -> Result<HotKey, String> {
    let hk: HotKey = s.trim().parse().map_err(|e: HotKeyParseError| match e {
        HotKeyParseError::UnsupportedKey(k) => format!("unknown key \"{k}\""),
        _ => "expected modifiers then one key, like Ctrl+Shift+Space".to_string(),
    })?;
    if hk.mods == Modifiers::empty() && !may_stand_alone(hk.key) {
        return Err("needs at least one modifier (Ctrl, Alt, Shift, Super)".to_string());
    }
    Ok(hk)
}

fn may_stand_alone(key: Code) -> bool {
    matches!(key, Code::PrintScreen | Code::Pause | Code::ScrollLock)
        || format!("{key:?}")
            .strip_prefix('F')
            .is_some_and(|n| n.parse::<u8>().is_ok())
}

impl Config {
    /// `<user config dir>/opencircle/config.toml`; `None` if the OS has no config dir.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("opencircle").join("config.toml"))
    }

    /// Read the config. Never fails: problems come back as warnings (one line
    /// each, naming the key) and the affected field takes its default.
    pub fn load() -> (Config, Vec<String>) {
        match Self::path() {
            Some(p) => Self::load_from(&p),
            None => (
                Config::default(),
                vec!["no user config directory; using defaults".into()],
            ),
        }
    }

    pub fn load_from(path: &Path) -> (Config, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), vec![]),
            Err(e) => (
                Config::default(),
                vec![format!("cannot read the config file ({e}); using defaults")],
            ),
        }
    }

    /// Validate TOML text field by field.
    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut w = Vec::new();
        let root: Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                let line = e.to_string().lines().next().unwrap_or_default().to_string();
                return (
                    Config::default(),
                    vec![format!(
                        "config is not valid TOML ({line}: {}); using defaults",
                        e.message()
                    )],
                );
            }
        };
        let d = Config::default();
        let mut c = Config {
            version: VERSION,
            ..d.clone()
        };

        if let Some(v) = root.get("version") {
            match v.as_integer() {
                Some(n) if n == VERSION as i64 => {}
                Some(n) if n > VERSION as i64 => w.push(format!(
                    "version: written by a newer OpenCircle (schema {n}); unknown keys are ignored and lost on save"
                )),
                _ => w.push("version: expected 1; reading it as version 1".into()),
            }
        }
        c.theme = get(&root, "", "theme", d.theme, &mut w);
        c.shortcut = get_checked(&root, "", "shortcut", d.shortcut, &mut w, |s: &String| {
            parse_shortcut(s).map(drop)
        });

        if let Some(t) = section(&root, "general", &mut w) {
            c.general.default_action = get(
                t,
                "general.",
                "default_action",
                d.general.default_action,
                &mut w,
            );
            c.general.save_folder = get_checked(
                t,
                "general.",
                "save_folder",
                d.general.save_folder,
                &mut w,
                |s: &String| check_folder(s),
            );
        }
        if let Some(t) = section(&root, "ai", &mut w) {
            c.ai.provider = get(t, "ai.", "provider", d.ai.provider, &mut w);
            c.ai.base_url =
                get_checked(t, "ai.", "base_url", d.ai.base_url, &mut w, |s: &String| {
                    check_base_url(s)
                });
            c.ai.model = get_checked(t, "ai.", "model", d.ai.model, &mut w, |s: &String| {
                check_model(s)
            });
        }
        if let Some(t) = section(&root, "translate", &mut w) {
            c.translate.target = get_checked(
                t,
                "translate.",
                "target",
                d.translate.target,
                &mut w,
                |s: &String| check_lang(s),
            );
            // Read as text so "offline" (or anything else) gets a clear warning;
            // the engine is Ai either way.
            let _: String = get_checked(
                t,
                "translate.",
                "engine",
                "ai".to_string(),
                &mut w,
                |s: &String| {
                    (s == "ai")
                        .then_some(())
                        .ok_or_else(|| "only \"ai\" is available in this version".into())
                },
            );
            c.translate.live_interval_ms = get_checked(
                t,
                "translate.",
                "live_interval_ms",
                d.translate.live_interval_ms,
                &mut w,
                |n: &u32| {
                    LIVE_INTERVAL_MS.contains(n).then_some(()).ok_or_else(|| {
                        format!(
                            "expected {} to {} milliseconds",
                            LIVE_INTERVAL_MS.start(),
                            LIVE_INTERVAL_MS.end()
                        )
                    })
                },
            );
        }
        if let Some(v) = root.get("quick_actions") {
            c.quick_actions = parse_quick_actions(v, &mut w);
        }

        // One physical shortcut = one job: the first user of a combination keeps it.
        let mut taken: HashSet<HotKey> = HashSet::new();
        taken.insert(parse_shortcut(&c.shortcut).expect("validated above"));
        for qa in &mut c.quick_actions {
            if let Ok(hk) = parse_shortcut(&qa.shortcut)
                && !taken.insert(hk)
            {
                w.push(format!(
                    "quick_actions: \"{}\" is already used; \"{}\" is left unbound",
                    qa.shortcut, qa.action
                ));
                qa.shortcut.clear();
            }
        }
        (c, w)
    }

    /// Write atomically (temp file in the same folder, then rename).
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| std::io::Error::other("no user config directory"))?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let body = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let text = format!(
            "# OpenCircle settings. Invalid values fall back to defaults.\n# API keys are never stored here (OS keyring only).\n\n{body}"
        );
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let tmp = path.with_extension("toml.tmp");
        let mut f = std::fs::File::create(&tmp)?;
        let written = f.write_all(text.as_bytes()).and_then(|()| f.sync_all());
        drop(f);
        // Rename replaces the target on Windows and Unix; on any failure drop the temp file.
        written
            .and_then(|()| std::fs::rename(&tmp, path))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })
    }
}

// ---- the running app's config: one source of truth ----

static CURRENT: RwLock<Option<Config>> = RwLock::new(None);

/// The config the running app uses (any thread). Read from disk on first use;
/// after that only `set_current` changes it (Settings saved, onboarding finished).
pub fn current() -> Config {
    if let Some(c) = CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return c.clone();
    }
    let (config, warnings) = Config::load();
    for warning in warnings {
        eprintln!("[opencircle] config: {warning}");
    }
    CURRENT
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert(config)
        .clone()
}

pub fn set_current(config: Config) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(config);
}

// ---- field readers: each returns the default (plus a warning) on a bad value ----

fn section<'a>(root: &'a Table, name: &str, w: &mut Vec<String>) -> Option<&'a Table> {
    match root.get(name)? {
        Value::Table(t) => Some(t),
        _ => {
            w.push(format!("{name}: expected a table; using defaults"));
            None
        }
    }
}

fn get<T: DeserializeOwned>(
    t: &Table,
    prefix: &str,
    key: &str,
    default: T,
    w: &mut Vec<String>,
) -> T {
    get_checked(t, prefix, key, default, w, |_| Ok(()))
}

fn get_checked<T: DeserializeOwned>(
    t: &Table,
    prefix: &str,
    key: &str,
    default: T,
    w: &mut Vec<String>,
    check: impl Fn(&T) -> Result<(), String>,
) -> T {
    let Some(v) = t.get(key) else { return default };
    match v
        .clone()
        .try_into::<T>()
        .map_err(|e| e.message().to_string())
        .and_then(|x| check(&x).map(|()| x))
    {
        Ok(x) => x,
        Err(why) => {
            w.push(format!("{prefix}{key}: {why}; using the default"));
            default
        }
    }
}

fn check_base_url(s: &str) -> Result<(), String> {
    let ok = s.is_empty()
        || ((s.starts_with("http://") || s.starts_with("https://"))
            && !s.contains(char::is_whitespace));
    ok.then_some(())
        .ok_or_else(|| "expected empty or an http(s):// URL".into())
}

/// Empty (system choice) or an absolute path; it need not exist (checked on use).
fn check_folder(s: &str) -> Result<(), String> {
    (s.is_empty() || Path::new(s).is_absolute())
        .then_some(())
        .ok_or_else(|| "expected empty or an absolute folder path".into())
}

fn check_model(s: &str) -> Result<(), String> {
    let ok = s.len() <= 200 && !s.contains(char::is_whitespace);
    ok.then_some(())
        .ok_or_else(|| "expected a model id without spaces".into())
}

/// "ar", "en", "pt-BR", "zh-Hant": 2-3 letters, then `-` parts of 2-8 alphanumerics.
fn check_lang(s: &str) -> Result<(), String> {
    let mut parts = s.split('-');
    let first = parts.next().unwrap_or_default();
    let ok = (2..=3).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_alphabetic())
        && parts
            .all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()));
    ok.then_some(())
        .ok_or_else(|| "expected a language code like \"ar\" or \"pt-BR\"".into())
}

fn parse_quick_actions(v: &Value, w: &mut Vec<String>) -> Vec<QuickAction> {
    let Some(items) = v.as_array() else {
        w.push("quick_actions: expected a list of tables; using the defaults".into());
        return Config::default().quick_actions;
    };
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(t) = item.as_table() else {
            w.push(format!(
                "quick_actions[{i}]: expected a table; entry skipped"
            ));
            continue;
        };
        let prefix = format!("quick_actions[{i}].");
        let action: String = get_checked(t, &prefix, "action", String::new(), w, |s: &String| {
            let ok = !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            ok.then_some(())
                .ok_or_else(|| "expected an action id like \"ai_explain\"".into())
        });
        if action.is_empty() {
            w.push(format!(
                "quick_actions[{i}]: no valid action; entry skipped"
            ));
            continue;
        }
        // Empty or invalid shortcut = unbound (never guess a binding).
        let shortcut = get_checked(t, &prefix, "shortcut", String::new(), w, |s: &String| {
            if s.is_empty() {
                Ok(())
            } else {
                parse_shortcut(s).map(drop)
            }
        });
        out.push(QuickAction {
            action,
            shortcut,
            target: get(t, &prefix, "target", Target::FullScreen, w),
            live: get(t, &prefix, "live", false, w),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oc-config-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn set_current_replaces_what_everyone_reads() {
        // The only test that touches the process-wide config.
        let mut c = Config::default();
        c.ai.model = "model-from-settings".into();
        set_current(c.clone());
        assert_eq!(current(), c);
        c.theme = Theme::Light;
        set_current(c.clone());
        assert_eq!(current(), c, "the next save replaces it");
    }

    #[test]
    fn defaults_match_design() {
        let c = Config::default();
        assert_eq!(c.version, 1);
        assert_eq!(c.theme, Theme::System);
        assert_eq!(c.shortcut, "Ctrl+Shift+Space");
        assert_eq!(c.translate.target, "ar");
        assert_eq!(c.ai.provider, Provider::Anthropic);
        assert_eq!(c.quick_actions.len(), 8);
        assert!(c.quick_actions[1].live);
        assert_eq!(c.general.default_action, DefaultAction::Lens);
        assert_eq!(c.general.save_folder, "");
        assert_eq!(c.translate.live_interval_ms, 1000);
        assert_eq!(c.translate.engine, Engine::Ai);
        // Snip: Print Screen = region, Alt = window, Ctrl = full screen.
        let snip: Vec<_> = c
            .quick_actions
            .iter()
            .filter(|q| q.action == "snip")
            .map(|q| (q.shortcut.as_str(), q.target))
            .collect();
        assert_eq!(
            snip,
            [
                ("PrintScreen", Target::Circle),
                ("Alt+PrintScreen", Target::Window),
                ("Ctrl+PrintScreen", Target::FullScreen),
            ]
        );
        // Every default shortcut is valid and unique.
        let (c2, warnings) = Config::parse("");
        assert_eq!(c2, c);
        assert!(warnings.is_empty());
    }

    #[test]
    fn missing_file_gives_defaults_without_warnings() {
        let (c, w) = Config::load_from(&tmp_dir("missing").join("nope.toml"));
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tmp_dir("rt");
        let path = dir.join("opencircle").join("config.toml");
        let mut c = Config {
            theme: Theme::Dark,
            ..Config::default()
        };
        c.ai.provider = Provider::OpenaiCompatible;
        c.ai.base_url = "http://localhost:11434/v1".into();
        c.ai.model = "llama3:8b".into();
        c.general.default_action = DefaultAction::Copy;
        c.general.save_folder = std::env::temp_dir().to_string_lossy().into_owned();
        c.translate.live_interval_ms = 2500;
        c.quick_actions.pop();
        c.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[[quick_actions]]") && !text.contains("api_key"),
            "{text}"
        );
        let (back, w) = Config::load_from(&path);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back, c);
        assert!(!path.with_extension("toml.tmp").exists());
        assert!(text.contains("[general]") && text.contains("default_action = \"copy\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_fields_are_validated() {
        let (c, w) = Config::parse(
            "[general]\ndefault_action = \"text\"\nsave_folder = \"relative/dir\"\n\
             [translate]\nlive_interval_ms = 100\n",
        );
        assert_eq!(
            c.general.default_action,
            DefaultAction::Text,
            "good survives"
        );
        assert_eq!(c.general.save_folder, "");
        assert_eq!(c.translate.live_interval_ms, 1000);
        assert!(
            w.iter().any(|m| m.starts_with("general.save_folder")),
            "{w:?}"
        );
        assert!(
            w.iter()
                .any(|m| m.starts_with("translate.live_interval_ms") && m.contains("500")),
            "{w:?}"
        );
        for bad in ["\"nope\"", "7", "\"translate\""] {
            let (c, w) = Config::parse(&format!("[general]\ndefault_action = {bad}\n"));
            assert_eq!(c.general.default_action, DefaultAction::Lens);
            assert!(
                w.iter().any(|m| m.starts_with("general.default_action")),
                "{w:?}"
            );
        }
        // Range ends are valid; a string or a negative number is not.
        for ok in [500, 5000] {
            let (c, w) = Config::parse(&format!("[translate]\nlive_interval_ms = {ok}\n"));
            assert_eq!((c.translate.live_interval_ms, w.len()), (ok, 0));
        }
        for bad in ["5001", "499", "-1", "\"1000\"", "1.5"] {
            let (c, w) = Config::parse(&format!("[translate]\nlive_interval_ms = {bad}\n"));
            assert_eq!(c.translate.live_interval_ms, 1000, "{bad}");
            assert_eq!(w.len(), 1, "{bad}: {w:?}");
        }
        let abs = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        let (c, w) = Config::parse(&format!("[general]\nsave_folder = \"{abs}\"\n"));
        assert_eq!((c.general.save_folder.as_str(), w.len()), (abs.as_str(), 0));
    }

    #[test]
    fn offline_engine_warns_and_uses_ai() {
        for value in ["\"offline\"", "\"gemini\"", "3"] {
            let (c, w) = Config::parse(&format!("[translate]\nengine = {value}\n"));
            assert_eq!(c.translate.engine, Engine::Ai);
            assert_eq!(w.len(), 1, "{value}: {w:?}");
            assert!(w[0].starts_with("translate.engine"), "{w:?}");
        }
        let (_, w) = Config::parse("[translate]\nengine = \"ai\"\n");
        assert!(w.is_empty());
        // A config rewritten by this build never names another engine.
        let text = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(text.contains("engine = \"ai\""), "{text}");
    }

    #[test]
    fn default_action_ids_are_registry_actions() {
        for a in DefaultAction::ALL {
            assert!(
                crate::actions::ACTIONS.iter().any(|x| x.id == a.id()),
                "{a:?}"
            );
            let (c, w) = Config::parse(&format!("[general]\ndefault_action = \"{}\"\n", a.id()));
            assert_eq!((c.general.default_action, w.len()), (a, 0));
        }
    }

    #[test]
    fn invalid_fields_warn_and_default() {
        let (c, w) = Config::parse(
            r#"
            theme = "neon"
            shortcut = "Ctrl+"
            [ai]
            provider = "gemini"
            base_url = "ftp://x"
            model = "has space"
            [translate]
            target = "not a code"
            engine = 3
            "#,
        );
        assert_eq!(c, Config::default());
        for key in [
            "theme",
            "shortcut",
            "ai.provider",
            "ai.base_url",
            "ai.model",
            "translate.target",
            "translate.engine",
        ] {
            assert!(
                w.iter().any(|m| m.starts_with(key)),
                "no warning for {key}: {w:?}"
            );
        }
    }

    #[test]
    fn good_fields_survive_next_to_bad_ones() {
        let (c, w) =
            Config::parse("theme = \"light\"\nshortcut = 5\n[translate]\ntarget = \"pt-BR\"\n");
        assert_eq!(c.theme, Theme::Light);
        assert_eq!(c.translate.target, "pt-BR");
        assert_eq!(c.shortcut, "Ctrl+Shift+Space");
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn broken_toml_and_wrong_shapes_do_not_crash() {
        let (c, w) = Config::parse("theme = = =");
        assert_eq!(c, Config::default());
        assert_eq!(w.len(), 1);
        let (c, w) = Config::parse("ai = 4\nquick_actions = \"x\"\nversion = 9\n");
        assert_eq!(c, Config::default());
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn quick_actions_are_validated_per_entry() {
        let (c, w) = Config::parse(
            r#"
            [[quick_actions]]
            action = "translate"
            shortcut = "Ctrl+Alt+9"
            target = "last_region"
            [[quick_actions]]
            shortcut = "Ctrl+Alt+8"
            [[quick_actions]]
            action = "lens"
            shortcut = "Banana+Q"
            target = "nowhere"
            [[quick_actions]]
            action = "copy_text"
            shortcut = "Ctrl+Alt+9"
            "#,
        );
        assert_eq!(c.quick_actions.len(), 3);
        assert_eq!(c.quick_actions[0].target, Target::LastRegion);
        assert_eq!(c.quick_actions[1].shortcut, "", "bad shortcut = unbound");
        assert_eq!(c.quick_actions[1].target, Target::FullScreen);
        assert_eq!(c.quick_actions[2].shortcut, "", "duplicate = unbound");
        assert!(w.iter().any(|m| m.contains("no valid action")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("already used")), "{w:?}");
    }

    #[test]
    fn capture_shortcut_conflict_unbinds_the_quick_action() {
        let (c, w) = Config::parse(
            "[[quick_actions]]\naction = \"lens\"\nshortcut = \"ctrl+shift+space\"\n",
        );
        assert_eq!(c.quick_actions[0].shortcut, "");
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn shortcut_validation() {
        for ok in [
            "Ctrl+Shift+Space",
            "ctrl+shift+t",
            "Alt+PrintScreen",
            "PrintScreen",
            "F9",
            "Ctrl+Alt+F12",
            "Super+KeyA",
        ] {
            assert!(parse_shortcut(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "Ctrl+",
            "Ctrl+Shift",
            "A",
            "Space",
            "Shift+Ctrl+",
            "Ctrl+Banana",
            "Ctrl+A+B",
            "Ctrl+B+Shift",
            "+",
        ] {
            assert!(parse_shortcut(bad).is_err(), "{bad}");
        }
        assert_eq!(
            parse_shortcut("Ctrl+Banana").unwrap_err(),
            "unknown key \"Banana\""
        );
    }
}
