//! The header's File / Edit / View menu bar (STUDIO_V3_SPEC §6, boards 6, 7,
//! 7b): a bar of three openers and the menu under the opened one, from the
//! kit's menu rows; Open recent and Arrange open a second menu beside it. Alt
//! opens File, arrows walk, Enter picks, Esc closes. The Edit rows are the
//! editor's own entries (`editing`), the same ones the right-click menu uses.
use iced::widget::{container, pin, stack};
use iced::{Center, Element, Fill, Padding};

use super::editing::{self, Action};
use super::keys;
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

/// A second menu beside a row of the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    Recent,
    Arrange,
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
pub struct Item<'a> {
    pub label: &'a str,
    pub keys: String,
    pub checked: bool,
    pub press: Msg,
    /// A hairline above it.
    pub sep: bool,
    /// It opens this second menu instead of acting.
    pub sub: Option<Sub>,
}

fn item<'a>(label: S, keys: &str, press: Msg, sep: bool) -> Item<'a> {
    Item {
        label: tr(label),
        keys: keys.to_string(),
        checked: false,
        press,
        sep,
        sub: None,
    }
}

fn opens<'a>(label: S, sub: Sub, sep: bool) -> Item<'a> {
    Item {
        sub: Some(sub),
        ..item(label, "", Msg::Menu(None), sep)
    }
}

/// The message of an editor menu action (Paste, Fit and the shortcuts sheet are
/// the Studio's own).
pub fn action_msg(a: Action) -> Msg {
    match a {
        Action::Paste => Msg::Paste,
        Action::Fit => Msg::Fit,
        Action::Shortcuts => Msg::Shortcuts(true),
        a => a.message().map_or(Msg::Menu(None), Msg::Editor),
    }
}

/// An editor action as a menu row.
fn action<'a>(s: &Studio, a: Action, sep: bool) -> Item<'a> {
    // the key is the editor's entry's own: one source for both menus
    let keys = s.editor.as_ref().map_or_else(String::new, |e| {
        let entries = editing::object_menu(e)
            .into_iter()
            .chain(editing::picture_menu(e));
        let found = entries.into_iter().find(|r| r.action == a);
        found.map_or_else(String::new, |r| super::sheets::chord_text(r.keys))
    });
    item(super::sheets::words(a), &keys, action_msg(a), sep)
}

/// The menu's rows (the panels' and preferences' checks follow the state).
pub fn items(menu: Menu, s: &Studio) -> Vec<Item<'_>> {
    let ed = Msg::Editor;
    match menu {
        Menu::File => vec![
            item(S::st_new_snip, keys::NEW_SNIP, Msg::NewSnip, false),
            item(S::st_open, keys::of(keys::Cmd::Open), Msg::Open, false),
            opens(S::st_open_recent, Sub::Recent, false),
            item(S::c_save, keys::SAVE, ed(Message::Save), true),
            item(
                S::st_save_as,
                keys::of(keys::Cmd::SaveAs),
                Msg::Export,
                false,
            ),
            item(S::st_export_walkthrough, "", Msg::ExportWalkthrough, false),
            item(
                S::st_copy_picture,
                keys::COPY_PICTURE,
                ed(Message::Copy),
                true,
            ),
            item(
                S::st_prefs,
                keys::of(keys::Cmd::Preferences),
                Msg::OpenPrefs,
                true,
            ),
            item(
                S::st_shortcuts,
                keys::of(keys::Cmd::Shortcuts),
                Msg::Shortcuts(true),
                false,
            ),
            item(
                S::st_close,
                keys::CLOSE,
                Msg::Caption(crate::shared::kit::header::Caption::Close),
                true,
            ),
        ],
        Menu::Edit => vec![
            item(S::sn_undo, keys::UNDO, ed(Message::Undo), false),
            item(S::sn_redo, keys::REDO, ed(Message::Redo), false),
            action(s, Action::Cut, true),
            action(s, Action::Copy, false),
            action(s, Action::Paste, false),
            item(S::st_paste_step, "", Msg::PasteStep, false),
            action(s, Action::Duplicate, false),
            action(s, Action::Delete, false),
            action(s, Action::SelectAll, true),
            item(S::st_deselect, keys::DESELECT, ed(Message::Deselect), false),
            opens(S::st_arrange, Sub::Arrange, true),
        ],
        Menu::View => {
            let p = s.panels;
            let mut v = vec![
                item(
                    S::st_steps,
                    keys::of(keys::Cmd::StepsSidebar),
                    Msg::Library,
                    false,
                ),
                item(
                    S::st_zoom_in,
                    keys::of(keys::Cmd::ZoomIn),
                    Msg::ZoomIn,
                    true,
                ),
                item(
                    S::st_zoom_out,
                    keys::of(keys::Cmd::ZoomOut),
                    Msg::ZoomOut,
                    false,
                ),
                item(S::st_fit, keys::of(keys::Cmd::Fit), Msg::Fit, false),
                item(
                    S::st_actual,
                    keys::of(keys::Cmd::Actual),
                    Msg::Actual,
                    false,
                ),
                item(S::st_snap, "", Msg::Snap, true),
                item(S::st_grid, "", Msg::Grid, false),
            ];
            v[0].checked = p.library;
            v[5].checked = s.snap;
            v[6].checked = s.grid;
            v
        }
    }
}

/// The second menu's rows.
pub fn sub_items(sub: Sub, s: &Studio) -> Vec<Item<'_>> {
    match sub {
        Sub::Arrange => [
            Action::Forward,
            Action::Backward,
            Action::Front,
            Action::Back,
        ]
        .into_iter()
        .map(|a| action(s, a, false))
        .collect(),
        Sub::Recent => s
            .recent
            .iter()
            .take(usize::from(
                crate::shared::config::current().studio.recent_files,
            ))
            .map(|(path, _)| Item {
                label: path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                keys: String::new(),
                checked: false,
                press: Msg::OpenRecent(path.clone()),
                sep: false,
                sub: None,
            })
            .chain((!s.recent.is_empty()).then(|| Item {
                label: tr(S::st_clear),
                keys: String::new(),
                checked: false,
                press: Msg::ClearRecent,
                sep: true,
                sub: None,
            }))
            .collect(),
    }
}

/// An opener's side padding in all (as the kit draws it), and the gap between openers.
const OPENER_PAD: f32 = 22.0;
const OPENER_GAP: f32 = 2.0;
/// The menu hangs under the header and its hairline; a second menu sits this far
/// from the first.
const HEADER_BELOW: f32 = 41.0;
const SUB_GAP: f32 = 4.0;
/// The menu's padding and a separator's height (as `option_list` draws them).
const MENU_PAD: f32 = 5.0;
const SEPARATOR_H: f32 = 9.0;

/// The three openers.
pub fn bar(s: &Studio) -> Element<'_, Msg> {
    let open = s.menu.map(|(m, _)| m);
    let row = Menu::ALL.map(|m| {
        let on = open == Some(m);
        side_panel::menu_opener(m.name(), on, Msg::Menu(if on { None } else { Some(m) }))
    });
    // the gear: Studio preferences, at the menus' end (the familiar spot for settings)
    let gear = crate::shared::kit::tooltip::hint(
        crate::shared::kit::icon_button::square(
            super::view::GEAR,
            crate::shared::kit::icon_button::State {
                active: s.prefs.is_some(),
                focused: false,
            },
            Some(Msg::OpenPrefs),
        ),
        crate::t!(pf_settings_tip),
        crate::shared::kit::tooltip::Why::Keys(keys::of(keys::Cmd::Preferences)),
        iced::widget::tooltip::Position::Bottom,
    );
    let mut parts: Vec<Element<'_, Msg>> = row.into_iter().collect();
    parts.push(gear);
    dir::row(parts).spacing(OPENER_GAP).align_y(Center).into()
}

/// Where the opener of `menu` starts, from the header's start edge.
fn offset(menu: Menu) -> f32 {
    let role = Role::NAV;
    let mut x = crate::shared::kit::header::menu_start(dir::rtl(), 3);
    for m in Menu::ALL {
        if m == menu {
            return x;
        }
        let w = text::width(m.name(), text::size(role), text::font(role));
        x += w.ceil() + OPENER_PAD + OPENER_GAP;
    }
    x
}

/// A menu's rows as the kit draws them.
fn panel<'a>(
    rows: Vec<Item<'a>>,
    focus: Option<usize>,
    pick: impl Fn(usize) -> Msg,
) -> Element<'a, Msg> {
    let mut list: Vec<Element<'a, Msg>> = Vec::new();
    for (i, r) in rows.into_iter().enumerate() {
        if r.sep {
            list.push(option_list::separator());
        }
        // the keys read left to right inside a right-to-left menu
        let hint = match (r.sub, r.keys.is_empty()) {
            (Some(_), _) => Some(if dir::rtl() { "‹" } else { "›" }.to_string()),
            (None, false) => Some(format!("\u{2066}{}\u{2069}", r.keys)),
            (None, true) => None,
        };
        list.push(option_list::item(
            r.label,
            r.checked,
            hint,
            focus == Some(i),
            pick(i),
        ));
    }
    option_list::menu(list)
}

/// `base` with the opened menu under its opener (and the second menu beside it).
pub fn over<'a>(base: Element<'a, Msg>, s: &'a Studio) -> Element<'a, Msg> {
    let Some((menu, focus)) = s.menu else {
        return base;
    };
    let rows = items(menu, s);
    // where the row that opens the second menu starts, down the first
    let sub_at = s.sub.and_then(|sub| {
        let at = rows.iter().position(|r| r.sub == Some(sub))?;
        let seps = rows[..=at].iter().filter(|r| r.sep).count() as f32;
        Some(MENU_PAD + at as f32 * option_list::ITEM_H + seps * SEPARATOR_H)
    });
    let first = panel(rows, Some(focus), move |i| Msg::MenuPick(menu, i));
    let x = offset(menu);
    let rise = s.rise();
    let place = |panel: Element<'a, Msg>, x: f32, y: f32| -> Element<'a, Msg> {
        let y = y + rise; // (opening: a little lower, rising to its place)
        match dir::rtl() {
            false => pin(panel).x(x).y(y).into(),
            true => container(panel)
                .padding(Padding {
                    top: y,
                    right: x,
                    ..Padding::ZERO
                })
                .align_right(Fill)
                .into(),
        }
    };
    // a press anywhere under the header (outside the menu) closes it
    let catch = |h: iced::Length| {
        iced::widget::mouse_area(iced::widget::Space::new().width(Fill).height(h))
            .on_press(Msg::Menu(None))
            .on_right_press(Msg::Menu(None))
    };
    // (the header row too, but never the caption buttons: they keep working)
    let outside = iced::widget::column![
        iced::widget::row![
            catch(HEADER_BELOW.into()),
            iced::widget::Space::new()
                .width(crate::shared::kit::tokens::size::CAPTION_BUTTON * 3.0)
                .height(HEADER_BELOW),
        ],
        catch(Fill),
    ];
    let mut layers = vec![base, outside.into(), place(first, x, HEADER_BELOW)];
    if let (Some(sub), Some(y)) = (s.sub, sub_at) {
        let second = panel(sub_items(sub, s), None, move |i| Msg::SubPick(sub, i));
        let beside = x + option_list::MENU_WIDTH + SUB_GAP;
        layers.push(place(second, beside, HEADER_BELOW + y));
    }
    stack(layers).into()
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
