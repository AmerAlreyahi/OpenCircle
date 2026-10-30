//! The overlay's state and logic: its messages, the work off the UI thread, key
//! focus, word picking and the quick actions. No widgets are built here.

pub(in crate::ui) mod background;
pub(in crate::ui) mod keys;
pub(in crate::ui) mod message;
pub(in crate::ui) mod opening;
pub(in crate::ui) mod quick;
pub(in crate::ui) mod word;

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::{self, key::Named};
use iced::window::{self};
use iced::{Point, Rectangle, Size, Subscription, Task};

use crate::translate::pipeline::Cache;

use self::background::triggers;
use self::capture::overlay_window;
use self::message::{Hiding, Message, Purpose};
use self::opening::Open;
use crate::ui::kit::motion;
use crate::ui::platform::monitor::can_hide_windows;
use crate::ui::{live_layer, overlay, update_card, windows};

pub(in crate::ui) mod actions;
pub(in crate::ui) mod bar;
pub(in crate::ui) mod capture;
pub(in crate::ui) mod editing;
pub(in crate::ui) mod results;
pub(in crate::ui) mod screens;
pub(in crate::ui) mod selection;
pub(in crate::ui) mod snip;
#[cfg(test)]
pub(in crate::ui) mod tests;
pub(in crate::ui) mod text;
pub(in crate::ui) mod update;
pub(in crate::ui) mod view;
pub(in crate::ui) mod zoom;

pub(in crate::ui) const HINT: &str = "Circle, drag or tap anything · Esc to close";

/// Shown over a selection that came out black (DRM / protected windows).
pub(in crate::ui) const PROTECTED: &str = "This window is protected; it can't be captured";

/// The ask bar at the bottom of the overlay: its text field, width, and the
/// height kept free for it (the action bar stays above it).
pub(in crate::ui) const ASK_INPUT: &str = "overlay-ask";

/// The bottom bar (the new design's search bar, owner 2026-10-26: "circle, then
/// choose"): grip, the AI field, the pills Search in browser · Copy text ·
/// Translate (`labs::bar`), then Live translate (Beta) and, while their Labs
/// switch is on, Song search and Edit. All there in every state, so it never
/// changes size; Search in browser and Copy text are dimmed until something is
/// selected. Nothing is sent until the user picks one (`[selection] after`).
/// The field's placeholder.
pub(in crate::ui) const ASK_PLACEHOLDER: &str = "Ask OpenCircle AI about your screen…";

/// Shorter ones where the font or a narrow screen leaves the field too little
/// room for it (#144): never cut off mid-word.
pub(in crate::ui) const ASK_PLACEHOLDERS: [&str; 4] = [
    ASK_PLACEHOLDER,
    "Ask AI about your screen…",
    "Ask OpenCircle AI…",
    "Ask AI…",
];

/// How long the bar takes to turn into the editing toolbar (and back),
/// and to settle on a new spot after a drag (the design's drag release).
pub(in crate::ui) const BAR_MORPH: Duration = motion::MORPH;

pub(in crate::ui) const GLIDE: Duration = motion::DOCK;

/// A QR code's chip under a selection (its room; the chip is centred in it).
pub(in crate::ui) const QR_WIDTH: f32 = 520.0;
/// The width the link / email chip is centred in.
pub(in crate::ui) const LINK_WIDTH: f32 = 400.0;
/// The "Select text" chip's room beside a snapped box (its chip is centred in it).
pub(in crate::ui) const SELECT_WIDTH: f32 = 160.0;

/// The bottom bar's Live translate icon (lucide "radio").
pub(in crate::ui) const LIVE: char = '\u{e145}';

pub(in crate::ui) const ASK_MARGIN: f32 = 24.0;

/// A capture still unanswered after this is given up (a Wayland portal may wait
/// on a dialog; its late answer is dropped).
pub(in crate::ui) const CAPTURE_TIMEOUT: Duration = Duration::from_secs(20);

/// Snip reads the controls (UI Automation) of this many topmost windows at
/// once, of any other when the pointer first goes over it.
pub(in crate::ui) const UIA_WINDOWS: usize = 4;

/// How far (logical px) the hint and the bottom bar slide in as the overlay opens.
pub(in crate::ui) const SLIDE: f32 = 80.0;

/// Presses this close together are a double (triple) click.
// (Windows' default; also forgiving when a busy machine delays the second press.)
pub(in crate::ui) const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// The text reader's helper starts this long after the opening animation.
pub(in crate::ui) const WARM_AFTER: Duration = Duration::from_millis(400);

/// The overlay fades in at most this long after it was shown, its first
/// full frame drawn or not (never left invisible over the screen).
pub(in crate::ui) const REVEAL_WAIT: Duration = Duration::from_millis(400);

/// Select-text mode on a snapped box: its words (`None` while OCR reads them)
/// and the ones picked by a press-drag over them.
pub(in crate::ui) struct TextMode {
    lines: Option<Vec<word::Line>>,
    boxes: Vec<Rectangle>,
    picked: Option<word::Picked>,
    picking: bool,
}

/// The text reader (OCR) behind text selection in main mode, as the note over
/// the bar tells it: without its models a press on text can only draw.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::ui) enum Reader {
    /// Nothing to say: the screen's text is read (or nothing reads it).
    Idle,
    /// Its models aren't downloaded (this many bytes to go): the note offers
    /// the download (one click).
    Missing(u64),
    /// Downloading them (0..=1), after that click.
    Downloading(f32),
    /// The download failed (why); the note offers it again.
    Failed(String),
}

pub(in crate::ui) const CHIP_SPACE: f32 = 12.0;

/// Below this (logical px) the window counts as shrunk.
pub(in crate::ui) const SHRUNK: f32 = 64.0;

/// Hide anyway after this many frames without the shrink (a WM may refuse it): a
/// window left on top of the screen would be far worse than unshrunk buffers.
// ponytail: fixed frame count, not a timer; frames come back to back while closing.
pub(in crate::ui) const SHRINK_FRAMES: u8 = 10;

/// The toast pill's gap to the screen's right edge and to the taskbar (design: 16).
pub(in crate::ui) const TOAST_MARGIN: f32 = 16.0;

pub(in crate::ui) struct App {
    /// Everything one capture holds, while the overlay is open (`Open`);
    /// `close` drops it whole.
    pub(in crate::ui) o: Open,
    /// What the capture on its way opens for (`Opening::purpose` at `open`).
    pub(in crate::ui) next: Purpose,
    /// The overlay window's id; a closed window's while `overlay_open` is false.
    pub(in crate::ui) overlay: window::Id,
    /// The overlay window exists: always where it is pre-created and hidden
    /// (`can_hide`), else only while it is shown.
    pub(in crate::ui) overlay_open: bool,
    /// `can_hide_windows()`, read once at boot.
    pub(in crate::ui) can_hide: bool,
    /// The number of the capture running on its thread. Cleared by Esc or the
    /// timeout: that capture's late answer is then dropped.
    pub(in crate::ui) capturing: Option<u32>,
    /// The overlay shows, unseen, for the snip being grabbed (`pre_show`).
    pub(in crate::ui) pre_shown: bool,
    /// Captures started (numbers them).
    pub(in crate::ui) captures: u32,
    /// The latest word tap's number (a tap still reading is stale after it).
    pub(in crate::ui) taps: u32,
    /// When the overlay drew each frame since the last `log_frames` (the
    /// VM frame budget: the opening, a bar or card drag, the glide after).
    pub(in crate::ui) frame_times: RefCell<Vec<Instant>>,
    /// Bumped on every open (resets the overlay's pointer state).
    pub(in crate::ui) opening: u32,
    /// Trigger time + expected size of the full-screen frame not drawn yet (latency log).
    pub(in crate::ui) first_draw: Cell<Option<(Instant, Size)>>,
    pub(in crate::ui) hiding: Hiding,
    /// Result panels opened (numbers them).
    pub(in crate::ui) panels: u32,
    pub(in crate::ui) windows: windows::Windows,
    /// The last selection per monitor, for `last_region` (memory only).
    pub(in crate::ui) regions: Vec<(Point, Rectangle)>,
    /// Translate sessions started (numbers them).
    pub(in crate::ui) translates: u32,
    /// Translations by text, shared by one-shot and live mode.
    pub(in crate::ui) cache: Arc<Cache>,
    /// Live translate, while on.
    pub(in crate::ui) live: Option<live_layer::Live>,
    /// The next snip capture is the one a timer waited for: no timer again.
    pub(in crate::ui) timed: bool,
    /// The next snip capture is the live pin quick action's (`Snip::live_pin`).
    pub(in crate::ui) live_next: bool,
    /// Where the user moved / sized the result card (this run of the app).
    pub(in crate::ui) card: Option<Rectangle>,
    /// The side the user docked the AI chat to (this run of the app).
    pub(in crate::ui) chat_side: Option<crate::ui::kit::panel_frame::Side>,
    /// The AI chat's width the user set by its inner edge (this run of the app).
    pub(in crate::ui) chat_width: Option<f32>,
    /// The AI chat once pinned: it outlives the overlay (`results::Resident`).
    pub(in crate::ui) chat: Option<results::Resident>,
    /// Song searches started (numbers them).
    pub(in crate::ui) songs: u32,
    /// A verified update waiting until nothing is open (`is_idle`).
    pub(in crate::ui) update: Option<crate::update::Staged>,
}

/// Translations kept for reuse (each a short text).
pub(in crate::ui) const CACHE_SIZE: usize = 512;

/// The part of a `screen` above the taskbar (`inset` px of its bottom).
pub(in crate::ui) fn work_area(screen: Size, inset: f32) -> Size {
    Size::new(screen.width, screen.height - inset)
}

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
            o: Open::default(),
            next: Purpose::Search,
            overlay,
            overlay_open: true,
            can_hide: true,
            capturing: None,
            pre_shown: false,
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
            live_next: false,
            card: None,
            chat_side: None,
            chat_width: None,
            chat: None,
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
        if self.chat.as_ref().and_then(|c| c.window) == Some(window) {
            return "OpenCircle AI".into();
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
            || self.o.chat_glide.is_some()
            || self.o.glow.moving()
            || self.o.zoom.is_some()
            || (self.o.panel.as_ref()).is_some_and(|(_, p)| p.spinning())
            || self.chat.as_ref().is_some_and(|c| c.panel.spinning())
            || self.o.bar_now.is_none()
            || (self.o.edit.as_ref()).is_some_and(crate::ui::screens::editor::Editing::growing)
            || !self.o.waiting.is_empty() // other screens wait for a frame after the opening
    }

    /// The gaps between the overlay's frames since the last log, as `what`
    /// (the VM frame budget, #23: p95 <= 16 ms, none over 25 ms); then forgets
    /// them.
    pub(in crate::ui) fn log_frames(&self, what: &str) {
        log_gaps(what, &self.frame_times);
    }
}

/// The gaps between the frames in `times`, as `what`; then forgets them.
pub(in crate::ui) fn log_gaps(what: &str, times: &RefCell<Vec<Instant>>) {
    let drawn = std::mem::take(&mut *times.borrow_mut());
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

impl App {
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
        // The pinned chat in its own window: where it was dragged, and its
        // spinner's frames while the AI thinks.
        let chat = match self.chat.as_ref().filter(|c| c.window.is_some()) {
            Some(chat) => Subscription::batch([
                iced::event::listen_with(|event, _, id| match event {
                    iced::Event::Window(window::Event::Moved(at)) => {
                        Some(Message::ChatMoved(id, at))
                    }
                    _ => None,
                }),
                match chat.panel.spinning() || chat.glide.is_some() {
                    true => motion::frames().map(Message::ChatFrame),
                    false => Subscription::none(),
                },
            ]),
            None => Subscription::none(),
        };
        let triggers = Subscription::batch([
            Subscription::run(triggers).map(Message::Trigger),
            self.windows.subscription().map(Message::Windows),
            moved,
            chat,
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
        if self.o.is_some() {
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
            // Snip: Shift or Alt held = no snapping.
            let shift = match self.o.purpose {
                Purpose::Snip(_) => iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => Some(
                        Message::Snip(crate::ui::snip::Msg::Shift(m.shift() || m.alt())),
                    ),
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
                true => iced::event::listen_with(|event, _, id| match event {
                    // (in that window's px: Snip's toolbar may be on another screen)
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::BarMovedOn(id, position))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::BarReleased),
                    _ => None,
                }),
                false => Subscription::none(),
            };
            // Several screens: the edge glow follows the pointer onto the one it
            // comes into (one message per crossing, none while it moves).
            let glow = match self.o.screens.is_empty() {
                true => Subscription::none(),
                false => iced::event::listen_with(|event, _, id| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorEntered) => {
                        Some(Message::PointerOn(id))
                    }
                    _ => None,
                }),
            };
            Subscription::batch([triggers, escape, alt, card, shift, frames, bar, glow])
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
