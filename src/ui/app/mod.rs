//! The overlay's state and logic: its messages, the work off the UI thread, key
//! focus, word picking and the quick actions. No widgets are built here.

pub(crate) mod background;
pub(crate) mod keys;
pub(crate) mod message;
pub(crate) mod opening;
pub(crate) mod quick;
pub(crate) mod word;

use crate::shared::app_language::S;
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
use crate::shared::kit::motion;
use crate::shared::platform::monitor::can_hide_windows;
use crate::shared::touch;
use crate::ui::{live_layer, overlay, windows};

pub(crate) mod actions;
pub(crate) mod bar;
pub(crate) mod capture;
pub(crate) mod screens;
pub(crate) mod selection;
pub(crate) mod snip;
pub(crate) mod snip_more;
pub(crate) mod snip_toolbar;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod text;
pub(crate) mod tools;
pub(crate) mod update;
pub(crate) mod updates;
pub(crate) mod view;
pub(crate) mod zoom;

/// The hint over the screen as the overlay opens (the app's language).
pub(crate) fn hint() -> &'static str {
    crate::t!(bar_hint)
}

/// Shown over a selection that came out black (DRM / protected windows).
pub(crate) fn protected() -> &'static str {
    crate::t!(bar_protected)
}

/// The ask bar at the bottom of the overlay: its text field, width, and the
/// height kept free for it (the action bar stays above it).
pub(crate) const ASK_INPUT: &str = "overlay-ask";

/// The bottom bar (the new design's search bar, owner 2026-10-26: "circle, then
/// choose"): grip, the AI field, the pills Search in browser · Copy text ·
/// Translate (`labs::bar`), then Live translate (Beta) and, while their Labs
/// switch is on, Song search and Edit. All there in every state, so it never
/// changes size; Search in browser and Copy text are dimmed until something is
/// selected. Nothing is sent until the user picks one (`[selection] after`).
/// The field's placeholder, then shorter ones where the font or a narrow
/// screen leaves the field too little room for it (#144): never cut off
/// mid-word.
pub(crate) fn ask_placeholders() -> [&'static str; 4] {
    use crate::t;
    [t!(bar_ask_1), t!(bar_ask_2), t!(bar_ask_3), t!(bar_ask_4)]
}

/// How long the bar takes to turn into the editing toolbar (and back),
/// and to settle on a new spot after a drag (the design's drag release).
pub(crate) const BAR_MORPH: Duration = motion::MORPH;

pub(crate) const GLIDE: Duration = motion::DOCK;

/// A QR code's chip under a selection (its room; the chip is centred in it).
pub(crate) const QR_WIDTH: f32 = 520.0;
/// The width the link / email chip is centred in.
pub(crate) const LINK_WIDTH: f32 = 400.0;
/// The "Select text" chip's room beside a snapped box (its chip is centred in it).
pub(crate) const SELECT_WIDTH: f32 = 160.0;

/// The bottom bar's Live translate icon (lucide "radio").
pub(crate) const LIVE: char = '\u{e145}';

pub(crate) const ASK_MARGIN: f32 = 24.0;

/// A capture still unanswered after this is given up (a Wayland portal may wait
/// on a dialog; its late answer is dropped).
pub(crate) const CAPTURE_TIMEOUT: Duration = Duration::from_secs(20);

/// Linux: a capture still unanswered after this waits on the desktop's own dialog
/// (the portal asks for permission): the shell's overview, if it shows, is left so
/// the dialog can be seen (#173). A silent capture takes 50-150 ms (ScreenCast).
#[cfg(target_os = "linux")]
pub(crate) const CAPTURE_SLOW: Duration = Duration::from_millis(1500);

/// Snip reads the controls (UI Automation) of this many topmost windows at
/// once, of any other when the pointer first goes over it.
pub(crate) const UIA_WINDOWS: usize = 4;

/// How far (logical px) the hint and the bottom bar slide in as the overlay opens.
pub(crate) const SLIDE: f32 = 80.0;

/// Presses this close together are a double (triple) click.
// (Windows' default; also forgiving when a busy machine delays the second press.)
pub(crate) const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// The text reader's helper starts this long after the opening animation.
pub(crate) const WARM_AFTER: Duration = Duration::from_millis(400);

/// The overlay fades in at most this long after it was shown, its first
/// full frame drawn or not (never left invisible over the screen).
pub(crate) const REVEAL_WAIT: Duration = Duration::from_millis(400);

/// Select-text mode on a snapped box: its words (`None` while OCR reads them)
/// and the ones picked by a press-drag over them.
pub(crate) struct TextMode {
    lines: Option<Vec<word::Line>>,
    boxes: Vec<Rectangle>,
    picked: Option<word::Picked>,
    picking: bool,
}

/// The text reader (OCR) behind text selection in main mode, as the note over
/// the bar tells it: without its models a press on text can only draw.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reader {
    /// Nothing to say: the screen's text is read (or nothing reads it).
    Idle,
    /// Its models aren't downloaded (this many bytes to go): the note offers
    /// the download (one click).
    Missing(u64),
    /// Downloading them (0..=1), after that click.
    Downloading(f32),
    /// The download failed (why); the note offers it again.
    Failed(S),
}

pub(crate) const CHIP_SPACE: f32 = 12.0;

/// Below this (logical px) the window counts as shrunk.
pub(crate) const SHRUNK: f32 = 64.0;

/// Hide anyway after this many frames without the shrink (a WM may refuse it): a
/// window left on top of the screen would be far worse than unshrunk buffers.
// ponytail: fixed frame count, not a timer; frames come back to back while closing.
pub(crate) const SHRINK_FRAMES: u8 = 10;

/// The toast pill's gap to the screen's right edge and to the taskbar (design: 16).
pub(crate) const TOAST_MARGIN: f32 = 16.0;

pub(crate) struct App {
    /// Everything one capture holds, while the overlay is open (`Open`);
    /// `close` drops it whole.
    pub(crate) o: Open,
    /// What the capture on its way opens for (`Opening::purpose` at `open`).
    pub(crate) next: Purpose,
    /// The overlay window's id; a closed window's while `overlay_open` is false.
    pub(crate) overlay: window::Id,
    /// The overlay window exists: always where it is pre-created and hidden
    /// (`can_hide`), else only while it is shown.
    pub(crate) overlay_open: bool,
    /// Linux: when the last opening's windows were closed (or hidden); the desktop
    /// still fades them out a moment, so a grab then waits (`grab_wait`).
    pub(crate) overlay_gone: Option<Instant>,
    /// `can_hide_windows()`, read once at boot.
    pub(crate) can_hide: bool,
    /// The number of the capture running on its thread. Cleared by Esc or the
    /// timeout: that capture's late answer is then dropped.
    pub(crate) capturing: Option<u32>,
    /// The overlay shows, unseen, for the snip being grabbed (`pre_show`).
    pub(crate) pre_shown: bool,
    /// Captures started (numbers them).
    pub(crate) captures: u32,
    /// The latest word tap's number (a tap still reading is stale after it).
    pub(crate) taps: u32,
    /// When the overlay drew each frame since the last `log_frames` (the
    /// VM frame budget: the opening, a bar or card drag, the glide after).
    pub(crate) frame_times: RefCell<Vec<Instant>>,
    /// Bumped on every open (resets the overlay's pointer state).
    pub(crate) opening: u32,
    /// Trigger time + expected size of the full-screen frame not drawn yet (latency log).
    pub(crate) first_draw: Cell<Option<(Instant, Size)>>,
    pub(crate) hiding: Hiding,
    /// Result panels opened (numbers them).
    pub(crate) panels: u32,
    pub(crate) windows: windows::Windows,
    /// The last selection per monitor, for `last_region` (memory only).
    pub(crate) regions: Vec<(Point, Rectangle)>,
    /// Translate sessions started (numbers them).
    pub(crate) translates: u32,
    /// Translations by text, shared by one-shot and live mode.
    pub(crate) cache: Arc<Cache>,
    /// Live translate, while on.
    pub(crate) live: Option<live_layer::Live>,
    /// The next snip capture is the one a timer waited for: no timer again.
    pub(crate) timed: bool,
    /// The next snip capture is the live pin quick action's (`Snip::live_pin`).
    pub(crate) live_next: bool,
    /// The next snip opens this screen tool (its quick action).
    pub(crate) tool_next: Option<crate::ui::screens::tools::Which>,
    /// The next snip opens this image picked: a step of the Steps strip, to
    /// update it (its index), or a long capture finished on Wayland (none).
    pub(crate) picked_next: Option<(Option<usize>, crate::actions::Crop)>,
    /// Wayland: a long capture going on after its overlay went (the user
    /// scrolls the page itself, its pop-up beside the area): `App::cast_done`.
    pub(crate) cast: Option<snip::Cast>,
    /// The colours the picker copied, newest first (this run of the app only).
    pub(crate) colours: Vec<[u8; 3]>,
    /// Where the user moved / sized the result card (this run of the app).
    pub(crate) card: Option<Rectangle>,
    /// The side the user docked the AI chat to (this run of the app).
    pub(crate) chat_side: Option<crate::shared::kit::panel_frame::Side>,
    /// The AI chat's width the user set by its inner edge (this run of the app).
    pub(crate) chat_width: Option<f32>,
    /// The AI chat once pinned: it outlives the overlay (`ai_chat::Resident`).
    pub(crate) chat: Option<crate::features::ask_ai::app::ai_chat::Resident>,
    /// The AI chat's own window (`ai_chat::ChatWindow`).
    pub(crate) chat_win: Option<crate::features::ask_ai::app::ai_chat::ChatWindow>,
    /// Song searches started (numbers them).
    pub(crate) songs: u32,
    /// The update waiting for a moment to install (`update::flow`), with its pill.
    pub(crate) update: Option<Waiting>,
    /// After a quiet install (a quit, a lock): its "Updated" pill, once.
    pub(crate) updated: Option<(
        crate::shared::update::Pill,
        crate::shared::update::Installed,
    )>,
    /// The update clock runs (`Message::UpdateTick`).
    pub(crate) update_ticks: bool,
}

/// An update waiting: what it is, how it installs, its pill, the lock watch.
pub(crate) struct Waiting {
    pub version: String,
    pub url: String,
    pub how: crate::shared::update::How,
    /// Downloaded and verified (none for one the user gets: "Get it").
    pub staged: Option<crate::shared::update::Staged>,
    pub pill: crate::shared::update::Pill,
    pub lock: crate::shared::update::Lock,
}

/// Translations kept for reuse (each a short text).
pub(crate) const CACHE_SIZE: usize = 512;

/// The part of a `screen` above the taskbar (`inset` px of its bottom).
pub(crate) fn work_area(screen: Size, inset: f32) -> Size {
    Size::new(screen.width, screen.height - inset)
}

impl App {
    pub(crate) fn boot() -> (Self, Task<Message>) {
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
        // Started by an update: it is confirmed (the old program goes), then
        // What's new (after Restart now) or its small pill (after a quiet install).
        let notes = match crate::shared::update::confirm() {
            Some(installed) => app.on_installed(installed),
            None => Task::none(),
        };
        (app, Task::batch([open, start, notes]))
    }

    /// Nothing open yet (the overlay window `overlay` pre-created, hidden).
    pub(crate) fn idle(overlay: window::Id) -> App {
        App {
            o: Open::default(),
            next: Purpose::Search,
            overlay,
            overlay_open: true,
            overlay_gone: None,
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
            tool_next: None,
            picked_next: None,
            cast: None,
            colours: Vec::new(),
            card: None,
            chat_side: None,
            chat_width: None,
            chat: None,
            chat_win: None,
            songs: 0,
            update: None,
            updated: None,
            update_ticks: false,
        }
    }

    pub(crate) fn title(&self, window: window::Id) -> String {
        if self.live.as_ref().and_then(live_layer::Live::window) == Some(window) {
            return crate::t!(win_t_live).into();
        }
        if self.live.as_ref().and_then(live_layer::Live::controller) == Some(window) {
            return crate::t!(win_t_live_ctl).into();
        }
        if self.chat_win.as_ref().is_some_and(|w| w.id == window) {
            return crate::t!(win_t_chat).into();
        }
        self.windows.title(window).to_string()
    }

    /// The live layer's (and its controller's) see-through background; the rest
    /// follow the OS.
    pub(crate) fn theme(&self, window: window::Id) -> Option<iced::Theme> {
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
    pub(crate) fn animating(&self) -> bool {
        self.o.appear.is_none()
            || self.o.reveal.is_some()
            || self.appearing()
            || self.swapping()
            || self.bar_gliding()
            || self.chat_opening()
            || self.o.chat_glide.is_some()
            || self.o.glow.moving()
            || self.o.zoom.is_some()
            || (self.o.ai_chat.as_ref()).is_some_and(|(_, p)| p.spinning())
            || self.chat.as_ref().is_some_and(|c| c.panel.spinning())
            || self.o.bar_now.is_none()
            || (self.o.edit.as_ref())
                .is_some_and(crate::ui::screens::snip_toolbar::SnipToolbar::growing)
            || !self.o.waiting.is_empty() // other screens wait for a frame after the opening
            || (self.reading_text().is_some() && !motion::reduced()) // Copy text's spinner (#169)
    }

    /// The gaps between the overlay's frames since the last log, as `what`
    /// (the VM frame budget, #23: p95 <= 16 ms, none over 25 ms); then forgets
    /// them.
    pub(crate) fn log_frames(&self, what: &str) {
        log_gaps(what, &self.frame_times);
    }

    /// Logs `what` (a mark of reading the screen's text, "[opencircle] ...") with the time since
    /// the overlay's first frame: the VM's timings.
    pub(crate) fn log_mark(&self, what: &str) {
        match self.o.appear.map(|(at, _)| at.elapsed().as_millis()) {
            Some(ms) => crate::log!("{what} {ms} ms after the first frame"),
            None => crate::log!("{what} before the first frame"),
        }
    }
}

/// The gaps between the frames in `times`, as `what`; then forgets them.
pub(crate) fn log_gaps(what: &str, times: &RefCell<Vec<Instant>>) {
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
    pub(crate) fn appeared(&self) -> f32 {
        self.o
            .appear
            .map_or(0.0, |(_, t)| motion::Curve::Out.ease(t))
    }

    /// A drawn stroke is still morphing into its box: the selection's actions
    /// wait for it (draw, release, snap, then the user acts).
    pub(crate) fn settling(&self) -> bool {
        (self.o.morph.as_ref())
            .is_some_and(|(_, at)| overlay::morph_progress(*at, Instant::now()) < 1.0)
    }

    /// The opening animation still runs (a frame drawn and more to come).
    pub(crate) fn appearing(&self) -> bool {
        self.o.frame.is_some() && self.appeared() < 1.0
    }

    pub(crate) fn subscription(&self) -> Subscription<Message> {
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
        // The chat in its own window: where it was dragged, and its
        // spinner's frames while the AI thinks (the overlay's frames while open).
        let chat = match (self.chat_win.as_ref()).zip(self.chat_shown()) {
            Some((window, (_, panel))) => Subscription::batch([
                iced::event::listen_with(|event, _, id| match event {
                    iced::Event::Window(window::Event::Moved(at)) => {
                        Some(Message::ChatMoved(id, at))
                    }
                    _ => None,
                }),
                match panel.spinning() || window.glide.is_some() {
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
            let card = match self.o.chat_drag {
                Some(_) => iced::event::listen_with(|event, _, id| match touch::drag(&event) {
                    Some(touch::Drag::Moved(position)) => Some(Message::CardMovedOn(id, position)),
                    Some(touch::Drag::Released) => Some(Message::CardReleased),
                    None => None,
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
                true => iced::event::listen_with(|event, _, id| match touch::drag(&event) {
                    // (in that window's px: Snip's toolbar may be on another screen)
                    Some(touch::Drag::Moved(position)) => Some(Message::BarMovedOn(id, position)),
                    Some(touch::Drag::Released) => Some(Message::BarReleased),
                    None => None,
                }),
                false => Subscription::none(),
            };
            // Several screens: the edge glow follows the pointer onto the one it
            // comes into, the mouse or a finger / pen (one message per crossing or
            // tap, none while it moves).
            let glow = match self.o.screens.is_empty() {
                true => Subscription::none(),
                false => iced::event::listen_with(|event, _, id| {
                    touch::came_in(&event).then_some(Message::PointerOn(id))
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
