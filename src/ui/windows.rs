//! The normal windows beside the overlay: Settings (from the tray) and the
//! first-run Onboarding. At most one of each; asking again focuses the open one.
//! Also the pinned images (any number): small borderless always-on-top windows.
//! Closing a window drops its state. Also owns the OS-theme feed.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use iced::widget::{container, image, mouse_area, row, stack};
use iced::window::{self, Level, Position};
use iced::{ContentFit, Element, Fill, Point, Size, Subscription, Task, keyboard, mouse};

use super::widgets::chrome;
use super::{motion, onboarding, settings, shape, theme, update_card, widgets};
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
}

#[derive(Default)]
pub struct Windows {
    settings: Option<(window::Id, settings::State)>,
    onboarding: Option<(window::Id, onboarding::State)>,
    permission: Option<(window::Id, onboarding::Access)>,
    pins: Vec<Pin>,
    /// Small message windows (a pill each) and their text.
    toasts: Vec<(window::Id, String, bool)>,
    /// The update card (release notes), at most one.
    update_card: Option<(window::Id, update_card::Card)>,
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

    /// A small message window: the toast pill (`widgets::toast_pill`) with `text`,
    /// floating over everything for `ms`, at `at` (logical desktop px) or centred.
    /// `ok` = the green dot; an error has none. The window is exactly the pill (see
    /// `floating`), so nothing shows around it.
    pub fn toast(&mut self, text: &str, ok: bool, at: Option<Point>, ms: u64) -> Task<Msg> {
        let size = toast_size(text, ok);
        let (id, open) = floating(
            window::Settings {
                size,
                position: at.map_or(Position::Centered, Position::Specific),
                resizable: false,
                ..window::Settings::default()
            },
            widgets::TOAST_RADIUS,
            TOAST,
        );
        self.toasts.push((id, text.to_string(), ok));
        // gone after `ms` (a thread sleeps, the UI never waits): fades out
        let (done, wait) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            let _ = done.send(());
        });
        open.discard()
            .chain(Task::future(wait).then(move |_| close(id)))
    }

    /// The open toasts' text and dot (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn toasts(&self) -> Vec<(String, bool)> {
        (self.toasts.iter())
            .map(|(_, text, ok)| (text.clone(), *ok))
            .collect()
    }

    /// The update card (release notes) with `card`; replaces an open one's content.
    pub fn update_card(&mut self, card: update_card::Card) -> Task<Msg> {
        if let Some((_, open)) = &mut self.update_card {
            *open = card;
            return Task::none();
        }
        let (id, open) = floating(
            window::Settings {
                size: update_card::SIZE,
                position: Position::Centered,
                resizable: false,
                ..window::Settings::default()
            },
            widgets::SURFACE_RADIUS,
            POPUP,
        );
        self.update_card = Some((id, card));
        open.discard()
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
        match msg {
            Msg::Settings(msg) => {
                if let Some((id, state)) = &mut self.settings {
                    let id = *id;
                    let outcome = settings::update(state, msg);
                    let leaving = state.leaving();
                    match outcome {
                        settings::Outcome::Saved(cfg) => apply(cfg),
                        settings::Outcome::Browse => return pick_folder(),
                        settings::Outcome::OpenLink(url) => open_url(&url),
                        settings::Outcome::CopyDiagnostics => {
                            let report = crate::diagnostics::report(&crate::config::current());
                            return match crate::actions::copy::text(report) {
                                Ok(()) => self.toast("Diagnostics copied", true, None, TOAST_MS),
                                Err(e) => {
                                    crate::log!("[opencircle] copy diagnostics failed: {e}");
                                    self.toast("Couldn't copy", false, None, TOAST_MS)
                                }
                            };
                        }
                        settings::Outcome::CheckUpdate => crate::update::check_now(),
                        settings::Outcome::None => {}
                    }
                    if leaving {
                        return window::close(id);
                    }
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
                    update_card::Msg::Card(widgets::card::Event::Grab(
                        widgets::card::Drag::Move,
                    )) => {
                        return window::drag(id);
                    }
                    update_card::Msg::Card(widgets::card::Event::Close) => {
                        return close(id);
                    }
                    update_card::Msg::Card(_) => {}
                    update_card::Msg::Link(url) => open_url(&url),
                    update_card::Msg::Page => open_url(&card.url),
                }
            }
            Msg::Onboarding(msg) => {
                if let Some((id, state)) = &mut self.onboarding {
                    match onboarding::update(state, msg) {
                        onboarding::Outcome::Finished(cfg) => {
                            let id = *id;
                            self.onboarding = None;
                            let notice = tray_notice(&cfg.shortcut);
                            apply(cfg);
                            return Task::batch([
                                window::close(id),
                                self.toast(&notice, true, None, NOTICE_MS),
                            ]);
                        }
                        onboarding::Outcome::AskScreenshot => {
                            return ask_screenshot(Msg::Onboarding);
                        }
                        onboarding::Outcome::None => {}
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
                forget_floating(id);
                HANDLES.lock().unwrap().retain(|(w, _)| *w != id);
                self.chrome.remove(&id);
                self.toasts.retain(|(t, ..)| *t != id);
                if self.update_card.as_ref().is_some_and(|(c, _)| *c == id) {
                    self.update_card = None;
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
            resizable: [Kind::Settings, Kind::Onboarding]
                .into_iter()
                .any(|kind| self.id(kind) == Some(id)),
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
            return Some(update_card::view(card).map(Msg::UpdateCard));
        }
        if let Some((_, text, ok)) = self.toasts.iter().find(|(t, ..)| *t == window) {
            return Some(
                widgets::toast_pill(text.as_str(), *ok)
                    .width(Fill)
                    .height(Fill)
                    .center(Fill)
                    .into(),
            );
        }
        let page = if let Some((id, state)) = &self.settings
            && *id == window
        {
            settings::view(state).map(Msg::Settings)
        } else if let Some((id, state)) = &self.onboarding
            && *id == window
        {
            onboarding::view(state).map(Msg::Onboarding)
        } else if let Some((id, access)) = &self.permission
            && *id == window
        {
            onboarding::prompt_view(access).map(Msg::Permission)
        } else {
            return None;
        };
        let title = self.header_title(window);
        Some(chrome::frame(title, self.mode(window), page, move |e| {
            Msg::Chrome(window, e)
        }))
    }

    /// The header's title (the OS title, `title`, is the longer name for the taskbar).
    fn header_title(&self, window: window::Id) -> &'static str {
        match (self.id(Kind::Settings), self.id(Kind::Onboarding)) {
            (Some(id), _) if id == window => "Settings",
            (_, Some(id)) if id == window => "Welcome to OpenCircle",
            _ => "Screenshot permission",
        }
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
        Subscription::batch([
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
        ])
    }

    /// Test hook, `OPENCIRCLE_TEST_OPEN=settings,shot,settings,shot`: one step every
    /// 1.5 s. `settings` = the tray's Settings item, `permission` / `toast` = those windows, `max` = maximise or restore Settings, `light`/`dark`/`save` and `next`/`finish` = the same clicks in Settings / onboarding, `close` = the X button on Settings, `problem` = an unsaveable edit, `shot` = save the open window as
    /// `$OPENCIRCLE_TEST_DIR/<kind>-<step>.png`, `quit`.
    fn test_step(&mut self, step: usize, what: &str) -> Task<Msg> {
        match what {
            "settings" => self.open(Kind::Settings),
            "permission" => self.open(Kind::Permission),
            "toast" => self.toast("Copied", true, None, TOAST_MS),
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
    let old = config::current();
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

// ---- the three ways to open a window, all through `open` (a test forbids any other) ----

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
        Kind::Onboarding => (onboarding::WINDOW_SIZE, Some(onboarding::MIN_SIZE), true),
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
    open(settings)
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

/// The setup's window (`ui::setup`): a fixed-size normal window (taskbar entry,
/// our own header), centred, cut to the design's rounded corners (`radius`, as
/// a floating window: nothing shows outside them, and it fades in and out by
/// `POPUP`). After a `window::resize`, `reshape` cuts it again.
pub fn setup_window(size: Size, radius: f32) -> (window::Id, Task<window::Id>) {
    let fades = shape::can_fade();
    let (id, open) = open(window::Settings {
        size,
        resizable: false,
        position: Position::Centered,
        exit_on_close_request: false,
        visible: !fades, // shown once cut (`appear`)
        ..window::Settings::default()
    });
    FLOATING.lock().unwrap().push((id, radius, POPUP));
    let open = open.then(move |id| {
        let shown = match fades {
            true => appear(id, POPUP),
            false => Task::none(),
        };
        reshape(id).discard().chain(shown).chain(Task::done(id))
    });
    (id, open)
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

/// A toast's window size (logical px): padding 14x8, a 12 px line, the dot.
pub fn toast_size(text: &str, ok: bool) -> Size {
    let dot = if ok { 16.0 } else { 0.0 };
    Size::new(30.0 + dot + 6.6 * text.chars().count() as f32, 34.0)
}

/// A small popup (toast, pin, live controller, any future one): THE way to open one.
/// Undecorated, always on top, no taskbar entry (Windows), transparent with a
/// fully clear background (`clear_theme`), and cut to its content's rounded
/// rectangle of at most `radius` logical px (`shape`): where the window has no
/// per-pixel alpha (Windows, X11 without a compositor) nothing outside the shape
/// exists, so no box shows behind a pill. Nothing in a floating window may paint
/// outside its content (no shadow: use `widgets::solid_surface` / `toast_pill`).
/// After a `window::resize` nothing is needed: the resize event reshapes it.
/// It comes and goes by `how` (`Motion`): faded in once its shape is cut,
/// faded out by `close`.
pub fn floating(
    settings: window::Settings,
    radius: f32,
    how: Motion,
) -> (window::Id, Task<window::Id>) {
    let fades = settings.visible && shape::can_fade();
    let settings = window::Settings {
        decorations: false,
        transparent: true,
        level: Level::AlwaysOnTop,
        exit_on_close_request: false,
        // shown once clear and cut (`appear`)
        visible: settings.visible && !fades,
        ..settings
    };
    // ponytail: Linux cannot hide a window from the taskbar through iced.
    #[cfg(windows)]
    let settings = window::Settings {
        platform_specific: window::settings::PlatformSpecific {
            skip_taskbar: true,
            ..Default::default()
        },
        ..settings
    };
    let (id, open) = open(settings);
    FLOATING.lock().unwrap().push((id, radius, how));
    let open = open.then(move |id| {
        let shown = match fades {
            true => appear(id, how),
            false => Task::none(),
        };
        reshape(id).discard().chain(shown).chain(Task::done(id))
    });
    (id, open)
}

/// How a floating window comes and goes (`motion`): its pace, and how far it
/// rises as it shows (sinks as it goes), logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub pace: motion::Pace,
    pub rise: f32,
}

/// Popups (pin, update card, live controller): the design's widget open/close.
pub const POPUP: Motion = Motion {
    pace: motion::WIDGET,
    rise: 0.0,
};
/// The toast pill: fade and an 8 px rise (#52).
pub const TOAST: Motion = Motion {
    pace: motion::TOAST_PACE,
    rise: motion::TOAST_RISE,
};

/// Shows the hidden window `id` from fully clear and fades it in by `how`
/// (`fade`; the task ends at once, the fade runs on).
pub fn appear<T: Send + 'static>(id: window::Id, how: Motion) -> Task<T> {
    opacity(id, 0.0)
        .chain(window::set_mode(id, window::Mode::Windowed))
        .chain(fade(id, true, how, false))
}

/// Closes the window `id`: a floating one fades out first (by its `Motion`),
/// where windows can fade; any other closes at once.
pub fn close<T: Send + 'static>(id: window::Id) -> Task<T> {
    let how = (FLOATING.lock().unwrap().iter())
        .find(|(f, ..)| *f == id)
        .map(|(_, _, how)| *how);
    match how {
        Some(how) if shape::can_fade() => fade(id, false, how, true).chain(window::close(id)),
        _ => window::close(id),
    }
}

/// The window `id` at `alpha` (0..1) of its opacity, as a whole (`shape::opacity`).
pub fn opacity<T: Send + 'static>(id: window::Id, alpha: f32) -> Task<T> {
    window::run(id, move |w| {
        if let Ok(handle) = w.window_handle()
            && let Err(e) = shape::opacity(handle.as_raw(), alpha)
        {
            crate::log!("[opencircle] window opacity: {e}");
        }
    })
    .discard()
}

/// Fades the window `id` in (`open`) or out by `how` on a thread of its own
/// (`shape::fade`: the OS composites each step, nothing is redrawn); the
/// task ends at once, or with the fade when `wait`. Nothing where windows
/// can't fade.
pub fn fade<T: Send + 'static>(id: window::Id, open: bool, how: Motion, wait: bool) -> Task<T> {
    if !shape::can_fade() {
        return Task::none();
    }
    let started = window::run(id, move |w| {
        let (done, finished) = iced::futures::channel::oneshot::channel::<()>();
        let window = w
            .window_handle()
            .ok()
            .and_then(|h| shape::fadeable(h.as_raw()));
        if let Some(window) = window {
            motion::refresh();
            let now = Instant::now();
            let shown = match open {
                true => motion::Shown::opening(how.pace, now),
                false => motion::Shown::closing(how.pace, now),
            };
            let run = shape::fade(window, shown, how.rise);
            std::thread::spawn(move || {
                run();
                let _ = done.send(());
            });
        }
        finished
    });
    match wait {
        true => started.then(Task::future).discard(),
        false => started.discard(),
    }
}

/// A screen-sized window (the capture overlay, the live layer): no frame, not
/// cut to a shape; each sets its own transparency and level.
pub fn screen(settings: window::Settings) -> (window::Id, Task<window::Id>) {
    open(settings)
}

/// What the three above share: `app_window`'s settings, and on Windows no classic
/// title bar painted over the window at (de)activation (`shape::no_caption`).
/// A window asked off the taskbar (`skip_taskbar`: the overlay, popups) is a tool
/// window for good (`shape::no_taskbar`: winit's own skip comes undone).
fn open(settings: window::Settings) -> (window::Id, Task<window::Id>) {
    #[cfg(windows)]
    let no_taskbar = settings.platform_specific.skip_taskbar;
    #[cfg(not(windows))]
    let no_taskbar = false;
    let (id, opened) = window::open(app_window(settings));
    let caption_off = move |id| {
        window::run(id, move |w| {
            if let Ok(handle) = w.window_handle() {
                shape::no_caption(handle.as_raw());
                if no_taskbar {
                    shape::no_taskbar(handle.as_raw());
                }
                if let Some(raw) = shape::fadeable(handle.as_raw()) {
                    HANDLES.lock().unwrap().push((id, raw));
                }
            }
        })
        .discard()
        .chain(Task::done(id))
    };
    (id, opened.then(caption_off))
}

/// Every window `open` opened, with its handle as `shape::fadeable` gives it
/// (where windows can fade), for the calls that cannot wait on a task.
static HANDLES: Mutex<Vec<(window::Id, isize)>> = Mutex::new(Vec::new());

/// The window `id` clear NOW, on the UI thread, any fade of it stopped
/// (`shape::clear_now`): every frame it draws from here on stays unseen until
/// its opacity is set again (`show` does). A `Task` would run after the next
/// frame may already be on screen. Nothing where windows can't fade.
pub fn clear_now(id: window::Id) {
    #[cfg(test)]
    CLEARED.with(|c| c.borrow_mut().push(id));
    let raw = (HANDLES.lock().unwrap().iter()).find_map(|(w, raw)| (*w == id).then_some(*raw));
    if let Some(raw) = raw {
        shape::clear_now(raw);
    }
}

#[cfg(test)]
thread_local! {
    /// The windows `clear_now` was called on (this test's thread).
    pub static CLEARED: std::cell::RefCell<Vec<window::Id>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Floating windows and their corner radius (see `floating`).
static FLOATING: Mutex<Vec<(window::Id, f32, Motion)>> = Mutex::new(Vec::new());

pub fn is_floating(id: window::Id) -> bool {
    FLOATING.lock().unwrap().iter().any(|(f, ..)| *f == id)
}

fn forget_floating(id: window::Id) {
    FLOATING.lock().unwrap().retain(|(f, ..)| *f != id);
}

/// The theme of floating windows: a fully clear background.
pub fn clear_theme() -> iced::Theme {
    static THEME: std::sync::LazyLock<iced::Theme> = std::sync::LazyLock::new(|| {
        let mut palette = iced::Theme::Dark.palette();
        palette.background = iced::Color::TRANSPARENT;
        iced::Theme::custom("clear", palette)
    });
    THEME.clone()
}

/// Cuts an existing window (the capture overlay shrunk to a pill) to its content's
/// rounded rectangle of at most `radius` px, now and after every resize, until
/// `uncut`. Where a window is opened as a popup, `floating` does this itself.
pub fn cut(id: window::Id, radius: f32) -> Task<Msg> {
    forget_floating(id);
    FLOATING.lock().unwrap().push((id, radius, POPUP));
    reshape(id)
}

/// Windows: keeps the window out of every screen capture (`on`), or back in. The
/// scrolling capture's pill sits over the area it grabs: in the frames its
/// changing text would keep the page from ever settling, and end up in the
/// stitched image. Needs Windows 10 2004+; elsewhere nothing.
pub fn hide_from_capture(id: window::Id, on: bool) -> Task<Msg> {
    window::run(id, move |w| {
        #[cfg(windows)]
        if let Ok(handle) = w.window_handle()
            && let iced::window::raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw()
        {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
            };
            let affinity = if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
            // SAFETY: `h` is this window's live handle (an iced window of ours).
            if let Err(e) =
                unsafe { SetWindowDisplayAffinity(HWND(h.hwnd.get() as *mut _), affinity) }
            {
                crate::log!("[opencircle] window affinity: {e}");
            }
        }
        #[cfg(not(windows))]
        let _ = (w, on);
    })
    .discard()
}

/// Whole again (and not floating): the overlay is going full-screen.
pub fn uncut(id: window::Id) -> Task<Msg> {
    forget_floating(id);
    window::run(id, |w| {
        if let Ok(handle) = w.window_handle()
            && let Err(e) = shape::clear(handle.as_raw())
        {
            crate::log!("[opencircle] window shape: {e}");
        }
    })
    .discard()
}

/// Cuts the floating window `id` to its content's shape again (it was resized).
/// Nothing for any other window.
pub fn reshape(id: window::Id) -> Task<Msg> {
    let radius = FLOATING
        .lock()
        .unwrap()
        .iter()
        .find(|(f, ..)| *f == id)
        .map(|(_, r, _)| *r);
    let Some(radius) = radius else {
        return Task::none();
    };
    window::run(id, move |w| {
        if let Ok(handle) = w.window_handle()
            && let Err(e) = shape::apply(handle.as_raw(), radius)
        {
            crate::log!("[opencircle] window shape: {e}");
        }
    })
    .discard()
}

/// `settings` with no OS title bar (every window of ours draws its own header or
/// none, whatever the caller asked for), our logo as the window icon (taskbar,
/// Alt+Tab) and, on Linux,
/// our app id: the desktop ties the window to `APP_ID.desktop`, and GNOME asks for
/// screenshot permission only for the app it sees in front.
fn app_window(settings: window::Settings) -> window::Settings {
    let logo = chrome::logo();
    let settings = window::Settings {
        decorations: false,
        icon: window::icon::from_rgba(logo.rgba.clone(), logo.width, logo.height).ok(),
        ..settings
    };
    #[cfg(target_os = "linux")]
    return window::Settings {
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::capture::portal::APP_ID.into(),
            ..settings.platform_specific
        },
        ..settings
    };
    #[cfg(not(target_os = "linux"))]
    settings
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
    if let Err(e) = crate::lens::open_link(url) {
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
    use crate::config::Provider;
    use crate::secrets::{Secret, SecretsError};
    use crate::ui::widgets::shot;

    struct NoKeys;
    impl settings::KeyStore for NoKeys {
        fn is_set(&self, _: Provider) -> Result<bool, SecretsError> {
            Ok(false)
        }
        fn set(&self, _: Provider, _: &Secret) -> Result<(), SecretsError> {
            Ok(())
        }
        fn delete(&self, _: Provider) -> Result<(), SecretsError> {
            Ok(())
        }
    }

    fn settings_state() -> settings::State {
        settings::State::new(Config::default(), vec![], None, Box::new(NoKeys))
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
        let refused = Err("refused by the desktop".to_string());
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
        assert!(texts.iter().any(|t| t == "Settings"), "{texts:?}");
        assert_eq!(windows.header_title(id), "Settings");
        assert!(windows.mode(id).resizable && !windows.mode(id).maximised);
        // Maximised once the OS says so; edges are gone, restore shows.
        windows.chrome.insert(id, Chrome::default());
        let _ = windows.update(Msg::Maximised(id, true));
        assert!(windows.mode(id).maximised);
        // Onboarding resizes too (#53: a short screen); the fixed-size
        // permission prompt cannot be maximised.
        windows.onboarding = Some((other, onboarding::State::open()));
        assert!(windows.mode(other).resizable);
        assert_eq!(windows.header_title(other), "Welcome to OpenCircle");
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
                let bg = theme::current().window;
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
            let floating = FLOATING.lock().unwrap();
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
    fn no_window_ever_gets_the_os_title_bar() {
        let asked = window::Settings {
            decorations: true,
            ..window::Settings::default()
        };
        assert!(!app_window(asked).decorations);
    }

    /// Every window is opened by `normal`, `floating`, `screen` or `setup_window` here, all through
    /// `open`, so none can show a box, a gray strip or the OS title bar by forgetting
    /// a setting.
    #[test]
    fn no_window_is_opened_outside_the_helpers() {
        let call = concat!("window::", "open(");
        let mut stray = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    // The tests of this very file mention the call.
                    let code = text.split("#[cfg(test)]").next().unwrap();
                    let n = code.matches(call).count();
                    if path.ends_with("ui/windows.rs") {
                        assert_eq!(n, 1, "normal, floating and screen all go through open");
                    } else if n > 0 {
                        stray.push(path.display().to_string());
                    }
                }
            }
        }
        assert!(
            stray.is_empty(),
            "open windows through windows::floating / screen: {stray:?}"
        );
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
