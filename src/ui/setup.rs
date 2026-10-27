//! The setup window (`crate::setup`): the new design's installer
//! (`newdesing/widgets/installer.js`, theme Sapphire, solid look). One card:
//! Install (folder under "Customize install") → progress → Launch, with "Start
//! with Windows" on the last step. Drawn with `sapphire` components; the blurs
//! the CPU renderer can't draw (the emblem's halo, the buttons' glow) are baked
//! images (`scripts/installer-art.py`). Every string is in `EN` (one table to
//! translate; rows use Fill spacers, no fixed x, so they can mirror for Arabic).
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use iced::font::Weight;
use iced::widget::image::Handle;
use iced::widget::text::LineHeight;
use iced::widget::{Space, column, container, image, mouse_area, row, stack, text};
use iced::window;
use iced::{Border, Center, Element, Fill, Size, Subscription, Task};

use super::motion::{self, Curve};
use super::sapphire::{self, mono, weight};
use super::theme::sapphire::*;
use super::{widgets, windows};
use crate::setup::msi::{self, Package};
use crate::setup::{self as setup_core, BadFolder, Installed};

/// The window: 420 logical px wide (`max-w-[420px]`), its height per step.
pub const WIDTH: f32 = 420.0;
/// The window's corner radius (`rounded-2xl`).
const RADIUS: f32 = 16.0;
/// The header strip (`h-9`).
const HEADER: f32 = 36.0;
/// How far a new step's body rises in (`animatePageSwitch`: 14 px, 190 ms expo-out).
const RISE: f32 = 14.0;
/// The logo's pulse while installing (`animate-pulse`: 2 s, opacity 1 ↔ .5).
const PULSE: f32 = 2.0;

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
    start_with_windows: &'static str,
    off: &'static str,
    on: &'static str,
    pick_title: &'static str,
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
    start_with_windows: "Start with Windows",
    off: "Off",
    on: "On",
    pick_title: "Choose where to install OpenCircle",
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
    Retry,
    Close,
    Drag,
    Frame(Instant),
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
}

/// The setup's whole program: the window until it closes.
pub fn run(msi_bytes: &'static [u8]) -> iced::Result {
    let package = match Package::write(msi_bytes) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("[opencircle] setup: cannot unpack the installer: {e}");
            return Ok(());
        }
    };
    let installed = msi::installed(&package);
    let size = msi::installed_size(&package);
    let file_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "OpenCircle-Setup.exe".into());
    let (config, warnings) = crate::config::Config::load();
    let shown = Config {
        shortcut: config.shortcut,
        autostart: !warnings.is_empty() || config.general.autostart,
    };
    let boot = move || {
        let mut state = State::new(
            installed.clone(),
            size,
            file_name.clone(),
            shown.clone(),
            Some(package.clone()),
        );
        let (id, open) = windows::setup_window(Size::new(WIDTH, state.height()), RADIUS);
        state.window = Some(id);
        (state, open.discard())
    };
    let mut app = iced::daemon(boot, State::update, State::view);
    for font in widgets::FONTS {
        app = app.font(font);
    }
    app.default_font(widgets::ui_font())
        .title(|_: &State, _| EN.window_title.to_string())
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

fn theme() -> iced::Theme {
    static THEME: LazyLock<iced::Theme> = LazyLock::new(|| {
        let mut palette = iced::Theme::Dark.palette();
        palette.background = BG;
        palette.text = TEXT;
        palette.primary = ACCENT;
        iced::Theme::custom("sapphire", palette)
    });
    THEME.clone()
}

/// A bundled PNG, decoded once per call site (`LazyLock`).
fn png(bytes: &[u8]) -> Handle {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("a bundled PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("sized")];
    let info = reader.next_frame(&mut rgba).expect("a bundled PNG decodes");
    rgba.truncate(info.buffer_size());
    Handle::from_rgba(info.width, info.height, rgba)
}

macro_rules! bundled {
    ($name:ident, $file:literal) => {
        fn $name() -> Handle {
            static HANDLE: LazyLock<Handle> =
                LazyLock::new(|| png(include_bytes!(concat!("../../assets/setup/", $file))));
            HANDLE.clone()
        }
    };
}
bundled!(logo, "logo.png");
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
            risen: now - motion::MORPH,
            now,
            package,
        }
    }

    /// The window's height in this state (the design's measured card heights).
    fn height(&self) -> f32 {
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

    fn resize(&self) -> Task<Message> {
        let Some(id) = self.window else {
            return Task::none();
        };
        window::resize(id, Size::new(WIDTH, self.height())).chain(windows::reshape(id).discard())
    }

    fn bar_at(&self) -> f32 {
        let (from, to, since) = self.bar;
        from + (to - from) * motion::progress(since, self.now, motion::GLIDE, Curve::Ease)
    }

    fn animating(&self) -> bool {
        self.step == Step::Installing
            || self.bar_at() != self.bar.1
            || motion::done(self.risen, self.now, motion::MORPH) < 1.0
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Frame(now) => {
                self.now = now;
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
                super::background(move |tx| {
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
                let work = super::background(move |tx| {
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
                self.autostart = on;
                Task::none()
            }
            Message::Launch => {
                self.keep_choice();
                if let Err(e) = std::process::Command::new(setup_core::app_in(&self.folder)).spawn()
                {
                    eprintln!("[opencircle] setup: launch: {e}");
                }
                self.close()
            }
            Message::Close => match self.step {
                Step::Installing => Task::none(), // Windows Installer is working: wait
                Step::Finish => {
                    self.keep_choice();
                    self.close()
                }
                _ => self.close(),
            },
        }
    }

    fn close(&self) -> Task<Message> {
        match self.window {
            Some(id) => windows::close(id).chain(iced::exit()),
            None => iced::exit(),
        }
    }

    /// "Start with Windows" as left on the last step: the Run entry for the
    /// installed exe now, and the app's own setting where it has one (a first
    /// run's onboarding asks again).
    fn keep_choice(&self) {
        let exe = setup_core::app_in(&self.folder);
        if let Err(e) = crate::autostart::choose(&exe, self.autostart) {
            eprintln!("[opencircle] setup: start with Windows: {e}");
        }
        if crate::config::Config::path().is_some_and(|p| p.exists()) {
            let (mut config, warnings) = crate::config::Config::load();
            if warnings.is_empty() && config.general.autostart != self.autostart {
                config.general.autostart = self.autostart;
                if let Err(e) = config.save() {
                    eprintln!("[opencircle] setup: save the setting: {e}");
                }
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let closing = window::close_requests().map(|_| Message::Close);
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
            container(image(back()).width(256).height(210)).center_x(Fill),
        ];
        let glow = match self.step {
            Step::Configure => Some(glow_accent()),
            Step::Finish => Some(glow_success()),
            _ => None,
        }
        .map(|g| {
            column![
                Space::new().height(185.0 + rise),
                image(g).width(418).height(128)
            ]
        });
        let header = row![
            mouse_area(
                container(
                    text(&self.file_name)
                        .font(mono(Weight::Medium))
                        .size(11)
                        .color(TEXT_MUTED)
                )
                .padding([0, 14])
                .center_y(HEADER)
                .width(Fill)
            )
            .on_press(Message::Drag),
            sapphire::close_button(HEADER, Message::Close),
        ]
        .height(HEADER);
        let body = container(self.body())
            .padding(iced::Padding {
                top: 8.0 + rise,
                right: 24.0,
                bottom: 0.0,
                left: 24.0,
            })
            .width(Fill);
        let mut layers = stack![backdrop].width(Fill).height(Fill);
        if let Some(glow) = glow {
            layers = layers.push(glow);
        }
        let card = layers.push(column![header, body]);
        container(card)
            .padding(1)
            .width(Fill)
            .height(Fill)
            .style(|_| iced::widget::container::Style {
                background: Some(BG.into()),
                border: Border::default().rounded(RADIUS).width(1).color(RIM),
                ..Default::default()
            })
            .into()
    }

    fn body(&self) -> Element<'_, Message> {
        let finished = self.step == Step::Finish;
        let title = row![
            text(if finished { EN.ready } else { EN.title })
                .size(18)
                .font(weight(Weight::Semibold))
                .color(TEXT),
            sapphire::chip(format!("v{}", self.version)),
        ]
        .spacing(8)
        .align_y(Center)
        .height(28);
        let intro = match finished {
            true => fill(EN.installed, &self.shortcut),
            false => EN.intro.to_string(),
        };
        let intro = container(
            text(intro)
                .size(12)
                .color(TEXT_SUB)
                .line_height(LineHeight::Absolute(19.5.into()))
                .align_x(Center)
                .width(Fill),
        )
        .width(300)
        .height(39);
        column![
            self.emblem(),
            Space::new().height(12),
            title,
            Space::new().height(4),
            intro,
            Space::new().height(24),
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
        let tile = container(image(logo()).width(46).height(46).opacity(pulse))
            .width(68)
            .height(68)
            .padding(11)
            .style(|_| iced::widget::container::Style {
                background: Some(HOVER.into()),
                border: Border::default().rounded(20).width(1).color(RIM),
                ..Default::default()
            });
        let mut emblem = stack![container(tile).padding(4)].width(76).height(76);
        if self.step == Step::Finish {
            let check = container(
                text("✓")
                    .font(mono(Weight::Semibold))
                    .size(11)
                    .color(ON_ACCENT),
            )
            .center(20)
            .style(|_| iced::widget::container::Style {
                background: Some(SUCCESS.into()),
                border: Border::default().rounded(10),
                ..Default::default()
            });
            emblem = emblem.push(
                container(check)
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
                let customize = sapphire::text_link(
                    row![
                        text(EN.customize).size(11.5),
                        text(chevron).font(mono(Weight::Normal)).size(10),
                    ]
                    .spacing(6)
                    .align_y(Center),
                    Message::Customize,
                );
                let dot = container(Space::new().width(6).height(6)).style(|_| {
                    iced::widget::container::Style {
                        background: Some(SUCCESS.into()),
                        border: Border::default().rounded(3),
                        ..Default::default()
                    }
                });
                let below = row![
                    customize,
                    Space::new().width(Fill),
                    row![dot, text(status).size(11).color(TEXT_MUTED)]
                        .spacing(6)
                        .align_y(Center),
                ]
                .padding([0, 2])
                .height(17)
                .align_y(Center);
                let mut parts = column![
                    sapphire::cta(
                        sapphire::cta_label(label, self.size.clone()),
                        ACCENT,
                        Message::Install
                    ),
                    Space::new().height(14),
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
                    s if s < 0.38 => EN.preparing,
                    s if s < 0.78 => EN.copying,
                    _ => EN.finishing,
                };
                let progress = sapphire::panel(column![
                    row![
                        text(stage)
                            .size(12)
                            .font(weight(Weight::Medium))
                            .color(TEXT_SUB),
                        Space::new().width(Fill),
                        text(format!("{}%", (share * 100.0).round()))
                            .size(12)
                            .font(mono(Weight::Semibold))
                            .color(ACCENT),
                    ]
                    .height(16)
                    .align_y(Center),
                    Space::new().height(10),
                    sapphire::progress_bar(share),
                ]);
                column![progress, Space::new().height(12), self.path_row(None)].into()
            }
            Step::Failed(why) => {
                let message = container(text(why).size(12).color(DANGER).width(Fill))
                    .height(48)
                    .center_y(48);
                let retry = sapphire::text_link(text(EN.try_again).size(11), Message::Retry);
                column![
                    sapphire::panel(message),
                    Space::new().height(12),
                    self.path_row(Some(retry)),
                ]
                .into()
            }
            Step::Finish => {
                let label = row![
                    sapphire::cta_label(EN.launch, None),
                    text("→")
                        .size(13)
                        .font(weight(Weight::Semibold))
                        .color(ON_ACCENT),
                ]
                .spacing(8)
                .align_y(Center);
                let start = row![
                    text(EN.start_with_windows).size(11.5).color(TEXT_SUB),
                    Space::new().width(Fill),
                    sapphire::segmented(
                        &[(false, EN.off), (true, EN.on)],
                        self.autostart,
                        Message::Autostart
                    ),
                ]
                .align_y(Center);
                column![
                    sapphire::cta(label, SUCCESS, Message::Launch),
                    Space::new().height(14),
                    sapphire::divider(),
                    Space::new().height(12),
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
        let mut line = row![
            text(path)
                .font(mono(Weight::Normal))
                .size(11)
                .color(TEXT_MUTED),
            Space::new().width(Fill),
        ]
        .padding([0, 2])
        .height(33)
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
            text(EN.folder).size(11.5).color(TEXT_SUB),
            row![
                container(sapphire::field(&self.folder, on_input, Message::Install)).width(Fill),
                sapphire::small_button(EN.browse, (!locked).then_some(Message::Browse)),
            ]
            .spacing(6)
            .align_y(Center),
        ]
        .spacing(8)
        .height(26)
        .align_y(Center);
        let mut drawer = column![sapphire::divider(), Space::new().height(12), folder];
        if let Some(bad) = self.bad {
            drawer = drawer
                .push(Space::new().height(4))
                .push(text(why(bad)).size(11).color(DANGER).height(16));
        }
        drawer.into()
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
    use crate::ui::widgets::shot;

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
        let update = state(Some(Installed {
            version: "0.1.0".into(),
            folder: Some(r"C:\Program Files\OpenCircle\".into()),
            same: false,
        }));
        vec![
            ("1-welcome", configure),
            ("2-folder", customize),
            ("2-folder-refused", bad),
            ("3-progress", installing),
            ("3-failed", failed),
            ("4-finish", finish),
            ("5-update", update),
        ]
    }

    fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut rgba).unwrap();
        rgba.truncate(info.buffer_size());
        (info.width, info.height, rgba)
    }

    /// Each setup screen, at 100 % and 200 %, matches its golden image
    /// (`tests/fixtures/setup/`; `OPENCIRCLE_UPDATE_GOLDEN=1` writes them anew,
    /// then look at every one). Only where the UI font is the bundled Inter
    /// (Windows draws Segoe UI).
    #[test]
    fn every_screen_matches_its_golden_image() {
        if widgets::ui_font() != iced::Font::with_name("Inter") {
            eprintln!("golden images are drawn with Inter; this machine has Segoe UI");
            return;
        }
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/setup");
        let update = std::env::var_os("OPENCIRCLE_UPDATE_GOLDEN").is_some();
        let mut failed = Vec::new();
        for (name, s) in screens() {
            for scale in [1.0, 2.0] {
                let size = Size::new(WIDTH, s.height());
                let (_, pixels) = shot::redrawn(
                    |i| (i == 0).then(|| s.view(window::Id::unique())),
                    size,
                    scale,
                );
                let (w, h) = ((WIDTH * scale) as u32, (s.height() * scale) as u32);
                let file = dir.join(format!("{name}@{scale}x.png"));
                if update {
                    std::fs::create_dir_all(&dir).unwrap();
                    crate::actions::write_png(&file, w, h, &pixels).unwrap();
                    continue;
                }
                let Ok(golden) = std::fs::read(&file) else {
                    failed.push(format!("{name}@{scale}x: no golden image"));
                    continue;
                };
                let (gw, gh, golden) = decode(&golden);
                if (gw, gh) != (w, h) {
                    failed.push(format!("{name}@{scale}x: {w}x{h}, golden {gw}x{gh}"));
                    continue;
                }
                // tiny differences in anti-aliasing between CPUs are fine
                let off = (pixels.chunks(4).zip(golden.chunks(4)))
                    .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 24))
                    .count();
                if off * 1000 > (w * h) as usize {
                    failed.push(format!("{name}@{scale}x: {off} pixels differ"));
                }
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }

    #[test]
    fn no_text_is_cut_off_on_any_screen() {
        for (name, s) in screens() {
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
