//! Look (step 3): Settings' theme cards (a card is its theme and its mode; no OS
//! mode or Glass rows: owner, 2026-10-28).
use iced::Element;

use super::super::flow::{Message, State};
use crate::ui::screens::settings::rows;

pub fn view(state: &State) -> Element<'_, Message> {
    rows::themes_row(&state.settings).map(Message::Settings)
}
