//! Design tokens: the ONLY place colours live (DESIGN.md "Themes").
use iced::Color;

/// One theme's surface colours.
pub struct Palette {
    pub panel: Color,
    pub border: Color,
    pub text: Color,
    /// Secondary text: key hints, captions.
    pub muted: Color,
    pub shadow: Color,
    /// Quiet button under the cursor / pressed.
    pub hover: Color,
    pub pressed: Color,
}

const DARK: Palette = Palette {
    panel: Color::from_rgba8(22, 22, 34, 0.86),
    border: Color::from_rgba8(255, 255, 255, 0.09),
    text: Color::from_rgb8(0xec, 0xec, 0xf4),
    muted: Color::from_rgb8(0xa3, 0xa3, 0xb8),
    shadow: Color::from_rgba8(0, 0, 0, 0.35),
    hover: Color::from_rgba8(255, 255, 255, 0.08),
    pressed: Color::from_rgba8(255, 255, 255, 0.14),
};

const LIGHT: Palette = Palette {
    panel: Color::from_rgba8(255, 255, 255, 0.90),
    border: Color::from_rgba8(16, 16, 40, 0.10),
    text: Color::from_rgb8(0x1a, 0x1a, 0x24),
    muted: Color::from_rgb8(0x5b, 0x5b, 0x6e),
    shadow: Color::from_rgba8(16, 16, 40, 0.15),
    hover: Color::from_rgba8(16, 16, 40, 0.06),
    pressed: Color::from_rgba8(16, 16, 40, 0.11),
};

/// Selection stroke gradient (both themes): violet → cyan.
pub const STROKE_START: Color = Color::from_rgb8(0xa9, 0x9b, 0xff);
pub const STROKE_END: Color = Color::from_rgb8(0x5f, 0xe0, 0xf0);

/// Accent gradient (both themes): primary buttons, active tools.
pub const ACCENT_START: Color = Color::from_rgb8(0x8f, 0x80, 0xff);
pub const ACCENT_END: Color = Color::from_rgb8(0x4f, 0xc9, 0xe0);
/// Text and icons on the accent gradient.
pub const ON_ACCENT: Color = Color::from_rgb8(0x10, 0x10, 0x1c);

/// Dim over the frozen frame: whole frame before a selection, outside it after.
pub const DIM_IDLE: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.12);
pub const DIM_OUTSIDE: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.45);

/// The palette in use.
// ponytail: "System" is hard-wired to dark; read the OS theme + the Light/Dark/System
// setting once settings exist.
pub fn current() -> &'static Palette {
    palette(true)
}

fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

#[cfg(test)]
mod tests {
    #[test]
    fn light_and_dark_differ() {
        assert_ne!(super::palette(true).text, super::palette(false).text);
    }
}
