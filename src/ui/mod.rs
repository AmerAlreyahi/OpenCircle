//! The iced daemon: owns the (pre-created, hidden) overlay window and its state.
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
use iced::{ContentFit, Element, Fill, Point, Rectangle, Size, Subscription, Task};

use crate::{Trigger, capture};

const HINT: &str = "Circle, drag or tap anything · Esc to close";

/// Handed from `run` to the subscription (a subscription builder can't capture).
static TRIGGERS: Mutex<Option<UnboundedReceiver<Trigger>>> = Mutex::new(None);

pub fn run(triggers: UnboundedReceiver<Trigger>) -> iced::Result {
    *TRIGGERS.lock().unwrap() = Some(triggers);
    iced::daemon(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Trigger(Trigger),
    Overlay(overlay::Event),
    TestShot(window::Screenshot),
    /// A frame was drawn while closing: the empty overlay is on screen, hide it now.
    Drawn,
}

struct App {
    overlay: window::Id,
    /// The frozen frame while the overlay is open; `None` = hidden, frame freed.
    frame: Option<image::Handle>,
    stroke: Vec<Point>,
    drawing: bool,
    selection: Option<Rectangle>,
    /// Trigger time + expected size of the full-screen frame not drawn yet (latency log).
    first_draw: Cell<Option<(Instant, Size)>>,
    /// Closing: waiting for one empty frame so the renderer drops its frame cache.
    hiding: bool,
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
            stroke: Vec::new(),
            drawing: false,
            selection: None,
            first_draw: Cell::new(None),
            hiding: false,
        };
        (app, open.discard())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Trigger(Trigger::Quit) => iced::exit(),
            Message::Trigger(Trigger::Capture(_)) if self.frame.is_some() => Task::none(),
            Message::Trigger(Trigger::Capture(at)) => self.open(at),
            Message::Overlay(event) => self.on_overlay(event),
            Message::Drawn if self.hiding => {
                self.hiding = false;
                window::set_mode(self.overlay, Mode::Hidden)
            }
            Message::Drawn => Task::none(),
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
        self.first_draw.set(Some((at, size)));
        self.hiding = false;
        let origin = Point::new(frame.x as f32 / frame.scale, frame.y as f32 / frame.scale);
        // ponytail: mixed-DPI multi-monitor placement untested (needs real hardware).
        window::move_to(self.overlay, origin)
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
                self.selection = overlay::fit_selection(&self.stroke);
                if self.selection.is_none() {
                    self.stroke.clear();
                }
            }
            overlay::Event::Close if std::env::var_os(TEST_SHOT).is_some() => {
                return window::screenshot(self.overlay).map(Message::TestShot);
            }
            overlay::Event::Close => return self.close(),
        }
        Task::none()
    }

    /// Frees the frame, then hides the window after one empty frame is drawn: the
    /// renderer only drops its cached copy of the frame on a draw without it.
    fn close(&mut self) -> Task<Message> {
        self.frame = None;
        self.stroke = Vec::new();
        self.selection = None;
        self.drawing = false;
        self.hiding = true;
        Task::none()
    }

    fn view(&self, _window: window::Id) -> Element<'_, Message> {
        let Some(frame) = &self.frame else {
            return space().into();
        };
        let overlay = overlay::Overlay {
            stroke: &self.stroke,
            drawing: self.drawing,
            selection: self.selection,
            first_draw: &self.first_draw,
        };
        // The frame is its own layer: iced draws a canvas's images above its shapes.
        Element::from(stack![
            image(frame.clone())
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Fill),
            canvas(overlay).width(Fill).height(Fill),
            container(widgets::pill(HINT)).center_x(Fill).padding(24),
        ])
        .map(Message::Overlay)
    }

    fn subscription(&self) -> Subscription<Message> {
        let triggers = Subscription::run(triggers).map(Message::Trigger);
        if self.hiding {
            Subscription::batch([triggers, window::frames().map(|_| Message::Drawn)])
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
    let result = std::fs::File::create(&path)
        .map_err(|e| e.to_string())
        .and_then(|file| {
            let mut encoder = png::Encoder::new(file, shot.size.width, shot.size.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer
                .write_image_data(&shot.rgba)
                .map_err(|e| e.to_string())
        });
    if let Err(e) = result {
        eprintln!("test shot failed: {e}");
    }
}

fn triggers() -> impl Stream<Item = Trigger> {
    stream::iter(TRIGGERS.lock().unwrap().take()).flatten()
}
