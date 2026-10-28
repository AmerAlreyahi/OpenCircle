//! Connect AI (step 2): Settings' Provider row; for a provider of your own its
//! preset (OpenAI-like) and key rows. The design's sign-in card is 0.2.0.
use iced::Element;
use iced::widget::column;

use super::super::flow::{Message, State};
use crate::config::Provider;
use crate::ui::screens::settings::rows;

pub fn view(state: &State) -> Element<'_, Message> {
    let s = &state.settings;
    let mut col = column![rows::provider_row(s)];
    if rows::keyed(s) {
        if s.draft().ai.provider == Provider::OpenaiCompatible {
            col = col.push(rows::preset_row(s));
        }
        col = col.push(rows::key_row(s));
    }
    Element::from(col).map(Message::Settings)
}
