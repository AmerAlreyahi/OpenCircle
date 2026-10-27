//! The design's 12 app themes as data (`newdesing/styles/tokens.css`, solid
//! values; names from `core/theme-context.js`), and the theme in use.
//! Glass values and shadows are not ported: the CPU renderer draws neither
//! (glass returns with the GPU renderer). Field names follow the design's
//! variables (`--app-bg-solid` = `bg`, `--ui-elevated-bg` = `elevated`, …).
// ponytail: used from step 5 on (the kit components); until then the setup reads Sapphire and Daylight.
#![allow(dead_code)]

use std::cell::Cell;

use iced::Color;

pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub light: bool,
    /// The theme card's three dots: background, elevated, accent.
    pub swatch: [Color; 3],
    pub c: Colours,
}

#[derive(Debug, PartialEq)]
pub struct Colours {
    /// `--app-*-solid`: the opaque surfaces (`Material`).
    pub bg: Color,
    pub sidebar: Color,
    pub widget: Color,
    pub dropdown: Color,
    pub track: Color,
    /// `--app-track-glass`: a recessed panel laid over a window (the setup's progress).
    pub track_glass: Color,
    /// `--ui-elevated-*`: buttons, the chosen segment; `elevated_hi` is the 1 px top line.
    pub elevated: Color,
    pub elevated_hover: Color,
    pub elevated_hi: Color,
    pub elevated_border: Color,
    /// `--ui-card-*`; light themes ring a selected card in `card_ring`.
    pub card_idle: Color,
    pub card_selected: Color,
    pub card_ring: Option<Color>,
    /// `--app-rim-border`, `--app-divider`, `--app-hover-bg`, `--app-keycap-*`.
    pub rim: Color,
    pub divider: Color,
    pub hover: Color,
    pub keycap: Color,
    pub keycap_border: Color,
    pub text: Color,
    pub text_sub: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
}

/// The OS mode setting: Light / Dark / System.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    System,
    Light,
    Dark,
}

thread_local! {
    /// The UI thread's theme (tests render several side by side, one per thread).
    static CURRENT: Cell<&'static Theme> = const { Cell::new(&SAPPHIRE) };
}

/// The theme in use.
pub fn current() -> &'static Theme {
    CURRENT.get()
}

pub fn set(theme: &'static Theme) {
    CURRENT.set(theme);
}

/// The theme to show: the dark pick or the light pick (theme ids) by `mode`,
/// `System` following the OS. An unknown id, or one of the other mode, falls
/// back to Sapphire / Daylight.
pub fn resolve(mode: Mode, dark: &str, light: &str, os_dark: bool) -> &'static Theme {
    let light_mode = match mode {
        Mode::Light => true,
        Mode::Dark => false,
        Mode::System => !os_dark,
    };
    let (id, fallback) = match light_mode {
        true => (light, &DAYLIGHT),
        false => (dark, &SAPPHIRE),
    };
    THEMES
        .into_iter()
        .find(|t| t.id == id && t.light == light_mode)
        .unwrap_or(fallback)
}

pub const OBSIDIAN: Theme = Theme {
    id: "obsidian",
    name: "Obsidian",
    desc: "Smoked dark glass",
    light: false,
    swatch: [
        Color::from_rgb8(0x0f, 0x12, 0x1a),
        Color::from_rgb8(0x23, 0x2b, 0x3e),
        Color::from_rgb8(0x38, 0xbd, 0xf8),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x10, 0x13, 0x1b),
        sidebar: Color::from_rgb8(0x0c, 0x0e, 0x15),
        widget: Color::from_rgb8(0x14, 0x17, 0x21),
        dropdown: Color::from_rgb8(0x14, 0x18, 0x24),
        track: Color::from_rgb8(0x0a, 0x0c, 0x12),
        track_glass: Color::from_rgba8(6, 8, 14, 0.44),
        elevated: Color::from_rgb8(0x23, 0x2b, 0x3e),
        elevated_hover: Color::from_rgb8(0x2a, 0x34, 0x4b),
        elevated_hi: Color::from_rgba8(255, 255, 255, 0.09),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(8, 11, 18, 0.36),
        card_selected: Color::from_rgb8(0x23, 0x2b, 0x3e),
        rim: Color::from_rgba8(255, 255, 255, 0.13),
        divider: Color::from_rgba8(255, 255, 255, 0.075),
        hover: Color::from_rgba8(255, 255, 255, 0.055),
        keycap: Color::from_rgb8(0x1c, 0x23, 0x33),
        keycap_border: Color::from_rgba8(255, 255, 255, 0.14),
        text: Color::from_rgb8(0xf4, 0xf6, 0xfb),
        text_sub: Color::from_rgba8(226, 232, 245, 0.64),
        text_muted: Color::from_rgba8(226, 232, 245, 0.42),
        accent: Color::from_rgb8(0x38, 0xbd, 0xf8),
        accent_soft: Color::from_rgba8(56, 189, 248, 0.16),
        card_ring: None,
    },
};

pub const MICA: Theme = Theme {
    id: "mica",
    name: "Mica Slate",
    desc: "Win11 Mica slate tint",
    light: false,
    swatch: [
        Color::from_rgb8(0x16, 0x1b, 0x28),
        Color::from_rgb8(0x27, 0x33, 0x4d),
        Color::from_rgb8(0x60, 0xa5, 0xfa),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x16, 0x1b, 0x28),
        sidebar: Color::from_rgb8(0x11, 0x15, 0x20),
        widget: Color::from_rgb8(0x1a, 0x20, 0x30),
        dropdown: Color::from_rgb8(0x1a, 0x21, 0x32),
        track: Color::from_rgb8(0x0e, 0x12, 0x1b),
        track_glass: Color::from_rgba8(9, 12, 20, 0.44),
        elevated: Color::from_rgb8(0x27, 0x33, 0x4d),
        elevated_hover: Color::from_rgb8(0x2f, 0x3d, 0x5c),
        elevated_hi: Color::from_rgba8(190, 220, 255, 0.11),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(10, 14, 24, 0.36),
        card_selected: Color::from_rgb8(0x27, 0x33, 0x4d),
        rim: Color::from_rgba8(190, 220, 255, 0.15),
        divider: Color::from_rgba8(190, 220, 255, 0.08),
        hover: Color::from_rgba8(190, 220, 255, 0.06),
        keycap: Color::from_rgb8(0x20, 0x2a, 0x40),
        keycap_border: Color::from_rgba8(190, 220, 255, 0.16),
        text: Color::from_rgb8(0xf3, 0xf7, 0xff),
        text_sub: Color::from_rgba8(214, 226, 248, 0.66),
        text_muted: Color::from_rgba8(214, 226, 248, 0.42),
        accent: Color::from_rgb8(0x60, 0xa5, 0xfa),
        accent_soft: Color::from_rgba8(96, 165, 250, 0.16),
        card_ring: None,
    },
};

pub const TITANIUM: Theme = Theme {
    id: "titanium",
    name: "Titanium",
    desc: "Warm brushed gunmetal",
    light: false,
    swatch: [
        Color::from_rgb8(0x18, 0x18, 0x1b),
        Color::from_rgb8(0x32, 0x31, 0x38),
        Color::from_rgb8(0xf5, 0xd0, 0x8a),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x18, 0x18, 0x1b),
        sidebar: Color::from_rgb8(0x13, 0x13, 0x16),
        widget: Color::from_rgb8(0x1d, 0x1d, 0x21),
        dropdown: Color::from_rgb8(0x22, 0x22, 0x27),
        track: Color::from_rgb8(0x10, 0x10, 0x13),
        track_glass: Color::from_rgba8(11, 11, 14, 0.45),
        elevated: Color::from_rgb8(0x32, 0x31, 0x38),
        elevated_hover: Color::from_rgb8(0x3b, 0x3a, 0x42),
        elevated_hi: Color::from_rgba8(255, 248, 235, 0.10),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(12, 12, 15, 0.36),
        card_selected: Color::from_rgb8(0x32, 0x31, 0x38),
        rim: Color::from_rgba8(255, 248, 235, 0.14),
        divider: Color::from_rgba8(255, 248, 235, 0.075),
        hover: Color::from_rgba8(255, 248, 235, 0.055),
        keycap: Color::from_rgb8(0x27, 0x27, 0x2c),
        keycap_border: Color::from_rgba8(255, 248, 235, 0.15),
        text: Color::from_rgb8(0xfa, 0xf8, 0xf5),
        text_sub: Color::from_rgba8(235, 230, 220, 0.64),
        text_muted: Color::from_rgba8(235, 230, 220, 0.42),
        accent: Color::from_rgb8(0xf5, 0xd0, 0x8a),
        accent_soft: Color::from_rgba8(245, 208, 138, 0.16),
        card_ring: None,
    },
};

pub const SAPPHIRE: Theme = Theme {
    id: "sapphire",
    name: "Sapphire",
    desc: "Deep cobalt refraction",
    light: false,
    swatch: [
        Color::from_rgb8(0x0c, 0x17, 0x2e),
        Color::from_rgb8(0x1c, 0x36, 0x64),
        Color::from_rgb8(0x38, 0xbd, 0xf8),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x0c, 0x17, 0x2e),
        sidebar: Color::from_rgb8(0x09, 0x12, 0x24),
        widget: Color::from_rgb8(0x10, 0x1f, 0x3d),
        dropdown: Color::from_rgb8(0x10, 0x20, 0x40),
        track: Color::from_rgb8(0x07, 0x10, 0x21),
        track_glass: Color::from_rgba8(4, 10, 24, 0.46),
        elevated: Color::from_rgb8(0x1c, 0x36, 0x64),
        elevated_hover: Color::from_rgb8(0x23, 0x42, 0x7a),
        elevated_hi: Color::from_rgba8(155, 205, 255, 0.13),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(5, 12, 28, 0.40),
        card_selected: Color::from_rgb8(0x1c, 0x36, 0x64),
        rim: Color::from_rgba8(155, 205, 255, 0.17),
        divider: Color::from_rgba8(155, 205, 255, 0.085),
        hover: Color::from_rgba8(155, 205, 255, 0.065),
        keycap: Color::from_rgb8(0x15, 0x29, 0x4d),
        keycap_border: Color::from_rgba8(155, 205, 255, 0.18),
        text: Color::from_rgb8(0xf0, 0xf7, 0xff),
        text_sub: Color::from_rgba8(198, 222, 255, 0.66),
        text_muted: Color::from_rgba8(198, 222, 255, 0.42),
        accent: Color::from_rgb8(0x38, 0xbd, 0xf8),
        accent_soft: Color::from_rgba8(56, 189, 248, 0.18),
        card_ring: None,
    },
};

pub const AMETHYST: Theme = Theme {
    id: "amethyst",
    name: "Amethyst",
    desc: "Velvet iris & violet",
    light: false,
    swatch: [
        Color::from_rgb8(0x17, 0x10, 0x29),
        Color::from_rgb8(0x32, 0x21, 0x56),
        Color::from_rgb8(0xc0, 0x84, 0xfc),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x17, 0x10, 0x29),
        sidebar: Color::from_rgb8(0x12, 0x0c, 0x20),
        widget: Color::from_rgb8(0x1d, 0x14, 0x33),
        dropdown: Color::from_rgb8(0x22, 0x16, 0x3a),
        track: Color::from_rgb8(0x0d, 0x08, 0x18),
        track_glass: Color::from_rgba8(10, 5, 20, 0.46),
        elevated: Color::from_rgb8(0x32, 0x21, 0x56),
        elevated_hover: Color::from_rgb8(0x3d, 0x29, 0x68),
        elevated_hi: Color::from_rgba8(216, 190, 255, 0.12),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(11, 6, 22, 0.40),
        card_selected: Color::from_rgb8(0x32, 0x21, 0x56),
        rim: Color::from_rgba8(205, 175, 255, 0.16),
        divider: Color::from_rgba8(205, 175, 255, 0.085),
        hover: Color::from_rgba8(205, 175, 255, 0.065),
        keycap: Color::from_rgb8(0x26, 0x18, 0x42),
        keycap_border: Color::from_rgba8(205, 175, 255, 0.18),
        text: Color::from_rgb8(0xf7, 0xf3, 0xff),
        text_sub: Color::from_rgba8(224, 208, 255, 0.65),
        text_muted: Color::from_rgba8(224, 208, 255, 0.42),
        accent: Color::from_rgb8(0xc0, 0x84, 0xfc),
        accent_soft: Color::from_rgba8(192, 132, 252, 0.18),
        card_ring: None,
    },
};

pub const EMERALD: Theme = Theme {
    id: "emerald",
    name: "Emerald",
    desc: "Deep forest jade glass",
    light: false,
    swatch: [
        Color::from_rgb8(0x0b, 0x1f, 0x1c),
        Color::from_rgb8(0x17, 0x40, 0x38),
        Color::from_rgb8(0x34, 0xd3, 0x99),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x0b, 0x1f, 0x1c),
        sidebar: Color::from_rgb8(0x08, 0x17, 0x15),
        widget: Color::from_rgb8(0x0f, 0x29, 0x25),
        dropdown: Color::from_rgb8(0x10, 0x2d, 0x28),
        track: Color::from_rgb8(0x06, 0x12, 0x10),
        track_glass: Color::from_rgba8(4, 15, 13, 0.46),
        elevated: Color::from_rgb8(0x17, 0x40, 0x38),
        elevated_hover: Color::from_rgb8(0x1d, 0x4f, 0x45),
        elevated_hi: Color::from_rgba8(150, 242, 208, 0.12),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(4, 15, 13, 0.40),
        card_selected: Color::from_rgb8(0x17, 0x40, 0x38),
        rim: Color::from_rgba8(140, 235, 200, 0.16),
        divider: Color::from_rgba8(140, 235, 200, 0.08),
        hover: Color::from_rgba8(140, 235, 200, 0.06),
        keycap: Color::from_rgb8(0x12, 0x31, 0x2b),
        keycap_border: Color::from_rgba8(140, 235, 200, 0.18),
        text: Color::from_rgb8(0xef, 0xfd, 0xf9),
        text_sub: Color::from_rgba8(196, 240, 226, 0.64),
        text_muted: Color::from_rgba8(196, 240, 226, 0.42),
        accent: Color::from_rgb8(0x34, 0xd3, 0x99),
        accent_soft: Color::from_rgba8(52, 211, 153, 0.18),
        card_ring: None,
    },
};

pub const ROSE: Theme = Theme {
    id: "rose",
    name: "Rose Quartz",
    desc: "Warm dusk crimson",
    light: false,
    swatch: [
        Color::from_rgb8(0x23, 0x12, 0x1b),
        Color::from_rgb8(0x48, 0x21, 0x33),
        Color::from_rgb8(0xfb, 0x71, 0x85),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x23, 0x12, 0x1b),
        sidebar: Color::from_rgb8(0x1a, 0x0d, 0x14),
        widget: Color::from_rgb8(0x2c, 0x16, 0x22),
        dropdown: Color::from_rgb8(0x30, 0x17, 0x24),
        track: Color::from_rgb8(0x13, 0x09, 0x0e),
        track_glass: Color::from_rgba8(16, 7, 12, 0.46),
        elevated: Color::from_rgb8(0x48, 0x21, 0x33),
        elevated_hover: Color::from_rgb8(0x57, 0x28, 0x3e),
        elevated_hi: Color::from_rgba8(254, 205, 211, 0.12),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(16, 7, 12, 0.40),
        card_selected: Color::from_rgb8(0x48, 0x21, 0x33),
        rim: Color::from_rgba8(254, 205, 211, 0.16),
        divider: Color::from_rgba8(254, 205, 211, 0.08),
        hover: Color::from_rgba8(254, 205, 211, 0.06),
        keycap: Color::from_rgb8(0x36, 0x19, 0x27),
        keycap_border: Color::from_rgba8(254, 205, 211, 0.18),
        text: Color::from_rgb8(0xff, 0xf2, 0xf5),
        text_sub: Color::from_rgba8(248, 210, 220, 0.65),
        text_muted: Color::from_rgba8(248, 210, 220, 0.42),
        accent: Color::from_rgb8(0xfb, 0x71, 0x85),
        accent_soft: Color::from_rgba8(251, 113, 133, 0.18),
        card_ring: None,
    },
};

pub const GRAPHITE: Theme = Theme {
    id: "graphite",
    name: "Graphite",
    desc: "Calm neutral carbon",
    light: false,
    swatch: [
        Color::from_rgb8(0x1f, 0x22, 0x28),
        Color::from_rgb8(0x34, 0x39, 0x44),
        Color::from_rgb8(0x94, 0xa3, 0xb8),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x1f, 0x22, 0x28),
        sidebar: Color::from_rgb8(0x18, 0x1a, 0x1f),
        widget: Color::from_rgb8(0x26, 0x29, 0x30),
        dropdown: Color::from_rgb8(0x2a, 0x2e, 0x36),
        track: Color::from_rgb8(0x13, 0x15, 0x19),
        track_glass: Color::from_rgba8(14, 15, 18, 0.44),
        elevated: Color::from_rgb8(0x34, 0x39, 0x44),
        elevated_hover: Color::from_rgb8(0x3e, 0x44, 0x51),
        elevated_hi: Color::from_rgba8(255, 255, 255, 0.09),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(15, 17, 21, 0.36),
        card_selected: Color::from_rgb8(0x34, 0x39, 0x44),
        rim: Color::from_rgba8(255, 255, 255, 0.13),
        divider: Color::from_rgba8(255, 255, 255, 0.07),
        hover: Color::from_rgba8(255, 255, 255, 0.055),
        keycap: Color::from_rgb8(0x2b, 0x2f, 0x38),
        keycap_border: Color::from_rgba8(255, 255, 255, 0.15),
        text: Color::from_rgb8(0xe6, 0xe8, 0xec),
        text_sub: Color::from_rgba8(200, 205, 215, 0.64),
        text_muted: Color::from_rgba8(200, 205, 215, 0.42),
        accent: Color::from_rgb8(0x94, 0xa3, 0xb8),
        accent_soft: Color::from_rgba8(148, 163, 184, 0.18),
        card_ring: None,
    },
};

pub const MIDNIGHT: Theme = Theme {
    id: "midnight",
    name: "Midnight OLED",
    desc: "Pitch-black high contrast",
    light: false,
    swatch: [
        Color::from_rgb8(0x05, 0x06, 0x0a),
        Color::from_rgb8(0x17, 0x20, 0x33),
        Color::from_rgb8(0x38, 0xbd, 0xf8),
    ],
    c: Colours {
        bg: Color::from_rgb8(0x05, 0x06, 0x0a),
        sidebar: Color::from_rgb8(0x03, 0x04, 0x07),
        widget: Color::from_rgb8(0x09, 0x0b, 0x12),
        dropdown: Color::from_rgb8(0x0a, 0x0d, 0x14),
        track: Color::from_rgb8(0x02, 0x03, 0x05),
        track_glass: Color::from_rgba8(0, 0, 0, 0.58),
        elevated: Color::from_rgb8(0x17, 0x20, 0x33),
        elevated_hover: Color::from_rgb8(0x1e, 0x2a, 0x42),
        elevated_hi: Color::from_rgba8(125, 211, 252, 0.12),
        elevated_border: Color::TRANSPARENT,
        card_idle: Color::from_rgba8(2, 4, 9, 0.55),
        card_selected: Color::from_rgb8(0x17, 0x20, 0x33),
        rim: Color::from_rgba8(255, 255, 255, 0.13),
        divider: Color::from_rgba8(255, 255, 255, 0.07),
        hover: Color::from_rgba8(255, 255, 255, 0.05),
        keycap: Color::from_rgb8(0x11, 0x17, 0x26),
        keycap_border: Color::from_rgba8(255, 255, 255, 0.15),
        text: Color::from_rgb8(0xf8, 0xfa, 0xfc),
        text_sub: Color::from_rgba8(203, 213, 225, 0.64),
        text_muted: Color::from_rgba8(203, 213, 225, 0.40),
        accent: Color::from_rgb8(0x38, 0xbd, 0xf8),
        accent_soft: Color::from_rgba8(56, 189, 248, 0.18),
        card_ring: None,
    },
};

pub const FROST: Theme = Theme {
    id: "frost",
    name: "Arctic Frost",
    desc: "Pale icy translucent",
    light: true,
    swatch: [
        Color::from_rgb8(0xe4, 0xea, 0xf4),
        Color::from_rgb8(0xff, 0xff, 0xff),
        Color::from_rgb8(0x25, 0x63, 0xeb),
    ],
    c: Colours {
        bg: Color::from_rgb8(0xe4, 0xea, 0xf4),
        sidebar: Color::from_rgb8(0xd9, 0xe1, 0xee),
        widget: Color::from_rgb8(0xee, 0xf3, 0xfb),
        dropdown: Color::from_rgb8(0xf2, 0xf6, 0xfc),
        track: Color::from_rgb8(0xd2, 0xdc, 0xeb),
        track_glass: Color::from_rgba8(16, 24, 48, 0.075),
        elevated: Color::from_rgb8(0xff, 0xff, 0xff),
        elevated_hover: Color::from_rgb8(0xf8, 0xfb, 0xff),
        elevated_hi: Color::from_rgba8(255, 255, 255, 0.95),
        elevated_border: Color::from_rgba8(16, 24, 48, 0.08),
        card_idle: Color::from_rgba8(255, 255, 255, 0.42),
        card_selected: Color::from_rgb8(0xff, 0xff, 0xff),
        rim: Color::from_rgba8(16, 24, 48, 0.12),
        divider: Color::from_rgba8(16, 24, 48, 0.08),
        hover: Color::from_rgba8(16, 24, 48, 0.05),
        keycap: Color::from_rgb8(0xff, 0xff, 0xff),
        keycap_border: Color::from_rgba8(16, 24, 48, 0.14),
        text: Color::from_rgb8(0x14, 0x18, 0x26),
        text_sub: Color::from_rgba8(20, 24, 38, 0.65),
        text_muted: Color::from_rgba8(20, 24, 38, 0.44),
        accent: Color::from_rgb8(0x25, 0x63, 0xeb),
        accent_soft: Color::from_rgba8(37, 99, 235, 0.12),
        card_ring: Some(Color::from_rgba8(37, 99, 235, 0.18)),
    },
};

pub const DAYLIGHT: Theme = Theme {
    id: "daylight",
    name: "Daylight",
    desc: "Crisp alabaster light",
    light: true,
    swatch: [
        Color::from_rgb8(0xf5, 0xf7, 0xfa),
        Color::from_rgb8(0xff, 0xff, 0xff),
        Color::from_rgb8(0x25, 0x63, 0xeb),
    ],
    c: Colours {
        bg: Color::from_rgb8(0xf5, 0xf7, 0xfa),
        sidebar: Color::from_rgb8(0xea, 0xee, 0xf4),
        widget: Color::from_rgb8(0xff, 0xff, 0xff),
        dropdown: Color::from_rgb8(0xff, 0xff, 0xff),
        track: Color::from_rgb8(0xe2, 0xe7, 0xf0),
        track_glass: Color::from_rgba8(15, 23, 42, 0.07),
        elevated: Color::from_rgb8(0xff, 0xff, 0xff),
        elevated_hover: Color::from_rgb8(0xf8, 0xfa, 0xfc),
        elevated_hi: Color::from_rgba8(255, 255, 255, 1.0),
        elevated_border: Color::from_rgba8(15, 23, 42, 0.08),
        card_idle: Color::from_rgba8(15, 23, 42, 0.035),
        card_selected: Color::from_rgb8(0xff, 0xff, 0xff),
        rim: Color::from_rgba8(15, 23, 42, 0.11),
        divider: Color::from_rgba8(15, 23, 42, 0.075),
        hover: Color::from_rgba8(15, 23, 42, 0.045),
        keycap: Color::from_rgb8(0xff, 0xff, 0xff),
        keycap_border: Color::from_rgba8(15, 23, 42, 0.14),
        text: Color::from_rgb8(0x0f, 0x14, 0x1e),
        text_sub: Color::from_rgba8(15, 20, 30, 0.62),
        text_muted: Color::from_rgba8(15, 20, 30, 0.42),
        accent: Color::from_rgb8(0x25, 0x63, 0xeb),
        accent_soft: Color::from_rgba8(37, 99, 235, 0.12),
        card_ring: Some(Color::from_rgba8(37, 99, 235, 0.18)),
    },
};

pub const SANDSTONE: Theme = Theme {
    id: "sandstone",
    name: "Sandstone",
    desc: "Warm paper & dune ivory",
    light: true,
    swatch: [
        Color::from_rgb8(0xf3, 0xed, 0xe2),
        Color::from_rgb8(0xff, 0xfd, 0xf9),
        Color::from_rgb8(0xd9, 0x77, 0x06),
    ],
    c: Colours {
        bg: Color::from_rgb8(0xf3, 0xed, 0xe2),
        sidebar: Color::from_rgb8(0xe7, 0xdf, 0xd1),
        widget: Color::from_rgb8(0xfa, 0xf6, 0xf0),
        dropdown: Color::from_rgb8(0xfa, 0xf5, 0xed),
        track: Color::from_rgb8(0xdf, 0xd6, 0xc7),
        track_glass: Color::from_rgba8(40, 30, 20, 0.075),
        elevated: Color::from_rgb8(0xff, 0xfd, 0xf9),
        elevated_hover: Color::from_rgb8(0xfa, 0xf5, 0xec),
        elevated_hi: Color::from_rgba8(255, 255, 255, 0.95),
        elevated_border: Color::from_rgba8(40, 30, 20, 0.09),
        card_idle: Color::from_rgba8(40, 30, 20, 0.04),
        card_selected: Color::from_rgb8(0xff, 0xfd, 0xf9),
        rim: Color::from_rgba8(40, 30, 20, 0.12),
        divider: Color::from_rgba8(40, 30, 20, 0.08),
        hover: Color::from_rgba8(40, 30, 20, 0.045),
        keycap: Color::from_rgb8(0xff, 0xfd, 0xf9),
        keycap_border: Color::from_rgba8(40, 30, 20, 0.15),
        text: Color::from_rgb8(0x1c, 0x18, 0x13),
        text_sub: Color::from_rgba8(28, 24, 19, 0.64),
        text_muted: Color::from_rgba8(28, 24, 19, 0.44),
        accent: Color::from_rgb8(0xd9, 0x77, 0x06),
        accent_soft: Color::from_rgba8(217, 119, 6, 0.14),
        card_ring: Some(Color::from_rgba8(217, 119, 6, 0.22)),
    },
};

pub const THEMES: [&Theme; 12] = [
    &OBSIDIAN, &MICA, &TITANIUM, &SAPPHIRE, &AMETHYST, &EMERALD, &ROSE, &GRAPHITE, &MIDNIGHT,
    &FROST, &DAYLIGHT, &SANDSTONE,
];
