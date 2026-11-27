//! Look & behaviour (`settings-window.js` "LOOK & BEHAVIOUR"): how a
//! selection behaves; the looks are in Appearance, the reader in Text reading.
use crate::shared::kit::icon::Icon;
use crate::t;
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, Section, State};
use crate::shared::config::SelectStyle;
use crate::shared::kit::button::Kind;
use crate::shared::kit::choice_card::{Choice, Mark, choice_cards};
use crate::shared::kit::section::section;
use crate::shared::kit::setting_row::{self, title};

pub fn view(state: &State) -> Element<'_, Message> {
    page_for(state, cfg!(windows))
}

/// The page for a platform (a parameter so both branches run in the Linux tests).
pub(in crate::features::settings) fn page_for(
    state: &State,
    windows: bool,
) -> Element<'_, Message> {
    let style = state.draft.selection.style;
    let styles = [
        (
            SelectStyle::Snap,
            Mark::Glyph(Icon::Square.glyph()),
            t!(l_sel_snap),
            t!(l_sel_snap_d),
        ),
        (
            SelectStyle::Shape,
            Mark::Glyph(Icon::Circle.glyph()),
            t!(l_sel_drawn),
            t!(l_sel_drawn_d),
        ),
    ];
    let cards: Vec<_> = (styles.iter())
        .map(|&(_, mark, title, desc)| Choice { mark, title, desc })
        .collect();
    let chosen = styles.iter().position(|s| s.0 == style).unwrap_or(0);
    let selection = setting_row::block(
        title(t!(l_sel)),
        t!(l_sel_desc),
        choice_cards(&cards, chosen, move |i| Message::SelectStyle(styles[i].0)),
    );
    // the design's pointer to where the looks went
    let themes = setting_row::row(
        title(t!(l_themes)),
        t!(l_themes_desc),
        setting_row::action(
            t!(l_themes_open_arrow),
            Kind::Elevated,
            Some(Message::Section(Section::Appearance)),
        ),
    );
    column![
        section(Some(t!(l_sec)), None, || vec![selection, themes]),
        super::text_reading::section_for(state, windows),
    ]
    .spacing(crate::shared::kit::tokens::settings::SECTION_GAP)
    .into()
}
