//! The design's 10 Active Looks (the screen edge while the overlay is open) as
//! data (`newdesing/core/theme-context.js` `ACTIVE_LOOKS`), plus the user's own
//! two colours.
// ponytail: `solid` and `Active` wait for their port; the edge reads `kind` and `glow`.
#![allow(dead_code)]

use iced::Color;

/// How the look draws the edge (the design's `type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An inset glow round the whole edge (`conic`).
    Glow,
    /// Light along the top and bottom edges (`horizon`).
    Horizon,
    /// Viewfinder corner brackets over a soft glow (`hud`).
    Hud,
    /// A thin frosted bezel (`bezel`).
    Bezel,
}

pub struct Look {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub badge: &'static str,
    pub kind: Kind,
    /// `solid1`, `solid2`: the look's accent pair.
    pub solid: [Color; 2],
    /// `c1`, `c2`: the edge glow near the edge and further in, alpha included.
    pub glow: [Color; 2],
    /// The look card's gradient stops (`previewGrad`).
    pub preview: &'static [Color],
}

/// The look in use: a design look or the user's own two colours.
#[derive(Clone, Copy)]
pub enum Active {
    Design(&'static Look),
    Custom([Color; 2]),
}

impl Active {
    /// The edge glow; a custom pair takes Spectrum's alphas.
    pub fn glow(self) -> [Color; 2] {
        match self {
            Active::Design(look) => look.glow,
            Active::Custom([a, b]) => {
                let [ga, gb] = SPECTRUM.glow;
                [Color { a: ga.a, ..a }, Color { a: gb.a, ..b }]
            }
        }
    }

    pub fn solid(self) -> [Color; 2] {
        match self {
            Active::Design(look) => look.solid,
            Active::Custom(pair) => pair,
        }
    }
}

const fn hex(rgb: u32) -> Color {
    Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const PRISM: Look = Look {
    id: "prism",
    name: "Liquid Prism",
    desc: "Cyan-violet glow round the screen",
    badge: "Popular",
    kind: Kind::Glow,
    solid: [hex(0x38bdf8), hex(0xa78bfa)],
    glow: [
        Color::from_rgba8(125, 211, 252, 0.42),
        Color::from_rgba8(167, 139, 250, 0.25),
    ],
    preview: &[hex(0x38bdf8), hex(0xffffff), hex(0xa78bfa)],
};

pub const SPECTRUM: Look = Look {
    id: "spectrum",
    name: "Spectrum",
    desc: "Multi-colour rainbow glow",
    badge: "Default",
    kind: Kind::Glow,
    solid: [hex(0xff6b5e), hex(0x4aa8ff)],
    glow: [
        Color::from_rgba8(155, 123, 255, 0.42),
        Color::from_rgba8(255, 107, 94, 0.26),
    ],
    preview: &[
        hex(0x9b7bff),
        hex(0xff6b5e),
        hex(0xffb547),
        hex(0x5fd38a),
        hex(0x4aa8ff),
    ],
};

pub const AURORA: Look = Look {
    id: "aurora",
    name: "Aurora Borealis",
    desc: "Northern emerald, teal & violet ribbon",
    badge: "New",
    kind: Kind::Glow,
    solid: [hex(0x34d399), hex(0x8b5cf6)],
    glow: [
        Color::from_rgba8(52, 211, 153, 0.42),
        Color::from_rgba8(139, 92, 246, 0.28),
    ],
    preview: &[hex(0x34d399), hex(0x2dd4bf), hex(0x8b5cf6)],
};

pub const HORIZON: Look = Look {
    id: "horizon",
    name: "Horizon Pulse",
    desc: "Soft light along the top & bottom",
    badge: "Calm",
    kind: Kind::Horizon,
    solid: [hex(0x38bdf8), hex(0x818cf8)],
    glow: [
        Color::from_rgba8(56, 189, 248, 0.52),
        Color::from_rgba8(99, 102, 241, 0.28),
    ],
    preview: &[hex(0x38bdf8), hex(0xffffff), hex(0x818cf8)],
};

pub const SUNSET: Look = Look {
    id: "sunset",
    name: "Sunset Flare",
    desc: "Warm amber, coral & magenta horizon",
    badge: "Warm",
    kind: Kind::Glow,
    solid: [hex(0xff8f3f), hex(0xff559a)],
    glow: [
        Color::from_rgba8(255, 143, 63, 0.44),
        Color::from_rgba8(255, 85, 154, 0.28),
    ],
    preview: &[hex(0xff8f3f), hex(0xff559a), hex(0xfb7185)],
};

pub const OCEAN: Look = Look {
    id: "ocean",
    name: "Ocean Ripple",
    desc: "Bioluminescent aqua & deep azure",
    badge: "Cool",
    kind: Kind::Glow,
    solid: [hex(0x2ad4c7), hex(0x3d7bff)],
    glow: [
        Color::from_rgba8(42, 212, 199, 0.45),
        Color::from_rgba8(61, 123, 255, 0.30),
    ],
    preview: &[hex(0x2ad4c7), hex(0x38bdf8), hex(0x3d7bff)],
};

pub const NEON: Look = Look {
    id: "neon",
    name: "Cyber Neon",
    desc: "High-energy electric cyan & hot pink",
    badge: "Vibrant",
    kind: Kind::Glow,
    solid: [hex(0x00f0ff), hex(0xff3fd0)],
    glow: [
        Color::from_rgba8(0, 240, 255, 0.50),
        Color::from_rgba8(255, 63, 208, 0.34),
    ],
    preview: &[hex(0x00f0ff), hex(0xff3fd0)],
};

pub const SOLAR: Look = Look {
    id: "solar",
    name: "Solar Gold",
    desc: "Champagne gold & warm white lustre",
    badge: "Luxury",
    kind: Kind::Horizon,
    solid: [hex(0xfbbf24), hex(0xf59e0b)],
    glow: [
        Color::from_rgba8(251, 191, 36, 0.48),
        Color::from_rgba8(245, 158, 11, 0.25),
    ],
    preview: &[hex(0xf59e0b), hex(0xfef3c7), hex(0xfbbf24)],
};

pub const HUD: Look = Look {
    id: "hud",
    name: "Optical HUD",
    desc: "Precision viewfinder corner brackets",
    badge: "Minimal",
    kind: Kind::Hud,
    solid: [hex(0xffffff), hex(0x7dd3fc)],
    glow: [
        Color::from_rgba8(255, 255, 255, 0.90),
        Color::from_rgba8(125, 211, 252, 0.60),
    ],
    preview: &[hex(0xffffff), hex(0x7dd3fc)],
};

pub const FROST: Look = Look {
    id: "frost",
    name: "Stealth Frost",
    desc: "Thin monochrome silver bezel",
    badge: "Quiet",
    kind: Kind::Bezel,
    solid: [hex(0xe2e8f0), hex(0x94a3b8)],
    glow: [
        Color::from_rgba8(255, 255, 255, 0.42),
        Color::from_rgba8(255, 255, 255, 0.07),
    ],
    preview: &[hex(0xe2e8f0), hex(0x64748b)],
};

/// In the design's order (the Settings cards). Stealth Frost (`FROST`) has
/// no card (owner, 2026-10-30); a config that has it ("mono") still shows it.
pub const LOOKS: [&Look; 9] = [
    &PRISM, &SPECTRUM, &AURORA, &HORIZON, &SUNSET, &OCEAN, &NEON, &SOLAR, &HUD,
];
