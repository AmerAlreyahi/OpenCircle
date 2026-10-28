//! The overlay's state and logic: its messages, the work off the UI thread, key
//! focus, word picking and the quick actions. No widgets are built here.

pub(in crate::ui) mod background;
pub(in crate::ui) mod keys;
pub(in crate::ui) mod message;
pub(in crate::ui) mod opening;
pub(in crate::ui) mod quick;
pub(in crate::ui) mod word;

use crate::ui::*;

pub(in crate::ui) mod actions;
pub(in crate::ui) mod bar;
pub(in crate::ui) mod capture;
pub(in crate::ui) mod editing;
pub(in crate::ui) mod results;
pub(in crate::ui) mod selection;
pub(in crate::ui) mod snip;
pub(in crate::ui) mod text;
pub(in crate::ui) mod update;
pub(in crate::ui) mod view;

impl App {
    pub(in crate::ui) fn boot() -> (Self, Task<Message>) {
        let can_hide = can_hide_windows();
        // Where windows can hide: created once, hidden, so a trigger only has to
        // show it (latency). Elsewhere `show` opens it on each use.
        let (overlay, open) = match can_hide {
            true => {
                let (id, open) = windows::screen(overlay_window(None));
                (id, open.discard())
            }
            false => (window::Id::unique(), Task::none()),
        };
        let mut app = App {
            cache: Arc::new(Cache::new(CACHE_SIZE)),
            can_hide,
            overlay_open: can_hide,
            ..App::idle(overlay)
        };
        let start = app.windows.start().map(Message::Windows);
        // Started by an update: it is confirmed (the old program goes), notes shown.
        let notes = match crate::update::confirm() {
            Some((version, notes, url)) => app
                .windows
                .update_card(update_card::Card::installed(&version, &notes, &url))
                .map(Message::Windows),
            None => Task::none(),
        };
        (app, Task::batch([open, start, notes]))
    }

    /// Nothing open yet (the overlay window `overlay` pre-created, hidden).
    pub(in crate::ui) fn idle(overlay: window::Id) -> App {
        App {
            o: Opening::default(),
            overlay,
            overlay_open: true,
            can_hide: true,
            capturing: None,
            captures: 0,
            taps: 0,
            frame_times: RefCell::default(),
            opening: 0,
            first_draw: Cell::new(None),
            hiding: Hiding::No,
            panels: 0,
            windows: windows::Windows::default(),
            regions: Vec::new(),
            translates: 0,
            cache: Arc::new(Cache::new(8)),
            live: None,
            timed: false,
            card: None,
            songs: 0,
            update: None,
        }
    }

    pub(in crate::ui) fn title(&self, window: window::Id) -> String {
        if self.live.as_ref().and_then(live_layer::Live::window) == Some(window) {
            return "OpenCircle live translate".into();
        }
        if self.live.as_ref().and_then(live_layer::Live::controller) == Some(window) {
            return "OpenCircle live translate controls".into();
        }
        self.windows.title(window).to_string()
    }

    /// The live layer's (and its controller's) see-through background; the rest
    /// follow the OS.
    pub(in crate::ui) fn theme(&self, window: window::Id) -> Option<iced::Theme> {
        if windows::is_floating(window) {
            return Some(windows::clear_theme());
        }
        let live = self.live.as_ref()?;
        [live.window(), live.controller()]
            .contains(&Some(window))
            .then(live_layer::theme)
    }

    /// Something on the overlay moves by the clock (`Message::Frame` wanted):
    /// the opening, the bar turning or gliding, a card opening.
    pub(in crate::ui) fn animating(&self) -> bool {
        self.o.appear.is_none()
            || self.o.reveal.is_some()
            || self.appearing()
            || self.swapping()
            || self.bar_gliding()
            || self.card_opening()
            || self.o.bar_now.is_none()
    }

    /// The gaps between the overlay's frames since the last log, as `what`
    /// (the VM frame budget, #23: p95 <= 16 ms, none over 25 ms); then forgets
    /// them.
    pub(in crate::ui) fn log_frames(&self, what: &str) {
        let drawn = std::mem::take(&mut *self.frame_times.borrow_mut());
        let mut gaps: Vec<u128> = (drawn.windows(2))
            .map(|w| (w[1] - w[0]).as_millis())
            .collect();
        gaps.sort_unstable();
        if let Some(&max) = gaps.last() {
            let p95 = gaps[(gaps.len() * 95).div_ceil(100) - 1];
            crate::log!(
                "[opencircle] {what}: {} frames, p95 {p95} ms, max {max} ms apart",
                gaps.len() + 1
            );
        }
    }

    /// How far the opening animation is (0..1, eased out); 0 on its first frame.
    pub(in crate::ui) fn appeared(&self) -> f32 {
        self.o
            .appear
            .map_or(0.0, |(_, t)| motion::Curve::Out.ease(t))
    }

    /// A drawn stroke is still morphing into its box: the selection's actions
    /// wait for it (draw, release, snap, then the user acts).
    pub(in crate::ui) fn settling(&self) -> bool {
        (self.o.morph.as_ref())
            .is_some_and(|(_, at)| overlay::morph_progress(*at, Instant::now()) < 1.0)
    }

    /// The opening animation still runs (a frame drawn and more to come).
    pub(in crate::ui) fn appearing(&self) -> bool {
        self.o.frame.is_some() && self.appeared() < 1.0
    }

    pub(in crate::ui) fn subscription(&self) -> Subscription<Message> {
        // Live translate: where its controller was dragged.
        let moved = match self.live {
            Some(_) => iced::event::listen_with(|event, _, id| match event {
                iced::Event::Window(window::Event::Moved(at)) => {
                    Some(Message::Live(live_layer::Msg::Moved(id, at)))
                }
                _ => None,
            }),
            None => Subscription::none(),
        };
        let triggers = Subscription::batch([
            Subscription::run(triggers).map(Message::Trigger),
            self.windows.subscription().map(Message::Windows),
            moved,
        ]);
        // Also when a widget took the key: a text field only lets go of the
        // focus on Esc, and the overlay stays (`EscapeTaken`).
        let escape = || {
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                }) => Some(match status {
                    iced::event::Status::Captured => Message::EscapeTaken,
                    iced::event::Status::Ignored => Message::Escape,
                }),
                _ => None,
            })
        };
        // Esc in any of our windows cancels a pending capture.
        let triggers = match self.capturing {
            Some(_) => Subscription::batch([triggers, escape()]),
            None => triggers,
        };
        if self.o.frame.is_some() {
            let escape = escape();
            // Alt held = the original under a translation.
            let alt = iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                    Some(Message::Peek(m.alt()))
                }
                _ => None,
            });
            let alt = if self.o.translate.is_some() {
                alt
            } else {
                Subscription::none()
            };
            // Dragging the result card: the pointer anywhere, until let go.
            let card = match self.o.card_drag {
                Some(_) => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::CardMoved(position))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::CardReleased),
                    _ => None,
                }),
                None => Subscription::none(),
            };
            // Snip: Shift held = no snapping; the pointer leaving may be for
            // another monitor.
            let shift = match self.o.purpose {
                Purpose::Snip(_) => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        Some(Message::Snip(crate::ui::snip::Msg::Shift(m.shift())))
                    }
                    iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
                        Some(Message::Snip(crate::ui::snip::Msg::Left))
                    }
                    _ => None,
                }),
                _ => Subscription::none(),
            };
            // One message per frame while something moves, none at rest.
            let frames = match self.animating() {
                true => motion::frames().map(Message::Frame),
                false => Subscription::none(),
            };
            // Dragging the bar by its handle: the pointer anywhere, until let go. On
            // whenever the bar shows (not only once held), so no move or release
            // is lost while the grab is still being handled.
            let bar = match self.bottom_bar_shown() || self.editing() {
                true => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::BarMoved(position))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::BarReleased),
                    _ => None,
                }),
                false => Subscription::none(),
            };
            Subscription::batch([triggers, escape, alt, card, shift, frames, bar])
        } else if self.hiding != Hiding::No {
            // One stream for both, so a resize is always seen before the frames after it.
            // Every message makes iced redraw, so frames keep coming while this runs.
            let steps = iced::event::listen_raw(|event, _, _| match event {
                iced::Event::Window(window::Event::RedrawRequested(_)) => Some(Message::Drawn),
                iced::Event::Window(window::Event::Resized(size)) => Some(Message::Resized(size)),
                _ => None,
            });
            Subscription::batch([triggers, steps])
        } else {
            triggers
        }
    }
}
