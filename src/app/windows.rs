//! The normal windows beside the overlay: Settings (from the tray) and the
//! first-run Onboarding. At most one of each; asking again focuses the open one.
//! Also the pinned images (any number): small borderless always-on-top windows;
//! a live pin's picture keeps following a part of one window (`capture::live`).
//! Closing a window drops its state. Also owns the OS-theme feed.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::widget::{container, image, mouse_area, row, stack};
use iced::window::{self, Position};
use iced::{ContentFit, Element, Fill, Point, Size, Subscription, Task, keyboard, mouse};

use super::screens::toast::{self, Tone};
use super::screens::tray_menu;
use crate::features::onboarding;
use crate::features::snip::screens::steps::{self, Strip};
use crate::shared::kit::chip;
use crate::shared::kit::header::{self as chrome, Caption};
use crate::shared::kit::icon::Icon;
use crate::shared::kit::icon_button::{self, State};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::tokens::{radius, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::platform;
use crate::shared::theme;
use crate::{
    app::screens::{update_card, update_pill},
    features::settings,
    shared::platform::shape,
};
// The helper is `platform::windows`; these names stay reachable as `windows::..` for the callers.
use crate::features::circle::actions::{Action, Run, copy, save};
use crate::features::snip::annotate::dragout::{self, DragFile};
use crate::features::snip::capture::live;
use crate::shared::app_language::S;
use crate::shared::config::{self, Config, Theme};
use crate::shared::picture::Crop;
#[cfg(test)]
pub use crate::shared::platform::windows::CLEARED;
pub use crate::shared::platform::windows::{
    POPUP, TOAST, activate_next, appear, clear_now, clear_theme, close, cut, fade, floating,
    floating_in_captures, foreground, forget, hide_from_capture, hole, is_floating, keep_above,
    no_backdrop, opacity, reshape, screen, setup_window, show, uncut,
};
use crate::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Settings,
    Onboarding,
    /// Wayland, after a refused capture: "OpenCircle needs screenshot permission".
    Permission,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Settings(settings::Message),
    Onboarding(onboarding::Message),
    Permission(onboarding::Message),
    /// Any window closed (ours are forgotten; the overlay is `app::App`'s).
    Closed(window::Id),
    /// The OS asked a window to close (Alt+F4, the taskbar). Only Settings
    /// listens: it asks first when it has unsaved changes (#136).
    CloseRequest(window::Id),
    /// The OS light/dark mode, at start and on every change.
    OsTheme(iced::theme::Mode),
    /// From a pinned image's window.
    Pin(window::Id, PinMsg),
    /// The folder picker answered (`None` = cancelled).
    Folder(Option<std::path::PathBuf>),
    /// From a normal window's header or edges.
    Chrome(window::Id, chrome::Event),
    /// A window was resized (any of ours; the overlay's is `app::App`'s).
    Resized(window::Id, f32),
    /// Whether a normal window is maximised now (asked after every resize).
    Maximised(window::Id, bool),
    /// Test hook step (see `test_hook`).
    Test(usize, String),
    TestShot(usize, &'static str, window::Screenshot),
    /// From the update card.
    UpdateCard(update_card::Msg),
    /// From the update pill (`app::App` decides what its buttons do).
    UpdatePill(update_pill::Msg),
    /// A frame while the Updating pill's logo spins.
    PillFrame(Instant),
    /// What's new after Restart now: the Updating pill goes, the card opens.
    ShowCard(Box<update_card::Card>),
    /// The result of Settings > Check now.
    UpdateStatus(crate::shared::update::Answer),
    /// From the tray menu's window.
    TrayMenu(tray_menu::Msg),
    /// A window lost the keyboard focus (the tray menu closes then).
    Unfocused(window::Id),
    /// A tray menu row was chosen and the menu is gone: `app::App` runs it as if it came from the tray.
    Run(crate::Trigger),
    /// From the Steps strip.
    Steps(steps::Msg),
    /// A key in a window (the Steps strip's, when it has the focus).
    StepKey(window::Id, steps::Key),
    /// Ctrl+C in a window that nothing in it took (Settings: its page's Copy).
    CopyKey(window::Id),
    /// A window moved (the Steps strip docks once it stops).
    Moved(window::Id, Point),
    /// Edit the strip's `i`th step in Snip: `app::App` opens it ("Update step N").
    EditStep(usize),
}

#[derive(Default)]
pub struct Windows {
    settings: Option<(window::Id, settings::State)>,
    onboarding: Option<(window::Id, onboarding::State)>,
    permission: Option<(window::Id, onboarding::Access)>,
    pins: Vec<Pin>,
    /// Small message windows (a pill each) and their text.
    toasts: Vec<(window::Id, String, Tone)>,
    /// The update card (release notes), at most one.
    update_card: Option<(window::Id, update_card::Card)>,
    /// The update pill above the tray, at most one, and when it opened (the
    /// Updating logo's turn).
    update_pill: Option<(window::Id, update_pill::Pill, Instant)>,
    /// Its logo's turn now (0..1).
    pill_turn: f32,
    /// The tray menu (Windows), at most one.
    tray_menu: Option<(window::Id, tray_menu::Menu)>,
    /// What each normal window's header knows (see `Chrome`).
    chrome: HashMap<window::Id, Chrome>,
    /// The Steps strip and its window (none until its first step is packed).
    steps: Option<(Option<window::Id>, Strip)>,
    /// Our floating windows taken out of a grab (Linux, `leave_frame`).
    out: Option<Out>,
}

/// What `Windows::leave_frame` took out of capture number `capture`'s grab.
struct Out {
    capture: u32,
    /// Where windows hide (X11): hidden, shown again as they are.
    hidden: Vec<window::Id>,
    /// Wayland (no hiding): closed, opened again after.
    pins: Vec<Pin>,
    steps: bool,
}

/// A normal window's header state.
#[derive(Debug, Default, Clone, Copy)]
struct Chrome {
    maximised: bool,
    /// When the header was last pressed (a second press soon after = double-click).
    last_press: Option<Instant>,
}

/// Two header presses within this are a double-click (maximise / restore).
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

fn is_double_click(last: Option<Instant>, now: Instant) -> bool {
    last.is_some_and(|t| now.duration_since(t) <= DOUBLE_CLICK)
}

// ---- pinned images ----

/// A pinned image: its pixels live only as long as its window.
struct Pin {
    id: window::Id,
    image: image::Handle,
    /// Logical size at zoom 1.
    base: Size,
    zoom: f32,
    /// The pointer is over it (the Copy / Save / Close bar shows).
    hover: bool,
    /// The PNG of its last drag-out (deleted with the pin).
    dragged: Option<DragFile>,
    /// A live pin: its source (dropped with the pin: the capture stops).
    live: Option<Live>,
}

/// What a live pin has beyond a still one.
struct Live {
    _source: live::Source,
    /// Physical px per logical px where it was pinned (a frame's size -> the window's).
    scale: f32,
    /// Its step in `OPACITY`.
    opacity: usize,
    /// Clicks pass through it (back: the tray menu's "Make pins clickable").
    through: bool,
}

/// A live pin's opacity steps (its button walks them) and their tooltips.
const OPACITY: [(f32, crate::shared::app_language::S); 4] = [
    (1.0, crate::shared::app_language::S::pin_op_100),
    (0.8, crate::shared::app_language::S::pin_op_80),
    (0.6, crate::shared::app_language::S::pin_op_60),
    (0.4, crate::shared::app_language::S::pin_op_40),
];
/// Live pins open at once: each holds a window capture and a thread.
pub const MAX_LIVE: usize = 4;

#[derive(Debug, Clone)]
pub enum PinMsg {
    /// Left button down: the OS moves the window.
    Drag,
    Zoom(mouse::ScrollDelta),
    Hover(bool),
    /// Left button down on the drag handle: OS drag of the image.
    DragOut,
    Copy,
    Save,
    Close,
    /// From a live pin's source.
    Live(live::Event),
    /// A live pin's next opacity step.
    Opacity,
    /// A live pin lets clicks pass through.
    ClickThrough,
}

/// A pin starts no bigger than this (logical px); a tall scrolling capture shrinks.
const PIN_MAX: Size = Size::new(1400.0, 900.0);
const PIN_ZOOM: std::ops::RangeInclusive<f32> = 0.1..=8.0;
/// Scroll pixels that count as one wheel notch.
const PIXELS_PER_NOTCH: f32 = 40.0;
/// The hover bar needs this much window (logical px) to fit; a live pin's
/// two more buttons need `LIVE_BAR_EXTRA` more width.
const PIN_BAR_ROOM: Size = Size::new(240.0, 80.0);
const LIVE_BAR_EXTRA: f32 = 80.0;

impl Pin {
    fn size(&self) -> Size {
        self.base * self.zoom
    }

    /// The pixels again, for Copy / Save.
    fn crop(&self) -> Option<Crop> {
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = &self.image
        else {
            return None;
        };
        Some(Crop {
            width: *width,
            height: *height,
            rgba: pixels.to_vec(),
        })
    }
}

/// The zoom a new pin starts at: 1, or less to fit `PIN_MAX`.
fn fit_zoom(base: Size) -> f32 {
    (PIN_MAX.width / base.width)
        .min(PIN_MAX.height / base.height)
        .min(1.0)
}

/// `zoom` after scrolling: 10 % per wheel notch, kept within `PIN_ZOOM`.
fn zoom_after(zoom: f32, delta: mouse::ScrollDelta) -> f32 {
    let notches = match delta {
        mouse::ScrollDelta::Lines { y, .. } => y,
        mouse::ScrollDelta::Pixels { y, .. } => y / PIXELS_PER_NOTCH,
    };
    (zoom * 1.1_f32.powf(notches)).clamp(*PIN_ZOOM.start(), *PIN_ZOOM.end())
}

/// Copy or Save of a pin: the registry's own finishing action.
fn finish(action: &Action, crop: Crop) {
    if let Run::Finish(run) = action.run
        && let Err(e) = run(crop)
    {
        crate::log!("{} failed: {e}", action.id);
    }
}

/// A live pin's Opacity and Click-through glyphs (in the subset font: `tests`).
const OPACITY_GLYPH: char = '\u{e5a0}'; // lucide "blend"
const THROUGH: char = '\u{e5eb}'; // lucide "mouse-pointer-ban"

fn pin_view(pin: &Pin) -> Element<'_, PinMsg> {
    use iced::widget::tooltip::Position as Side;
    const COPY: char = '\u{e0a2}'; // lucide "copy"
    const SAVE: char = '\u{e0b6}'; // lucide "download"
    let picture = image(pin.image.clone())
        .width(Fill)
        .height(Fill)
        .content_fit(ContentFit::Fill);
    let mut layers = stack![picture];
    let size = pin.size();
    let room = PIN_BAR_ROOM.width + pin.live.as_ref().map_or(0.0, |_| LIVE_BAR_EXTRA);
    if pin.hover && size.width >= room && size.height >= PIN_BAR_ROOM.height {
        // the editing toolbar's Copy, Save and Drag chip: one look for one action
        let button = |glyph, message, label, keys| {
            hint(
                icon_button::square(glyph, State::default(), Some(message)),
                label,
                Why::Keys(keys),
                Side::Bottom,
            )
        };
        // its word "Drag" says it: no tooltip (owner rule: only when it adds)
        let drag =
            dragout::SUPPORTED.then(|| chip::drag_out(PinMsg::DragOut, crate::t!(chip_drag)));
        let live = pin.live.as_ref().map(|l| {
            let tool = |glyph, message, label| {
                let button = icon_button::square(glyph, State::default(), Some(message));
                hint(button, label, Why::Icon, Side::Bottom)
            };
            row![
                tool(
                    OPACITY_GLYPH,
                    PinMsg::Opacity,
                    crate::shared::app_language::tr(OPACITY[l.opacity].1)
                ),
                tool(THROUGH, PinMsg::ClickThrough, crate::t!(pin_through)),
            ]
            .spacing(space::GAP)
        });
        let bar = surface(
            Material::Widget,
            radius::WINDOW,
            row![
                drag,
                live,
                button(COPY, PinMsg::Copy, crate::t!(c_copy), "Ctrl+C"),
                button(SAVE, PinMsg::Save, crate::t!(c_save), "Ctrl+S"),
                button(
                    Icon::Close.glyph(),
                    PinMsg::Close,
                    crate::t!(sn_close),
                    "Esc"
                ),
            ]
            .spacing(space::GAP)
            .align_y(iced::Center),
        )
        .padding(space::BAR_PAD);
        layers = layers.push(
            container(bar)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Top)
                .padding(space::PAGE[0] / 2.0),
        );
    }
    // a thin rim around the picture (a pin window has no title bar)
    let pinned = surface(Material::Widget, 0.0, layers)
        .width(Fill)
        .height(Fill)
        .padding(1);
    mouse_area(pinned)
        .on_press(PinMsg::Drag)
        .on_double_click(PinMsg::Close)
        .on_scroll(PinMsg::Zoom)
        .on_enter(PinMsg::Hover(true))
        .on_exit(PinMsg::Hover(false))
        .into()
}

/// Esc, Ctrl+C and Ctrl+S in whichever window has the focus; `Windows::update`
/// ignores ids that are not pins.
fn pin_keys() -> Subscription<Msg> {
    iced::event::listen_with(|event, _status, window| {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) = event
        else {
            return None;
        };
        let msg = match (&key, modifiers.command(), key.to_latin(physical_key)) {
            (keyboard::Key::Named(keyboard::key::Named::Escape), ..) => PinMsg::Close,
            (_, true, Some('c')) => PinMsg::Copy,
            (_, true, Some('s')) => PinMsg::Save,
            _ => return None,
        };
        Some(Msg::Pin(window, msg))
    })
}

/// Ctrl+C that no widget took (a field copies its own selection), with its
/// window: `Msg::CopyKey`.
fn copy_keys() -> Subscription<Msg> {
    iced::event::listen_with(|event, status, window| match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) if status == iced::event::Status::Ignored
            && modifiers.command()
            && key.to_latin(physical_key) == Some('c') =>
        {
            Some(Msg::CopyKey(window))
        }
        _ => None,
    })
}

/// While the Steps strip is open: its window's moves (it docks once it stops)
/// and the keys of whichever window has the focus (`Windows::update` keeps
/// the strip's).
fn steps_events() -> Subscription<Msg> {
    use keyboard::key::Named;
    iced::event::listen_with(|event, _status, window| match event {
        iced::Event::Window(window::Event::Moved(at)) => Some(Msg::Moved(window, at)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) => {
            use steps::Key;
            let ctrl = modifiers.command();
            let key = match (&key, key.to_latin(physical_key)) {
                (keyboard::Key::Named(Named::ArrowUp), _) if ctrl => Key::MoveUp,
                (keyboard::Key::Named(Named::ArrowDown), _) if ctrl => Key::MoveDown,
                (keyboard::Key::Named(Named::ArrowUp), _) => Key::Up,
                (keyboard::Key::Named(Named::ArrowDown), _) => Key::Down,
                (keyboard::Key::Named(Named::Enter), _) => Key::Enter,
                (keyboard::Key::Named(Named::Delete), _) => Key::Delete,
                (_, Some('c')) if ctrl && modifiers.shift() => Key::CopyAll,
                (_, Some('c')) if ctrl => Key::Copy,
                (_, Some('z')) if ctrl => Key::Undo,
                _ => return None,
            };
            Some(Msg::StepKey(window, key))
        }
        _ => None,
    })
}

/// A drag of the strip has stopped once it has not moved for this long.
const STEPS_SETTLE: Duration = Duration::from_millis(250);

impl Windows {
    /// The Steps strip's window, once open.
    fn steps_window(&self) -> Option<window::Id> {
        self.steps.as_ref().and_then(|(id, _)| *id)
    }

    /// Steps in the strip (`None`: no strip, or it says "Steps cleared"):
    /// Snip's toolbar shows "Add step N" while there is one.
    pub fn steps(&self) -> Option<usize> {
        let (_, strip) = self.steps.as_ref().filter(|(_, s)| !s.cleared)?;
        Some(strip.len())
    }

    /// The strip's `i`th step's capture (to edit it).
    pub fn step_crop(&self, i: usize) -> Option<Crop> {
        self.steps.as_ref()?.1.crop(i)
    }

    /// Adds `crop` (a snip, annotated and flattened) as the next step, or in
    /// place of the `update`th; packed on a thread. The first step opens the
    /// strip on `monitor` (`inset`: its taskbar, logical px).
    pub fn add_step(
        &mut self,
        crop: Crop,
        update: Option<usize>,
        monitor: platform::monitor::Place,
        inset: f32,
    ) -> Task<Msg> {
        let fresh = match &self.steps {
            None => true,
            Some((_, strip)) => strip.cleared && update.is_none(),
        };
        if fresh {
            // a cleared strip's steps go now (its window shows the new strip)
            let id = self.steps.take().and_then(|(id, _)| id);
            let rtl = crate::shared::kit::dir::rtl(); // the strip docks on the side the text starts
            self.steps = Some((id, Strip::new(monitor, inset, rtl)));
        }
        crate::app::background::background(move |tx| match steps::Step::new(&crop) {
            Ok(step) => {
                let _ = tx.unbounded_send(std::sync::Arc::new(step));
            }
            Err(e) => crate::log!("[opencircle] step not added: {e}"),
        })
        .map(move |step| Msg::Steps(steps::Msg::Ready(step, update)))
    }

    fn on_steps(&mut self, msg: steps::Msg) -> Task<Msg> {
        let Some((id, strip)) = &mut self.steps else {
            return Task::none();
        };
        let window = *id;
        let todo = match msg {
            steps::Msg::Ready(step, update) => {
                match update {
                    Some(i) => strip.replace(i, step),
                    None if strip.push(step) => {}
                    None => crate::log!("[opencircle] steps full: not added"),
                }
                strip.cleared = false;
                crate::log!(
                    "[opencircle] steps: {} ({} KB in memory)",
                    strip.len(),
                    strip.bytes() / 1024
                );
                let place = strip.place();
                let Some(id) = window else {
                    return self.open_steps(place);
                };
                return self.place_steps(id, place);
            }
            steps::Msg::Expire(n) => {
                return match window {
                    Some(id) if strip.cleared && strip.clears == n => close(id),
                    _ => Task::none(),
                };
            }
            steps::Msg::Copied(ok) => return self.steps_toast(ok),
            // stopped: its window's scale now says which monitor it is on
            steps::Msg::Settle(n) => {
                return match window {
                    Some(id) => {
                        window::scale_factor(id).map(move |k| Msg::Steps(steps::Msg::Docked(n, k)))
                    }
                    None => Task::none(),
                };
            }
            msg => strip.update(msg),
        };
        let Some(todo) = todo else {
            return Task::none();
        };
        match (todo, window) {
            // (no window yet: its first step is still being packed)
            (steps::Do::Drag | steps::Do::Resize | steps::Do::Cleared, None) => Task::none(),
            (steps::Do::Drag, Some(id)) => window::drag(id),
            (steps::Do::Resize, Some(id)) => {
                let place = strip.place();
                self.place_steps(id, place)
            }
            (steps::Do::Cleared, Some(id)) => {
                let clears = strip.clears;
                let place = strip.place();
                let place = self.place_steps(id, place);
                let expire = crate::app::background::background(move |tx| {
                    std::thread::sleep(Duration::from_millis(steps::UNDO_MS));
                    let _ = tx.unbounded_send(());
                })
                .map(move |()| Msg::Steps(steps::Msg::Expire(clears)));
                Task::batch([place, expire])
            }
            (steps::Do::Wait(moves), _) => crate::app::background::background(move |tx| {
                std::thread::sleep(STEPS_SETTLE);
                let _ = tx.unbounded_send(());
            })
            .map(move |()| Msg::Steps(steps::Msg::Settle(moves))),
            (steps::Do::Edit(i), _) => Task::done(Msg::EditStep(i)),
            (steps::Do::Copy(i), _) => {
                let ok = strip.crop(i).is_some_and(copy_image);
                self.steps_toast(ok)
            }
            (steps::Do::CopyAll, _) => {
                use crate::shared::kit::themes::c;
                let crops = strip.crops();
                let colours = [c().bg, c().accent, c().rim].map(|c| c.into_rgba8());
                let rtl = strip.rtl;
                crate::app::background::background(move |tx| {
                    let ok = steps::stack_numbered(&crops, colours, rtl).is_some_and(copy_image);
                    let _ = tx.unbounded_send(ok);
                })
                .map(|ok| Msg::Steps(steps::Msg::Copied(ok)))
            }
        }
    }

    /// Puts the strip's window `id` at `place`. Wayland: winit applies a
    /// program's resize without a `Resized` event, so iced kept drawing the old
    /// size and the compositor stretched it (Copy all pushed out of the window):
    /// the strip's window is replaced by one opened at the new size instead.
    fn place_steps(&mut self, id: window::Id, place: platform::monitor::Place) -> Task<Msg> {
        if platform::monitor::can_hide_windows() {
            return platform::monitor::place_window(id, place);
        }
        let open = self.open_steps(place); // the strip's window is the new one now
        Task::batch([open, close(id)])
    }

    /// The strip's window at `place` (physical px): floating (so out of
    /// captures), faded in once there.
    fn open_steps(&mut self, place: platform::monitor::Place) -> Task<Msg> {
        let (id, open) = floating(
            window::Settings {
                size: place.size(),
                position: Position::Specific(place.logical_origin()),
                visible: false,
                resizable: false,
                ..window::Settings::default()
            },
            steps::RADIUS,
            POPUP,
        );
        if let Some((window, _)) = &mut self.steps {
            *window = Some(id);
        }
        open.then(move |id| {
            platform::monitor::place_window::<()>(id, place).chain(appear(id, POPUP))
        })
        .discard()
    }

    /// "Copied" (or why not) under the strip.
    fn steps_toast(&mut self, ok: bool) -> Task<Msg> {
        let text = match ok {
            true => copy::COPIED_PILL.to_string(),
            false => t!(pill_cant_copy).to_string(),
        };
        let text = text.as_str();
        let Some((_, strip)) = &self.steps else {
            return Task::none();
        };
        let at = strip.under(toast_size(text));
        self.toast(text, ok, Some(at), TOAST_MS)
    }
}

/// `crop` on the clipboard through THE copy path (Win+V history too).
fn copy_image(crop: Crop) -> bool {
    match copy::ACTION.run {
        Run::Finish(run) => run(crop).is_ok(),
        _ => false,
    }
}

/// While the tray menu is open: its keys (it has the focus; it closes when it loses it) and
/// any window's focus loss.
fn tray_menu_events() -> Subscription<Msg> {
    use keyboard::key::Named;
    iced::event::listen_with(|event, _status, window| match event {
        iced::Event::Window(window::Event::Unfocused) => Some(Msg::Unfocused(window)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(named),
            ..
        }) => {
            let key = match named {
                Named::ArrowUp => tray_menu::Key::Up,
                Named::ArrowDown => tray_menu::Key::Down,
                Named::Enter => tray_menu::Key::Enter,
                Named::Escape => tray_menu::Key::Escape,
                _ => return None,
            };
            Some(Msg::TrayMenu(tray_menu::Msg::Key(key)))
        }
        _ => None,
    })
}

impl Windows {
    /// Call from `boot`: applies the saved theme, reads the OS theme, and opens
    /// the onboarding when there is no config file yet (first run).
    pub fn start(&mut self) -> Task<Msg> {
        theme::apply(config::current().theme);
        let mut tasks = vec![iced::system::theme().map(Msg::OsTheme), test_hook()];
        if Config::path().is_some_and(|p| !p.exists()) {
            tasks.push(self.open(Kind::Onboarding));
        } else {
            tasks.push(self.taken_once(crate::shared::platform::hotkey::taken_shortcuts()));
        }
        Task::batch(tasks)
    }

    /// With no tray to reach the app from: the onboarding while it is open (first run), else Settings.
    pub fn open_main(&mut self) -> Task<Msg> {
        self.open(match self.onboarding.is_some() {
            true => Kind::Onboarding,
            false => Kind::Settings,
        })
    }

    /// Shortcuts another app holds (`taken`, refused at the start): one quiet
    /// notice, once; the next start with the same ones says nothing (Settings
    /// shows each such row's note).
    fn taken_once(&mut self, taken: Vec<String>) -> Task<Msg> {
        let told = config::current().general.shortcuts_taken_shown;
        let notice = taken_notice(&taken, &told);
        if taken != told {
            std::thread::spawn(move || {
                if let Err(e) = config::update(|c| c.general.shortcuts_taken_shown = taken) {
                    crate::log!("[opencircle] the taken shortcuts were not saved: {e}");
                }
            });
        }
        match notice {
            Some(text) => self.toast(&text, Tone::Neutral, None, NOTICE_MS),
            None => Task::none(),
        }
    }

    /// The open window of this kind, if any.
    fn id(&self, kind: Kind) -> Option<window::Id> {
        match kind {
            Kind::Settings => self.settings.as_ref().map(|(id, _)| *id),
            Kind::Onboarding => self.onboarding.as_ref().map(|(id, _)| *id),
            Kind::Permission => self.permission.as_ref().map(|(id, _)| *id),
        }
    }

    /// Pins `crop` (physical px, `scale` per logical px: its monitor's) in its
    /// own always-on-top window, its top-left at `at` (physical desktop px, on
    /// any monitor) or centred. Any number can be open.
    pub fn pin(&mut self, crop: Crop, at: Option<(i32, i32)>, scale: f32) -> Task<Msg> {
        self.open_pin(crop, at, scale).1
    }

    fn open_pin(
        &mut self,
        crop: Crop,
        at: Option<(i32, i32)>,
        scale: f32,
    ) -> (window::Id, Task<Msg>) {
        let scale = scale.max(0.1);
        let base = Size::new(crop.width as f32, crop.height as f32) * (1.0 / scale);
        let zoom = fit_zoom(base);
        let px = |v: f32| (v * scale).round() as u32;
        let place = at.map(|origin| platform::monitor::Place {
            origin,
            px: (px(base.width * zoom), px(base.height * zoom)),
            scale,
        });
        let (id, open) = floating(
            window::Settings {
                size: base * zoom,
                position: place.map_or(Position::Centered, |p| {
                    Position::Specific(p.logical_origin())
                }),
                // placed first, then faded in (below)
                visible: place.is_none(),
                resizable: false,
                ..window::Settings::default()
            },
            0.0, // a picture: square
            POPUP,
        );
        // At its exact place in physical px before it shows: iced places a new
        // window in logical px at the scale of the monitor it opens on, not the
        // one it goes to (#149; a pin of another screen at another scale).
        let open = match place {
            Some(place) => open
                .then(move |id| {
                    platform::monitor::place_window::<()>(id, place).chain(appear(id, POPUP))
                })
                .discard(),
            None => open.discard(),
        };
        crate::log!("[opencircle] pinned {}x{}", crop.width, crop.height);
        self.pins.push(Pin {
            id,
            image: image::Handle::from_rgba(crop.width, crop.height, crop.rgba),
            base,
            zoom,
            hover: false,
            dragged: None,
            live: None,
        });
        (id, open)
    }

    /// Live pins open now.
    fn live_pins(&self) -> usize {
        self.pins.iter().filter(|p| p.live.is_some()).count()
    }

    /// A live pin of `region` (physical desktop px) of the window `source`,
    /// showing `first` until the window's first frame: a pin (as `pin`) whose
    /// picture keeps following that part of the window (`capture::live`). At
    /// most `MAX_LIVE`; a region beside the window's client area is refused (a toast).
    pub fn live_pin(
        &mut self,
        source: isize,
        region: crate::shared::platform::capture::window_rect::WinRect,
        first: Crop,
        at: Option<(i32, i32)>,
        scale: f32,
    ) -> Task<Msg> {
        if self.live_pins() >= MAX_LIVE {
            return self.toast(t!(pill_pins_max), Tone::Neutral, None, TOAST_MS);
        }
        let Some(pick) = (live::geom(source).ok()).and_then(|g| live::pick(region, &g)) else {
            let text = t!(pill_pins_pick);
            return self.toast(text, Tone::Neutral, None, TOAST_MS);
        };
        let (id, open) = self.open_pin(first, at, scale);
        let (tx, rx) = iced::futures::channel::mpsc::unbounded();
        let (source, run) = live::source(source, pick, move |e| tx.unbounded_send(e).is_ok());
        if let Err(e) = std::thread::Builder::new()
            .name("live pin".into())
            .spawn(run)
        {
            crate::log!("cannot start a thread: {e}");
        }
        if let Some(pin) = self.pins.last_mut() {
            pin.live = Some(Live {
                _source: source,
                scale,
                opacity: 0,
                through: false,
            });
        }
        crate::log!("[opencircle] live pin {}x{} of a window", pick.w, pick.h);
        let frames = Task::stream(rx).map(move |e| Msg::Pin(id, PinMsg::Live(e)));
        Task::batch([open, frames])
    }

    /// The tray menu's "Make pins clickable": every live pin takes clicks again.
    fn clickable_pins(&mut self) -> Task<Msg> {
        let through = self.pins.iter_mut().filter_map(|p| {
            let live = p.live.as_mut().filter(|l| l.through)?;
            live.through = false;
            Some(window::disable_mouse_passthrough(p.id))
        });
        Task::batch(through.collect::<Vec<_>>())
    }

    /// Some live pin lets clicks pass through (the tray menu offers them back).
    fn pins_through(&self) -> bool {
        (self.pins.iter()).any(|p| p.live.as_ref().is_some_and(|l| l.through))
    }

    /// A small message window: the toast pill (`screens::toast`) with `text`,
    /// floating over everything for `ms`, at `at` (physical desktop px, its
    /// monitor's scale) or centred. `tone` colours its dot (`true` = done,
    /// `false` = failed). The window is exactly the pill (see `floating`), so
    /// nothing shows around it.
    pub fn toast(
        &mut self,
        text: &str,
        tone: impl Into<Tone>,
        at: Option<platform::monitor::Place>,
        ms: u64,
    ) -> Task<Msg> {
        let size = toast_size(text);
        let (id, open) = floating(
            window::Settings {
                size,
                position: match at {
                    Some(place) => Position::Specific(place.logical_origin()),
                    None => Position::Centered,
                },
                // placed first, then faded in (below)
                visible: at.is_none(),
                resizable: false,
                ..window::Settings::default()
            },
            toast::RADIUS,
            TOAST,
        );
        // At its exact place in physical px before it fades in (its rise moves
        // from there): iced places a new window in logical px at the scale of
        // the monitor it opens on, not the one it goes to (#149).
        let open = match at {
            Some(place) => open
                .then(move |id| {
                    platform::monitor::place_window::<()>(id, place).chain(appear(id, TOAST))
                })
                .discard(),
            None => open.discard(),
        };
        self.toasts.push((id, text.to_string(), tone.into()));
        // gone after `ms` (a thread sleeps, the UI never waits): fades out
        let (done, wait) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            let _ = done.send(());
        });
        open.chain(Task::future(wait).then(move |_| close(id)))
    }

    /// The open toasts' text and whether each says done (tests).
    #[cfg(test)]
    pub fn toasts(&self) -> Vec<(String, bool)> {
        (self.toasts.iter())
            .map(|(_, text, tone)| (text.clone(), *tone == Tone::Ok))
            .collect()
    }

    /// The update card (release notes) with `card`; replaces an open one's content.
    pub fn update_card(&mut self, card: update_card::Card) -> Task<Msg> {
        let size = spoken(|| card.size());
        let old = self.update_card.take().map(|(id, _)| id);
        // Wayland can't resize it in place (`monitor::resize`): a new window replaces it
        let reuse = old.filter(|_| platform::monitor::can_hide_windows());
        if let Some(id) = reuse {
            self.update_card = Some((id, card));
            return platform::monitor::resize(id, size);
        }
        let (id, open) = floating_in_captures(
            window::Settings {
                size,
                position: Position::Centered,
                resizable: false,
                ..window::Settings::default()
            },
            update_card::RADIUS,
            POPUP,
        );
        self.update_card = Some((id, card));
        match old {
            Some(old) => Task::batch([open.discard(), close(old)]),
            None => open.discard(),
        }
    }

    /// The update pill (`pill`; `None` = it goes): docked above the tray, bottom
    /// right of the main screen, 16 px from its edge and 12 px above the taskbar
    /// (GNOME on Wayland places it itself). Never takes the focus. A new pill
    /// replaces the open one.
    pub fn update_pill(&mut self, pill: Option<update_pill::Pill>) -> Task<Msg> {
        let old = self.update_pill.take().map(|(id, ..)| close(id));
        let Some(pill) = pill else {
            return old.unwrap_or_else(Task::none);
        };
        let size = spoken(|| pill.size());
        let place = platform::monitor::places()
            .into_iter()
            .min_by_key(|m| (m.origin.0 != 0 || m.origin.1 != 0, m.origin))
            .map(|m| {
                let inset = platform::monitor::taskbar_inset(m.logical_origin(), m.size(), m.scale);
                pill_place(m, inset, size)
            });
        let (id, open) = floating_in_captures(
            window::Settings {
                size,
                position: match place {
                    Some(p) => Position::Specific(p.logical_origin()),
                    None => Position::Default,
                },
                visible: place.is_none(),
                resizable: false,
                ..window::Settings::default()
            },
            update_pill::RADIUS,
            TOAST,
        );
        let open = match place {
            Some(place) => open
                .then(move |id| {
                    platform::monitor::place_window::<()>(id, place).chain(appear(id, TOAST))
                })
                .discard(),
            None => open.discard(),
        };
        self.update_pill = Some((id, pill, Instant::now()));
        self.pill_turn = 0.0;
        Task::batch([old.unwrap_or_else(Task::none), open])
    }

    /// The update's state changed (a check answered, an update now waits):
    /// Settings > General shows it.
    pub fn refresh_update(&mut self) {
        if let Some((_, s)) = &mut self.settings {
            let waiting = crate::shared::update::offer().map(|o| (o.version, o.how));
            s.set_update(waiting, crate::shared::update::checked());
        }
    }

    /// The update pill shown now, if any.
    pub fn shown_pill(&self) -> Option<&update_pill::Kind> {
        self.update_pill.as_ref().map(|(_, p, _)| &p.kind)
    }

    /// The What's new card of the update waiting now (Settings > What's new).
    fn whats_next(&mut self) -> Task<Msg> {
        match crate::shared::update::offer() {
            Some(o) => self.update_card(update_card::Card::preview(&o.version, &o.notes, &o.url)),
            None => Task::none(),
        }
    }

    /// The tray menu at the tray icon (`icon`: physical px), with the keyboard; `live_on` = live
    /// translate runs (its row's ✓). An open one goes first (a second right-click).
    pub fn tray_menu(&mut self, icon: [i32; 4], live_on: bool) -> Task<Msg> {
        let cfg = config::current();
        let mut menu = tray_menu::Menu::new(&cfg.labs, &cfg, live_on);
        if let Some(label) = platform::tray::update_label() {
            menu = menu.with_update(label);
        }
        if self.pins_through() {
            menu = menu.with_click_pins();
        }
        let size = spoken(|| menu.size());
        let place = platform::tray::menu_place(icon, size);
        crate::log!(
            "[opencircle] tray menu at {:?} (icon {icon:?})",
            place.origin
        );
        let before = match self.tray_menu.take() {
            Some((id, _)) => window::close(id),
            None => Task::none(),
        };
        let (id, open) = floating(
            window::Settings {
                size,
                position: Position::Specific(place.logical_origin()),
                resizable: false,
                ..window::Settings::default()
            },
            tray_menu::RADIUS,
            POPUP,
        );
        self.tray_menu = Some((id, menu));
        // the exact place in physical px, whatever scale iced moved the window at
        let placed = platform::monitor::place_window(id, place);
        Task::batch([
            before,
            open.discard().chain(placed).chain(window::gain_focus(id)),
        ])
    }

    /// The tray menu leaves; a chosen `entry` runs once it is off the screen (a capturing one
    /// after the screen redrew what it covered too).
    fn leave_tray_menu(&mut self, entry: Option<platform::tray::Entry>) -> Task<Msg> {
        let Some((id, _)) = self.tray_menu.take() else {
            return Task::none();
        };
        let Some(entry) = entry else {
            return close(id);
        };
        if entry == platform::tray::Entry::ClickPins {
            return Task::batch([close(id), self.clickable_pins()]);
        }
        let trigger = entry.trigger(std::time::Instant::now());
        let settle = match entry.captures() {
            true => platform::tray::SETTLE,
            false => Duration::ZERO,
        };
        // after the fade-out (`close` waits for it); a thread sleeps, the UI never waits
        close(id).chain(Task::future(async move {
            let (done, wait) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(settle);
                let _ = done.send(());
            });
            let _ = wait.await;
            Msg::Run(trigger)
        }))
    }

    /// Linux: our floating windows (Steps strip, pins, pills, the tray menu; not
    /// the update's, which show in captures, nor the AI chat's: `App::leave_frame`) out of capture number `capture`'s
    /// grab: no OS call keeps a window out of an X11 grab or the Wayland portal's
    /// shot (Windows: every floating window is out of every capture,
    /// `windows::floating`, so `None` there). Hidden where windows hide, else
    /// closed (pills for good; the others open again in `back_in_frame`).
    /// `None`: nothing to take out (the grab needn't wait).
    pub fn leave_frame(&mut self, capture: u32) -> Option<Task<Msg>> {
        if cfg!(windows) {
            return None;
        }
        self.leave_frame_as(capture, platform::monitor::can_hide_windows())
    }

    fn leave_frame_as(&mut self, capture: u32, can_hide: bool) -> Option<Task<Msg>> {
        if let Some(out) = &mut self.out {
            out.capture = capture; // still out: this grab's now
            return None;
        }
        let toasts: Vec<_> = self.toasts.drain(..).map(|(id, ..)| id).collect();
        let menu = self.tray_menu.take().map(|(id, _)| id);
        let mut gone: Vec<window::Id> = toasts.into_iter().chain(menu).collect();
        let mut out = Out {
            capture,
            hidden: Vec::new(),
            pins: Vec::new(),
            steps: false,
        };
        let steps = self.steps.as_mut().and_then(|(id, _)| match can_hide {
            true => *id,
            false => id.take(), // its strip stays
        });
        if can_hide {
            out.hidden = (self.pins.iter().map(|p| p.id)).chain(steps).collect();
        } else {
            out.steps = steps.is_some();
            out.pins = std::mem::take(&mut self.pins);
            gone.extend(out.pins.iter().map(|p| p.id).chain(steps));
        }
        if gone.is_empty() && out.hidden.is_empty() {
            return None;
        }
        // at once (no fade): the grab follows
        let hide = (out.hidden.iter()).map(|id| window::set_mode(*id, window::Mode::Hidden));
        let tasks: Vec<_> = (gone.into_iter().map(|id| {
            forget(id);
            window::close(id)
        }))
        .chain(hide)
        .collect();
        crate::log!(
            "[opencircle] {} floating windows out of the grab",
            tasks.len()
        );
        self.out = Some(out);
        Some(Task::batch(tasks))
    }

    /// Some window is out of a grab (`leave_frame`).
    #[cfg(test)]
    pub fn out_of_frame(&self) -> bool {
        self.out.is_some()
    }

    /// The windows `leave_frame` took out of capture `capture`'s grab (any
    /// capture's: `None`) come back: shown again, or opened again where they
    /// can't hide (the strip at its place; pins and the card where the
    /// desktop puts them: Wayland places new windows itself).
    pub fn back_in_frame(&mut self, capture: Option<u32>) -> Task<Msg> {
        if capture.is_some_and(|n| self.out.as_ref().is_some_and(|o| o.capture != n)) {
            return Task::none();
        }
        let Some(out) = self.out.take() else {
            return Task::none();
        };
        crate::log!("[opencircle] floating windows back");
        let mut tasks: Vec<Task<Msg>> = (out.hidden.into_iter())
            .map(|id| show(id, window::Mode::Windowed))
            .collect();
        for mut pin in out.pins {
            // ponytail: a live pin's frames stay keyed by its first window (Wayland has no live pins).
            let (id, open) = floating(
                window::Settings {
                    size: pin.base * pin.zoom,
                    resizable: false,
                    ..window::Settings::default()
                },
                0.0,
                POPUP,
            );
            pin.id = id;
            self.pins.push(pin);
            tasks.push(open.discard());
        }
        let place = self.steps.as_ref().map(|(_, strip)| strip.place());
        if let Some(place) = place.filter(|_| out.steps) {
            tasks.push(self.open_steps(place));
        }
        Task::batch(tasks)
    }

    /// Something the user is in the middle of lives in a window: Settings,
    /// onboarding, the permission prompt or a pinned image (an update waits).
    pub fn busy(&self) -> bool {
        self.settings.is_some()
            || self.onboarding.is_some()
            || self.permission.is_some()
            || !self.pins.is_empty()
            || self.steps.is_some()
            || self.out.is_some()
    }

    /// Opens the window, or focuses it when it is already open.
    pub fn open(&mut self, kind: Kind) -> Task<Msg> {
        if let Some(id) = self.id(kind) {
            crate::log!("[opencircle] {kind:?} window is open: focusing it");
            return window::minimize(id, false).chain(window::gain_focus(id));
        }
        crate::log!("[opencircle] opening the {kind:?} window");
        // from a shortcut in the shell's overview (the permission window), or
        // a second launch's token: in front, not behind the overview (#173)
        #[cfg(target_os = "linux")]
        crate::shared::platform::sys::shell::leave_overview();
        activate_next();
        let (id, open) = normal(kind);
        match kind {
            Kind::Settings => {
                self.settings = Some((id, settings::State::open()));
                self.refresh_update();
            }
            Kind::Onboarding => self.onboarding = Some((id, onboarding::State::open())),
            Kind::Permission => self.permission = Some((id, onboarding::Access::Unasked)),
        }
        self.chrome.insert(id, Chrome::default());
        // the preview stage decodes at the px it is shown at
        let scale = window::scale_factor(id).map(|s| Msg::Settings(settings::Message::Scale(s)));
        let height = window::size(id).map(|s| Msg::Settings(settings::Message::Height(s.height)));
        let open = open.discard().chain(window::gain_focus(id));
        match kind {
            Kind::Settings => open.chain(scale).chain(height),
            _ => open,
        }
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.handle(msg);
        // A shortcut field that records gets the keys: the global shortcuts are
        // held off until it stops (a key set, Esc, another page, the window closed).
        let recording = self.settings.as_ref().is_some_and(|(_, s)| s.recording());
        crate::shared::platform::hotkey::suspend(recording);
        task
    }

    fn handle(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Settings(settings::Message::Caption(caption)) => {
                if let Some(id) = self.id(Kind::Settings) {
                    return self.on_chrome(id, chrome::Event::from(caption));
                }
            }
            Msg::Settings(msg) => {
                // A landed write takes effect even if the window was closed meanwhile.
                if let settings::Message::Written(done) = &msg
                    && let Some((before, now)) = done.landed()
                {
                    apply_from(before, now.clone());
                }
                if let Some((id, state)) = &mut self.settings {
                    let id = *id;
                    let outcome = settings::update(state, msg);
                    let leaving = state.leaving();
                    let keys = state.take_key_jobs();
                    let keys = Task::batch(keys.into_iter().map(|(song, job)| {
                        crate::app::background::background(move |tx| {
                            let _ = tx.unbounded_send(job.run());
                        })
                        .map(move |k| {
                            Msg::Settings(match song {
                                true => settings::Message::SongKey(k),
                                false => settings::Message::ApiKey(k),
                            })
                        })
                    }));
                    match outcome {
                        settings::Outcome::Saved(_) => {} // applied above, when the write landed
                        settings::Outcome::Write(w) => {
                            return Task::batch([
                                keys,
                                crate::app::background::background(move |tx| {
                                    let _ = tx.unbounded_send(w.run());
                                })
                                .map(|d| Msg::Settings(settings::Message::Written(d))),
                            ]);
                        }
                        settings::Outcome::Browse => return Task::batch([keys, pick_folder()]),
                        settings::Outcome::OpenLink(url) => open_url(&url),
                        settings::Outcome::CopyDiagnostics => {
                            let report = crate::shared::diagnostics::report(
                                &crate::shared::config::current(),
                            );
                            let toast = match crate::features::circle::actions::copy::text(report) {
                                Ok(()) => self.toast(t!(pill_diag), true, None, TOAST_MS),
                                Err(e) => {
                                    crate::log!("[opencircle] copy diagnostics failed: {e}");
                                    self.toast(t!(pill_cant_copy), false, None, TOAST_MS)
                                }
                            };
                            return Task::batch([keys, toast]);
                        }
                        settings::Outcome::CheckUpdate => crate::shared::update::check_now(),
                        settings::Outcome::RestartToUpdate => {
                            return Task::batch([
                                keys,
                                Task::done(Msg::Run(crate::Trigger::RestartToUpdate)),
                            ]);
                        }
                        settings::Outcome::WhatsNew => {
                            return Task::batch([keys, self.whats_next()]);
                        }
                        settings::Outcome::History(clear) => {
                            let tidy = crate::app::background::background(move |tx| {
                                let _ = tx.unbounded_send(crate::shared::history::tidy(&clear));
                            });
                            let kept = tidy.map(|k| Msg::Settings(settings::Message::Kept(k)));
                            return Task::batch([keys, kept]);
                        }
                        settings::Outcome::Fetch(key, names) => {
                            let fetch = crate::app::background::background(move |tx| {
                                use settings::previews::online;
                                let ok = online::files(&names)
                                    .is_some_and(|f| online::fetch(&f).is_ok());
                                let _ = tx.unbounded_send(ok);
                            });
                            let done = fetch
                                .map(move |ok| Msg::Settings(settings::Message::Fetched(key, ok)));
                            return Task::batch([keys, done]);
                        }
                        settings::Outcome::Focus(field) => {
                            return Task::batch([keys, iced::widget::operation::focus(field)]);
                        }
                        settings::Outcome::LoadModels(job) => {
                            let load = crate::app::background::background(move |tx| {
                                let _ = tx.unbounded_send(job.run());
                            });
                            let done = load.map(|(ai, n)| {
                                Msg::Settings(settings::Message::ModelsLoaded(ai, n))
                            });
                            return Task::batch([keys, done]);
                        }
                        settings::Outcome::UpdateModels => {
                            let update = crate::app::background::background(move |tx| {
                                let (pairs, _) = crate::features::translate::mt::installed();
                                let progress = |done: u64, total: u64| {
                                    let _ = tx.unbounded_send(settings::Message::ModelsProgress(
                                        done as f32 / total.max(1) as f32,
                                    ));
                                };
                                let got = match crate::features::translate::mt::update(
                                    &pairs, progress,
                                ) {
                                    // Cancel: not a failure (the row is counted again)
                                    Err(crate::features::text_reading::OcrError::Stopped) => Ok(()),
                                    got => got.map_err(|e| {
                                        crate::features::text_reading::screens::model_download::why(
                                            &e,
                                        )
                                    }),
                                };
                                let _ = tx.unbounded_send(settings::Message::ModelsUpdated(got));
                            });
                            return Task::batch([keys, update.map(Msg::Settings)]);
                        }
                        settings::Outcome::None => {}
                    }
                    if leaving {
                        return Task::batch([keys, window::close(id)]);
                    }
                    return keys;
                }
            }
            // through Settings' own path, so its auto-save write runs
            Msg::Folder(dir) => return self.handle(Msg::Settings(settings::Message::Folder(dir))),
            Msg::Pin(id, msg) => return self.on_pin(id, msg),
            Msg::Steps(msg) => return self.on_steps(msg),
            Msg::StepKey(id, key) if self.steps_window() == Some(id) => {
                return self.on_steps(steps::Msg::Key(key));
            }
            Msg::Moved(id, at) if self.steps_window() == Some(id) => {
                return self.on_steps(steps::Msg::Moved(at));
            }
            Msg::CopyKey(id) => {
                let copy = (self.settings.as_ref())
                    .filter(|(settings, _)| *settings == id)
                    .and_then(|(_, state)| settings::copy_key(state));
                if let Some(copy) = copy {
                    return self.update(Msg::Settings(copy));
                }
            }
            Msg::StepKey(..) | Msg::Moved(..) | Msg::EditStep(_) => {}
            Msg::UpdateStatus(status) => {
                if let Some((_, state)) = &mut self.settings {
                    settings::update(state, settings::Message::UpdateStatus(status));
                }
                self.refresh_update();
            }
            Msg::UpdateCard(msg) => {
                let Some((id, card)) = &self.update_card else {
                    return Task::none();
                };
                let id = *id;
                match msg {
                    update_card::Msg::Drag => return window::drag(id),
                    update_card::Msg::Close => return close(id),
                    update_card::Msg::Link(url) => open_url(&url),
                    update_card::Msg::Page => open_url(&card.url),
                    update_card::Msg::Fold(i) => {
                        let mut card = card.clone();
                        card.fold(i);
                        return self.update_card(card);
                    }
                }
            }
            Msg::UpdatePill(_) => {} // the App's (`App::on_update_pill`)
            Msg::ShowCard(card) => {
                let pill = match self.update_pill.as_ref().is_some_and(|(_, p, _)| p.spins()) {
                    true => self.update_pill(None),
                    false => Task::none(),
                };
                return Task::batch([pill, self.update_card(*card)]);
            }
            Msg::PillFrame(now) => {
                if let Some((_, _, opened)) = &self.update_pill {
                    self.pill_turn = crate::shared::kit::spinner::turn(now.duration_since(*opened));
                }
            }
            Msg::Onboarding(onboarding::Message::Caption(caption)) => {
                if let Some(id) = self.id(Kind::Onboarding) {
                    return self.on_chrome(id, chrome::Event::from(caption));
                }
            }
            Msg::Onboarding(msg) => {
                if let Some((id, state)) = &mut self.onboarding {
                    let outcome = onboarding::update(state, msg);
                    let keys = state.take_key_jobs();
                    let keys = Task::batch(keys.into_iter().map(|job| {
                        crate::app::background::background(move |tx| {
                            let _ = tx.unbounded_send(job.run());
                        })
                        .map(|k| {
                            Msg::Onboarding(onboarding::Message::Settings(
                                settings::Message::ApiKey(k),
                            ))
                        })
                    }));
                    match outcome {
                        onboarding::Outcome::Finished(cfg) => {
                            let id = *id;
                            self.onboarding = None;
                            let notice = tray_notice(&cfg.shortcut);
                            apply(cfg);
                            return Task::batch([
                                keys,
                                window::close(id),
                                self.toast(&notice, Tone::Neutral, None, NOTICE_MS),
                            ]);
                        }
                        onboarding::Outcome::AskScreenshot => {
                            return Task::batch([keys, ask_screenshot(Msg::Onboarding)]);
                        }
                        onboarding::Outcome::None => return keys,
                    }
                }
            }
            Msg::Permission(msg) => {
                if let Some((id, access)) = &mut self.permission {
                    let id = *id;
                    if onboarding::ask(access, msg) == onboarding::Outcome::AskScreenshot {
                        return ask_screenshot(Msg::Permission);
                    }
                    if *access == onboarding::Access::Allowed {
                        self.permission = None;
                        return window::close(id);
                    }
                }
            }
            Msg::TrayMenu(msg) => {
                let outcome = self.tray_menu.as_mut().and_then(|(_, m)| m.update(msg));
                return match outcome {
                    Some(tray_menu::Outcome::Run(entry)) => self.leave_tray_menu(Some(entry)),
                    Some(tray_menu::Outcome::Close) => self.leave_tray_menu(None),
                    None => Task::none(),
                };
            }
            // A click outside it (or Alt+Tab): the menu goes, like the OS's.
            Msg::Unfocused(id) if self.tray_menu.as_ref().is_some_and(|(m, _)| *m == id) => {
                return self.leave_tray_menu(None);
            }
            Msg::Unfocused(_) => {}
            // `app::App` takes it before here.
            Msg::Run(_) => {}
            Msg::Chrome(id, event) => return self.on_chrome(id, event),
            Msg::Resized(id, h) => {
                if self.id(Kind::Settings) == Some(id) {
                    let _ = self.handle(Msg::Settings(settings::Message::Height(h)));
                }
                if self.chrome.contains_key(&id) {
                    return window::is_maximized(id).map(move |on| Msg::Maximised(id, on));
                }
                return reshape(id);
            }
            Msg::Maximised(id, on) => {
                if let Some(c) = self.chrome.get_mut(&id) {
                    c.maximised = on;
                }
            }
            Msg::CloseRequest(id) => {
                if self.id(Kind::Settings) == Some(id) {
                    return self.close_window(id);
                }
            }
            Msg::Closed(id) => {
                platform::windows::forget(id);
                self.chrome.remove(&id);
                self.toasts.retain(|(t, ..)| *t != id);
                if self.update_card.as_ref().is_some_and(|(c, _)| *c == id) {
                    self.update_card = None;
                }
                if self.update_pill.as_ref().is_some_and(|(p, ..)| *p == id) {
                    self.update_pill = None;
                }
                if self.tray_menu.as_ref().is_some_and(|(m, _)| *m == id) {
                    self.tray_menu = None;
                }
                self.pins.retain(|p| p.id != id); // the OS closed it: free the pixels
                if self.steps_window() == Some(id) {
                    self.steps = None; // the steps go with it (memory only)
                }
                if self.id(Kind::Permission) == Some(id) {
                    self.permission = None;
                }
                let ours =
                    self.id(Kind::Settings) == Some(id) || self.id(Kind::Onboarding) == Some(id);

                if self.id(Kind::Settings) == Some(id) {
                    self.settings = None;
                }
                if self.id(Kind::Onboarding) == Some(id) {
                    self.onboarding = None;
                }
                if ours {
                    // Closed without saving: undo the live theme preview.
                    theme::apply(config::current().theme);
                    // the window's state, glyphs and pixels are freed: give them back
                    // (a beat later, once the renderer has dropped its buffers)
                    crate::shared::platform::sys::mem::trim_heap_soon();
                    return Task::none();
                }
            }
            Msg::OsTheme(mode) => theme::set_os(mode),
            Msg::Test(step, what) => return self.test_step(step, &what),
            Msg::TestShot(step, name, shot) => save_test_shot(step, name, &shot),
        }
        Task::none()
    }

    /// Closes `id`, except Settings with unsaved changes: it asks first (its footer).
    fn close_window(&mut self, id: window::Id) -> Task<Msg> {
        if let Some((sid, state)) = &mut self.settings
            && *sid == id
            && !state.close_requested()
        {
            // the question's Cancel (Keep editing) takes the focus: Enter keeps editing
            let cancel = iced::widget::operation::focus(crate::shared::kit::focus::CANCEL);
            return Task::batch([window::gain_focus(id), cancel]);
        }
        window::close(id)
    }

    /// The header and edges of a normal window.
    fn on_chrome(&mut self, id: window::Id, event: chrome::Event) -> Task<Msg> {
        let resizable = self.mode(id).resizable;
        let c = self.chrome.entry(id).or_default();
        match event {
            chrome::Event::Caption(Caption::Drag) => {
                let now = Instant::now();
                if resizable && is_double_click(c.last_press, now) {
                    c.last_press = None;
                    window::toggle_maximize(id)
                } else {
                    c.last_press = Some(now);
                    window::drag(id)
                }
            }
            chrome::Event::Caption(Caption::Minimise) => window::minimize(id, true),
            chrome::Event::Caption(Caption::Maximise) if resizable => window::toggle_maximize(id),
            chrome::Event::Caption(Caption::Maximise) => Task::none(),
            chrome::Event::Caption(Caption::Close) => self.close_window(id),
            chrome::Event::Resize(direction) if !c.maximised => window::drag_resize(id, direction),
            chrome::Event::Resize(_) => Task::none(),
        }
    }

    /// What the header offers this window.
    fn mode(&self, id: window::Id) -> chrome::Mode {
        chrome::Mode {
            resizable: self.id(Kind::Settings) == Some(id),
            maximised: self.chrome.get(&id).is_some_and(|c| c.maximised),
        }
    }

    fn on_pin(&mut self, id: window::Id, msg: PinMsg) -> Task<Msg> {
        let Some(at) = self.pins.iter().position(|p| p.id == id) else {
            return Task::none(); // not a pin (the key feed reports every window)
        };
        let pin = &mut self.pins[at];
        match msg {
            PinMsg::Drag => window::drag(id),
            PinMsg::Hover(on) => {
                pin.hover = on;
                Task::none()
            }
            PinMsg::Zoom(delta) => {
                pin.zoom = zoom_after(pin.zoom, delta);
                platform::monitor::resize(id, pin.size())
            }
            PinMsg::DragOut => {
                pin.dragged = None; // one temp file per pin
                if let Some(crop) = pin.crop() {
                    match DragFile::write(&crop) {
                        Ok(mut file) => {
                            if let Err(e) = dragout::start(&mut file, &crop, false) {
                                crate::log!("[opencircle] {e}");
                            }
                            pin.dragged = Some(file);
                        }
                        Err(e) => crate::log!("[opencircle] drag-out: {e}"),
                    }
                }
                Task::none()
            }
            PinMsg::Copy | PinMsg::Save => {
                let action = if matches!(msg, PinMsg::Copy) {
                    &copy::ACTION
                } else {
                    &save::ACTION
                };
                if let Some(crop) = pin.crop() {
                    finish(action, crop);
                }
                Task::none()
            }
            PinMsg::Close => {
                // the image goes when the window has (`Closed`): it fades out first
                close(id)
            }
            PinMsg::Live(live::Event::Frame(crop)) => {
                let Some(live) = &pin.live else {
                    return Task::none();
                };
                let base = Size::new(crop.width as f32, crop.height as f32) * (1.0 / live.scale);
                pin.image = image::Handle::from_rgba(crop.width, crop.height, crop.rgba);
                if base == pin.base {
                    return Task::none();
                }
                // the window was resized or changed DPI: the pin keeps the crop's shape
                pin.base = base;
                platform::monitor::resize(id, pin.size())
            }
            PinMsg::Live(live::Event::Gone) => {
                let gone = self.toast(t!(pill_pin_gone), Tone::Neutral, None, TOAST_MS);
                Task::batch([close(id), gone])
            }
            PinMsg::Live(live::Event::Failed(e)) => {
                crate::log!("[opencircle] live pin: {e}");
                pin.live = None; // a still pin from here
                let text = t!(pill_pin_still);
                self.toast(text, false, None, TOAST_MS)
            }
            PinMsg::Opacity => {
                let Some(live) = &mut pin.live else {
                    return Task::none();
                };
                live.opacity = (live.opacity + 1) % OPACITY.len();
                opacity(id, OPACITY[live.opacity].0)
            }
            PinMsg::ClickThrough => {
                let Some(live) = &mut pin.live else {
                    return Task::none();
                };
                live.through = true;
                pin.hover = false; // it gets no pointer from now on
                window::enable_mouse_passthrough(id)
            }
        }
    }

    pub fn view(&self, window: window::Id) -> Option<Element<'_, Msg>> {
        if let Some((_, strip)) = self.steps.as_ref().filter(|(w, _)| *w == Some(window)) {
            return Some(strip.view(shape::radius(steps::RADIUS)).map(Msg::Steps));
        }
        if let Some(pin) = self.pins.iter().find(|p| p.id == window) {
            let id = pin.id;
            return Some(pin_view(pin).map(move |msg| Msg::Pin(id, msg)));
        }
        if let Some((_, card)) = self.update_card.as_ref().filter(|(c, _)| *c == window) {
            let radius = shape::radius(update_card::RADIUS);
            return Some(spoken(|| update_card::view(card, radius)).map(Msg::UpdateCard));
        }
        if let Some((_, pill, _)) = self.update_pill.as_ref().filter(|(p, ..)| *p == window) {
            let radius = shape::radius(update_pill::RADIUS);
            let still = crate::shared::kit::motion::reduced();
            let view = spoken(|| update_pill::view(pill, radius, self.pill_turn, still));
            return Some(view.map(Msg::UpdatePill));
        }
        if let Some((_, menu)) = self.tray_menu.as_ref().filter(|(m, _)| *m == window) {
            return Some(spoken(|| tray_menu::view(menu)).map(Msg::TrayMenu));
        }
        if let Some((_, text, tone)) = self.toasts.iter().find(|(t, ..)| *t == window) {
            let radius = shape::radius(toast::RADIUS);
            return Some(spoken(|| toast::view(text, *tone, radius)));
        }
        if let Some((id, state)) = &self.settings
            && *id == window
        {
            // Settings draws the design's own header (logo, title, auto-save pill).
            let mode = self.mode(window);
            let page = settings::view(state, mode.maximised).map(Msg::Settings);
            return Some(chrome::with_edges(page, mode, move |e| {
                Msg::Chrome(window, e)
            }));
        }
        if let Some((id, state)) = &self.onboarding
            && *id == window
        {
            // Onboarding draws the design's own header too (fixed size: no edges).
            return Some(onboarding::view(state).map(Msg::Onboarding));
        }
        let page = match &self.permission {
            Some((id, access)) if *id == window => {
                onboarding::prompt_view(access).map(Msg::Permission)
            }
            _ => return None,
        };
        Some(chrome::frame(
            crate::shared::kit::logo::logo(16),
            crate::t!(win_t_perm_head),
            self.mode(window),
            page,
            move |e| Msg::Chrome(window, e),
        ))
    }

    pub fn title(&self, window: window::Id) -> &'static str {
        if self.pins.iter().any(|p| p.id == window) {
            return crate::t!(win_t_pin);
        }
        if self.steps_window() == Some(window) {
            return crate::t!(win_t_steps);
        }
        if self.id(Kind::Permission) == Some(window) {
            return crate::t!(o_needs_shots);
        }
        if self.update_card.as_ref().is_some_and(|(c, _)| *c == window) {
            return crate::t!(win_t_update);
        }
        if self
            .update_pill
            .as_ref()
            .is_some_and(|(p, ..)| *p == window)
        {
            return crate::t!(win_t_update_status);
        }
        if self.tray_menu.as_ref().is_some_and(|(m, _)| *m == window) {
            return crate::t!(win_t_menu);
        }
        match (self.id(Kind::Settings), self.id(Kind::Onboarding)) {
            (Some(id), _) if id == window => crate::t!(win_title),
            (_, Some(id)) if id == window => crate::t!(o_title),
            _ => "OpenCircle",
        }
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let keys = match &self.settings {
            Some((_, state)) => settings::subscription(state).map(Msg::Settings),
            None => Subscription::none(),
        };
        let welcome = match &self.onboarding {
            Some((_, state)) => onboarding::subscription(state).map(Msg::Onboarding),
            None => Subscription::none(),
        };
        Subscription::batch([
            welcome,
            window::close_events().map(Msg::Closed),
            window::close_requests().map(Msg::CloseRequest),
            window::resize_events().map(|(id, size)| Msg::Resized(id, size.height)),
            iced::system::theme_changes().map(Msg::OsTheme),
            keys,
            match self.settings {
                Some(_) => copy_keys(),
                None => Subscription::none(),
            },
            if self.pins.is_empty() {
                Subscription::none()
            } else {
                pin_keys()
            },
            match self.tray_menu {
                Some(_) => tray_menu_events(),
                None => Subscription::none(),
            },
            match self.steps_window() {
                Some(_) => steps_events(),
                None => Subscription::none(),
            },
            // the Updating pill's logo turns (a frame clock only while it shows)
            match &self.update_pill {
                Some((_, p, _)) if p.spins() && !crate::shared::kit::motion::reduced() => {
                    crate::shared::kit::motion::frames().map(Msg::PillFrame)
                }
                _ => Subscription::none(),
            },
        ])
    }

    /// Test hook, `OPENCIRCLE_TEST_OPEN=settings,shot,settings,shot`: one step every
    /// 1.5 s. `settings` = the tray's Settings item, `permission` / `toast` / `toast-error` / `notice` / `update` / `whatsnew` = those windows (`close-card` closes the card), `max` = maximise or restore Settings, `light`/`dark`/`save` and `next`/`finish` = the same clicks in Settings / onboarding, `close` = the X button on Settings, `pin` / `zoom` / `unpin` = a pin of the test frame, the wheel over it, closing it, `problem` = an unsaveable edit, `restart-update` = Restart now on a waiting update, `shot` = save the open window as
    /// `$OPENCIRCLE_TEST_DIR/<kind>-<step>.png`, `quit`.
    fn test_step(&mut self, step: usize, what: &str) -> Task<Msg> {
        match what {
            "settings" => self.open(Kind::Settings),
            "permission" => self.open(Kind::Permission),
            "toast" => self.toast("Copied", true, None, TOAST_MS),
            "toast-error" => self.toast("Couldn't save the picture", false, None, TOAST_MS),
            "notice" => self.toast(
                &tray_notice("Ctrl+Shift+Space"),
                Tone::Neutral,
                None,
                TOAST_MS,
            ),
            "update" | "whatsnew" => {
                const NOTES: &str = "## Fixes\n- [A crash](https://example.com) when the screen sleeps\n- Text selection on a second screen";
                let url = "https://example.com";
                self.update_card(match what {
                    "update" => update_card::Card::preview("9.9.9", NOTES, url),
                    _ => update_card::Card::installed("9.9.9", NOTES, url),
                })
            }
            "close-card" => self
                .update_card
                .as_ref()
                .map_or_else(Task::none, |(id, _)| close(*id)),
            "max" => self
                .id(Kind::Settings)
                .map_or_else(Task::none, window::toggle_maximize),
            "shot" => {
                // The newest pin first, else Settings, else Onboarding.
                let open = (self.pins.last().map(|p| ("pin", p.id)))
                    .or_else(|| Some(("settings", self.id(Kind::Settings)?)))
                    .or_else(|| Some(("onboarding", self.id(Kind::Onboarding)?)));
                let Some((name, id)) = open else {
                    return Task::none();
                };
                window::screenshot(id).map(move |shot| Msg::TestShot(step, name, shot))
            }
            "pin" => match crate::shared::platform::capture::monitor_under_cursor() {
                // The test frame (OPENCIRCLE_TEST_FRAME) or the screen, 400x300 of it.
                Ok(f) => {
                    let (w, h) = (f.width.min(400), f.height.min(300));
                    let rgba = (0..h)
                        .flat_map(|y| {
                            let from = (y * f.width * 4) as usize;
                            f.rgba[from..from + (w * 4) as usize].iter().copied()
                        })
                        .collect();
                    let crop = Crop {
                        width: w,
                        height: h,
                        rgba,
                    };
                    self.pin(crop, Some((200, 150)), 1.0)
                }
                Err(e) => {
                    crate::log!("test pin failed: {e}");
                    Task::none()
                }
            },
            // the wheel over the newest pin: it grows 10 % a notch, through `monitor::resize`
            "zoom" => match self.pins.last().map(|p| p.id) {
                Some(id) => self.on_pin(
                    id,
                    PinMsg::Zoom(mouse::ScrollDelta::Lines { x: 0.0, y: 4.0 }),
                ),
                None => Task::none(),
            },
            "unpin" => {
                let last = self.pins.last().map(|p| p.id);
                last.map_or_else(Task::none, |id| self.on_pin(id, PinMsg::Close))
            }
            "light" | "dark" => {
                let theme = if what == "light" {
                    Theme::Light
                } else {
                    Theme::Dark
                };
                self.update(Msg::Settings(settings::Message::Theme(theme)))
            }
            "save" => self.update(Msg::Settings(settings::Message::Save)),
            // A draft Settings cannot save yet (no model for a custom server): closing asks.
            "problem" => self.update(Msg::Settings(settings::Message::Provider(
                crate::shared::config::Provider::OpenaiCompatible,
            ))),
            "close" => match self.id(Kind::Settings) {
                Some(id) => self.close_window(id),
                None => Task::none(),
            },
            "next" => self.update(Msg::Onboarding(onboarding::Message::Next)),
            "finish" => self.update(Msg::Onboarding(onboarding::Message::Finish)),
            "restart-update" => Task::done(Msg::Run(crate::Trigger::RestartToUpdate)),
            "quit" => iced::exit(),
            _ => Task::none(),
        }
    }
}

/// A saved config becomes the running one: theme, shortcuts (only if they changed;
/// on Wayland a re-bind closes and reopens the portal session) and the shared
/// config every panel reads.
fn apply(cfg: Config) {
    apply_from(&config::current(), cfg);
}

/// `apply` against the config that was running before (a write has already made
/// the new one current, so `current()` can no longer tell what changed).
fn apply_from(old: &Config, cfg: Config) {
    theme::apply(cfg.theme);
    if old.shortcut != cfg.shortcut
        || old.quick_actions != cfg.quick_actions
        || old.labs != cfg.labs
    {
        crate::shared::platform::hotkey::reload(&cfg);
    }
    let language_changed = old.general.app_language != cfg.general.app_language;
    crate::shared::platform::autostart::apply(&cfg);
    // Keep chat history turned off: what was kept goes now, nothing more is written
    if old.chat.keep_history && !cfg.chat.keep_history {
        crate::features::ask_ai::chats::forget_all();
    }
    // Recent captures likewise
    if old.snip.history && !cfg.snip.history {
        std::thread::spawn(crate::shared::history::recent::clear);
    }
    let labs_changed = old.labs != cfg.labs;
    config::set_current(cfg);
    if labs_changed || language_changed {
        crate::shared::platform::tray::refresh(); // reads the config just set
    }
}

// ---- the normal window; the others and `open` are in `platform::windows` (a test forbids any other opener) ----

/// A normal window (Settings, Onboarding, the permission prompt): no OS title bar,
/// our own header (`kit::header`), centred, our logo as its icon.
fn normal(kind: Kind) -> (window::Id, Task<window::Id>) {
    let mut position = Position::Centered;
    let (size, min_size, resizable) = match kind {
        Kind::Settings => {
            let (size, min, at) = settings_placement();
            position = at.map_or(position, Position::Specific);
            (size, Some(min), true)
        }
        Kind::Onboarding => (onboarding::WINDOW_SIZE, None, false),
        Kind::Permission => (onboarding::PROMPT_SIZE, None, false),
    };
    let settings = window::Settings {
        size,
        min_size,
        resizable,
        decorations: false,
        position,
        // Settings asks about unsaved changes first (`Msg::CloseRequest`).
        exit_on_close_request: kind != Kind::Settings,
        ..window::Settings::default()
    };
    #[cfg(windows)]
    let settings = window::Settings {
        // Windows 11 rounds the corners (10 ignores it) and keeps the shadow.
        platform_specific: window::settings::PlatformSpecific {
            corner_preference: window::settings::platform::CornerPreference::Round,
            undecorated_shadow: true,
            ..Default::default()
        },
        ..settings
    };
    platform::windows::open(settings)
}

/// Settings' opening size, smallest size and centred origin (logical px) on the screen
/// under the pointer (primary when unknown), kept inside its usable area. The origin is
/// `None` where screens can't be listed (the OS then centres it).
fn settings_placement() -> (Size, Size, Option<Point>) {
    let monitors = crate::shared::platform::capture::monitor_list();
    let chosen = crate::shared::platform::capture::choose(
        &monitors,
        None,
        crate::shared::platform::capture::pointer(),
    );
    let Some(m) = chosen.map(|i| monitors[i]) else {
        let (size, min) = settings::window_limits(settings::WINDOW_SIZE * 1.2);
        return (size, min, None);
    };
    let (x, y, w, h) = m.rect;
    let s = m.scale.max(0.1);
    let origin = Point::new(x as f32 / s, y as f32 / s);
    let full = Size::new(w as f32 / s, h as f32 / s);
    let work = Size::new(
        full.width,
        full.height - crate::shared::platform::monitor::taskbar_inset(origin, full, s),
    );
    let (size, min) = settings::window_limits(work);
    let at = Point::new(
        origin.x + (work.width - size.width) / 2.0,
        origin.y + (work.height - size.height) / 2.0,
    );
    (size, min, Some(at))
}

/// How long a confirmation toast shows (the design's 1800 ms).
pub const TOAST_MS: u64 = 1800;

/// How long the first-run tray notice shows (longer than a confirmation: it is read).
pub const NOTICE_MS: u64 = 6000;

/// The notice after onboarding (the window closes, the app lives on in the tray); it
/// names the shortcut that is really configured.
pub fn tray_notice(shortcut: &str) -> String {
    let keys = crate::features::settings::key_labels(shortcut).join("+");
    crate::shared::app_language::fmt(S::pill_tray, &[&keys])
}

/// The start notice for shortcuts another app holds (`taken`); none when
/// `told` (the last notice's) names them all.
pub fn taken_notice(taken: &[String], told: &[String]) -> Option<String> {
    if taken.iter().all(|k| told.contains(k)) {
        return None;
    }
    let keys: Vec<String> = (taken.iter())
        .map(|k| crate::features::settings::key_labels(k).join("+"))
        .collect();
    let key = match keys.len() {
        1 => S::pill_taken_one,
        _ => S::pill_taken_many,
    };
    Some(crate::shared::app_language::fmt(key, &[&keys.join(", ")]))
}

/// The update pill's window on `monitor` (a taskbar `inset` logical px tall at
/// its bottom) for a pill of `size` logical px: its bottom-right corner, 16 px
/// from the edge and 12 px above the taskbar (the design's board "Update").
fn pill_place(
    monitor: platform::monitor::Place,
    inset: f32,
    size: Size,
) -> platform::monitor::Place {
    const RIGHT: f32 = 16.0;
    const ABOVE: f32 = 12.0;
    let s = monitor.scale;
    let px = (
        (size.width * s).round() as i32,
        (size.height * s).round() as i32,
    );
    let (ox, oy) = monitor.origin;
    let (w, h) = (monitor.px.0 as i32, monitor.px.1 as i32);
    platform::monitor::Place {
        origin: (
            ox + w - (RIGHT * s).round() as i32 - px.0,
            oy + h - ((inset + ABOVE) * s).round() as i32 - px.1,
        ),
        px: (px.0 as u32, px.1 as u32),
        scale: s,
    }
}

/// A toast's window size (logical px): the pill's (`screens::toast::size`).
pub fn toast_size(text: &str) -> Size {
    spoken(|| toast::size(text))
}

/// `build` with the layout direction of the app's language (the floating
/// widgets of part 2a: sizes are measured, and views built, in it).
fn spoken<T>(build: impl FnOnce() -> T) -> T {
    let _dir = crate::shared::kit::dir::scope(crate::shared::app_language::rtl());
    build()
}

/// One screenshot request on its own thread (Wayland), sent while the window asking
/// has the focus; the answer comes back as `Allowed` to `to`.
fn ask_screenshot(to: fn(onboarding::Message) -> Msg) -> Task<Msg> {
    crate::app::background::background(|tx| {
        #[cfg(target_os = "linux")]
        let answer = crate::shared::platform::capture::portal::allow();
        #[cfg(not(target_os = "linux"))]
        let answer = Ok(());
        let _ = tx.unbounded_send(answer);
    })
    .map(move |answer| to(onboarding::Message::Allowed(answer)))
}

fn test_hook() -> Task<Msg> {
    let Some(script) = crate::shared::platform::sys::test_hook("OPENCIRCLE_TEST_OPEN")
        .and_then(|s| s.into_string().ok())
    else {
        return Task::none();
    };
    crate::app::background::background(move |tx| {
        for (i, what) in script.split(',').enumerate() {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let _ = tx.unbounded_send((i, what.to_string()));
        }
    })
    .map(|(i, what)| Msg::Test(i, what))
}

/// The folder picker (native dialog) on its own thread; answers `Msg::Folder`.
fn pick_folder() -> Task<Msg> {
    let start = crate::app::background::background(|tx| {
        let picked = rfd::FileDialog::new()
            .set_title(crate::t!(dlg_folder))
            .pick_folder();
        let _ = tx.unbounded_send(picked);
    });
    start.map(Msg::Folder)
}

/// Opens `url` in the browser if it is one plain web link (`search_in_browser::open_link`).
pub fn open_url(url: &str) {
    if let Err(e) = crate::shared::platform::sys::open::open_link(url) {
        crate::log!("cannot open {url}: {e}");
    }
}

fn save_test_shot(step: usize, name: &str, shot: &window::Screenshot) {
    let Some(dir) = crate::shared::platform::sys::test_hook("OPENCIRCLE_TEST_DIR") else {
        return;
    };
    let name = format!("{name}-{step}.png");
    let path = std::path::Path::new(&dir).join(name);
    let result =
        crate::shared::picture::write_png(&path, shot.size.width, shot.size.height, &shot.rgba);
    if let Err(e) = result {
        crate::log!("test shot failed: {e}");
    }
}

#[cfg(test)]
mod tests;
