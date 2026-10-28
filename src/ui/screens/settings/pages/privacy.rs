//! Privacy (`settings-window.js` "PRIVACY"): what leaves this PC, for each
//! feature, and where it goes, as a card with three columns; what is true
//! today (docs/privacy.md), Labs features only while on.
use iced::widget::{column, container, row};
use iced::{Element, Fill, Length};

use super::super::state::{Message, State};
use crate::labs::Feature;
use crate::ui::kit::card::card;
use crate::ui::kit::divider::divider;
use crate::ui::kit::text::{sub, text};
use crate::ui::kit::tokens::{Role, space};

/// The page's note (`PAGES`).
pub const NOTE: &str =
    "Nothing leaves your PC until you choose an action. No telemetry or analytics.";

/// (feature, what leaves your PC, where it goes)
const ROWS: [(&str, &str, &str); 7] = [
    (
        "Search in browser",
        "The selected area, or only the text you selected",
        "Google Lens or Search, in your default browser",
    ),
    (
        "Ask AI, no key",
        "Your question and the text read from the selection",
        "Google AI Mode, in your default browser",
    ),
    (
        "Ask AI, your key",
        "Your question and the selected area",
        "Your AI provider, or your own server",
    ),
    (
        "Translate with AI",
        "The text read from the screen",
        "Your AI provider",
    ),
    ("Text reading (OCR)", "Nothing", "Runs on this PC"),
    ("Offline translate", "Nothing", "Runs on this PC"),
    (
        "Updates",
        "Only the request for the latest version (when on)",
        "GitHub Releases",
    ),
];

const SONG: (&str, &str, &str) = (
    "Song search (Labs)",
    "A ~10-second clip, only when you press the music button",
    "AudD (with your token)",
);

pub fn view(state: &State) -> Element<'_, Message> {
    let mut lines = ROWS.to_vec();
    if state.draft.labs.has(Feature::Song) {
        lines.insert(lines.len() - 1, SONG);
    }
    let cells = |a: Element<'static, Message>, b, c| {
        container(
            row![
                container(a).width(Length::FillPortion(3)),
                container(b).width(Length::FillPortion(4)),
                container(c).width(Length::FillPortion(4)),
            ]
            .spacing(space::GAP),
        )
        .padding([space::ROW_Y, 0.0])
    };
    let head = |s| sub(Role::CARD_TITLE, s).into();
    let mut table = column![cells(
        head("Feature"),
        head("What leaves your PC"),
        head("Where it goes")
    )];
    for (feature, leaves, goes) in lines {
        table = table.push(divider()).push(cells(
            text(Role::CONTROL, feature).into(),
            sub(Role::NOTE, leaves).into(),
            sub(Role::NOTE, goes).into(),
        ));
    }
    card(table.width(Fill))
}
