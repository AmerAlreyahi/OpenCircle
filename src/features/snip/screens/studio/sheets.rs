//! The Studio's small sheets: the shortcuts list (Ctrl+/, board 9), the
//! restore sheet after a crash (board 20) and the right-click menu (board 3).
use iced::widget::{column, container};
use iced::{Center, Element, Fill, Length};

use super::{Msg, editing};
use crate::shared::app_language::{S, tr};
use crate::shared::kit::button::{self, Kind, label};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, sub, text};
use crate::shared::kit::tokens::{Role, radius, space};
use crate::shared::kit::{card, dir, divider, keycap, option_list};
use crate::t;

const LEAD: f32 = 2.0;
const CAP_GAP: f32 = 4.0;
/// Board 9: 1000 wide, three columns.
const SHEET_W: f32 = 1000.0;
const COLUMN_GAP: f32 = 32.0;
const RESTORE_BUTTON: f32 = 96.0;

/// A row: what, and its key names (`keys`).
type Row<'a> = (&'a str, Vec<String>);

fn row<'a>(what: &'a str, keys: &[&str]) -> Row<'a> {
    (what, keys.iter().map(|k| k.to_string()).collect())
}

/// One shortcut row: the name, then its keys as key caps (a key name's parts
/// are one cap each), wrapping when there are many.
fn shortcut_row<'a>((what, chords): Row<'a>) -> Element<'a, Msg> {
    let caps = chords.into_iter().map(|c| {
        let parts = c
            .split('+')
            .map(|k| keycap::keycap(k.to_string()))
            .collect::<Vec<_>>();
        Element::from(dir::row(parts).spacing(CAP_GAP / 2.0))
    });
    let caps = iced::widget::row(caps.collect::<Vec<_>>())
        .spacing(CAP_GAP * 2.0)
        .wrap();
    column![sub(Role::ROW_DESC, what), caps]
        .spacing(CAP_GAP)
        .padding([ROW_PAD, 0.0])
        .into()
}

const ROW_PAD: f32 = 5.0;

fn shortcut_column<'a>(title: &'a str, rows: Vec<Row<'a>>) -> Element<'a, Msg> {
    let rows = rows.into_iter().map(shortcut_row);
    column![
        muted(Role::SECTION, title),
        divider::divider(),
        column(rows.collect::<Vec<_>>())
    ]
    .spacing(space::GAP)
    .width(Fill)
    .into()
}

/// The shortcuts sheet (board 9): three columns (the tools, editing, view and
/// files, every key from `keys` and the tool table), a foot note, Edit
/// shortcuts… (Settings) and Close.
pub fn shortcuts<'a>() -> Element<'a, Msg> {
    use super::keys as k;
    use crate::features::snip::annotate::{Group, Tool, info};
    let mut tools: Vec<Row<'a>> = Vec::new();
    for g in Group::ALL {
        let mut shown: Vec<_> = g.tools().collect();
        if g == Group::Select {
            shown.push(info(Tool::Crop));
        }
        for t in shown {
            tools.push(row(tr(t.name), &[&t.key.to_ascii_uppercase().to_string()]));
        }
    }
    let editing = vec![
        row(tr(S::st_sc_copy), &[k::COPY, k::CUT, k::PASTE]),
        row(tr(S::st_sc_whole), &[k::COPY_PICTURE]),
        row(tr(S::st_sc_dup), &[k::DUPLICATE, k::DELETE, k::SELECT_ALL]),
        row(tr(S::st_sc_undo), &[k::UNDO, k::REDO]),
        row(tr(S::st_sc_nudge), &[k::NUDGE, k::NUDGE_FAR]),
        row(
            tr(S::st_sc_order),
            &[k::FORWARD, k::BACKWARD, k::FRONT, k::BACK],
        ),
        row(tr(S::st_sc_steps), &[k::ADD_STEPS]),
    ];
    let view = vec![
        row(
            tr(S::st_sc_panels),
            &[k::LEFT_PANEL, k::RIGHT_PANEL, k::ALL_PANELS],
        ),
        row(
            tr(S::st_sc_zoom),
            &[k::ZOOM_IN, k::ZOOM_OUT, k::FIT, k::ACTUAL],
        ),
        row(tr(S::st_sc_pan), &[k::PAN]),
        row(tr(S::st_sc_file), &[k::OPEN, k::SAVE, k::SAVE_AS]),
        row(tr(S::st_sc_misc), &[k::NEW_SNIP, k::CLOSE, k::SETTINGS]),
        row(tr(S::st_shortcuts), &[k::SHORTCUTS]),
    ];
    let columns = crate::dir_row![
        shortcut_column(t!(st_sc_g_tools), tools),
        shortcut_column(t!(st_sc_g_edit), editing),
        shortcut_column(t!(st_sc_g_view), view),
    ]
    .spacing(COLUMN_GAP);
    let head = crate::dir_row![
        column![
            text(Role::CARD_HEADING, t!(st_sc_title)),
            sub(Role::ROW_DESC, t!(st_sc_sub)),
        ]
        .width(Fill),
        button::button(
            label(t!(st_close)),
            Kind::Elevated,
            Some(Msg::Shortcuts(false))
        ),
    ]
    .align_y(Center);
    let foot = crate::dir_row![
        sub(Role::META, t!(st_sc_foot)).width(Fill),
        button::button(
            label(t!(st_sc_edit)),
            Kind::Elevated,
            Some(Msg::EditShortcuts)
        ),
    ]
    .spacing(space::PAGE[1])
    .align_y(Center);
    let body = column![head, divider::divider(), columns, divider::divider(), foot]
        .spacing(space::PAGE[1]);
    surface(
        Material::Widget,
        radius::CARD,
        container(body).padding(space::PAGE),
    )
    .width(Length::Fixed(SHEET_W))
    .into()
}

/// A picture kept after a crash.
pub struct Restorable {
    pub name: String,
    pub size: (u32, u32),
    pub png: Vec<u8>,
    /// The drawings, as `Annotator::annotations_json` wrote them.
    pub notes: String,
}

/// The restore sheet.
pub fn restore<'a>(r: &'a Restorable, restore: Msg, discard: Msg, later: Msg) -> Element<'a, Msg> {
    let sized = |b: Element<'a, Msg>| container(b).width(RESTORE_BUTTON);
    let body = column![
        text(Role::CARD_HEADING, t!(st_restore_title)),
        sub(Role::ROW_DESC, t!(st_restore_hint)),
        muted(
            Role::KEYCAP,
            format!("{} · {} × {}", r.name, r.size.0, r.size.1)
        ),
        crate::dir_row![
            iced::widget::Space::new().width(Fill),
            sized(button::button(
                label(t!(st_later)),
                Kind::Quiet,
                Some(later)
            )),
            sized(button::button(
                label(t!(st_discard)),
                Kind::Elevated,
                Some(discard)
            )),
            sized(button::button(
                label(t!(st_restore)),
                Kind::Accent,
                Some(restore)
            )),
        ]
        .spacing(space::GAP),
    ]
    .spacing(space::GAP);
    card::card(body)
}

pub(super) fn words(a: editing::Action) -> S {
    use editing::Action as A;
    match a {
        A::Cut => S::st_cut,
        A::Copy => S::c_copy,
        A::Paste => S::st_paste,
        A::Duplicate => S::st_duplicate,
        A::Delete => S::st_delete,
        A::Forward => S::st_forward,
        A::Backward => S::st_backward,
        A::Front => S::st_to_front,
        A::Back => S::st_to_back,
        A::SelectAll => S::st_select_all,
        A::CopyPicture => S::st_copy_picture,
        A::Fit => S::st_fit,
        A::Shortcuts => S::st_shortcuts,
    }
}

/// A row's key as text ("Ctrl+Shift+C").
pub(super) fn chord_text(k: editing::Chord) -> String {
    caps(k).join("+")
}

/// A row's key as key caps.
fn caps(k: editing::Chord) -> Vec<String> {
    use editing::Chord as C;
    match k {
        C::Ctrl(c) => vec!["Ctrl".into(), c.to_string()],
        C::CtrlShift(c) => vec!["Ctrl".into(), "Shift".into(), c.to_string()],
        C::Delete => vec!["Delete".into()],
    }
}

/// The right-click menu over the editor's `entries`: a row that can't act is
/// dimmed; a line above the rows that start a group.
pub fn context(entries: &[editing::Entry]) -> Element<'_, Msg> {
    let mut rows: Vec<Element<'_, Msg>> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if e.divider {
            rows.push(option_list::separator());
        }
        rows.push(option_list::action(option_list::ActionRow {
            lead: iced::widget::Space::new().width(LEAD).into(),
            label: tr(words(e.action)),
            external: false,
            caps: caps(e.keys),
            reading: false,
            focused: false,
            press: e.enabled.then_some(Msg::ContextPick(i)),
        }));
    }
    option_list::menu(rows)
}
