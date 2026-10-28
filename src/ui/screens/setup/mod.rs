//! The setup window (`crate::setup`): the new design's installer
//! (`newdesing/widgets/installer.js`, theme Sapphire, solid look). One card:
//! Install (folder under "Customize install") → progress → Launch, with "Start
//! with Windows" on the last step. Composed from kit parts; the blurs the CPU
//! renderer can't draw (the emblem's halo, the buttons' glow) are baked
//! images (`scripts/installer-art.py`). Every string is in `EN` (one table to
//! translate; rows use Fill spacers, no fixed x, so they can mirror for Arabic).
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use iced::widget::image::Handle;
use iced::widget::text::LineHeight;
use iced::widget::{Space, column, container, image, row, stack, text};
use iced::window;
use iced::{Center, Element, Fill, Padding, Size, Subscription, Task};

use crate::setup::msi::{self, Package};
use crate::setup::{self as setup_core, BadFolder, Installed};
use crate::ui::kit::button::{self, Kind, cta_label};
use crate::ui::kit::divider::divider;
use crate::ui::kit::field::field;
use crate::ui::kit::motion::{self, Curve};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text as ktext;
use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
use crate::ui::kit::tokens::fixed::{DANGER, ON_ACCENT, SUCCESS};
use crate::ui::kit::tokens::{Role, radius, space};
use crate::ui::kit::{chip, header, progress, segmented};
use crate::ui::widgets::{self, chrome::png_handle};
use crate::ui::windows;

/// The window: 420 logical px wide (`max-w-[420px]`), its height per step.
pub const WIDTH: f32 = 420.0;
/// The window's corner radius (`rounded-2xl`).
const RADIUS: f32 = radius::WINDOW;
/// How far a new step's body rises in (`animatePageSwitch`: 14 px, 190 ms expo-out).
const RISE: f32 = 14.0;
/// The logo's pulse while installing (`animate-pulse`: 2 s, opacity 1 ↔ .5).
const PULSE: f32 = 2.0;

// The layout (installer.js), in logical px.
/// The window's rim inside its edge.
const RIM: f32 = 1.0;
/// The backdrop art, and the glow under the call to action (`GLOW_TOP` down).
const BACK: Size = Size::new(256.0, 210.0);
const GLOW: Size = Size::new(418.0, 128.0);
const GLOW_TOP: f32 = 185.0;
/// The body's room above (under the header) and at each side.
const BODY_TOP: f32 = 8.0;
const BODY_X: f32 = 24.0;
/// The emblem: the logo on its tile (radius 20), 4 px of room round it for the check.
const LOGO: f32 = 46.0;
const TILE: f32 = 68.0;
const TILE_RADIUS: f32 = 20.0;
const CHECK_ROOM: f32 = 4.0;
/// The title row, and the gap between its parts (and a CTA's label and arrow).
const TITLE_H: f32 = 28.0;
const TITLE_GAP: f32 = 8.0;
/// The intro under the title: its box and its line height.
const INTRO: Size = Size::new(300.0, 39.0);
const INTRO_LINE: f32 = 19.5;
/// The gaps down the card.
const BELOW_EMBLEM: f32 = 12.0;
const BELOW_TITLE: f32 = 4.0;
const BELOW_INTRO: f32 = 24.0;
const BELOW_CTA: f32 = 14.0;
const BELOW_DIVIDER: f32 = 12.0;
const BELOW_PANEL: f32 = 12.0;
const ABOVE_BAR: f32 = 4.0;
/// Rows: under the CTA, the progress's stage line, the path line, the folder
/// row, why a folder is refused, why the install failed.
const BELOW_H: f32 = 17.0;
const STAGE_H: f32 = 16.0;
const PATH_H: f32 = 33.0;
const FOLDER_H: f32 = 26.0;
const ERROR_H: f32 = 16.0;
const FAILED_H: f32 = 54.0;
/// A line's room at its ends (the status, the path).
const LINE_X: f32 = 2.0;
/// The recessed panel's room inside (less below: the progress glow takes 6).
const PANEL_PAD: f32 = 14.0;
const PANEL_BOTTOM: f32 = 8.0;
/// The small text: status, path, "Try again", why a folder is refused; the chevron.
const META: f32 = 11.0;
const CHEVRON: f32 = 10.0;

/// Every user-visible string (English; an Arabic table goes beside it).
struct Strings {
    window_title: &'static str,
    title: &'static str,
    ready: &'static str,
    intro: &'static str,
    /// `{}` = the capture shortcut.
    installed: &'static str,
    install: &'static str,
    update: &'static str,
    repair: &'static str,
    customize: &'static str,
    for_everyone: &'static str,
    /// `{}` = the installed version.
    replaces: &'static str,
    same: &'static str,
    folder: &'static str,
    browse: &'static str,
    preparing: &'static str,
    copying: &'static str,
    finishing: &'static str,
    try_again: &'static str,
    launch: &'static str,
    starting: &'static str,
    start_with_windows: &'static str,
    off: &'static str,
    on: &'static str,
    pick_title: &'static str,
    uninstall_title: &'static str,
    uninstall_intro: &'static str,
    uninstall: &'static str,
    your_data: &'static str,
    keep: &'static str,
    remove: &'static str,
    removing: &'static str,
    removed: &'static str,
    removed_kept: &'static str,
    removed_all: &'static str,
    close: &'static str,
    no_folder: &'static str,
    not_local: &'static str,
    drive_root: &'static str,
    bad_character: &'static str,
    reserved: &'static str,
    too_long: &'static str,
}

const EN: Strings = Strings {
    window_title: "OpenCircle Setup",
    title: "OpenCircle",
    ready: "OpenCircle is ready",
    intro: "Circle anything on your screen to search, read text, translate in place, or ask AI.",
    installed: "Installed cleanly. Press {} anytime to circle your screen.",
    install: "Install OpenCircle",
    update: "Update OpenCircle",
    repair: "Repair OpenCircle",
    customize: "Customize install",
    for_everyone: "For everyone on this PC",
    replaces: "Updates v{} · keeps your settings",
    same: "Already installed · repairs it",
    folder: "Folder",
    browse: "…",
    preparing: "Preparing…",
    copying: "Installing OpenCircle to your PC…",
    finishing: "Finishing…",
    try_again: "Try again",
    launch: "Launch OpenCircle",
    starting: "Starting OpenCircle…",
    start_with_windows: "Start with Windows",
    off: "Off",
    on: "On",
    pick_title: "Choose where to install OpenCircle",
    uninstall_title: "Uninstall OpenCircle",
    uninstall_intro: "Removes OpenCircle from this PC. Your settings can stay for a later install.",
    uninstall: "Uninstall",
    your_data: "Settings, models and keys",
    keep: "Keep",
    remove: "Remove",
    removing: "Removing OpenCircle…",
    removed: "OpenCircle was removed",
    removed_kept: "Your settings, models and keys are kept for a later install.",
    removed_all: "Your settings, models and keys were removed too.",
    close: "Close",
    no_folder: "Choose a folder.",
    not_local: r"Use a folder on this PC, like C:\Program Files\OpenCircle.",
    drive_root: "Choose a folder, not a whole drive.",
    bad_character: "A folder name can't contain < > : \" | ? or *.",
    reserved: "Windows reserves that name. Choose another.",
    too_long: "That path is too long. Choose a shorter one.",
};

fn fill(template: &str, value: &str) -> String {
    template.replacen("{}", value, 1)
}

fn why(bad: BadFolder) -> &'static str {
    match bad {
        BadFolder::Empty => EN.no_folder,
        BadFolder::NotLocal => EN.not_local,
        BadFolder::DriveRoot => EN.drive_root,
        BadFolder::BadCharacter => EN.bad_character,
        BadFolder::ReservedName => EN.reserved,
        BadFolder::TooLong => EN.too_long,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Step {
    Configure,
    Installing,
    Finish,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    Customize,
    Folder(String),
    Browse,
    Picked(Option<PathBuf>),
    Install,
    Msi(msi::Event),
    Launch,
    Autostart(bool),
    /// The uninstall's Keep (false) / Remove (true) for the user's data.
    RemoveData(bool),
    Retry,
    Close,
    /// The last step's work is done (`State::leave`): close, or why the app
    /// did not start.
    Left(Result<(), String>),
    Drag,
    Frame(Instant),
    /// The OS's light / dark mode (Daylight / Sapphire).
    Os(iced::theme::Mode),
}

pub struct State {
    window: Option<window::Id>,
    step: Step,
    /// "Customize install" is open.
    open: bool,
    folder: String,
    bad: Option<BadFolder>,
    installed: Option<Installed>,
    /// " · 19 MB": what the app takes on disk.
    size: Option<String>,
    autostart: bool,
    /// The switch's thumb glides from this value since then (`motion::GLIDE`).
    autostart_glide: (bool, Instant),
    /// The window's height: from, to, since (200 ms ease, the design's
    /// `transition-all`), and the height last given to the window.
    height: (f32, f32, Instant),
    applied: f32,
    /// The capture shortcut as the finish text names it.
    shortcut: String,
    file_name: String,
    /// This setup's version (`X.Y.Z`).
    version: &'static str,
    /// The progress bar: where it moves from and to, since when (200 ms ease).
    bar: (f32, f32, Instant),
    /// When the current step's body began to rise in.
    risen: Instant,
    now: Instant,
    package: Option<Arc<Package>>,
    /// Launch or close was chosen on the last step: on its way out.
    leaving: bool,
    /// Why Launch did not start the app (shown in place of the finish text).
    launch_error: Option<String>,
    /// The last step's work (`finish`; tests put a stand-in here).
    finish: fn(&str, bool, bool, bool) -> Result<(), String>,
    /// The uninstall window: the product code it removes.
    uninstall: Option<String>,
    /// The uninstall removes the user's data too (the Keep / Remove switch),
    /// and its thumb glides from this value since then.
    remove_data: bool,
    data_glide: (bool, Instant),
}

/// The setup's whole program: the window until it closes.
pub fn run(msi_bytes: &'static [u8]) -> iced::Result {
    let started = std::time::Instant::now();
    let package = match Package::write(msi_bytes) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            crate::log!("[opencircle] setup: cannot unpack the installer: {e}");
            return Ok(());
        }
    };
    let installed = msi::installed(&package);
    let read = started.elapsed();
    let size = msi::installed_size(&package);
    crate::log!(
        "[opencircle] setup: unpacked + read the installed copy in {} ms, sized in {} ms",
        read.as_millis(),
        (started.elapsed() - read).as_millis()
    );
    let file_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "OpenCircle-Setup.exe".into());
    let (config, warnings) = crate::config::Config::peek();
    let shown = Config {
        shortcut: config.shortcut,
        autostart: !warnings.is_empty() || config.general.autostart,
    };
    show(move || {
        State::new(
            installed.clone(),
            size,
            file_name.clone(),
            shown.clone(),
            Some(package.clone()),
        )
    })
}

/// The uninstall window (`setup::uninstall_ui`): removes the installed product
/// `code` from `folder`, and the user's data too if they choose so.
pub fn run_uninstall(code: String, folder: String) -> iced::Result {
    let (config, _) = crate::config::Config::peek();
    let shown = Config {
        shortcut: config.shortcut,
        autostart: false,
    };
    show(move || {
        let mut state = State::new(None, None, "opencircle.exe".into(), shown.clone(), None);
        state.folder = folder.clone();
        state.uninstall = Some(code.clone());
        state
    })
}

/// The setup's window with the state `make` builds, until it closes.
fn show(make: impl Fn() -> State + 'static) -> iced::Result {
    let boot = move || {
        let mut state = make();
        let (id, open) = windows::setup_window(Size::new(WIDTH, state.height()), RADIUS);
        state.window = Some(id);
        let h = state.height();
        state.height = (h, h, state.now);
        state.applied = h;
        // A setup is what the user just started: in front, with the focus (a window
        // opened behind another, e.g. from a script, was not drawn until activated).
        let front = window::gain_focus(id);
        let drawn = Task::perform(async {}, |()| Message::Frame(Instant::now()));
        let os = iced::system::theme().map(Message::Os);
        (
            state,
            Task::batch([os, open.discard().chain(front).chain(drawn)]),
        )
    };
    let mut app = iced::daemon(boot, State::update, State::view);
    for font in widgets::FONTS {
        app = app.font(font);
    }
    app.default_font(widgets::ui_font())
        .title(|s: &State, _| match s.uninstall {
            Some(_) => EN.uninstall_title.to_string(),
            None => EN.window_title.to_string(),
        })
        .theme(|_: &State, _| Some(theme()))
        .subscription(State::subscription)
        .run()
}

/// The parts of the user's config the setup shows.
#[derive(Debug, Clone)]
struct Config {
    shortcut: String,
    autostart: bool,
}

/// iced's theme for the window: the token set's colours (Sapphire or Daylight).
fn theme() -> iced::Theme {
    let tokens = themes::c();
    let mut palette = iced::Theme::Dark.palette();
    palette.background = tokens.bg;
    palette.text = tokens.text;
    palette.primary = tokens.accent;
    iced::Theme::custom("setup", palette)
}

/// What the OS reports: light = Daylight, dark (or no preference) = Sapphire.
fn set_light(light: bool) {
    themes::set(if light { &DAYLIGHT } else { &SAPPHIRE });
}

/// A bundled PNG, decoded once per call site (`LazyLock`).
macro_rules! bundled {
    ($name:ident, $file:literal) => {
        fn $name() -> Handle {
            static HANDLE: LazyLock<Handle> = LazyLock::new(|| {
                png_handle(include_bytes!(concat!("../../../../assets/setup/", $file)))
            });
            HANDLE.clone()
        }
    };
}
bundled!(logo, "logo.png");
bundled!(logo_light, "logo-light.png");
bundled!(back, "back.png");
bundled!(glow_accent, "glow-accent.png");
bundled!(glow_success, "glow-success.png");

impl State {
    fn new(
        installed: Option<Installed>,
        size: Option<u64>,
        file_name: String,
        config: Config,
        package: Option<Arc<Package>>,
    ) -> State {
        let folder = installed
            .as_ref()
            .and_then(|i| i.folder.as_deref())
            .and_then(|f| setup_core::check_folder(f).ok())
            .unwrap_or_else(setup_core::default_folder);
        let now = Instant::now();
        State {
            window: None,
            step: Step::Configure,
            open: false,
            bad: setup_core::check_folder(&folder).err(),
            folder,
            installed,
            size: size.map(|b| format!("· {} MB", (b + (1 << 19)) >> 20)),
            autostart: config.autostart,
            shortcut: super::settings::key_labels(&config.shortcut).join(" + "),
            file_name,
            version: env!("CARGO_PKG_VERSION"),
            bar: (0.0, 0.0, now),
            autostart_glide: (config.autostart, now - motion::GLIDE),
            height: (0.0, 0.0, now - motion::GLIDE),
            applied: 0.0,
            risen: now - motion::MORPH,
            now,
            package,
            leaving: false,
            launch_error: None,
            finish,
            uninstall: None,
            remove_data: false,
            data_glide: (false, now - motion::GLIDE),
        }
    }

    /// The window's height in this state (the design's measured card heights).
    fn height(&self) -> f32 {
        if self.uninstall.is_some() {
            return match &self.step {
                Step::Configure => 359.0,
                Step::Installing => 360.0,
                Step::Failed(_) => 376.0,
                Step::Finish => 330.0,
            };
        }
        match &self.step {
            Step::Configure if self.open => 367.0 + if self.bad.is_some() { 20.0 } else { 0.0 },
            Step::Configure => 328.0,
            Step::Installing => 360.0,
            Step::Failed(_) => 376.0,
            Step::Finish => 359.0,
        }
    }

    /// Where the folder can't change: an update or repair goes where the app is.
    fn folder_locked(&self) -> bool {
        self.installed.as_ref().is_some_and(|i| i.folder.is_some())
    }

    fn go(&mut self, step: Step) -> Task<Message> {
        self.step = step;
        self.risen = Instant::now();
        self.now = self.risen;
        self.resize()
    }

    /// The window's height glides to this state's (`height_at`, by `Frame`).
    fn resize(&mut self) -> Task<Message> {
        let to = self.height();
        if to != self.height.1 {
            let now = Instant::now();
            self.now = now;
            self.height = (self.height_at(), to, now);
        }
        self.apply_height()
    }

    fn height_at(&self) -> f32 {
        let (from, to, since) = self.height;
        from + (to - from) * motion::progress(since, self.now, motion::GLIDE, Curve::Ease)
    }

    /// Gives the window its height of this frame, when it changed.
    fn apply_height(&mut self) -> Task<Message> {
        let h = self.height_at().round();
        let Some(id) = self.window.filter(|_| h != self.applied) else {
            return Task::none();
        };
        self.applied = h;
        window::resize(id, Size::new(WIDTH, h)).chain(windows::reshape::<()>(id).discard())
    }

    fn bar_at(&self) -> f32 {
        let (from, to, since) = self.bar;
        from + (to - from) * motion::progress(since, self.now, motion::GLIDE, Curve::Ease)
    }

    fn animating(&self) -> bool {
        self.step == Step::Installing
            || self.bar_at() != self.bar.1
            || self.height_at() != self.height.1
            || motion::done(self.autostart_glide.1, self.now, motion::GLIDE) < 1.0
            || motion::done(self.data_glide.1, self.now, motion::GLIDE) < 1.0
            || motion::done(self.risen, self.now, motion::MORPH) < 1.0
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Frame(now) => {
                self.now = now;
                self.apply_height()
            }
            Message::Os(mode) => {
                set_light(mode == iced::theme::Mode::Light);
                Task::none()
            }
            Message::Drag => self.window.map_or(Task::none(), window::drag),
            Message::Customize => {
                self.open = !self.open;
                self.resize()
            }
            Message::Folder(folder) => {
                self.bad = setup_core::check_folder(&folder).err();
                self.folder = folder;
                self.resize()
            }
            Message::Browse => {
                let start = PathBuf::from(&self.folder);
                crate::ui::app::background::background(move |tx| {
                    let mut dialog = rfd::FileDialog::new().set_title(EN.pick_title);
                    if let Some(parent) = start.parent().filter(|p| p.is_dir()) {
                        dialog = dialog.set_directory(parent);
                    }
                    let _ = tx.unbounded_send(dialog.pick_folder());
                })
                .map(Message::Picked)
            }
            Message::Picked(None) => Task::none(),
            Message::Picked(Some(picked)) => self.update(Message::Folder(chosen(&picked))),
            Message::Install if self.uninstall.is_some() => {
                let code = self.uninstall.clone().unwrap_or_default();
                let remove_data = self.remove_data;
                self.bar = (0.0, 0.0, Instant::now());
                let steps = self.go(Step::Installing);
                let work = crate::ui::app::background::background(move |tx| {
                    msi::uninstall(&code, &|event| {
                        if remove_data && event == msi::Event::Done(Ok(())) {
                            setup_core::remove_user_data();
                        }
                        let _ = tx.unbounded_send(event);
                    });
                });
                steps.chain(work.map(Message::Msi))
            }
            Message::RemoveData(on) => {
                if on != self.remove_data {
                    self.now = Instant::now();
                    self.data_glide = (self.remove_data, self.now);
                    self.remove_data = on;
                }
                Task::none()
            }
            Message::Install => {
                let Ok(folder) = setup_core::check_folder(&self.folder) else {
                    self.open = true;
                    return self.resize();
                };
                let Some(package) = self.package.clone() else {
                    return Task::none();
                };
                self.folder = folder.clone();
                self.bar = (0.0, 0.0, Instant::now());
                let repair = self.installed.as_ref().is_some_and(|i| i.same);
                let steps = self.go(Step::Installing);
                let work = crate::ui::app::background::background(move |tx| {
                    setup_core::quit_running_app();
                    msi::install(&package, &folder, repair, &|event| {
                        let _ = tx.unbounded_send(event);
                    });
                });
                steps.chain(work.map(Message::Msi))
            }
            Message::Msi(msi::Event::Progress(share)) => {
                self.now = Instant::now();
                self.bar = (self.bar_at(), share, self.now);
                Task::none()
            }
            Message::Msi(msi::Event::Done(Ok(()))) => self.go(Step::Finish),
            Message::Msi(msi::Event::Done(Err(why))) => self.go(Step::Failed(why)),
            Message::Retry => self.go(Step::Configure),
            Message::Autostart(on) => {
                if on != self.autostart {
                    self.now = Instant::now();
                    self.autostart_glide = (self.autostart, self.now);
                    self.autostart = on;
                }
                Task::none()
            }
            Message::Launch => self.leave(true),
            Message::Close => match self.step {
                Step::Installing => Task::none(), // Windows Installer is working: wait
                Step::Finish if self.uninstall.is_some() => self.close(),
                Step::Finish => self.leave(false),
                _ => self.close(),
            },
            Message::Left(Ok(())) => self.close(),
            Message::Left(Err(why)) => {
                self.leaving = false;
                self.launch_error = Some(why);
                Task::none()
            }
        }
    }

    /// The last step's way out: the choice kept and (`launch`) the app started
    /// on a thread of their own (registry, config file, process start: the
    /// window keeps answering meanwhile), then the setup closes. Once only.
    fn leave(&mut self, launch: bool) -> Task<Message> {
        if self.leaving {
            return Task::none();
        }
        self.leaving = true;
        let (folder, autostart) = (self.folder.clone(), self.autostart);
        let updating = self.installed.is_some();
        self.launch_error = None;
        let work = self.finish;
        crate::ui::app::background::background(move |tx| {
            let _ = tx.unbounded_send(work(&folder, autostart, launch, updating));
        })
        .map(Message::Left)
    }

    fn close(&self) -> Task<Message> {
        match self.window {
            Some(id) => windows::close(id).chain(iced::exit()),
            None => iced::exit(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let closing = Subscription::batch([
            window::close_requests().map(|_| Message::Close),
            iced::system::theme_changes().map(Message::Os),
        ]);
        match self.animating() {
            true => Subscription::batch([closing, motion::frames().map(Message::Frame)]),
            false => closing,
        }
    }

    pub fn view(&self, _: window::Id) -> Element<'_, Message> {
        let rise = RISE * (1.0 - motion::progress(self.risen, self.now, motion::MORPH, Curve::Out));
        // the window's inside is 418 px (a 1 px rim): backdrop, glow, content
        let backdrop = column![
            Space::new().height(rise),
            container(image(back()).width(BACK.width).height(BACK.height)).center_x(Fill),
        ];
        let glow = match self.step {
            Step::Configure => Some(glow_accent()),
            Step::Finish => Some(glow_success()),
            _ => None,
        }
        .map(|g| {
            column![
                Space::new().height(GLOW_TOP + rise),
                image(g).width(GLOW.width).height(GLOW.height)
            ]
        });
        let header = header::compact(&self.file_name, Message::Drag, Message::Close);
        let body = container(self.body())
            .padding(Padding {
                top: BODY_TOP + rise,
                ..Padding::from([0.0, BODY_X])
            })
            .width(Fill);
        let mut layers = stack![backdrop].width(Fill).height(Fill);
        if let Some(glow) = glow {
            layers = layers.push(glow);
        }
        surface(
            Material::Window,
            radius::WINDOW,
            layers.push(column![header, body]),
        )
        .padding(RIM)
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn body(&self) -> Element<'_, Message> {
        let finished = self.step == Step::Finish;
        let uninstall = self.uninstall.is_some();
        let heading = match (uninstall, finished) {
            (true, true) => EN.removed,
            (true, false) => EN.uninstall_title,
            (false, true) => EN.ready,
            (false, false) => EN.title,
        };
        let title = row![
            ktext::text(Role::APP_NAME, heading),
            chip::chip(format!("v{}", self.version)),
        ]
        .spacing(TITLE_GAP)
        .align_y(Center)
        .height(TITLE_H);
        let intro = match (&self.launch_error, finished) {
            _ if uninstall && finished && self.remove_data => EN.removed_all.to_string(),
            _ if uninstall && finished => EN.removed_kept.to_string(),
            _ if uninstall => EN.uninstall_intro.to_string(),
            (Some(why), true) => why.clone(),
            (None, true) => fill(EN.installed, &self.shortcut),
            (_, false) => EN.intro.to_string(),
        };
        let intro = match self.launch_error.is_some() && finished {
            true => ktext::error(Role::NOTE, intro),
            false => ktext::sub(Role::NOTE, intro),
        };
        let intro = container(
            intro
                .line_height(LineHeight::Absolute(INTRO_LINE.into()))
                .align_x(Center)
                .width(Fill),
        )
        .width(INTRO.width)
        .height(INTRO.height);
        column![
            self.emblem(),
            Space::new().height(BELOW_EMBLEM),
            title,
            Space::new().height(BELOW_TITLE),
            intro,
            Space::new().height(BELOW_INTRO),
            self.action(),
        ]
        .align_x(Center)
        .width(Fill)
        .into()
    }

    /// The logo on its tile (68 px, radius 20), pulsing while installing, with
    /// the green check once done; 4 px of room round it for the check.
    fn emblem(&self) -> Element<'_, Message> {
        let pulse = match self.step {
            Step::Installing => {
                let t =
                    self.now.saturating_duration_since(self.risen).as_secs_f32() % PULSE / PULSE;
                let half = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
                1.0 - 0.5 * Curve::Pulse.ease(half)
            }
            _ => 1.0,
        };
        let logo = if themes::current().light {
            logo_light()
        } else {
            logo()
        };
        let tile = surface(
            Material::Tile,
            TILE_RADIUS,
            image(logo).width(LOGO).height(LOGO).opacity(pulse),
        )
        .width(TILE)
        .height(TILE)
        .padding((TILE - LOGO) / 2.0);
        let mut emblem = stack![container(tile).padding(CHECK_ROOM)]
            .width(TILE + 2.0 * CHECK_ROOM)
            .height(TILE + 2.0 * CHECK_ROOM);
        if self.step == Step::Finish {
            emblem = emblem.push(
                container(chip::done())
                    .width(Fill)
                    .height(Fill)
                    .align_right(Fill)
                    .align_bottom(Fill),
            );
        }
        emblem.into()
    }

    /// The step's main control and what is below it.
    fn action(&self) -> Element<'_, Message> {
        match &self.step {
            Step::Configure if self.uninstall.is_some() => {
                let data = row![
                    ktext::sub(Role::ROW_DESC, EN.your_data),
                    Space::new().width(Fill),
                    segmented::boolean(
                        [EN.keep, EN.remove],
                        self.remove_data,
                        self.data_glide.0,
                        motion::progress(
                            self.data_glide.1,
                            self.now,
                            motion::GLIDE,
                            Curve::Standard
                        ),
                        Message::RemoveData
                    ),
                ]
                .align_y(Center);
                column![
                    cta(
                        cta_label(EN.uninstall, None),
                        Kind::Danger,
                        Message::Install
                    ),
                    Space::new().height(BELOW_CTA),
                    divider(),
                    Space::new().height(BELOW_DIVIDER),
                    data,
                ]
                .into()
            }
            Step::Finish if self.uninstall.is_some() => {
                cta(cta_label(EN.close, None), Kind::Accent, Message::Close)
            }
            Step::Configure => {
                let label = match &self.installed {
                    None => EN.install,
                    Some(i) if i.same => EN.repair,
                    Some(_) => EN.update,
                };
                let status = match &self.installed {
                    None => EN.for_everyone.to_string(),
                    Some(i) if i.same => EN.same.to_string(),
                    Some(i) => fill(EN.replaces, &i.version),
                };
                let chevron = if self.open { "▴" } else { "▾" };
                let customize = link(
                    row![
                        text(EN.customize).size(Role::ROW_DESC.size),
                        text(chevron).font(ktext::MONO).size(CHEVRON),
                    ]
                    .spacing(space::GAP)
                    .align_y(Center),
                    Message::Customize,
                );
                let below = row![
                    customize,
                    Space::new().width(Fill),
                    row![chip::dot(SUCCESS), muted(status)]
                        .spacing(space::GAP)
                        .align_y(Center),
                ]
                .padding([0.0, LINE_X])
                .height(BELOW_H)
                .align_y(Center);
                let mut parts = column![
                    cta(
                        cta_label(label, self.size.clone()),
                        Kind::Accent,
                        Message::Install
                    ),
                    Space::new().height(BELOW_CTA),
                    below,
                ];
                if self.open {
                    parts = parts.push(self.drawer());
                }
                parts.into()
            }
            Step::Installing => {
                let share = self.bar_at();
                let stage = match self.bar.1 {
                    _ if self.uninstall.is_some() => EN.removing,
                    s if s < 0.38 => EN.preparing,
                    s if s < 0.78 => EN.copying,
                    _ => EN.finishing,
                };
                let progress = panel(column![
                    row![
                        ktext::sub(Role::CONTROL, stage),
                        Space::new().width(Fill),
                        ktext::accent(Role::FIGURE, format!("{}%", (share * 100.0).round())),
                    ]
                    .height(STAGE_H)
                    .align_y(Center),
                    Space::new().height(ABOVE_BAR),
                    progress::bar(share),
                ]);
                column![
                    progress,
                    Space::new().height(BELOW_PANEL),
                    self.path_row(None)
                ]
                .into()
            }
            Step::Failed(why) => {
                let message = container(ktext::error(Role::NOTE, why).width(Fill))
                    .height(FAILED_H)
                    .center_y(FAILED_H);
                let retry = link(text(EN.try_again).size(META), Message::Retry);
                column![
                    panel(message),
                    Space::new().height(BELOW_PANEL),
                    self.path_row(Some(retry)),
                ]
                .into()
            }
            Step::Finish => {
                let label = row![
                    cta_label(if self.leaving { EN.starting } else { EN.launch }, None),
                    ktext::text(Role::CTA, "→").color(ON_ACCENT),
                ]
                .spacing(TITLE_GAP)
                .align_y(Center);
                let start = row![
                    ktext::sub(Role::ROW_DESC, EN.start_with_windows),
                    Space::new().width(Fill),
                    segmented::boolean(
                        [EN.off, EN.on],
                        self.autostart,
                        self.autostart_glide.0,
                        motion::progress(
                            self.autostart_glide.1,
                            self.now,
                            motion::GLIDE,
                            Curve::Standard
                        ),
                        Message::Autostart
                    ),
                ]
                .align_y(Center);
                column![
                    cta(label, Kind::Success, Message::Launch),
                    Space::new().height(BELOW_CTA),
                    divider(),
                    Space::new().height(BELOW_DIVIDER),
                    start,
                ]
                .into()
            }
        }
    }

    /// The install folder under the progress (mono, cut short), and `extra` at the right.
    fn path_row<'a>(&'a self, extra: Option<Element<'a, Message>>) -> Element<'a, Message> {
        const MAX: usize = 40;
        let path = match self.folder.chars().count() > MAX {
            true => format!("{}…", self.folder.chars().take(MAX).collect::<String>()),
            false => self.folder.clone(),
        };
        let mut line = row![muted(path).font(ktext::MONO), Space::new().width(Fill)]
            .padding([0.0, LINE_X])
            .height(PATH_H)
            .align_y(Center);
        if let Some(extra) = extra {
            line = line.push(extra);
        }
        line.into()
    }

    /// "Customize install": the folder (Browse "…"), and why a typed one is refused.
    fn drawer(&self) -> Element<'_, Message> {
        let locked = self.folder_locked();
        let on_input = (!locked).then_some(Message::Folder);
        let folder = row![
            ktext::sub(Role::ROW_DESC, EN.folder),
            row![
                container(field(&self.folder, on_input, Message::Install)).width(Fill),
                button::button(
                    button::label(EN.browse),
                    Kind::Elevated,
                    (!locked).then_some(Message::Browse)
                ),
            ]
            .spacing(space::GAP)
            .align_y(Center),
        ]
        .spacing(TITLE_GAP)
        .height(FOLDER_H)
        .align_y(Center);
        let mut drawer = column![divider(), Space::new().height(BELOW_DIVIDER), folder];
        if let Some(bad) = self.bad {
            drawer = drawer
                .push(Space::new().height(ABOVE_BAR))
                .push(text(why(bad)).size(META).color(DANGER).height(ERROR_H));
        }
        drawer.into()
    }
}

/// The big call to action (Install / Launch / Uninstall).
fn cta<'a>(
    content: impl Into<Element<'a, Message>>,
    kind: Kind,
    on_press: Message,
) -> Element<'a, Message> {
    button::button(content, kind, Some(on_press))
}

/// Quiet text that acts ("Customize install ▾", "Try again").
fn link<'a>(content: impl Into<Element<'a, Message>>, on_press: Message) -> Element<'a, Message> {
    button::button(content, Kind::Quiet, Some(on_press))
}

/// A muted 11 px line (the status, the install path).
fn muted<'a>(s: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(s).size(META).color(themes::c().text_muted)
}

/// A recessed panel (the progress, why it failed): 14 px inside (8 px below:
/// the progress bar's glow takes 6).
fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    surface(Material::Recessed, radius::CARD, content)
        .width(Fill)
        .padding(Padding {
            bottom: PANEL_BOTTOM,
            ..Padding::new(PANEL_PAD)
        })
        .into()
}

/// The last step's work: the choice kept, and (`launch`) the app started.
fn finish(folder: &str, autostart: bool, launch: bool, updating: bool) -> Result<(), String> {
    keep_choice(folder, autostart);
    match launch {
        true => setup_core::launch(&setup_core::app_in(folder), updating),
        false => Ok(()),
    }
}

/// "Start with Windows" as left on the last step: the Run entry for the
/// installed exe now, and the app's own setting where it has one (a first
/// run's onboarding asks again).
fn keep_choice(folder: &str, autostart: bool) {
    let exe = setup_core::app_in(folder);
    if let Err(e) = crate::autostart::choose(&exe, autostart) {
        crate::log!("[opencircle] setup: start with Windows: {e}");
    }
    if crate::config::Config::path().is_some_and(|p| p.exists()) {
        let (config, warnings) = crate::config::Config::load();
        if warnings.is_empty()
            && config.general.autostart != autostart
            && let Err(e) = crate::config::update(|c| c.general.autostart = autostart)
        {
            crate::log!("[opencircle] setup: save the setting: {e}");
        }
    }
}

/// A picked folder as the install folder: its own `OpenCircle` folder inside
/// it, unless it is one already.
fn chosen(picked: &Path) -> String {
    let named = picked
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("OpenCircle"));
    match named {
        true => picked.display().to_string(),
        false => picked.join("OpenCircle").display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::{golden, shot};

    fn state(installed: Option<Installed>) -> State {
        let mut s = State::new(
            installed,
            Some(19 << 20),
            "OpenCircle-Setup-0.2.0-x64.exe".into(),
            Config {
                shortcut: "Ctrl+Shift+Space".into(),
                autostart: true,
            },
            None,
        );
        s.folder = r"C:\Program Files\OpenCircle\".into();
        s.bad = None;
        s.version = "0.2.0"; // the goldens don't change with each release
        s
    }

    /// The screens the golden images cover, as the user meets them.
    fn screens() -> Vec<(&'static str, State)> {
        let configure = state(None);
        let mut customize = state(None);
        customize.open = true;
        let mut bad = state(None);
        bad.open = true;
        bad.folder = r"C:\".into();
        bad.bad = Some(BadFolder::DriveRoot);
        let mut installing = state(None);
        installing.step = Step::Installing;
        installing.bar = (0.46, 0.46, installing.now);
        let mut failed = state(None);
        failed.step = Step::Failed(
            "Another installation is running. Wait for it to finish, then try again.".into(),
        );
        let mut finish = state(None);
        finish.step = Step::Finish;
        // Start with Windows turned off (the thumb on the first pill)
        let mut finish_off = state(None);
        finish_off.step = Step::Finish;
        finish_off.autostart = false;
        finish_off.autostart_glide.0 = false;
        let update = state(Some(Installed {
            version: "0.1.0".into(),
            folder: Some(r"C:\Program Files\OpenCircle\".into()),
            same: false,
        }));
        let uninstall = || {
            let mut s = state(None);
            s.uninstall = Some("{046BC980-BD0C-45D2-B990-202898DD0BAD}".into());
            s
        };
        let mut remove = uninstall();
        remove.remove_data = true;
        remove.data_glide.0 = true;
        let mut removing = uninstall();
        removing.step = Step::Installing;
        removing.bar = (0.62, 0.62, removing.now);
        let mut removed = uninstall();
        removed.step = Step::Finish;
        vec![
            ("6-uninstall", uninstall()),
            ("6-uninstall-remove", remove),
            ("7-removing", removing),
            ("8-removed", removed),
            ("1-welcome", configure),
            ("2-folder", customize),
            ("2-folder-refused", bad),
            ("3-progress", installing),
            ("3-failed", failed),
            ("4-finish", finish),
            ("4-finish-off", finish_off),
            ("5-update", update),
        ]
    }

    /// Each setup screen, dark (Sapphire) and light (Daylight), at 100 % and
    /// 200 %, matches its golden image (`tests/fixtures/setup/`,
    /// `kit::testing::golden`).
    #[test]
    fn every_screen_matches_its_golden_image() {
        let mut failed = Vec::new();
        for ((name, s), light) in (screens().into_iter().map(|n| (n, false)))
            .chain(screens().into_iter().map(|n| (n, true)))
        {
            set_light(light);
            let name = match light {
                true => format!("{name}-light"),
                false => name.to_string(),
            };
            let size = Size::new(WIDTH, s.height());
            failed.extend(golden::check("setup", &name, size, &[1.0, 2.0], || {
                s.view(window::Id::unique())
            }));
        }
        set_light(false);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    #[test]
    fn no_text_is_cut_off_on_any_screen() {
        for ((name, s), light) in (screens().into_iter().map(|n| (n, false)))
            .chain(screens().into_iter().map(|n| (n, true)))
        {
            set_light(light);
            let lost =
                shot::out_of_reach(s.view(window::Id::unique()), Size::new(WIDTH, s.height()));
            assert!(lost.is_empty(), "{name}: {lost:?}");
        }
    }

    #[test]
    fn a_refused_folder_keeps_the_setup_on_its_first_step() {
        let mut s = state(None);
        let _ = s.update(Message::Folder("Program Files".into()));
        assert_eq!(s.bad, Some(BadFolder::NotLocal));
        let _ = s.update(Message::Install);
        assert_eq!(s.step, Step::Configure);
        assert!(s.open, "the folder shows with why");
        let _ = s.update(Message::Folder(r"D:\Apps\OpenCircle".into()));
        assert_eq!(s.bad, None);
    }

    #[test]
    fn the_install_ends_on_launch_or_on_why_not() {
        let mut s = state(None);
        s.step = Step::Installing;
        let _ = s.update(Message::Msi(msi::Event::Progress(0.5)));
        assert_eq!(s.bar.1, 0.5);
        let _ = s.update(Message::Msi(msi::Event::Done(Err("no".into()))));
        assert_eq!(s.step, Step::Failed("no".into()));
        let _ = s.update(Message::Retry);
        assert_eq!(s.step, Step::Configure);
        let _ = s.update(Message::Msi(msi::Event::Done(Ok(()))));
        assert_eq!(s.step, Step::Finish);
    }

    /// The owner's "Not Responding" on the finish step: Launch's work (the
    /// old copy's quit, the registry, the config file, the process start) ran
    /// on the UI thread. It runs on a thread of its own: `update` returns at
    /// once, the button says it is starting, a second press does nothing, and
    /// a start that failed says why instead of closing.
    #[test]
    fn launch_never_blocks_the_window() {
        let mut s = state(None);
        s.step = Step::Finish;
        s.finish = |_, _, _, _| {
            std::thread::sleep(std::time::Duration::from_secs(2));
            Ok(())
        };
        let started = Instant::now();
        let _ = s.update(Message::Launch);
        let _ = s.update(Message::Launch);
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert!(s.leaving);
        let _ = s.update(Message::Left(Err("OpenCircle did not start.".into())));
        assert!(!s.leaving, "Launch can be pressed again");
        assert_eq!(s.launch_error.as_deref(), Some("OpenCircle did not start."));
        assert_eq!(s.step, Step::Finish);
    }

    /// Launch after an update or repair asks for Settings; after a fresh install it
    /// does not (the first run's onboarding shows).
    #[test]
    fn launch_asks_for_settings_only_when_updating() {
        use std::sync::atomic::{AtomicU8, Ordering::SeqCst};
        static SEEN: AtomicU8 = AtomicU8::new(0); // 1 = fresh, 2 = updating
        let old = Installed {
            version: "0.1.0".into(),
            folder: None,
            same: false,
        };
        for (installed, want) in [(None, 1), (Some(old), 2)] {
            SEEN.store(0, SeqCst);
            let mut s = state(installed);
            s.step = Step::Finish;
            s.finish = |_, _, _, updating| {
                SEEN.store(if updating { 2 } else { 1 }, SeqCst);
                Ok(())
            };
            let _ = s.update(Message::Launch);
            for _ in 0..50 {
                if SEEN.load(SeqCst) != 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            assert_eq!(SEEN.load(SeqCst), want);
        }
    }

    /// The uninstall window: Keep / Remove, then removing, then removed; it
    /// never touches "Start with Windows" or starts the app on its way out.
    #[test]
    fn the_uninstall_asks_removes_and_closes() {
        let mut s = state(None);
        s.uninstall = Some("{046BC980-BD0C-45D2-B990-202898DD0BAD}".into());
        s.finish = |_, _, _, _| panic!("the uninstall keeps no choice and starts nothing");
        assert!(!s.remove_data, "keeps the data unless asked");
        let _ = s.update(Message::RemoveData(true));
        assert!(
            s.remove_data && !s.data_glide.0,
            "the thumb glides from Keep"
        );
        s.step = Step::Installing;
        let _ = s.update(Message::Close);
        assert_eq!(s.step, Step::Installing, "no closing while it removes");
        let _ = s.update(Message::Msi(msi::Event::Done(Ok(()))));
        assert_eq!(s.step, Step::Finish);
        let _ = s.update(Message::Close);
        assert!(!s.leaving);
    }

    #[test]
    fn an_update_installs_where_the_app_is() {
        let s = state(Some(Installed {
            version: "0.1.0".into(),
            folder: Some(r"D:\Tools\OpenCircle".into()),
            same: false,
        }));
        let s = State::new(
            s.installed.clone(),
            None,
            String::new(),
            Config {
                shortcut: String::new(),
                autostart: true,
            },
            None,
        );
        assert_eq!(s.folder, r"D:\Tools\OpenCircle\");
        assert!(s.folder_locked());
    }

    #[test]
    fn the_window_height_and_the_switch_glide_on_the_designs_clock() {
        motion::set_reduced(false);
        let mut s = state(None);
        let h = s.height();
        s.height = (h, h, s.now);
        let _ = s.update(Message::Customize);
        let (from, to, since) = s.height;
        assert_eq!((from, to), (328.0, 367.0));
        assert_eq!(s.height_at(), from, "starts where it was");
        assert!(s.animating());
        s.now = since + motion::GLIDE / 2;
        let mid = s.height_at();
        assert!(from < mid && mid < to, "{mid}");
        s.now = since + motion::GLIDE;
        assert_eq!(s.height_at(), to);
        assert!(!s.animating());

        s.step = Step::Finish;
        let _ = s.update(Message::Autostart(false));
        assert!(s.autostart_glide.0, "the thumb leaves On");
        assert!(s.animating(), "and glides");
        let view = |s: &State| shot::render(s.view(window::Id::unique()), WIDTH as u32, 359);
        let start = view(&s);
        s.now = s.autostart_glide.1 + motion::GLIDE / 2;
        assert_ne!(view(&s), start, "the thumb moved");
        s.now = s.autostart_glide.1 + motion::GLIDE;
        assert!(!s.animating());
    }

    #[test]
    fn a_picked_folder_gets_its_own_opencircle_folder() {
        assert_eq!(
            chosen(Path::new("/x/Apps")),
            Path::new("/x/Apps")
                .join("OpenCircle")
                .display()
                .to_string()
        );
        assert_eq!(chosen(Path::new("/x/opencircle")), "/x/opencircle");
    }
}
