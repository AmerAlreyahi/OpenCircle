//! The update card: a release's notes in the shared floating card
//! (`widgets::card::frame`), after an update ("What's new in X") or for an
//! update this install can't apply itself ("X is available" + the release page).
//! Links open in the browser only when clicked.

use iced::widget::{column, row};
use iced::{Element, Rectangle, Size};

use super::widgets::{self, card};
use crate::update::{NoteKind, NoteLine, parse_notes};

/// The card's window (logical px).
pub const SIZE: Size = Size::new(460.0, 440.0);
const DOWNLOAD: char = '\u{e0b6}'; // lucide "download"

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    title: String,
    lines: Vec<NoteLine>,
    pub url: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Card(card::Event),
    /// A link in the notes (opened only if it is one plain web link).
    Link(String),
    /// "Open release page".
    Page,
}

impl Card {
    /// After an update to `version`.
    pub fn installed(version: &str, notes: &str, url: &str) -> Card {
        Card::new(format!("What's new in OpenCircle {version}"), notes, url)
    }

    /// `version` is out, but this install updates by hand (deb, rpm, a failed swap).
    pub fn available(version: &str, notes: &str, url: &str) -> Card {
        Card::new(format!("OpenCircle {version} is available"), notes, url)
    }

    fn new(title: String, notes: &str, url: &str) -> Card {
        let mut lines = parse_notes(notes);
        if lines.is_empty() {
            lines = parse_notes("See the release page for what changed.");
        }
        Card {
            title,
            lines,
            url: url.to_string(),
        }
    }
}

pub fn view(c: &Card) -> Element<'_, Msg> {
    let lines = c.lines.iter().map(|line| {
        let parts = line
            .spans
            .iter()
            .map(|s| (s.text.as_str(), s.link.as_deref()));
        match line.kind {
            NoteKind::Heading => widgets::linked(parts, 16.0, Msg::Link),
            NoteKind::Text => widgets::linked(parts, 14.0, Msg::Link),
            NoteKind::Bullet => row![widgets::body("•"), widgets::linked(parts, 14.0, Msg::Link)]
                .spacing(8)
                .into(),
        }
    });
    let body = widgets::scroll(column(lines).spacing(8).padding([0, 8]));
    let footer = row![widgets::chip("Open release page", Msg::Page)].into();
    let at = Rectangle::new(iced::Point::ORIGIN, SIZE);
    card::frame(
        card::Header {
            icon: DOWNLOAD,
            title: &c.title,
            chip: None,
        },
        body,
        Some(footer),
        at,
        SIZE,
        false,
        Msg::Card,
    )
}

/// What the notification toast says when an update is ready.
pub fn ready_toast(version: &str) -> String {
    format!("Update {version} available — installing quietly")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_shows_title_notes_and_the_page_button() {
        let c = Card::installed(
            "9.9.9",
            "## Fixed\n- [crash](https://e.x/1) on start",
            "https://e.x/r",
        );
        let texts = crate::ui::widgets::shot::texts(view(&c));
        for want in ["What's new in OpenCircle 9.9.9", "•", "Open release page"] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let empty = Card::available("9.9.9", "", "https://e.x/r");
        assert_eq!(empty.lines.len(), 1, "never an empty card");
    }
}
