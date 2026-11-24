//! The right-click menu on selected text (#201): when it opens, what it answers on, and what its
//! rows do. The rows are the bar's own actions run the way their keys run them, so Copy, Search
//! and the link close the overlay and Ask AI and Translate keep it (owner, 2026-11-24).
use iced::{Point, Rectangle, Task};

use crate::app::App;
use crate::app::message::{Message, Purpose};
use crate::features::circle::actions::entities::{self, Kind};
use crate::features::circle::actions::{self, Action};
use crate::features::circle::screens::context_menu::{Key, Menu, Msg, Outcome, Row};
use crate::features::circle::screens::overlay::{self, Event};
use crate::shared::kit::option_list::place;

/// The menu keeps this far from the screen's edges.
const EDGE: f32 = 8.0;

impl App {
    /// The text a menu action works on: the picked words, else select-text mode's box.
    pub(crate) fn menu_text(&self) -> Option<String> {
        self.picked_text()
            .or_else(|| self.box_text())
            .filter(|t| !t.trim().is_empty())
    }

    /// Where a right press opens the menu (image px): the selected words' lines, or the
    /// select-text box with nothing picked in it. Nothing when it is not main mode's selection.
    pub(crate) fn menu_target(&self) -> Vec<Rectangle> {
        if !matches!(self.o.purpose, Purpose::Search)
            || self.o.edit.is_some()
            || self.o.translate.is_some()
        {
            return Vec::new();
        }
        if let Some(mode) = &self.o.text_mode {
            return match &mode.picked {
                Some(picked) if !mode.picking => picked.rects(),
                None if self.box_text().is_some() => vec![self.area()],
                _ => Vec::new(),
            };
        }
        (self.o.word.as_ref())
            .filter(|w| !w.is_empty() && self.o.selection.is_some())
            .map_or_else(Vec::new, |w| w.rects())
    }

    /// Opens the menu at `at` (screen px), or by a key at the selection's end.
    pub(crate) fn open_context(&mut self, at: Option<Point>) -> Task<Message> {
        let (target, Some(text)) = (self.menu_target(), self.menu_text()) else {
            return Task::none();
        };
        let keyboard = at.is_none();
        let at = at.unwrap_or_else(|| {
            let last = target.last().copied().unwrap_or_default();
            let r = self.o.view.rect_to_screen(last);
            Point::new(r.x, r.y + r.height)
        });
        let entity = entities::find(&text)
            .into_iter()
            .find(|e| e.kind != Kind::Phone);
        let mut menu = Menu::new(at, self.can_select_all(), entity, keyboard);
        menu.at = place(at, menu.size(), self.o.screen, EDGE);
        self.o.menu = Some(menu);
        Task::none()
    }

    /// Not all the text is selected yet.
    fn can_select_all(&self) -> bool {
        match &self.o.text_mode {
            Some(mode) => !mode.picked.as_ref().is_some_and(|p| p.is_all()),
            None => self.reads_text() && !self.o.word.as_ref().is_some_and(|w| w.is_all()),
        }
    }

    /// An overlay event while the menu is open: the keys drive it, a click anywhere else closes
    /// it and goes on as usual. `None`: the menu is not open.
    pub(crate) fn menu_event(&mut self, event: Event) -> Option<Task<Message>> {
        let menu = self.o.menu.as_mut()?;
        let key = match event {
            Event::Arrow { dy: -1, dx: 0, .. } => Key::Up,
            Event::Arrow { dy: 1, dx: 0, .. } => Key::Down,
            Event::Tab { back: true } => Key::Up,
            Event::Tab { back: false } => Key::Down,
            Event::Enter | Event::Space => Key::Enter,
            Event::ContextKey => Key::Escape,
            // the pointer moving and Alt change nothing in it
            Event::Hover(_) | Event::Move(_) | Event::Alt(_) => return Some(Task::none()),
            _ => {
                self.o.menu = None;
                return None; // a letter, a click, the wheel: as if it were closed
            }
        };
        let outcome = menu.key(key);
        Some(self.menu_outcome(outcome))
    }

    /// A row was clicked.
    pub(crate) fn on_context_menu(&mut self, msg: Msg) -> Task<Message> {
        let outcome = self.o.menu.as_mut().and_then(|m| m.update(msg));
        self.menu_outcome(outcome)
    }

    /// Esc.
    pub(crate) fn close_context(&mut self) -> bool {
        self.o.menu.take().is_some()
    }

    fn menu_outcome(&mut self, outcome: Option<Outcome>) -> Task<Message> {
        match outcome {
            Some(Outcome::Close) => {
                self.o.menu = None;
                Task::none()
            }
            Some(Outcome::Run(row)) => {
                self.o.menu = None;
                self.run_row(row)
            }
            None => Task::none(),
        }
    }

    /// The bar's key for `action`, pressed.
    fn press(&mut self, action: &Action) -> Task<Message> {
        let key = action.key;
        self.on_overlay(overlay::Event::Key {
            letter: key.letter,
            ctrl: key.ctrl,
        })
    }

    fn run_row(&mut self, row: Row) -> Task<Message> {
        match row {
            Row::Copy => self.press(actions::by_id("copy")),
            Row::Search => self.press(actions::by_id("lens")),
            Row::Translate => self.translate_picked(),
            Row::Ask => self.ask_selected_text(),
            Row::SelectAll => self.select_all_text(),
            Row::Open(entity) => self.open_entity(entity),
        }
    }

    /// Ask AI about the selected text: the text goes with the question, not the picture (the
    /// bar's Ask AI keeps sending the picture, 2026-11-08). No text: the picture, as the bar does.
    fn ask_selected_text(&mut self) -> Task<Message> {
        let Some(text) = self.menu_text() else {
            return self.run(actions::by_id("ai"));
        };
        let Some(crop) = self.crop() else {
            return Task::none();
        };
        self.ask_with_text(crop, crate::features::ask_ai::text_context(&text))
    }

    /// Ctrl+A's selection: all the screen's text, or all of select-text mode's box.
    fn select_all_text(&mut self) -> Task<Message> {
        if self.o.text_mode.is_some() {
            self.pick_all_in_box();
            return Task::none();
        }
        self.on_overlay(overlay::Event::Key {
            letter: 'a',
            ctrl: true,
        })
    }
}
