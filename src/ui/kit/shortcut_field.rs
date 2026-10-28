//! `.oc-shortcut-box`: a shortcut as one keycap per key, as wide as a
//! settings row's control column and 36 px tall for every shortcut (owner
//! rule: every shortcut field the same width), `track` with a
//! `rim` edge, radius 10. Three cells (more for a longer shortcut), the keys
//! at the right, the rest `keycap::empty`; no keys = "Not set". Under the
//! cursor the edge is `accent`; recording (the design has no look for it) is
//! a 2 px `accent` edge with the hint in `accent`; a conflict a 2 px `DANGER`
//! edge. The box is the same size in every state.
use iced::widget::{button, container, row, text};
use iced::{Border, Element, Fill};

use super::keycap::{empty, keycap};
use super::text::font;
use super::themes::c;
use super::tokens::{Role, fixed, radius, size};

/// The design's cells and their gap (`grid-cols-3 gap-[5px]`), its side pad.
const CELLS: usize = 3;
const GAP: f32 = 5.0;
const PAD: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Rest,
    /// Waiting for the keys.
    Recording,
    /// The keys pressed are taken (or not a shortcut): the row says why.
    Conflict,
}

pub fn shortcut_field<'a, Message: Clone + 'a>(
    keys: Vec<String>,
    state: State,
    recording_hint: &'a str,
    on_press: Message,
) -> Element<'a, Message> {
    let line = |s: &'a str, colour| -> Element<'a, Message> {
        let role = Role::ROW_DESC;
        container(text(s).size(role.size).font(font(role)).color(colour))
            .center(Fill)
            .into()
    };
    let inner = match state {
        State::Recording => line(recording_hint, c().accent),
        _ if keys.is_empty() => line("Not set", c().text_muted),
        _ => {
            let blank = CELLS.saturating_sub(keys.len());
            let cells = (0..blank)
                .map(|_| empty())
                .chain(keys.into_iter().map(keycap));
            row(cells).spacing(GAP).into()
        }
    };
    button(container(inner).center_y(Fill))
        .width(size::SHORTCUT.width)
        .height(size::SHORTCUT.height)
        .padding([0.0, PAD])
        .on_press(on_press)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let (width, edge) = match state {
                State::Recording => (2.0, c().accent),
                State::Conflict => (2.0, fixed::DANGER),
                State::Rest if hot => (1.0, c().accent),
                State::Rest => (1.0, c().rim),
            };
            button::Style {
                background: Some(c().track.into()),
                text_color: c().text,
                border: Border::default()
                    .rounded(radius::TRACK)
                    .width(width)
                    .color(edge),
                ..button::Style::default()
            }
        })
        .into()
}
