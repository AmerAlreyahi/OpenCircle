//! Studio: the rows of OpenCircle Studio's own preferences (opening, saving, editing). They are
//! the Studio's preferences model's rows (`State::studio_rows`), so Settings and the Studio's
//! dialog read and write one store and one change shows on the other. The "Open pictures in" row
//! links to Snip's After a capture; the Preferences row opens the dialog for the rest.
use crate::shared::kit::section::section;
use crate::t;
use iced::Element;

use super::super::state::{Message, Section, State};
use crate::shared::kit::button::Kind;
use crate::shared::kit::setting_row::{self, title};

pub fn view(state: &State) -> Element<'_, Message> {
    let opening = section(Some(t!(st_opening)), None, || {
        let mut rows = vec![setting_row::row(
            title(t!(ss_open_in)),
            t!(ss_open_in_snip),
            setting_row::action(
                t!(ss_open_snip),
                Kind::Elevated,
                Some(Message::Section(Section::Snip)),
            ),
        )];
        rows.extend(state.studio_rows(&[t!(ss_reopen), t!(ss_ask_unsaved), t!(ss_keep)]));
        rows
    });
    let saving = section(Some(t!(pf_tab_saving)), None, || {
        state.studio_rows(&[t!(ss_name), t!(ss_format), t!(ss_folder), t!(ss_copy_path)])
    });
    let editing = section(Some(t!(pf_tab_editing)), None, || {
        let mut rows =
            state.studio_rows(&[t!(ss_snap), t!(ss_zoom), t!(ss_wheel), t!(ss_tool_styles)]);
        rows.push(setting_row::row(
            title(t!(ss_prefs)),
            t!(ss_prefs_d),
            setting_row::action(
                t!(ss_prefs_open),
                Kind::Elevated,
                Some(Message::OpenStudioPreferences),
            ),
        ));
        rows
    });
    iced::widget::column![opening, saving, editing]
        .spacing(crate::shared::kit::tokens::settings::SECTION_GAP)
        .into()
}
