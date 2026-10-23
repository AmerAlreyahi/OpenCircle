//! The iced daemon: owns the (pre-created, hidden) overlay window and its state.
mod bar;
mod overlay;
mod panel;
pub mod theme;
mod tray;
pub(crate) mod widgets;

pub use tray::start as start_tray;

use std::cell::Cell;
use std::sync::Mutex;
use std::time::Instant;

use iced::futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use iced::futures::stream::{self, Stream, StreamExt};
use iced::keyboard::{self, key::Named};
use iced::widget::{canvas, container, image, space, stack};
use iced::window::{self, Level, Mode};
use iced::{ContentFit, Element, Fill, Point, Size, Subscription, Task};

use crate::actions::{self, Action, Crop, Run, lens};
use crate::{Trigger, capture};

const HINT: &str = "Circle, drag or tap anything · Esc to close";

/// Handed from `run` to the subscription (a subscription builder can't capture).
static TRIGGERS: Mutex<Option<UnboundedReceiver<Trigger>>> = Mutex::new(None);

/// Call first in `main`, before any thread starts (glibc fixes the arena limit
/// at the first thread's first allocation). Frames and panel work are allocated
/// on short-lived threads; with per-thread arenas each overlay session kept a
/// whole frame (~20 MB at 3440x1440) after closing. One arena keeps the app at
/// its idle size.
pub fn limit_malloc_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: only changes an allocator setting; no memory is touched.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

pub fn run(triggers: UnboundedReceiver<Trigger>) -> iced::Result {
    *TRIGGERS.lock().unwrap() = Some(triggers);
    crate::lens::sweep_stale(); // pages left by a crash
    iced::daemon(App::boot, App::update, App::view)
        .font(widgets::ICON_FONT_BYTES)
        .subscription(App::subscription)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Trigger(Trigger),
    /// The capture thread is done (`Instant` = trigger time, latency log).
    Captured(Instant, Result<Shot, String>),
    Overlay(overlay::Event),
    /// Esc, wherever the focus is.
    Escape,
    /// From the panel with this number (stale numbers are ignored).
    Panel(u32, panel::Msg),
    /// Run an action on the selection (action bar button or its key).
    Run(&'static Action),
    TestShot(window::Screenshot),
    /// While closing: a frame was drawn / the window was resized (see `Hiding`).
    Drawn,
    Resized(Size),
}

/// Closing steps. The renderer frees the frame cache and resizes its buffers only
/// when it draws, and a hidden window never draws, so shrink first, then hide.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hiding {
    No,
    /// Wait for one empty full-size frame (drops the cached frame).
    Empty,
    /// Left full-screen and asked for 1x1; wait for the resize to land (frames so far).
    Shrinking(u8),
    /// Wait for one frame at the small size (shrinks the buffers), then hide.
    Shrunk,
}

/// Below this (logical px) the window counts as shrunk.
const SHRUNK: f32 = 64.0;
/// Hide anyway after this many frames without the shrink (a WM may refuse it): a
/// window left on top of the screen would be far worse than unshrunk buffers.
// ponytail: fixed frame count, not a timer; frames come back to back while closing.
const SHRINK_FRAMES: u8 = 10;

/// A captured monitor, ready to show.
#[derive(Debug, Clone)]
struct Shot {
    image: image::Handle,
    scale: f32,
    /// Logical px: the monitor's origin and size.
    origin: Point,
    size: Size,
}

struct App {
    overlay: window::Id,
    /// A capture is running on its thread.
    capturing: bool,
    /// The frozen frame while the overlay is open; `None` = hidden, frame freed.
    /// Its RGBA bytes are also what actions crop (shared, never copied whole).
    frame: Option<image::Handle>,
    /// Physical px per logical px of `frame`.
    scale: f32,
    /// The overlay's logical size (the monitor's).
    screen: Size,
    stroke: Vec<Point>,
    drawing: bool,
    selection: Option<overlay::Selection>,
    /// Bumped on every open (resets the overlay's pointer state).
    opening: u32,
    /// Trigger time + expected size of the full-screen frame not drawn yet (latency log).
    first_draw: Cell<Option<(Instant, Size)>>,
    hiding: Hiding,
    /// The open result panel and its number.
    panel: Option<(u32, panel::Panel)>,
    panels: u32,
    /// Lens pages opened from this overlay, released when it closes.
    lens_pages: Vec<lens::Held>,
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        // Created once, hidden, so a trigger only has to show it (latency).
        let (overlay, open) = window::open(window::Settings {
            visible: false,
            decorations: false,
            resizable: false,
            level: Level::AlwaysOnTop,
            exit_on_close_request: false,
            ..window::Settings::default()
        });
        let app = App {
            overlay,
            capturing: false,
            frame: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
            opening: 0,
            first_draw: Cell::new(None),
            hiding: Hiding::No,
            panel: None,
            panels: 0,
            lens_pages: Vec::new(),
        };
        (app, open.discard())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Trigger(Trigger::Quit) => iced::exit(),
            Message::Trigger(Trigger::Capture(_) | Trigger::Quick(..))
                if self.frame.is_some() || self.capturing =>
            {
                Task::none()
            }
            Message::Trigger(Trigger::Capture(at)) => self.capture(at),
            Message::Trigger(Trigger::Quick(at, quick)) => {
                eprintln!(
                    "quick action {} not wired yet; opening the overlay",
                    quick.action
                );
                self.capture(at)
            }
            Message::Captured(at, shot) => self.open(at, shot),
            Message::Overlay(event) => self.on_overlay(event),
            Message::Escape if self.frame.is_none() => Task::none(),
            Message::Escape if std::env::var_os(TEST_SHOT).is_some() => {
                // Shoot after the next frame, with no message (= no relayout) in
                // between: a relayout drops the text editor's drawn text.
                let overlay = self.overlay;
                background(|tx| {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let _ = tx.unbounded_send(());
                })
                .then(move |()| window::screenshot(overlay))
                .map(Message::TestShot)
            }
            Message::Escape => self.close(),
            Message::Panel(id, msg) => self.on_panel(id, msg),
            Message::Run(action) => self.run(action),
            // Leave full-screen while still shown (on Windows, leaving it while hidden
            // restores a placement saved while visible: shows it again), then shrink.
            // open() resizes it back before showing.
            Message::Drawn if self.hiding == Hiding::Empty => {
                self.hiding = Hiding::Shrinking(0);
                window::set_mode(self.overlay, Mode::Windowed)
                    .chain(window::resize(self.overlay, Size::new(1.0, 1.0)))
            }
            Message::Resized(size)
                if matches!(self.hiding, Hiding::Shrinking(_))
                    && size.width.max(size.height) < SHRUNK =>
            {
                self.hiding = Hiding::Shrunk;
                Task::none()
            }
            Message::Drawn
                if matches!(
                    self.hiding,
                    Hiding::Shrunk | Hiding::Shrinking(SHRINK_FRAMES..)
                ) =>
            {
                self.hiding = Hiding::No;
                // Panel threads (PNG, HTTP, OCR I/O) leave freed memory in their
                // own malloc arenas; give it back to the OS while idle.
                #[cfg(all(target_os = "linux", target_env = "gnu"))]
                // SAFETY: malloc_trim only releases free heap pages.
                unsafe {
                    libc::malloc_trim(0);
                }
                window::set_mode(self.overlay, Mode::Hidden)
            }
            Message::Drawn => {
                if let Hiding::Shrinking(frames) = &mut self.hiding {
                    *frames += 1;
                }
                Task::none()
            }
            Message::Resized(_) => Task::none(),
            Message::TestShot(shot) => {
                save_test_shot(&shot);
                self.close()
            }
        }
    }

    /// Captures on a thread: a Wayland portal may wait minutes for the user's OK.
    fn capture(&mut self, at: Instant) -> Task<Message> {
        self.capturing = true;
        background(|tx| {
            let shot = capture::monitor_under_cursor().map(|frame| {
                let scale = frame.scale;
                Shot {
                    origin: Point::new(frame.x as f32 / scale, frame.y as f32 / scale),
                    size: Size::new(frame.width as f32, frame.height as f32) * (1.0 / scale),
                    image: image::Handle::from_rgba(frame.width, frame.height, frame.rgba),
                    scale,
                }
            });
            let _ = tx.unbounded_send(shot);
        })
        .map(move |shot| Message::Captured(at, shot))
    }

    fn open(&mut self, at: Instant, shot: Result<Shot, String>) -> Task<Message> {
        self.capturing = false;
        let shot = match shot {
            Ok(shot) => shot,
            Err(e) => {
                eprintln!("capture failed: {e}");
                return Task::none();
            }
        };
        eprintln!(
            "[opencircle] capture {}x{} took {} ms",
            shot.size.width * shot.scale,
            shot.size.height * shot.scale,
            at.elapsed().as_millis()
        );
        let (origin, size) = (shot.origin, shot.size);
        self.frame = Some(shot.image);
        self.scale = shot.scale;
        self.screen = size;
        self.opening = self.opening.wrapping_add(1);
        self.first_draw.set(Some((at, size)));
        self.hiding = Hiding::No;
        // Place and size the hidden window to cover the monitor BEFORE set_mode shows
        // it: iced makes it visible before going full-screen, so whatever is shown
        // first must already cover the monitor (no small black box flash).
        // ponytail: mixed-DPI multi-monitor placement untested (needs real hardware).
        window::move_to(self.overlay, origin)
            .chain(window::resize(self.overlay, size))
            .chain(window::set_mode(self.overlay, Mode::Fullscreen))
            .chain(window::gain_focus(self.overlay))
    }

    fn on_overlay(&mut self, event: overlay::Event) -> Task<Message> {
        match event {
            overlay::Event::Press(p) => {
                // A new selection: the panel was about the old one.
                self.panel = None;
                self.stroke = vec![p];
                self.selection = None;
                self.drawing = true;
            }
            overlay::Event::Move(p) if self.drawing => self.stroke.push(p),
            overlay::Event::Move(_) => {}
            overlay::Event::Release => {
                self.drawing = false;
                self.selection = overlay::select(&self.stroke);
                if self.selection.is_none() {
                    self.stroke.clear();
                }
            }
            overlay::Event::Enter => {
                if let Some(default) = actions::ACTIONS.first() {
                    return self.run(default);
                }
            }
            overlay::Event::Key { letter, ctrl } => {
                if let Some(action) = actions::for_key(letter, ctrl) {
                    return self.run(action);
                }
            }
        }
        Task::none()
    }

    /// Runs `action` on the selection: a finishing action closes the overlay first
    /// (so dialogs aren't hidden under it); a panel action opens its panel.
    /// No selection yet = nothing to do.
    fn run(&mut self, action: &'static Action) -> Task<Message> {
        let Some(crop) = self.crop() else {
            return Task::none();
        };
        match action.run {
            Run::Finish(finish) => {
                let closing = self.close();
                if let Err(e) = finish(crop) {
                    eprintln!("{} failed: {e}", action.id);
                }
                closing
            }
            Run::Panel(kind) => self.open_panel(kind, crop, None),
        }
    }

    fn open_panel(
        &mut self,
        kind: actions::Panel,
        crop: Crop,
        question: Option<String>,
    ) -> Task<Message> {
        self.panels = self.panels.wrapping_add(1);
        let id = self.panels;
        let (panel, task) = panel::Panel::open(kind, crop, question);
        self.panel = Some((id, panel));
        task.map(move |msg| Message::Panel(id, msg))
    }

    fn on_panel(&mut self, id: u32, msg: panel::Msg) -> Task<Message> {
        // Every opened page is held, whichever panel is showing now.
        if let panel::Msg::LensOpened(Ok(page)) = &msg {
            if self.frame.is_some() {
                self.lens_pages.push(page.clone());
            } else {
                page.clone().release(); // the overlay closed meanwhile
            }
        }
        let Some((_, panel)) = self.panel.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            panel::Msg::Cancel => self.panel = None,
            panel::Msg::Search => return self.run(&lens::ACTION),
            panel::Msg::CopyText => {
                let text = panel.text().unwrap_or_default();
                let closing = self.close();
                if let Err(e) = actions::copy::text(text) {
                    eprintln!("copy text failed: {e}");
                }
                return closing;
            }
            // Lens follow-up: hand the question over to Ask AI.
            panel::Msg::Submit if panel.kind() == actions::Panel::Lens => {
                let question = panel.take_input();
                if question.trim().is_empty() {
                    return Task::none();
                }
                let Some(crop) = self.crop() else {
                    return Task::none();
                };
                return self.open_panel(actions::Panel::Ask, crop, Some(question));
            }
            msg => return panel.update(msg).map(move |msg| Message::Panel(id, msg)),
        }
        Task::none()
    }

    /// The selection cut out of the frozen frame, in physical pixels.
    fn crop(&self) -> Option<Crop> {
        let (
            Some(image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            }),
            Some(selection),
        ) = (&self.frame, self.selection)
        else {
            return None;
        };
        actions::crop(pixels, *width, *height, self.scale, selection.area)
    }

    /// Frees the frame and drops the panel (stopping its work); Lens pages go
    /// once old enough. The window is shrunk and hidden in steps (see `Hiding`).
    fn close(&mut self) -> Task<Message> {
        self.panel = None;
        self.lens_pages.drain(..).for_each(lens::Held::release);
        self.frame = None;
        self.stroke = Vec::new();
        self.selection = None;
        self.drawing = false;
        self.hiding = Hiding::Empty;
        Task::none()
    }

    fn view(&self, _window: window::Id) -> Element<'_, Message> {
        let Some(frame) = &self.frame else {
            return space().into();
        };
        let overlay = overlay::Overlay {
            stroke: &self.stroke,
            selection: self.selection,
            opening: self.opening,
            first_draw: &self.first_draw,
        };
        // The frame is its own layer: iced draws a canvas's images above its shapes.
        let layers = stack![
            image(frame.clone())
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Fill),
            Element::from(canvas(overlay).width(Fill).height(Fill)).map(Message::Overlay),
        ];
        match (&self.panel, self.selection) {
            (Some((id, panel)), _) => {
                let id = *id;
                layers.push(
                    panel
                        .view(self.screen)
                        .map(move |msg| Message::Panel(id, msg)),
                )
            }
            (None, Some(selection)) => {
                layers.push(bar::view(selection.area, self.screen, Message::Run))
            }
            (None, None) => layers.push(container(widgets::pill(HINT)).center_x(Fill).padding(24)),
        }
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let triggers = Subscription::run(triggers).map(Message::Trigger);
        if self.frame.is_some() {
            // Also when a text field took the key (it only unfocuses on Esc).
            let escape = iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                }) => Some(Message::Escape),
                _ => None,
            });
            Subscription::batch([triggers, escape])
        } else if self.hiding != Hiding::No {
            // One stream for both, so a resize is always seen before the frames after it.
            // Every message makes iced redraw, so frames keep coming while this runs.
            let steps = iced::event::listen_raw(|event, _, _| match event {
                iced::Event::Window(window::Event::RedrawRequested(_)) => Some(Message::Drawn),
                iced::Event::Window(window::Event::Resized(size)) => Some(Message::Resized(size)),
                _ => None,
            });
            Subscription::batch([triggers, steps])
        } else {
            triggers
        }
    }
}

/// Test hook: on Esc, save what the overlay shows before closing (WSLg can't grab it).
const TEST_SHOT: &str = "OPENCIRCLE_TEST_SHOT";

fn save_test_shot(shot: &window::Screenshot) {
    let Some(path) = std::env::var_os(TEST_SHOT) else {
        return;
    };
    let result = actions::write_png(path.as_ref(), shot.size.width, shot.size.height, &shot.rgba);
    if let Err(e) = result {
        eprintln!("test shot failed: {e}");
    }
}

/// Runs `work` on its own thread; what it sends arrives as messages, so the UI
/// never waits on it. Dropping (aborting) the task disconnects `work`'s sender.
fn background<T: Send + 'static>(
    work: impl FnOnce(&UnboundedSender<T>) + Send + 'static,
) -> Task<T> {
    let (tx, rx) = mpsc::unbounded();
    let spawned = std::thread::Builder::new()
        .name("background".into())
        .spawn(move || work(&tx));
    if let Err(e) = spawned {
        eprintln!("cannot start a thread: {e}");
    }
    Task::stream(rx)
}

fn triggers() -> impl Stream<Item = Trigger> {
    stream::iter(TRIGGERS.lock().unwrap().take()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closing_app() -> App {
        let mut app = App {
            overlay: window::Id::unique(),
            capturing: false,
            frame: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
            opening: 0,
            first_draw: Cell::new(None),
            hiding: Hiding::No,
            panel: None,
            panels: 0,
            lens_pages: Vec::new(),
        };
        let _ = app.close();
        app
    }

    #[test]
    fn close_shrinks_then_hides() {
        let mut app = closing_app();
        let _ = app.update(Message::Drawn);
        let _ = app.update(Message::Drawn); // a full-size frame before the resize lands
        let _ = app.update(Message::Resized(Size::new(3440.0, 1440.0)));
        assert_eq!(app.hiding, Hiding::Shrinking(1));
        let _ = app.update(Message::Resized(Size::new(1.0, 1.0)));
        let _ = app.update(Message::Drawn);
        assert_eq!(app.hiding, Hiding::No);
    }

    #[test]
    fn close_hides_even_if_the_window_never_shrinks() {
        let mut app = closing_app();
        for _ in 0..=SHRINK_FRAMES {
            let _ = app.update(Message::Drawn);
        }
        assert_ne!(app.hiding, Hiding::No);
        let _ = app.update(Message::Drawn);
        assert_eq!(app.hiding, Hiding::No);
    }
}
