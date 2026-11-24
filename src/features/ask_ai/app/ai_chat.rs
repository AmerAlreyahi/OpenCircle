//! The result panel and card, and the crops they take; the AI chat that
//! stays open (`Resident`): over the overlay while it is open, else in its
//! own floating window docked at its side.

use std::time::Duration;

use iced::window;
use iced::{Point, Rectangle, Size, Task};

use crate::features::circle::actions::{self, search_in_browser};
use crate::shared::config::{self, Provider};

use crate::app::background::background;
use crate::app::keys;
use crate::app::message::{Message, Purpose};
use crate::app::windows;
use crate::features::ask_ai::screens::ai_chat;
use crate::features::settings;
use crate::shared::kit::motion;
use crate::shared::kit::panel_frame as card;
use crate::shared::platform::monitor::{self, Place, place_window};
use crate::shared::platform::shape::Anchor;

use crate::app::App;
use crate::shared::picture::{Crop, stack_crops};

/// Where a chat window goes: its monitor (origin, size, taskbar inset), its
/// top-left and size (logical desktop px), and the same in physical px.
type Placing = ((Point, Size, f32), Point, Size, Place);

/// The AI chat once pinned (`ai_chat::AiChat::pinned`): it outlives the overlay
/// (`App::chat`) until its pin closes it. Never more than one.
pub(crate) struct Resident {
    /// Its panel number (`Message::AiChat`).
    pub id: u32,
    pub panel: ai_chat::AiChat,
}

/// The AI chat's own window (`App::chat_win`): a floating one (cut to its
/// corners, on top, out of captures). Where windows hide (Windows, X11:
/// `App::own_window`) it is THE chat's window from its first frame to its
/// close: above the overlay while that is open (`windows::keep_above`), never
/// re-created when the overlay opens or closes (#170). Wayland: only while the
/// overlay is closed (the overlay draws the chat while it is open).
pub(crate) struct ChatWindow {
    pub id: window::Id,
    /// The monitor it docks on: origin and size (logical px), taskbar inset.
    pub home: (Point, Size, f32),
    /// Where it is (logical desktop px) and how often it moved: a drag
    /// ends when it stops moving (`ChatSettle`).
    pub at: Point,
    pub moves: u32,
    /// Its size (logical px).
    pub size: Size,
    /// Gliding to its dock: since when, from and to (desktop px).
    pub glide: Option<(std::time::Instant, Point, Point)>,
    /// Where the overlay last put it (physical px): it moves only when that
    /// changes (`sync_chat_window`).
    pub placed: Option<Place>,
    /// Hidden for now: Snip is open (its frozen screen holds the chat's
    /// picture, and nothing of ours covers Snip's outline), or `out`.
    /// Windows: clear (opacity 0), not hidden, so it comes back at once.
    pub hidden: bool,
    /// X11: out of a capture's grab, until it is over (`App::grabbed`).
    pub out: bool,
    /// Its inner edge held (W61): its window, sized once to the widest the
    /// drag can reach (`Wide`), shows only the panel (`windows::panel_cut`).
    pub wide: Option<Wide>,
}

/// The chat's window during an inner-edge drag: its outer edge where the
/// panel's is, its inner one as far as the drag can go. Each step only the
/// cut and the panel inside change: the window neither moves nor resizes,
/// so the outer edge, the border and the icons stay still (W61).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Wide {
    /// The window (physical px).
    pub place: Place,
    /// Its top-left (logical desktop px) and size (logical px).
    pub at: Point,
    pub size: Size,
    /// The panel's side of it (its outer edge).
    pub anchor: Anchor,
}

impl ChatWindow {
    /// Where its window really is (physical px): the wide one while its edge is held.
    pub fn window_place(&self) -> Option<Place> {
        self.wide.map(|w| w.place).or(self.placed)
    }

    /// The panel inside its window (that window's logical px), and the window's size.
    pub fn inside(&self) -> (Rectangle, Size) {
        match self.wide {
            Some(wide) => (
                Rectangle::new(Point::ORIGIN + (self.at - wide.at), self.size),
                wide.size,
            ),
            None => (Rectangle::with_size(self.size), self.size),
        }
    }

    /// Its window put where the panel is (`placed`): while its inner edge is
    /// held (`resizing`) the window is made wide once, then only cut to the
    /// panel; let go, it takes the panel's size once.
    fn put(&mut self, resizing: bool) -> Task<Message> {
        let Some(place) = self.placed else {
            return Task::none();
        };
        if !resizing {
            return match self.wide.take() {
                // sized to the panel while still cut to it (the cut follows the
                // size at once: the panel's part of the panel = all of it), then
                // the panel cut dropped: no frame shows the wide window
                Some(_) => place_window(self.id, place).chain(windows::panel_cut(self.id, None)),
                None => place_window(self.id, place),
            };
        }
        let first = self.wide.is_none();
        let wide = *self.wide.get_or_insert_with(|| {
            let (origin, screen, _) = self.home;
            let here = Rectangle::new(self.at - (origin - Point::ORIGIN), self.size);
            let (x0, y0, x1, y1) = (here.x, here.y, here.x + here.width, here.y + here.height);
            let (r, anchor) = match card::side(here, screen) {
                card::Side::Left => {
                    let x = x0.min(card::GAP);
                    ((x, y0, x1, y1), Anchor::Right)
                }
                card::Side::Right => ((x0, y0, x1.max(screen.width - card::GAP), y1), Anchor::Left),
                card::Side::Top => ((x0, y0.min(card::GAP), x1, y1), Anchor::Bottom),
            };
            let at = origin + iced::Vector::new(r.0, r.1);
            let size = Size::new(r.2 - r.0, r.3 - r.1);
            Wide {
                place: place_px(at, size, place.scale),
                at,
                size,
                anchor,
            }
        });
        let extent = match wide.anchor {
            Anchor::Bottom => place.px.1,
            _ => place.px.0,
        };
        let cut = windows::panel_cut(self.id, Some((wide.anchor, extent)));
        match first {
            true => cut.chain(place_window(self.id, wide.place)),
            false => cut,
        }
    }
}

/// `t` (0..1, eased) of the way from `from` to `to`, on whole px; exactly
/// `to` at the end.
pub(crate) fn glided(from: Point, to: Point, t: f32) -> Point {
    let at = from + (to - from) * t;
    Point::new(at.x.round(), at.y.round())
}

/// A window at `at` (logical desktop px), `size` big, in physical px at
/// `scale`: each EDGE rounded, so an edge that stays put in logical px stays
/// put on the screen while the other one moves (rounding the size would move
/// it by a px every other step at 125 %).
pub(crate) fn place_px(at: Point, size: Size, scale: f32) -> Place {
    let px = |v: f32| (v * scale).round();
    let (x0, y0) = (px(at.x), px(at.y));
    let (x1, y1) = (px(at.x + size.width), px(at.y + size.height));
    Place {
        origin: (x0 as i32, y0 as i32),
        px: ((x1 - x0).max(0.0) as u32, (y1 - y0).max(0.0) as u32),
        scale,
    }
}

/// The pointer on the desktop (physical px) while it drags in the chat's own
/// window placed at `placed` (`p`: in that window's px): never its px in a
/// window that moves under it.
pub(crate) fn desk_pointer(placed: Place, p: Point) -> Point {
    #[cfg(not(test))]
    let pointer = crate::shared::platform::capture::pointer();
    #[cfg(test)]
    let pointer = None::<(i32, i32)>; // (not the test machine's)
    pointer.map_or_else(
        || {
            let (x, y) = placed.origin;
            Point::new(x as f32 + p.x * placed.scale, y as f32 + p.y * placed.scale)
        },
        |(x, y)| Point::new(x as f32, y as f32),
    )
}

/// A dragged widget's top-left `at` (`size`) held inside the `screen`: 6 px
/// from its left, right and top, 54 px from its bottom (design 3.4).
pub(crate) fn held_in(at: Point, size: Size, desk: Rectangle) -> Point {
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

/// How long the chat's new window paints before it takes over from the overlay.
const HANDOVER_BEAT: Duration = Duration::from_millis(60);

/// Ask AI only opens the browser: the provider is Google AI Mode (no key).
pub(crate) fn hands_off() -> bool {
    #[cfg(test)]
    if !HAND_OFF.get() {
        return false; // most tests drive the chat with the browser provider's setup
    }
    config::current().ai.provider == Provider::Browser
}

#[cfg(test)]
thread_local! {
    /// Tests: `hands_off` asks the config (else the chat opens, as with a key).
    pub(crate) static HAND_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl App {
    pub(crate) fn open_chat(&mut self, crop: Crop, question: Option<String>) -> Task<Message> {
        self.open_chat_with(crop, question, self.o.include)
    }

    /// Ask AI about selected `text` (the right-click menu's): the chat opens with the text in the
    /// composer, no picture and no question of ours sent; the user types the question. The browser
    /// provider has no composer: the text goes to it as it is.
    pub(crate) fn ask_with_text(&mut self, crop: Crop, text: String) -> Task<Message> {
        if hands_off() {
            return self.open_chat_with(crop, Some(text), false);
        }
        let task = self.open_chat_with(crop, None, false);
        let context = format!("{text}\n\n");
        match (&mut self.chat, &mut self.o.ai_chat) {
            (Some(chat), _) => chat.panel.prefill(&context),
            (None, Some((_, panel))) => panel.prefill(&context),
            _ => {}
        }
        task
    }

    /// `open_chat`, the selection going with the question or not (`include`).
    pub(crate) fn open_chat_with(
        &mut self,
        crop: Crop,
        question: Option<String>,
        include: bool,
    ) -> Task<Message> {
        // one chat: a question while one stays open goes into it, with this crop
        if let Some(chat) = &mut self.chat {
            let id = chat.id;
            let asks = question.is_some();
            let task = (chat.panel.ask_about(crop, question, include))
                .map(move |msg| Message::AiChat(id, msg));
            // a question sent into a chat pinned before this overlay: the
            // overlay closes by itself, as after the follow-up (C4c, #170)
            if asks && self.o.chat_came_pinned && self.ends_tool() {
                return Task::batch([task, self.close()]);
            }
            return task;
        }
        // the overlay's own chat, still open (the bar is beside it): the same chat
        if let Some((id, panel)) = &mut self.o.ai_chat {
            let id = *id;
            return (panel.ask_about(crop, question, include))
                .map(move |msg| Message::AiChat(id, msg));
        }
        // No key (Google AI Mode): the browser opens, a pill says so, the
        // overlay closes; the chat is for an AI provider only.
        if hands_off() {
            return self.confirm_after(move || ai_chat::hand_off(&crop, question, include));
        }
        self.panels = self.panels.wrapping_add(1);
        let id = self.panels;
        let (panel, task) = ai_chat::AiChat::open(crop, question, include);
        self.o.ai_chat = Some((id, panel));
        self.keep_chat();
        task.map(move |msg| Message::AiChat(id, msg))
    }

    /// The overlay's chat pinned itself: the App keeps it from now on.
    pub(crate) fn keep_chat(&mut self) {
        if !(self.o.ai_chat.as_ref()).is_some_and(|(_, p)| p.pinned()) {
            return;
        }
        let Some((id, panel)) = self.o.ai_chat.take() else {
            return;
        };
        crate::log!("[opencircle] AI chat pinned");
        self.chat = Some(Resident { id, panel });
    }

    /// The first time the chat pins itself (`pinned_now`: a follow-up or the header's pin) and the
    /// overlay closes at once, a pill says why (F14, once per install). Not asked for a chat
    /// that was already pinned, nor when the close waits for a menu or download (`close_when_idle`).
    fn pinned_hint(&mut self, pinned_now: bool) -> Task<Message> {
        if !pinned_now || config::current().general.pin_hint_seen {
            return Task::none();
        }
        config::update_in_background(|c| c.general.pin_hint_seen = true);
        let text = crate::t!(pill_chat_pinned);
        (self.windows)
            .toast(
                text,
                crate::app::screens::toast::Tone::Neutral,
                None,
                windows::NOTICE_MS,
            )
            .map(Message::Windows)
    }

    /// A chat that was unpinned (C2) is a quick chat again: a new overlay, a new capture's question
    /// or a question from a window opens a fresh one, so it goes (with its window).
    pub(crate) fn release_unpinned_chat(&mut self) {
        if self.chat.as_ref().is_some_and(|c| !c.panel.pinned()) {
            crate::log!("[opencircle] unpinned AI chat released");
            self.chat = None;
        }
    }

    /// The pinned chat's x: it closes (its window with it, `sync_chat_window`).
    pub(crate) fn drop_chat(&mut self) -> Task<Message> {
        if self.chat.take().is_some() {
            crate::log!("[opencircle] AI chat unpinned and closed");
        }
        Task::none()
    }

    /// The chat has its own window from its first frame, above the overlay
    /// (#170): where windows hide (Windows, X11). On Wayland a window can't be
    /// placed or kept above the overlay, so the overlay draws the chat while
    /// it is open and the chat's window comes only when it closes (C6).
    // ponytail: Wayland keeps the old path; an XWayland chat needs a helper
    // process (winit runs one backend) and its keyboard focus is unproven.
    pub(crate) fn own_window(&self) -> bool {
        self.can_hide
    }

    /// The chat shown: the pinned one, else the overlay's.
    pub(crate) fn chat_shown(&self) -> Option<(u32, &ai_chat::AiChat)> {
        (self.chat.as_ref().map(|c| (c.id, &c.panel)))
            .or_else(|| self.o.ai_chat.as_ref().map(|(id, p)| (*id, p)))
    }

    /// Where the overlay docks the chat (`chat_rect`), on the screen it is on:
    /// that monitor (`ChatWindow::home`), its top-left (logical desktop px),
    /// its size (that monitor's logical px), and both in physical px.
    fn chat_place(&self) -> ((Point, Size, f32), Point, Size, Place) {
        let at = self.chat_rect();
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
        let desk = origin + iced::Vector::new(at.x, at.y);
        let place = place_px(desk, at.size(), scale);
        ((origin, screen, inset), desk, at.size(), place)
    }

    /// The chat's own window, opened where the overlay docks the chat. Where
    /// windows hide: kept above the overlay and faded in by the design's
    /// widget open (`POPUP`), the keyboard left with the overlay (a click on
    /// the chat's input takes it, as on the overlay: until then letters are
    /// the overlay's shortcuts). Wayland (`handover`, the overlay closing with
    /// a pinned chat): shown fully clear over the overlay's own drawing of the
    /// same card, given a beat to paint, then solid at once, so the overlay
    /// can go with no blank beat.
    fn open_chat_window(&mut self, handover: bool) -> Task<Message> {
        let placing = self.chat_place();
        self.open_chat_window_at(Ok(placing), handover)
    }

    /// `open_chat_window` where `placing` says (`Err(screen)`: where the desktop
    /// puts it, as on Wayland: the size alone is ours, from that screen).
    fn open_chat_window_at(
        &mut self,
        placing: Result<Placing, Size>,
        handover: bool,
    ) -> Task<Message> {
        let given = placing.is_ok();
        let ((origin, screen, inset), at, size, place) = placing.unwrap_or_else(|screen| {
            let size = card::docked(screen, card::Side::Right, 0.0, self.chat_width).size();
            let at = Point::ORIGIN;
            (
                (Point::ORIGIN, screen, 0.0),
                at,
                size,
                place_px(at, size, 1.0),
            )
        });
        let home = (origin, screen, inset);
        let (id, open) = windows::floating(
            window::Settings {
                size,
                position: match given {
                    true => window::Position::Specific(at),
                    false => window::Position::Default,
                },
                visible: false, // placed, then shown
                resizable: false,
                ..window::Settings::default()
            },
            card::CARD_RADIUS,
            windows::POPUP,
        );
        self.chat_win = Some(ChatWindow {
            id,
            home,
            at,
            moves: 0,
            size,
            glide: None,
            placed: given.then_some(place),
            hidden: false,
            out: false,
            wide: None,
        });
        let overlay = self.overlay;
        let shown = move |id| match handover {
            true => windows::opacity(id, 0.0)
                .chain(windows::show(id, window::Mode::Windowed))
                .chain(background(|tx| {
                    std::thread::sleep(HANDOVER_BEAT);
                    let _ = tx.unbounded_send(());
                }))
                .chain(windows::opacity(id, 1.0)),
            // (winit shows a window activated: the overlay takes the keys back)
            false => windows::keep_above(id, overlay)
                .chain(windows::opacity(id, 0.0))
                .chain(windows::show(id, window::Mode::Windowed))
                .chain(windows::foreground(overlay))
                .chain(windows::fade(id, true, windows::POPUP, false)),
        };
        open.then(move |id| match given {
            true => place_window::<()>(id, place).chain(shown(id)),
            false => shown(id),
        })
        .discard()
    }

    /// Ask AI from a window with no overlay (the Studio): the chat is pinned and opens
    /// in its own window docked on the screen the pointer is on (on Wayland
    /// the desktop places it); a chat already pinned takes the question, as a
    /// new capture's does.
    pub(crate) fn ask_from_window(
        &mut self,
        crop: Crop,
        from: Option<window::Id>,
    ) -> Task<Message> {
        self.release_unpinned_chat();
        if let Some(chat) = &mut self.chat {
            let id = chat.id;
            return (chat.panel.ask_about(crop, None, true)).map(move |m| Message::AiChat(id, m));
        }
        self.panels = self.panels.wrapping_add(1);
        let id = self.panels;
        let (mut panel, task) = ai_chat::AiChat::open(crop, None, true);
        let _ = panel.update(ai_chat::Msg::Pin);
        self.chat = Some(Resident { id, panel });
        let window = match (self.chat_win.is_some(), self.dock_beside_pointer(), from) {
            (true, ..) => Task::none(),
            (false, Some(placing), _) => self.open_chat_window_at(Ok(placing), true),
            // the pointer is unknown (Wayland): the size comes from the
            // asking window's own monitor (its own size if the OS won't say)
            (false, None, Some(id)) => window::monitor_size(id)
                .then(move |monitor| match monitor {
                    Some(size) => Task::done(size),
                    None => window::size(id),
                })
                .map(Message::ChatHome),
            (false, None, None) => Task::none(),
        };
        Task::batch([task.map(move |m| Message::AiChat(id, m)), window])
    }

    /// The screen a window-asked chat sizes from is known: its window opens
    /// where the desktop puts it.
    pub(crate) fn chat_home(&mut self, screen: Size) -> Task<Message> {
        match self.chat.is_some() && self.chat_win.is_none() {
            true => self.open_chat_window_at(Err(screen), true),
            false => Task::none(),
        }
    }

    /// Where the chat docks with no overlay: the right of the screen the
    /// pointer is on (`None`: unknown, the desktop decides).
    fn dock_beside_pointer(&self) -> Option<Placing> {
        let (x, y) = crate::shared::platform::capture::pointer()?;
        let places = monitor::places();
        let m = places.iter().copied().find(|m| {
            (m.origin.0..m.origin.0 + m.px.0 as i32).contains(&x)
                && (m.origin.1..m.origin.1 + m.px.1 as i32).contains(&y)
        })?;
        let (origin, screen) = (m.logical_origin(), m.size());
        let inset = monitor::taskbar_inset(origin, screen, m.scale);
        let dock = self.chat_dock(screen, self.chat_side.unwrap_or(card::Side::Right), inset);
        let at = origin + (dock.position() - Point::ORIGIN);
        Some((
            (origin, screen, inset),
            at,
            dock.size(),
            place_px(at, dock.size(), m.scale),
        ))
    }

    /// Wayland: the overlay closes with a pinned chat: it goes on in its own
    /// window, at its dock.
    pub(crate) fn float_chat(&mut self) -> Task<Message> {
        match self.chat.is_some() && self.chat_win.is_none() && !self.own_window() {
            true => self.open_chat_window(true),
            false => Task::none(),
        }
    }

    /// Wayland: the overlay is up again: it draws the chat, its window goes.
    pub(crate) fn unfloat_chat(&mut self) -> Task<Message> {
        if self.own_window() {
            return Task::none();
        }
        match self.chat_win.take() {
            Some(w) => window::close(w.id),
            None => Task::none(),
        }
    }

    /// The chat's window follows the chat, after every message (`update`), so
    /// no path leaves a chat without its window or a window without its chat:
    /// closed (faded out) with it; and where windows hide, opened with it,
    /// hidden while Snip is open or a grab runs (X11), and put where the
    /// overlay docks it while the overlay holds it (not one it found pinned
    /// there, `Opening::chat_fixed`, until it is picked up).
    pub(crate) fn sync_chat_window(&mut self) -> Task<Message> {
        let task = self.sync_chat_place();
        // its inner edge, on the side the screen holds it: the width is set
        // by that edge, as on the overlay (W52)
        let edge = (self.chat_win.as_ref())
            .filter(|_| self.own_window())
            .map(|w| {
                let (origin, screen, _) = w.home;
                card::side(
                    Rectangle::new(w.at - (origin - Point::ORIGIN), w.size),
                    screen,
                )
            });
        if let Some(c) = self.chat.as_mut() {
            c.panel.edge = edge;
        }
        if self.o.is_some()
            && let Some((_, panel)) = self.o.ai_chat.as_mut()
        {
            panel.edge = edge;
        }
        task
    }

    /// `sync_chat_window`'s window: opened, hidden, shown and placed.
    fn sync_chat_place(&mut self) -> Task<Message> {
        if self.chat_shown().is_none() {
            return match self.chat_win.take() {
                Some(w) => windows::close(w.id),
                None => Task::none(),
            };
        }
        if !self.own_window() {
            return Task::none();
        }
        let open = self.o.is_some();
        let snip = open && matches!(self.o.purpose, Purpose::Snip(_));
        // Where windows fade (Windows), the chat goes clear and back instead of hiding: no
        // blank first frame on its return. It goes once the overlay is fully up over it (the
        // frozen picture shows it there meanwhile, never the screen without it).
        let fades = crate::shared::platform::shape::can_fade();
        let up = self.o.reveal.is_none() && self.appeared() >= 1.0;
        if self.chat_win.is_none() {
            return match open && !snip {
                true => self.open_chat_window(false),
                false => Task::none(),
            };
        }
        let place = (open && self.o.chat_fixed.is_none()).then(|| self.chat_place());
        let resizing = self.chat_resizing();
        let (overlay, capturing) = (self.overlay, self.capturing.is_some());
        let Some(w) = self.chat_win.as_mut() else {
            return Task::none();
        };
        // a grab that never came (cancelled, failed): back
        if w.out && !capturing && !open {
            w.out = false;
        }
        let mut tasks = Vec::new();
        let hide = (snip && (up || !fades || w.hidden)) || w.out;
        if hide != w.hidden && fades {
            w.hidden = hide;
            windows::opacity_now(w.id, if hide { 0.0 } else { 1.0 });
        } else if hide != w.hidden {
            w.hidden = hide;
            tasks.push(match hide {
                true => window::set_mode(w.id, window::Mode::Hidden),
                false => windows::show(w.id, window::Mode::Windowed),
            });
            if !hide && open {
                tasks.push(windows::foreground(overlay));
            }
            // Snip is over and the overlay with it: showing the window activates
            // it and it would take the keyboard from the user's app (#170): hand
            // the focus back to the window that had it.
            if !hide && !open {
                let prior = crate::shared::platform::capture::window_rect::foreground();
                tasks.push(
                    background(move |tx| {
                        std::thread::sleep(Duration::from_millis(150));
                        crate::shared::platform::capture::window_rect::activate(prior);
                        let _ = tx.unbounded_send(());
                    })
                    .discard(),
                );
            }
        }
        if let Some((home, at, size, place)) = place.filter(|_| !hide)
            && w.placed != Some(place)
        {
            (w.home, w.at, w.size, w.placed, w.glide) = (home, at, size, Some(place), None);
            tasks.push(w.put(resizing));
        } else if !hide && w.wide.is_some() != resizing {
            // its inner edge just picked up or let go (W61)
            tasks.push(w.put(resizing));
        }
        Task::batch(tasks)
    }

    /// The chat's window moved (dragged by the OS, the overlay closed): it
    /// docks once it stops.
    pub(crate) fn chat_moved(&mut self, window: window::Id, at: Point) -> Task<Message> {
        // the overlay put it there (`sync_chat_window`), or its inner edge is
        // held and we size it: that is no drag, and a report of an earlier
        // size's spot must not move the edge the user isn't holding (W56)
        if self.o.is_some() || self.chat_resize.is_some() {
            return Task::none();
        }
        let Some(chat) = self.chat_win.as_mut().filter(|c| c.id == window) else {
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

    /// The overlay closed, the pinned chat's inner edge dragged (`p` in its
    /// window's px): its window takes the new width, as the overlay's card does
    /// (`card::drag`, kept on its screen), and the next chat keeps it.
    pub(crate) fn chat_resized(&mut self, window: window::Id, p: Point) -> Task<Message> {
        let Some(w) = self.chat_win.as_ref().filter(|w| w.id == window) else {
            return Task::none();
        };
        let (Some(placed), Some(started)) = (w.window_place(), self.chat_resize) else {
            return Task::none();
        };
        let desk = desk_pointer(placed, p);
        let (origin, screen, _) = w.home;
        // sized from where the drag started, on whole px: the width follows the
        // pointer 1:1 and only the inner edge moves (W56)
        let (from, start) = started.unwrap_or_else(|| {
            let here = Rectangle::new(w.at - (origin - Point::ORIGIN), w.size);
            (desk, here)
        });
        self.chat_resize = Some(Some((from, start)));
        let by = (desk - from) * (1.0 / placed.scale);
        let by = iced::Vector::new(by.x.round(), by.y.round());
        let to = card::drag(start, card::Drag::Resize, by, screen);
        self.chat_width = Some(to.width);
        self.put_chat_window(to)
    }

    /// Its inner edge double-clicked, the overlay closed: back to its dock at
    /// the design's width.
    fn redock_chat_window(&mut self) -> Task<Message> {
        let Some(w) = self.chat_win.as_ref() else {
            return Task::none();
        };
        let (origin, screen, inset) = w.home;
        let here = Rectangle::new(w.at - (origin - Point::ORIGIN), w.size);
        let side = self.chat_side.unwrap_or(card::dock_side(here, screen));
        let dock = self.chat_dock(screen, side, inset);
        self.put_chat_window(dock)
    }

    /// The chat's own window put at `r` (its monitor's logical px).
    fn put_chat_window(&mut self, r: Rectangle) -> Task<Message> {
        let resizing = self.chat_resizing();
        let Some(w) = self.chat_win.as_mut() else {
            return Task::none();
        };
        let scale = w.placed.map_or(1.0, |p| p.scale);
        let (origin, _, _) = w.home;
        let at = origin + (r.position() - Point::ORIGIN);
        let place = place_px(at, r.size(), scale);
        if w.placed == Some(place) {
            return Task::none();
        }
        (w.at, w.size, w.placed, w.glide) = (at, r.size(), Some(place), None);
        w.put(resizing)
    }

    /// The chat's inner edge is held, on the overlay or in its own window.
    pub(crate) fn chat_resizing(&self) -> bool {
        self.chat_resize.is_some() || matches!(self.o.chat_drag, Some((card::Drag::Resize, _)))
    }

    /// The drag is over: its window's scale now says which monitor it is on.
    pub(crate) fn chat_settle(&mut self, moves: u32) -> Task<Message> {
        match self.chat_win.as_ref().filter(|c| c.moves == moves) {
            Some(w) if self.o.is_none() => {
                window::scale_factor(w.id).map(move |k| Message::ChatDocked(moves, k))
            }
            _ => Task::none(),
        }
    }

    /// The window docks to the side its middle is on, on the monitor of
    /// `monitors` holding it (`scale`: its window's).
    pub(crate) fn chat_docked(
        &mut self,
        moves: u32,
        scale: f32,
        monitors: &[Place],
    ) -> Task<Message> {
        if self.o.is_some() {
            return Task::none(); // (the overlay docks it while open)
        }
        let Some(chat) = self.chat_win.as_mut().filter(|c| c.moves == moves) else {
            return Task::none();
        };
        let id = chat.id;
        if let Some(m) = monitor::holding(monitors, chat.at, chat.size, scale) {
            let (origin, size) = (m.logical_origin(), m.size());
            chat.home = (origin, size, monitor::taskbar_inset(origin, size, m.scale));
        }
        let (origin, screen, inset) = chat.home;
        let here = Rectangle::new(chat.at - iced::Vector::new(origin.x, origin.y), chat.size);
        let side = card::dock_side(here, screen);
        self.chat_side = Some(side);
        let dock = self.chat_dock(screen, side, inset);
        let Some(chat) = self.chat_win.as_mut() else {
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

    /// A frame of the chat window's glide to its dock: where it is now.
    pub(crate) fn chat_glide(&mut self, now: std::time::Instant) -> Task<Message> {
        let Some(chat) = self.chat_win.as_mut() else {
            return Task::none();
        };
        let Some((began, from, to)) = chat.glide else {
            return Task::none();
        };
        let t = motion::progress(began, now, motion::DOCK, motion::Curve::Standard);
        let at = glided(from, to, t);
        chat.glide = (t < 1.0).then_some((began, from, to));
        chat.at = at;
        let id = chat.id;
        // when each step ran (the overlay is closed: its frame log is the glide's)
        self.frame_times
            .borrow_mut()
            .push(std::time::Instant::now());
        if t >= 1.0 {
            self.log_frames("chat glide");
        }
        window::move_to(id, at)
    }

    /// The overlay opening on a screen at `origin` (logical px) finds the
    /// pinned chat: its window's spot is out of the frame (`chat_hole`), and
    /// it stays where it is, live above the overlay (#170 C4).
    pub(crate) fn find_chat(&mut self, origin: Point) {
        self.release_unpinned_chat();
        let found = (self.chat_win.as_ref())
            .filter(|_| self.chat.is_some())
            .map(|c| Rectangle::new(c.at - (origin - Point::ORIGIN), c.size));
        self.o.chat_hole = found;
        self.o.chat_fixed = found.filter(|_| self.own_window());
        self.o.chat_came_pinned = self.chat.is_some();
    }

    /// A chat the overlay found pinned where it is (`Opening::chat_fixed`) is
    /// picked up: from now on the overlay docks it, from that very spot (its
    /// screen, side, width and offset off its dock).
    fn adopt_chat(&mut self) {
        let Some(r) = self.o.chat_fixed.take() else {
            return;
        };
        self.o.chat_on = self.screen_holding(r);
        let (screen, inset, origin, k) = self.chat_room();
        let local = Rectangle::new(Point::ORIGIN + (r.position() - origin) * k, r.size() * k);
        let side = card::dock_side(local, screen);
        self.chat_side = Some(side);
        self.chat_width = Some(local.width);
        let dock = self.chat_dock(screen, side, inset);
        let dock = origin + (dock.position() - Point::ORIGIN) * (1.0 / k);
        self.o.chat_off = r.position() - dock;
    }

    pub(crate) fn on_ai_chat(&mut self, id: u32, msg: ai_chat::Msg) -> Task<Message> {
        let resident = self.chat.as_ref().is_some_and(|c| c.id == id);
        let open = self.o.is_some();
        if !resident && !open {
            return Task::none(); // a late answer for a closed overlay's panel
        }
        // the overlay closed: the OS moves the chat's own window
        let floating = (self.chat_win.as_ref())
            .map(|w| w.id)
            .filter(|_| resident && !open);
        if open && matches!(msg, ai_chat::Msg::Card(card::Event::Grab(_))) {
            self.adopt_chat(); // a chat found pinned there: the overlay docks it from now on
        }
        if matches!(msg, ai_chat::Msg::Saved) {
            self.windows.refresh_config(); // the chat wrote the config (web search, model)
        }
        let ends = self.ends_tool();
        let came_pinned = self.o.chat_came_pinned;
        let Some(panel) = (match resident {
            true => self.chat.as_mut().map(|c| &mut c.panel),
            false => (self.o.ai_chat.as_mut())
                .filter(|(open, _)| *open == id)
                .map(|(_, p)| p),
        }) else {
            return Task::none();
        };
        match msg {
            ai_chat::Msg::Card(card::Event::Grab(card::Drag::Move)) if floating.is_some() => {
                return floating.map_or_else(Task::none, window::drag);
            }
            ai_chat::Msg::Card(card::Event::Grab(how)) if open => {
                panel.held = how == card::Drag::Move; // picked up (design 3.4)
                self.o.chat_drag = Some((how, None));
                self.o.chat_sized = None;
                self.o.chat_docks = true;
                self.frame_times.borrow_mut().clear();
            }
            // the overlay closed: the chat's own window takes the width (`chat_resized`)
            ai_chat::Msg::Card(card::Event::Grab(card::Drag::Resize)) if floating.is_some() => {
                self.chat_resize = Some(None);
            }
            ai_chat::Msg::Card(card::Event::Hover(on)) if open => self.o.chat_hover = on,
            ai_chat::Msg::Card(card::Event::Reset) => {
                (self.card, self.chat_width) = (None, None);
                if !open {
                    return self.redock_chat_window();
                }
                self.o.chat_fixed = None; // (to its dock, at the design's width)
            }
            ai_chat::Msg::Card(card::Event::Grab(_) | card::Event::Hover(_)) => {}
            ai_chat::Msg::Search if open => return self.run(&search_in_browser::ACTION),
            ai_chat::Msg::Search => {}
            // A card's focused field gave Ctrl+C back (nothing selected in it).
            ai_chat::Msg::Key(keys::Keyed::Copy) if open => {
                return self.update(Message::Field(keys::Keyed::Copy));
            }
            ai_chat::Msg::Key(_) => {}
            // the x: a pinned chat and its window go; so does a quick one (the overlay stays)
            ai_chat::Msg::Close if resident => return self.drop_chat(),
            ai_chat::Msg::Close => self.o.ai_chat = None,
            // on a pinned chat the pin unpins it (the panel); it stays open
            ai_chat::Msg::Pin if resident => {
                let _ = panel.update(msg);
                // unpin = really leaves pinned mode: not on top any more (pin: on top again)
                return chat_level(self.chat_win.as_ref().map(|w| w.id), panel.pinned());
            }
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
                let asks = open
                    && matches!(
                        msg,
                        ai_chat::Msg::Submit
                            | ai_chat::Msg::Regenerate
                            | ai_chat::Msg::Preset(_)
                            | ai_chat::Msg::Ready(_)
                    );
                // the overlay's own chat, not pinned yet: this question may pin it
                let by_hand = open && matches!(msg, ai_chat::Msg::Pin);
                let pins = (asks || by_hand) && !resident && !panel.pinned();
                // a chat pinned before this overlay opened (C4c)
                let into_pinned = asks && resident && came_pinned;
                let was_pinned = panel.pinned();
                let task = panel.update(msg);
                // a follow-up in an unpinned chat pins it again: on top again
                let level = match resident && was_pinned != panel.pinned() {
                    true => chat_level(self.chat_win.as_ref().map(|w| w.id), panel.pinned()),
                    false => Task::none(),
                };
                // the chat's Include screen is the AI field's (one state)
                if let (Some(on), true) = (panel.include(), open) {
                    self.o.include = on;
                }
                self.keep_chat();
                let task = task.map(move |msg| Message::AiChat(id, msg));
                // The follow-up that pinned the chat ends the tool: the pinned
                // chat goes on alone (owner suggestion, #170), and so does a
                // question asked into a chat pinned before (C4c). Not in Snip,
                // not mid-drag; "Always" pinned it from the first question, no close.
                if ((pins && self.chat.is_some()) || into_pinned) && ends {
                    return Task::batch([task, self.pinned_hint(pins), self.close()]);
                }
                // The header's pin takes the same path; if a menu, download or
                // song search is busy, the overlay closes right after it ends.
                if by_hand && pins && self.chat.is_some() {
                    self.o.close_when_idle = true;
                }
                return Task::batch([task, level]);
            }
        }
        Task::none()
    }

    /// A question sent now may end the tool (#170): not in Snip, never while
    /// the user draws or drags the chat, nor while a menu or a download card is
    /// open or a song is being searched: the overlay waits (the user closes it).
    pub(crate) fn ends_tool(&self) -> bool {
        let menu = (self.o.translate.as_ref()).is_some_and(|(_, s)| s.picker_open());
        let downloading = matches!(self.o.reader, crate::app::Reader::Downloading(_));
        !self.o.drawing
            && self.o.chat_drag.is_none()
            && !matches!(self.o.purpose, Purpose::Snip(_))
            && !menu
            && !downloading
            && self.o.song.is_none()
    }

    /// The selection cut out of the frozen frame, in physical pixels; with the
    /// edits once it was edited (Ask AI then sees the annotations).
    pub(crate) fn crop(&self) -> Option<Crop> {
        if let Some(edit) = &self.o.edit {
            return Some(edit.image());
        }
        self.crop_of(self.crop_area()?)
    }

    /// What `crop` cuts out (logical px): every selection's union.
    pub(crate) fn crop_area(&self) -> Option<Rectangle> {
        let selection = self.o.selection?;
        Some((self.o.others.iter()).fold(selection.area, |all, (s, _)| all.union(&s.area)))
    }

    /// The pinned chat's spot (`Opening::chat_hole`) in the selection's crop px,
    /// if the crop meets it, on whichever screen (through desktop px, as the
    /// crop is cut: a crop across screens is one image); not on an edited capture.
    pub(crate) fn chat_hole(&self) -> Option<[u32; 4]> {
        if self.o.edit.is_some() || self.o.frame.is_none() {
            return None;
        }
        let area = self.crop_area()?;
        let origin = self.crop_origin(area)?;
        let size = self.crop_of(area).map(|c| (c.width, c.height))?;
        let (start, hole) = (self.o.place(), self.o.chat_hole?);
        let a = crate::app::monitors::to_desktop(start, hole.position());
        let b = crate::app::monitors::to_desktop(
            start,
            hole.position() + iced::Vector::from(hole.size()),
        );
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
    pub(crate) fn stacked(&self) -> Option<Crop> {
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
    pub(crate) fn chat_card_at(&self) -> Rectangle {
        let selection = self.o.selection.map(|s| self.o.view.rect_to_screen(s.area));
        if self.chatting() {
            return self.chat_at(selection);
        }
        let at = card::place(self.o.screen, selection, self.card);
        if self.o.translate.is_some() {
            return crate::features::translate::screens::pill::view::clear_of_pill(
                at,
                self.o.screen,
            );
        }
        at
    }

    /// The pinned chat as the overlay lays out round it (the bar, the chips
    /// keep off it): not in Snip. Snip's frozen screen shows it already (its
    /// window is in the grab, a pick like any window, `widgets_in_grab`), and
    /// nothing of ours covers Snip's outline (its window hides meanwhile).
    pub(crate) fn pinned_drawn(&self) -> Option<&Resident> {
        self.chat
            .as_ref()
            .filter(|_| !matches!(self.o.purpose, Purpose::Snip(_)))
    }

    /// The overlay draws the AI chat's card itself: only where the chat has no
    /// window of its own while the overlay is open (Wayland, `own_window`).
    pub(crate) fn draws_chat(&self) -> bool {
        !self.own_window()
    }

    /// The AI chat is the card shown.
    pub(crate) fn chatting(&self) -> bool {
        self.o.ai_chat.is_some()
    }

    /// The AI chat's card: docked on the side the user last let it go on,
    /// else the side `card::place` picks (away from the `selection`); held
    /// off it while dragged, gliding back once let go (`motion::DOCK`).
    fn chat_at(&self, selection: Option<Rectangle>) -> Rectangle {
        if let Some(found) = self.o.chat_fixed {
            return found; // a pinned chat's window, where the overlay found it
        }
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
    pub(crate) fn chat_room(&self) -> (Size, f32, Point, f32) {
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
    pub(crate) fn chat_window(&self) -> window::Id {
        self.window_of(self.screen_holding(self.chat_rect()))
    }

    /// The chat docked on `side` of a `screen` (taskbar `inset`), as wide as
    /// the user made it; full height (the bar moves aside, `bar_room`).
    pub(crate) fn chat_dock(&self, screen: Size, side: card::Side, inset: f32) -> Rectangle {
        card::docked(screen, side, inset, self.chat_width)
    }

    /// The AI chat's card on the overlay (the pinned one or the overlay's).
    pub(crate) fn chat_rect(&self) -> Rectangle {
        self.chat_at(self.o.selection.map(|s| self.o.view.rect_to_screen(s.area)))
    }

    /// The chat card let go: it docks to the side its middle is on, gliding there.
    pub(crate) fn dock_chat(&mut self, now: std::time::Instant) {
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
    pub(crate) fn chat_grown(&self) -> f32 {
        match (&self.o.ai_chat, self.o.chat_open) {
            _ if motion::reduced() || self.own_window() => 1.0, // (its window fades in)
            (Some((id, _)), Some((was, _, grown))) if *id == was => grown,
            _ => 0.0,
        }
    }

    /// The result card still opening (frames wanted).
    pub(crate) fn chat_opening(&self) -> bool {
        self.o.frame.is_some() && self.o.ai_chat.is_some() && self.chat_grown() < 1.0
    }
}

/// The chat window `id` on top of other windows while pinned, an ordinary window once unpinned.
fn chat_level(id: Option<window::Id>, pinned: bool) -> Task<Message> {
    let Some(id) = id else {
        return Task::none();
    };
    let level = match pinned {
        true => window::Level::AlwaysOnTop,
        false => window::Level::Normal,
    };
    window::set_level(id, level)
}
