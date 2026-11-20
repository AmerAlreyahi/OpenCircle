//! Studio: one row. OpenCircle Studio's preferences live in the Studio's own dialog
//! (`features::snip::screens::studio::preferences`, Ctrl+, there), the one source of
//! those rows; Settings only opens it.
use crate::t;
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, State};
use crate::shared::kit::button::Kind;
use crate::shared::kit::setting_row::{self, title};

pub fn view(_: &State) -> Element<'_, Message> {
    column![setting_row::row(
        title(t!(ss_prefs)),
        t!(ss_prefs_d),
        setting_row::action(
            t!(ss_prefs_open),
            Kind::Elevated,
            Some(Message::OpenStudioPreferences)
        ),
    )]
    .into()
}
