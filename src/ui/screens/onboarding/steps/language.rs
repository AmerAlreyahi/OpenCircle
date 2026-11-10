//! Your language (step 1, owner 2026-11-10): the language picker's list
//! (search, Suggested, A-Z; the OS language preselected) beside what the pick
//! means: text is translated into it, and which local model the first
//! translation downloads (nothing is downloaded before that, nothing is sent).
//! The pick is the app language too, shown from the multi-language release on.
use iced::Element;
use iced::widget::{Space, column, row};

use super::super::flow::{Message, State};
use crate::mt;
use crate::translate::lang::{self, AUTO};
use crate::ui::kit::card::card;
use crate::ui::kit::text::{sub, text};
use crate::ui::kit::tokens::{Role, space};
use crate::ui::screens::model_download::mb;

pub fn view(state: &State) -> Element<'_, Message> {
    let target = &state.settings.draft().translate.target;
    let words = column![
        card(
            column![
                text(Role::ROW_TITLE, "Your language"),
                sub(
                    Role::ROW_DESC,
                    "OpenCircle translates text on your screen into it. You can change it any \
                     time in Settings > Translate."
                )
                .line_height(1.5),
            ]
            .spacing(space::GAP / 2.0)
        ),
        card(sub(Role::ROW_DESC, model_note(target)).line_height(1.5)),
    ]
    .spacing(space::GAP)
    .width(iced::Fill);
    let list = state.picker.list().map(|l| l.map(Message::Language));
    row![words, Space::new().width(space::GAP)]
        .push(list)
        .into()
}

/// Which local model translating into `target` downloads the first time,
/// and its size (English text: the most common source).
pub(in super::super) fn model_note(target: &str) -> String {
    let name = lang::name(target);
    if lang::find(target).is_some_and(|l| l.code == "en") {
        return "Text in another language is translated into English with that language's local \
                model, downloaded the first time you translate it. Nothing is downloaded or sent \
                before that."
            .into();
    }
    match mt::next("en", target) {
        Some(p) if p.missing_bytes() == 0 => format!("{} is already on this PC.", p.label()),
        Some(p) => format!(
            "Your first translation downloads {} (about {} MB). Nothing is downloaded or sent \
             before that.",
            p.label(),
            mb(p.missing_bytes())
        ),
        None if mt::available(AUTO, target) => format!(
            "Your first translation into {name} downloads its local model. Nothing is downloaded \
             or sent before that."
        ),
        None => format!(
            "No local model translates into {name} yet: choose AI in Settings > Translate to \
             translate into it."
        ),
    }
}
