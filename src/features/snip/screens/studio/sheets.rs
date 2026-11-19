//! The Studio's small sheets: the shortcuts list (Ctrl+/, board 9), the
//! restore sheet after a crash (board 20) and the right-click menu (board 3).
use iced::widget::{column, container};
use iced::{Center, Element, Fill, Length};

use super::Msg;
use crate::shared::app_language::{S, tr};
use crate::shared::kit::button::{self, Kind, label};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, sub, text};
use crate::shared::kit::tokens::{Role, radius, space};
use crate::shared::kit::{card, dir, divider, keycap, option_list};
use crate::t;

const ROW_H: f32 = 30.0;
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

/// One row of the right-click menu: what, its keys, and whether it can act.
#[cfg_attr(not(test), allow(dead_code))] // ponytail: wired when W86's / W88's APIs land
pub struct Entry {
    pub what: S,
    pub keys: &'static str,
    pub press: Option<Msg>,
    pub sep: bool,
}

/// The right-click menu over `entries` (the editing model decides which).
#[cfg_attr(not(test), allow(dead_code))] // ponytail: wired when W86's / W88's APIs land
pub fn context<'a>(entries: Vec<Entry>) -> Element<'a, Msg> {
    let mut rows: Vec<Element<'a, Msg>> = Vec::new();
    for e in entries {
        if e.sep {
            rows.push(option_list::separator());
        }
        let hint = (!e.keys.is_empty()).then(|| e.keys.to_string());
        rows.push(option_list::item(
            tr(e.what),
            false,
            hint,
            false,
            e.press.unwrap_or(Msg::Menu(None)),
        ));
    }
    option_list::menu(rows)
}
