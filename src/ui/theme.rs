//! Design tokens: the ONLY place colours live (DESIGN.md "Themes").
use std::cell::Cell;

use iced::Color;

use crate::config::{self, BarStyle, BracketColour, ColourSet, Look, Rgb, Theme, WidgetTheme};

/// One theme's surface colours.
pub struct Palette {
    pub panel: Color,
    /// `panel` laid over `window` as one solid colour: for panels over busy
    /// content (the frozen frame), where see-through glass shows text behind.
    pub panel_solid: Color,
    pub border: Color,
    pub text: Color,
    /// Secondary text: key hints, captions.
    pub muted: Color,
    pub shadow: Color,
    /// Quiet button under the cursor / pressed.
    pub hover: Color,
    pub pressed: Color,
    /// Settings / onboarding window background.
    pub window: Color,
    /// Input fields, segmented tracks (quiet fill); `field_focus` = hovered, focused or recording.
    pub field: Color,
    pub field_focus: Color,
    /// One key inside a shortcut field.
    pub keycap: Color,
    /// Error text and invalid-field outline.
    pub danger: Color,
}

/// `over` (with its alpha) laid on `under`, as one opaque colour.
const fn solid(over: Color, under: Color) -> Color {
    Color {
        r: over.r * over.a + under.r * (1.0 - over.a),
        g: over.g * over.a + under.g * (1.0 - over.a),
        b: over.b * over.a + under.b * (1.0 - over.a),
        a: 1.0,
    }
}

const DARK_PANEL: Color = Color::from_rgba8(22, 22, 34, 0.86);
const DARK_WINDOW: Color = Color::from_rgb8(0x12, 0x13, 0x1f);
const LIGHT_PANEL: Color = Color::from_rgba8(255, 255, 255, 0.90);
const LIGHT_WINDOW: Color = Color::from_rgb8(0xf6, 0xf6, 0xfa);

const DARK_SOLID: Color = solid(DARK_PANEL, DARK_WINDOW);
const LIGHT_SOLID: Color = solid(LIGHT_PANEL, LIGHT_WINDOW);

const DARK: Palette = Palette {
    panel: DARK_SOLID,
    panel_solid: DARK_SOLID,
    border: Color::from_rgba8(255, 255, 255, 0.09),
    text: Color::from_rgb8(0xec, 0xec, 0xf4),
    muted: Color::from_rgb8(0xa3, 0xa3, 0xb8),
    shadow: Color::from_rgba8(0, 0, 0, 0.35),
    hover: Color::from_rgba8(255, 255, 255, 0.08),
    pressed: Color::from_rgba8(255, 255, 255, 0.14),
    window: DARK_WINDOW,
    field: Color::from_rgba8(255, 255, 255, 0.06),
    field_focus: Color::from_rgba8(255, 255, 255, 0.12),
    keycap: Color::from_rgba8(255, 255, 255, 0.10),
    danger: Color::from_rgb8(0xff, 0x8a, 0x8a),
};

const LIGHT: Palette = Palette {
    panel: LIGHT_SOLID,
    panel_solid: LIGHT_SOLID,
    border: Color::from_rgba8(16, 16, 40, 0.10),
    text: Color::from_rgb8(0x1a, 0x1a, 0x24),
    muted: Color::from_rgb8(0x5b, 0x5b, 0x6e),
    shadow: Color::from_rgba8(16, 16, 40, 0.15),
    hover: Color::from_rgba8(16, 16, 40, 0.06),
    pressed: Color::from_rgba8(16, 16, 40, 0.11),
    window: LIGHT_WINDOW,
    field: Color::from_rgba8(16, 16, 40, 0.05),
    field_focus: Color::from_rgba8(16, 16, 40, 0.10),
    keycap: Color::from_rgba8(255, 255, 255, 0.95),
    danger: Color::from_rgb8(0xc6, 0x28, 0x28),
};

// Widget themes (config `widget_theme`): the floating widgets only. Every widget
// surface is OPAQUE for now (owner, 2026-10-27: the page behind never shows
// through); glass returns with the GPU renderer, so `panel` and `panel_solid`
// are the same solid colour until then.
const FROST_WINDOW: Color = Color::from_rgb8(0xe4, 0xea, 0xf4);
const FROST_SOLID: Color = solid(Color::from_rgba8(246, 249, 255, 0.86), FROST_WINDOW);
/// Frosted glass: a pale tint, a bright 1 px edge, dark text.
const FROST: Palette = Palette {
    panel: FROST_SOLID,
    panel_solid: FROST_SOLID,
    border: Color::from_rgba8(255, 255, 255, 0.70),
    text: Color::from_rgb8(0x14, 0x18, 0x26),
    muted: Color::from_rgb8(0x3f, 0x46, 0x5a),
    shadow: Color::from_rgba8(16, 24, 48, 0.18),
    hover: Color::from_rgba8(255, 255, 255, 0.45),
    pressed: Color::from_rgba8(255, 255, 255, 0.70),
    window: FROST_WINDOW,
    field: Color::from_rgba8(16, 24, 48, 0.06),
    field_focus: Color::from_rgba8(16, 24, 48, 0.12),
    keycap: Color::from_rgba8(255, 255, 255, 0.80),
    danger: Color::from_rgb8(0xb3, 0x26, 0x26),
};

const GRAPHITE_WINDOW: Color = Color::from_rgb8(0x1f, 0x22, 0x28);
const GRAPHITE_SOLID: Color = solid(Color::from_rgba8(38, 41, 48, 0.96), GRAPHITE_WINDOW);
/// Calm dark graphite: a warm grey tint, a hairline edge.
const GRAPHITE: Palette = Palette {
    panel: GRAPHITE_SOLID,
    panel_solid: GRAPHITE_SOLID,
    border: Color::from_rgba8(255, 255, 255, 0.10),
    text: Color::from_rgb8(0xe6, 0xe8, 0xec),
    muted: Color::from_rgb8(0x9a, 0xa0, 0xaa),
    shadow: Color::from_rgba8(0, 0, 0, 0.35),
    hover: Color::from_rgba8(255, 255, 255, 0.07),
    pressed: Color::from_rgba8(255, 255, 255, 0.12),
    window: GRAPHITE_WINDOW,
    field: Color::from_rgba8(255, 255, 255, 0.05),
    field_focus: Color::from_rgba8(255, 255, 255, 0.10),
    keycap: Color::from_rgba8(255, 255, 255, 0.09),
    danger: Color::from_rgb8(0xf0, 0x8c, 0x8c),
};

/// Selection stroke and screen border gradient (both themes): violet → cyan.
pub const STROKE_START: Color = Color::from_rgb8(0xa9, 0x9b, 0xff);

/// Accent gradient (both themes): primary buttons, active tools.
pub const ACCENT_START: Color = Color::from_rgb8(0x8f, 0x80, 0xff);
pub const ACCENT_END: Color = Color::from_rgb8(0x4f, 0xc9, 0xe0);
/// Selected text in inputs (both themes): the accent, see-through.
pub const SELECTION: Color = Color::from_rgba8(0x8f, 0x80, 0xff, 0.35);
/// Text and icons on the accent gradient.
pub const ON_ACCENT: Color = Color::from_rgb8(0x10, 0x10, 0x1c);

/// The window header's close button under the cursor, and its glyph (both themes).
pub const CLOSE_HOVER: Color = Color::from_rgb8(0xc4, 0x2b, 0x1c);
pub const ON_CLOSE_HOVER: Color = Color::WHITE;

/// Dim outside the editor's crop (the overlay's dims follow the look: `dims`).
pub const DIM_OUTSIDE: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.45);

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

fn rgb(c: Rgb) -> Color {
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

/// The accent gradient of the active look (buttons, handles, the stroke).
pub fn accent() -> (Color, Color) {
    accent_of(&config::look())
}

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

/// Text highlighted in select-text mode: every word, and the picked ones.
pub const WORD: Color = Color::from_rgba8(0x9b, 0xb4, 0xff, 0.22);
pub const WORD_PICKED: Color = Color::from_rgba8(0x5a, 0x8c, 0xff, 0.55);

/// The palette floating surfaces (bar, pills, panels) use under the active
/// look's bar style: glass dark / light force one; the rest follow the app.
pub fn floating() -> &'static Palette {
    widget_palette(config::widgets().0).unwrap_or_else(app_floating)
}

/// A widget theme's own palette (`Default` has none: it follows the app).
fn widget_palette(t: WidgetTheme) -> Option<&'static Palette> {
    match t {
        WidgetTheme::Frost => Some(&FROST),
        WidgetTheme::Graphite => Some(&GRAPHITE),
        WidgetTheme::Default => None,
    }
}

fn app_floating() -> &'static Palette {
    match config::look().bar {
        BarStyle::GlassDark => &DARK,
        BarStyle::GlassLight => &LIGHT,
        _ => current(),
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

/// The editor's colour picker (annotations keep their colour in both themes:
/// they belong to the image, not the UI). The first is the default.
pub const ANNOTATION: [Color; 12] = [
    Color::from_rgb8(0xff, 0x3b, 0x30), // red
    Color::from_rgb8(0xff, 0x95, 0x00), // orange
    Color::from_rgb8(0xff, 0xd6, 0x0a), // yellow
    Color::from_rgb8(0x34, 0xc7, 0x59), // green
    Color::from_rgb8(0x00, 0xc7, 0xbe), // teal
    Color::from_rgb8(0x0a, 0x84, 0xff), // blue
    Color::from_rgb8(0x5e, 0x5c, 0xe6), // indigo
    Color::from_rgb8(0xaf, 0x52, 0xde), // purple
    Color::from_rgb8(0xff, 0x2d, 0x9a), // pink
    Color::from_rgb8(0xff, 0xff, 0xff), // white
    Color::from_rgb8(0x8e, 0x8e, 0x93), // grey
    Color::from_rgb8(0x1c, 0x1c, 0x1e), // black
];

/// Light palette instead of dark on this thread (the editor's snapshot test).
#[allow(dead_code)]
pub fn set_light(light: bool) {
    apply(if light { Theme::Light } else { Theme::Dark });
}

/// The palette in use.
pub fn current() -> &'static Palette {
    palette(SETTING.with(Cell::get), OS_DARK.with(Cell::get))
}

thread_local! {
    /// The UI thread's theme choice and the OS's mode (tests render Light and Dark
    /// side by side).
    static SETTING: Cell<Theme> = const { Cell::new(Theme::System) };
    static OS_DARK: Cell<bool> = const { Cell::new(true) };
}

/// The Light / Dark / System choice from Settings; System follows `set_os`.
pub fn apply(theme: Theme) {
    SETTING.with(|s| s.set(theme));
}

/// What the OS reports (at start, then on every change). "No preference" counts as dark.
pub fn set_os(mode: iced::theme::Mode) {
    OS_DARK.with(|d| d.set(mode != iced::theme::Mode::Light));
}

fn palette(setting: Theme, os_dark: bool) -> &'static Palette {
    match (setting, os_dark) {
        (Theme::Dark, _) | (Theme::System, true) => &DARK,
        (Theme::Light, _) | (Theme::System, false) => &LIGHT,
    }
}

/// The setup's view of the kit's themes (`kit::themes`): Sapphire for a dark OS,
/// Daylight for a light one (`set_light`, from the OS). Goes when the setup is on
/// the kit (plan step 21); `scripts/installer-art.py` repeats a few colours for
/// the setup's baked images.
pub mod sapphire {
    use crate::ui::kit::themes::{self, Colours, DAYLIGHT, SAPPHIRE};
    pub use crate::ui::kit::tokens::fixed::{DANGER, ON_ACCENT, SUCCESS};

    /// What the OS reports: light = Daylight, dark (or no preference) = Sapphire.
    pub fn set_light(light: bool) {
        themes::set(if light { &DAYLIGHT } else { &SAPPHIRE });
    }

    pub fn is_light() -> bool {
        themes::current().light
    }

    /// The token set in use.
    pub fn t() -> &'static Colours {
        themes::c()
    }
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

    #[test]
    fn light_and_dark_differ() {
        assert_ne!(DARK.text, LIGHT.text);
    }

    #[test]
    fn solid_panels_are_opaque_and_between_panel_and_window() {
        for p in [&DARK, &LIGHT] {
            assert_eq!(p.panel_solid.a, 1.0);
            for (s, a, b) in [
                (p.panel_solid.r, p.panel.r, p.window.r),
                (p.panel_solid.g, p.panel.g, p.window.g),
                (p.panel_solid.b, p.panel.b, p.window.b),
            ] {
                assert!(s >= a.min(b) - 1e-6 && s <= a.max(b) + 1e-6);
            }
        }
        // 0.86 of (22,22,34) over (18,19,31): close to the panel colour
        assert!((DARK.panel_solid.r - (22.0 * 0.86 + 18.0 * 0.14) / 255.0).abs() < 1e-3);
        assert!((LIGHT.panel_solid.r - (255.0 * 0.9 + 246.0 * 0.1) / 255.0).abs() < 1e-3);
    }

    #[test]
    fn every_widget_theme_resolves_and_has_a_solid_fallback() {
        let all = [
            WidgetTheme::Default,
            WidgetTheme::Frost,
            WidgetTheme::Graphite,
        ];
        let mut texts = Vec::new();
        for t in all {
            let p = widget_palette(t).unwrap_or(&DARK);
            assert_eq!(p.panel_solid.a, 1.0);
            texts.push(p.text);
        }
        assert!(texts[0] != texts[1] && texts[1] != texts[2] && texts[0] != texts[2]);
        // Readable: text far from the tint in lightness.
        let l = |c: Color| 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
        for p in [&FROST, &GRAPHITE] {
            assert!((l(p.text) - l(p.panel_solid)).abs() > 0.6);
        }
    }

    #[test]
    fn every_widget_surface_is_opaque_in_every_theme() {
        for t in [
            WidgetTheme::Default,
            WidgetTheme::Frost,
            WidgetTheme::Graphite,
        ] {
            for p in [widget_palette(t).unwrap_or(&DARK), &LIGHT] {
                assert_eq!((p.panel.a, p.panel_solid.a, p.window.a), (1.0, 1.0, 1.0));
            }
        }
        // the setup's Sapphire / Daylight: kit::tests::every_surface_is_opaque_in_every_theme
    }

    #[test]
    fn system_follows_the_os_and_the_others_force() {
        let text = |setting, os_dark| palette(setting, os_dark).text;
        assert_eq!(text(Theme::System, true), DARK.text);
        assert_eq!(text(Theme::System, false), LIGHT.text);
        assert_eq!(text(Theme::Light, true), LIGHT.text);
        assert_eq!(text(Theme::Dark, false), DARK.text);
    }

    #[test]
    fn os_mode_reaches_current() {
        apply(Theme::System);
        set_os(Mode::Light);
        assert_eq!(current().text, LIGHT.text);
        set_os(Mode::Dark);
        assert_eq!(current().text, DARK.text);
        set_os(Mode::None);
        assert_eq!(current().text, DARK.text);
        apply(Theme::Light);
        assert_eq!(current().text, LIGHT.text);
    }
}
