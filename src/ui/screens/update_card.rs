//! What's new (`kit::panel_frame`, the design's board "Update" u4 / u5; owner,
//! 2026-11-10): "What's new in OpenCircle <v>", the release's summary, at most
//! six Highlights rendered (bold labels, keys, short links), Fixes and Upgrade
//! notes folded with their counts, Done and "Full release notes". Shown after
//! Restart now ("Updated just now"), and from Settings > What's new before an
//! update installs. Links open in the browser only when clicked. Pressing the
//! heading drags it.
use iced::widget::{column, row};
use iced::{Center, Element, Size};

use crate::ui::kit::button::{FOLD_H, Kind, button, fold, label, regular};
use crate::ui::kit::icon::{Icon, icon};
use crate::ui::kit::panel_frame::{PADDING, frame, height};
use crate::ui::kit::text::{Ink, LINKED_LINE, font, lines, muted, styled, width};
use crate::ui::kit::tokens::{Role, radius, size, space};
use crate::update::{NoteLine, NoteStyle, Notes, parse_notes, plain_note};

/// The card's width (the design: 520) and its body's (padding 22).
pub const WIDTH: f32 = 520.0;
const BODY: f32 = WIDTH - 2.0 * PADDING;
/// The tallest the card grows (logical px); longer notes scroll.
const MAX_HEIGHT: f32 = 600.0;
/// The corners the window is cut to (`.as-card`).
pub const RADIUS: f32 = radius::DRAWER;
/// Between the blocks, and between a bullet and its text.
const GAP: f32 = 10.0;
const BULLET_GAP: f32 = 8.0;
const ACTION_GAP: f32 = 12.0;
/// The summary and the highlights; a folded section's lines.
const TEXT: Role = Role::CHAT;
const SMALL: Role = Role::NOTE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    title: String,
    /// "Updated just now" (after an update), none before one.
    installed: bool,
    notes: Notes,
    pub url: String,
    /// Fixes, Upgrade notes: opened.
    open: [bool; 2],
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The heading was pressed: the window follows the pointer.
    Drag,
    /// Done: the card goes (nothing else opens).
    Close,
    /// A link in the notes (opened only if it is one plain web link).
    Link(String),
    /// "Full release notes".
    Page,
    /// A folded section (0 Fixes, 1 Upgrade notes) opens or closes.
    Fold(usize),
}

impl Card {
    /// After an update to `version`.
    pub fn installed(version: &str, notes: &str, url: &str) -> Card {
        Card::new(version, notes, url, true)
    }

    /// `version`'s notes before it installs (Settings > What's new).
    pub fn preview(version: &str, notes: &str, url: &str) -> Card {
        Card::new(version, notes, url, false)
    }

    fn new(version: &str, notes: &str, url: &str, installed: bool) -> Card {
        Card {
            title: format!("What's new in OpenCircle {version}"),
            installed,
            notes: parse_notes(notes),
            url: url.to_string(),
            open: [false; 2],
        }
    }

    pub fn fold(&mut self, i: usize) {
        if let Some(open) = self.open.get_mut(i) {
            *open = !*open;
        }
    }

    /// The folded sections: title, lines, opened.
    fn folds(&self) -> impl Iterator<Item = (usize, &'static str, &[NoteLine], bool)> {
        [
            ("Fixes", &self.notes.fixes),
            ("Upgrade notes", &self.notes.upgrade),
        ]
        .into_iter()
        .enumerate()
        .filter(|(_, (_, l))| !l.is_empty())
        .map(|(i, (t, l))| (i, t, l.as_slice(), self.open[i]))
    }

    /// The card's window (logical px): as tall as what it shows, up to `MAX_HEIGHT`.
    pub fn size(&self) -> Size {
        let tall = |line: &NoteLine, role: Role, wrap: f32| {
            let n = lines(&plain_note(line), role.size, font(role), wrap).max(1);
            n as f32 * role.size * LINKED_LINE
        };
        let bullet = width("•", TEXT.size, font(TEXT)) + BULLET_GAP;
        let mut blocks = Vec::new();
        if !self.notes.summary.is_empty() {
            blocks.push(tall(&self.notes.summary, TEXT, BODY));
        }
        if !self.notes.highlights.is_empty() {
            blocks.push(Role::SECTION.size * LINKED_LINE);
        }
        blocks.extend((self.notes.highlights.iter()).map(|l| tall(l, TEXT, BODY - bullet)));
        if self.notes.more > 0 {
            blocks.push(SMALL.size * LINKED_LINE);
        }
        for (_, _, items, open) in self.folds() {
            blocks.push(FOLD_H);
            if open {
                blocks.extend(items.iter().map(|l| tall(l, SMALL, BODY - 18.0 - bullet)));
            }
        }
        let body = blocks.iter().sum::<f32>() + GAP * blocks.len().saturating_sub(1) as f32;
        Size::new(WIDTH, height(body, MAX_HEIGHT))
    }
}

/// A line's runs for `kit::text::styled`.
fn runs(line: &NoteLine) -> impl Iterator<Item = (&str, Ink, Option<&str>)> {
    line.iter().map(|s| {
        let ink = match s.style {
            NoteStyle::Plain => Ink::Plain,
            NoteStyle::Bold => Ink::Strong,
            NoteStyle::Code => Ink::Key,
        };
        (s.text.as_str(), ink, s.link.as_deref())
    })
}

fn bullet<'a>(line: &'a NoteLine, role: Role, mark: Ink) -> Element<'a, Msg> {
    row![
        styled([("•", mark, None)], role, Msg::Link),
        styled(runs(line), role, Msg::Link)
    ]
    .spacing(BULLET_GAP)
    .into()
}

/// The card, corners of `radius` (`shape::radius(RADIUS)`: square where
/// windows can't be cut).
pub fn view(c: &Card, radius: f32) -> Element<'_, Msg> {
    let n = &c.notes;
    let mut body = column![].spacing(GAP);
    if !n.summary.is_empty() {
        body = body.push(styled(runs(&n.summary), TEXT, Msg::Link));
    }
    if !n.highlights.is_empty() {
        body = body.push(muted(Role::SECTION, "HIGHLIGHTS"));
    }
    for line in &n.highlights {
        body = body.push(bullet(line, TEXT, Ink::Strong));
    }
    if n.more > 0 {
        let more = format!("{} more in the full release notes", n.more);
        body = body.push(muted(SMALL, more));
    }
    for (i, title, items, open) in c.folds() {
        body = body.push(fold(title, items.len(), open, Msg::Fold(i)));
        if open {
            let items = column(items.iter().map(|l| bullet(l, SMALL, Ink::Plain)))
                .spacing(GAP)
                .padding(iced::Padding::ZERO.left(18.0));
            body = body.push(items);
        }
    }
    let full = row![
        label("Full release notes"),
        icon(Icon::External, size::ICON_SMALL)
    ]
    .spacing(space::GAP)
    .align_y(Center);
    let actions = row![
        regular("Done", true, Some(Msg::Close)),
        button(full, Kind::Quiet, Some(Msg::Page)),
    ]
    .spacing(ACTION_GAP)
    .align_y(Center);
    let (glyph, aside) = match c.installed {
        true => (Icon::Check, Some("Updated just now")),
        false => (Icon::Download, None),
    };
    frame(glyph, &c.title, aside, body, actions, radius, Msg::Drag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::{golden, shot};
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    /// v0.1.4's shape: a bold summary, more highlights than shown, Fixes,
    /// Install, Upgrade notes, Known issues.
    const NOTES: &str = "**OpenCircle v0.1.5: Ask AI sends what you circled, and every widget works on every screen.**\n\n## Highlights\n- **Ask AI sends the circled picture.** Your question and the picture open together in your browser.\n- **Snip edges snap.** Areas land on window edges; hold `Shift` to turn it off.\n- **Copy only when you ask:** `Ctrl+C` or Copy; nothing doubles in clipboard history.\n- **Finger and pen** work wherever the mouse does.\n- **Lighter translation:** about a third less memory per language pair.\n- **Downloads resume** where they stopped.\n- **Less memory** after use.\n\n## Fixes\n- Touch screen and pen did nothing (https://github.com/AmerAlreyahi/OpenCircle/issues/153).\n- The Lens page opened in Edge, not your default browser (https://github.com/AmerAlreyahi/OpenCircle/issues/155).\n- A blur stroke that went too far could not be pulled back (https://github.com/AmerAlreyahi/OpenCircle/issues/154).\n\n## Install\n- Verify your download against `SHA256SUMS.txt`.\n\n## Upgrade notes\n- The settings file moves to schema 11 with a backup.\n- A \"Copy when picked\" that was on moves to off once.\n\n## Known issues\n- Linux (Wayland): no window picking.";

    /// Summary, six highlights and their rest counted, the folds with their
    /// counts, Done and Full release notes; no Install / Known issues and no
    /// "Get started". Before: every line of the notes and "Get started".
    #[test]
    fn whats_new_shows_the_summary_highlights_and_folds() {
        let c = Card::installed("0.1.5", NOTES, "https://e.x/r");
        let texts = shot::texts(view(&c, RADIUS));
        let all = texts.join("\n");
        for want in [
            "What's new in OpenCircle 0.1.5",
            "Updated just now",
            "HIGHLIGHTS",
            "1 more in the full release notes",
            "Fixes",
            "3",
            "Upgrade notes",
            "Done",
            "Full release notes",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        // the notes' runs (rich text): the summary and six highlights only
        let shown: String = std::iter::once(&c.notes.summary)
            .chain(&c.notes.highlights)
            .map(plain_note)
            .collect();
        assert!(
            shown.starts_with("Ask AI sends what you circled"),
            "{shown}"
        );
        assert_eq!(c.notes.highlights.len(), 6);
        for gone in [
            "Get started",
            "SHA256SUMS",
            "window picking",
            "Touch screen",
        ] {
            assert!(
                !all.contains(gone) && !shown.contains(gone),
                "{gone}: {texts:?}"
            );
        }
        let preview = Card::preview("0.1.5", NOTES, "https://e.x/r");
        assert!(
            !shot::texts(view(&preview, RADIUS))
                .iter()
                .any(|t| t == "Updated just now")
        );
    }

    /// A fold opens in place (its lines show, the card grows) and closes again;
    /// Done closes the card, the link opens the full notes.
    #[test]
    fn a_fold_opens_in_place_and_done_closes() {
        let mut c = Card::installed("0.1.5", NOTES, "https://e.x/r");
        let closed = c.size();
        c.fold(0);
        assert_eq!(
            c.folds().map(|(_, _, _, open)| open).collect::<Vec<_>>(),
            [true, false]
        );
        let fix = plain_note(&c.notes.fixes[0]);
        assert_eq!(
            fix, "Touch screen and pen did nothing (#153).",
            "a short label"
        );
        assert_eq!(c.size().height, MAX_HEIGHT, "grown to its most: {closed:?}");
        c.fold(0);
        assert_eq!(c.size(), closed);
        let size = c.size();
        let at = |x: f32, y: f32| shot::click(view(&c, RADIUS), size, iced::Point::new(x, y));
        let bottom = size.height - 22.0 - 16.0;
        assert!(matches!(at(40.0, bottom)[..], [Msg::Close]));
        assert!(matches!(at(150.0, bottom)[..], [Msg::Page]));
        assert!(matches!(at(100.0, 34.0)[..], [Msg::Drag]));
    }

    #[test]
    fn empty_notes_still_say_something() {
        let c = Card::installed("9.9.9", "", "https://e.x/r");
        assert!(plain_note(&c.notes.summary).contains("release page"));
        assert!(c.size().height < 200.0, "{:?}", c.size());
    }

    /// Short and long (a fold open), Sapphire and Daylight.
    #[test]
    fn every_whats_new_card_matches_its_golden_image() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let mut open = Card::installed("0.1.5", NOTES, "https://e.x/r");
            open.fold(0);
            for (name, c) in [
                (
                    "whats-new",
                    Card::installed("0.1.5", NOTES, "https://e.x/r"),
                ),
                ("whats-new-fixes", open),
            ] {
                let file = format!("update-card-{name}-{}", theme.id);
                failed.extend(golden::check("screens", &file, c.size(), &[1.0], || {
                    view(&c, RADIUS)
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
