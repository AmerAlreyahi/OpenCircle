//! The normal windows beside the overlay: Settings (from the tray) and the
//! first-run Onboarding. At most one of each; asking again focuses the open one.
//! Closing a window drops its state. Also owns the OS-theme feed.
use iced::window::{self, Position};
use iced::{Element, Size, Subscription, Task};

use super::{onboarding, settings, theme};
use crate::config::{self, Config, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Settings,
    Onboarding,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Settings(settings::Message),
    Onboarding(onboarding::Message),
    /// Any window closed (ours are forgotten, the overlay never closes).
    Closed(window::Id),
    /// The OS light/dark mode, at start and on every change.
    OsTheme(iced::theme::Mode),
    /// Test hook step (see `test_hook`).
    Test(usize, String),
    TestShot(usize, Kind, window::Screenshot),
}

#[derive(Default)]
pub struct Windows {
    settings: Option<(window::Id, settings::State)>,
    onboarding: Option<(window::Id, onboarding::State)>,
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
        }
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
        }
        open.discard()
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Settings(msg) => {
                if let Some((_, state)) = &mut self.settings
                    && let settings::Outcome::Saved(cfg) = settings::update(state, msg)
                {
                    apply(cfg);
                }
            }
            Msg::Onboarding(msg) => {
                if let Some((id, state)) = &mut self.onboarding
                    && let onboarding::Outcome::Finished(cfg) = onboarding::update(state, msg)
                {
                    let id = *id;
                    self.onboarding = None;
                    apply(cfg);
                    return window::close(id);
                }
            }
            Msg::Closed(id) => {
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
            Msg::TestShot(step, kind, shot) => save_test_shot(step, kind, &shot),
        }
        Task::none()
    }

    pub fn view(&self, window: window::Id) -> Option<Element<'_, Msg>> {
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
        None
    }

    pub fn title(&self, window: window::Id) -> &'static str {
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
        ])
    }

    /// Test hook, `OPENCIRCLE_TEST_OPEN=settings,shot,settings,shot`: one step every
    /// 1.5 s. `settings` = the tray's Settings item, `light`/`dark`/`save` and `next`/`finish` = the same clicks in Settings / onboarding, `close` = the X button on Settings, `shot` = save the open window as
    /// `$OPENCIRCLE_TEST_DIR/<kind>-<step>.png`, `quit`.
    fn test_step(&mut self, step: usize, what: &str) -> Task<Msg> {
        match what {
            "settings" => self.open(Kind::Settings),
            "shot" => {
                let open = [Kind::Settings, Kind::Onboarding]
                    .into_iter()
                    .find_map(|kind| Some((kind, self.id(kind)?)));
                let Some((kind, id)) = open else {
                    return Task::none();
                };
                window::screenshot(id).map(move |shot| Msg::TestShot(step, kind, shot))
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
    };
    window::Settings {
        size,
        min_size,
        resizable,
        position: Position::Centered,
        ..window::Settings::default()
    }
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

fn save_test_shot(step: usize, kind: Kind, shot: &window::Screenshot) {
    let Some(dir) = std::env::var_os("OPENCIRCLE_TEST_DIR") else {
        return;
    };
    let name = format!("{}-{step}.png", format!("{kind:?}").to_lowercase());
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
