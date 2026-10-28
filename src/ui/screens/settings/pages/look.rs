//! Look & behaviour (`settings-window.js` "LOOK & BEHAVIOUR" and the active
//! look cards): how a selection behaves, who reads the text, and the screen
//! edge's look (the design's 10, or the user's own: its rows under the cards).
use iced::Element;
use iced::widget::{Space, column, row};

use super::super::legacy::custom_rows;
use super::super::state::{Message, Pick, State};
use crate::config::{After, LookTheme, PRESETS, SearchWith, SelectStyle, TextReader};
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::card::{grid, look_card};
use crate::ui::kit::divider::divider;
use crate::ui::kit::looks::{LOOKS, Look};
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::sub;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::theme;

/// The look cards' columns (`grid-cols-2`).
const COLUMNS: usize = 2;

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
    rows = rows.push(setting_row::block(
        title("Active look"),
        "The screen edge and the accent while OpenCircle is open",
        grid(COLUMNS, cards(state)),
    ));
    if state.draft.look.theme == LookTheme::Custom {
        for r in custom_rows(state) {
            rows = rows.push(r).push(divider());
        }
    }
    rows.into()
}

/// What each design look's card writes (by look id): the stored value of the
/// look shown, kept as it is when its card is picked again ("calm" too), else
/// the first preset showing it (Prism is written "aurora", Frost "mono", as
/// before schema 4). `None`: the config has no value for that look yet.
pub fn picks(state: &State) -> Vec<(&'static str, Option<LookTheme>)> {
    let now = state.draft.look.theme;
    let shows = |t: LookTheme, look: &Look| theme::kit_look(t).is_some_and(|l| l.id == look.id);
    LOOKS
        .into_iter()
        .map(|look| {
            let pick = match shows(now, look) {
                true => Some(now),
                false => (PRESETS.iter()).map(|&(t, _)| t).find(|&t| shows(t, look)),
            };
            (look.id, pick)
        })
        .collect()
}

/// The design's looks, then the user's own.
fn cards(state: &State) -> Vec<Element<'_, Message>> {
    let now = state.draft.look.theme;
    let mut cards: Vec<_> = (LOOKS.into_iter().zip(picks(state)))
        .map(|(look, (_, pick))| {
            let chosen = pick == Some(now);
            let on = pick.map(Message::Look);
            look_card(look.name, look.badge, look.desc, look.preview, chosen, on)
        })
        .collect();
    let (a, b) = theme::accent_of(&state.look(LookTheme::Custom));
    cards.push(look_card(
        "Custom",
        "Yours",
        "Your own colours, glow, dim, corners and edges",
        &[a, b],
        now == LookTheme::Custom,
        Some(Message::Look(LookTheme::Custom)),
    ));
    cards
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
