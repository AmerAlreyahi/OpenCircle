//! Design tokens: the ONLY place colours live (DESIGN.md "Themes").
use std::cell::Cell;

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

const DARK: Palette = Palette {
    panel: Color::from_rgba8(22, 22, 34, 0.86),
    border: Color::from_rgba8(255, 255, 255, 0.09),
    text: Color::from_rgb8(0xec, 0xec, 0xf4),
    muted: Color::from_rgb8(0xa3, 0xa3, 0xb8),
    shadow: Color::from_rgba8(0, 0, 0, 0.35),
    hover: Color::from_rgba8(255, 255, 255, 0.08),
    pressed: Color::from_rgba8(255, 255, 255, 0.14),
    window: Color::from_rgb8(0x12, 0x13, 0x1f),
    field: Color::from_rgba8(255, 255, 255, 0.06),
    field_focus: Color::from_rgba8(255, 255, 255, 0.12),
    keycap: Color::from_rgba8(255, 255, 255, 0.10),
    danger: Color::from_rgb8(0xff, 0x8a, 0x8a),
};

const LIGHT: Palette = Palette {
    panel: Color::from_rgba8(255, 255, 255, 0.90),
    border: Color::from_rgba8(16, 16, 40, 0.10),
    text: Color::from_rgb8(0x1a, 0x1a, 0x24),
    muted: Color::from_rgb8(0x5b, 0x5b, 0x6e),
    shadow: Color::from_rgba8(16, 16, 40, 0.15),
    hover: Color::from_rgba8(16, 16, 40, 0.06),
    pressed: Color::from_rgba8(16, 16, 40, 0.11),
    window: Color::from_rgb8(0xf6, 0xf6, 0xfa),
    field: Color::from_rgba8(16, 16, 40, 0.05),
    field_focus: Color::from_rgba8(16, 16, 40, 0.10),
    keycap: Color::from_rgba8(255, 255, 255, 0.95),
    danger: Color::from_rgb8(0xc6, 0x28, 0x28),
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
    palette(DARK_MODE.with(Cell::get))
}

thread_local! {
    /// The UI thread's resolved theme (tests render Light and Dark side by side).
    static DARK_MODE: Cell<bool> = const { Cell::new(true) };
}

/// Switch the palette `current()` returns, from the Settings choice.
// ponytail: System = dark until the OS theme is read.
#[allow(dead_code)] // called by the Settings/Onboarding screens once ui/mod.rs opens them
pub fn apply(theme: crate::config::Theme) {
    DARK_MODE.with(|d| d.set(theme != crate::config::Theme::Light));
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
