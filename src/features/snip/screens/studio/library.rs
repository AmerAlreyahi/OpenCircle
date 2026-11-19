//! The Library (left panel, STUDIO_V3_SPEC §5): Recent files, Objects and
//! Steps as three collapsible sections, never empty (each says what goes
//! there), in the kit's side panel.
use std::sync::Arc;

use iced::widget::{column, container, image, mouse_area};
use iced::{Center, ContentFit, Element, Fill};

use super::{Msg, Studio};
use crate::features::snip::annotate::{Annotator, Message, Shape, Tool, info};
use crate::features::snip::screens::steps::Step;
use crate::shared::app_language::tr;
use crate::shared::kit::side_panel::{self, lead, row, section_head, visible_mark};
use crate::shared::kit::text::{muted, text};
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::{button, divider};
use crate::t;

/// A Recent row's height, an Object row's, the thumbnail.
const FILE_ROW: f32 = 44.0;
pub(super) const OBJECT_ROW: f32 = 30.0;
const THUMB: (f32, f32) = (40.0, 28.0);
pub(super) const PAD: f32 = 12.0;
const ROW_GAP: f32 = 10.0;

/// The three sections, in order (also the indexes of `Studio::shut`).
pub const RECENT: usize = 0;
pub const OBJECTS: usize = 1;
pub const STEPS: usize = 2;

fn section<'a>(
    s: &Studio,
    at: usize,
    title: &'a str,
    count: Option<usize>,
    body: Element<'a, Msg>,
) -> Element<'a, Msg> {
    let open = !s.shut[at];
    let mut col = column![section_head(title, open, count, Msg::Section(at))];
    if open {
        col = col.push(body);
    }
    col.into()
}

/// The shape's tool (its icon and name); one the list doesn't know is Select.
pub(super) fn tool_of(shape: &Shape) -> Tool {
    match shape {
        Shape::Rect { .. } => Tool::Rect,
        Shape::Ellipse { .. } => Tool::Ellipse,
        Shape::Arrow { .. } => Tool::Arrow,
        Shape::Pen(_) => Tool::Pen,
        Shape::Highlighter(_) => Tool::Highlighter,
        Shape::Text { .. } => Tool::Text,
        Shape::Step { .. } => Tool::Step,
        Shape::Blur { .. } => Tool::Blur,
        Shape::Redact { .. } => Tool::Redact,
        Shape::Spotlight { .. } => Tool::Spotlight,
        _ => Tool::Select,
    }
}

fn note(words: &str) -> Element<'_, Msg> {
    container(muted(Role::ROW_DESC, words))
        .padding([0.0, PAD])
        .into()
}

fn recent(s: &Studio) -> Element<'_, Msg> {
    let mut col = column![];
    let shown = usize::from(crate::shared::config::current().studio.recent_files);
    if shown == 0 {
        col = col.push(note(t!(st_recent_zero)));
    } else if !crate::shared::config::current().snip.history {
        col = col.push(note(t!(st_recent_off)));
    } else if s.recent.is_empty() {
        col = col.push(note(t!(st_recent_none)));
    }
    for (at, (path, picture)) in s.recent.iter().take(shown).enumerate() {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let info = s.recent_info.get(at).cloned().unwrap_or_default();
        let thumb = image(picture.clone())
            .width(THUMB.0)
            .height(THUMB.1)
            .content_fit(ContentFit::Cover);
        let words = column![
            text(Role::CONTROL, name).wrapping(iced::widget::text::Wrapping::None),
            muted(Role::META, info),
        ];
        let line = crate::dir_row![thumb, words.width(Fill)]
            .spacing(ROW_GAP)
            .align_y(Center);
        let press = Some(Msg::OpenRecent(path.clone()));
        col = col.push(row(line.into(), FILE_ROW, false, press));
    }
    let open = button::button(
        button::label(t!(st_open)),
        button::Kind::Elevated,
        Some(Msg::Open),
    );
    let clear = button::button(
        button::label(t!(st_clear)),
        button::Kind::Quiet,
        (!s.recent.is_empty()).then_some(Msg::ClearRecent),
    );
    let foot = crate::dir_row![open, clear].spacing(space::GAP);
    col.push(container(foot).padding([space::GAP, PAD])).into()
}

fn objects(e: Option<&Annotator>) -> Element<'_, Msg> {
    let items = e.map_or(&[][..], |e| &e.doc.items[..]);
    if items.is_empty() {
        return note(t!(st_objects_empty));
    }
    let selected = e.map_or_else(Vec::new, |e| e.selection());
    let n = items.len();
    // top = front: the list runs top to bottom
    let rows = items.iter().enumerate().rev().map(|(i, item)| {
        let t = info(tool_of(&item.shape));
        let on = selected.contains(&i);
        let line = crate::dir_row![
            lead(t.icon, on),
            text(Role::CONTROL, tr(t.name)).width(Fill)
        ]
        .spacing(space::GAP)
        .align_y(Center);
        let shown = !item.hidden;
        let eye = visible_mark(shown, Msg::Editor(Message::ToggleHidden(i)));
        let line = crate::dir_row![line.width(Fill), eye].align_y(Center);
        let slot = n - 1 - i;
        let held = row(line.into(), OBJECT_ROW, on, None);
        mouse_area(held).on_press(Msg::ObjPress(slot)).into()
    });
    let list = column(rows.collect::<Vec<Element<'_, Msg>>>());
    mouse_area(list)
        .on_move(move |at| Msg::ObjMove(at, n))
        .on_release(Msg::ObjRelease)
        .into()
}

/// The whole Library panel. `steps`: the Steps strip's steps.
pub fn view<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    let objs = s.editor.as_ref().map(|e| e.doc.items.len());
    let count = Some(steps.len()).filter(|n| *n > 0);
    let body = column![
        section(s, RECENT, t!(st_sec_recent), None, recent(s)),
        divider::divider(),
        section(
            s,
            OBJECTS,
            t!(st_sec_objects),
            objs.filter(|n| *n > 0),
            objects(s.editor.as_ref())
        ),
        divider::divider(),
        section(
            s,
            STEPS,
            t!(st_steps),
            count,
            super::view::steps_list(s, steps)
        ),
    ];
    side_panel::panel(
        super::LIBRARY,
        t!(st_library),
        super::view::PANEL_LEFT,
        Msg::Library,
        t!(st_hide_library),
        body.into(),
    )
}
