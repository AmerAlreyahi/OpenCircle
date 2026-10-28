//! The glue between the config and the kit: the Light / Dark / System choice
//! and the `[appearance]` picks set the kit's theme (`kit::themes`); a stored
//! `[look]` gives its edge glow, accent pair, dims and bracket colour. Every
//! UI colour itself lives in the kit (`kit::themes`, `kit::looks`, `kit::tokens`).
use std::cell::Cell;

use iced::Color;

use crate::config::{self, BracketColour, ColourSet, Look, Rgb, Theme};

/// The accent gradient of the Aurora / Spectrum looks (the selection stroke).
const ACCENT_START: Color = Color::from_rgb8(0x8f, 0x80, 0xff);
const ACCENT_END: Color = Color::from_rgb8(0x4f, 0xc9, 0xe0);

// ---- Active looks (config `[look]`): the presets' colours, one table ----

/// A palette's screen-edge glow (the two colours of the design's look of the
/// same name, alpha included: the inner and the outer layer) and its accent
/// gradient.
pub struct Colours {
    pub glow: [Color; 2],
    pub accent: (Color, Color),
}

const AURORA: Colours = Colours {
    glow: [
        Color::from_rgba8(125, 211, 252, 0.42),
        Color::from_rgba8(167, 139, 250, 0.25),
    ],
    accent: (ACCENT_START, ACCENT_END),
};

/// Our own spectrum: violet, coral, amber, lemon, mint, sky.
const SPECTRUM: Colours = Colours {
    glow: [
        Color::from_rgba8(155, 123, 255, 0.42),
        Color::from_rgba8(255, 107, 94, 0.26),
    ],
    accent: (ACCENT_START, ACCENT_END),
};

const SUNSET: Colours = Colours {
    glow: [
        Color::from_rgba8(255, 143, 63, 0.44),
        Color::from_rgba8(255, 85, 154, 0.28),
    ],
    accent: (
        Color::from_rgb8(0xff, 0xa0, 0x58),
        Color::from_rgb8(0xff, 0x6f, 0xa8),
    ),
};

const OCEAN: Colours = Colours {
    glow: [
        Color::from_rgba8(42, 212, 199, 0.45),
        Color::from_rgba8(61, 123, 255, 0.3),
    ],
    accent: (
        Color::from_rgb8(0x3f, 0xd8, 0xcc),
        Color::from_rgb8(0x5a, 0x92, 0xff),
    ),
};

const NEON: Colours = Colours {
    glow: [
        Color::from_rgba8(0, 240, 255, 0.5),
        Color::from_rgba8(255, 63, 208, 0.34),
    ],
    accent: (
        Color::from_rgb8(0x2a, 0xf2, 0xff),
        Color::from_rgb8(0xff, 0x5c, 0xd8),
    ),
};

const MONO: Colours = Colours {
    glow: [
        Color::from_rgba8(255, 255, 255, 0.42),
        Color::from_rgba8(255, 255, 255, 0.07),
    ],
    accent: (
        Color::from_rgb8(0xe8, 0xe8, 0xee),
        Color::from_rgb8(0xb4, 0xb8, 0xc2),
    ),
};

/// `palette`'s colours (`TwoColour`: Aurora's; its own two are in the look).
pub fn colours(palette: ColourSet) -> &'static Colours {
    match palette {
        ColourSet::Spectrum => &SPECTRUM,
        ColourSet::Aurora | ColourSet::TwoColour => &AURORA,
        ColourSet::Sunset => &SUNSET,
        ColourSet::Ocean => &OCEAN,
        ColourSet::Neon => &NEON,
        ColourSet::Mono => &MONO,
    }
}

pub fn rgb(c: Rgb) -> Color {
    let [r, g, b] = c.0;
    Color::from_rgb8(r, g, b)
}

/// The look's edge glow colours c1 (near the edge), c2 (further in), with
/// alpha (a `TwoColour` look: its own two, at the Spectrum's alphas).
pub fn edge_glow(look: &Look) -> [Color; 2] {
    let [a, b] = colours(ColourSet::Spectrum).glow.map(|c| c.a);
    match look.palette {
        ColourSet::TwoColour => [
            Color {
                a,
                ..rgb(look.colours[0])
            },
            Color {
                a: b,
                ..rgb(look.colours[1])
            },
        ],
        p => colours(p).glow,
    }
}

/// The accent gradient of `look` (handles, the stroke).
pub fn accent_of(look: &Look) -> (Color, Color) {
    match look.palette {
        ColourSet::TwoColour => (rgb(look.colours[0]), rgb(look.colours[1])),
        p => colours(p).accent,
    }
}

/// The dim over the whole frozen frame (before a selection) and outside a
/// selection, for the look's `dim` %.
pub fn dims(look: &Look) -> (Color, Color) {
    let idle = f32::from(look.dim) / 100.0;
    (
        Color::from_rgba(0.0, 0.0, 0.0, idle),
        Color::from_rgba(0.0, 0.0, 0.0, (0.33 + idle).min(0.85)),
    )
}

/// The selection's corner brackets.
pub fn bracket(look: &Look) -> Color {
    match look.brackets {
        BracketColour::White => Color::WHITE,
        BracketColour::Accent => accent_of(look).0,
    }
}

/// The live-translate layer's background: see-through where no patch is.
/// Windows keys out `live::COLOUR_KEY`; elsewhere the window is transparent.
#[cfg(windows)]
pub const LAYER_CLEAR: Color = {
    let [r, g, b] = crate::live::COLOUR_KEY;
    Color::from_rgb8(r, g, b)
};
#[cfg(not(windows))]
pub const LAYER_CLEAR: Color = Color::TRANSPARENT;

/// The editor's colour picker lives in `editor::style` (annotations belong to the image,
/// not the UI); re-exported for the Settings colour field.
pub use crate::editor::style::ANNOTATION;

/// Light instead of dark on this thread (snapshot tests).
#[cfg(test)]
pub fn set_light(light: bool) {
    apply(if light { Theme::Light } else { Theme::Dark });
}

thread_local! {
    /// The UI thread's theme choice and the OS's mode (tests render Light and Dark
    /// side by side).
    static SETTING: Cell<Theme> = const { Cell::new(Theme::System) };
    static OS_DARK: Cell<bool> = const { Cell::new(true) };
}

/// The Light / Dark / System choice from Settings; System follows `set_os`.
pub fn apply(theme: Theme) {
    preview(theme, &config::appearance());
}

/// `apply` with `[appearance]` picks not saved yet (Settings shows a pick at once).
pub fn preview(theme: Theme, picks: &config::AppearanceConfig) {
    SETTING.with(|s| s.set(theme));
    crate::ui::kit::themes::set(kit_theme(theme, os_dark(), picks));
}

/// The OS is in dark mode (or says nothing).
pub fn os_dark() -> bool {
    OS_DARK.with(Cell::get)
}

/// What the OS reports (at start, then on every change). "No preference" counts as dark.
pub fn set_os(mode: iced::theme::Mode) {
    OS_DARK.with(|d| d.set(mode != iced::theme::Mode::Light));
    to_kit();
}

/// The kit's theme (the screens ported to it): the one place it is set from the
/// config, by the same mode choice and the `[appearance]` picks.
fn to_kit() {
    let picks = config::appearance();
    crate::ui::kit::themes::set(kit_theme(
        SETTING.with(Cell::get),
        OS_DARK.with(Cell::get),
        &picks,
    ));
}

pub fn kit_theme(
    setting: Theme,
    os_dark: bool,
    picks: &config::AppearanceConfig,
) -> &'static crate::ui::kit::themes::Theme {
    use crate::ui::kit::themes::{self, Mode};
    let mode = match setting {
        Theme::System => Mode::System,
        Theme::Light => Mode::Light,
        Theme::Dark => Mode::Dark,
    };
    themes::resolve(mode, &picks.dark, &picks.light, os_dark)
}

/// The design look a stored look shows (names and cards come from it): "aurora"
/// is the design's Prism, "mono" its Frost, "calm" Prism's colours without the
/// glow. `None` = the user's own (`custom`). Stored values never change.
pub fn kit_look(theme: config::LookTheme) -> Option<&'static crate::ui::kit::looks::Look> {
    use crate::config::LookTheme as T;
    use crate::ui::kit::looks::*;
    Some(match theme {
        T::Spectrum => &SPECTRUM,
        T::Aurora | T::Calm => &PRISM,
        T::Sunset => &SUNSET,
        T::Ocean => &OCEAN,
        T::Neon => &NEON,
        T::Mono => &FROST,
        T::Horizon => &HORIZON,
        T::Solar => &SOLAR,
        T::Hud => &HUD,
        T::Borealis => &AURORA,
        T::Custom => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::theme::Mode;

    /// Today's edge colours are the design looks' (config v4 maps `aurora`
    /// to `prism`, `mono` to `frost`).
    #[test]
    fn todays_edge_colours_are_the_design_looks() {
        use crate::ui::kit::looks;
        for (set, look) in [
            (ColourSet::Aurora, &looks::PRISM),
            (ColourSet::Spectrum, &looks::SPECTRUM),
            (ColourSet::Sunset, &looks::SUNSET),
            (ColourSet::Ocean, &looks::OCEAN),
            (ColourSet::Neon, &looks::NEON),
            (ColourSet::Mono, &looks::FROST),
        ] {
            assert_eq!(colours(set).glow, look.glow, "{}", look.id);
        }
        let custom = looks::Active::Custom([Color::WHITE, Color::BLACK]).glow();
        let edge = edge_glow(&Look {
            palette: ColourSet::TwoColour,
            colours: [Rgb([255; 3]), Rgb([0; 3])],
            ..config::LookConfig::default().custom
        });
        assert_eq!(custom, edge);
    }

    /// What the OS reports reaches the kit under System; "no preference" is dark.
    #[test]
    fn os_mode_reaches_the_kit() {
        use crate::ui::kit::themes;
        apply(Theme::System);
        set_os(Mode::Light);
        assert!(themes::current().light);
        set_os(Mode::None);
        assert!(!themes::current().light);
        apply(Theme::Light);
        assert!(themes::current().light);
    }

    /// The kit's screens (toast, update card) follow the same choice.
    #[test]
    fn the_kit_follows_light_and_dark() {
        use crate::ui::kit::themes;
        let kit = || themes::current().id;
        apply(Theme::Light);
        assert_eq!(kit(), "daylight");
        apply(Theme::System);
        set_os(Mode::Dark);
        assert_eq!(kit(), "sapphire");
        set_os(Mode::Light);
        assert_eq!(kit(), "daylight");
        apply(Theme::Dark);
        assert_eq!(kit(), "sapphire");
    }

    /// The kit shows the `[appearance]` pick of the mode in use, the OS's under
    /// System; config's id lists are the kit's.
    #[test]
    fn stored_looks_read_as_the_design_looks() {
        use config::LookTheme as T;
        let id = |t| kit_look(t).map(|l| l.id);
        assert_eq!(id(T::Aurora), Some("prism"));
        assert_eq!(id(T::Mono), Some("frost"));
        assert_eq!(id(T::Calm), Some("prism"));
        assert_eq!(id(T::Hud), Some("hud"));
        assert_eq!(id(T::Custom), None);
        // Every preset shows its design look's colours.
        for (t, look) in config::PRESETS {
            let kit = kit_look(t).unwrap();
            match look.palette {
                ColourSet::TwoColour => {
                    let rgb = |c: Color| Rgb([c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8));
                    assert_eq!(look.colours, kit.solid.map(rgb), "{}", kit.id);
                }
                p => assert_eq!(colours(p).glow, kit.glow, "{}", kit.id),
            }
        }
    }

    #[test]
    fn the_kit_shows_the_appearance_picks() {
        use crate::ui::kit::themes::THEMES;
        let picks = config::AppearanceConfig {
            dark: "emerald".into(),
            light: "sandstone".into(),
            ..Default::default()
        };
        let id = |setting, os_dark| kit_theme(setting, os_dark, &picks).id;
        for (setting, os_dark, want) in [
            (Theme::System, true, "emerald"),
            (Theme::System, false, "sandstone"),
            (Theme::Dark, false, "emerald"),
            (Theme::Light, true, "sandstone"),
        ] {
            assert_eq!(id(setting, os_dark), want, "{setting:?} os_dark={os_dark}");
        }
        let d = config::AppearanceConfig::default();
        assert_eq!(kit_theme(Theme::Dark, true, &d).id, "sapphire");
        assert_eq!(kit_theme(Theme::Light, true, &d).id, "daylight");
        let ids = |light| {
            THEMES
                .iter()
                .filter(move |t| t.light == light)
                .map(|t| t.id)
        };
        assert!(ids(false).eq(config::DARK_THEMES));
        assert!(ids(true).eq(config::LIGHT_THEMES));
    }
}
