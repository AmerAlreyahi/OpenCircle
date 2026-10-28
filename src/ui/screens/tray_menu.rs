//! The tray menu (Windows; the design's widgets/tray-menu.js): `platform::tray::entries` as
//! `kit::option_list` rows with the user's shortcuts as hints, a separator above Settings, a ✓ on
//! Live translate while it runs. Arrows walk the rows, Enter picks, Esc closes (`Menu::key`); the
//! window (`ui::windows`) closes it on focus loss, which an outside click is.
use iced::{Element, Size};

use crate::config::Config;
use crate::labs::Labs;
use crate::ui::kit::option_list::{MENU_WIDTH, item, menu, menu_height, separator};
use crate::ui::kit::tokens::radius;
use crate::ui::platform::tray::{Entry, entries};
use crate::ui::widgets::tip;

/// The corners the window is cut to (`platform::windows::floating`).
pub const RADIUS: f32 = radius::MENU;

/// The open menu: its rows and the keyboard's row.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    rows: Vec<(Entry, Option<String>)>,
    live_on: bool,
    focus: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// A row was clicked.
    Pick(usize),
    Key(Key),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Escape,
}

/// What the window does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Run(Entry),
    Close,
}

impl Menu {
    /// The rows for `labs`, hints from `cfg`; `live_on` = live translate runs now (its ✓).
    pub fn new(labs: &Labs, cfg: &Config, live_on: bool) -> Menu {
        let rows = (entries(labs).into_iter())
            .map(|e| (e, e.shortcut(cfg)))
            .collect();
        Menu {
            rows,
            live_on,
            focus: None,
        }
    }

    /// The window's size (logical px).
    pub fn size(&self) -> Size {
        let separators = self.rows.iter().filter(|(e, _)| e.separated()).count();
        Size::new(MENU_WIDTH, menu_height(self.rows.len(), separators))
    }

    /// The entries shown, in order.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn entries(&self) -> Vec<Entry> {
        self.rows.iter().map(|(e, _)| *e).collect()
    }

    /// A message from the window: what to do, if anything (the arrows only move the focus, round
    /// the ends; Enter without a focused row does nothing).
    pub fn update(&mut self, msg: Msg) -> Option<Outcome> {
        let last = self.rows.len().checked_sub(1)?;
        match msg {
            Msg::Pick(i) => self.rows.get(i).map(|(e, _)| Outcome::Run(*e)),
            Msg::Key(Key::Escape) => Some(Outcome::Close),
            Msg::Key(Key::Enter) => self.focus.map(|i| Outcome::Run(self.rows[i].0)),
            Msg::Key(Key::Down) => {
                self.focus = Some(self.focus.map_or(0, |i| if i == last { 0 } else { i + 1 }));
                None
            }
            Msg::Key(Key::Up) => {
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
    for (i, (entry, hint)) in m.rows.iter().enumerate() {
        if entry.separated() {
            rows.push(separator());
        }
        let checked = *entry == Entry::Live && m.live_on;
        let row = item(
            entry.label(),
            checked,
            hint.clone(),
            m.focus == Some(i),
            Msg::Pick(i),
        );
        rows.push(tip(
            row,
            entry.label(),
            iced::widget::tooltip::Position::Left,
        ));
    }
    menu(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labs::Feature;
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    fn all_on() -> Labs {
        let mut labs = Labs::default();
        for f in [Feature::Snip, Feature::Live, Feature::Song] {
            labs.set(f, true);
        }
        labs
    }

    /// Down from nothing is the first row, Up the last; both wrap; Enter runs the focused row (none:
    /// nothing), Esc closes, a click runs its row.
    #[test]
    fn the_keyboard_walks_the_rows() {
        let mut m = Menu::new(&Labs::default(), &Config::default(), false);
        assert_eq!(m.update(Msg::Key(Key::Enter)), None, "no row focused yet");
        assert_eq!(m.update(Msg::Key(Key::Down)), None);
        assert_eq!(
            m.update(Msg::Key(Key::Enter)),
            Some(Outcome::Run(Entry::Circle))
        );
        m.update(Msg::Key(Key::Down));
        assert_eq!(
            m.update(Msg::Key(Key::Enter)),
            Some(Outcome::Run(Entry::Translate))
        );
        m.update(Msg::Key(Key::Down));
        m.update(Msg::Key(Key::Down));
        assert_eq!(
            m.update(Msg::Key(Key::Enter)),
            Some(Outcome::Run(Entry::Quit))
        );
        m.update(Msg::Key(Key::Down)); // round to the top
        assert_eq!(
            m.update(Msg::Key(Key::Enter)),
            Some(Outcome::Run(Entry::Circle))
        );
        m.update(Msg::Key(Key::Up)); // and back to the end
        assert_eq!(
            m.update(Msg::Key(Key::Enter)),
            Some(Outcome::Run(Entry::Quit))
        );
        let mut m = Menu::new(&Labs::default(), &Config::default(), false);
        m.update(Msg::Key(Key::Up));
        assert_eq!(
            m.update(Msg::Key(Key::Enter)),
            Some(Outcome::Run(Entry::Quit))
        );
        assert_eq!(m.update(Msg::Key(Key::Escape)), Some(Outcome::Close));
        assert_eq!(m.update(Msg::Pick(1)), Some(Outcome::Run(Entry::Translate)));
        assert_eq!(m.update(Msg::Pick(9)), None);
    }

    /// The window is as tall as its rows and the separator: 4 rows (Labs off) or 7.
    #[test]
    fn the_window_fits_the_rows() {
        let off = Menu::new(&Labs::default(), &Config::default(), false);
        assert_eq!(off.entries().len(), 4);
        assert_eq!(off.size(), Size::new(240.0, 10.0 + 4.0 * 32.0 + 9.0));
        let on = Menu::new(&all_on(), &Config::default(), false);
        assert_eq!(on.size().height, 10.0 + 7.0 * 32.0 + 9.0);
    }

    /// Labs off and on (live translate running: its ✓), Sapphire and Daylight, at 1x and 2x.
    #[test]
    fn the_tray_menu_matches_its_golden_image() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for (name, labs, live) in [("off", Labs::default(), false), ("on", all_on(), true)] {
                let mut m = Menu::new(&labs, &Config::default(), live);
                m.update(Msg::Key(Key::Down)); // the first row focused
                let file = format!("tray-menu-labs-{name}-{}", theme.id);
                failed.extend(golden::check(
                    "screens",
                    &file,
                    m.size(),
                    &[1.0, 2.0],
                    || view(&m),
                ));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
