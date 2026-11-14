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
    /// lucide "arrow-up" and "square": a question's Send and Stop.
    Send,
    Stop,
    /// lucide "type" (text recognition) and "languages" (translation): the
    /// model download card's emblems.
    Text,
    Translate,
    /// lucide "corner-down-left" (the AI field's "AI ↵" badge) and "star"
    /// (About's Star button): bundled, so no system font draws them.
    Enter,
    Star,
    /// lucide "screen-share": the AI field's Include screen.
    Screen,
    /// lucide "pin" (the AI chat stays open), "pin-off" (a pinned chat's
    /// unpin) and "arrow-left" (a drawer's back).
    Pin,
    Unpin,
    Back,
    /// lucide "grip-vertical": a drag-out handle (the Drag chip's dots).
    Grip,
    /// lucide "image" (Snip's size chip: the picture) and "ellipsis" (More).
    Picture,
    More,
    /// lucide "search": a list's search field.
    Search,
    /// lucide "chevron-down" / "chevron-up": a list's opener, closed / open;
    /// "arrow-right": the language pill's source -> target.
    ChevronDown,
    ChevronUp,
    Forward,
    /// lucide "rotate-ccw" (Restart now), "shield" (Windows asks for permission)
    /// and "external-link" (a page in the browser: the release notes, Get it).
    Restart,
    Shield,
    External,
    /// lucide "circle-alert" (the AI error card).
    Alert,
    /// lucide "book-a" (the translate pill's glossary).
    Glossary,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 28] = [
        Icon::Close,
        Icon::Download,
        Icon::Check,
        Icon::Minimise,
        Icon::Maximise,
        Icon::Restore,
        Icon::Send,
        Icon::Stop,
        Icon::Text,
        Icon::Translate,
        Icon::Enter,
        Icon::Star,
        Icon::Screen,
        Icon::Pin,
        Icon::Unpin,
        Icon::Back,
        Icon::Grip,
        Icon::Picture,
        Icon::More,
        Icon::Search,
        Icon::ChevronDown,
        Icon::ChevronUp,
        Icon::Forward,
        Icon::Restart,
        Icon::Shield,
        Icon::External,
        Icon::Alert,
        Icon::Glossary,
    ];

    /// A directional icon as the layout direction reads it: Back and Forward
    /// swap in RTL; icons that don't point stay as they are.
    pub fn directed(self) -> Icon {
        match (self, super::dir::rtl()) {
            (Icon::Back, true) => Icon::Forward,
            (Icon::Forward, true) => Icon::Back,
            (icon, _) => icon,
        }
    }

    pub const fn glyph(self) -> char {
        match self {
            Icon::Close => '\u{e1b1}',
            Icon::Download => '\u{e0b6}',
            Icon::Check => '\u{e070}',
            Icon::Minimise => '\u{e11f}',
            Icon::Maximise => '\u{e16a}',
            Icon::Restore => '\u{e0a2}',
            Icon::Send => '\u{e04e}',
            Icon::Stop => '\u{e16a}',
            Icon::Text => '\u{e197}',
            Icon::Translate => '\u{e103}',
            Icon::Enter => '\u{e0a5}',
            Icon::Star => '\u{e179}',
            Icon::Screen => '\u{e152}',
            Icon::Pin => '\u{e258}',
            Icon::Unpin => '\u{e2b5}',
            Icon::Back => '\u{e04c}',
            Icon::Grip => '\u{e0ee}',
            Icon::Picture => '\u{e0f9}',
            Icon::More => '\u{e0ba}',
            Icon::Search => '\u{e154}',
            Icon::ChevronDown => '\u{e071}',
            Icon::ChevronUp => '\u{e074}',
            Icon::Forward => '\u{e04d}',
            Icon::Restart => '\u{e14b}',
            Icon::Shield => '\u{e15b}',
            Icon::External => '\u{e0bd}',
            Icon::Alert => '\u{e07b}',
            Icon::Glossary => '\u{e548}',
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
