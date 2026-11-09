//! The result panel and card, and the crops they take; the AI chat that
//! stays open (`Resident`): over the overlay while it is open, else in its
//! own floating window docked at its side.

use std::time::Duration;

use iced::window;
use iced::{Point, Rectangle, Size, Task};

use crate::actions::{self, Crop, search_in_browser, stack_crops};
use crate::config::{self, Provider};

use crate::ui::ai_chat;
use crate::ui::app::background::background;
use crate::ui::app::keys;
use crate::ui::app::message::Message;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame as card;
use crate::ui::platform::monitor::{self, Place, place_window};
use crate::ui::screens::settings;
use crate::ui::windows;

use super::App;

/// The AI chat once pinned (`ai_chat::AiChat::pinned`): it outlives the overlay
/// (`App::chat`) until its pin closes it. Never more than one.
pub(in crate::ui) struct Resident {
    /// Its panel number (`Message::AiChat`).
    pub id: u32,
    pub panel: ai_chat::AiChat,
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
    /// Its window gliding to its dock: since when, from and to (desktop px).
    pub glide: Option<(std::time::Instant, Point, Point)>,
}

/// `t` (0..1, eased) of the way from `from` to `to`, on whole px; exactly
/// `to` at the end.
pub(in crate::ui) fn glided(from: Point, to: Point, t: f32) -> Point {
    let at = from + (to - from) * t;
    Point::new(at.x.round(), at.y.round())
}

/// A dragged widget's top-left `at` (`size`) held inside the `screen`: 6 px
/// from its left, right and top, 54 px from its bottom (design 3.4).
pub(in crate::ui) fn held_in(at: Point, size: Size, desk: Rectangle) -> Point {
    let (e, b) = (motion::DRAG_EDGE, motion::DRAG_EDGE_BOTTOM);
    let (right, bottom) = (desk.x + desk.width, desk.y + desk.height);
    Point::new(
        at.x.clamp(desk.x + e, (right - e - size.width).max(desk.x + e)),
        at.y.clamp(desk.y + e, (bottom - b - size.height).max(desk.y + e)),
    )
}

/// A dragged chat window docks this long after its last move (the OS drags
/// it: no release is reported).
const SETTLE: Duration = Duration::from_millis(250);

/// Ask AI only opens the browser: the provider is Google AI Mode (no key).
fn hands_off() -> bool {
    #[cfg(test)]
    if !HAND_OFF.get() {
        return false; // most tests drive the chat with the browser provider's setup
    }
    config::current().ai.provider == Provider::Browser
}

#[cfg(test)]
thread_local! {
    /// Tests: `hands_off` asks the config (else the chat opens, as with a key).
    pub(in crate::ui) static HAND_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl App {
    pub(in crate::ui) fn open_chat(
        &mut self,
        crop: Crop,
        question: Option<String>,
    ) -> Task<Message> {
        // one chat: a question while one stays open goes into it, with this crop
        if let Some(chat) = &mut self.chat {
            let id = chat.id;
            return (chat.panel.ask_about(crop, question, self.o.include))
                .map(move |msg| Message::AiChat(id, msg));
        }
        // No key (Google AI Mode): the browser opens, a pill says so, the
        // overlay closes; the chat is for an AI provider only.
        if hands_off() {
            let include = self.o.include;
            return self.confirm_after(move || ai_chat::hand_off(&crop, question, include));
        }
        self.panels = self.panels.wrapping_add(1);
        let id = self.panels;
        let (panel, task) = ai_chat::AiChat::open(crop, question, self.o.include);
        self.o.ai_chat = Some((id, panel));
        self.keep_chat();
        task.map(move |msg| Message::AiChat(id, msg))
    }

    /// The overlay's chat pinned itself: the App keeps it from now on.
    pub(in crate::ui) fn keep_chat(&mut self) {
        if !(self.o.ai_chat.as_ref()).is_some_and(|(_, p)| p.pinned()) {
            return;
        }
        let Some((id, panel)) = self.o.ai_chat.take() else {
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
            glide: None,
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
        // on the screen it was docked on (its px, its scale)
        let (origin, screen, inset, scale, at) =
            match self.o.chat_on.and_then(|i| self.o.screens.get(i)) {
                Some(s) => {
                    let (_, _, from, k) = self.chat_room();
                    let local = Point::ORIGIN + (at.position() - from) * k;
                    let at = Rectangle::new(local, at.size() * k);
                    (
                        s.place.logical_origin(),
                        s.place.size(),
                        s.inset,
                        s.place.scale,
                        at,
                    )
                }
                None => (
                    self.o.origin,
                    self.o.screen,
                    self.o.work_inset,
                    self.o.scale,
                    at,
                ),
            };
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
        open.then(move |id| {
            place_window::<()>(id, place).chain(windows::appear(id, windows::POPUP))
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

    /// The drag is over: its window's scale now says which monitor it is on.
    pub(in crate::ui) fn chat_settle(&mut self, moves: u32) -> Task<Message> {
        match self.chat.as_ref().filter(|c| c.moves == moves) {
            Some(Resident {
                window: Some(id), ..
            }) => window::scale_factor(*id).map(move |k| Message::ChatDocked(moves, k)),
            _ => Task::none(),
        }
    }

    /// The window docks to the side its middle is on, on the monitor of
    /// `monitors` holding it (`scale`: its window's).
    pub(in crate::ui) fn chat_docked(
        &mut self,
        moves: u32,
        scale: f32,
        monitors: &[Place],
    ) -> Task<Message> {
        let Some(chat) = self.chat.as_mut().filter(|c| c.moves == moves) else {
            return Task::none();
        };
        let Some(id) = chat.window else {
            return Task::none();
        };
        if let Some(m) = monitor::holding(monitors, chat.at, chat.size, scale) {
            let (origin, size) = (m.logical_origin(), m.size());
            chat.home = (origin, size, monitor::taskbar_inset(origin, size, m.scale));
        }
        let (origin, screen, inset) = chat.home;
        let here = Rectangle::new(chat.at - iced::Vector::new(origin.x, origin.y), chat.size);
        let side = card::dock_side(here, screen);
        self.chat_side = Some(side);
        let dock = self.chat_dock(screen, side, inset);
        let Some(chat) = self.chat.as_mut() else {
            return Task::none();
        };
        let to = origin + iced::Vector::new(dock.x.round(), dock.y.round());
        if (to.x - chat.at.x).abs() < 1.0 && (to.y - chat.at.y).abs() < 1.0 {
            return Task::none();
        }
        if motion::reduced() {
            return window::move_to(id, to);
        }
        // the overlay card's glide (`motion::DOCK`), the window moved frame by frame
        chat.glide = Some((std::time::Instant::now(), chat.at, to));
        self.frame_times.borrow_mut().clear(); // its frames, logged at its end
        Task::none()
    }

    /// A frame of the pinned chat window's glide to its dock: where it is now.
    pub(in crate::ui) fn chat_glide(&mut self, now: std::time::Instant) -> Task<Message> {
        let Some(chat) = self.chat.as_mut() else {
            return Task::none();
        };
        let (Some((began, from, to)), Some(id)) = (chat.glide, chat.window) else {
            return Task::none();
        };
        let t = motion::progress(began, now, motion::DOCK, motion::Curve::Standard);
        let at = glided(from, to, t);
        chat.glide = (t < 1.0).then_some((began, from, to));
        chat.at = at;
        // when each step ran (the overlay is closed: its frame log is the glide's)
        self.frame_times
            .borrow_mut()
            .push(std::time::Instant::now());
        if t >= 1.0 {
            self.log_frames("chat glide");
        }
        window::move_to(id, at)
    }

    pub(in crate::ui) fn on_ai_chat(&mut self, id: u32, msg: ai_chat::Msg) -> Task<Message> {
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
            false => (self.o.ai_chat.as_mut())
                .filter(|(open, _)| *open == id)
                .map(|(_, p)| p),
        }) else {
            return Task::none();
        };
        match msg {
            // its own window: the OS moves it
            ai_chat::Msg::Card(card::Event::Grab(card::Drag::Move)) if floating.is_some() => {
                return floating.map_or_else(Task::none, window::drag);
            }
            ai_chat::Msg::Card(card::Event::Grab(how)) if open => {
                panel.held = how == card::Drag::Move; // picked up (design 3.4)
                self.o.chat_drag = Some((how, None));
                self.o.chat_docks = true;
                self.frame_times.borrow_mut().clear();
            }
            ai_chat::Msg::Card(card::Event::Hover(on)) if open => self.o.chat_hover = on,
            ai_chat::Msg::Card(card::Event::Reset) => (self.card, self.chat_width) = (None, None),
            ai_chat::Msg::Card(card::Event::Grab(_) | card::Event::Hover(_)) => {}
            ai_chat::Msg::Search if open => return self.run(&search_in_browser::ACTION),
            ai_chat::Msg::Search => {}
            // A card's focused field gave Ctrl+C back (nothing selected in it).
            ai_chat::Msg::Key(keys::Keyed::Copy) if open => {
                return self.update(Message::Field(keys::Keyed::Copy));
            }
            ai_chat::Msg::Key(_) => {}
            // pinned: its pin is the way out
            ai_chat::Msg::Pin if resident => return self.drop_chat(),
            ai_chat::Msg::OpenAiSettings => {
                let settings = self.windows.open(windows::Kind::Settings);
                let section =
                    self.windows
                        .update(windows::Msg::Settings(settings::Message::Section(
                            settings::Section::Ai,
                        )));
                return settings.chain(section).map(Message::Windows);
            }
            msg => {
                let task = panel.update(msg);
                // the chat's Include screen is the AI field's (one state)
                if let (Some(on), true) = (panel.include(), open) {
                    self.o.include = on;
                }
                self.keep_chat();
                return task.map(move |msg| Message::AiChat(id, msg));
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
    // ponytail: one stacked image, not one image per selection: the AI chat
    // sends one; give `ai::Message` several images when it can.
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
    pub(in crate::ui) fn chat_card_at(&self) -> Rectangle {
        let selection = self.o.selection.map(|s| self.o.view.rect_to_screen(s.area));
        if self.chatting() {
            return self.chat_at(selection);
        }
        let at = card::place(self.o.screen, selection, self.card);
        if self.o.translate.is_some() {
            return crate::ui::screens::translate::view::clear_of_pill(at, self.o.screen);
        }
        at
    }

    /// The AI chat is the card shown.
    pub(in crate::ui) fn chatting(&self) -> bool {
        self.o.ai_chat.is_some()
    }

    /// The AI chat's card: docked on the side the user last let it go on,
    /// else the side `card::place` picks (away from the `selection`); held
    /// off it while dragged, gliding back once let go (`motion::DOCK`).
    fn chat_at(&self, selection: Option<Rectangle>) -> Rectangle {
        let (screen, inset, origin, k) = self.chat_room();
        let selection = selection.filter(|_| self.o.chat_on.is_none());
        let side = self
            .chat_side
            .unwrap_or_else(|| card::dock_side(card::place(screen, selection, None), screen));
        // docked in its screen's own px, shown in the starting screen's
        let dock = self.chat_dock(screen, side, inset);
        let dock = Rectangle::new(
            origin + (dock.position() - Point::ORIGIN) * (1.0 / k),
            dock.size() * (1.0 / k),
        );
        let (at, size) = match self.o.chat_glide {
            Some((_, from, t)) => (
                from.position() + (dock.position() - from.position()) * t,
                dock.size(),
            ),
            None => {
                // held whole on the screen holding most of it (never across a
                // bezel: one window draws it), shorter if that one is
                let held = Rectangle::new(dock.position() + self.o.chat_off, dock.size());
                let on = self.screen_holding(held);
                let area = self.screen_area(on);
                let (e, b) = (motion::DRAG_EDGE, motion::DRAG_EDGE_BOTTOM);
                let size = match on == self.o.chat_on {
                    true => dock.size(),
                    false => Size::new(
                        dock.width.min(area.width - 2.0 * e).max(0.0),
                        dock.height.min(area.height - e - b).max(0.0),
                    ),
                };
                let at = held_in(held.position(), size, area);
                // too tall to move up or down inside the edges: only across,
                // never a jump up when picked up
                match dock.height + e + b > screen.height / k {
                    true => (
                        Point::new(
                            at.x,
                            dock.y.clamp(area.y, area.y + area.height - size.height),
                        ),
                        size,
                    ),
                    false => (at, size),
                }
            }
        };
        // on whole px: a card moved by fractions of a px left thin ghosts of
        // its edge on the frame (VM, 2026-10-28)
        Rectangle::new(
            Point::new(at.x.round(), at.y.round()),
            iced::Size::new(size.width.round(), size.height.round()),
        )
    }

    /// Where the chat docks: the screen's logical size, its taskbar inset, its
    /// origin in the starting screen's px and its scale against that (`k`).
    pub(in crate::ui) fn chat_room(&self) -> (Size, f32, Point, f32) {
        let other = (self.o.chat_on).and_then(|i| self.work_areas().get(i + 1).copied());
        match other {
            Some((work, k)) => (
                Size::new(work.width * k, work.height * k),
                0.0, // (a work area: the taskbar is out of it)
                work.position(),
                k,
            ),
            None => (self.o.screen, self.o.work_inset, Point::ORIGIN, 1.0),
        }
    }

    /// The window drawing the AI chat's card: the screen holding most of it.
    pub(in crate::ui) fn chat_window(&self) -> window::Id {
        self.window_of(self.screen_holding(self.chat_rect()))
    }

    /// The chat docked on `side` of a `screen` (taskbar `inset`), as wide as
    /// the user made it; full height (the bar moves aside, `bar_room`).
    pub(in crate::ui) fn chat_dock(&self, screen: Size, side: card::Side, inset: f32) -> Rectangle {
        card::docked(screen, side, inset, self.chat_width)
    }

    /// The AI chat's card on the overlay (the pinned one or the overlay's).
    pub(in crate::ui) fn chat_rect(&self) -> Rectangle {
        self.chat_at(self.o.selection.map(|s| self.o.view.rect_to_screen(s.area)))
    }

    /// The chat card let go: it docks to the side its middle is on, gliding there.
    pub(in crate::ui) fn dock_chat(&mut self, now: std::time::Instant) {
        let at = self.chat_rect();
        self.o.chat_on = self.screen_holding(at); // dropped across a bezel: the larger part
        let (screen, _, origin, k) = self.chat_room();
        let local = Rectangle::new(Point::ORIGIN + (at.position() - origin) * k, at.size() * k);
        self.chat_side = Some(card::dock_side(local, screen));
        self.o.chat_off = iced::Vector::ZERO;
        self.o.chat_glide = Some((now, at, 0.0));
    }

    /// How open the result card is (0..1; 0 before its first frame, 1 at once
    /// when the OS asks for no animations).
    pub(in crate::ui) fn chat_grown(&self) -> f32 {
        match (&self.o.ai_chat, self.o.chat_open) {
            _ if motion::reduced() => 1.0,
            (Some((id, _)), Some((was, _, grown))) if *id == was => grown,
            _ => 0.0,
        }
    }

    /// The result card still opening (frames wanted).
    pub(in crate::ui) fn chat_opening(&self) -> bool {
        self.o.frame.is_some() && self.o.ai_chat.is_some() && self.chat_grown() < 1.0
    }
}
