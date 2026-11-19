//! The header's File / Edit / View menu bar (STUDIO_V3_SPEC §6, boards 6, 7,
//! 7b): a bar of three openers and the menu under the opened one, from the
//! kit's menu rows. Alt opens File, arrows walk, Enter picks, Esc closes.
use iced::widget::{container, pin, stack};
use iced::{Center, Element, Fill, Padding};

use super::{Msg, Studio};
use crate::features::snip::annotate::Message;
use crate::shared::app_language::{S, tr};
use crate::shared::kit::tokens::Role;
use crate::shared::kit::{dir, option_list, side_panel, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    View,
}

impl Menu {
    pub const ALL: [Menu; 3] = [Menu::File, Menu::Edit, Menu::View];

    fn name(self) -> &'static str {
        tr(match self {
            Menu::File => S::st_menu_file,
            Menu::Edit => S::st_menu_edit,
            Menu::View => S::st_menu_view,
        })
    }

    fn step(self, by: isize) -> Menu {
        let at = Menu::ALL.iter().position(|m| *m == self).unwrap_or(0) as isize;
        Menu::ALL[(at + by).rem_euclid(3) as usize]
    }
}

/// What the keyboard does to the menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// Alt: open File, or close the menu.
    Alt,
    Prev,
    Next,
    Up,
    Down,
    Enter,
}

/// One menu row.
pub struct Item {
    pub label: &'static str,
    pub keys: &'static str,
    pub checked: bool,
    pub press: Msg,
    /// A hairline above it.
    pub sep: bool,
}

fn item(label: S, keys: &'static str, press: Msg, sep: bool) -> Item {
    Item {
        label: tr(label),
        keys,
        checked: false,
        press,
        sep,
    }
}

/// The menu's rows (the panels' checks follow the Studio's state).
pub fn items(menu: Menu, s: &Studio) -> Vec<Item> {
    let ed = |m| Msg::Editor(m);
    match menu {
        Menu::File => vec![
            item(S::st_new_snip, "Alt+A", Msg::NewSnip, false),
            item(S::st_open, "Ctrl+O", Msg::Open, false),
            item(S::c_save, "Ctrl+S", ed(Message::Save), true),
            item(S::st_save_as, "Ctrl+Shift+S", Msg::Export, false),
            item(S::st_export_walkthrough, "", Msg::ExportWalkthrough, false),
            item(S::st_copy_picture, "Ctrl+Shift+C", ed(Message::Copy), true),
            item(S::st_settings, "Ctrl+,", Msg::Settings, true),
            item(S::st_shortcuts, "Ctrl+/", Msg::Shortcuts(true), false),
            item(
                S::st_close,
                "Ctrl+W",
                Msg::Caption(crate::shared::kit::header::Caption::Close),
                true,
            ),
        ],
        Menu::Edit => vec![
            item(S::sn_undo, "Ctrl+Z", ed(Message::Undo), false),
            item(S::sn_redo, "Ctrl+Y", ed(Message::Redo), false),
            item(S::st_paste, "Ctrl+V", Msg::Paste, true),
            item(S::st_delete, "Delete", ed(Message::Delete), true),
        ],
        Menu::View => {
            let p = s.panels;
            let mut v = vec![
                item(S::st_left_panel, "Ctrl+B", Msg::Library, false),
                item(S::st_right_panel, "Ctrl+Alt+B", Msg::Panel, false),
                item(S::st_all_panels, "Ctrl+\\", Msg::Both, false),
                item(S::st_zoom_in, "Ctrl+=", Msg::ZoomIn, true),
                item(S::st_zoom_out, "Ctrl+-", Msg::ZoomOut, false),
                item(S::st_fit, "Ctrl+0", Msg::Fit, false),
                item(S::st_actual, "Ctrl+1", Msg::Actual, false),
            ];
            v[0].checked = p.library;
            v[1].checked = p.options;
            v[2].checked = p.library && p.options;
            v
        }
    }
}

/// The menu hangs under the header and its hairline.
const HEADER_BELOW: f32 = 41.0;

/// An opener's side padding in all (as the kit draws it), and the gap between openers.
const OPENER_PAD: f32 = 22.0;
const OPENER_GAP: f32 = 2.0;

/// The three openers.
pub fn bar(s: &Studio) -> Element<'_, Msg> {
    let open = s.menu.map(|(m, _)| m);
    let row = Menu::ALL.map(|m| {
        let on = open == Some(m);
        side_panel::menu_opener(m.name(), on, Msg::Menu(if on { None } else { Some(m) }))
    });
    dir::row(row).spacing(OPENER_GAP).align_y(Center).into()
}

/// Where the opener of `menu` starts, from the header's start edge.
fn offset(menu: Menu) -> f32 {
    let role = Role::NAV;
    let mut x = 14.0 + 16.0 + 10.0 + 14.0; // pad, logo, gap, pad
    for m in Menu::ALL {
        if m == menu {
            return x;
        }
        let w = text::width(m.name(), text::size(role), text::font(role));
        x += w.ceil() + OPENER_PAD + OPENER_GAP;
    }
    x
}

/// `base` with the opened menu under its opener.
pub fn over<'a>(base: Element<'a, Msg>, s: &'a Studio) -> Element<'a, Msg> {
    let Some((menu, focus)) = s.menu else {
        return base;
    };
    let rows = items(menu, s);
    let mut list: Vec<Element<'a, Msg>> = Vec::new();
    for (i, r) in rows.into_iter().enumerate() {
        if r.sep {
            list.push(option_list::separator());
        }
        // the keys read left to right inside a right-to-left menu
        let hint = (!r.keys.is_empty()).then(|| format!("\u{2066}{}\u{2069}", r.keys));
        list.push(option_list::item(
            r.label,
            r.checked,
            hint,
            i == focus,
            Msg::MenuPick(menu, i),
        ));
    }
    let panel = option_list::menu(list);
    let x = offset(menu);
    let placed: Element<'a, Msg> = match dir::rtl() {
        false => pin(panel).x(x).y(HEADER_BELOW).into(),
        true => container(panel)
            .padding(Padding {
                top: 41.0,
                right: x,
                ..Padding::ZERO
            })
            .align_right(Fill)
            .into(),
    };
    stack![base, placed].into()
}

/// The keyboard on the bar: the new state and, for Enter, the row picked.
pub fn nav(s: &Studio, key: Nav) -> (Option<(Menu, usize)>, Option<Msg>) {
    let rows = |m| items(m, s).len();
    match (s.menu, key) {
        (None, Nav::Alt) => (Some((Menu::File, 0)), None),
        (Some(_), Nav::Alt) => (None, None),
        (None, _) => (None, None),
        (Some((m, _)), Nav::Prev) => (Some((m.step(-1), 0)), None),
        (Some((m, _)), Nav::Next) => (Some((m.step(1), 0)), None),
        (Some((m, i)), Nav::Up) => (Some((m, (i + rows(m) - 1) % rows(m))), None),
        (Some((m, i)), Nav::Down) => (Some((m, (i + 1) % rows(m))), None),
        (Some((m, i)), Nav::Enter) => (None, items(m, s).into_iter().nth(i).map(|r| r.press)),
    }
}
