//! The update card (`kit::panel_frame`, the design's "App update" card): a
//! release's notes after an update ("What's new in X") or for an update this
//! install can't apply itself ("X is available" + the release page). Links
//! open in the browser only when clicked. Pressing the heading drags it.
use iced::widget::{column, row};
use iced::{Center, Element, Size};

use crate::ui::kit::button::{Kind, button, cta_label, label};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::panel_frame::frame;
use crate::ui::kit::text::linked;
use crate::ui::kit::tokens::{Role, radius, space};
use crate::update::{NoteKind, NoteLine, parse_notes};

/// The card's window (logical px).
pub const SIZE: Size = Size::new(460.0, 440.0);
/// The corners the window is cut to (`.as-card`).
pub const RADIUS: f32 = radius::DRAWER;
/// Between the notes' lines, and between the call to action and "Later".
const LINE_GAP: f32 = space::GAP;
const ACTION_GAP: f32 = 16.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    title: String,
    lines: Vec<NoteLine>,
    pub url: String,
    /// Installed already ("What's new"), else to get by hand.
    installed: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The heading was pressed: the window follows the pointer.
    Drag,
    /// "Get started" / "Later": the card goes.
    Close,
    /// A link in the notes (opened only if it is one plain web link).
    Link(String),
    /// "Open release page" / "Release page".
    Page,
}

impl Card {
    /// After an update to `version`.
    pub fn installed(version: &str, notes: &str, url: &str) -> Card {
        let title = format!("What's new in OpenCircle {version}");
        Card::new(title, notes, url, true)
    }

    /// `version` is out, but this install updates by hand (deb, rpm, a failed swap).
    pub fn available(version: &str, notes: &str, url: &str) -> Card {
        let title = format!("OpenCircle {version} is available");
        Card::new(title, notes, url, false)
    }

    fn new(title: String, notes: &str, url: &str, installed: bool) -> Card {
        let mut lines = parse_notes(notes);
        if lines.is_empty() {
            lines = parse_notes("See the release page for what changed.");
        }
        Card {
            title,
            lines,
            url: url.to_string(),
            installed,
        }
    }
}

/// The card, corners of `radius` (`shape::radius(RADIUS)`: square where
/// windows can't be cut).
pub fn view(c: &Card, radius: f32) -> Element<'_, Msg> {
    let lines = c.lines.iter().map(|line| {
        let parts = (line.spans.iter()).map(|s| (s.text.as_str(), s.link.as_deref()));
        match line.kind {
            NoteKind::Heading => linked(parts, Role::ROW_TITLE, false, Msg::Link),
            NoteKind::Text => linked(parts, Role::NOTE, true, Msg::Link),
            NoteKind::Bullet => row![
                linked([("•", None)], Role::NOTE, true, Msg::Link),
                linked(parts, Role::NOTE, true, Msg::Link)
            ]
            .spacing(LINE_GAP)
            .into(),
        }
    });
    let (glyph, main, other) = match c.installed {
        true => (
            Icon::Check,
            ("Get started", Msg::Close),
            ("Release page", Msg::Page),
        ),
        false => (
            Icon::Download,
            ("Open release page", Msg::Page),
            ("Later", Msg::Close),
        ),
    };
    let actions = row![
        button(cta_label(main.0, None), Kind::Accent, Some(main.1)),
        button(label(other.0), Kind::Quiet, Some(other.1)),
    ]
    .spacing(ACTION_GAP)
    .align_y(Center);
    let notes = column(lines).spacing(LINE_GAP);
    frame(glyph, &c.title, notes, actions, radius, Msg::Drag)
}

/// What the notification toast says when an update is ready.
pub fn ready_toast(version: &str) -> String {
    format!("Update {version} available — installing quietly")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::{golden, shot};
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    const NOTES: &str = "## Fixed\n- [A crash](https://e.x/1) when the screen sleeps\n- Text selection on a second screen\n\nThanks to everyone who reported these.";

    #[test]
    fn the_card_shows_title_notes_and_its_buttons() {
        let c = Card::installed("9.9.9", NOTES, "https://e.x/r");
        let texts = shot::texts(view(&c, RADIUS));
        for want in [
            "What's new in OpenCircle 9.9.9",
            "Get started",
            "Release page",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let empty = Card::available("9.9.9", "", "https://e.x/r");
        assert_eq!(empty.lines.len(), 1, "never an empty card");
        let texts = shot::texts(view(&empty, RADIUS));
        for want in [
            "OpenCircle 9.9.9 is available",
            "Open release page",
            "Later",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
    }

    /// Clicking the buttons and the heading says what they do.
    #[test]
    fn its_buttons_close_open_the_page_and_drag() {
        let c = Card::available("9.9.9", NOTES, "https://e.x/r");
        let at = |x: f32, y: f32| shot::click(view(&c, RADIUS), SIZE, iced::Point::new(x, y));
        assert!(matches!(at(100.0, 34.0)[..], [Msg::Drag]));
        let bottom = SIZE.height - 22.0 - 22.0;
        assert!(matches!(at(150.0, bottom)[..], [Msg::Page]));
        assert!(matches!(at(SIZE.width - 40.0, bottom)[..], [Msg::Close]));
    }

    /// Both states in Sapphire and Daylight.
    #[test]
    fn every_update_card_matches_its_golden_image() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for (name, c) in [
                (
                    "available",
                    Card::available("0.1.2", NOTES, "https://e.x/r"),
                ),
                (
                    "installed",
                    Card::installed("0.1.2", NOTES, "https://e.x/r"),
                ),
            ] {
                let file = format!("update-card-{name}-{}", theme.id);
                failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                    view(&c, RADIUS)
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
