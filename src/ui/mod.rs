//! The iced daemon: owns the (pre-created, hidden) overlay window and its state.
mod bar;
mod overlay;
pub mod theme;
mod tray;
mod widgets;

pub use tray::start as start_tray;

use std::cell::Cell;
use std::sync::Mutex;
use std::time::Instant;

use iced::futures::channel::mpsc::UnboundedReceiver;
use iced::futures::stream::{self, Stream, StreamExt};
use iced::widget::{canvas, container, image, space, stack};
use iced::window::{self, Level, Mode};
use iced::{ContentFit, Element, Fill, Point, Size, Subscription, Task};

use crate::actions::{self, Action};
use crate::{Trigger, capture};

const HINT: &str = "Circle, drag or tap anything · Esc to close";

/// Handed from `run` to the subscription (a subscription builder can't capture).
static TRIGGERS: Mutex<Option<UnboundedReceiver<Trigger>>> = Mutex::new(None);

pub fn run(triggers: UnboundedReceiver<Trigger>) -> iced::Result {
    *TRIGGERS.lock().unwrap() = Some(triggers);
    iced::daemon(App::boot, App::update, App::view)
        .font(widgets::ICON_FONT_BYTES)
        .subscription(App::subscription)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Trigger(Trigger),
    Overlay(overlay::Event),
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

struct App {
    overlay: window::Id,
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
            frame: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
            opening: 0,
            first_draw: Cell::new(None),
            hiding: Hiding::No,
        };
        (app, open.discard())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Trigger(Trigger::Quit) => iced::exit(),
            Message::Trigger(Trigger::Capture(_)) if self.frame.is_some() => Task::none(),
            Message::Trigger(Trigger::Capture(at)) => self.open(at),
            Message::Overlay(event) => self.on_overlay(event),
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

    fn open(&mut self, at: Instant) -> Task<Message> {
        let frame = match capture::monitor_under_cursor() {
            Ok(frame) => frame,
            Err(e) => {
                eprintln!("capture failed: {e}");
                return Task::none();
            }
        };
        eprintln!(
            "[opencircle] capture {}x{} took {} ms",
            frame.width,
            frame.height,
            at.elapsed().as_millis()
        );
        self.frame = Some(image::Handle::from_rgba(
            frame.width,
            frame.height,
            frame.rgba,
        ));
        let size = Size::new(frame.width as f32, frame.height as f32) * (1.0 / frame.scale);
        self.scale = frame.scale;
        self.screen = size;
        self.opening = self.opening.wrapping_add(1);
        self.first_draw.set(Some((at, size)));
        self.hiding = Hiding::No;
        let origin = Point::new(frame.x as f32 / frame.scale, frame.y as f32 / frame.scale);
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
            overlay::Event::Close if std::env::var_os(TEST_SHOT).is_some() => {
                return window::screenshot(self.overlay).map(Message::TestShot);
            }
            overlay::Event::Close => return self.close(),
        }
        Task::none()
    }

    /// Crops the selection, closes the overlay (so dialogs aren't hidden under it),
    /// then runs `action` on the crop. No selection yet = nothing to do.
    fn run(&mut self, action: &'static Action) -> Task<Message> {
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
            return Task::none();
        };
        let Some(crop) = actions::crop(pixels, *width, *height, self.scale, selection.area) else {
            return Task::none();
        };
        let closing = self.close();
        if let Err(e) = (action.run)(crop) {
            eprintln!("{} failed: {e}", action.id);
        }
        closing
    }

    /// Frees the frame; the window is shrunk and hidden in steps (see `Hiding`).
    fn close(&mut self) -> Task<Message> {
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
        match self.selection {
            Some(selection) => layers.push(bar::view(selection.area, self.screen, Message::Run)),
            None => layers.push(container(widgets::pill(HINT)).center_x(Fill).padding(24)),
        }
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let triggers = Subscription::run(triggers).map(Message::Trigger);
        if self.hiding != Hiding::No {
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

fn triggers() -> impl Stream<Item = Trigger> {
    stream::iter(TRIGGERS.lock().unwrap().take()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closing_app() -> App {
        let mut app = App {
            overlay: window::Id::unique(),
            frame: None,
            scale: 1.0,
            screen: Size::ZERO,
            stroke: Vec::new(),
            drawing: false,
            selection: None,
            opening: 0,
            first_draw: Cell::new(None),
            hiding: Hiding::No,
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
