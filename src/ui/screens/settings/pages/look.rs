//! Look & behaviour (`settings-window.js` "LOOK & BEHAVIOUR"): how a
//! selection behaves; the looks are in Appearance, the reader in Text reading.
use crate::t;
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, Pick, Section, State};
use crate::shared::config::SelectStyle;
use crate::shared::kit::button::Kind;
use crate::shared::kit::segmented::segmented;
use crate::shared::kit::setting_row::{self, title};

pub fn view(state: &State) -> Element<'_, Message> {
    let mut rows = column![selection_row(state)];
    // the design's pointer to where the looks went
    rows = rows.push(setting_row::row(
        title(t!(l_themes)),
        t!(l_themes_desc),
        setting_row::action(
            t!(l_themes_open_arrow),
            Kind::Elevated,
            Some(Message::Section(Section::Appearance)),
        ),
    ));
    rows.into()
}

/// Snap to box / As drawn (Settings, and onboarding's first step).
pub fn selection_row(state: &State) -> Element<'_, Message> {
    let style = state.draft.selection.style;
    let (from, t) = state.thumb(style, |p| match p {
        Pick::Select(v) => Some(v),
        _ => None,
    });
    setting_row::row(
        title(t!(l_sel)),
        t!(l_sel_desc),
        segmented(
            &[
                (SelectStyle::Snap, t!(l_sel_snap)),
                (SelectStyle::Shape, t!(l_sel_drawn)),
            ],
            style,
            from,
            t,
            Message::SelectStyle,
        ),
    )
}
