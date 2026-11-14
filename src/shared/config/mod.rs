//! The one config reader/validator: `<user config dir>/opencircle/config.toml`.
//!
//! Schema (version 12). Every key is optional; a missing file means defaults.
//!
//! ```toml
//! version = 12                      # schema version, written by save()
//! theme = "system"                  # system | light | dark
//! glass_effect = true               # frost / graphite blur the screen behind them (false = solid tint)
//!                                   # (both kept and written; unused until glass returns)
//! shortcut = "Ctrl+Shift+Space"     # global capture shortcut
//!
//! [appearance]                      # the app's theme picks (ids of the design's themes)
//! dark = "sapphire"                 # the theme in dark mode: obsidian | mica | titanium | sapphire
//!                                   # | amethyst | emerald | rose | graphite | midnight
//! light = "daylight"                # the theme in light mode: frost | daylight | sandstone
//! logo = "theme"                    # theme (the theme's accent) | purple
//! hidden_previews = []              # the Settings pages whose feature preview the user hid
//!
//! [general]
//! default_action = "lens"           # Enter on the action bar: lens | ai | text | copy
//! save_folder = ""                  # Save dialog starts here; "" = the system's choice
//! autostart = true                  # start with Windows / at login (tray only, no window)
//! language = ""                     # the language picked in onboarding ("" = the OS language;
//!                                   # written only when set)
//! app_language = ""                 # the app's own language: "" = follow `language` (Settings' "Your
//!                                   # language (...)"), else en | ar (written only when the user picks one)
//! shortcuts_taken_shown = []        # the shortcuts another app held when the one-time notice said so
//!                                   # (written only when not empty)
//!
//! [bar]                             # the overlay's action bar, where the user docked it
//! anchor = "bottom"                 # bottom | top | bottom_left | bottom_right
//!
//! [ai]                              # the API key is NOT here: OS keyring only
//! provider = "browser"              # browser | anthropic | openai_compatible (missing = browser)
//! base_url = ""                     # "" = provider default (Anthropic only)
//! model = "claude-sonnet-5-5"       # model id sent to the provider
//!
//! [translate]
//! source = "auto"                   # language code the text is in; auto = detect
//! target = "en"                     # language code to translate into; a new config: the OS language
//!                                   # when it is in the language table, else English (never Arabic by default)
//! engine = "offline"                # offline | ai (ai falls back to offline when AI isn't set up)
//! live_interval_ms = 1500           # live translate repeats every N ms (500-5000)
//! live_note_shown = false           # the one-time live-mode note was shown
//! whole_screen = false              # live translate reads the whole screen,
//!                                   # not just the foreground window (no taskbar, no browser bars)
//! recent_sources = []               # the language picker's last picks, newest first
//! recent_targets = []               #   (written only when not empty)
//!
//! [selection]
//! style = "snap"                    # snap (a rounded box with corner brackets) | shape (as drawn)
//! text_reader = "auto"              # who reads text to select: auto | windows (Windows OCR) | pp_ocr
//!
//! [look]                            # how the screen looks while OpenCircle is active
//! theme = "spectrum"                # spectrum | aurora | sunset | ocean | neon | mono | calm | custom
//!                                   # | horizon | solar | hud | borealis (aurora = the design's Prism, mono = its Frost)
//! [look.custom]                     # theme = "custom" only (a copy of a preset to edit)
//! palette = "spectrum"              # spectrum | aurora | sunset | ocean | neon | mono | two_colour
//! colours = ["#8f80ff", "#4fc9e0"]  # palette = two_colour: from, to
//! glow = 100                        # edge glow strength, 0-100 % (100 = the design's; 0 = no edge)
//! dim = 12                          # dim over the frozen screen, 0-60 %
//! brackets = "white"                # white | accent: the selection's corner colour
//! bracket_style = "corners"         # corners | frame
//! bar = "glass"                     # glass | glass_dark | glass_light | solid | outline
//! [look.custom.overlay]             # the screen edge per mode: overlay, live, snip
//! style = "glow"                    # glow | none (thin: the glow too)
//! thickness = 6                     # 1-12 px; only the line on the live layer (Windows)
//! [song]                            # song search (the service key: OS keyring only)
//! source = "system"                 # system (what this PC plays) | microphone
//! history = false                   # keep recent songs on this PC
//! [update]
//! auto = true                       # keep OpenCircle up to date (unset = not asked yet: off)
//! [snip]
//! after = "both"                    # both (copy when picked) | editor; an old "copy" reads as both
//! max_edge = 0                      # Copy / Save scale the long edge down to this; 0 = original (default)
//! format = "png"                    # Save writes png | jpeg (quality 85)
//! save_original = false             # Save ignores max_edge
//! cursor = false                    # draw the mouse pointer into the capture
//! history = false                   # keep the last 20 captures (user data dir only)
//! delay = 0                         # timer before a snip: 0 (off) | 3 | 5 | 10 seconds
//! long_max = 20000                  # a long capture stops at this height: 5000 | 10000 | 20000 px
//!                                   # (written only when not 20000)
//!
//! [editor]                          # the annotation editor; all but line_assist written
//! line_assist = true                # only once the editor changed them (missing = default)
//! tool = "arrow"                    # the last tool (a Tool, snake_case; never crop)
//! colour = "#FF3B30"                # the last colour
//! size = "medium"                   # thin | medium | thick: the shared size
//! text_size = 26                    # the Text tool's size, 8-96 px
//! outline = false                   # shapes, arrows, callouts get an outline
//! fill = false                      # rectangles and ellipses get a tint
//! blur_size = "medium"              # the blur brush: thin | medium | thick; missing = Auto
//! blur_cell = "thin"                # the blur's pixel size: thin (6 px) | medium (9) | thick (12)
//! stroke = 5                        # the line width of shapes, arrows, pen: 1-32 px
//! text_font = "sans"                # the Text tool's face: sans | serif | mono
//! text_bold = true                  # the Text tool's weight
//! text_back = "none"                # behind the glyphs: none | outline | box
//! group_tools = ["select", "arrow", "pen", "text", "blur"]  # each toolbar group's last tool
//!
//! [labs]                            # unfinished features: off = hidden everywhere, settings kept
//! song = false                      # song search
//! pins = false                      # pinning a capture to the screen
//! live = false                      # live translate (unfinished until Live translation): bar button, tray item, shortcut
//!
//! [[quick_actions]]                 # one direct shortcut per action
//! action = "translate"              # id from the action registry
//! shortcut = "Alt+Shift+T"         # "" = not bound
//! target = "full_screen"            # full_screen | circle | last_region | window
//! live = false                      # optional: repeat while shown
//! ```
//!
//! Rules: an invalid value gives a warning plus that field's default, never a
//! crash and never a rewrite of the file (only `save` writes). One exception:
//! an unknown `ai.provider` is kept as `Provider::Invalid` (fail closed: AI is
//! off until the user picks one, never a fallback to another provider); a file
//! that exists but can't be read or parsed gives `Provider::Unreadable`. Unknown
//! keys are ignored. Once released, this format is frozen: migrate, never break.
//!
//! Versions: 1 = v0.1.0; 2 adds `[labs]`; 3 sets `selection.after` to "wait" (nothing
//! is sent before the user picks an action) and drops `[results]` (the web view card
//! is gone: its keys are ignored); 4 adds `[appearance]` and the looks horizon, solar,
//! hud and borealis (written only when picked; stored look values never change); without `[appearance]` an older file's
//! `widget_theme` frost / graphite becomes the light / dark pick; 5 makes `snip.after`
//! "both" the default, and an older file whose Snip was still off in Labs (never used)
//! moves from the old default "copy" to it (a Snip user's value stays); 6 retires "copy" (copy and
//! close): every "copy" becomes "both", so `snip.after` is one switch, "Copy when picked"
//! ("editor" = no auto copy; "both" = copy; a stored "copy" still reads as "both"); 7 moves the old default `snip.max_edge` 1568 to 0 (Original; any other value stays); 8 moves Snip's old default
//! shortcut PrintScreen to Alt+A (unless Alt+A is taken; a user's own key stays); 9 drops retired keys on the
//! next save (still read without a warning): `[labs] snip` / `editor` and the window / full-screen snip
//! entries of `[[quick_actions]]` (Snip has one shortcut); 10 makes "editor" (a pick copies nothing; Ctrl+C or Copy
//! does) the default `snip.after`: every stored "both" moves to it once (0.1.3 wrote the old default). 11 stops writing the keys nothing
//! reads (`widget_theme`, `selection.after` / `search_with`, `activation_glow`, edge `animate`) and the removed search history
//! (`general.search_history`, `labs.history`); they still read without a warning.
//! 12 moves the old default Translate / Live translate keys Ctrl+Shift+T / L to Alt+Shift+T / L (unless taken; a user's own
//! key stays). A file older than `VERSION` is copied to
//! `config.toml.bak-v<old>` (never over an existing copy) and then rewritten as the
//! current version, by `migrate` (add one step there per schema change).

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use global_hotkey::hotkey::{Code, HotKey, HotKeyParseError, Modifiers};
use serde::{Deserialize, Serialize};
use toml::Table;

use crate::features::settings::labs::Labs;
use crate::shared::platform::hotkey::QuickId;

mod look;
mod migrate;
mod parse;
mod store;

pub use look::PRESETS;
pub use store::{appearance, current, look, look_config, rebase, set_current, update, update_at};
use {look::*, migrate::*, parse::*};

/// Schema version this build reads and writes.
pub const VERSION: u32 = 12;
const DEFAULT_CAPTURE_SHORTCUT: &str = "Ctrl+Shift+Space";

/// The OS's language as the language table has it, else English
/// (`lang::yours`): a new config's `translate.target` (owner, 2026-11-10: no
/// Arabic default) and onboarding's preselected "Your language". Tests get
/// English on every machine.
pub fn os_language() -> &'static str {
    if cfg!(test) {
        return "en";
    }
    static OS: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    OS.get_or_init(|| {
        crate::features::translate::lang::yours(&crate::shared::platform::sys::locale::language())
    })
}
/// Ends the warning when `Provider::Unreadable` is set.
const AI_OFF: &str = ", but AI is off until you choose a provider in Settings > Ask AI";
/// Live translate repeats this often (ms); the allowed range is `LIVE_INTERVAL_MS`.
pub const DEFAULT_LIVE_INTERVAL_MS: u32 = 1500;
pub const LIVE_INTERVAL_MS: std::ops::RangeInclusive<u32> = 500..=5000;
/// Long edge (px) Copy / Save scale a capture down to: what AI chats take
/// without shrinking it again themselves. `0` = original size.
pub const DEFAULT_MAX_EDGE: u32 = 0;
pub const MAX_EDGE: std::ops::RangeInclusive<u32> = 256..=8192;
/// The heights (px) a long capture may grow to (`snip.long_max`): the choices
/// in Settings > Snip; the tallest is the default.
pub const LONG_MAX: [u32; 3] = [5_000, 10_000, 20_000];
pub const DEFAULT_LONG_MAX: u32 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The dark themes `[appearance] dark` may name (`shared::kit::themes`; a test there keeps
/// both lists equal to its own).
pub const DARK_THEMES: [&str; 9] = [
    "sapphire", "obsidian", "mica", "titanium", "amethyst", "emerald", "rose", "graphite",
    "midnight",
];
/// The light themes `[appearance] light` may name.
pub const LIGHT_THEMES: [&str; 3] = ["frost", "daylight", "sandstone"];

/// `[appearance]` (schema 4): the app theme picked for each OS mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppearanceConfig {
    /// A `DARK_THEMES` id.
    pub dark: String,
    /// A `LIGHT_THEMES` id.
    pub light: String,
    pub logo: Logo,
    /// The Settings pages whose feature preview is hidden.
    pub hidden_previews: Vec<String>,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        AppearanceConfig {
            dark: "sapphire".into(),
            light: "daylight".into(),
            logo: Logo::Theme,
            hidden_previews: Vec::new(),
        }
    }
}

/// The logo's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Logo {
    /// The theme's accent.
    #[default]
    Theme,
    Purple,
}

/// The look of the floating widgets (bar, cards, toolbar), from `shared/theme.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetTheme {
    /// Follows the app theme (and the look's bar style).
    #[default]
    Default,
    /// Frosted glass: a light tint over the blurred screen.
    Frost,
    /// Calm dark graphite.
    Graphite,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Provider {
    /// No key: the question opens in Google AI Mode in the user's own browser.
    #[default]
    Browser,
    Anthropic,
    OpenaiCompatible,
    /// The config names a provider we don't know (a typo like "antropic"): the
    /// user's value as written. Nothing is sent anywhere until they pick one.
    Invalid(String),
    /// The config file exists but could not be read or parsed (or `ai` is not a
    /// table): we can't know what the user chose, so AI is off. Never written.
    Unreadable,
}

impl Provider {
    /// Stable id, used for the keyring entry name.
    pub fn id(&self) -> &'static str {
        match self {
            Provider::Browser => "browser",
            Provider::Anthropic => "anthropic",
            Provider::OpenaiCompatible => "openai_compatible",
            Provider::Invalid(_) => "invalid",
            Provider::Unreadable => "unreadable",
        }
    }

    /// Missing = the default (Browser). Present but unknown = `Invalid`, never a
    /// fallback: that would send the user's screen to someone they didn't choose.
    fn read(t: &Table, w: &mut Vec<String>) -> Provider {
        let Some(v) = t.get("provider") else {
            return Provider::default();
        };
        let raw = v.as_str().map_or_else(|| v.to_string(), str::to_string);
        match raw.as_str() {
            "browser" => Provider::Browser,
            "anthropic" => Provider::Anthropic,
            "openai_compatible" => Provider::OpenaiCompatible,
            _ => {
                w.push(format!(
                    "ai.provider: \"{raw}\" is not browser, anthropic or openai_compatible; \
                     AI is off until you choose a provider in Settings > Ask AI"
                ));
                Provider::Invalid(raw)
            }
        }
    }
}

/// Written as its id; `Invalid` writes the user's own value back, never "browser".
/// `Unreadable` refuses: saving must not replace an unread choice with a guess.
impl Serialize for Provider {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Provider::Invalid(raw) => s.serialize_str(raw),
            Provider::Unreadable => Err(serde::ser::Error::custom(
                "ai.provider: choose a provider (the settings file couldn't be read)",
            )),
            p => s.serialize_str(p.id()),
        }
    }
}

/// How text is translated. Anything else in the file is read as `Offline` with a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// The AI provider (`[ai]`). Without a usable one (the browser provider,
    /// no key) translation falls back to `Offline` (`actions::translate::deps`).
    Ai,
    /// On-device model (`crate::features::translate::mt`): no key, nothing leaves the PC. The
    /// default: AI only when the user picks it in Settings > Translate.
    #[default]
    Offline,
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
    #[cfg(test)]
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
    pub action: QuickId,
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
    /// "auto" or a language code: what the text is in (picks the OCR
    /// recognizer and the offline model).
    pub source: String,
    pub target: String,
    pub engine: Engine,
    /// Live translate repeats every this many ms (`LIVE_INTERVAL_MS`).
    pub live_interval_ms: u32,
    /// Live mode's one-time note (games must be borderless) was shown.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub live_note_shown: bool,
    /// Live translate reads the whole screen instead of the
    /// foreground window's page (`capture::window_rect::region`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub whole_screen: bool,
    /// The language picker's recent picks, most recent first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recent_sources: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recent_targets: Vec<String>,
}

/// What a finished snip does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnipAfter {
    /// Picked with the Snip toolbar, copied only by Ctrl+C or the Copy button
    /// (the default since schema 10).
    #[default]
    Editor,
    /// Picked with the Snip toolbar and already copied. The retired "copy"
    /// (copy and close, schema 6) reads as this.
    #[serde(alias = "copy")]
    Both,
}

/// The file Save writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Png,
    /// Quality 85.
    Jpeg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnipConfig {
    pub after: SnipAfter,
    /// Copy / Save scale the long edge down to this (`MAX_EDGE`); 0 = original size.
    pub max_edge: u32,
    pub format: Format,
    /// Save keeps the original size (Copy still scales).
    pub save_original: bool,
    /// Draw the mouse pointer into the capture.
    pub cursor: bool,
    /// Keep the last captures in the user's data dir (off = nothing on disk).
    pub history: bool,
    /// Seconds to wait (the overlay hidden) before a snip captures; 0 = off.
    pub delay: u8,
    /// A long capture stops growing at this height (px, one of `LONG_MAX`).
    /// Written only when changed: the default file stays as it was.
    #[serde(skip_serializing_if = "is_default_long_max")]
    pub long_max: u32,
}

fn is_default_long_max(px: &u32) -> bool {
    *px == DEFAULT_LONG_MAX
}

impl Default for SnipConfig {
    fn default() -> Self {
        SnipConfig {
            after: SnipAfter::Editor,
            max_edge: DEFAULT_MAX_EDGE,
            format: Format::Png,
            save_original: false,
            cursor: false,
            history: false,
            delay: 0,
            long_max: DEFAULT_LONG_MAX,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneralConfig {
    pub default_action: DefaultAction,
    /// Where the Save dialog starts; "" = the system's choice.
    pub save_folder: String,
    /// Start OpenCircle with the OS (`autostart`). On by default.
    pub autostart: bool,
    /// The app language onboarding's "Your language" picked (a language code);
    /// "" = the OS language. Read from the multi-language release on.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// Settings' App language: "" = follow `language`, else a UI language code
    /// ("en", "ar"); written only when the user picks one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub app_language: String,
    /// The shortcuts another app held when the start notice last said so
    /// (`hotkey::taken_notice`): the same ones aren't told again.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shortcuts_taken_shown: Vec<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            default_action: DefaultAction::default(),
            save_folder: String::new(),
            autostart: true,
            language: String::new(),
            app_language: String::new(),
            shortcuts_taken_shown: Vec::new(),
        }
    }
}

/// The overlay's action bar (`[bar]`): the spot the user docked it at. The
/// app writes it when a drag ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct BarConfig {
    pub anchor: BarAnchor,
}

/// Where the overlay's action bar docks (always inside the work area).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarAnchor {
    /// Centred, above the taskbar.
    #[default]
    Bottom,
    /// Centred at the top, under the hint.
    Top,
    BottomLeft,
    BottomRight,
    /// The top corners, level with `Top` (added 2026-10-29).
    TopLeft,
    TopRight,
}

/// How a finished stroke shows (`[selection] style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectStyle {
    /// The stroke turns into a rounded box with corner brackets.
    #[default]
    Snap,
    /// The stroke stays as drawn.
    Shape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct SelectionConfig {
    pub style: SelectStyle,
    pub text_reader: TextReader,
}

/// Who reads the screen's text for selecting it (`ocr::route`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextReader {
    /// Windows OCR where it has the language (not right-to-left), else PP-OCR.
    #[default]
    Auto,
    /// Windows OCR whenever it has a recognizer.
    Windows,
    /// Always the downloaded text reader (PP-OCR): Arabic, games.
    PpOcr,
}

/// The active look: a preset, or the user's own bundle (`[look.custom]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookTheme {
    Spectrum,
    /// The thin violet-to-cyan line: the design's Prism (stored as "aurora").
    Aurora,
    Sunset,
    Ocean,
    Neon,
    /// White and grey: the design's Frost (stored as "mono").
    Mono,
    Calm,
    Custom,
    /// Schema 4: the design's other looks (`shared::kit::looks`), written only when picked.
    Horizon,
    Solar,
    Hud,
    /// The design's Aurora Borealis ("aurora" is its Prism).
    Borealis,
    /// The main look (2026-11-03): the owner's blue-to-indigo glow.
    #[default]
    SapphireGlow,
}

/// The edge / accent colours: a preset palette (its colours live in
/// `shared/theme.rs`) or two of the user's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourSet {
    #[default]
    Spectrum,
    Aurora,
    Sunset,
    Ocean,
    Neon,
    Mono,
    TwoColour,
}

/// The screen edge: a soft glow fading inward, a thin line, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeStyle {
    #[default]
    Glow,
    Thin,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BracketColour {
    #[default]
    White,
    Accent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BracketStyle {
    /// Four L-shaped corners.
    #[default]
    Corners,
    /// A thin outline all round.
    Frame,
}

/// The floating bar / pill surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarStyle {
    /// Glass in the app theme's colours.
    #[default]
    Glass,
    GlassDark,
    GlassLight,
    /// Opaque, app theme colours.
    Solid,
    /// See-through with an accent outline.
    Outline,
}

/// A colour written as "#rrggbb".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().strip_prefix('#')?;
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        Some(Rgb([byte(0)?, byte(2)?, byte(4)?]))
    }

    /// `#rrggbb`, or `#RRGGBB` with `upper` (the file always writes lower case).
    pub fn hex(self, upper: bool) -> String {
        let [r, g, b] = self.0;
        match upper {
            true => format!("#{r:02X}{g:02X}{b:02X}"),
            false => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex(false))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Rgb, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).ok_or_else(|| serde::de::Error::custom("expected a colour like \"#8f80ff\""))
    }
}

/// One mode's screen edge (overlay, live translate, snip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Edge {
    pub style: EdgeStyle,
    /// Logical px (`EDGE_THICKNESS`); a glow reaches 8x this far inward.
    pub thickness: u8,
    /// Read, ignored, not written (schema 11): the edge is static.
    #[serde(skip_serializing)]
    pub animate: bool,
}

pub const EDGE_THICKNESS: std::ops::RangeInclusive<u8> = 1..=12;
pub const GLOW: std::ops::RangeInclusive<u8> = 0..=100;
pub const DIM: std::ops::RangeInclusive<u8> = 0..=60;

/// Everything an active look sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Look {
    pub palette: ColourSet,
    /// `palette = two_colour`: from, to.
    pub colours: [Rgb; 2],
    /// Edge glow strength, % (`GLOW`).
    pub glow: u8,
    /// Dim over the frozen screen, % (`DIM`).
    pub dim: u8,
    /// Read, ignored, not written (schema 11).
    #[serde(skip_serializing)]
    pub activation_glow: bool,
    pub brackets: BracketColour,
    pub bracket_style: BracketStyle,
    pub bar: BarStyle,
    pub overlay: Edge,
    pub live: Edge,
    pub snip: Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LookConfig {
    pub theme: LookTheme,
    /// The user's own look (`theme = "custom"`); kept when a preset is chosen.
    pub custom: Look,
}

/// Where a screen edge is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeMode {
    Overlay,
    Live,
    Snip,
}

impl EdgeMode {
    pub const ALL: [EdgeMode; 3] = [EdgeMode::Overlay, EdgeMode::Live, EdgeMode::Snip];

    fn key(self) -> &'static str {
        match self {
            EdgeMode::Overlay => "overlay",
            EdgeMode::Live => "live",
            EdgeMode::Snip => "snip",
        }
    }
}

impl LookConfig {
    /// The look in use.
    pub fn active(&self) -> Look {
        self.theme.look(&self.custom)
    }

    /// Edit `mode`'s edge of the custom look (Settings).
    pub fn edge_mut(&mut self, mode: EdgeMode) -> &mut Edge {
        self.custom.edge_mut(mode)
    }
}

impl Default for LookConfig {
    fn default() -> Self {
        LookConfig {
            theme: LookTheme::SapphireGlow,
            custom: PRESETS[0].1,
        }
    }
}

/// The annotation editor's remembered options. The others are written only once
/// the editor changed them (`None` = the editor's default; the default file stays
/// as frozen).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorConfig {
    /// The blur brush keeps to the text line it starts on.
    pub line_assist: bool,
    /// The last tool (never Crop).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<crate::features::snip::annotate::Tool>,
    /// `#RRGGBB`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    /// The shared size (pen, arrows, shapes, steps, callouts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<crate::features::snip::annotate::Width>,
    /// The Text tool's font size, display px (`TEXT_SIZE`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_size: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<bool>,
    /// The blur brush's picked size; `None` = Auto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur_size: Option<crate::features::snip::annotate::Width>,
    /// The blur's pixel size (its mosaic cells).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur_cell: Option<crate::features::snip::annotate::Width>,
    /// The line width of shapes, arrows and the pen, display px (`STROKE`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<u8>,
    /// The Text tool's face, weight and background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_font: Option<crate::features::snip::annotate::TextFont>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_back: Option<crate::features::snip::annotate::TextBack>,
    /// Each toolbar group's last tool (the grouped toolbar shows it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_tools: Option<Vec<crate::features::snip::annotate::Tool>>,
}

/// `editor.text_size`'s range (display px).
pub const TEXT_SIZE: std::ops::RangeInclusive<u8> = 8..=96;
/// `editor.stroke`'s range (display px).
pub const STROKE: std::ops::RangeInclusive<u8> = 1..=32;

impl Default for EditorConfig {
    fn default() -> Self {
        EditorConfig {
            line_assist: true,
            tool: None,
            colour: None,
            size: None,
            text_size: None,
            outline: None,
            fill: None,
            blur_size: None,
            blur_cell: None,
            stroke: None,
            text_font: None,
            text_bold: None,
            text_back: None,
            group_tools: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Config {
    pub version: u32,
    pub theme: Theme,
    /// Blur the screen behind `Frost` / `Graphite` widgets (off = solid tint).
    pub glass_effect: bool,
    pub shortcut: String,
    pub appearance: AppearanceConfig,
    pub general: GeneralConfig,
    pub bar: BarConfig,
    pub selection: SelectionConfig,
    pub look: LookConfig,
    pub editor: EditorConfig,
    pub ai: AiConfig,
    /// Written only once changed (the default file stays as frozen).
    #[serde(skip_serializing_if = "ChatConfig::is_default")]
    pub chat: ChatConfig,
    pub translate: TranslateConfig,
    pub snip: SnipConfig,
    pub quick_actions: Vec<QuickAction>,
    pub song: SongConfig,
    pub update: UpdateConfig,
    pub labs: Labs,
}

/// `[chat]`: the AI chat card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChatConfig {
    /// When the chat pins itself: it then stays open (docked above other
    /// apps after the overlay closes) until its pin button closes it.
    pub stays_open: StaysOpen,
    /// Chats are saved on this PC (text only, `history::chat`); off = none kept.
    pub keep_history: bool,
}

impl ChatConfig {
    fn is_default(&self) -> bool {
        *self == ChatConfig::default()
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        ChatConfig {
            stays_open: StaysOpen::FollowUp,
            keep_history: true,
        }
    }
}

/// Settings > Ask AI "AI chat stays open".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaysOpen {
    /// After a follow-up question (one answer alone closes as before).
    #[default]
    FollowUp,
    /// From the first question.
    Always,
}

/// `[update]`: automatic updates from GitHub Releases (`crate::shared::update`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct UpdateConfig {
    /// `None` = never asked (no checks); onboarding asks, Settings changes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
}

/// What song search listens to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongSource {
    /// What this PC plays (loopback / the sink's monitor).
    #[default]
    System,
    Microphone,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct SongConfig {
    pub source: SongSource,
    /// Keep recent songs on this PC (`music::history`). Off by default.
    pub history: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: Provider::Browser,
            base_url: String::new(),
            model: crate::features::ask_ai::DEFAULT_ANTHROPIC_MODEL.to_string(),
        }
    }
}

impl Default for TranslateConfig {
    fn default() -> Self {
        TranslateConfig {
            source: crate::features::translate::lang::AUTO.to_string(),
            target: os_language().to_string(),
            engine: Engine::Offline,
            live_interval_ms: DEFAULT_LIVE_INTERVAL_MS,
            live_note_shown: false,
            whole_screen: false,
            recent_sources: Vec::new(),
            recent_targets: Vec::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let qa = |action: &str, shortcut: &str, target, live| QuickAction {
            action: action.into(),
            shortcut: shortcut.to_string(),
            target,
            live,
        };
        Config {
            version: VERSION,
            theme: Theme::System,
            glass_effect: true,
            shortcut: DEFAULT_CAPTURE_SHORTCUT.to_string(),
            appearance: AppearanceConfig::default(),
            general: GeneralConfig::default(),
            bar: BarConfig::default(),
            selection: SelectionConfig::default(),
            look: LookConfig::default(),
            editor: EditorConfig::default(),
            ai: AiConfig::default(),
            chat: ChatConfig::default(),
            translate: TranslateConfig::default(),
            snip: SnipConfig::default(),
            // DESIGN.md "Quick actions".
            quick_actions: vec![
                qa("translate", "Alt+Shift+T", Target::FullScreen, false),
                qa("translate", "Alt+Shift+L", Target::FullScreen, true),
                qa("ai_explain", "Ctrl+Shift+E", Target::Circle, false),
                qa("copy_text", "Ctrl+Shift+X", Target::Circle, false),
                qa("lens", "", Target::Circle, false),
                // Snip: one shortcut, a common screenshot key that leaves Print
                // Screen to Windows (owner, 2026-11-03).
                qa("snip", "Alt+A", Target::Circle, false),
                // Opt-in (needs the user's key): no key bound by default.
                qa("song_search", "", Target::FullScreen, false),
            ],
            song: SongConfig::default(),
            update: UpdateConfig::default(),
            labs: Labs::default(),
        }
    }
}

/// The key a quick action starts with ("" = none): read from `Config::default`'s list,
/// the one place the defaults live (Settings' "Reset shortcuts").
pub fn default_quick_shortcut(action: &QuickId, live: bool) -> String {
    let all = Config::default().quick_actions;
    let found = all
        .into_iter()
        .find(|q| q.action == *action && q.live == live);
    found.map(|q| q.shortcut).unwrap_or_default()
}

/// Parse a shortcut string ("Ctrl+Shift+Space") with `global-hotkey`'s parser,
/// plus one rule of ours: a bare typing key (letter, digit, Space, ...) cannot
/// be global; only F-keys, PrintScreen, Pause and ScrollLock may stand alone.
pub fn parse_shortcut(s: &str) -> Result<HotKey, String> {
    parse_shortcut_typed(s).map_err(|e| e.to_string())
}

/// Why a shortcut string is refused: the UI words it in its language
/// (`Display` is the English of the settings file's messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutFault {
    UnknownKey(String),
    Shape,
    NeedsModifier,
}

impl std::fmt::Display for ShortcutFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShortcutFault::UnknownKey(k) => write!(f, "unknown key \"{k}\""),
            ShortcutFault::Shape => {
                f.write_str("expected modifiers then one key, like Ctrl+Shift+Space")
            }
            ShortcutFault::NeedsModifier => {
                f.write_str("needs at least one modifier (Ctrl, Alt, Shift, Super)")
            }
        }
    }
}

/// `parse_shortcut` with a typed refusal.
pub fn parse_shortcut_typed(s: &str) -> Result<HotKey, ShortcutFault> {
    let hk: HotKey = s.trim().parse().map_err(|e: HotKeyParseError| match e {
        HotKeyParseError::UnsupportedKey(k) => ShortcutFault::UnknownKey(k.to_string()),
        _ => ShortcutFault::Shape,
    })?;
    if hk.mods == Modifiers::empty() && !may_stand_alone(hk.key) {
        return Err(ShortcutFault::NeedsModifier);
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
    /// Quick actions that came after the default file was frozen (that file stays byte for
    /// byte): Settings adds the missing ones, unbound, so their rows show; a file has them
    /// once Settings saved it.
    pub fn add_new_quick_actions(&mut self) {
        // Labs Pins, Windows: a part of a window kept live on top.
        // Snip's screen tools: the colour picker and the ruler (unbound: no free
        // combination is safe on every PC).
        let unbound = |action, target| QuickAction {
            action,
            shortcut: String::new(),
            target,
            live: false,
        };
        let new = [
            unbound(QuickId::LivePin, Target::Circle),
            unbound(QuickId::ColourPicker, Target::FullScreen),
            unbound(QuickId::Ruler, Target::FullScreen),
        ];
        for q in new {
            if !self.quick_actions.iter().any(|c| c.action == q.action) {
                self.quick_actions.push(q);
            }
        }
    }

    /// `<user config dir>/opencircle/config.toml`; `None` if the OS has no config dir,
    /// and in tests (hermetic: the machine's config, e.g. a widget theme, never leaks in).
    pub fn path() -> Option<PathBuf> {
        if cfg!(test) {
            return None;
        }
        crate::shared::paths::config_file()
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

    /// The user's config read as is, never migrated or written (the setup only
    /// looks at it; the app it installs migrates it on its first start).
    pub fn peek() -> (Config, Vec<String>) {
        match Self::path().map(std::fs::read_to_string) {
            Some(Ok(text)) => Self::parse(&text),
            _ => (Config::default(), vec![]),
        }
    }

    pub fn load_from(path: &Path) -> (Config, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let (config, warnings) = Self::parse(&text);
                if warnings.is_empty() {
                    migrate(path, &text, &config);
                }
                (config, warnings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), vec![]),
            Err(e) => (
                Self::unreadable(),
                vec![format!(
                    "cannot read the config file ({e}); using defaults{AI_OFF}"
                )],
            ),
        }
    }

    /// Defaults, but AI off: the file is there, so the user may have chosen a
    /// provider we can't see. Never guess Browser for them.
    fn unreadable() -> Config {
        let mut c = Config::default();
        c.ai.provider = Provider::Unreadable;
        c
    }

    /// Write atomically (a uniquely named temp file in the same folder, then
    /// rename). Writers go through `update`, which serialises them.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let body = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let text = format!(
            "# OpenCircle settings. Invalid values fall back to defaults.\n# API keys are never stored here (OS keyring only).\n\n{body}"
        );
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path.with_extension(format!("toml.tmp{}-{n}", std::process::id()));
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

#[cfg(test)]
mod tests;
