//! First-run onboarding as a self-contained component (own `State`, `Message`,
//! `update`, `view`), four short steps: what it does + the shortcut, pick an AI
//! (optional), pick a look, done. Finish writes the config.
//!
//! ```ignore
//! let mut state = onboarding::State::open();           // loads the config, real keyring
//! // window size: onboarding::WINDOW_SIZE
//! // update: if let onboarding::Outcome::Finished(cfg) = onboarding::update(&mut state, msg) { ...close, start... }
//! // view: onboarding::view(&state)
//! ```
//!
//! The API key goes to the OS keyring (same `KeyField` as Settings) and is never
//! shown again or written anywhere else.

use std::fmt;
use std::path::PathBuf;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, row, space};
use iced::{Element, Length, Size};

use super::settings::{KeyField, KeyMessage, KeyStatus, KeyStore, Keyring, PRESETS, key_labels};
use crate::config::{Config, Provider, Theme};
use crate::ui::theme;
use crate::ui::widgets::{
    self, FORM_WIDTH, body, caption, divider, error_text, key_cap, label_button, segmented, tip,
    title,
};

/// Logical size of the onboarding window.
pub const WINDOW_SIZE: Size = Size::new(620.0, 520.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Ai,
    Look,
    Done,
}

const STEPS: [Step; 4] = [Step::Welcome, Step::Ai, Step::Look, Step::Done];

/// What the user picks on the AI step. "Skip" is the footer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Google AI Mode in the user's browser: no key.
    Browser,
    Claude,
    /// Index into `settings::PRESETS`.
    Preset(usize),
    Skip,
}

pub static CHOICES: [Choice; 7] = [
    Choice::Browser,
    Choice::Claude,
    Choice::Preset(0),
    Choice::Preset(1),
    Choice::Preset(2),
    Choice::Preset(3),
    Choice::Preset(4),
];

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Choice::Browser => f.write_str("Google AI Mode (browser, no key)"),
            Choice::Claude => f.write_str("Claude (Anthropic)"),
            Choice::Preset(i) => f.write_str(PRESETS[*i].name),
            Choice::Skip => f.write_str("Skip, set up later"),
        }
    }
}

impl Choice {
    fn provider(self) -> Option<Provider> {
        match self {
            Choice::Browser => Some(Provider::Browser),
            Choice::Claude => Some(Provider::Anthropic),
            Choice::Preset(_) => Some(Provider::OpenaiCompatible),
            Choice::Skip => None,
        }
    }

    fn local(self) -> bool {
        matches!(self, Choice::Preset(i) if PRESETS[i].local)
    }
}

pub struct State {
    step: Step,
    base: Config,
    theme: Theme,
    choice: Choice,
    key: KeyField,
    store: Box<dyn KeyStore>,
    /// `None` = the real config path.
    path: Option<PathBuf>,
    error: Option<String>,
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("onboarding::State")
            .field("step", &self.step)
            .field("choice", &self.choice)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Next,
    Back,
    /// "Skip for now" on the AI step.
    SkipAi,
    Choose(Choice),
    ApiKey(KeyMessage),
    Theme(Theme),
    Finish,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    None,
    /// The config was written; close the window and carry on.
    Finished(Config),
}

impl State {
    /// The real config file and the real keyring. Google AI Mode is preselected.
    pub fn open() -> Self {
        let (config, _warnings) = Config::load();
        Self::new(config, None, Box::new(Keyring))
    }

    pub fn new(base: Config, path: Option<PathBuf>, store: Box<dyn KeyStore>) -> Self {
        theme::apply(base.theme);
        let key = KeyField::new(&*store, Provider::Anthropic);
        State {
            step: Step::Welcome,
            theme: base.theme,
            base,
            choice: Choice::Browser,
            key,
            store,
            path,
            error: None,
        }
    }

    #[cfg(test)]
    pub fn step(&self) -> Step {
        self.step
    }

    /// The config Finish will write: the loaded one, plus the theme and AI picks.
    pub fn config(&self) -> Config {
        let mut c = self.base.clone();
        c.theme = self.theme;
        match self.choice {
            Choice::Browser => c.ai.provider = Provider::Browser,
            Choice::Claude => {
                c.ai.provider = Provider::Anthropic;
                c.ai.base_url.clear();
                if c.ai.model.is_empty() || PRESETS.iter().any(|p| p.model == c.ai.model) {
                    c.ai.model = crate::ai::DEFAULT_ANTHROPIC_MODEL.to_string();
                }
            }
            Choice::Preset(i) => {
                c.ai.provider = Provider::OpenaiCompatible;
                c.ai.base_url = PRESETS[i].base_url.to_string();
                c.ai.model = PRESETS[i].model.to_string();
            }
            Choice::Skip => {}
        }
        c
    }

    fn choose(&mut self, choice: Choice) {
        let before = self.choice.provider();
        self.choice = choice;
        // The browser needs no key: nothing to look up.
        let keyed = choice.provider().filter(|p| *p != Provider::Browser);
        if let Some(p) = keyed.filter(|p| Some(*p) != before) {
            self.key.refresh(&*self.store, p);
        }
    }

    fn go(&mut self, to: isize) {
        let i = STEPS.iter().position(|s| *s == self.step).unwrap_or(0) as isize;
        self.step = STEPS[(i + to).clamp(0, STEPS.len() as isize - 1) as usize];
    }
}

pub fn update(state: &mut State, message: Message) -> Outcome {
    state.error = None;
    match message {
        Message::Next => state.go(1),
        Message::Back => state.go(-1),
        Message::SkipAi => {
            state.choose(Choice::Skip);
            state.go(1);
        }
        Message::Choose(c) => state.choose(c),
        Message::ApiKey(k) => {
            if let Some(p) = state.choice.provider().filter(|p| *p != Provider::Browser) {
                state.key.update(&*state.store, p, k);
            }
        }
        Message::Theme(t) => {
            state.theme = t;
            theme::apply(t);
        }
        Message::Finish => {
            let config = state.config();
            let result = match &state.path {
                Some(p) => config.save_to(p),
                None => config.save(),
            };
            match result {
                Ok(()) => return Outcome::Finished(config),
                Err(e) => state.error = Some(format!("Could not save your settings: {e}")),
            }
        }
    }
    Outcome::None
}

pub fn view(state: &State) -> Element<'_, Message> {
    let page = match state.step {
        Step::Welcome => welcome(state),
        Step::Ai => ai(state),
        Step::Look => look(state),
        Step::Done => done(state),
    };
    widgets::window(column![
        container(page)
            .padding([40, 48])
            .width(Length::Fill)
            .height(Length::Fill),
        divider(),
        footer(state),
    ])
    .into()
}

fn keys<'a>(shortcut: &str) -> Element<'a, Message> {
    row(key_labels(shortcut).into_iter().map(|k| key_cap(k).into()))
        .spacing(6)
        .into()
}

fn welcome(state: &State) -> Element<'_, Message> {
    column![
        title("Circle anything on your screen"),
        body(
            "Press the shortcut anywhere. The screen freezes: circle or drag over what you \
             want, then search it, read it, translate it or ask an AI about it."
        ),
        keys(&state.base.shortcut),
        caption("Nothing leaves your PC until you pick an action. You can change the shortcut in Settings."),
    ]
    .spacing(20)
    .into()
}

fn ai(state: &State) -> Element<'_, Message> {
    let mut col = column![
        title("Pick an AI (optional)"),
        body(
            "An AI can explain what you circle. Google AI Mode needs no key; use your own \
             key to translate in place too. You can change this any time in Settings."
        ),
        tip(
            widgets::dropdown(&CHOICES, Some(state.choice), "Provider", Message::Choose),
            "AI provider",
            Position::Top,
        ),
    ]
    .spacing(20)
    .width(FORM_WIDTH + 40.0);
    if state.choice == Choice::Browser {
        col = col.push(caption(
            "No key needed. Your question opens in Google AI Mode in your browser, \
             only when you send it.",
        ));
    } else if state.choice.local() {
        col = col.push(caption("This runs on your PC. No API key needed."));
    } else if state.choice.provider().is_some() {
        col = col.push(state.key.view().map(Message::ApiKey));
        col = col.push(caption(
            if *state.key.status() == KeyStatus::Saved {
                "Your key is kept in the system keyring, never in a file."
            } else {
                "Paste a key now, or add it later in Settings. AI actions stay off until a key is saved."
            },
        ));
    }
    col.into()
}

fn look(state: &State) -> Element<'_, Message> {
    column![
        title("Choose a look"),
        body("System follows your OS. You can change it later in Settings."),
        segmented(
            &[
                (Theme::System, "System", "Follow the system theme"),
                (Theme::Light, "Light", "Use the light theme"),
                (Theme::Dark, "Dark", "Use the dark theme"),
            ],
            state.theme,
            Message::Theme,
        ),
    ]
    .spacing(20)
    .into()
}

fn done(state: &State) -> Element<'_, Message> {
    let ai = match state.choice {
        Choice::Skip if state.base.ai.provider != Provider::Browser => {
            "AI: not set up yet".to_string()
        }
        Choice::Skip | Choice::Browser => "AI: Google AI Mode in your browser".to_string(),
        c if c.local() => format!("AI: {c}"),
        c if *state.key.status() == KeyStatus::Saved => format!("AI: {c}, key saved"),
        c => format!("AI: {c}, no key yet"),
    };
    column![
        title("You're all set"),
        body("OpenCircle keeps running in the tray. Press the shortcut to start:"),
        keys(&state.base.shortcut),
        caption(ai),
        caption(format!(
            "Theme: {}",
            match state.theme {
                Theme::System => "System",
                Theme::Light => "Light",
                Theme::Dark => "Dark",
            }
        )),
        caption("Everything can be changed in Settings, from the tray icon."),
    ]
    .spacing(14)
    .into()
}

fn footer(state: &State) -> Element<'_, Message> {
    let n = STEPS.iter().position(|s| *s == state.step).unwrap_or(0);
    let left: Element<'_, Message> = match &state.error {
        Some(e) => error_text(e.as_str()).size(13).into(),
        None => caption(format!("Step {} of {}", n + 1, STEPS.len()))
            .size(13)
            .into(),
    };
    let back = label_button("Back", false, (n > 0).then_some(Message::Back));
    let mut buttons = row![tip(back, "Go back", Position::Top)].spacing(8);
    if state.step == Step::Ai {
        let skip = label_button("Skip for now", false, Some(Message::SkipAi));
        buttons = buttons.push(tip(skip, "Skip AI setup", Position::Top));
    }
    buttons = buttons.push(if state.step == Step::Done {
        tip(
            label_button("Finish", true, Some(Message::Finish)),
            "Finish setup",
            Position::Top,
        )
    } else {
        tip(
            label_button("Next", true, Some(Message::Next)),
            "Continue",
            Position::Top,
        )
    });
    row![left, space::horizontal(), buttons]
        .align_y(iced::Center)
        .padding([14, 32])
        .into()
}

#[cfg(test)]
mod tests {
    use super::super::settings::tests::{MemoryStore, tmp};
    use super::*;
    use crate::secrets::Secret;
    use crate::ui::widgets::{TIPS, shot};

    const FAKE_KEY: &str = "sk-ant-FAKE-9876543210";

    fn state(path: Option<PathBuf>) -> State {
        State::new(Config::default(), path, Box::new(MemoryStore::default()))
    }

    fn next(s: &mut State, times: usize) {
        for _ in 0..times {
            update(s, Message::Next);
        }
    }

    #[test]
    fn the_browser_is_preselected_and_steps_stay_in_range() {
        let mut s = state(None);
        assert_eq!(s.choice, Choice::Browser);
        assert_eq!(CHOICES[0], Choice::Browser, "listed first");
        assert_eq!(s.config().ai, Config::default().ai);
        assert_eq!(s.config().ai.provider, Provider::Browser);
        update(&mut s, Message::Back);
        assert_eq!(s.step(), Step::Welcome);
        next(&mut s, 9);
        assert_eq!(s.step(), Step::Done);
        update(&mut s, Message::Back);
        assert_eq!(s.step(), Step::Look);
    }

    #[test]
    fn finish_writes_the_picked_config() {
        let path = tmp("onboarding-claude");
        let mut s = state(Some(path.clone()));
        next(&mut s, 1);
        update(&mut s, Message::Choose(Choice::Claude));
        update(
            &mut s,
            Message::ApiKey(KeyMessage::Input(Secret::new(FAKE_KEY))),
        );
        update(&mut s, Message::ApiKey(KeyMessage::Save));
        next(&mut s, 1);
        update(&mut s, Message::Theme(Theme::Light));
        next(&mut s, 1);
        assert_eq!(s.step(), Step::Done);
        let Outcome::Finished(cfg) = update(&mut s, Message::Finish) else {
            panic!("{:?}", s.error)
        };
        let (back, warnings) = Config::load_from(&path);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back, cfg);
        let mut want = Config {
            theme: Theme::Light,
            ..Config::default()
        };
        want.ai.provider = Provider::Anthropic;
        assert_eq!(back, want);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("FAKE"), "the key stays out of the file");
        theme::apply(Theme::Dark);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_local_preset_and_skip() {
        let path = tmp("onboarding-ollama");
        let mut s = state(Some(path.clone()));
        update(&mut s, Message::Choose(Choice::Preset(3)));
        assert!(s.choice.local());
        let Outcome::Finished(cfg) = update(&mut s, Message::Finish) else {
            panic!()
        };
        assert_eq!(cfg.ai.provider, Provider::OpenaiCompatible);
        assert_eq!(cfg.ai.base_url, "http://localhost:11434/v1");
        assert!(!cfg.ai.model.is_empty());
        // "Skip for now" moves on and leaves the AI settings as they were.
        let mut s = state(Some(path.clone()));
        next(&mut s, 1);
        update(&mut s, Message::SkipAi);
        assert_eq!(s.step(), Step::Look);
        assert_eq!(s.config().ai, Config::default().ai);
        theme::apply(Theme::Dark);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_unwritable_path_is_reported_not_finished() {
        let dir = tmp("onboarding-blocked");
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, "a file where a folder should be").unwrap();
        let mut s = state(Some(dir.join("config.toml")));
        assert_eq!(update(&mut s, Message::Finish), Outcome::None);
        assert!(s.error.as_deref().unwrap().contains("Could not save"));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn the_key_never_shows_up() {
        let mut s = state(None);
        let typing = Message::ApiKey(KeyMessage::Input(Secret::new(FAKE_KEY)));
        assert!(!format!("{typing:?}").contains("FAKE"));
        next(&mut s, 1);
        update(&mut s, Message::Choose(Choice::Claude));
        update(&mut s, typing);
        update(&mut s, Message::ApiKey(KeyMessage::Save));
        assert_eq!(*s.key.status(), KeyStatus::Saved);
        for step in STEPS {
            s.step = step;
            let texts = shot::texts(view(&s));
            assert!(
                !texts.iter().any(|t| t.contains("FAKE")),
                "{step:?}: {texts:?}"
            );
            assert!(!format!("{s:?}").contains("FAKE"));
        }
    }

    #[test]
    fn every_control_has_its_own_label_and_each_step_renders() {
        for (name, choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
            let mut s = state(None);
            update(&mut s, Message::Theme(choice));
            for (i, step) in STEPS.into_iter().enumerate() {
                s.step = step;
                TIPS.with(|t| t.borrow_mut().clear());
                let px = shot::save(
                    &format!("onboarding-{}-{name}", i + 1),
                    view(&s),
                    WINDOW_SIZE,
                );
                let tips = TIPS.with(|t| t.borrow().clone());
                let mut unique = tips.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(tips.len(), unique.len(), "{step:?}: {tips:?}");
                let window = theme::current().window;
                let bg = [window.r, window.g, window.b].map(|c| (c * 255.0).round() as u8);
                let other = px
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[..3] != bg)
                    .count();
                assert!(other > 3_000, "{step:?} looks empty ({other})");
            }
        }
        theme::apply(Theme::Dark);
    }
}
