//! Done (step 4): OpenCircle runs in the tray, the shortcut that opens it (the one
//! really set), and Settings' Start with the OS row. The design's feature
//! cards are left out (owner: no showcase in onboarding; the previews live on
//! each feature's Settings page).
use iced::widget::{Space, column, container, row};
use iced::{Element, Fill};

use super::super::flow::{Message, State};
use crate::ui::kit::keycap::keycap;
use crate::ui::kit::tokens::space;
use crate::ui::screens::settings::{self, rows};

/// The logo's size in the card (`w-10`, its tile's inside).
const LOGO: f32 = 32.0;
/// A key's cap: its label at about 7 px a letter (mono 11) and 10 px each side.
const KEY_PAD: f32 = 20.0;
const KEY_LETTER: f32 = 7.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let s = &state.settings;
    let keys = settings::key_labels(&s.draft().shortcut);
    let caps = row(keys.iter().map(|k| {
        let w = KEY_PAD + KEY_LETTER * k.chars().count() as f32;
        container(keycap(k.clone())).width(w).into()
    }))
    .spacing(space::GAP);
    column![
        super::logo_card(
            LOGO,
            format!("OpenCircle runs in the tray. Press {}.", keys.join("+")),
            "Ready whenever you need to search, read, translate or ask about your screen",
            Some(caps.into()),
        ),
        Space::new().height(Fill),
        rows::start_row(s).map(Message::Settings),
    ]
    .into()
}
