//! The result panel and card, and the crops they take; the AI chat that
//! stays open (`Resident`): over the overlay while it is open, else in its
//! own floating window docked at its side.

use std::time::Duration;

use iced::window;
use iced::{Point, Rectangle, Size, Task};

use crate::actions::{self, Crop, lens, stack_crops};

use crate::ui::app::background::background;
use crate::ui::app::keys;
use crate::ui::app::message::Message;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame as card;
use crate::ui::panel;
use crate::ui::platform::monitor::{Place, place_window};
use crate::ui::screens::settings;
use crate::ui::windows;

use super::App;

/// The AI chat once pinned (`panel::Panel::pinned`): it outlives the overlay
/// (`App::chat`) until its pin closes it. Never more than one.
pub(in crate::ui) struct Resident {
    /// Its panel number (`Message::Panel`).
    pub id: u32,
    pub panel: panel::Panel,
    /// Its own window while the overlay is closed.
    pub window: Option<window::Id>,
    /// The monitor it docks on: origin and size (logical px), taskbar inset.
    pub home: (Point, Size, f32),
    /// Where its window is (logical desktop px) and how often it moved: a drag
    /// ends when it stops moving (`ChatSettle`).
    pub at: Point,
    pub moves: u32,
    /// Its window's size (logical px).
    pub size: Size,
}

/// Test hook: the chat's own window is not hidden from screen capture.
const CAPTURABLE: &str = "OPENCIRCLE_TEST_CAPTURABLE";

/// A dragged chat window docks this long after its last move (the OS drags
/// it: no release is reported).
const SETTLE: Duration = Duration::from_millis(250);

impl App {
    pub(in crate::ui) fn open_panel(
        &mut self,
        kind: actions::Panel,
        crop: Crop,
        question: Option<String>,
    ) -> Task<Message> {
        // one chat: a question while one stays open goes into it, with this crop
        if let (actions::Panel::Ask, Some(chat)) = (kind, &mut self.chat) {
            let id = chat.id;
            return (chat.panel.ask_about(crop, question, self.o.include))
                .map(move |msg| Message::Panel(id, msg));
        }
        self.panels = self.panels.wrapping_add(1);
        let id = self.panels;
        let (panel, task) = panel::Panel::open(kind, crop, question, self.o.include);
        self.o.panel = Some((id, panel));
        self.keep_chat();
        task.map(move |msg| Message::Panel(id, msg))
    }

    /// The overlay's chat pinned itself: the App keeps it from now on.
    pub(in crate::ui) fn keep_chat(&mut self) {
        if !(self.o.panel.as_ref()).is_some_and(|(_, p)| p.pinned()) {
            return;
        }
        let Some((id, panel)) = self.o.panel.take() else {
            return;
        };
        crate::log!("[opencircle] AI chat pinned");
        self.chat = Some(Resident {
            id,
            panel,
            window: None,
            home: (self.o.origin, self.o.screen, self.o.work_inset),
            at: Point::ORIGIN,
            moves: 0,
            size: Size::ZERO,
        });
    }

    /// The pinned chat's pin: it closes (with its window, if any).
    pub(in crate::ui) fn drop_chat(&mut self) -> Task<Message> {
        let Some(chat) = self.chat.take() else {
            return Task::none();
        };
        crate::log!("[opencircle] AI chat unpinned and closed");
        match chat.window {
            Some(id) => {
                windows::forget(id);
                windows::close(id)
            }
            None => Task::none(),
        }
    }

    /// The overlay closes with a pinned chat: it goes on in its own window, at
    /// its dock (solid, always on top, cut to its corners, out of captures).
    pub(in crate::ui) fn float_chat(&mut self) -> Task<Message> {
        let at = self.chat_rect();
        let (origin, screen, inset, scale) = (
            self.o.origin,
            self.o.screen,
            self.o.work_inset,
            self.o.scale,
        );
        let Some(chat) = self.chat.as_mut().filter(|c| c.window.is_none()) else {
            return Task::none();
        };
        chat.home = (origin, screen, inset);
        chat.size = at.size();
        chat.at = origin + iced::Vector::new(at.x, at.y);
        let px = |v: f32| (v * scale).round();
        let place = Place {
            origin: (px(chat.at.x) as i32, px(chat.at.y) as i32),
            px: (px(at.width) as u32, px(at.height) as u32),
            scale,
        };
        let (id, open) = windows::floating(
            window::Settings {
                size: at.size(),
                position: window::Position::Specific(chat.at),
                visible: false, // placed, then faded in
                resizable: false,
                ..window::Settings::default()
            },
            card::CARD_RADIUS,
            windows::POPUP,
        );
        chat.window = Some(id);
        // (test hook: VM screenshots can see it)
        let hidden = std::env::var_os(CAPTURABLE).is_none();
        open.then(move |id| {
            place_window::<()>(id, place)
                .chain(windows::hide_from_capture(id, hidden))
                .chain(windows::appear(id, windows::POPUP))
        })
        .discard()
    }

    /// The overlay is up again: the chat is drawn on it, its window goes.
    pub(in crate::ui) fn unfloat_chat(&mut self) -> Task<Message> {
        match self.chat.as_mut().and_then(|c| c.window.take()) {
            Some(id) => {
                windows::forget(id);
                window::close(id)
            }
            None => Task::none(),
        }
    }

    /// The chat's window moved (dragged by the OS): it docks once it stops.
    pub(in crate::ui) fn chat_moved(&mut self, window: window::Id, at: Point) -> Task<Message> {
        let Some(chat) = self.chat.as_mut().filter(|c| c.window == Some(window)) else {
            return Task::none();
        };
        chat.at = at;
        chat.moves = chat.moves.wrapping_add(1);
        let moves = chat.moves;
        background(move |tx| {
            std::thread::sleep(SETTLE);
            let _ = tx.unbounded_send(Message::ChatSettle(moves));
        })
    }

    /// The drag is over: the window docks to the side its middle is on.
    pub(in crate::ui) fn chat_settle(&mut self, moves: u32) -> Task<Message> {
        let Some(chat) = self.chat.as_mut().filter(|c| c.moves == moves) else {
            return Task::none();
        };
        let Some(id) = chat.window else {
            return Task::none();
        };
        let (origin, screen, inset) = chat.home;
        let here = Rectangle::new(
            chat.at - iced::Vector::new(origin.x, origin.y),
            card::docked(screen, card::Side::Right, inset).size(),
        );
        let side = card::dock_side(here, screen);
        self.chat_side = Some(side);
        let dock = card::docked(screen, side, inset);
        let to = origin + iced::Vector::new(dock.x, dock.y);
        if (to.x - chat.at.x).abs() < 1.0 && (to.y - chat.at.y).abs() < 1.0 {
            return Task::none();
        }
        // ponytail: a jump to the dock, not the overlay's glide; animate the move if it shows
        window::move_to(id, to)
    }

    pub(in crate::ui) fn on_panel(&mut self, id: u32, msg: panel::Msg) -> Task<Message> {
        // Every opened page is held, whichever panel is showing now.
        if let panel::Msg::LensOpened(Ok(page)) = &msg {
            if self.o.frame.is_some() {
                self.o.lens_pages.push(page.clone());
                // The search is in the browser now: the overlay, its panel and
                // pills must not stay on screen behind or over it. Close.
                return self.close();
            } else {
                page.clone().release(); // the overlay closed meanwhile
            }
        }
        let resident = self.chat.as_ref().is_some_and(|c| c.id == id);
        let open = self.o.is_some();
        if !resident && !open {
            return Task::none(); // a late answer for a closed overlay's panel
        }
        let floating = self
            .chat
            .as_ref()
            .and_then(|c| c.window)
            .filter(|_| resident);
        let Some(panel) = (match resident {
            true => self.chat.as_mut().map(|c| &mut c.panel),
            false => (self.o.panel.as_mut())
                .filter(|(open, _)| *open == id)
                .map(|(_, p)| p),
        }) else {
            return Task::none();
        };
        match msg {
            // its own window: the OS moves it
            panel::Msg::Card(card::Event::Grab(card::Drag::Move)) if floating.is_some() => {
                return floating.map_or_else(Task::none, window::drag);
            }
            panel::Msg::Card(card::Event::Grab(how)) if open => {
                let chat = panel.kind() == actions::Panel::Ask;
                self.o.card_drag = Some((how, None));
                self.o.card_chat = chat;
                self.frame_times.borrow_mut().clear();
            }
            panel::Msg::Card(card::Event::Hover(on)) if open => self.o.card_hover = on,
            panel::Msg::Card(card::Event::Reset) => self.card = None,
            panel::Msg::Card(card::Event::Grab(_) | card::Event::Hover(_)) => {}
            panel::Msg::Search if open => return self.run(&lens::ACTION),
            panel::Msg::Search => {}
            // A card's focused field gave Ctrl+C back (nothing selected in it).
            panel::Msg::Key(keys::Keyed::Copy) if open => {
                return self.update(Message::Field(keys::Keyed::Copy));
            }
            panel::Msg::Key(_) => {}
            // pinned: its pin is the way out
            panel::Msg::Pin if resident => return self.drop_chat(),
            panel::Msg::OpenAiSettings => {
                let settings = self.windows.open(windows::Kind::Settings);
                let section =
                    self.windows
                        .update(windows::Msg::Settings(settings::Message::Section(
                            settings::Section::Ai,
                        )));
                return settings.chain(section).map(Message::Windows);
            }
            // Lens follow-up: hand the question over to Ask AI.
            panel::Msg::Submit if panel.kind() == actions::Panel::Lens => {
                let question = panel.take_input();
                if question.trim().is_empty() {
                    return Task::none();
                }
                let Some(crop) = self.crop() else {
                    return Task::none();
                };
                return self.open_panel(actions::Panel::Ask, crop, Some(question));
            }
            msg => {
                let task = panel.update(msg);
                // the chat's Include screen is the AI field's (one state)
                if let (Some(on), true) = (panel.include(), open) {
                    self.o.include = on;
                }
                self.keep_chat();
                return task.map(move |msg| Message::Panel(id, msg));
            }
        }
        Task::none()
    }

    /// The selection cut out of the frozen frame, in physical pixels; with the
    /// edits once it was edited (Ask AI then sees the annotations).
    pub(in crate::ui) fn crop(&self) -> Option<Crop> {
        if let Some(edit) = &self.o.edit {
            return Some(edit.image());
        }
        self.crop_of(self.crop_area()?)
    }

    /// What `crop` cuts out (logical px): every selection's union.
    pub(in crate::ui) fn crop_area(&self) -> Option<Rectangle> {
        let selection = self.o.selection?;
        Some((self.o.others.iter()).fold(selection.area, |all, (s, _)| all.union(&s.area)))
    }

    /// The pinned chat's spot (`Opening::chat_hole`) in the selection's crop px,
    /// if the crop meets it, on whichever screen (through desktop px, as the
    /// crop is cut: a crop across screens is one image); not on an edited capture.
    pub(in crate::ui) fn chat_hole(&self) -> Option<[u32; 4]> {
        if self.o.edit.is_some() || self.o.frame.is_none() {
            return None;
        }
        let area = self.crop_area()?;
        let origin = self.crop_origin(area)?;
        let size = self.crop_of(area).map(|c| (c.width, c.height))?;
        let (start, hole) = (self.o.place(), self.o.chat_hole?);
        let a = super::screens::to_desktop(start, hole.position());
        let b =
            super::screens::to_desktop(start, hole.position() + iced::Vector::from(hole.size()));
        // the edges round outwards
        let (x, y) = (a.x.floor(), a.y.floor());
        let rect = (
            x as i32,
            y as i32,
            (b.x.ceil() - x) as u32,
            (b.y.ceil() - y) as u32,
        );
        actions::qr::crop_rects(origin, size, &[rect])
            .first()
            .copied()
    }

    /// Every selection's crop, in the order made, stacked top to bottom on white
    /// (one image: Copy text reads them in that order, Ask AI sees them all).
    /// One selection: just its crop.
    // ponytail: one stacked image, not one image per selection: the Ask panel
    // (panel.rs) sends one; give `ai::Message` several images when it can.
    pub(in crate::ui) fn stacked(&self) -> Option<Crop> {
        if self.o.others.is_empty() || self.o.edit.is_some() {
            return self.crop();
        }
        let areas =
            (self.o.others.iter().map(|(s, _)| s.area)).chain(self.o.selection.map(|s| s.area));
        let crops: Vec<Crop> = areas.filter_map(|area| self.crop_of(area)).collect();
        stack_crops(&crops)
    }

    /// Where the result card shows: where the user left it, else away from
    /// the selection (`card::place`). The AI chat docks (`chat_at`).
    pub(in crate::ui) fn card_at(&self) -> Rectangle {
        let selection = self.o.selection.map(|s| self.o.view.rect_to_screen(s.area));
        if self.chatting() {
            return self.chat_at(selection);
        }
        card::place(self.o.screen, selection, self.card)
    }

    /// The AI chat is the card shown.
    pub(in crate::ui) fn chatting(&self) -> bool {
        (self.o.panel.as_ref()).is_some_and(|(_, p)| p.kind() == actions::Panel::Ask)
    }

    /// The AI chat's card: docked on the side the user last let it go on,
    /// else the side `card::place` picks (away from the `selection`); held
    /// off it while dragged, gliding back once let go (`motion::DOCK`).
    fn chat_at(&self, selection: Option<Rectangle>) -> Rectangle {
        let screen = self.o.screen;
        let side = self
            .chat_side
            .unwrap_or_else(|| card::dock_side(card::place(screen, selection, None), screen));
        let dock = card::docked(screen, side, self.o.work_inset);
        let at = match self.o.chat_glide {
            Some((_, from, t)) => from.position() + (dock.position() - from.position()) * t,
            None => dock.position() + self.o.chat_off,
        };
        // on whole px: a card moved by fractions of a px left thin ghosts of
        // its edge on the frame (VM, 2026-10-28)
        Rectangle::new(
            Point::new(at.x.round(), at.y.round()),
            iced::Size::new(dock.width.round(), dock.height.round()),
        )
    }

    /// The AI chat's card on the overlay (the pinned one or the overlay's).
    pub(in crate::ui) fn chat_rect(&self) -> Rectangle {
        self.chat_at(self.o.selection.map(|s| self.o.view.rect_to_screen(s.area)))
    }

    /// The chat card let go: it docks to the side its middle is on, gliding there.
    pub(in crate::ui) fn dock_chat(&mut self, now: std::time::Instant) {
        let at = self.chat_rect();
        self.chat_side = Some(card::dock_side(at, self.o.screen));
        self.o.chat_off = iced::Vector::ZERO;
        self.o.chat_glide = Some((now, at, 0.0));
    }

    /// How open the result card is (0..1; 0 before its first frame, 1 at once
    /// when the OS asks for no animations).
    pub(in crate::ui) fn card_grown(&self) -> f32 {
        match (&self.o.panel, self.o.card_open) {
            _ if motion::reduced() => 1.0,
            (Some((id, _)), Some((was, _, grown))) if *id == was => grown,
            _ => 0.0,
        }
    }

    /// The result card still opening (frames wanted).
    pub(in crate::ui) fn card_opening(&self) -> bool {
        self.o.frame.is_some() && self.o.panel.is_some() && self.card_grown() < 1.0
    }
}
