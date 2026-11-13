//! Wayland's long capture pop-up (owner, 2026-11-10, after W39's research on
//! GNOME 42): a Wayland window can't be placed or kept on top there, and the
//! overlay can't be cut, so the picture so far, its count and Copy / Save /
//! Done / Cancel run in a helper process of this exe on XWayland
//! (`opencircle long-capture-popup`, WAYLAND_DISPLAY unset): an override-
//! redirect window beside the area (`long_capture::popup_at`), exactly where
//! asked and over every window the whole time, never over the area (the
//! stream would hold it). The app streams and stitches and sends the helper
//! each change on its stdin (`Msg`); the helper prints a click back, one word
//! a line (`word`). Its stdin closing ends it. Without X11 (no DISPLAY, the
//! helper can't open its window) the tray's capturing state is the sign.
//! KDE / wlroots run XWayland too; a layer-shell surface only if ever needed.

use std::io::{self, BufRead, Write};
use std::process::{ChildStdout, Command, ExitCode, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, channel, sync_channel};
use std::time::Duration;

use iced::widget::image;
use iced::window::{self, Position};
use iced::{Element, Point, Rectangle, Size, Task};

use crate::app::background::background;
use crate::features::snip::screens::long_capture::{self, Finish};
use crate::shared::picture::Crop;
use crate::shared::platform::windows as platform;

/// The helper's hidden argument (spawned by the app only).
pub const ARG: &str = "long-capture-popup";

/// How long the pop-up shows how the capture ended ("Copied") before it goes.
const END_HOLD: Duration = Duration::from_millis(1500);
/// The window's shape is set again this long after it opens (`Event::Reshape`).
const RESHAPE_AFTER: Duration = Duration::from_millis(300);
/// The longest wait for the helper to go (`Popup::close`).
const CLOSE_WAIT: Duration = Duration::from_millis(500);
/// The largest picture a frame may carry (the preview is at most ~280 px wide).
const MAX_PICTURE: usize = 64 << 20;

/// What the app tells the pop-up.
#[derive(Debug, Clone)]
pub enum Msg {
    /// Where it is (logical desktop px; X11's px under XWayland at scale 1)
    /// and what it shows.
    Frame(Frame),
    /// The capture ended so ("Copied", "Saved"): shown a moment, then it goes.
    End(String),
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub window: Rectangle,
    /// Left of the area (or inside it): its content against its right edge.
    pub left: bool,
    /// Inside the area (an area with no room beside it): one row against
    /// its corner, with the whole-screen tip (`long_capture::popup_view`).
    pub inside: bool,
    pub tip: bool,
    /// The count: px so far, the limit reached, the page jumped.
    pub count: (u32, bool, bool),
    /// The picture so far (its px) and its logical size; none before the first.
    pub picture: Option<(Crop, Size)>,
}

impl Msg {
    /// One header line, then the picture's RGBA.
    fn encode(&self) -> Vec<u8> {
        match self {
            Msg::End(text) => format!("end {}\n", text.replace('\n', " ")).into_bytes(),
            Msg::Frame(f) => {
                let r = f.window;
                let (height, full, lost) = f.count;
                let (pw, ph, lw, lh, rgba) = match &f.picture {
                    Some((c, s)) => (c.width, c.height, s.width, s.height, &c.rgba[..]),
                    None => (0, 0, 0.0, 0.0, &[][..]),
                };
                let mut out = format!(
                    "frame {} {} {} {} {} {} {} {height} {} {} {pw} {ph} {lw} {lh}\n",
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    f.left as u8,
                    f.inside as u8,
                    f.tip as u8,
                    full as u8,
                    lost as u8
                )
                .into_bytes();
                out.extend_from_slice(rgba);
                out
            }
        }
    }

    /// The next message on `input`; none at its end (or on nonsense).
    fn decode(input: &mut impl BufRead) -> Option<Msg> {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end_matches('\n');
        if let Some(text) = line.strip_prefix("end ") {
            return Some(Msg::End(text.to_string()));
        }
        let mut words = line.strip_prefix("frame ")?.split(' ');
        let mut f = || words.next()?.parse::<f32>().ok();
        let window = Rectangle::new(Point::new(f()?, f()?), Size::new(f()?, f()?));
        let left = f()? != 0.0;
        let inside = f()? != 0.0;
        let tip = f()? != 0.0;
        let count = (f()? as u32, f()? != 0.0, f()? != 0.0);
        let (pw, ph) = (f()? as u32, f()? as u32);
        let shown = Size::new(f()?, f()?);
        let len = (pw as usize) * (ph as usize) * 4;
        let picture = match len {
            0 => None,
            _ if len > MAX_PICTURE => return None,
            _ => {
                let mut rgba = vec![0; len];
                input.read_exact(&mut rgba).ok()?;
                let crop = Crop {
                    width: pw,
                    height: ph,
                    rgba,
                };
                Some((crop, shown))
            }
        };
        Some(Msg::Frame(Frame {
            window,
            left,
            inside,
            tip,
            count,
            picture,
        }))
    }
}

/// A click's word on the helper's stdout, and back.
fn word(how: Finish) -> &'static str {
    match how {
        Finish::Copy => "copy",
        Finish::Save => "save",
        Finish::Done => "done",
        Finish::Cancel => "cancel",
        Finish::Pin => "pin",
        Finish::Share => "share",
        Finish::CopyText => "copy_text",
        Finish::AskAi => "ask_ai",
    }
}

pub fn parse(line: &str) -> Option<Finish> {
    [Finish::Copy, Finish::Save, Finish::Done, Finish::Cancel]
        .into_iter()
        .find(|f| word(*f) == line.trim())
}

/// The app's end: the helper running, fed by a thread (a frame never waits
/// on the helper: one it hasn't read yet is dropped, the next is newer).
pub struct Popup {
    to: SyncSender<Vec<u8>>,
    /// Says the helper has exited.
    gone: Receiver<()>,
}

impl Popup {
    /// Starts the helper on X11 (XWayland), with its clicks as they come.
    /// None without a DISPLAY or when it can't start.
    pub fn start() -> Option<(Popup, Task<Finish>)> {
        #[cfg(test)]
        return Popup::fake();
        #[cfg(not(test))]
        Popup::spawn()
    }

    #[cfg_attr(test, allow(dead_code))]
    fn spawn() -> Option<(Popup, Task<Finish>)> {
        let Some(display) = std::env::var_os("DISPLAY").filter(|d| !d.is_empty()) else {
            crate::log!("[opencircle] long capture: no XWayland (DISPLAY), no pop-up");
            return None;
        };
        let started = std::env::current_exe().and_then(|exe| {
            Command::new(exe)
                .arg(ARG)
                .env_remove("WAYLAND_DISPLAY")
                .env_remove("WAYLAND_SOCKET")
                .env("DISPLAY", display)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        });
        let mut child = match started {
            Ok(child) => child,
            Err(e) => {
                crate::log!("[opencircle] long capture: the pop-up didn't start: {e}");
                return None;
            }
        };
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return None;
        };
        let (to, from) = sync_channel::<Vec<u8>>(2);
        let (went, gone) = channel();
        std::thread::spawn(move || {
            for msg in from {
                if stdin.write_all(&msg).and_then(|()| stdin.flush()).is_err() {
                    break;
                }
            }
            drop(stdin); // the helper's end
            let _ = child.wait();
            let _ = went.send(());
        });
        Some((Popup { to, gone }, clicks(stdout)))
    }

    /// Tests: no process; what it is sent goes to `tests::sent` (none where
    /// `tests::NO_X11` is set: no XWayland).
    #[cfg(test)]
    fn fake() -> Option<(Popup, Task<Finish>)> {
        if tests::NO_X11.get() {
            return None;
        }
        let (to, from) = sync_channel(64);
        tests::SENT.set(Some(from));
        let (_, gone) = channel();
        Some((Popup { to, gone }, Task::none()))
    }

    /// Ends the helper and waits (briefly) until its window is gone, so the
    /// capture that follows (Done: Snip on a fresh frame) never holds it.
    pub fn close(self) {
        let Popup { to, gone } = self;
        drop(to);
        let _ = gone.recv_timeout(CLOSE_WAIT);
    }

    pub fn frame(&self, frame: Frame) {
        match self.to.try_send(Msg::Frame(frame).encode()) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
        }
    }

    /// Shows how it ended, then the helper goes (its stdin closes).
    pub fn end(self, text: &str) {
        let _ = self.to.send(Msg::End(text.into()).encode());
    }
}

/// The helper's clicks, as they come (`Finish`); ends with its stdout.
#[cfg_attr(test, allow(dead_code))]
fn clicks(stdout: ChildStdout) -> Task<Finish> {
    background(move |tx| {
        for line in io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(how) = parse(&line) {
                let _ = tx.unbounded_send(how);
            }
        }
    })
}

/// `opencircle long-capture-popup`: the pop-up's process (X11), until its
/// stdin ends.
pub fn main() -> ExitCode {
    let (config, _) = crate::shared::config::Config::load();
    crate::shared::config::set_current(config);
    let run =
        crate::app::shell::with_fonts(iced::daemon(Helper::boot, Helper::update, Helper::view))
            .theme(|_: &Helper, _| Some(platform::clear_theme()))
            .run();
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            crate::log!("[opencircle] long capture pop-up: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone)]
enum Event {
    /// From the app; none = its end.
    In(Option<Msg>),
    Click(Finish),
    OsTheme(iced::theme::Mode),
    /// The shape again, a moment after the window opened (`frame`).
    Reshape,
    Quit,
}

#[derive(Default)]
struct Helper {
    window: Option<(window::Id, Rectangle)>,
    left: bool,
    inside: bool,
    tip: bool,
    count: (u32, bool, bool),
    picture: Option<(image::Handle, Size)>,
    end: Option<String>,
    clicked: bool,
}

impl Helper {
    fn boot() -> (Helper, Task<Event>) {
        crate::shared::theme::apply(crate::shared::config::current().theme);
        let input = background(|tx| {
            let mut stdin = io::BufReader::new(io::stdin().lock());
            loop {
                let msg = Msg::decode(&mut stdin);
                let end = msg.is_none();
                if tx.unbounded_send(Event::In(msg)).is_err() || end {
                    return;
                }
            }
        });
        let os = iced::system::theme().map(Event::OsTheme);
        (Helper::default(), Task::batch([input, os]))
    }

    fn update(&mut self, event: Event) -> Task<Event> {
        match event {
            Event::In(Some(Msg::Frame(f))) => self.frame(f),
            Event::In(Some(Msg::End(text))) => {
                self.end = Some(text);
                background(|tx| {
                    std::thread::sleep(END_HOLD);
                    let _ = tx.unbounded_send(Event::Quit);
                })
            }
            // the app went: ended (its "Copied" holds on) or gone
            Event::In(None) if self.end.is_some() => Task::none(),
            Event::In(None) | Event::Quit => iced::exit(),
            Event::Click(how) if !self.clicked && self.end.is_none() => {
                self.clicked = true;
                let mut out = io::stdout().lock();
                let _ = writeln!(out, "{}", word(how)).and_then(|()| out.flush());
                Task::none()
            }
            Event::Click(_) => Task::none(),
            Event::Reshape => match self.window {
                Some((id, r)) => {
                    let picture = self.picture.as_ref().map_or(Size::ZERO, |(_, s)| *s);
                    let parts = long_capture::popup_parts(
                        r.size(),
                        picture,
                        self.left,
                        self.tip,
                        self.inside,
                    );
                    platform::popup_shape(id, parts, 1.0)
                }
                None => Task::none(),
            },
            Event::OsTheme(mode) => {
                crate::shared::theme::set_os(mode);
                Task::none()
            }
        }
    }

    /// Shows `f`: opens the window the first time, then follows its place.
    fn frame(&mut self, f: Frame) -> Task<Event> {
        (self.left, self.inside, self.tip, self.count) = (f.left, f.inside, f.tip, f.count);
        if let Some((c, size)) = f.picture {
            self.picture = Some((image::Handle::from_rgba(c.width, c.height, c.rgba), size));
        }
        let picture = self.picture.as_ref().map_or(Size::ZERO, |(_, s)| *s);
        let parts = long_capture::popup_parts(f.window.size(), picture, f.left, f.tip, f.inside);
        // ponytail: X11 at scale 1 (XWayland on GNOME without HiDPI scaling);
        // fractional / HiDPI XWayland not checked (W39)
        let shape = move |id| platform::popup_shape(id, parts.clone(), 1.0);
        match self.window {
            None => {
                let (id, open) = platform::x11_popup(window::Settings {
                    size: f.window.size(),
                    position: Position::Specific(f.window.position()),
                    ..window::Settings::default()
                });
                self.window = Some((id, f.window));
                // the shape set as it opens didn't hold on GNOME 42 (W46: a
                // first frame with a picture and the tip showed a black box
                // beside them until the next frame); set again a moment later
                let again = background(|tx| {
                    std::thread::sleep(RESHAPE_AFTER);
                    let _ = tx.unbounded_send(Event::Reshape);
                });
                Task::batch([open.then(shape), again])
            }
            Some((id, was)) => {
                self.window = Some((id, f.window));
                let mut moved = Task::none();
                if was.position() != f.window.position() {
                    moved = window::move_to(id, f.window.position());
                }
                if was.size() != f.window.size() {
                    moved = moved.chain(crate::shared::platform::monitor::resize(
                        id,
                        f.window.size(),
                    ));
                }
                moved.chain(shape(id))
            }
        }
    }

    fn view(&self, _: window::Id) -> Element<'_, Event> {
        let picture = self.picture.as_ref().map(|(h, s)| (h, *s));
        let tip = self.tip && self.end.is_none();
        long_capture::popup_view(
            picture,
            self.count,
            self.end.as_deref(),
            self.left,
            tip,
            self.inside,
        )
        .map(Event::Click)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    thread_local! {
        /// No XWayland: `Popup::start` gives none.
        pub static NO_X11: Cell<bool> = const { Cell::new(false) };
        /// What the last started pop-up was sent.
        pub static SENT: RefCell<Option<Receiver<Vec<u8>>>> = const { RefCell::new(None) };
    }

    /// What the pop-up was sent so far; `wait` = until its end (an `End`, or
    /// the app let go of it), at most 5 s.
    pub fn sent(wait: bool) -> Vec<Msg> {
        let mut bytes = Vec::new();
        SENT.with_borrow(|rx| {
            let Some(rx) = rx else { return };
            loop {
                let next = match wait {
                    true => rx.recv_timeout(Duration::from_secs(5)).ok(),
                    false => rx.try_recv().ok(),
                };
                let Some(msg) = next else { break };
                let end = msg.starts_with(b"end ");
                bytes.extend(msg);
                if end {
                    break;
                }
            }
        });
        let mut input = io::Cursor::new(bytes);
        std::iter::from_fn(|| Msg::decode(&mut input)).collect()
    }

    #[test]
    fn frames_and_ends_go_through_the_pipe_whole() {
        let frame = Msg::Frame(Frame {
            window: Rectangle::new(Point::new(812.0, 140.0), Size::new(174.0, 420.0)),
            left: true,
            inside: true,
            tip: true,
            count: (3240, false, true),
            picture: Some((
                Crop {
                    width: 3,
                    height: 2,
                    rgba: (0..24).collect(),
                },
                Size::new(3.0, 2.0),
            )),
        });
        let none = Msg::Frame(Frame {
            window: Rectangle::new(Point::new(1.0, 2.0), Size::new(3.0, 4.0)),
            left: false,
            inside: false,
            tip: false,
            count: (0, false, false),
            picture: None,
        });
        let end = Msg::End("Copied".into());
        let mut bytes = Vec::new();
        for m in [&frame, &none, &end] {
            bytes.extend(m.encode());
        }
        let mut input = io::Cursor::new(bytes);
        for m in [frame, none, end] {
            let read = Msg::decode(&mut input).expect("a message");
            assert_eq!(read.encode(), m.encode(), "{read:?}");
        }
        assert!(Msg::decode(&mut input).is_none(), "its end");
    }

    #[test]
    fn clicks_read_back_as_how_it_ends() {
        for how in [Finish::Copy, Finish::Save, Finish::Done, Finish::Cancel] {
            assert_eq!(parse(&format!("{}\n", word(how))), Some(how));
        }
        assert_eq!(parse("pin"), None, "only the pop-up's four");
        assert_eq!(parse("nonsense"), None);
    }
}
