//! Shortcuts (`settings-window.js` "SHORTCUTS"): one row per shortcut (Open
//! OpenCircle, then each quick action whose Labs feature is on), each a
//! shortcut box of the one width. A click records; the row's line then says
//! how to cancel, and a key that can't be used says why under its name, in
//! red.
use crate::t;
use iced::Element;
use iced::widget::column;
use iced::widget::tooltip::Position;

use super::super::state::{Message, Slot, State, key_labels};
use crate::ui::kit::button::Kind as Button;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::shortcut_field::{self, shortcut_field};
use crate::ui::kit::text::{error, text};
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::tooltip::tip;

pub fn view(state: &State) -> Element<'_, Message> {
    let rows = state.slots().map(|slot| row(state, slot));
    column(rows.chain([reset(state)])).into()
}

/// The last row: Reset shortcuts. Always shown (like History's Clear all), dimmed with
/// its reason while every key is at its default; pressed, it asks first; after a reset
/// it offers Undo until another key changes or the page is left.
fn reset(state: &State) -> Element<'_, Message> {
    if state.resetting {
        let buttons = crate::dir_row![
            setting_row::action(t!(c_cancel), Button::Elevated, Some(Message::KeepShortcuts)),
            setting_row::action(
                t!(k_reset_do),
                Button::Elevated,
                Some(Message::ResetShortcuts(true))
            ),
        ]
        .spacing(space::GAP);
        return setting_row::row(title(t!(k_reset)), t!(k_reset_ask), buttons);
    }
    let (desc, text, action) = match (state.shortcuts_differ(), state.reset_undo.is_some()) {
        (true, _) => (
            t!(k_reset_desc),
            t!(k_reset_btn),
            Some(Message::ResetShortcuts(false)),
        ),
        (false, true) => (
            t!(k_reset_done),
            t!(k_reset_undo),
            Some(Message::UndoShortcuts),
        ),
        (false, false) => (t!(k_reset_same), t!(k_reset_btn), None),
    };
    let ask = setting_row::action(text, Button::Elevated, action);
    setting_row::row(title(t!(k_reset)), desc, ask)
}

/// One shortcut's row (also on the Snip page).
pub(super) fn row(state: &State, slot: Slot) -> Element<'_, Message> {
    let (label, hint, name) = match slot {
        Slot::Capture => (t!(k_open), t!(k_open_desc), t!(k_open_tip)),
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
        (true, Slot::Capture) => t!(k_esc),
        (true, Slot::Quick(_)) => t!(k_cancel),
    };
    let field = shortcut_field(
        key_labels(state.shortcut(slot)),
        field_state,
        t!(c_press_keys),
        t!(c_not_set),
        Message::Record(slot),
    );
    // the problem under the name, above the hint
    let mut name_block =
        column![text(Role::ROW_TITLE, label)].align_x(crate::ui::kit::dir::start());
    if let Some(p) = problem {
        name_block = name_block.push(error(Role::ROW_DESC, p));
    }
    setting_row::row(name_block.into(), hint, tip(field, name, Position::Top))
}
