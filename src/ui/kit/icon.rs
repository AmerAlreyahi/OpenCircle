//! The icon font (a Lucide subset, `assets/lucide/`) and its glyphs by name.
use iced::Font;
use iced::widget::{Text, text};

/// The icon font; registered with the others (`text::FONTS`).
pub const FONT_BYTES: &[u8] = include_bytes!("../../../assets/lucide/lucide-subset.ttf");
pub const FONT: Font = Font::with_name("lucide");

/// The kit's icons (each joins with its first screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// lucide "x".
    Close,
    /// lucide "download" (an update to get).
    Download,
    /// lucide "check" (done: an update installed).
    Check,
    /// lucide "minus", "square", "copy": a window's minimise, maximise and
    /// restore caption buttons.
    Minimise,
    Maximise,
    Restore,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 6] = [
        Icon::Close,
        Icon::Download,
        Icon::Check,
        Icon::Minimise,
        Icon::Maximise,
        Icon::Restore,
    ];

    pub const fn glyph(self) -> char {
        match self {
            Icon::Close => '\u{e1b1}',
            Icon::Download => '\u{e0b6}',
            Icon::Check => '\u{e070}',
            Icon::Minimise => '\u{e11f}',
            Icon::Maximise => '\u{e16a}',
            Icon::Restore => '\u{e0a2}',
        }
    }
}

/// `icon` at `px`.
pub fn icon<'a>(icon: Icon, px: f32) -> Text<'a> {
    text(icon.glyph()).font(FONT).size(px)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_icon_is_in_the_subset_font() {
        let script = include_str!("../../../assets/lucide/subset.sh");
        for icon in super::Icon::ALL {
            let code = format!("U+{:04X}", icon.glyph() as u32);
            assert!(script.contains(&code), "add {code} ({icon:?}) to subset.sh");
        }
    }
}
