//! Privacy (`settings-window.js` "PRIVACY"): what leaves this PC, for each
//! feature, and where it goes, as a card with three columns; what is true
//! today (docs/privacy.md), Labs features only while on.
use iced::widget::{column, container};
use iced::{Element, Fill, Length};

use super::super::state::{Message, State};
use crate::labs::Feature;
use crate::shared::app_language::{S, tr};
use crate::shared::kit::card::card;
use crate::shared::kit::dir;
use crate::shared::kit::divider::divider;
use crate::shared::kit::text::{sub, text};
use crate::shared::kit::tokens::{Role, space};
use crate::t;

/// (feature, what leaves your PC, where it goes). Ask AI with no key sends
/// the circled part too (owner, 2026-11-08): the browser posts the crop.
const ROWS: [(S, S, S); 7] = [
    (S::p_r1a, S::p_r1b, S::p_r1c),
    (S::p_r2a, S::p_r3b, S::p_r2c),
    (S::p_r3a, S::p_r3b, S::p_r3c),
    (S::p_r4a, S::p_r4b, S::p_r4c),
    (S::p_r5a, S::p_nothing, S::p_here),
    (S::p_r6a, S::p_nothing, S::p_here),
    (S::p_r7a, S::p_r7b, S::p_r7c),
];

const SONG: (S, S, S) = (S::p_song_a, S::p_song_b, S::p_song_c);

pub fn view(state: &State) -> Element<'_, Message> {
    let mut lines = ROWS.to_vec();
    if state.draft.labs.has(Feature::Song) {
        lines.insert(lines.len() - 1, SONG);
    }
    let cells = |a: Element<'static, Message>, b, c| {
        container(
            crate::dir_row![
                container(a)
                    .width(Length::FillPortion(3))
                    .align_x(dir::start()),
                container(b)
                    .width(Length::FillPortion(4))
                    .align_x(dir::start()),
                container(c)
                    .width(Length::FillPortion(4))
                    .align_x(dir::start()),
            ]
            .spacing(space::GAP),
        )
        .padding([space::ROW_Y, 0.0])
    };
    let head = |s| sub(Role::CARD_TITLE, s).into();
    let mut table = column![cells(head(t!(p_h1)), head(t!(p_h2)), head(t!(p_h3)))];
    for (feature, leaves, goes) in lines {
        table = table.push(divider()).push(cells(
            text(Role::CONTROL, tr(feature)).into(),
            sub(Role::NOTE, tr(leaves)).into(),
            sub(Role::NOTE, tr(goes)).into(),
        ));
    }
    card(table.width(Fill))
}
