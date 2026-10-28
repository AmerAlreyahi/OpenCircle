//! The look presets and the custom look's reader.

use toml::Table;

use super::*;

pub(super) const fn edge(style: EdgeStyle, thickness: u8, animate: bool) -> Edge {
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

/// A design look drawn with its two colours (`ui::kit::looks` `solid`) until the
/// edge draws the design's looks.
const fn two(a: u32, b: u32) -> Look {
    const fn rgb(c: u32) -> Rgb {
        Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
    }
    Look {
        colours: [rgb(a), rgb(b)],
        ..preset(ColourSet::TwoColour, 100, 12, true, BarStyle::Glass)
    }
}

/// The presets: one table (their colours: `ui::theme::palette`).
pub const PRESETS: [(LookTheme, Look); 10] = {
    let thin = edge(EdgeStyle::Thin, 3, true);
    let still = edge(EdgeStyle::Thin, 2, false);
    // Today's look: the thin violet → cyan line with its sweep.
    let aurora = Look {
        overlay: thin,
        live: thin,
        ..preset(ColourSet::Aurora, 100, 12, false, BarStyle::Glass)
    };
    let mono = Look {
        bracket_style: BracketStyle::Frame,
        overlay: thin,
        live: thin,
        ..preset(ColourSet::Mono, 100, 10, false, BarStyle::Solid)
    };
    [
        (
            LookTheme::Spectrum,
            preset(ColourSet::Spectrum, 100, 12, true, BarStyle::Glass),
        ),
        (LookTheme::Aurora, aurora),
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
        (LookTheme::Mono, mono),
        (
            LookTheme::Calm,
            Look {
                overlay: still,
                live: still,
                snip: still,
                ..preset(ColourSet::Aurora, 0, 6, false, BarStyle::Glass)
            },
        ),
        (LookTheme::Horizon, two(0x38bdf8, 0x818cf8)),
        (LookTheme::Solar, two(0xfbbf24, 0xf59e0b)),
        (LookTheme::Hud, two(0xffffff, 0x7dd3fc)),
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

    pub(super) fn edge_mut(&mut self, mode: EdgeMode) -> &mut Edge {
        match mode {
            EdgeMode::Overlay => &mut self.overlay,
            EdgeMode::Live => &mut self.live,
            EdgeMode::Snip => &mut self.snip,
        }
    }
}

/// `[look.custom]`, field by field over `d`.
pub(super) fn read_look(t: &Table, d: Look, w: &mut Vec<String>) -> Look {
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
