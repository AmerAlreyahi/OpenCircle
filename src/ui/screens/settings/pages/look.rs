//! Look & behaviour (`settings-window.js` "LOOK & BEHAVIOUR"): how a
//! selection behaves; the looks are in Appearance, the reader in Text reading.
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, Pick, Section, State};
use crate::config::{After, SearchWith, SelectStyle};
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};

pub fn view(state: &State) -> Element<'_, Message> {
    let sel = state.draft.selection;
    let (from, t) = state.thumb(sel.style, |p| match p {
        Pick::Select(v) => Some(v),
        _ => None,
    });
    let mut rows = column![setting_row::row(
        title("Selection"),
        "Snap: a rounded box with corners to drag",
        segmented(
            &[
                (SelectStyle::Snap, "Snap to box"),
                (SelectStyle::Shape, "As drawn")
            ],
            sel.style,
            from,
            t,
            Message::SelectStyle,
        ),
    )];
    let (from, t) = state.thumb(sel.after, |p| match p {
        Pick::After(v) => Some(v),
        _ => None,
    });
    rows = rows.push(setting_row::row(
        title("Auto action"),
        match sel.after {
            After::Wait => "Nothing is sent until you pick an action",
            After::Search => "Sends each selection at once, as Search with says",
        },
        segmented(
            &[(After::Wait, "Wait for me"), (After::Search, "Run at once")],
            sel.after,
            from,
            t,
            Message::After,
        ),
    ));
    let (from, t) = state.thumb(sel.search_with, |p| match p {
        Pick::SearchWith(v) => Some(v),
        _ => None,
    });
    rows = rows.push(setting_row::row(
        title("Search with"),
        "What searching a selection opens",
        segmented(
            &[
                (SearchWith::Lens, "Lens"),
                (SearchWith::Ai, "Ask AI"),
                (SearchWith::Both, "Both"),
            ],
            sel.search_with,
            from,
            t,
            Message::SearchWith,
        ),
    ));
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
