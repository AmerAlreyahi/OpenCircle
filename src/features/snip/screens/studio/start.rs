//! The empty Studio's start screen (STUDIO v3.1): big thumbnails of the recent
//! pictures (`studio.recent_files` of them), then Open and Paste; a picture
//! can be dropped anywhere on the window (the drop zone lights).
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container, image, mouse_area};
use iced::{Center, ContentFit, Element, Fill};

use super::{Msg, Studio};
use crate::shared::kit::button::{self, Kind};
use crate::shared::kit::icon::Icon;
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, sub, text};
use crate::shared::kit::tokens::{Role, radius, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{chip, dir};
use crate::t;

/// A recent picture's thumbnail, and how many sit in a row.
const THUMB: (f32, f32) = (176.0, 118.0);
const PER_ROW: usize = 4;

/// The recent pictures as big thumbnails with their names (nothing when
/// Settings > Studio shows none, or none were opened yet).
fn recent(s: &Studio) -> Option<Element<'_, Msg>> {
    let shown = usize::from(crate::shared::config::current().studio.recent_files);
    if shown == 0 || s.recent.is_empty() {
        return None;
    }
    let cells: Vec<Element<'_, Msg>> = s
        .recent
        .iter()
        .take(shown)
        .map(|(path, picture)| {
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let shot = image(picture.clone())
                .width(THUMB.0)
                .height(THUMB.1)
                .content_fit(ContentFit::Cover);
            let card = column![
                surface(Material::Recessed, radius::TRACK, shot),
                muted(Role::META, name).wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(space::GAP / 2.0)
            .width(THUMB.0);
            mouse_area(card)
                .on_press(Msg::OpenRecent(path.clone()))
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
        })
        .collect();
    let mut rows: Vec<Element<'_, Msg>> = Vec::new();
    let mut cells = cells;
    for chunk in cells.chunks_mut(PER_ROW) {
        let line = chunk
            .iter_mut()
            .map(|c| std::mem::replace(c, Space::new().into()));
        rows.push(
            dir::row(line.collect::<Vec<_>>())
                .spacing(space::PAGE[1])
                .into(),
        );
    }
    Some(
        column![
            sub(Role::SECTION, t!(st_recent_pictures)),
            column(rows).spacing(space::PAGE[1])
        ]
        .spacing(space::GAP)
        .into(),
    )
}

/// The "Show recordings" row: a quiet labelled button.
fn labelled_quiet(words: &'static str, m: Msg) -> Element<'static, Msg> {
    let w = button::labelled_width(words);
    button::labelled(super::view::OPEN, words, w, Kind::Elevated, false, Some(m))
}

/// Opened alone: recent pictures, Open, Paste, or drop a picture.
pub fn view(s: &Studio) -> Element<'_, Msg> {
    let labelled = |glyph, words: &'static str, kind, m| {
        let w = button::labelled_width(words);
        button::labelled(glyph, words, w, kind, false, Some(m))
    };
    let open = labelled(super::view::OPEN, t!(st_open), Kind::Accent, Msg::Open);
    let paste = labelled(super::view::PASTE, t!(st_paste), Kind::Elevated, Msg::Paste);
    let buttons = crate::dir_row![
        hint(
            open,
            t!(st_open),
            Why::Keys(super::keys::of(super::keys::Cmd::Open)),
            Position::Bottom
        ),
        hint(
            paste,
            t!(st_paste),
            Why::Keys(super::keys::PASTE),
            Position::Bottom
        ),
    ]
    .spacing(space::GAP);
    let mut card = column![
        chip::emblem(Icon::Picture),
        text(Role::CARD_HEADING, t!(st_empty_title)),
        sub(Role::ROW_DESC, t!(st_empty_hint)),
    ]
    .spacing(space::PAGE[0])
    .align_x(Center);
    // Settings > Studio > Show the start screen: off = just the drop zone's words
    if crate::shared::config::current().studio.start_screen {
        card = card.push(buttons);
        if let Some(r) = recent(s) {
            card = card.push(r);
        }
        // recordings nobody saved wait in Recent: one quiet row opens their folder
        if s.recordings {
            card = card.push(labelled_quiet(t!(st_show_recordings), Msg::ShowRecordings));
        }
    }
    let zone = match s.hovered {
        true => Material::Held, // its accent edge: "drop it here"
        false => Material::Recessed,
    };
    container(
        surface(zone, radius::CARD, container(card).center(Fill))
            .width(Fill)
            .height(Fill),
    )
    .padding(space::PAGE[1])
    .width(Fill)
    .height(Fill)
    .into()
}
