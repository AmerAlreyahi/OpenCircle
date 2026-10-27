//! The one config reader/validator: `<user config dir>/opencircle/config.toml`.
//!
//! Schema (version 3). Every key is optional; a missing file means defaults.
//!
//! ```toml
//! version = 3                       # schema version, written by save()
//! theme = "system"                  # system | light | dark
//! widget_theme = "default"          # floating widgets (bar, cards, toolbar): default | frost | graphite
//! glass_effect = true               # frost / graphite blur the screen behind them (false = solid tint)
//! shortcut = "Ctrl+Shift+Space"     # global capture shortcut
//!
//! [general]
//! default_action = "lens"           # Enter on the action bar: lens | ai | text | copy
//! save_folder = ""                  # Save dialog starts here; "" = the system's choice
//! search_history = false            # keep the searches handed to the browser on this PC (opt-in)
//! autostart = true                  # start with Windows / at login (tray only, no window)
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
//! target = "ar"                     # language code to translate into
//! engine = "ai"                     # ai | offline (ai falls back to offline when AI isn't set up)
//! live_interval_ms = 1500           # live translate repeats every N ms (500-5000)
//! live_note_shown = false           # the one-time live-mode note was shown
//! whole_screen = false              # live translate reads the whole screen,
//!                                   # not just the foreground window (no taskbar, no browser bars)
//!
//! [selection]
//! style = "snap"                    # snap (a rounded box with corner brackets) | shape (as drawn)
//! after = "wait"                    # wait (nothing is sent until the user picks an action) | search (run search_with at once)
//! search_with = "lens"              # lens | ai | both
//! text_reader = "auto"              # who reads text to select: auto | windows (Windows OCR) | pp_ocr
//!
//! [look]                            # how the screen looks while OpenCircle is active
//! theme = "spectrum"                # spectrum | aurora | sunset | ocean | neon | mono | calm | custom
//! [look.custom]                     # theme = "custom" only (a copy of a preset to edit)
//! palette = "spectrum"              # spectrum | aurora | sunset | ocean | neon | mono | two_colour
//! colours = ["#8f80ff", "#4fc9e0"]  # palette = two_colour: from, to
//! glow = 100                        # edge glow strength, 0-100 % (100 = the design's; 0 = no edge)
//! dim = 12                          # dim over the frozen screen, 0-60 %
//! activation_glow = true            # kept, unused: the edge is static (2026-10-27)
//! brackets = "white"                # white | accent: the selection's corner colour
//! bracket_style = "corners"         # corners | frame
//! bar = "glass"                     # glass | glass_dark | glass_light | solid | outline
//! [look.custom.overlay]             # the screen edge per mode: overlay, live, snip
//! style = "glow"                    # glow | none (thin: the glow too)
//! thickness = 6                     # 1-12 px; only the line on the live layer (Windows)
//! animate = true                    # kept, unused: the edge is static
//! [song]                            # song search (the service key: OS keyring only)
//! source = "system"                 # system (what this PC plays) | microphone
//! history = false                   # keep recent songs on this PC
//! [update]
//! auto = true                       # keep OpenCircle up to date (unset = not asked yet: off)
//! [snip]
//! after = "copy"                    # after a capture: copy | editor | both
//! max_edge = 1568                   # Copy / Save scale the long edge down to this; 0 = original
//! format = "png"                    # Save writes png | jpeg (quality 85)
//! save_original = false             # Save ignores max_edge
//! cursor = false                    # draw the mouse pointer into the capture
//! history = false                   # keep the last 20 captures (user data dir only)
//! delay = 0                         # timer before a snip: 0 (off) | 3 | 5 | 10 seconds
//!
//! [labs]                            # unfinished features: off = hidden everywhere, settings kept
//! snip = false                      # Snip mode, its shortcuts and tray item
//! editor = false                    # the image editor (Edit image)
//! song = false                      # song search
//! history = false                   # the local search history
//! pins = false                      # pinning a capture to the screen
//! live = false                      # live translate (unfinished until 0.3.0): bar button, tray item, shortcut
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
//! crash and never a rewrite of the file (only `save` writes). One exception:
//! an unknown `ai.provider` is kept as `Provider::Invalid` (fail closed: AI is
//! off until the user picks one, never a fallback to another provider); a file
//! that exists but can't be read or parsed gives `Provider::Unreadable`. Unknown
//! keys are ignored. Once released, this format is frozen: migrate, never break.
//!
//! Versions: 1 = v0.1.0; 2 adds `[labs]`; 3 sets `selection.after` to "wait" (nothing
//! is sent before the user picks an action) and drops `[results]` (the web view card
//! is gone: its keys are ignored). A file older than `VERSION` is copied to
//! `config.toml.bak-v<old>` (never over an existing copy) and then rewritten as the
//! current version, by `migrate` (add one step there per schema change).

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use global_hotkey::hotkey::{Code, HotKey, HotKeyParseError, Modifiers};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::labs::Labs;

/// Schema version this build reads and writes.
pub const VERSION: u32 = 3;
const DEFAULT_CAPTURE_SHORTCUT: &str = "Ctrl+Shift+Space";
const DEFAULT_TRANSLATE_TARGET: &str = "ar";
/// Ends the warning when `Provider::Unreadable` is set.
const AI_OFF: &str = ", but AI is off until you choose a provider in Settings > AI";
/// Live translate repeats this often (ms); the allowed range is `LIVE_INTERVAL_MS`.
pub const DEFAULT_LIVE_INTERVAL_MS: u32 = 1500;
pub const LIVE_INTERVAL_MS: std::ops::RangeInclusive<u32> = 500..=5000;
/// Long edge (px) Copy / Save scale a capture down to: what AI chats take
/// without shrinking it again themselves. `0` = original size.
pub const DEFAULT_MAX_EDGE: u32 = 1568;
pub const MAX_EDGE: std::ops::RangeInclusive<u32> = 256..=8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The look of the floating widgets (bar, cards, toolbar), from `ui/theme.rs`.
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
                     AI is off until you choose a provider in Settings > AI"
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

/// How text is translated. Anything else in the file is read as `Ai` with a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// The AI provider (`[ai]`). Without a usable one (the browser provider,
    /// no key) translation falls back to `Offline` (`actions::translate::deps`).
    #[default]
    Ai,
    /// On-device model (`crate::mt`): no key, nothing leaves the PC.
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
}

/// What a finished snip does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnipAfter {
    /// To the clipboard at once (a toast offers Edit for a moment).
    #[default]
    Copy,
    /// The editor opens on it.
    Editor,
    /// Both.
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
}

impl Default for SnipConfig {
    fn default() -> Self {
        SnipConfig {
            after: SnipAfter::Copy,
            max_edge: DEFAULT_MAX_EDGE,
            format: Format::Png,
            save_original: false,
            cursor: false,
            history: false,
            delay: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneralConfig {
    pub default_action: DefaultAction,
    /// Where the Save dialog starts; "" = the system's choice.
    pub save_folder: String,
    /// Keep the searches handed to the browser on this PC (`search_history`). Off by default.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub search_history: bool,
    /// Start OpenCircle with the OS (`autostart`). On by default.
    pub autostart: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            default_action: DefaultAction::default(),
            save_folder: String::new(),
            search_history: false,
            autostart: true,
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

/// What happens when a selection is made (`[selection] after`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    /// Run `search_with` at once (the user opted in: "Auto action").
    Search,
    /// Show the action bar and wait: nothing is sent until the user picks.
    #[default]
    Wait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchWith {
    #[default]
    Lens,
    Ai,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct SelectionConfig {
    pub style: SelectStyle,
    pub after: After,
    pub search_with: SearchWith,
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
    #[default]
    Spectrum,
    Aurora,
    Sunset,
    Ocean,
    Neon,
    Mono,
    Calm,
    Custom,
}

/// The edge / accent colours: a preset palette (its colours live in
/// `ui/theme.rs`) or two of the user's own.
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

    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
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
    /// Overlay / snip: once when it opens. Live: all the time.
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

const fn edge(style: EdgeStyle, thickness: u8, animate: bool) -> Edge {
    Edge {
        style,
        thickness,
        animate,
    }
}

const fn preset(palette: ColourSet, glow: u8, dim: u8, activation: bool, bar: BarStyle) -> Look {
    Look {
        palette,
        colours: [Rgb([0x8f, 0x80, 0xff]), Rgb([0x4f, 0xc9, 0xe0])],
        glow,
        dim,
        activation_glow: activation,
        brackets: BracketColour::White,
        bracket_style: BracketStyle::Corners,
        bar,
        overlay: edge(EdgeStyle::Glow, 6, true),
        live: edge(EdgeStyle::Glow, 6, true),
        snip: edge(EdgeStyle::Thin, 3, false),
    }
}

/// The presets: one table (their colours: `ui::theme::palette`).
pub const PRESETS: [(LookTheme, Look); 7] = {
    let thin = edge(EdgeStyle::Thin, 3, true);
    let still = edge(EdgeStyle::Thin, 2, false);
    [
        (
            LookTheme::Spectrum,
            preset(ColourSet::Spectrum, 100, 12, true, BarStyle::Glass),
        ),
        // Today's look: the thin violet → cyan line with its sweep.
        (
            LookTheme::Aurora,
            Look {
                overlay: thin,
                live: thin,
                ..preset(ColourSet::Aurora, 100, 12, false, BarStyle::Glass)
            },
        ),
        (
            LookTheme::Sunset,
            preset(ColourSet::Sunset, 100, 15, true, BarStyle::GlassDark),
        ),
        (
            LookTheme::Ocean,
            preset(ColourSet::Ocean, 100, 12, true, BarStyle::Glass),
        ),
        (
            LookTheme::Neon,
            Look {
                brackets: BracketColour::Accent,
                overlay: edge(EdgeStyle::Glow, 8, true),
                live: edge(EdgeStyle::Glow, 8, true),
                ..preset(ColourSet::Neon, 100, 25, true, BarStyle::Outline)
            },
        ),
        (
            LookTheme::Mono,
            Look {
                bracket_style: BracketStyle::Frame,
                overlay: thin,
                live: thin,
                ..preset(ColourSet::Mono, 100, 10, false, BarStyle::Solid)
            },
        ),
        (
            LookTheme::Calm,
            Look {
                overlay: still,
                live: still,
                snip: still,
                ..preset(ColourSet::Aurora, 0, 6, false, BarStyle::Glass)
            },
        ),
    ]
};

impl LookTheme {
    /// What this theme looks like (`custom` for `Custom`).
    pub fn look(self, custom: &Look) -> Look {
        PRESETS
            .iter()
            .find(|(t, _)| *t == self)
            .map_or(*custom, |(_, look)| *look)
    }
}

impl Look {
    pub fn edge(&self, mode: EdgeMode) -> Edge {
        match mode {
            EdgeMode::Overlay => self.overlay,
            EdgeMode::Live => self.live,
            EdgeMode::Snip => self.snip,
        }
    }

    fn edge_mut(&mut self, mode: EdgeMode) -> &mut Edge {
        match mode {
            EdgeMode::Overlay => &mut self.overlay,
            EdgeMode::Live => &mut self.live,
            EdgeMode::Snip => &mut self.snip,
        }
    }
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
            theme: LookTheme::Spectrum,
            custom: PRESETS[0].1,
        }
    }
}

/// The annotation editor's remembered options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorConfig {
    /// The blur brush keeps to the text line it starts on.
    pub line_assist: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        EditorConfig { line_assist: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Config {
    pub version: u32,
    pub theme: Theme,
    pub widget_theme: WidgetTheme,
    /// Blur the screen behind `Frost` / `Graphite` widgets (off = solid tint).
    pub glass_effect: bool,
    pub shortcut: String,
    pub general: GeneralConfig,
    pub bar: BarConfig,
    pub selection: SelectionConfig,
    pub look: LookConfig,
    pub editor: EditorConfig,
    pub ai: AiConfig,
    pub translate: TranslateConfig,
    pub snip: SnipConfig,
    pub quick_actions: Vec<QuickAction>,
    pub song: SongConfig,
    pub update: UpdateConfig,
    pub labs: Labs,
}

/// `[update]`: automatic updates from GitHub Releases (`crate::update`).
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
            model: crate::ai::DEFAULT_ANTHROPIC_MODEL.to_string(),
        }
    }
}

impl Default for TranslateConfig {
    fn default() -> Self {
        TranslateConfig {
            source: crate::translate::lang::AUTO.to_string(),
            target: DEFAULT_TRANSLATE_TARGET.to_string(),
            engine: Engine::Ai,
            live_interval_ms: DEFAULT_LIVE_INTERVAL_MS,
            live_note_shown: false,
            whole_screen: false,
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
            widget_theme: WidgetTheme::Default,
            glass_effect: true,
            shortcut: DEFAULT_CAPTURE_SHORTCUT.to_string(),
            general: GeneralConfig::default(),
            bar: BarConfig::default(),
            selection: SelectionConfig::default(),
            look: LookConfig::default(),
            editor: EditorConfig::default(),
            ai: AiConfig::default(),
            translate: TranslateConfig::default(),
            snip: SnipConfig::default(),
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
                // Opt-in (needs the user's key): no key bound by default.
                qa("song_search", "", Target::FullScreen, false),
            ],
            song: SongConfig::default(),
            update: UpdateConfig::default(),
            labs: Labs::default(),
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
    /// `<user config dir>/opencircle/config.toml`; `None` if the OS has no config dir,
    /// and in tests (hermetic: the machine's config, e.g. a widget theme, never leaks in).
    pub fn path() -> Option<PathBuf> {
        if cfg!(test) {
            return None;
        }
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

    /// Validate TOML text field by field.
    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut w = Vec::new();
        let root: Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                let line = e.to_string().lines().next().unwrap_or_default().to_string();
                return (
                    Self::unreadable(),
                    vec![format!(
                        "config is not valid TOML ({line}: {}); using defaults{AI_OFF}",
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
                Some(n) if (1..=VERSION as i64).contains(&n) => {}
                Some(n) if n > VERSION as i64 => w.push(format!(
                    "version: written by a newer OpenCircle (schema {n}); unknown keys are ignored and lost on save"
                )),
                _ => w.push(format!(
                    "version: expected 1 to {VERSION}; reading it as version {VERSION}"
                )),
            }
        }
        c.theme = get(&root, "", "theme", d.theme, &mut w);
        c.widget_theme = get(&root, "", "widget_theme", d.widget_theme, &mut w);
        c.glass_effect = get(&root, "", "glass_effect", d.glass_effect, &mut w);
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
            c.general.search_history = get(
                t,
                "general.",
                "search_history",
                d.general.search_history,
                &mut w,
            );
            c.general.autostart = get(t, "general.", "autostart", d.general.autostart, &mut w);
        }
        // `[results]` (schema 1-2: the web view card's side and size) is ignored.
        if let Some(t) = section(&root, "bar", &mut w) {
            c.bar.anchor = get(t, "bar.", "anchor", d.bar.anchor, &mut w);
        }
        if let Some(t) = section(&root, "selection", &mut w) {
            let (p, d) = ("selection.", d.selection);
            c.selection.style = get(t, p, "style", d.style, &mut w);
            c.selection.after = get(t, p, "after", d.after, &mut w);
            // Schema 3: nothing is sent before the user picks (owner, 2026-10-26).
            // An older file's "search" becomes "wait"; Settings opts back in.
            if root.get("version").map_or(Some(1), Value::as_integer) < Some(3) {
                c.selection.after = After::Wait;
            }
            c.selection.search_with = get(t, p, "search_with", d.search_with, &mut w);
            c.selection.text_reader = get(t, p, "text_reader", d.text_reader, &mut w);
        }
        if let Some(t) = section(&root, "look", &mut w) {
            c.look.theme = get(t, "look.", "theme", d.look.theme, &mut w);
            if let Some(custom) = sub_section(t, "look.", "custom", &mut w) {
                c.look.custom = read_look(custom, d.look.custom, &mut w);
            }
        }
        if let Some(t) = section(&root, "editor", &mut w) {
            c.editor.line_assist = get(t, "editor.", "line_assist", d.editor.line_assist, &mut w);
        }
        if root.get("ai").is_some_and(|v| !v.is_table()) {
            w.push(format!("ai: expected a table; using defaults{AI_OFF}"));
            c.ai.provider = Provider::Unreadable;
        } else if let Some(t) = section(&root, "ai", &mut w) {
            c.ai.provider = Provider::read(t, &mut w);
            c.ai.base_url =
                get_checked(t, "ai.", "base_url", d.ai.base_url, &mut w, |s: &String| {
                    check_base_url(s)
                });
            c.ai.model = get_checked(t, "ai.", "model", d.ai.model, &mut w, |s: &String| {
                check_model(s)
            });
        }
        if let Some(t) = section(&root, "translate", &mut w) {
            c.translate.source = get_checked(
                t,
                "translate.",
                "source",
                d.translate.source,
                &mut w,
                |s: &String| match s.as_str() {
                    crate::translate::lang::AUTO => Ok(()),
                    s => check_lang(s).map_err(|e| format!("{e}, or \"auto\"")),
                },
            );
            c.translate.target = get_checked(
                t,
                "translate.",
                "target",
                d.translate.target,
                &mut w,
                |s: &String| check_lang(s),
            );
            c.translate.engine = get(t, "translate.", "engine", d.translate.engine, &mut w);
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
            c.translate.live_note_shown = get(t, "translate.", "live_note_shown", false, &mut w);
            c.translate.whole_screen = get(t, "translate.", "whole_screen", false, &mut w);
        }
        if let Some(t) = section(&root, "song", &mut w) {
            c.song.source = get(t, "song.", "source", d.song.source, &mut w);
            c.song.history = get(t, "song.", "history", d.song.history, &mut w);
        }
        if let Some(t) = section(&root, "update", &mut w) {
            c.update.auto = get(t, "update.", "auto", None, &mut w);
        }
        if let Some(t) = section(&root, "snip", &mut w) {
            let p = "snip.";
            c.snip.after = get(t, p, "after", d.snip.after, &mut w);
            c.snip.max_edge = get_checked(t, p, "max_edge", d.snip.max_edge, &mut w, |n: &u32| {
                (*n == 0 || MAX_EDGE.contains(n))
                    .then_some(())
                    .ok_or_else(|| {
                        format!(
                            "expected 0 (original) or {} to {} px",
                            MAX_EDGE.start(),
                            MAX_EDGE.end()
                        )
                    })
            });
            c.snip.format = get(t, p, "format", d.snip.format, &mut w);
            c.snip.save_original = get(t, p, "save_original", false, &mut w);
            c.snip.cursor = get(t, p, "cursor", false, &mut w);
            c.snip.history = get(t, p, "history", false, &mut w);
            c.snip.delay = get_checked(t, p, "delay", 0, &mut w, |n: &u8| {
                [0, 3, 5, 10]
                    .contains(n)
                    .then_some(())
                    .ok_or_else(|| "expected 0 (off), 3, 5 or 10 seconds".to_string())
            });
        }
        if let Some(t) = section(&root, "labs", &mut w) {
            let (p, d) = ("labs.", d.labs);
            c.labs.snip = get(t, p, "snip", d.snip, &mut w);
            c.labs.editor = get(t, p, "editor", d.editor, &mut w);
            c.labs.song = get(t, p, "song", d.song, &mut w);
            c.labs.history = get(t, p, "history", d.history, &mut w);
            c.labs.pins = get(t, p, "pins", d.pins, &mut w);
            c.labs.live = get(t, p, "live", d.live, &mut w);
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

// ---- schema migration ----

/// The schema version a file says it has (missing = 1, the first release), or
/// `None` when it is not a table or says something else than a number.
fn file_version(text: &str) -> Option<i64> {
    let root: Table = text.parse().ok()?;
    match root.get("version") {
        None => Some(1),
        Some(v) => v.as_integer(),
    }
}

/// A file written by an older schema is copied to `config.toml.bak-v<old>` and then
/// rewritten as `config` (read from that file without warnings, new keys at their
/// defaults). A failed copy leaves the file as it is: no backup, no rewrite. Every
/// later schema step adds its defaults in `parse`; this only has to carry them to disk.
fn migrate(path: &Path, text: &str, config: &Config) {
    let Some(old) = file_version(text).filter(|v| (1..VERSION as i64).contains(v)) else {
        return;
    };
    let Some(backup) = backup_old(path, text, old) else {
        crate::log!("[opencircle] config: could not back up the v{old} file; leaving it as it is");
        return;
    };
    match config.save_to(path) {
        Ok(()) => crate::log!(
            "[opencircle] config: updated from schema {old} to {VERSION} (old file: {})",
            backup.display()
        ),
        Err(e) => crate::log!("[opencircle] config: could not write the updated file ({e})"),
    }
}

/// Writes `text` next to `path` as `<name>.bak-v<old>` (`-2`, `-3` ... if taken: an
/// existing backup is never overwritten). `None` if it cannot be written.
fn backup_old(path: &Path, text: &str, old: i64) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    for n in 1..100 {
        let tag = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        let backup = path.with_file_name(format!("{name}.bak-v{old}{tag}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(mut f) => {
                let written = f.write_all(text.as_bytes()).and_then(|()| f.sync_all());
                return written.is_ok().then_some(backup);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
    }
    None
}

// ---- the running app's config: one source of truth ----

static CURRENT: RwLock<Option<Config>> = RwLock::new(None);

/// The active look, without cloning the whole config (the screen edge reads it
/// every frame, the widgets on every style).
pub fn look() -> Look {
    if let Some(c) = CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return c.look.active();
    }
    current().look.active()
}

/// The floating widgets' theme and whether they blur the screen behind them.
pub fn widgets() -> (WidgetTheme, bool) {
    let c = CURRENT.read().unwrap_or_else(|e| e.into_inner());
    c.as_ref().map_or((WidgetTheme::Default, true), |c| {
        (c.widget_theme, c.glass_effect)
    })
}

/// The config the running app uses (any thread). Read from disk on first use;
/// after that only `set_current` changes it (Settings saved, onboarding finished).
pub fn current() -> Config {
    if let Some(c) = CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return c.clone();
    }
    let (config, warnings) = Config::load();
    for warning in warnings {
        crate::log!("[opencircle] config: {warning}");
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
    sub_section(root, "", name, w)
}

fn sub_section<'a>(
    t: &'a Table,
    prefix: &str,
    name: &str,
    w: &mut Vec<String>,
) -> Option<&'a Table> {
    match t.get(name)? {
        Value::Table(t) => Some(t),
        _ => {
            w.push(format!("{prefix}{name}: expected a table; using defaults"));
            None
        }
    }
}

/// `n` is in `range` (`unit` names it in the warning).
fn within(
    range: std::ops::RangeInclusive<u8>,
    unit: &'static str,
) -> impl Fn(&u8) -> Result<(), String> {
    move |n| {
        (range.contains(n))
            .then_some(())
            .ok_or_else(|| format!("expected {} to {}{unit}", range.start(), range.end()))
    }
}

/// `[look.custom]`, field by field over `d`.
fn read_look(t: &Table, d: Look, w: &mut Vec<String>) -> Look {
    let p = "look.custom.";
    let mut look = Look {
        palette: get(t, p, "palette", d.palette, w),
        colours: get(t, p, "colours", d.colours, w),
        glow: get_checked(t, p, "glow", d.glow, w, within(GLOW, " %")),
        dim: get_checked(t, p, "dim", d.dim, w, within(DIM, " %")),
        activation_glow: get(t, p, "activation_glow", d.activation_glow, w),
        brackets: get(t, p, "brackets", d.brackets, w),
        bracket_style: get(t, p, "bracket_style", d.bracket_style, w),
        bar: get(t, p, "bar", d.bar, w),
        ..d
    };
    for mode in EdgeMode::ALL {
        let Some(e) = sub_section(t, p, mode.key(), w) else {
            continue;
        };
        let p = format!("{p}{}.", mode.key());
        let old = d.edge(mode);
        let thickness = within(EDGE_THICKNESS, " px");
        *look.edge_mut(mode) = Edge {
            style: get(e, &p, "style", old.style, w),
            thickness: get_checked(e, &p, "thickness", old.thickness, w, thickness),
            animate: get(e, &p, "animate", old.animate, w),
        };
    }
    look
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

    const V010_DEFAULT: &str = include_str!("../tests/fixtures/config/v0.1.0-default.toml");
    const V010_USED: &str = include_str!("../tests/fixtures/config/v0.1.0-used.toml");

    /// A config folder holding `text` as config.toml.
    fn config_with(tag: &str, text: &str) -> (PathBuf, PathBuf) {
        let dir = tmp_dir(tag);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn a_v0_1_0_file_loads_without_warnings_and_gets_labs_defaults() {
        for (name, text) in [("default", V010_DEFAULT), ("used", V010_USED)] {
            let (c, w) = Config::parse(text);
            assert!(w.is_empty(), "{name}: {w:?}");
            assert_eq!(c.labs, Labs::default(), "{name}: Labs off");
            assert_eq!(c.version, VERSION);
        }
        // What the user had set is read as before, snip / song / history included.
        let (c, _) = Config::parse(V010_USED);
        assert_eq!(c.theme, Theme::Dark);
        assert!(c.general.search_history && c.snip.history && c.song.history);
        assert_eq!((c.snip.after, c.snip.max_edge), (SnipAfter::Both, 2048));
        assert_eq!(c.song.source, SongSource::Microphone);
        let quick: Vec<_> = c
            .quick_actions
            .iter()
            .map(|q| (q.action.as_str(), q.shortcut.as_str()))
            .collect();
        assert_eq!(
            quick,
            [
                ("translate", "Ctrl+Shift+T"),
                ("snip", "PrintScreen"),
                ("snip", "Ctrl+Shift+S"),
                ("song_search", "Ctrl+Shift+M"),
            ]
        );
    }

    #[test]
    fn migrating_backs_the_old_file_up_once_then_rewrites_it_as_the_new_schema() {
        let (dir, path) = config_with("migrate", V010_USED);
        let (c, w) = Config::load_from(&path);
        assert!(w.is_empty(), "{w:?}");
        // The old file, byte for byte, next to the new one.
        let backup = dir.join("config.toml.bak-v1");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), V010_USED);
        // The file is the new schema now, with every old value and the Labs switches.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("version = 3") && text.contains("[labs]"),
            "{text}"
        );
        let (again, w) = Config::load_from(&path);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(again, c, "nothing lost in the rewrite");
        assert_eq!(
            again.quick_actions,
            Config::parse(V010_USED).0.quick_actions
        );
        // Loading again changes nothing and writes no second backup.
        let after = std::fs::read_to_string(&path).unwrap();
        Config::load_from(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
        let backups = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(backups, 2, "config.toml and one backup");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_backup_is_never_overwritten() {
        let (dir, path) = config_with("keep-backup", V010_DEFAULT);
        std::fs::write(dir.join("config.toml.bak-v1"), "an older copy").unwrap();
        Config::load_from(&path);
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml.bak-v1")).unwrap(),
            "an older copy"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml.bak-v1-2")).unwrap(),
            V010_DEFAULT
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_current_file_is_left_alone_and_a_missing_one_is_not_created() {
        let (dir, path) = config_with("current", "version = 3\n[labs]\nsong = true\n");
        let (c, w) = Config::load_from(&path);
        assert!(w.is_empty() && c.labs.song && !c.labs.snip, "{w:?}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no backup");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "version = 3\n[labs]\nsong = true\n"
        );
        let gone = dir.join("none.toml");
        Config::load_from(&gone);
        assert!(!gone.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_with_a_problem_is_not_rewritten() {
        let bad = "version = 1\nshortcut = \"nonsense\"\n";
        let (dir, path) = config_with("bad", bad);
        let (_, w) = Config::load_from(&path);
        assert!(!w.is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            bad,
            "kept as the user wrote it"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn labs_switches_round_trip_and_keep_the_hidden_features_settings() {
        let (mut c, _) = Config::parse(V010_USED);
        c.labs.snip = true;
        c.labs.pins = true;
        let dir = tmp_dir("labs-roundtrip");
        let path = dir.join("config.toml");
        c.save_to(&path).unwrap();
        let (back, w) = Config::load_from(&path);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back, c);
        assert!(back.labs.snip && back.labs.pins && !back.labs.song);
        let _ = std::fs::remove_dir_all(dir);
        let (c, w) = Config::parse("version = 2\n[labs]\nsnip = \"yes\"\n");
        assert!(
            !c.labs.snip && w.iter().any(|w| w.starts_with("labs.snip")),
            "{w:?}"
        );
    }

    #[test]
    fn tests_never_read_or_write_the_machines_config() {
        assert_eq!(Config::path(), None);
        assert!(Config::default().save().is_err());
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
        assert_eq!(c.version, 3);
        assert_eq!(c.labs, Labs::default(), "Labs: everything off");
        assert_eq!(c.theme, Theme::System);
        assert_eq!(c.shortcut, "Ctrl+Shift+Space");
        assert_eq!(c.translate.target, "ar");
        assert_eq!(c.ai.provider, Provider::Browser);
        assert_eq!(c.quick_actions.len(), 9);
        // Song search: unbound, history off.
        let song = c.quick_actions.last().unwrap();
        assert_eq!(
            (song.action.as_str(), song.shortcut.as_str()),
            ("song_search", "")
        );
        assert_eq!(c.song.source, SongSource::System);
        assert!(!c.song.history);
        assert!(c.quick_actions[1].live);
        assert_eq!(c.general.default_action, DefaultAction::Lens);
        assert_eq!(c.general.save_folder, "");
        assert_eq!(c.translate.live_interval_ms, 1500);
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
    fn look_and_selection_defaults_round_trip_and_old_files_keep_working() {
        let d = Config::default();
        assert_eq!(d.selection.style, SelectStyle::Snap);
        assert_eq!(
            d.selection.after,
            After::Wait,
            "nothing is sent before a choice"
        );
        assert_eq!(d.selection.search_with, SearchWith::Lens);
        assert_eq!(d.look.theme, LookTheme::Spectrum);
        let look = d.look.active();
        assert_eq!(look.overlay, edge(EdgeStyle::Glow, 6, true));
        assert_eq!(look.live, edge(EdgeStyle::Glow, 6, true));
        assert_eq!(look.snip, edge(EdgeStyle::Thin, 3, false));
        assert!(look.activation_glow && look.dim == 12);
        // a file from before these sections: the defaults, no warning
        let (old, w) = Config::parse("theme = \"dark\"\n[general]\ndefault_action = \"ai\"\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((old.selection, old.look), (d.selection, d.look));
        // every preset is a theme, and every theme but Custom has one
        for (theme, _) in PRESETS {
            assert_ne!(theme, LookTheme::Custom);
        }
        assert_eq!(PRESETS.len(), 7);
        // a custom look and a selection setting survive a save
        let mut c = Config {
            selection: SelectionConfig {
                style: SelectStyle::Shape,
                after: After::Wait,
                search_with: SearchWith::Both,
                text_reader: TextReader::PpOcr,
            },
            ..Config::default()
        };
        c.look.theme = LookTheme::Custom;
        c.look.custom.palette = ColourSet::TwoColour;
        c.look.custom.colours = [Rgb([1, 2, 3]), Rgb([0xfe, 0xdc, 0xba])];
        c.look.custom.bar = BarStyle::Outline;
        *c.look.edge_mut(EdgeMode::Live) = edge(EdgeStyle::None, 12, false);
        let text = toml::to_string_pretty(&c).unwrap();
        assert!(
            text.contains("\"#fedcba\"") && text.contains("[look.custom.live]"),
            "{text}"
        );
        let (back, w) = Config::parse(&text);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back, c);
        assert_eq!(back.look.active().live.style, EdgeStyle::None);
        // a preset ignores the custom look but keeps it
        c.look.theme = LookTheme::Calm;
        assert_eq!(c.look.active().overlay.style, EdgeStyle::Thin);
        assert_eq!(c.look.custom.bar, BarStyle::Outline);
    }

    #[test]
    fn look_values_are_checked_one_by_one() {
        let (c, w) = Config::parse(
            "[selection]\nstyle = \"blob\"\nafter = \"wait\"\n\
             [look]\ntheme = \"custom\"\n\
             [look.custom]\nglow = 101\ndim = 61\ncolours = [\"#12345\", \"#000000\"]\n\
             bar = \"solid\"\n\
             [look.custom.overlay]\nthickness = 13\nstyle = \"thin\"\n\
             [look.custom.snip]\nthickness = 0\n",
        );
        let d = Config::default();
        assert_eq!(c.selection.style, SelectStyle::Snap);
        assert_eq!(c.selection.after, After::Wait, "good survives");
        let look = c.look.active();
        assert_eq!((look.glow, look.dim), (100, 12));
        assert_eq!(look.colours, d.look.custom.colours);
        assert_eq!(look.bar, BarStyle::Solid);
        assert_eq!(look.overlay.style, EdgeStyle::Thin);
        assert_eq!(look.overlay.thickness, 6);
        assert_eq!(look.snip.thickness, 3);
        for key in [
            "selection.style",
            "look.custom.glow",
            "look.custom.dim",
            "look.custom.colours",
            "look.custom.overlay.thickness",
            "look.custom.snip.thickness",
        ] {
            assert!(w.iter().any(|m| m.starts_with(key)), "{key}: {w:?}");
        }
        assert_eq!(w.len(), 6, "{w:?}");
        // range ends are fine
        let (c, w) = Config::parse(
            "[look.custom]\nglow = 0\ndim = 60\n[look.custom.live]\nthickness = 12\n",
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((c.look.custom.glow, c.look.custom.dim), (0, 60));
        assert_eq!(Rgb::parse("#8F80ff"), Some(Rgb([0x8f, 0x80, 0xff])));
        assert_eq!(Rgb::parse("8f80ff"), None);
    }

    #[test]
    fn missing_file_gives_defaults_without_warnings() {
        let (c, w) = Config::load_from(&tmp_dir("missing").join("nope.toml"));
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
    }

    #[test]
    fn the_bar_anchor_reads_back_and_bad_values_fall_back() {
        let (c, w) = Config::parse("[bar]\nanchor = \"bottom_left\"\n");
        assert_eq!((c.bar.anchor, w.len()), (BarAnchor::BottomLeft, 0));
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(Config::parse(&text).0.bar.anchor, BarAnchor::BottomLeft);
        let (bad, w) = Config::parse("[bar]\nanchor = \"middle\"\n");
        assert_eq!((bad.bar.anchor, w.len()), (BarAnchor::Bottom, 1));
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
        c.translate.live_note_shown = true;
        c.translate.whole_screen = true;
        c.translate.source = "ja".into();
        c.song.source = SongSource::Microphone;
        c.song.history = true;
        c.update.auto = Some(false);
        c.snip.after = SnipAfter::Editor;
        c.snip.history = true;
        c.snip.max_edge = 0;
        c.snip.delay = 3;
        c.editor.line_assist = false;
        c.widget_theme = WidgetTheme::Frost;
        c.glass_effect = false;
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
        assert!(text.contains("[editor]\nline_assist = false"), "{text}");
        assert!(text.contains("widget_theme = \"frost\"") && text.contains("glass_effect = false"));
        // A typo falls back to the default with a warning.
        let (bad, w) = Config::parse("widget_theme = \"frosty\"\nglass_effect = 3\n");
        assert_eq!(
            (bad.widget_theme, bad.glass_effect),
            (WidgetTheme::Default, true)
        );
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(text.contains("[update]\nauto = false"), "{text}");
        // Not asked yet: no [update] key written, none read.
        let unasked = Config::default();
        assert!(
            !toml::to_string(&unasked)
                .unwrap()
                .contains("[update]\nauto")
        );
        assert_eq!(
            Config::parse("[update]\nauto = true\n").0.update.auto,
            Some(true)
        );
        let (c, w) = Config::parse("[update]\nauto = \"yes\"\n");
        assert!(
            c.update.auto.is_none() && w[0].starts_with("update.auto"),
            "{w:?}"
        );
        // Older files have no [editor]: line assist on, no warning.
        let (c, w) = Config::parse("version = 1\n");
        assert!(c.editor.line_assist && w.is_empty(), "{w:?}");
        let (c, w) = Config::parse("[editor]\nline_assist = \"yes\"\n");
        assert!(
            c.editor.line_assist && w[0].starts_with("editor.line_assist"),
            "{w:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn translate_source_defaults_to_auto_and_is_checked() {
        // a config from before `source` existed: auto, no warning
        let (c, w) = Config::parse("[translate]\ntarget = \"fr\"\n");
        assert_eq!(
            (c.translate.source.as_str(), c.translate.target.as_str()),
            ("auto", "fr")
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(Config::default().translate.source, "auto");
        let (c, _) = Config::parse("[translate]\nsource = \"zh-Hant\"\n");
        assert_eq!(c.translate.source, "zh-Hant");
        let (c, w) = Config::parse("[translate]\nsource = \"automatic\"\n");
        assert_eq!(c.translate.source, "auto");
        assert!(
            w[0].starts_with("translate.source") && w[0].contains("\"auto\""),
            "{w:?}"
        );
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
        assert_eq!(c.translate.live_interval_ms, 1500);
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
            assert_eq!(
                c.translate.live_interval_ms, DEFAULT_LIVE_INTERVAL_MS,
                "{bad}"
            );
            assert_eq!(w.len(), 1, "{bad}: {w:?}");
        }
        let abs = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        let (c, w) = Config::parse(&format!("[general]\nsave_folder = \"{abs}\"\n"));
        assert_eq!((c.general.save_folder.as_str(), w.len()), (abs.as_str(), 0));
        // Search history: opt-in.
        assert!(!Config::default().general.search_history);
        let (c, w) = Config::parse("[general]\nsearch_history = true\n");
        assert!(c.general.search_history && w.is_empty());
        // Start with the OS: on unless turned off; a non-bool is reported.
        assert!(Config::default().general.autostart);
        let (c, w) = Config::parse("[general]\nautostart = false\n");
        assert!(!c.general.autostart && w.is_empty());
        let (c, w) = Config::parse("[general]\nautostart = \"yes\"\n");
        assert!(c.general.autostart && w.len() == 1, "{w:?}");
    }

    /// The web view card is gone (N-02): its `[results]` keys, good or bad, load
    /// silently and are not written back.
    #[test]
    fn old_results_keys_load_with_no_warning_and_are_dropped() {
        for text in [
            "[results]\nside = \"left\"\nwidth = 640\nheight = 300\n",
            "version = 2\n[results]\nside = \"top\"\nwidth = 5\nheight = -1\n",
            "version = 3\nresults = 7\n",
        ] {
            let (c, w) = Config::parse(text);
            assert!(w.is_empty(), "{text}: {w:?}");
            let saved = toml::to_string_pretty(&c).unwrap();
            assert!(!saved.contains("results"), "{saved}");
        }
    }

    /// Schema 3 (N-03): a file from before it that searched by itself waits for the
    /// user's choice now, with the old file backed up; a schema-3 file that opted
    /// back in ("Auto action") keeps "search".
    #[test]
    fn an_old_auto_search_migrates_to_wait_with_a_backup() {
        for (tag, old) in [
            ("v1", V010_DEFAULT.to_string()),
            ("v2", V010_DEFAULT.replace("version = 1", "version = 2")),
        ] {
            assert!(old.contains("after = \"search\""), "the fixture searched");
            let (dir, path) = config_with(&format!("wait-{tag}"), &old);
            let (c, w) = Config::load_from(&path);
            assert!(w.is_empty(), "{w:?}");
            assert_eq!(c.selection.after, After::Wait, "{tag}");
            let v = if tag == "v1" { 1 } else { 2 };
            let backup = dir.join(format!("config.toml.bak-v{v}"));
            assert_eq!(std::fs::read_to_string(&backup).unwrap(), old, "{tag}");
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                text.contains("version = 3") && text.contains("after = \"wait\""),
                "{text}"
            );
            assert!(!text.contains("[results]"), "{text}");
            let _ = std::fs::remove_dir_all(dir);
        }
        let (c, w) = Config::parse("version = 3\n[selection]\nafter = \"search\"\n");
        assert!(w.is_empty() && c.selection.after == After::Search, "{w:?}");
    }

    #[test]
    fn a_config_from_before_the_timer_setting_still_loads() {
        // v0.1.0 wrote no `delay`: it loads with the default, nothing else changes.
        let (c, w) = Config::parse("[snip]\nafter = \"both\"\nmax_edge = 1024\nhistory = true\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            (c.snip.delay, c.snip.after, c.snip.max_edge),
            (0, SnipAfter::Both, 1024)
        );
        let (c, w) = Config::parse("[snip]\ndelay = 5\n");
        assert!(w.is_empty() && c.snip.delay == 5, "{w:?}");
        let (c, w) = Config::parse("[snip]\ndelay = 7\n");
        assert!(c.snip.delay == 0 && w.len() == 1, "{w:?}");
    }

    #[test]
    fn snip_section_defaults_and_checks() {
        let d = Config::default().snip;
        assert_eq!(d.after, SnipAfter::Copy, "copy for the AI chat by default");
        assert_eq!((d.max_edge, d.format), (1568, Format::Png));
        assert!(!d.history, "nothing kept on disk unless turned on");
        assert!(!d.cursor && !d.save_original);
        assert_eq!(d.delay, 0, "no timer unless set");
        let (c, w) = Config::parse(
            "[snip]\nafter = \"both\"\nmax_edge = 0\nformat = \"jpeg\"\n\
             save_original = true\ncursor = true\nhistory = true\n",
        );
        assert!(w.is_empty(), "{w:?}");
        let s = &c.snip;
        assert_eq!(
            (s.after, s.max_edge, s.format),
            (SnipAfter::Both, 0, Format::Jpeg)
        );
        assert!(s.save_original && s.cursor && s.history);
        // and back through the writer
        let text = toml::to_string_pretty(&c).unwrap();
        assert!(
            text.contains("[snip]") && text.contains("after = \"both\""),
            "{text}"
        );
        assert_eq!(Config::parse(&text).0, c);
        for bad in [
            "max_edge = 100",
            "max_edge = 9000",
            "after = \"paste\"",
            "format = \"gif\"",
            "history = \"yes\"",
        ] {
            let (c, w) = Config::parse(&format!("[snip]\n{bad}\n"));
            assert_eq!(c.snip, d, "{bad}");
            assert_eq!(w.len(), 1, "{bad}: {w:?}");
            assert!(w[0].starts_with("snip."), "{w:?}");
        }
    }

    #[test]
    fn engine_is_ai_or_offline() {
        for value in ["\"gemini\"", "\"Offline\"", "3"] {
            let (c, w) = Config::parse(&format!("[translate]\nengine = {value}\n"));
            assert_eq!(c.translate.engine, Engine::Ai);
            assert_eq!(w.len(), 1, "{value}: {w:?}");
            assert!(w[0].starts_with("translate.engine"), "{w:?}");
        }
        for (value, engine) in [("ai", Engine::Ai), ("offline", Engine::Offline)] {
            let (c, w) = Config::parse(&format!("[translate]\nengine = \"{value}\"\n"));
            assert!(
                w.is_empty() && c.translate.engine == engine,
                "{value}: {w:?}"
            );
            // and saved the same way
            let text = toml::to_string_pretty(&c).unwrap();
            assert!(text.contains(&format!("engine = \"{value}\"")), "{text}");
        }
        assert_eq!(Config::default().translate.engine, Engine::Ai);
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
    fn browser_is_the_default_provider_and_old_configs_keep_theirs() {
        assert_eq!(Config::parse("").0.ai.provider, Provider::Browser);
        // A file written before the browser provider existed: no provider line, or an API one.
        let (c, w) = Config::parse("[ai]\nmodel = \"claude-sonnet-5-5\"\n");
        assert_eq!((c.ai.provider, w.len()), (Provider::Browser, 0));
        for (id, provider) in [
            ("browser", Provider::Browser),
            ("anthropic", Provider::Anthropic),
            ("openai_compatible", Provider::OpenaiCompatible),
        ] {
            assert_eq!(provider.id(), id);
            let url = if provider == Provider::OpenaiCompatible {
                "base_url = \"http://localhost:11434/v1\"\nmodel = \"llama3\"\n"
            } else {
                ""
            };
            let (c, w) = Config::parse(&format!("[ai]\nprovider = \"{id}\"\n{url}"));
            assert_eq!((c.ai.provider.clone(), w.len()), (provider, 0), "{w:?}");
            // and back through the serializer
            let text = toml::to_string_pretty(&c).unwrap();
            assert!(text.contains(&format!("provider = \"{id}\"")), "{text}");
            assert_eq!(Config::parse(&text).0, c);
        }
    }

    #[test]
    fn unknown_provider_fails_closed_and_keeps_the_users_value() {
        for (value, raw) in [
            ("\"antropic\"", "antropic"),
            ("\"Anthropic\"", "Anthropic"),
            ("\"\"", ""),
            ("3", "3"),
        ] {
            let text = format!("[ai]\nprovider = {value}\nmodel = \"m\"\n");
            let (c, w) = Config::parse(&text);
            assert_eq!(c.ai.provider, Provider::Invalid(raw.into()), "{value}");
            assert_ne!(c.ai.provider, Provider::Browser);
            assert_eq!(w.len(), 1, "{value}: {w:?}");
            assert!(
                w[0].starts_with("ai.provider:") && w[0].contains("off"),
                "{w:?}"
            );
            assert_eq!(c.ai.model, "m", "the rest of [ai] is still read");
            // Written back as the user wrote it (never "browser"), and still invalid.
            let out = toml::to_string_pretty(&c).unwrap();
            assert!(out.contains(&format!("provider = \"{raw}\"")), "{out}");
            assert!(!out.contains("provider = \"browser\""), "{out}");
            assert_eq!(Config::parse(&out).0.ai.provider, c.ai.provider);
        }
        // Loading never rewrites the file.
        let dir = tmp_dir("bad-provider");
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "[ai]\nprovider = \"antropic\"\n").unwrap();
        let (c, _) = Config::load_from(&path);
        assert_eq!(c.ai.provider, Provider::Invalid("antropic".into()));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[ai]\nprovider = \"antropic\"\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_fields_warn_and_default() {
        let (c, w) = Config::parse(
            r#"
            theme = "neon"
            shortcut = "Ctrl+"
            [ai]
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
        // Defaults everywhere, except AI: off (fail closed), never Browser.
        let (c, w) = Config::parse("theme = = =");
        assert_eq!(c, Config::unreadable());
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("AI is off"), "{w:?}");
        let (c, w) = Config::parse("ai = 4\nquick_actions = \"x\"\nversion = 9\n");
        assert_eq!(c, Config::unreadable());
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(
            w.iter()
                .any(|m| m.starts_with("ai:") && m.contains("AI is off"))
        );
        // Nothing can save it: an unread choice is never replaced by a guess.
        assert!(toml::to_string_pretty(&c).is_err());
        let dir = tmp_dir("unreadable");
        assert!(c.save_to(&dir.join("config.toml")).is_err());
        // A file that exists but can't be read (here: a folder) is the same.
        std::fs::create_dir_all(&dir).unwrap();
        let (c, w) = Config::load_from(&dir);
        assert_eq!((c.ai.provider, w.len()), (Provider::Unreadable, 1));
        let _ = std::fs::remove_dir_all(&dir);
        // Only a missing file is a first run: Browser.
        let (c, _) = Config::load_from(&dir.join("nope.toml"));
        assert_eq!(c.ai.provider, Provider::Browser);
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
