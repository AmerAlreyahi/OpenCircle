//! Design tokens: the ONLY place colours live (DESIGN.md "Themes").
use std::cell::Cell;

use iced::Color;

use crate::config::Theme;

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
/// Selected text in inputs (both themes): the accent, see-through.
pub const SELECTION: Color = Color::from_rgba8(0x8f, 0x80, 0xff, 0.35);
/// Text and icons on the accent gradient.
pub const ON_ACCENT: Color = Color::from_rgb8(0x10, 0x10, 0x1c);

/// Dim over the frozen frame: whole frame before a selection, outside it after.
pub const DIM_IDLE: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.12);
pub const DIM_OUTSIDE: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.45);

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

#[cfg(test)]
mod tests {
    use super::*;
    use iced::theme::Mode;

    #[test]
    fn light_and_dark_differ() {
        assert_ne!(DARK.text, LIGHT.text);
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
