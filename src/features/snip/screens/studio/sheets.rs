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

const ROW_H: f32 = 30.0;
const LEAD: f32 = 2.0;
const CAP_GAP: f32 = 4.0;
const SHEET_W: f32 = 560.0;

/// The rows of the shortcuts sheet (STUDIO_V3_SPEC §7): what, and its keys.
const ROWS: [(S, &[&str]); 11] = [
    (S::st_sc_tools, &["V R O L A P H X T C N B K S M"]),
    (S::st_sc_copy, &["Ctrl+C", "Ctrl+X", "Ctrl+V"]),
    (S::st_sc_whole, &["Ctrl+Shift+C"]),
    (S::st_sc_dup, &["Ctrl+D", "Delete", "Ctrl+A"]),
    (S::st_sc_undo, &["Ctrl+Z", "Ctrl+Y"]),
    (S::st_sc_nudge, &["←↑→↓", "Shift+←↑→↓"]),
    (S::st_sc_order, &["Ctrl+]", "Ctrl+["]),
    (S::st_sc_panels, &["Ctrl+B", "Ctrl+Alt+B", "Ctrl+\\"]),
    (S::st_sc_zoom, &["Ctrl+=", "Ctrl+-", "Ctrl+0", "Ctrl+1"]),
    (S::st_sc_file, &["Ctrl+O", "Ctrl+S", "Ctrl+Shift+S"]),
    (S::st_sc_misc, &["Alt+A", "Ctrl+W", "Ctrl+,"]),
];

/// The shortcuts sheet: a card over the window, Close at its foot.
pub fn shortcuts<'a>() -> Element<'a, Msg> {
    let rows = ROWS.iter().map(|(what, keys)| {
        let caps = dir::row(keys.iter().map(|k| keycap::keycap(*k))).spacing(CAP_GAP);
        crate::dir_row![sub(Role::ROW_DESC, tr(*what)).width(Fill), caps]
            .align_y(Center)
            .height(ROW_H)
            .into()
    });
    let body = column![
        text(Role::CARD_HEADING, t!(st_sc_title)),
        divider::divider(),
        column(rows),
        container(button::button(
            label(t!(st_close)),
            Kind::Elevated,
            Some(Msg::Shortcuts(false))
        ))
        .align_right(Fill),
    ]
    .spacing(space::GAP);
    surface(
        Material::Widget,
        radius::CARD,
        container(body).padding(space::PAGE),
    )
    .width(Length::Fixed(SHEET_W))
    .into()
}

/// A picture kept after a crash.
#[cfg_attr(not(test), allow(dead_code))] // ponytail: wired when W86's / W88's APIs land
pub struct Restorable {
    pub name: String,
    pub size: (u32, u32),
    pub png: Vec<u8>,
}

/// The restore sheet.
#[cfg_attr(not(test), allow(dead_code))] // ponytail: wired when W86's / W88's APIs land
pub fn restore<'a>(r: &'a Restorable, restore: Msg, discard: Msg) -> Element<'a, Msg> {
    let body = column![
        text(Role::CARD_HEADING, t!(st_restore_title)),
        sub(Role::ROW_DESC, t!(st_restore_hint)),
        muted(
            Role::KEYCAP,
            format!("{} · {} × {}", r.name, r.size.0, r.size.1)
        ),
        crate::dir_row![
            iced::widget::Space::new().width(Fill),
            button::button(label(t!(st_discard)), Kind::Elevated, Some(discard)),
            button::button(label(t!(st_restore)), Kind::Accent, Some(restore)),
        ]
        .spacing(space::GAP),
    ]
    .spacing(space::GAP);
    card::card(body)
}

fn words(a: editing::Action) -> S {
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
