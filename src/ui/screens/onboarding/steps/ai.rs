//! Connect AI (step 2): Settings' Answers from row; for an AI provider its
//! Provider, (Custom server) Server URL and key rows. The design's sign-in
//! card is 0.2.0.
use iced::Element;
use iced::widget::column;

use super::super::flow::{Message, State};
use crate::ui::screens::settings::rows;

pub fn view(state: &State) -> Element<'_, Message> {
    let s = &state.settings;
    let mut col = column![rows::provider_row(s)];
    if rows::keyed(s) {
        col = col.push(rows::preset_row(s)).push(rows::url_row(s));
        col = col.push(rows::key_row(s));
    }
    Element::from(col).map(Message::Settings)
}
