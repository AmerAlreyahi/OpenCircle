//! The tray menu (Windows; the design's widgets/tray-menu.js): `platform::tray::entries` as
//! `kit::option_list` rows with the user's shortcuts as hints, a separator above Settings, a ✓ on
//! Live translate while it runs. Arrows walk the rows, Enter picks, Esc closes (`Menu::key`); the
//! window (`ui::windows`) closes it on focus loss, which an outside click is.
use iced::{Element, Size};

use crate::config::Config;
use crate::labs::Labs;
use crate::ui::kit::option_list::{MENU_WIDTH, item, menu, menu_height, separator};
use crate::ui::kit::tokens::radius;
use crate::ui::kit::tooltip::tip;
use crate::ui::platform::tray::{Entry, entries};

/// The corners the window is cut to (`platform::windows::floating`).
pub const RADIUS: f32 = radius::MENU;

/// The open menu: its rows and the keyboard's row.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    rows: Vec<(Entry, Option<String>)>,
    live_on: bool,
    focus: Option<usize>,
    /// The update row's text, while an update waits (`with_update`).
    update: Option<String>,
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
        let rows = (entries(labs, false).into_iter())
            .map(|e| (e, e.shortcut(cfg)))
            .collect();
        Menu {
            rows,
            live_on,
            focus: None,
            update: None,
        }
    }

    /// With the waiting update's row first ("Restart to update (0.1.5)").
    pub fn with_update(mut self, label: String) -> Menu {
        self.rows.insert(0, (Entry::Update, None));
        self.update = Some(label);
        self
    }

    /// A row's text.
    fn label(&self, entry: Entry) -> &str {
        match (entry, &self.update) {
            (Entry::Update, Some(label)) => label,
            _ => entry.label(),
        }
    }

    /// A separator goes above row `i`.
    fn separated(&self, i: usize) -> bool {
        i > 0 && self.rows[i].0.separated()
    }

    /// With the "Make pins clickable" row (a live pin lets clicks pass through), above Settings.
    pub fn with_click_pins(mut self) -> Menu {
        let at = (self.rows.iter().position(|(e, _)| *e == Entry::Settings)).unwrap_or(0);
        self.rows.insert(at, (Entry::ClickPins, None));
        self
    }

    /// The window's size (logical px).
    pub fn size(&self) -> Size {
        let separators = (0..self.rows.len()).filter(|i| self.separated(*i)).count();
        Size::new(MENU_WIDTH, menu_height(self.rows.len(), separators))
    }

    /// The entries shown, in order.
    #[cfg(test)]
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
        if m.separated(i) {
            rows.push(separator());
        }
        let checked = *entry == Entry::Live && m.live_on;
        let label = m.label(*entry);
        let row = item(
            label,
            checked,
            hint.clone(),
            m.focus == Some(i),
            Msg::Pick(i),
        );
        rows.push(tip(row, label, iced::widget::tooltip::Position::Left));
    }
    menu(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labs::Feature;
    use crate::ui::kit::testing::golden;

    fn all_on() -> Labs {
        let mut labs = Labs::default();
        for f in [Feature::Live, Feature::Song] {
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

    /// The window is as tall as its rows and the separator: 5 rows (Labs off:
    /// Snip is out of Labs) or 7.
    #[test]
    fn the_window_fits_the_rows() {
        let off = Menu::new(&Labs::default(), &Config::default(), false);
        assert_eq!(off.entries().len(), 5);
        assert_eq!(off.size(), Size::new(240.0, 10.0 + 5.0 * 32.0 + 9.0));
        let on = Menu::new(&all_on(), &Config::default(), false);
        assert_eq!(on.size().height, 10.0 + 7.0 * 32.0 + 9.0);
    }

    /// Arabic: the same rows from the table, mirrored (✓ and hints at the left).
    #[test]
    fn the_arabic_tray_menu_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            let _ar = crate::ui::kit::testing::arabic();
            let cfg = Config::default();
            let mut m = Menu::new(&all_on(), &cfg, true);
            m.update(Msg::Key(Key::Down));
            let m = m.with_update(crate::t!(tray_update).to_string());
            let file = format!("tray-menu-ar-{}", theme.id);
            failed.extend(golden::check("screens", &file, m.size(), &[1.0], || {
                view(&m)
            }));
        });
    }

    /// Labs off and on (live translate running: its ✓), Sapphire and Daylight, at 1x and 2x.
    #[test]
    fn the_tray_menu_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
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
            // an update waits: its row first, a separator under it
            let m = Menu::new(&Labs::default(), &Config::default(), false)
                .with_update("Restart to update (0.1.5)".into());
            assert_eq!(m.size().height, 10.0 + 6.0 * 32.0 + 2.0 * 9.0);
            let file = format!("tray-menu-update-{}", theme.id);
            failed.extend(golden::check(
                "screens",
                &file,
                m.size(),
                &[1.0, 2.0],
                || view(&m),
            ));
        });
    }
}
