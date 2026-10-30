//! The update card (`kit::panel_frame`, the design's "App update" card): a
//! release's notes after an update ("What's new in X") or for an update this
//! install can't apply itself ("X is available" + the release page), or one
//! waiting for the user ("X is ready to install": Install now / Later). Links
//! open in the browser only when clicked. Pressing the heading drags it.
use iced::widget::{column, row};
use iced::{Center, Element, Size};

use crate::ui::kit::button::{Kind, button, cta_label, label};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::panel_frame::{BODY_WIDTH, WIDTH, frame, height};
use crate::ui::kit::text::{LINKED_LINE, font, lines, linked, width};
use crate::ui::kit::tokens::{Role, radius, space};
use crate::update::{NoteKind, NoteLine, parse_notes};

/// The tallest the card grows (logical px); longer notes scroll.
const MAX_HEIGHT: f32 = 440.0;
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
    stage: Stage,
}

/// What the card is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Installed already ("What's new").
    Installed,
    /// Downloaded and verified, waiting for the user (a copy the setup installed:
    /// its install asks Windows for admin rights, so never on its own).
    Ready,
    /// To get by hand.
    Available,
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
    /// "Install now" (`Card::ready`): the App runs the waiting update.
    Install,
}

impl Card {
    /// After an update to `version`.
    pub fn installed(version: &str, notes: &str, url: &str) -> Card {
        let title = format!("What's new in OpenCircle {version}");
        Card::new(title, notes, url, Stage::Installed)
    }

    /// `version` is downloaded and verified; it installs when the user says so.
    pub fn ready(version: &str, notes: &str, url: &str) -> Card {
        let title = format!("OpenCircle {version} is ready to install");
        Card::new(title, notes, url, Stage::Ready)
    }

    /// `version` is out, but this install updates by hand (deb, rpm, a failed swap).
    pub fn available(version: &str, notes: &str, url: &str) -> Card {
        let title = format!("OpenCircle {version} is available");
        Card::new(title, notes, url, Stage::Available)
    }

    fn new(title: String, notes: &str, url: &str, stage: Stage) -> Card {
        let mut lines = parse_notes(notes);
        if lines.is_empty() {
            lines = parse_notes("See the release page for what changed.");
        }
        Card {
            title,
            lines,
            url: url.to_string(),
            stage,
        }
    }

    /// The card's window (logical px): as tall as its notes, up to `MAX_HEIGHT`.
    pub fn size(&self) -> Size {
        let bullet = width("•", Role::NOTE.size, font(Role::NOTE)) + LINE_GAP;
        let notes: f32 = (self.lines.iter())
            .map(|line| {
                let (role, wrap) = match line.kind {
                    NoteKind::Heading => (Role::ROW_TITLE, BODY_WIDTH),
                    NoteKind::Text => (Role::NOTE, BODY_WIDTH),
                    NoteKind::Bullet => (Role::NOTE, BODY_WIDTH - bullet),
                };
                let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                let n = lines(&text, role.size, font(role), wrap).max(1);
                n as f32 * role.size * LINKED_LINE + LINE_GAP
            })
            .sum();
        Size::new(WIDTH, height(notes, MAX_HEIGHT))
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
    let (glyph, main, other) = match c.stage {
        Stage::Installed => (
            Icon::Check,
            ("Get started", Msg::Close),
            ("Release page", Msg::Page),
        ),
        Stage::Ready => (
            Icon::Download,
            ("Install now", Msg::Install),
            ("Later", Msg::Close),
        ),
        Stage::Available => (
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
        let size = c.size();
        let at = |x: f32, y: f32| shot::click(view(&c, RADIUS), size, iced::Point::new(x, y));
        assert!(matches!(at(100.0, 34.0)[..], [Msg::Drag]));
        let bottom = size.height - 22.0 - 22.0;
        assert!(matches!(at(150.0, bottom)[..], [Msg::Page]));
        assert!(matches!(at(size.width - 40.0, bottom)[..], [Msg::Close]));
    }

    /// An installed copy's update waits for the user: "Install now" asks for it.
    #[test]
    fn the_ready_card_installs_only_when_asked() {
        let c = Card::ready("9.9.9", NOTES, "https://e.x/r");
        let texts = shot::texts(view(&c, RADIUS));
        for want in [
            "OpenCircle 9.9.9 is ready to install",
            "Install now",
            "Later",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let size = c.size();
        let at = |x: f32, y: f32| shot::click(view(&c, RADIUS), size, iced::Point::new(x, y));
        let bottom = size.height - 22.0 - 22.0;
        assert!(matches!(at(100.0, bottom)[..], [Msg::Install]));
        assert!(matches!(at(size.width - 40.0, bottom)[..], [Msg::Close]));
    }

    /// The card is as tall as its notes (no empty space under them): every
    /// note line adds its height; long notes stop at `MAX_HEIGHT` and scroll.
    #[test]
    fn the_card_grows_with_its_notes() {
        let short = Card::available("9.9.9", NOTES, "https://e.x/r").size();
        let one = Card::available("9.9.9", "One line.", "https://e.x/r").size();
        assert_eq!(short.width, 480.0);
        // header 24 + 14, actions 18 + 44, padding 2 x 22: 144 px around the notes
        assert!(one.height < 144.0 + 30.0, "{one:?}");
        // three more lines (heading, two bullets) of about 20-21 px + gaps
        let grew = short.height - one.height;
        assert!((70.0..100.0).contains(&grew), "{short:?} {one:?}");
        let long =
            "- a note that is long enough to wrap onto a second line in the card\n".repeat(30);
        let long = Card::available("9.9.9", &long, "https://e.x/r").size();
        assert_eq!(long.height, MAX_HEIGHT);
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
                // the golden's height is the content's: a card taller than its notes fails
                failed.extend(golden::check("screens", &file, c.size(), &[1.0], || {
                    view(&c, RADIUS)
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
