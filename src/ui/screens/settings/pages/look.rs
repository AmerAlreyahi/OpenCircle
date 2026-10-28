//! Look & behaviour (`settings-window.js` "LOOK & BEHAVIOUR"): how a
//! selection behaves and who reads the text; the looks are in Appearance.
use iced::Element;
use iced::widget::{Space, column, row};

use super::super::state::{Message, Pick, Section, State};
use crate::config::{After, SearchWith, SelectStyle, TextReader};
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::sub;
use crate::ui::kit::tokens::{Role, space};

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
    if cfg!(windows) {
        rows = rows.push(text_reader(state));
    }
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

/// Who reads the screen's text (Windows: Windows OCR or the downloaded
/// reader), with a note and a way to add a missing language.
fn text_reader(state: &State) -> Element<'_, Message> {
    let reader = state.draft.selection.text_reader;
    let (from, t) = state.thumb(reader, |p| match p {
        Pick::Reader(v) => Some(v),
        _ => None,
    });
    let mut control = column![segmented(
        &[
            (TextReader::Auto, "Auto"),
            (TextReader::Windows, "Windows"),
            (TextReader::PpOcr, "Downloaded"),
        ],
        reader,
        from,
        t,
        Message::TextReader,
    )]
    .spacing(space::GAP)
    .align_x(iced::Right);
    if let Some(note) = state.reader_note() {
        let add = button(
            label("Add in Windows"),
            Kind::Elevated,
            Some(Message::OpenLanguageSettings),
        );
        control = control
            .push(sub(Role::ROW_DESC, note))
            .push(row![Space::new().width(iced::Fill), add]);
    }
    setting_row::wide(
        title("Text reader"),
        "Who reads the screen's text, so you can select it",
        control,
    )
}
