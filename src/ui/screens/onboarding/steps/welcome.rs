//! Welcome (step 1): what OpenCircle does, then Settings' Selection, Auto action
//! and Keep up to date rows.
use iced::widget::{Space, column};
use iced::{Element, Fill};

use super::super::flow::{Message, State};
use crate::ui::screens::settings::rows;

/// The logo's size in the card (`w-12`, its tile's inside).
const LOGO: f32 = 40.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let s = &state.settings;
    column![
        super::logo_card(
            LOGO,
            "Welcome to OpenCircle",
            "Press a shortcut, circle anything on your screen, then choose whether to \
             search in your browser, ask AI, copy text or translate.",
            None,
        ),
        Space::new().height(Fill),
        Element::from(column![
            rows::selection_row(s),
            rows::after_row(s),
            rows::update_row(s)
        ])
        .map(Message::Settings),
    ]
    .into()
}
