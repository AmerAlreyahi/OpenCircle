//! The normal windows beside the overlay: Settings (from the tray) and the
//! first-run Onboarding. At most one of each; asking again focuses the open one.
//! Also the pinned images (any number): small borderless always-on-top windows.
//! Closing a window drops its state. Also owns the OS-theme feed.
use iced::widget::{container, image, mouse_area, row, stack};
use iced::window::{self, Level, Position};
use iced::{ContentFit, Element, Fill, Point, Size, Subscription, Task, keyboard, mouse};

use super::{onboarding, settings, theme, widgets};
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
    /// Any window closed (ours are forgotten, the overlay never closes).
    Closed(window::Id),
    /// The OS light/dark mode, at start and on every change.
    OsTheme(iced::theme::Mode),
    /// From a pinned image's window.
    Pin(window::Id, PinMsg),
    /// The folder picker answered (`None` = cancelled).
    Folder(Option<std::path::PathBuf>),
    /// Test hook step (see `test_hook`).
    Test(usize, String),
    TestShot(usize, &'static str, window::Screenshot),
}

#[derive(Default)]
pub struct Windows {
    settings: Option<(window::Id, settings::State)>,
    onboarding: Option<(window::Id, onboarding::State)>,
    permission: Option<(window::Id, onboarding::Access)>,
    pins: Vec<Pin>,
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
        eprintln!("{} failed: {e}", action.id);
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
        let button = |glyph, message, tip| {
            widgets::with_tip(
                widgets::icon_button(glyph, false, message),
                tip,
                Side::Bottom,
            )
        };
        let drag = dragout::SUPPORTED.then(|| {
            widgets::with_tip(
                widgets::drag_handle(widgets::DRAG, PinMsg::DragOut),
                "Drag the image into another app",
                Side::Bottom,
            )
        });
        let bar = widgets::surface(
            row![
                drag,
                button(COPY, PinMsg::Copy, "Copy the image (Ctrl+C)"),
                button(SAVE, PinMsg::Save, "Save the image (Ctrl+S)"),
                button(widgets::CLOSE, PinMsg::Close, "Close (Esc or double-click)"),
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
        let (id, open) = window::open(app_window(window::Settings {
            size: base * zoom,
            position: at.map_or(Position::Centered, Position::Specific),
            decorations: false,
            resizable: false,
            level: Level::AlwaysOnTop,
            ..window::Settings::default()
        }));
        eprintln!("[opencircle] pinned {}x{}", crop.width, crop.height);
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

    /// Opens the window, or focuses it when it is already open.
    pub fn open(&mut self, kind: Kind) -> Task<Msg> {
        if let Some(id) = self.id(kind) {
            eprintln!("[opencircle] {kind:?} window is open: focusing it");
            return window::minimize(id, false).chain(window::gain_focus(id));
        }
        eprintln!("[opencircle] opening the {kind:?} window");
        let (id, open) = window::open(window_settings(kind));
        match kind {
            Kind::Settings => self.settings = Some((id, settings::State::open())),
            Kind::Onboarding => self.onboarding = Some((id, onboarding::State::open())),
            Kind::Permission => self.permission = Some((id, onboarding::Access::Unasked)),
        }
        open.discard().chain(window::gain_focus(id))
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Settings(msg) => {
                if let Some((_, state)) = &mut self.settings {
                    match settings::update(state, msg) {
                        settings::Outcome::Saved(cfg) => apply(cfg),
                        settings::Outcome::Browse => return pick_folder(),
                        settings::Outcome::OpenProject => open_url(settings::PROJECT_URL),
                        settings::Outcome::None => {}
                    }
                }
            }
            Msg::Folder(dir) => {
                if let Some((_, state)) = &mut self.settings {
                    settings::update(state, settings::Message::Folder(dir));
                }
            }
            Msg::Pin(id, msg) => return self.on_pin(id, msg),
            Msg::Onboarding(msg) => {
                if let Some((id, state)) = &mut self.onboarding {
                    match onboarding::update(state, msg) {
                        onboarding::Outcome::Finished(cfg) => {
                            let id = *id;
                            self.onboarding = None;
                            apply(cfg);
                            return window::close(id);
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
            Msg::Closed(id) => {
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
                                eprintln!("[opencircle] {e}");
                            }
                            pin.dragged = Some(file);
                        }
                        Err(e) => eprintln!("[opencircle] drag-out: {e}"),
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
                self.pins.remove(at); // frees the image
                window::close(id)
            }
        }
    }

    pub fn view(&self, window: window::Id) -> Option<Element<'_, Msg>> {
        if let Some(pin) = self.pins.iter().find(|p| p.id == window) {
            let id = pin.id;
            return Some(pin_view(pin).map(move |msg| Msg::Pin(id, msg)));
        }
        if let Some((id, state)) = &self.settings
            && *id == window
        {
            return Some(settings::view(state).map(Msg::Settings));
        }
        if let Some((id, state)) = &self.onboarding
            && *id == window
        {
            return Some(onboarding::view(state).map(Msg::Onboarding));
        }
        if let Some((id, access)) = &self.permission
            && *id == window
        {
            return Some(onboarding::prompt_view(access).map(Msg::Permission));
        }
        None
    }

    pub fn title(&self, window: window::Id) -> &'static str {
        if self.pins.iter().any(|p| p.id == window) {
            return "OpenCircle pin";
        }
        if self.id(Kind::Permission) == Some(window) {
            return "OpenCircle needs screenshot permission";
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
    /// 1.5 s. `settings` = the tray's Settings item, `light`/`dark`/`save` and `next`/`finish` = the same clicks in Settings / onboarding, `close` = the X button on Settings, `shot` = save the open window as
    /// `$OPENCIRCLE_TEST_DIR/<kind>-<step>.png`, `quit`.
    fn test_step(&mut self, step: usize, what: &str) -> Task<Msg> {
        match what {
            "settings" => self.open(Kind::Settings),
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
                    eprintln!("test pin failed: {e}");
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
            "close" => self
                .id(Kind::Settings)
                .map_or_else(Task::none, window::close),
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
    if old.shortcut != cfg.shortcut || old.quick_actions != cfg.quick_actions {
        crate::hotkey::reload(&cfg);
    }
    config::set_current(cfg);
}

fn window_settings(kind: Kind) -> window::Settings {
    let (size, min_size, resizable) = match kind {
        Kind::Settings => (settings::WINDOW_SIZE, Some(Size::new(720.0, 520.0)), true),
        Kind::Onboarding => (onboarding::WINDOW_SIZE, None, false),
        Kind::Permission => (onboarding::PROMPT_SIZE, None, false),
    };
    app_window(window::Settings {
        size,
        min_size,
        resizable,
        position: Position::Centered,
        ..window::Settings::default()
    })
}

/// `settings` with our app id (Linux): the desktop ties the window to
/// `APP_ID.desktop`, and GNOME asks for screenshot permission only for the app it
/// sees in front.
pub fn app_window(settings: window::Settings) -> window::Settings {
    #[cfg(target_os = "linux")]
    return window::Settings {
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::capture::portal::APP_ID.into(),
            ..Default::default()
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

/// Opens `url` (one of our own constants) in the default browser.
fn open_url(url: &'static str) {
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    use std::process::Stdio;
    match cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        // Reaped off the UI thread: xdg-open can outlive the click.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(e) => eprintln!("cannot open {url}: {e}"),
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
        eprintln!("test shot failed: {e}");
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

        // Esc / double-click / the close button: that pin goes, the others stay.
        let _ = windows.update(Msg::Pin(ids[1], PinMsg::Close));
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
    fn os_theme_message_reaches_the_palette() {
        let mut windows = Windows::default();
        theme::apply(Theme::System);
        let _ = windows.update(Msg::OsTheme(iced::theme::Mode::Light));
        let light = theme::current().text;
        let _ = windows.update(Msg::OsTheme(iced::theme::Mode::Dark));
        assert_ne!(theme::current().text, light);
    }
}
