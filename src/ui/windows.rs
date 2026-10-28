//! The normal windows beside the overlay: Settings (from the tray) and the
//! first-run Onboarding. At most one of each; asking again focuses the open one.
//! Also the pinned images (any number): small borderless always-on-top windows.
//! Closing a window drops its state. Also owns the OS-theme feed.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::widget::{container, image, mouse_area, row, stack};
use iced::window::{self, Position};
use iced::{ContentFit, Element, Fill, Point, Size, Subscription, Task, keyboard, mouse};

use super::kit::header::Caption;
use super::platform;
use super::screens::onboarding;
use super::screens::toast::{self, Tone};
use super::screens::tray_menu;
use super::widgets::chrome;
use super::{settings, shape, theme, update_card, widgets};
// The helper is `platform::windows`; these names stay reachable as `windows::..` for the callers.
#[cfg(test)]
pub use super::platform::windows::CLEARED;
pub use super::platform::windows::{
    POPUP, TOAST, appear, clear_now, clear_theme, close, cut, fade, floating, hide_from_capture,
    is_floating, opacity, reshape, screen, setup_window, uncut,
};
use crate::actions::{Action, Crop, Run, copy, save};
use crate::config::{self, Config, Theme};
use crate::editor::dragout::{self, DragFile};

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
    /// Any window closed (ours are forgotten; the overlay is `ui::App`'s).
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
    /// A window was resized (any of ours; the overlay's is `ui::App`'s).
    Resized(window::Id),
    /// Whether a normal window is maximised now (asked after every resize).
    Maximised(window::Id, bool),
    /// Test hook step (see `test_hook`).
    Test(usize, String),
    TestShot(usize, &'static str, window::Screenshot),
    /// From the update card.
    UpdateCard(update_card::Msg),
    /// The result of Settings > Check now.
    UpdateStatus(String),
    /// From the tray menu's window.
    TrayMenu(tray_menu::Msg),
    /// A window lost the keyboard focus (the tray menu closes then).
    Unfocused(window::Id),
    /// A tray menu row was chosen and the menu is gone: `ui::App` runs it as if it came from the tray.
    Run(crate::Trigger),
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
    /// The tray menu (Windows), at most one.
    tray_menu: Option<(window::Id, tray_menu::Menu)>,
    /// What each normal window's header knows (see `Chrome`).
    chrome: HashMap<window::Id, Chrome>,
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
}

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
}

/// A pin starts no bigger than this (logical px); a tall scrolling capture shrinks.
const PIN_MAX: Size = Size::new(1400.0, 900.0);
const PIN_ZOOM: std::ops::RangeInclusive<f32> = 0.1..=8.0;
/// Scroll pixels that count as one wheel notch.
const PIXELS_PER_NOTCH: f32 = 40.0;
/// The hover bar needs this much window (logical px) to fit.
const PIN_BAR_ROOM: Size = Size::new(240.0, 80.0);

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
    if pin.hover && size.width >= PIN_BAR_ROOM.width && size.height >= PIN_BAR_ROOM.height {
        let button = |glyph, message, label, keys| {
            widgets::hint(
                widgets::icon_button(glyph, false, message),
                label,
                widgets::Why::Keys(keys),
                Side::Bottom,
            )
        };
        let drag = dragout::SUPPORTED.then(|| {
            widgets::hint(
                widgets::drag_handle(widgets::DRAG, PinMsg::DragOut),
                "Drag the image into another app",
                widgets::Why::Icon,
                Side::Bottom,
            )
        });
        let bar = widgets::surface(
            row![
                drag,
                button(COPY, PinMsg::Copy, "Copy", "Ctrl+C"),
                button(SAVE, PinMsg::Save, "Save", "Ctrl+S"),
                button(widgets::CLOSE, PinMsg::Close, "Close", "Esc"),
            ]
            .spacing(4),
        );
        layers = layers.push(
            container(bar)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Top)
                .padding(8),
        );
    }
    mouse_area(widgets::pinned(layers))
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
        }
        Task::batch(tasks)
    }

    /// The open window of this kind, if any.
    fn id(&self, kind: Kind) -> Option<window::Id> {
        match kind {
            Kind::Settings => self.settings.as_ref().map(|(id, _)| *id),
            Kind::Onboarding => self.onboarding.as_ref().map(|(id, _)| *id),
            Kind::Permission => self.permission.as_ref().map(|(id, _)| *id),
        }
    }

    /// Pins `crop` (physical px, `scale` per logical px) in its own always-on-top
    /// window, at `at` (logical desktop px) or centred. Any number can be open.
    pub fn pin(&mut self, crop: Crop, at: Option<Point>, scale: f32) -> Task<Msg> {
        let base = Size::new(crop.width as f32, crop.height as f32) * (1.0 / scale.max(0.1));
        let zoom = fit_zoom(base);
        let (id, open) = floating(
            window::Settings {
                size: base * zoom,
                position: at.map_or(Position::Centered, Position::Specific),
                resizable: false,
                ..window::Settings::default()
            },
            0.0, // a picture: square
            POPUP,
        );
        crate::log!("[opencircle] pinned {}x{}", crop.width, crop.height);
        self.pins.push(Pin {
            id,
            image: image::Handle::from_rgba(crop.width, crop.height, crop.rgba),
            base,
            zoom,
            hover: false,
            dragged: None,
        });
        open.discard()
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn toasts(&self) -> Vec<(String, bool)> {
        (self.toasts.iter())
            .map(|(_, text, tone)| (text.clone(), *tone == Tone::Ok))
            .collect()
    }

    /// The update card (release notes) with `card`; replaces an open one's content.
    pub fn update_card(&mut self, card: update_card::Card) -> Task<Msg> {
        let size = card.size();
        if let Some((id, open)) = &mut self.update_card {
            *open = card;
            return window::resize(*id, size);
        }
        let (id, open) = floating(
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
        open.discard()
    }

    /// The tray menu at the tray icon (`icon`: physical px), with the keyboard; `live_on` = live
    /// translate runs (its row's ✓). An open one goes first (a second right-click).
    pub fn tray_menu(&mut self, icon: [i32; 4], live_on: bool) -> Task<Msg> {
        let cfg = config::current();
        let menu = tray_menu::Menu::new(&cfg.labs, &cfg, live_on);
        let size = menu.size();
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

    /// Something the user is in the middle of lives in a window: Settings,
    /// onboarding, the permission prompt or a pinned image (an update waits).
    pub fn busy(&self) -> bool {
        self.settings.is_some()
            || self.onboarding.is_some()
            || self.permission.is_some()
            || !self.pins.is_empty()
    }

    /// Opens the window, or focuses it when it is already open.
    pub fn open(&mut self, kind: Kind) -> Task<Msg> {
        if let Some(id) = self.id(kind) {
            crate::log!("[opencircle] {kind:?} window is open: focusing it");
            return window::minimize(id, false).chain(window::gain_focus(id));
        }
        crate::log!("[opencircle] opening the {kind:?} window");
        let (id, open) = normal(kind);
        match kind {
            Kind::Settings => self.settings = Some((id, settings::State::open())),
            Kind::Onboarding => self.onboarding = Some((id, onboarding::State::open())),
            Kind::Permission => self.permission = Some((id, onboarding::Access::Unasked)),
        }
        self.chrome.insert(id, Chrome::default());
        open.discard().chain(window::gain_focus(id))
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.handle(msg);
        // A shortcut field that records gets the keys: the global shortcuts are
        // held off until it stops (a key set, Esc, another page, the window closed).
        let recording = self.settings.as_ref().is_some_and(|(_, s)| s.recording());
        crate::hotkey::suspend(recording);
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
                        super::background(move |tx| {
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
                                super::background(move |tx| {
                                    let _ = tx.unbounded_send(w.run());
                                })
                                .map(|d| Msg::Settings(settings::Message::Written(d))),
                            ]);
                        }
                        settings::Outcome::Browse => return Task::batch([keys, pick_folder()]),
                        settings::Outcome::OpenLink(url) => open_url(&url),
                        settings::Outcome::CopyDiagnostics => {
                            let report = crate::diagnostics::report(&crate::config::current());
                            let toast = match crate::actions::copy::text(report) {
                                Ok(()) => self.toast("Diagnostics copied", true, None, TOAST_MS),
                                Err(e) => {
                                    crate::log!("[opencircle] copy diagnostics failed: {e}");
                                    self.toast("Couldn't copy", false, None, TOAST_MS)
                                }
                            };
                            return Task::batch([keys, toast]);
                        }
                        settings::Outcome::CheckUpdate => crate::update::check_now(),
                        settings::Outcome::None => {}
                    }
                    if leaving {
                        return Task::batch([keys, window::close(id)]);
                    }
                    return keys;
                }
            }
            Msg::Folder(dir) => {
                if let Some((_, state)) = &mut self.settings {
                    settings::update(state, settings::Message::Folder(dir));
                }
            }
            Msg::Pin(id, msg) => return self.on_pin(id, msg),
            Msg::UpdateStatus(status) => {
                if let Some((_, state)) = &mut self.settings {
                    settings::update(state, settings::Message::UpdateStatus(status));
                }
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
                }
            }
            // The design's maximise does nothing for this fixed-size window.
            Msg::Onboarding(onboarding::Message::Caption(Caption::Maximise)) => {}
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
                        super::background(move |tx| {
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
            // `ui::App` takes it before here.
            Msg::Run(_) => {}
            Msg::Chrome(id, event) => return self.on_chrome(id, event),
            Msg::Resized(id) => {
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
                if self.tray_menu.as_ref().is_some_and(|(m, _)| *m == id) {
                    self.tray_menu = None;
                }
                self.pins.retain(|p| p.id != id); // the OS closed it: free the pixels
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
                    return Task::future(async {
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            crate::sys::mem::trim_heap();
                        });
                    })
                    .discard();
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
            return window::gain_focus(id);
        }
        window::close(id)
    }

    /// The header and edges of a normal window.
    fn on_chrome(&mut self, id: window::Id, event: chrome::Event) -> Task<Msg> {
        let resizable = self.mode(id).resizable;
        let c = self.chrome.entry(id).or_default();
        match event {
            chrome::Event::Press => {
                let now = Instant::now();
                if resizable && is_double_click(c.last_press, now) {
                    c.last_press = None;
                    window::toggle_maximize(id)
                } else {
                    c.last_press = Some(now);
                    window::drag(id)
                }
            }
            chrome::Event::Minimise => window::minimize(id, true),
            chrome::Event::Maximise => window::toggle_maximize(id),
            chrome::Event::Close => self.close_window(id),
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
                window::resize(id, pin.size())
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
        }
    }

    pub fn view(&self, window: window::Id) -> Option<Element<'_, Msg>> {
        if let Some(pin) = self.pins.iter().find(|p| p.id == window) {
            let id = pin.id;
            return Some(pin_view(pin).map(move |msg| Msg::Pin(id, msg)));
        }
        if let Some((_, card)) = self.update_card.as_ref().filter(|(c, _)| *c == window) {
            let radius = shape::radius(update_card::RADIUS);
            return Some(update_card::view(card, radius).map(Msg::UpdateCard));
        }
        if let Some((_, menu)) = self.tray_menu.as_ref().filter(|(m, _)| *m == window) {
            return Some(tray_menu::view(menu).map(Msg::TrayMenu));
        }
        if let Some((_, text, tone)) = self.toasts.iter().find(|(t, ..)| *t == window) {
            return Some(toast::view(text, *tone, shape::radius(toast::RADIUS)));
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
            "Screenshot permission",
            self.mode(window),
            page,
            move |e| Msg::Chrome(window, e),
        ))
    }

    pub fn title(&self, window: window::Id) -> &'static str {
        if self.pins.iter().any(|p| p.id == window) {
            return "OpenCircle pin";
        }
        if self.id(Kind::Permission) == Some(window) {
            return "OpenCircle needs screenshot permission";
        }
        if self.update_card.as_ref().is_some_and(|(c, _)| *c == window) {
            return "OpenCircle update";
        }
        if self.tray_menu.as_ref().is_some_and(|(m, _)| *m == window) {
            return "OpenCircle menu";
        }
        match (self.id(Kind::Settings), self.id(Kind::Onboarding)) {
            (Some(id), _) if id == window => "OpenCircle Settings",
            (_, Some(id)) if id == window => "Welcome to OpenCircle",
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
            window::resize_events().map(|(id, _)| Msg::Resized(id)),
            iced::system::theme_changes().map(Msg::OsTheme),
            keys,
            if self.pins.is_empty() {
                Subscription::none()
            } else {
                pin_keys()
            },
            match self.tray_menu {
                Some(_) => tray_menu_events(),
                None => Subscription::none(),
            },
        ])
    }

    /// Test hook, `OPENCIRCLE_TEST_OPEN=settings,shot,settings,shot`: one step every
    /// 1.5 s. `settings` = the tray's Settings item, `permission` / `toast` / `toast-error` / `notice` / `update` / `whatsnew` = those windows (`close-card` closes the card), `max` = maximise or restore Settings, `light`/`dark`/`save` and `next`/`finish` = the same clicks in Settings / onboarding, `close` = the X button on Settings, `problem` = an unsaveable edit, `shot` = save the open window as
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
                const NOTES: &str = "## Fixed\n- [A crash](https://example.com) when the screen sleeps\n- Text selection on a second screen";
                let url = "https://example.com";
                self.update_card(match what {
                    "update" => update_card::Card::available("9.9.9", NOTES, url),
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
            "pin" => match crate::capture::monitor_under_cursor() {
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
                    self.pin(crop, Some(Point::new(200.0, 150.0)), 1.0)
                }
                Err(e) => {
                    crate::log!("test pin failed: {e}");
                    Task::none()
                }
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
                crate::config::Provider::OpenaiCompatible,
            ))),
            "close" => match self.id(Kind::Settings) {
                Some(id) => self.close_window(id),
                None => Task::none(),
            },
            "next" => self.update(Msg::Onboarding(onboarding::Message::Next)),
            "finish" => self.update(Msg::Onboarding(onboarding::Message::Finish)),
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
        crate::hotkey::reload(&cfg);
    }
    crate::autostart::apply(&cfg);
    let labs_changed = old.labs != cfg.labs;
    config::set_current(cfg);
    if labs_changed {
        super::refresh_tray(); // reads the config just set
    }
}

// ---- the normal window; the others and `open` are in `platform::windows` (a test forbids any other opener) ----

/// A normal window (Settings, Onboarding, the permission prompt): no OS title bar,
/// our own header (`widgets::chrome`), centred, our logo as its icon.
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
    let monitors = crate::capture::monitor_list();
    let chosen = crate::capture::choose(&monitors, None, crate::capture::pointer());
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
        full.height - super::taskbar_inset(origin, full, s),
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
    let keys = super::settings::key_labels(shortcut).join("+");
    format!("OpenCircle is running in the tray. Press {keys}.")
}

/// A toast's window size (logical px): the pill's (`screens::toast::size`).
pub fn toast_size(text: &str) -> Size {
    toast::size(text)
}

/// One screenshot request on its own thread (Wayland), sent while the window asking
/// has the focus; the answer comes back as `Allowed` to `to`.
fn ask_screenshot(to: fn(onboarding::Message) -> Msg) -> Task<Msg> {
    super::background(|tx| {
        #[cfg(target_os = "linux")]
        let answer = crate::capture::portal::allow();
        #[cfg(not(target_os = "linux"))]
        let answer = Ok(());
        let _ = tx.unbounded_send(answer);
    })
    .map(move |answer| to(onboarding::Message::Allowed(answer)))
}

fn test_hook() -> Task<Msg> {
    let Ok(script) = std::env::var("OPENCIRCLE_TEST_OPEN") else {
        return Task::none();
    };
    super::background(move |tx| {
        for (i, what) in script.split(',').enumerate() {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let _ = tx.unbounded_send((i, what.to_string()));
        }
    })
    .map(|(i, what)| Msg::Test(i, what))
}

/// The folder picker (native dialog) on its own thread; answers `Msg::Folder`.
fn pick_folder() -> Task<Msg> {
    let start = super::background(|tx| {
        let picked = rfd::FileDialog::new()
            .set_title("Choose the save folder")
            .pick_folder();
        let _ = tx.unbounded_send(picked);
    });
    start.map(Msg::Folder)
}

/// Opens `url` in the browser if it is one plain web link (`lens::open_link`).
fn open_url(url: &str) {
    if let Err(e) = crate::sys::open::open_link(url) {
        crate::log!("cannot open {url}: {e}");
    }
}

fn save_test_shot(step: usize, name: &str, shot: &window::Screenshot) {
    let Some(dir) = std::env::var_os("OPENCIRCLE_TEST_DIR") else {
        return;
    };
    let name = format!("{name}-{step}.png");
    let path = std::path::Path::new(&dir).join(name);
    let result = crate::actions::write_png(&path, shot.size.width, shot.size.height, &shot.rgba);
    if let Err(e) = result {
        crate::log!("test shot failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{AiKey, Secret, SecretsError};
    use crate::ui::kit::motion;
    use crate::ui::widgets::shot;

    struct NoKeys;
    impl crate::secrets::KeyStore for NoKeys {
        fn is_set(&self, _: AiKey) -> Result<bool, SecretsError> {
            Ok(false)
        }
        fn read(&self, _: AiKey) -> Result<Option<Secret>, SecretsError> {
            Ok(None)
        }
        fn set(&self, _: AiKey, _: &Secret) -> Result<(), SecretsError> {
            Ok(())
        }
        fn delete(&self, _: AiKey) -> Result<(), SecretsError> {
            Ok(())
        }
    }

    fn settings_state() -> settings::State {
        settings::State::new(Config::default(), vec![], None, Box::new(NoKeys))
    }

    /// While a shortcut field records, the app's own shortcuts are held off
    /// (Ctrl+Shift+X opened the overlay instead of being recorded); back on
    /// when it stops: Esc, a key set, or the window closed mid-recording.
    #[test]
    fn global_shortcuts_are_held_off_while_a_shortcut_field_records() {
        use settings::{Chord, Slot};
        let key = |key: &str, ctrl: bool| {
            let chord = Chord {
                ctrl,
                alt: false,
                shift: ctrl,
                logo: false,
                key: key.into(),
            };
            Msg::Settings(settings::Message::Key(chord))
        };
        let record = Msg::Settings(settings::Message::Record(Slot::Capture));
        let mut windows = Windows::default();
        let id = window::Id::unique();
        windows.settings = Some((id, settings_state()));
        assert!(!crate::hotkey::suspended());
        let _ = windows.update(record.clone());
        assert!(crate::hotkey::suspended(), "recording: held off");
        let _ = windows.update(key("Escape", false));
        assert!(!crate::hotkey::suspended(), "Esc: back on");
        let _ = windows.update(record.clone());
        let _ = windows.update(key("KeyK", true));
        assert!(!crate::hotkey::suspended(), "a key set: back on");
        let _ = windows.update(record);
        assert!(crate::hotkey::suspended());
        let _ = windows.update(Msg::Closed(id));
        assert!(!crate::hotkey::suspended(), "the window closed: back on");
    }

    #[test]
    fn a_saved_config_takes_effect_even_when_the_settings_window_is_gone() {
        let dir = std::env::temp_dir().join(format!("oc-win-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut state = settings::State::new(
            Config::default(),
            vec![],
            Some(dir.join("config.toml")),
            Box::new(NoKeys),
        );
        settings::update(
            &mut state,
            settings::Message::Theme(crate::config::Theme::Dark),
        );
        let settings::Outcome::Write(w) = settings::update(&mut state, settings::Message::Save)
        else {
            panic!("no write")
        };
        let done = w.run();
        drop(state); // the window closed while the write ran
        let mut windows = Windows::default();
        let _ = windows.update(Msg::Settings(settings::Message::Written(done)));
        assert_eq!(config::current().theme, crate::config::Theme::Dark);
        config::set_current(Config::default());
        theme::apply(crate::config::Theme::System);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Closing Settings mid-preview drops its state: the clock (its
    /// subscription) stops and the preview's memory is freed.
    #[test]
    fn closing_settings_stops_a_playing_preview() {
        let mut windows = Windows::default();
        let id = window::Id::unique();
        windows.settings = Some((id, settings_state()));
        let section = settings::Message::Section(settings::Section::Translate);
        let _ = windows.update(Msg::Settings(section));
        assert!(
            windows
                .settings
                .as_ref()
                .is_some_and(|(_, s)| s.animating())
        );
        let _ = windows.update(Msg::Closed(id));
        assert!(windows.settings.is_none());
    }

    #[test]
    fn one_settings_window_at_a_time() {
        let mut windows = Windows::default();
        assert_eq!(windows.id(Kind::Settings), None);
        let id = window::Id::unique();
        windows.settings = Some((id, settings_state()));
        // A second request focuses the open window: same id, state kept.
        let _ = windows.open(Kind::Settings);
        assert_eq!(windows.id(Kind::Settings), Some(id));
        assert_eq!(windows.title(id), "OpenCircle Settings");
        assert!(windows.view(id).is_some());
        assert!(windows.view(window::Id::unique()).is_none());
        assert_eq!(windows.id(Kind::Onboarding), None);
    }

    #[test]
    fn the_permission_prompt_closes_once_allowed() {
        let mut windows = Windows::default();
        let _ = windows.open(Kind::Permission);
        let id = windows.id(Kind::Permission).unwrap();
        assert_eq!(windows.title(id), "OpenCircle needs screenshot permission");
        assert!(windows.view(id).is_some());
        let _ = windows.update(Msg::Permission(onboarding::Message::Allow));
        let refused = Err(crate::capture::CaptureError::Refused(
            "refused by the desktop".into(),
        ));
        let _ = windows.update(Msg::Permission(onboarding::Message::Allowed(refused)));
        assert_eq!(windows.id(Kind::Permission), Some(id), "stays to try again");
        let _ = windows.update(Msg::Permission(onboarding::Message::Allow));
        let _ = windows.update(Msg::Permission(onboarding::Message::Allowed(Ok(()))));
        assert_eq!(windows.id(Kind::Permission), None);
    }

    #[test]
    fn closing_frees_the_state() {
        let mut windows = Windows::default();
        let (id, other) = (window::Id::unique(), window::Id::unique());
        windows.settings = Some((id, settings_state()));
        let _ = windows.update(Msg::Closed(other)); // the overlay or a stranger
        assert_eq!(windows.id(Kind::Settings), Some(id));
        let _ = windows.update(Msg::Closed(id));
        assert_eq!(windows.id(Kind::Settings), None);
        assert!(windows.view(id).is_none());
    }

    /// A `w`x`h` picture with a diagonal gradient and a frame, so a render shows it.
    fn picture(w: u32, h: u32) -> Crop {
        let rgba = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let edge = x < 6 || y < 6 || x >= w - 6 || y >= h - 6;
                if edge {
                    [255, 200, 40, 255]
                } else {
                    [(x * 255 / w) as u8, (y * 255 / h) as u8, 160, 255]
                }
            })
            .collect();
        Crop {
            width: w,
            height: h,
            rgba,
        }
    }

    fn pin_ids(windows: &Windows) -> Vec<window::Id> {
        windows.pins.iter().map(|p| p.id).collect()
    }

    #[test]
    fn several_pins_coexist_and_each_frees_its_image_on_close() {
        let mut windows = Windows::default();
        for n in 0..3 {
            let at = Point::new(100.0 * n as f32, 50.0);
            let _ = windows.pin(picture(200 + n * 10, 100), Some(at), 1.0);
        }
        let ids = pin_ids(&windows);
        assert_eq!(ids.len(), 3);
        assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
        for id in &ids {
            assert!(windows.view(*id).is_some());
            assert_eq!(windows.title(*id), "OpenCircle pin");
        }
        assert_eq!(windows.pins[1].size(), Size::new(210.0, 100.0));
        // The pixels are held by the pin only.
        assert!(
            windows.pins[0]
                .crop()
                .is_some_and(|c| c.rgba.len() == 200 * 100 * 4)
        );

        // Esc / double-click / the close button: that pin goes, the others
        // stay. It fades out first: shown until its window has closed.
        let _ = windows.update(Msg::Pin(ids[1], PinMsg::Close));
        assert!(windows.view(ids[1]).is_some(), "fading out");
        let _ = windows.update(Msg::Closed(ids[1]));
        assert_eq!(pin_ids(&windows), [ids[0], ids[2]]);
        assert!(
            windows.view(ids[1]).is_none(),
            "its view and pixels are gone"
        );
        // The OS closing a window (Alt+F4) frees it too.
        let _ = windows.update(Msg::Closed(ids[0]));
        assert_eq!(pin_ids(&windows), [ids[2]]);
        // A key aimed at another window (or a closed pin) does nothing.
        let _ = windows.update(Msg::Pin(window::Id::unique(), PinMsg::Close));
        let _ = windows.update(Msg::Pin(ids[1], PinMsg::Close));
        assert_eq!(pin_ids(&windows), [ids[2]]);
        let _ = windows.update(Msg::Pin(ids[2], PinMsg::Close));
        let _ = windows.update(Msg::Closed(ids[2]));
        assert!(windows.pins.is_empty());
    }

    #[test]
    fn a_pin_keeps_its_on_screen_size_and_big_ones_shrink_to_fit() {
        let mut windows = Windows::default();
        // 150 % scaling: 600x300 physical = 400x200 logical.
        let _ = windows.pin(picture(600, 300), None, 1.5);
        assert_eq!(windows.pins[0].size(), Size::new(400.0, 200.0));
        // A tall scrolling capture starts reduced to fit.
        let _ = windows.pin(picture(780, 6000), None, 1.0);
        let tall = windows.pins[1].size();
        assert!(
            tall.height <= PIN_MAX.height + 0.01 && tall.width < 780.0,
            "{tall:?}"
        );
        assert!(
            (tall.width / tall.height - 780.0 / 6000.0).abs() < 1e-4,
            "same shape"
        );
    }

    #[test]
    fn scrolling_zooms_within_limits() {
        let lines = |y| mouse::ScrollDelta::Lines { x: 0.0, y };
        assert!(zoom_after(1.0, lines(1.0)) > 1.0);
        assert!(zoom_after(1.0, lines(-1.0)) < 1.0);
        assert!((zoom_after(zoom_after(1.0, lines(1.0)), lines(-1.0)) - 1.0).abs() < 1e-5);
        let pixels = mouse::ScrollDelta::Pixels {
            x: 0.0,
            y: PIXELS_PER_NOTCH,
        };
        assert!((zoom_after(1.0, pixels) - zoom_after(1.0, lines(1.0))).abs() < 1e-6);
        assert_eq!(zoom_after(7.9, lines(50.0)), 8.0, "upper limit");
        assert_eq!(zoom_after(0.11, lines(-50.0)), 0.1, "lower limit");

        let mut windows = Windows::default();
        let _ = windows.pin(picture(200, 100), None, 1.0);
        let id = windows.pins[0].id;
        let _ = windows.update(Msg::Pin(id, PinMsg::Zoom(lines(3.0))));
        let size = windows.pins[0].size();
        assert!(
            (size.width / 200.0 - 1.1_f32.powi(3)).abs() < 1e-4,
            "{size:?}"
        );
        assert!((size.width / size.height - 2.0).abs() < 1e-4, "shape kept");
    }

    #[test]
    fn hover_shows_the_bar_only_when_it_fits() {
        let mut windows = Windows::default();
        let _ = windows.pin(picture(300, 200), None, 1.0);
        let _ = windows.pin(picture(60, 40), None, 1.0);
        let (big, small) = (windows.pins[0].id, windows.pins[1].id);
        let draw = |windows: &Windows, id, w, h| shot::render(windows.view(id).unwrap(), w, h);
        let (calm_big, calm_small) = (draw(&windows, big, 300, 200), draw(&windows, small, 60, 40));
        for id in [big, small] {
            let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
        }
        assert!(draw(&windows, big, 300, 200) != calm_big, "the bar shows");
        assert!(
            draw(&windows, small, 60, 40) == calm_small,
            "no room for it"
        );
    }

    #[test]
    fn renders_a_pinned_image() {
        let mut windows = Windows::default();
        let _ = windows.pin(picture(420, 260), None, 1.0);
        let id = windows.pins[0].id;
        for (name, hover) in [("pin", false), ("pin-hover", true)] {
            let _ = windows.update(Msg::Pin(id, PinMsg::Hover(hover)));
            let px = shot::save(name, windows.view(id).unwrap(), Size::new(420.0, 260.0));
            // The picture fills the window: no window-colour background shows.
            let bg = theme::current().window;
            let bg = [bg.r, bg.g, bg.b].map(|c| (c * 255.0).round() as u8);
            let background = px.chunks(4).filter(|p| p[..3] == bg).count();
            assert!(background < 100, "{name}: {background} background pixels");
        }
        // Hover adds the bar: the two renders differ.
        let calm = shot::render(windows.view(id).unwrap(), 420, 260);
        let _ = windows.update(Msg::Pin(id, PinMsg::Hover(false)));
        let still = shot::render(windows.view(id).unwrap(), 420, 260);
        assert!(calm != still);
    }

    #[test]
    fn the_pin_bar_adds_the_shortcuts_to_its_icons() {
        use crate::ui::widgets::tests_support::{assert_tips_add_something, shown};
        let mut windows = Windows::default();
        let _ = windows.pin(picture(300, 200), None, 1.0);
        let id = windows.pins[0].id;
        let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
        let _ = windows.view(id).unwrap();
        // Drag-out is Windows-only, so its handle (and hint) is too.
        let mut expected = vec!["Copy · Ctrl+C", "Save · Ctrl+S", "Close · Esc"];
        if cfg!(windows) {
            expected.insert(0, "Drag the image into another app");
        }
        assert_eq!(shown(), expected);
        assert_tips_add_something("pin bar");
    }

    #[test]
    fn normal_windows_draw_their_own_header() {
        let mut windows = Windows::default();
        let (id, other) = (window::Id::unique(), window::Id::unique());
        windows.settings = Some((id, settings_state()));
        let texts = shot::texts(windows.view(id).unwrap());
        assert!(
            texts.iter().any(|t| t == "OpenCircle Settings"),
            "{texts:?}"
        );
        assert!(windows.mode(id).resizable && !windows.mode(id).maximised);
        // Maximised once the OS says so; edges are gone, restore shows.
        windows.chrome.insert(id, Chrome::default());
        let _ = windows.update(Msg::Maximised(id, true));
        assert!(windows.mode(id).maximised);
        // Onboarding has the design's fixed size, its own header; nor can the
        // permission prompt be maximised.
        windows.onboarding = Some((other, onboarding::State::open()));
        assert!(!windows.mode(other).resizable);
        let texts = shot::texts(windows.view(other).unwrap());
        assert!(
            texts.iter().any(|t| t == "Welcome to OpenCircle"),
            "{texts:?}"
        );
        let prompt = window::Id::unique();
        windows.permission = Some((prompt, onboarding::Access::Unasked));
        assert!(!windows.mode(prompt).resizable);
    }

    #[test]
    fn renders_the_header_in_both_themes() {
        let mut windows = Windows::default();
        let (id, other) = (window::Id::unique(), window::Id::unique());
        windows.settings = Some((id, settings_state()));
        windows.onboarding = Some((other, onboarding::State::open()));
        for (name, choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
            theme::apply(choice);
            for (what, id, size) in [
                ("settings", id, settings::WINDOW_SIZE),
                ("onboarding", other, onboarding::WINDOW_SIZE),
            ] {
                let px = shot::save(
                    &format!("{what}-header-{name}"),
                    windows.view(id).unwrap(),
                    size,
                );
                // The header is the window's own colour all the way across (no gray strip).
                let w = size.width as usize;
                let bg = crate::ui::kit::themes::c().bg;
                let bg = [bg.r, bg.g, bg.b].map(|c| (c * 255.0).round() as u8);
                let row = &px[(10 * w) * 4..(11 * w) * 4];
                let plain = row.chunks(4).filter(|p| p[..3] == bg).count();
                assert!(plain > w * 6 / 10, "{what} {name}: {plain} of {w}");
            }
        }
        theme::apply(Theme::Dark);
    }

    #[test]
    fn two_quick_presses_on_the_header_are_a_double_click() {
        let t = Instant::now();
        assert!(!is_double_click(None, t));
        assert!(is_double_click(Some(t), t + Duration::from_millis(200)));
        assert!(!is_double_click(Some(t), t + Duration::from_millis(900)));
    }

    #[test]
    fn floating_windows_are_known_until_they_close() {
        let (id, _open) = floating(window::Settings::default(), 20.0, POPUP);
        assert!(is_floating(id));
        assert!(!is_floating(window::Id::unique()));
        let mut windows = Windows::default();
        let _ = windows.update(Msg::Closed(id));
        assert!(!is_floating(id));
    }

    #[test]
    fn the_tray_notice_names_the_configured_shortcut() {
        assert_eq!(
            tray_notice("Ctrl+Shift+Space"),
            "OpenCircle is running in the tray. Press Ctrl+Shift+Space."
        );
        assert!(tray_notice("Alt+PrintScreen").ends_with("Alt+PrtSc."));
    }

    /// Popups come and go by the design's motion (`motion`): the toast pill
    /// fades with an 8 px rise in 200 ms each way (#52), the others like
    /// every widget (210 ms open eased out, 150 ms close eased in). Where the
    /// OS composites a window's opacity it starts clear and fades in; the
    /// toast's fade by the clock: clear and 8 px low, then there.
    #[test]
    fn popups_come_and_go_by_the_design_motion() {
        let motion_of = |id| {
            let floating = platform::windows::FLOATING.lock().unwrap();
            floating.iter().find(|(f, ..)| *f == id).map(|(.., m)| *m)
        };
        let mut windows = Windows::default();
        let _ = windows.toast("Copied", true, None, TOAST_MS);
        let toast = windows.toasts.last().unwrap().0;
        assert_eq!(motion_of(toast), Some(TOAST));
        assert_eq!((TOAST.pace, TOAST.rise), (motion::TOAST_PACE, 8.0));
        let _ = windows.pin(picture(20, 10), None, 1.0);
        assert_eq!(motion_of(windows.pins[0].id), Some(POPUP));
        assert_eq!((POPUP.pace, POPUP.rise), (motion::WIDGET, 0.0));
        let t0 = Instant::now();
        let shown = motion::Shown::opening(TOAST.pace, t0);
        let at = |ms| shown.at(t0 + Duration::from_millis(ms));
        let low = |a: f32| (1.0 - a) * TOAST.rise;
        assert_eq!((at(0), low(at(0))), (0.0, 8.0));
        assert!(at(100) > 0.5 && at(100) < 1.0);
        assert_eq!((at(200), low(at(200))), (1.0, 0.0));
        let gone = motion::Shown::closing(TOAST.pace, t0);
        assert_eq!(gone.at(t0 + motion::TOAST), 0.0);
        let _ = windows.update(Msg::Closed(toast));
        assert!(motion_of(toast).is_none());
    }

    #[test]
    fn os_theme_message_reaches_the_palette() {
        let mut windows = Windows::default();
        theme::apply(Theme::System);
        let _ = windows.update(Msg::OsTheme(iced::theme::Mode::Light));
        let light = theme::current().text;
        let _ = windows.update(Msg::OsTheme(iced::theme::Mode::Dark));
        assert_ne!(theme::current().text, light);
    }
}
