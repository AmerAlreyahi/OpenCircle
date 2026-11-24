//! The right-click menu on selected text (#201, owner 2026-11-24): Copy, then Search the web / Ask
//! AI / Translate, then Select all text and, when the selection is a link or an address, its Open /
//! Send email row. The rows' keys and labels come from the action registry (`actions::ACTIONS`);
//! the list is `kit::option_list`'s menu. Arrows walk the rows, Enter runs, Esc closes (`Menu::key`);
//! `App` places it (`option_list::place`) and runs the row (`app::context_menu`).
use iced::{Element, Point, Size};

use crate::features::circle::actions::entities::{Entity, Kind};
use crate::features::circle::actions::{self, Action};
use crate::shared::app_language::{S, fmt, tr};
use crate::shared::kit::option_list::{MENU_WIDTH, item, menu, menu_height, separator};
use crate::shared::kit::tooltip::tip;

use super::bar::geometry::key_hint;

/// What a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Copy,
    Search,
    Ask,
    Translate,
    SelectAll,
    Open(Entity),
}

/// The open menu: where it is (logical px), its rows (the row, its text, its key) and the
/// keyboard's row.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub at: Point,
    rows: Vec<(Row, String, Option<String>)>,
    focus: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// A row was clicked.
    Pick(usize),
    /// A press outside the menu.
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Escape,
}

/// What the app does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Run(Row),
    Close,
}

fn keyed(action: &Action, label: S) -> (String, Option<String>) {
    let hint = key_hint(action.key);
    // the keys read left to right inside a right-to-left menu
    let hint = match crate::shared::kit::dir::rtl() {
        true => format!("\u{2066}{hint}\u{2069}"),
        false => hint,
    };
    (tr(label).to_string(), Some(hint))
}

impl Menu {
    /// The menu at `at`: `select_all` = not all the text is selected yet; `entity` = the link or
    /// address the selection is (its row names the host / address); `keyboard` = opened by a key
    /// (the first row is focused).
    pub fn new(at: Point, select_all: bool, entity: Option<Entity>, keyboard: bool) -> Menu {
        let mut rows = Vec::new();
        let (copy, ask) = (actions::by_id("copy"), actions::by_id("ai"));
        let (translate, search) = (actions::by_id("translate"), actions::by_id("lens"));
        for (row, action, label) in [
            (Row::Copy, copy, copy.label),
            (Row::Search, search, S::ctx_search),
            (Row::Ask, ask, ask.label),
            (Row::Translate, translate, translate.label),
        ] {
            let (text, hint) = keyed(action, label);
            rows.push((row, text, hint));
        }
        if select_all {
            rows.push((Row::SelectAll, tr(S::ctx_select_all).to_string(), None));
        }
        if let Some(e) = entity.filter(|e| e.kind != Kind::Phone) {
            let label = match e.kind {
                Kind::Email => fmt(S::ctx_send_email, &[&e.shown()]),
                _ => fmt(S::ctx_open_link, &[&e.shown()]),
            };
            rows.push((Row::Open(e), label, None));
        }
        Menu {
            at,
            rows,
            focus: keyboard.then_some(0),
        }
    }

    /// A separator goes above row `i`: Search starts the second group, the rows after Translate
    /// the third.
    fn separated(&self, i: usize) -> bool {
        i == 1 || i == 4
    }

    /// The menu's size (logical px).
    pub fn size(&self) -> Size {
        let separators = (0..self.rows.len()).filter(|i| self.separated(*i)).count();
        Size::new(MENU_WIDTH, menu_height(self.rows.len(), separators))
    }

    /// The rows, in order.
    #[cfg(test)]
    pub fn rows(&self) -> Vec<&Row> {
        self.rows.iter().map(|(r, ..)| r).collect()
    }

    /// A row's text.
    #[cfg(test)]
    pub fn label(&self, i: usize) -> &str {
        &self.rows[i].1
    }

    /// What a click does.
    pub fn update(&mut self, msg: Msg) -> Option<Outcome> {
        match msg {
            Msg::Pick(i) => self.rows.get(i).map(|(r, ..)| Outcome::Run(r.clone())),
            Msg::Close => Some(Outcome::Close),
        }
    }

    /// A key while it is open: the arrows move the focus (round the ends), Enter runs the focused
    /// row, Esc closes.
    pub fn key(&mut self, key: Key) -> Option<Outcome> {
        let last = self.rows.len().checked_sub(1)?;
        match key {
            Key::Escape => Some(Outcome::Close),
            Key::Enter => self.focus.map(|i| Outcome::Run(self.rows[i].0.clone())),
            Key::Down => {
                self.focus = Some(self.focus.map_or(0, |i| if i == last { 0 } else { i + 1 }));
                None
            }
            Key::Up => {
                self.focus = Some(
                    self.focus
                        .map_or(last, |i| i.checked_sub(1).unwrap_or(last)),
                );
                None
            }
        }
    }
}

pub fn view(m: &Menu) -> Element<'_, Msg> {
    let mut rows = Vec::new();
    for (i, (_, label, hint)) in m.rows.iter().enumerate() {
        if m.separated(i) {
            rows.push(separator());
        }
        let row = item(label, false, hint.clone(), m.focus == Some(i), Msg::Pick(i));
        rows.push(tip(row, label, iced::widget::tooltip::Position::Left));
    }
    menu(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::golden;

    fn link() -> Entity {
        Entity {
            kind: Kind::Link,
            text: "https://shop.example.com/track/4412".into(),
        }
    }

    fn menu_with(select_all: bool, entity: Option<Entity>) -> Menu {
        Menu::new(Point::ORIGIN, select_all, entity, false)
    }

    /// 201.1: the rows, in the owner's order, with the registry's keys.
    #[test]
    fn w98_201_1_the_rows_and_their_keys() {
        let m = menu_with(true, None);
        assert_eq!(
            m.rows(),
            [
                &Row::Copy,
                &Row::Search,
                &Row::Ask,
                &Row::Translate,
                &Row::SelectAll
            ]
        );
        let hints: Vec<_> = m.rows.iter().map(|r| r.2.as_deref()).collect();
        assert_eq!(
            hints,
            [Some("Ctrl+C"), Some("L"), Some("A"), Some("R"), None]
        );
        assert_eq!(m.label(1), "Search the web");
    }

    /// 201.2: Select all text only while not all is selected; the link row only for a link or
    /// address, named by its host / address (a phone number gets none).
    #[test]
    fn w98_201_2_select_all_and_the_link_row_are_conditional() {
        assert!(!menu_with(false, None).rows().contains(&&Row::SelectAll));
        let m = menu_with(false, Some(link()));
        assert_eq!(m.label(4), "Open shop.example.com");
        let mail = Entity {
            kind: Kind::Email,
            text: "me@example.com".into(),
        };
        assert_eq!(
            menu_with(false, Some(mail)).label(4),
            "Send email to me@example.com"
        );
        let phone = Entity {
            kind: Kind::Phone,
            text: "+1 555 0100".into(),
        };
        assert_eq!(menu_with(false, Some(phone)).rows().len(), 4);
    }

    /// 201.12: in Arabic the Latin host / address and the key caps are isolated, so they read
    /// right inside the right-to-left row.
    #[test]
    fn w98_201_12_arabic_isolates_the_host_and_the_keys() {
        let _ar = crate::shared::kit::testing::arabic();
        let m = menu_with(false, Some(link()));
        assert!(m.label(4).contains("\u{2068}shop.example.com\u{2069}"));
        let mail = Entity {
            kind: Kind::Email,
            text: "me@example.com".into(),
        };
        let m = menu_with(false, Some(mail));
        assert!(m.label(4).contains("\u{2068}me@example.com\u{2069}"));
        assert_eq!(m.rows[0].2.as_deref(), Some("\u{2066}Ctrl+C\u{2069}"));
    }

    /// 201.3: arrows walk and wrap, Enter runs the focused row, Esc closes, a click runs its row;
    /// opened by a key the first row is focused.
    #[test]
    fn w98_201_3_keyboard_walks_the_rows() {
        let mut m = menu_with(true, None);
        assert_eq!(m.key(Key::Enter), None, "no row focused yet");
        m.key(Key::Down);
        assert_eq!(m.key(Key::Enter), Some(Outcome::Run(Row::Copy)));
        m.key(Key::Up);
        assert_eq!(
            m.key(Key::Enter),
            Some(Outcome::Run(Row::SelectAll)),
            "wraps"
        );
        assert_eq!(m.key(Key::Escape), Some(Outcome::Close));
        assert_eq!(m.update(Msg::Pick(2)), Some(Outcome::Run(Row::Ask)));
        let keyed = Menu::new(Point::ORIGIN, true, None, true);
        assert_eq!(keyed.focus, Some(0));
    }

    /// 201.4: light and dark, Latin and Arabic, with and without the link row.
    #[test]
    fn w98_201_4_the_menu_matches_its_golden_images() {
        golden::each_theme(|theme, failed| {
            for (name, ar, entity) in [
                ("plain", false, None),
                ("link", false, Some(link())),
                ("plain-ar", true, None),
                ("link-ar", true, Some(link())),
            ] {
                let _arabic = ar.then(crate::shared::kit::testing::arabic);
                let mut m = menu_with(true, entity);
                m.key(Key::Down); // the first row focused
                let file = format!("context-menu-{name}-{}", theme.id);
                failed.extend(golden::check(
                    "screens",
                    &file,
                    m.size(),
                    &[1.0, 2.0],
                    || view(&m),
                ));
            }
        });
    }
}
