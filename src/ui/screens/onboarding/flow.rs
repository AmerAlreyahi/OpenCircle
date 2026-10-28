//! Onboarding's state and logic (`State`, `update`, the `STEPS` list) and its
//! shell: the header, the step track (its thumb glides; the step itself switches at once: its rise redrew the whole window each frame on the CPU renderer, it returns with the GPU renderer, 0.1.3),
//! the step and the footer (Back, Skip on the AI step, Next or Finish).
//!
//! ```ignore
//! let mut state = onboarding::State::open();           // loads the config, real keyring
//! // window size: onboarding::WINDOW_SIZE (fixed: every step fits, nothing scrolls)
//! // update: if let onboarding::Outcome::Finished(cfg) = onboarding::update(&mut state, msg) { ...close, start... }
//! // view: onboarding::view(&state)
//! ```
use std::fmt;
use std::path::PathBuf;
use std::time::Instant;

use iced::widget::{Space, column, container, row};
use iced::{Center, Element, Fill, Padding, Size, Subscription};

use super::steps;
use super::steps::screenshots::{Access, ask};
use crate::config::{AiConfig, Config};
use crate::secrets::{KeyStore, Keyring};
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::divider::divider;
use crate::ui::kit::header::{Caption, regular};
use crate::ui::kit::motion::{self, Curve};
use crate::ui::kit::segmented::spread;
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::error;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::screens::settings::{self, KeyJob};

/// The design's fixed size, the same for every step.
pub const WINDOW_SIZE: Size = Size::new(680.0, 525.0);

/// The window's 1 px rim.
const RIM: f32 = 1.0;
/// Round the track, step and footer (`px-5 pt-2 pb-4`), and between them (`gap-3.5`).
const PAD: Padding = Padding {
    top: 8.0,
    right: 20.0,
    bottom: 16.0,
    left: 20.0,
};
const GAP: f32 = 14.0;
/// Over the footer's buttons, under its hairline (`pt-2.5`).
const FOOTER_TOP: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    /// Wayland only: the desktop's screenshot permission, asked while we have the focus.
    Screenshots,
    Ai,
    Look,
    Done,
}

/// One step: its track label, its view, and whether only Wayland shows it.
pub struct Page {
    pub id: Step,
    pub label: &'static str,
    pub wayland: bool,
    pub view: for<'a> fn(&'a State) -> Element<'a, Message>,
}

const fn page(
    id: Step,
    label: &'static str,
    view: for<'a> fn(&'a State) -> Element<'a, Message>,
) -> Page {
    Page {
        id,
        label,
        wayland: false,
        view,
    }
}

/// In the design's order. A new step = one entry here + one view file.
pub const STEPS: [Page; 5] = [
    page(Step::Welcome, "Welcome", steps::welcome::view),
    Page {
        wayland: true,
        ..page(Step::Screenshots, "Screenshots", steps::screenshots::view)
    },
    page(Step::Ai, "Connect AI", steps::ai::view),
    page(Step::Look, "Look", steps::look::view),
    page(Step::Done, "Done", steps::done::view),
];

pub struct State {
    /// The Settings state whose rows the steps show: its draft is what Finish writes.
    pub(super) settings: settings::State,
    pub(super) step: Step,
    /// The AI settings as loaded (Skip puts them back).
    base_ai: AiConfig,
    /// `None` = the real config path.
    path: Option<PathBuf>,
    /// Wayland: the Screenshots step is shown.
    pub(super) wayland: bool,
    pub(super) access: Access,
    pub(super) error: Option<String>,
    /// The last step change (the step it left, and when): the thumb glides.
    moved: Option<(Step, Instant)>,
    /// The track's labels, numbered ("1. Welcome").
    labels: Vec<(Step, String)>,
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("onboarding::State")
            .field("step", &self.step)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A pill of the step track.
    Go(Step),
    Next,
    Back,
    /// "Skip" on the AI step: the AI settings stay as they were.
    Skip,
    Finish,
    /// The Allow button.
    Allow,
    /// The desktop's answer to the screenshot request.
    Allowed(Result<(), crate::capture::CaptureError>),
    /// A Settings row (the same messages as in Settings).
    Settings(settings::Message),
    /// The header: drag, minimise, close (the shell does them).
    Caption(Caption),
    Tick,
}

// One short-lived value per message: a boxed Config would buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    None,
    /// The config was written; close the window and carry on.
    Finished(Config),
    /// Send one screenshot request now (this window has the focus); answer with
    /// `Message::Allowed`.
    AskScreenshot,
}

impl State {
    /// The real config file and the real keyring. Google AI Mode is preselected.
    pub fn open() -> Self {
        let (config, _warnings) = Config::load();
        #[cfg(target_os = "linux")]
        let wayland = crate::sys::session::wayland_session();
        #[cfg(not(target_os = "linux"))]
        let wayland = false;
        Self::new(config, None, Box::new(Keyring)).on_wayland(wayland)
    }

    /// Updates and starting with the OS are pre-set on (owner: opt-in asked
    /// on first run, pre-set on).
    pub fn new(mut base: Config, path: Option<PathBuf>, store: Box<dyn KeyStore>) -> Self {
        base.update.auto.get_or_insert(true);
        base.general.autostart = true;
        let base_ai = base.ai.clone();
        let mut s = State {
            settings: settings::State::new(base, Vec::new(), path.clone(), store),
            step: Step::Welcome,
            base_ai,
            path,
            wayland: false,
            access: Access::Unasked,
            error: None,
            moved: None,
            labels: Vec::new(),
        };
        s.number();
        s
    }

    pub(super) fn on_wayland(mut self, wayland: bool) -> Self {
        self.wayland = wayland;
        self.number();
        self
    }

    fn number(&mut self) {
        self.labels = (self.steps().enumerate())
            .map(|(i, p)| (p.id, format!("{}. {}", i + 1, p.label)))
            .collect();
    }

    /// The keyring calls the last message started: the shell runs them off the UI
    /// thread and sends each answer back as `Message::Settings(ApiKey(Done))`.
    pub fn take_key_jobs(&mut self) -> Vec<KeyJob> {
        let jobs = self.settings.take_key_jobs();
        jobs.into_iter().map(|(_, job)| job).collect()
    }

    pub(super) fn steps(&self) -> impl Iterator<Item = &'static Page> {
        let wayland = self.wayland;
        STEPS.iter().filter(move |p| wayland || !p.wayland)
    }

    #[cfg(test)]
    pub fn step(&self) -> Step {
        self.step
    }

    /// The config Finish writes.
    pub fn config(&self) -> &Config {
        self.settings.draft()
    }

    fn at(&self) -> usize {
        self.steps().position(|p| p.id == self.step).unwrap_or(0)
    }

    fn go(&mut self, by: isize) {
        let shown: Vec<Step> = self.steps().map(|p| p.id).collect();
        let i = (self.at() as isize + by).clamp(0, shown.len() as isize - 1);
        self.go_to(shown[i as usize]);
    }

    fn go_to(&mut self, step: Step) {
        if step != self.step {
            self.moved = Some((self.step, Instant::now()));
            self.step = step;
        }
    }

    /// A step change under way: (the step it left, the glide's progress);
    /// `None` at rest.
    fn moving(&self) -> Option<(Step, f32)> {
        let (from, t) = self.moved?;
        Some((
            from,
            motion::progress(t, Instant::now(), motion::GLIDE, Curve::Standard),
        ))
    }
}

pub fn update(state: &mut State, message: Message) -> Outcome {
    if !matches!(
        message,
        Message::Tick | Message::Settings(settings::Message::Tick)
    ) {
        state.error = None;
    }
    match message {
        Message::Go(step) => state.go_to(step),
        Message::Next => state.go(1),
        Message::Back => state.go(-1),
        Message::Skip => {
            state.settings.set_ai(state.base_ai.clone());
            state.go(1);
        }
        // Settings with auto-save off: its rows edit the draft only.
        Message::Settings(m) => {
            let _ = settings::update(&mut state.settings, m);
        }
        Message::Caption(_) => {}
        Message::Tick => {
            if state
                .moved
                .is_some_and(|(_, t)| t.elapsed() >= motion::GLIDE)
            {
                state.moved = None;
            }
        }
        Message::Finish => return finish(state),
        Message::Allow | Message::Allowed(_) => return ask(&mut state.access, message),
    }
    Outcome::None
}

fn finish(state: &mut State) -> Outcome {
    if let Some(p) = state.settings.problems().first() {
        let why = p.split_once(": ").map_or(p.as_str(), |(_, why)| why);
        state.error = Some(format!("Fix this first: {why}"));
        return Outcome::None;
    }
    let config = state.config().clone();
    match crate::config::update_at(state.path.as_deref(), |c| *c = config.clone()) {
        Ok(_) => Outcome::Finished(config),
        Err(e) => {
            state.error = Some(format!("Could not save your settings: {e}"));
            Outcome::None
        }
    }
}

/// The frame clock while the track glides (or a row's thumb does).
pub fn subscription(state: &State) -> Subscription<Message> {
    let own = match state.moved {
        Some(_) => motion::frames().map(|_| Message::Tick),
        None => Subscription::none(),
    };
    Subscription::batch([
        own,
        settings::subscription(&state.settings).map(Message::Settings),
    ])
}

pub fn view(state: &State) -> Element<'_, Message> {
    let logo = crate::ui::kit::logo::logo(16);
    let header = regular(
        logo,
        "Welcome to OpenCircle",
        Space::new(),
        false,
        Message::Caption,
    );
    let (from, glide) = match state.moving() {
        Some((from, glide)) => (from, glide),
        None => (state.step, 1.0),
    };
    let options: Vec<(Step, &str)> = (state.labels.iter())
        .map(|(s, l)| (*s, l.as_str()))
        .collect();
    let across = WINDOW_SIZE.width - 2.0 * RIM - PAD.left - PAD.right;
    let track = spread(across, &options, state.step, from, glide, Message::Go);
    let page = STEPS
        .iter()
        .find(|p| p.id == state.step)
        .expect("every step has a page");
    // fixed size, nothing scrolls: the design's `overflow-hidden`
    let body = container(column![(page.view)(state)])
        .height(Fill)
        .clip(true);
    let content = column![track, body, footer(state)]
        .spacing(GAP)
        .padding(PAD)
        .height(Fill);
    surface(Material::Window, 0.0, column![header, content])
        .padding(RIM)
        .width(Fill)
        .height(Fill)
        .into()
}

/// Back (off on the first step), what went wrong, then Skip (the AI step)
/// and Next, or Finish on the last step.
fn footer(state: &State) -> Element<'_, Message> {
    let first = state.at() == 0;
    let last = state.at() + 1 == state.steps().count();
    let back = button(
        label("← Back"),
        Kind::Elevated,
        (!first).then_some(Message::Back),
    );
    let mut line = row![back].spacing(space::GAP).align_y(Center);
    if let Some(e) = &state.error {
        line = line.push(error(Role::ROW_DESC, e.as_str()));
    }
    line = line.push(Space::new().width(Fill));
    if state.step == Step::Ai {
        line = line.push(button(label("Skip"), Kind::Quiet, Some(Message::Skip)));
    }
    line = line.push(match last {
        true => button(label("Finish"), Kind::Elevated, Some(Message::Finish)),
        false => button(label("Next →"), Kind::Elevated, Some(Message::Next)),
    });
    column![divider(), Space::new().height(FOOTER_TOP), line].into()
}
