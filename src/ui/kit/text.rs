//! The UI's fonts and text styles: the design's font stack, its mono font,
//! and `text(Role, s)`, a line of text in a type role (`tokens::Role`) in the
//! theme's text colour.
use iced::Font;
use iced::font::Weight;
use iced::widget::text::{IntoFragment, Text};

use super::themes;
use super::tokens::Role;

/// Every font the UI registers at start (`ui::run`; the headless renders too):
/// the icons, Inter (the UI font where Segoe UI is missing), Noto Sans
/// Arabic (Arabic where the UI font has none) and the design's mono font
/// (`MONO`: a Latin subset of JetBrains Mono, weights 400-600, plus ▴ ▾ ✓ →).
/// OFL licences in `assets/fonts/`.
pub const FONTS: [&[u8]; 4] = [
    super::icon::FONT_BYTES,
    include_bytes!("../../../assets/fonts/Inter.ttf"),
    include_bytes!("../../../assets/fonts/NotoSansArabic.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono.ttf"),
];

/// The design's `.mono` text (file names, versions, sizes, paths).
pub const MONO: Font = Font::with_name("JetBrains Mono");

/// The UI font, the design's stack: Segoe UI Variable Text, else Segoe UI
/// (Windows), else the bundled Inter. Picked once, from the fonts on this PC.
pub fn ui_font() -> Font {
    Font::with_name(ui_font_name())
}

pub fn ui_font_name() -> &'static str {
    static FONT: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system");
        let db = fonts.raw().db();
        let found = |name: &&str| {
            db.faces()
                .any(|f| f.families.iter().any(|(family, _)| family == name))
        };
        ["Segoe UI Variable Text", "Segoe UI"]
            .into_iter()
            .find(found)
            .unwrap_or("Inter")
    })
}

/// The UI font at a weight (the design's 500 / 600).
pub fn weight(weight: Weight) -> Font {
    Font {
        weight,
        ..ui_font()
    }
}

/// The mono font at a weight.
pub fn mono(weight: Weight) -> Font {
    Font { weight, ..MONO }
}

/// The font of `role`.
pub fn font(role: Role) -> Font {
    match role.mono {
        true => mono(role.weight),
        false => weight(role.weight),
    }
}

/// `s` in `role`: its size and font, in the theme's text colour (a section
/// head upper case and muted).
pub fn text<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    let c = &themes::current().c;
    let (line, colour) = match role == Role::SECTION {
        true => (s.into_fragment().to_uppercase().into(), c.text_muted),
        false => (s.into_fragment(), c.text),
    };
    iced::widget::text(line)
        .size(role.size)
        .font(font(role))
        .color(colour)
}
