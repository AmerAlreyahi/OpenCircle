//! Studio: one row. OpenCircle Studio's preferences live in the Studio's own dialog
//! (`features::snip::screens::studio::preferences`, Ctrl+, there), the one source of
//! those rows; Settings only opens it.
use crate::shared::kit::section::section;
use crate::t;
use iced::Element;

use super::super::state::{Message, Section, State};
use crate::shared::kit::button::Kind;
use crate::shared::kit::setting_row::{self, title};

pub fn view(_: &State) -> Element<'_, Message> {
    section(Some(t!(st_opening)), None, || {
        vec![
            setting_row::row(
                title(t!(ss_open_in)),
                t!(ss_open_in_snip),
                setting_row::action(
                    t!(ss_open_snip),
                    Kind::Elevated,
                    Some(Message::Section(Section::Snip)),
                ),
            ),
            setting_row::row(
                title(t!(ss_prefs)),
                t!(ss_prefs_d),
                setting_row::action(
                    t!(ss_prefs_open),
                    Kind::Elevated,
                    Some(Message::OpenStudioPreferences),
                ),
            ),
        ]
    })
}
