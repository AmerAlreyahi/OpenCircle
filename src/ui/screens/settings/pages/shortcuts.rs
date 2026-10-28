//! Shortcuts (`settings-window.js` "SHORTCUTS"): one row per shortcut (Open
//! OpenCircle, then each quick action whose Labs feature is on), each a
//! shortcut box of the one width. A click records; the row's line then says
//! how to cancel, and a key that can't be used says why under its name, in
//! red.
use iced::Element;
use iced::widget::column;
use iced::widget::tooltip::Position;

use super::super::state::{Message, Slot, State, key_labels};
use crate::ui::kit::setting_row;
use crate::ui::kit::shortcut_field::{self, shortcut_field};
use crate::ui::kit::text::{error, text};
use crate::ui::kit::tokens::Role;
use crate::ui::widgets::tip;

/// The page's note (`PAGES`).
pub const NOTE: &str = "Click any box to record keys";

pub fn view(state: &State) -> Element<'_, Message> {
    column(state.slots().map(|slot| row(state, slot))).into()
}

fn row(state: &State, slot: Slot) -> Element<'_, Message> {
    let (label, hint, name) = match slot {
        Slot::Capture => (
            "Open OpenCircle",
            "Starts a capture",
            "Open OpenCircle shortcut",
        ),
        Slot::Quick(i) => {
            let t = &state.quick_text[i];
            (t.label.as_str(), t.hint.as_str(), t.tip.as_str())
        }
    };
    let recording = state.recording == Some(slot);
    let problem = (state.shortcut_error.as_ref())
        .filter(|(s, _)| *s == slot)
        .map(|(_, e)| e.clone())
        // never claim a key the desktop doesn't use
        .or_else(|| (!recording).then(|| state.desktop_note(slot)).flatten());
    let field_state = match (recording, &problem) {
        (_, Some(_)) => shortcut_field::State::Conflict,
        (true, None) => shortcut_field::State::Recording,
        _ => shortcut_field::State::Rest,
    };
    let hint = match (recording, slot) {
        (false, _) => hint,
        (true, Slot::Capture) => "Esc cancels",
        (true, Slot::Quick(_)) => "Esc cancels · Delete clears",
    };
    let field = shortcut_field(
        key_labels(state.shortcut(slot)),
        field_state,
        "Press keys…",
        Message::Record(slot),
    );
    // the problem under the name, above the hint
    let mut name_block = column![text(Role::ROW_TITLE, label)];
    if let Some(p) = problem {
        name_block = name_block.push(error(Role::ROW_DESC, p));
    }
    setting_row::row(name_block.into(), hint, tip(field, name, Position::Top))
}
