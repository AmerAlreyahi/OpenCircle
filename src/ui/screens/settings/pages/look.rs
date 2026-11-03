//! Look & behaviour (`settings-window.js` "LOOK & BEHAVIOUR"): how a
//! selection behaves; the looks are in Appearance, the reader in Text reading.
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, Pick, Section, State};
use crate::config::SelectStyle;
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};

pub fn view(state: &State) -> Element<'_, Message> {
    let mut rows = column![selection_row(state)];
    // the design's pointer to where the looks went
    rows = rows.push(setting_row::row(
        title("Themes and screen borders"),
        "All 12 themes and the screen border looks live in Appearance",
        button(
            label("Open Appearance →"),
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
        title("Selection"),
        "Snap: a rounded box with corners to drag",
        segmented(
            &[
                (SelectStyle::Snap, "Snap to box"),
                (SelectStyle::Shape, "As drawn"),
            ],
            style,
            from,
            t,
            Message::SelectStyle,
        ),
    )
}
