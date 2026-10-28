//! Edit in place and the snip editor: the annotation editor (`crate::editor`) over
//! the frozen frame, its toolbar docked where the bottom bar is (the host
//! places it: same anchors, same drag).
//! Add capture (board): the editor steps aside and an area is dragged on the
//! frozen frame; its crop joins the board.
use iced::mouse;
use iced::widget::{container, image, mouse_area, pin, stack};
use iced::{ContentFit, Element, Fill, Point, Rectangle, Size};

pub mod canvas;
mod picker;
pub mod toolbar;

use crate::actions::Crop;
use crate::editor::{self, Editor, Image, Message, Output};
use crate::ui::screens::bar::geometry::{BOTTOM_GAP, GAP};
use crate::ui::widgets;

/// The hint's distance from the edge.
const HINT_PAD: f32 = 24.0;
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

    /// Scaled down to fit the work area `screen` (the screen above the
    /// taskbar), with room for the toolbar docked under it (a scrolling capture).
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

    /// The toolbar's size on a `screen` this wide.
    pub fn toolbar_size(screen: f32) -> Size {
        toolbar_size(screen - 2.0 * GAP)
    }

    /// The canvas over its place and the toolbar with its top-left at
    /// `toolbar` (`None`: not shown, e.g. while the bar turns into it); a press
    /// on the toolbar outside its buttons sends `grab` (drags it, as the bar).
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        screen: Size,
        toolbar: Option<Point>,
        map: fn(editor::Message) -> M,
        grab: M,
    ) -> Element<'a, M> {
        if let Some(drag) = self.picking {
            let hint = container(widgets::pill(PICK_HINT))
                .center_x(Fill)
                .padding(HINT_PAD);
            return Element::from(stack![
                iced::widget::canvas(picker::Picker { drag })
                    .width(Fill)
                    .height(Fill),
                hint,
            ])
            .map(map);
        }
        let canvas = self.canvas_rect();
        let mut layers = stack![
            pin(Element::from(canvas::canvas(&self.editor)).map(map)).position(canvas.position())
        ];
        if let Some(at) = toolbar {
            let room = screen.width - 2.0 * GAP;
            let size = toolbar_size(room);
            let (toolbar, popover) = toolbar::view_at(&self.editor, room, Rectangle::new(at, size));
            let toolbar = mouse_area(toolbar.map(map))
                .on_press(grab)
                .interaction(mouse::Interaction::Grab);
            layers = layers
                .push(pin(toolbar).position(at))
                .extend(popover.map(|p| p.map(map)));
        }
        layers.into()
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

/// The logical rectangle the frame pixels `[x0, y0, x1, y1]` cover.
fn on_screen([x0, y0, x1, y1]: [u32; 4], scale: f32) -> Rectangle {
    let px = |v: u32| v as f32 / scale;
    Rectangle::new(
        Point::new(px(x0), px(y0)),
        Size::new(px(x1 - x0), px(y1 - y0)),
    )
}

use toolbar::size as toolbar_size;

/// Where an image of `size` px goes on the work area `screen` and its zoom (at
/// most `max_zoom`): centred, scaled down to leave room for the toolbar docked
/// under it (the bar's bottom spot).
fn fit(size: Size, screen: Size, max_zoom: f32) -> (Point, f32) {
    let toolbar = toolbar_size(screen.width - 2.0 * GAP).height;
    let room = Size::new(
        screen.width - 2.0 * GAP,
        screen.height - 2.0 * GAP - BOTTOM_GAP - toolbar,
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
            at.y >= GAP
                && at.y + 6000.0 * zoom + GAP + bar.height <= screen.height - BOTTOM_GAP + 1e-3
        );
        // Small: never enlarged past its on-screen size (150 % → zoom 1 / 1.5).
        let (_, zoom) = fit(Size::new(400.0, 300.0), screen, 1.0 / 1.5);
        assert_eq!(zoom, 1.0 / 1.5);
        // The toolbar fits a 1920 px screen.
        assert!(bar.width < 1920.0 - 2.0 * GAP, "{bar:?}");
    }
}
