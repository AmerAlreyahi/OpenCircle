//! Edit in place and the snip editor: the annotation editor (`crate::editor`) over
//! the frozen frame, its toolbar where the action bar was (DESIGN.md "Flow" 4).
//! Add capture (board): the editor steps aside and an area is dragged on the
//! frozen frame; its crop joins the board.
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke, fill};
use iced::widget::{container, image, pin, stack};
use iced::{Color, ContentFit, Element, Fill, Point, Rectangle, Renderer, Size, Theme};

use super::bar::{self, GAP};
use super::{theme, widgets};
use crate::actions::Crop;
use crate::editor::{self, Editor, Image, Message, Output};

/// Shown while an area for the board is picked.
const PICK_HINT: &str = "Drag an area to add to the board · Esc to go back";

pub struct Editing {
    pub editor: Editor,
    /// Screen position (logical px) of the image's top-left pixel.
    origin: Point,
    /// Display px per image px.
    zoom: f32,
    /// Back at the action bar: the flattened edits, shown over the selection.
    flat: Option<image::Handle>,
    /// Picking another capture's area: the drag so far (from, to; logical px).
    picking: Option<Option<(Point, Point)>>,
    /// The area just picked (logical px), for the host to crop (`take_picked`).
    picked: Option<Rectangle>,
}

impl Editing {
    /// Edit in place: `crop` is the frame's pixels `bounds` (see `actions::crop_bounds`)
    /// of a frame with `scale` physical px per logical px; it shows exactly there.
    pub fn in_place(crop: Crop, bounds: [u32; 4], scale: f32) -> Self {
        Self::new(crop, on_screen(bounds, scale).position(), 1.0 / scale)
    }

    /// Scaled down to fit the screen, with room for the toolbar (a scrolling capture).
    pub fn fitted(crop: Crop, screen: Size, scale: f32) -> Self {
        let size = Size::new(crop.width as f32, crop.height as f32);
        let (origin, zoom) = fit(size, screen, 1.0 / scale);
        Self::new(crop, origin, zoom)
    }

    fn new(crop: Crop, origin: Point, zoom: f32) -> Self {
        let image = Image {
            width: crop.width,
            height: crop.height,
            rgba: crop.rgba,
        };
        Editing {
            editor: Editor::new(image, zoom),
            origin,
            zoom,
            flat: None,
            picking: None,
            picked: None,
        }
    }

    /// The editor's messages; while an area is picked, the picker's (Esc = back
    /// to the editor).
    pub fn update(&mut self, message: Message) -> Option<Output> {
        let Some(drag) = &mut self.picking else {
            let out = self.editor.update(message);
            if matches!(out, Some(Output::AddCapture)) {
                self.picking = Some(None);
            }
            return out;
        };
        match message {
            Message::Press(p) => *drag = Some((p, p)),
            Message::Move(p) => {
                if let Some((_, to)) = drag {
                    *to = p;
                }
            }
            Message::Release => {
                let area = drag.take().map(|(a, b)| editor::span(&[a, b]));
                // A click is no area: keep picking.
                if let Some(area) = area.filter(|r| r.width >= 4.0 && r.height >= 4.0) {
                    self.picking = None;
                    self.picked = Some(area);
                }
            }
            Message::Back => self.picking = None,
            _ => {}
        }
        None
    }

    /// The area picked for the board (logical px), once.
    pub fn take_picked(&mut self) -> Option<Rectangle> {
        self.picked.take()
    }

    /// Adds `crop` to the board, refit to `screen` (never larger than now).
    pub fn add(&mut self, crop: Crop, screen: Size) {
        self.editor.append(Image {
            width: crop.width,
            height: crop.height,
            rgba: crop.rgba,
        });
        let (origin, zoom) = fit(self.editor.doc.view().size(), screen, self.zoom);
        (self.origin, self.zoom) = (origin, zoom);
        self.editor.set_zoom(zoom);
    }

    /// The toolbar is showing (not back at the action bar).
    pub fn is_open(&self) -> bool {
        self.flat.is_none()
    }

    pub fn reopen(&mut self) {
        self.flat = None;
    }

    /// Back to the action bar: the edits stay, shown flattened.
    pub fn back(&mut self) {
        let flat = self.image();
        self.flat = Some(image::Handle::from_rgba(flat.width, flat.height, flat.rgba));
    }

    /// The edited image (crop applied, annotations drawn): what actions get.
    pub fn image(&self) -> Crop {
        editor::render(&self.editor.doc)
    }

    /// Where the canvas is on screen, logical px (moves with the editor's crop).
    pub fn canvas_rect(&self) -> Rectangle {
        let view = self.editor.doc.view();
        Rectangle::new(
            self.origin + iced::Vector::new(view.x, view.y) * self.zoom,
            self.editor.size(),
        )
    }

    /// The canvas over its place and the toolbar beside it (as the action bar is).
    pub fn view(&self, screen: Size) -> Element<'_, editor::Message> {
        if let Some(drag) = self.picking {
            let hint = container(widgets::pill(PICK_HINT))
                .center_x(Fill)
                .padding(24);
            return stack![
                iced::widget::canvas(Picker { drag })
                    .width(Fill)
                    .height(Fill),
                hint,
            ]
            .into();
        }
        let canvas = self.canvas_rect();
        let room = screen.width - 2.0 * GAP;
        let size = toolbar_size(room);
        let at = bar::place(canvas, size, screen);
        let (toolbar, popover) = self.editor.toolbar_at(room, Rectangle::new(at, size));
        stack![
            pin(self.editor.canvas()).position(canvas.position()),
            pin(toolbar).position(at),
        ]
        .extend(popover)
        .into()
    }

    /// Back at the action bar: the flattened edits at the canvas's place.
    pub fn flat_view<'a, M: 'a>(&self) -> Option<Element<'a, M>> {
        let flat = self.flat.clone()?;
        let r = self.canvas_rect();
        Some(
            pin(image(flat)
                .width(r.width)
                .height(r.height)
                .content_fit(ContentFit::Fill))
            .position(r.position())
            .into(),
        )
    }
}

/// The full-screen area picker: dims all but the dragged box.
struct Picker {
    drag: Option<(Point, Point)>,
}

impl canvas::Program<Message> for Picker {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        use iced::keyboard::{self, Key, key::Named};
        let message = match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Message::Press(cursor.position_in(bounds)?)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if self.drag.is_some() => {
                Message::Move(cursor.position_in(bounds)?)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Message::Release
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(Named::Escape),
                ..
            }) => Message::Back,
            _ => return None,
        };
        Some(Action::publish(message).and_capture())
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let keep = self.drag.map(|(a, b)| editor::span(&[a, b]));
        let outside = Path::new(|p| {
            p.rectangle(Point::ORIGIN, bounds.size());
            if let Some(r) = keep {
                p.rectangle(r.position(), r.size());
            }
        });
        frame.fill(
            &outside,
            canvas::Fill {
                style: canvas::Style::Solid(theme::DIM_OUTSIDE),
                rule: fill::Rule::EvenOdd,
            },
        );
        if let Some(r) = keep {
            let edge = Stroke::default().with_color(Color::WHITE).with_width(1.5);
            frame.stroke(&Path::rectangle(r.position(), r.size()), edge);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::Crosshair
    }
}

/// The logical rectangle the frame pixels `[x0, y0, x1, y1]` cover.
fn on_screen([x0, y0, x1, y1]: [u32; 4], scale: f32) -> Rectangle {
    let px = |v: u32| v as f32 / scale;
    Rectangle::new(
        Point::new(px(x0), px(y0)),
        Size::new(px(x1 - x0), px(y1 - y0)),
    )
}

use editor::toolbar_size;

/// Where an image of `size` px goes on `screen` and its zoom (at most `max_zoom`):
/// centred, scaled down to leave room for the toolbar under it.
fn fit(size: Size, screen: Size, max_zoom: f32) -> (Point, f32) {
    let room = Size::new(
        screen.width - 2.0 * GAP,
        screen.height - 3.0 * GAP - toolbar_size(screen.width - 2.0 * GAP).height,
    );
    let zoom = max_zoom
        .min(room.width / size.width)
        .min(room.height / size.height);
    let shown = size * zoom;
    let origin = Point::new(
        (screen.width - shown.width) / 2.0,
        GAP + (room.height - shown.height) / 2.0,
    );
    (origin, zoom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::crop_bounds;

    fn crop(width: u32, height: u32) -> Crop {
        Crop {
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
        }
    }

    #[test]
    fn in_place_canvas_covers_the_selection_on_its_own_pixels() {
        // 150 % scaling: a 2880x1620 frame shown as 1920x1080.
        let scale = 1.5;
        let area = Rectangle::new(Point::new(100.3, 200.7), Size::new(300.2, 150.1));
        let bounds = crop_bounds(2880, 1620, scale, area).unwrap();
        let [x0, y0, x1, y1] = bounds;
        let e = Editing::in_place(crop(x1 - x0, y1 - y0), bounds, scale);
        let r = e.canvas_rect();
        // Exactly the cropped pixels, back in logical px...
        assert_eq!(
            [r.x, r.y, r.x + r.width, r.y + r.height].map(|v| (v * scale).round() as u32),
            bounds
        );
        // ...so it covers the selection, at most one physical px more on each side.
        assert!(r.x <= area.x && r.y <= area.y);
        assert!(r.x + r.width >= area.x + area.width);
        assert!(r.y + r.height >= area.y + area.height);
        assert!(area.x - r.x < 1.0 / scale && r.width - area.width < 2.0 / scale);
        // The editor shows it at its on-screen size (zoom = 1 / scale).
        assert_eq!(e.editor.size(), r.size());

        // Cropping in the editor moves the canvas to the kept part, same zoom.
        let mut e = e;
        e.editor.doc.crop = Some(Rectangle::new(
            Point::new(30.0, 60.0),
            Size::new(90.0, 45.0),
        ));
        let c = e.canvas_rect();
        assert!((c.x - (r.x + 20.0)).abs() < 1e-3 && (c.y - (r.y + 40.0)).abs() < 1e-3);
        assert_eq!(c.size(), Size::new(60.0, 30.0));
    }

    #[test]
    fn back_keeps_the_edits_and_shows_them_flattened() {
        let mut e = Editing::in_place(crop(40, 30), [10, 10, 50, 40], 1.0);
        assert!(e.is_open() && e.flat_view::<()>().is_none());
        e.back();
        assert!(!e.is_open() && e.flat_view::<()>().is_some());
        assert_eq!((e.image().width, e.image().height), (40, 30));
        e.reopen();
        assert!(e.is_open());
    }

    #[test]
    fn tall_captures_fit_the_screen_with_the_toolbar() {
        let screen = Size::new(1920.0, 1080.0);
        let bar = toolbar_size(screen.width - 2.0 * GAP);
        // A tall scrolling capture: scaled down, centred, toolbar room below.
        let (at, zoom) = fit(Size::new(780.0, 6000.0), screen, 1.0);
        assert!(zoom < 0.2, "{zoom}");
        assert!((at.x + 780.0 * zoom / 2.0 - 960.0).abs() < 1e-3);
        assert!(
            at.y >= GAP && at.y + 6000.0 * zoom + GAP + bar.height <= screen.height - GAP + 1e-3
        );
        // Small: never enlarged past its on-screen size (150 % → zoom 1 / 1.5).
        let (_, zoom) = fit(Size::new(400.0, 300.0), screen, 1.0 / 1.5);
        assert_eq!(zoom, 1.0 / 1.5);
        // The toolbar fits a 1920 px screen.
        assert!(bar.width < 1920.0 - 2.0 * GAP, "{bar:?}");
    }
}
