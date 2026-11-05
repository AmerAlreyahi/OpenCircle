//! The Snip toolbar and its canvas: the annotation editor (`crate::snip::annotate`) over
//! the frozen frame, its toolbar docked where the bottom bar is (the host
//! places it: same anchors, same drag).
use iced::mouse;
use iced::widget::{container, image, mouse_area, pin, stack};
use iced::{ContentFit, Element, Point, Rectangle, Size};

pub mod canvas;
pub mod toolbar;

use crate::actions::Crop;
use crate::snip::annotate::{self, Annotator, Image, Message, Output};
use crate::ui::kit::chip;
use crate::ui::kit::key_hint::{self, key_hint};
use crate::ui::screens::bar::geometry::{BOTTOM_GAP, GAP};

/// Snip's keyboard hint (after an arrow key) and its position readout's width.
const NUDGE_HINT: [key_hint::Group<'static>; 5] = [
    (Some("← ↑ → ↓"), "1 px"),
    (Some("Shift"), "10 px"),
    (Some("Ctrl"), "Resize"),
    (Some("Enter"), "Copy"),
    (Some("Esc"), "Back"),
];
const POSITION_W: f32 = 120.0;
/// Snip's handles scale in after the pick (the design's 120 ms).
const HANDLES_IN: std::time::Duration = std::time::Duration::from_millis(120);

pub struct SnipToolbar {
    pub annotator: Annotator,
    /// Screen position (logical px) of the image's top-left pixel.
    origin: Point,
    /// Display px per image px.
    zoom: f32,
    /// Back at the action bar: the flattened edits, shown over the selection.
    flat: Option<image::Handle>,
    /// Snip's handles scaling in after the pick: since when (the first frame)
    /// and how far (0..1).
    grow: (Option<std::time::Instant>, f32),
}

impl SnipToolbar {
    /// Snip's live area: the whole frozen monitor `frame` (physical px), or
    /// every monitor's frame joined (desktop px), its top-left pixel shown at
    /// `origin` (logical px), with the frame pixels `bounds` as the area, which
    /// then moves and resizes (`annotate::area`), across screens too; it starts
    /// with Select, so a drag inside moves it until a tool is picked.
    pub fn live(frame: Image, bounds: [u32; 4], scale: f32, origin: Point) -> Self {
        let mut e = Self::new_image(frame, origin, 1.0 / scale);
        let [x0, y0, x1, y1] = bounds;
        let px = |v: u32| v as f32;
        e.annotator.doc.crop = Some(Rectangle::new(
            Point::new(px(x0), px(y0)),
            Size::new(px(x1 - x0), px(y1 - y0)),
        ));
        e.annotator.movable = true;
        e.annotator.tool = annotate::Tool::Select;
        e.grow = (None, 0.0);
        e.annotator.refresh();
        e
    }

    /// A capture of its own (a stitched long capture, a recent capture, a
    /// toast's across screens) in Snip's picked state: the live area is the
    /// whole image (its handles cut it down), with Snip's toolbar docked to it.
    /// Shown whole on the work area `screen` (the screen above the taskbar),
    /// scaled down to leave room for the toolbar under it, never past its
    /// on-screen size: an image taller than the screen is shown smaller, so
    /// every edge and handle is in reach (Copy and Save keep its full size).
    // ponytail: no pan / zoom in the editor; a very tall stitch shows small.
    pub fn fitted(crop: Crop, screen: Size, scale: f32) -> Self {
        let size = Size::new(crop.width as f32, crop.height as f32);
        let (origin, zoom) = fit(size, screen, 1.0 / scale);
        let bounds = [0, 0, crop.width, crop.height];
        let image = Image {
            width: crop.width,
            height: crop.height,
            rgba: crop.rgba,
        };
        let mut e = Self::live(image, bounds, 1.0 / zoom, origin);
        e.annotator.own_image = true; // (its base, the whole image, was painted on open)
        e
    }

    /// Refit an imported image when the compositor reports the window size.
    pub fn refit(&mut self, screen: Size, scale: f32) {
        let image = &self.annotator.doc.image;
        let size = Size::new(image.width as f32, image.height as f32);
        let (origin, zoom) = fit(size, screen, 1.0 / scale);
        self.origin = origin;
        self.zoom = zoom;
        self.annotator.zoom = zoom;
    }

    fn new_image(image: Image, origin: Point, zoom: f32) -> Self {
        SnipToolbar {
            annotator: Annotator::new(image, zoom),
            origin,
            zoom,
            flat: None,
            grow: (None, 1.0),
        }
    }

    /// Snip's handles scale in from the pick's first frame (120 ms; at once
    /// with reduced motion).
    pub fn tick(&mut self, now: std::time::Instant) {
        use crate::ui::kit::motion;
        let start = *self.grow.0.get_or_insert(now);
        if self.grow.1 < 1.0 {
            self.grow.1 = motion::progress(start, now, HANDLES_IN, motion::Curve::Standard);
        }
    }

    /// The handles are still scaling in.
    pub fn growing(&self) -> bool {
        self.grow.1 < 1.0
    }

    /// The editor's messages.
    pub fn update(&mut self, message: Message) -> Option<Output> {
        {
            // Snip's toolbar: Tab walks it, Enter / Space press what it is on.
            let message = match message {
                Message::Focus(back) => {
                    let n = toolbar::stops(&self.annotator).len();
                    let next = match (self.annotator.focus, back) {
                        (None, false) => 0,
                        (None, true) => n - 1,
                        (Some(i), false) => (i + 1) % n,
                        (Some(i), true) => (i + n - 1) % n,
                    };
                    self.annotator.focus = Some(next);
                    return None;
                }
                // More's rows: the arrow keys walk the live ones
                Message::MenuStep(back) => {
                    let rows = toolbar::more_rows(&self.annotator);
                    let live: Vec<usize> =
                        (0..rows.len()).filter(|&i| rows[i].3.is_some()).collect();
                    let n = live.len();
                    if n > 0 {
                        let at = self
                            .annotator
                            .row
                            .and_then(|r| live.iter().position(|&i| i == r));
                        let next = match (at, back) {
                            (None, false) => 0,
                            (None, true) => n - 1,
                            (Some(i), false) => (i + 1) % n,
                            (Some(i), true) => (i + n - 1) % n,
                        };
                        self.annotator.row = Some(live[next]);
                    }
                    return None;
                }
                Message::Activate if self.annotator.menu == Some(annotate::Menu::More) => {
                    let rows = toolbar::more_rows(&self.annotator);
                    let row = self.annotator.row.and_then(|i| rows.into_iter().nth(i));
                    self.annotator.menu = None;
                    row?.3?
                }
                Message::Activate => {
                    let stops = toolbar::stops(&self.annotator);
                    self.annotator.focus.and_then(|i| stops.get(i))?.clone()
                }
                m => m,
            };
            self.annotator.update(message)
        }
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
        annotate::render(&self.annotator.doc)
    }

    /// Where the canvas is on screen, logical px (moves with the editor's crop).
    pub fn canvas_rect(&self) -> Rectangle {
        let view = self.annotator.doc.view();
        Rectangle::new(
            self.origin + iced::Vector::new(view.x, view.y) * self.zoom,
            self.annotator.size(),
        )
    }

    /// The toolbar's size (the same in every state)...
    pub fn toolbar_size() -> Size {
        toolbar::size()
    }

    /// ...but while the Steps strip is open: Copy's place holds "Add step N".
    pub fn bar_size(&self) -> Size {
        toolbar::size_for(&self.annotator)
    }

    /// The canvas over its place and the toolbar with its top-left at
    /// `toolbar` (`None`: not shown, e.g. while the bar turns into it, or on
    /// another screen); a press on the toolbar outside its buttons sends `grab`
    /// (drags it, as the bar). `lens`: the window it is drawn in (`ONE` = the
    /// starting screen, which alone shows the keyboard hints); `screen` = that
    /// window's size, `toolbar` in its px.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        screen: Size,
        toolbar: Option<Point>,
        lens: canvas::ScreenMap,
        map: fn(annotate::Message) -> M,
        grab: M,
    ) -> Element<'a, M> {
        let canvas = self.canvas_rect();
        let m = canvas::margin(&self.annotator);
        let at = lens.to_window(canvas.position() - iced::Vector::new(m, m));
        // the area in this window's px (another screen's: through its lens)
        let shown = Rectangle::new(lens.to_window(canvas.position()), canvas.size() * lens.k);
        let lens = canvas::ScreenMap {
            origin: self.origin,
            ..lens
        };
        // a long capture shows the live page there: the toolbar alone
        let mut layers = stack![];
        if !self.annotator.capturing {
            let drawn = canvas::canvas(&self.annotator, self.grow.1, lens);
            layers = layers.push(pin(Element::from(drawn).map(map)).position(at));
        }
        // Snip moved by the keyboard: its keys above the area, its position
        // (frame px) in its bottom-right corner, until the pointer is used;
        // on the screen its toolbar docks on, whichever that is.
        // ponytail: shown until the next press, not the design's 2 s / 800 ms fades.
        if toolbar.is_some() && self.annotator.movable && self.annotator.nudged {
            let canvas = shown;
            let hint = key_hint(&NUDGE_HINT, false);
            let y = (canvas.y - 8.0 - key_hint::HEIGHT).max(8.0);
            let hint = container(hint).center_x(screen.width);
            layers = layers.push(pin(hint).position(Point::new(0.0, y)));
            let view = self.annotator.doc.view();
            let label = format!("x {}  y {}", view.x.round(), view.y.round());
            let spot = Point::new(
                canvas.x + canvas.width - 8.0 - POSITION_W,
                canvas.y + canvas.height - 8.0 - chip::READOUT_H,
            );
            let readout = container(chip::readout(label, false, None))
                .align_right(POSITION_W)
                .width(POSITION_W);
            layers = layers.push(pin(readout).position(spot));
        }
        if let Some(at) = toolbar {
            let size = toolbar::size_for(&self.annotator);
            // its tooltips and panels away from the area: above it when docked above
            let side = match at.y + size.height <= canvas.y {
                true => iced::widget::tooltip::Position::Top,
                false => iced::widget::tooltip::Position::Bottom,
            };
            let (toolbar, popover) =
                toolbar::view_at(&self.annotator, Rectangle::new(at, size), side);
            let toolbar = mouse_area(toolbar.map(map))
                .on_press(grab)
                .interaction(mouse::Interaction::Move);
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

/// Where an image of `size` px goes on the work area `screen` and its zoom (at
/// most `max_zoom`): centred, scaled down to leave room for Snip's toolbar
/// docked under it.
fn fit(size: Size, screen: Size, max_zoom: f32) -> (Point, f32) {
    let toolbar = toolbar::size().height;
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

    fn crop(width: u32, height: u32) -> Crop {
        Crop {
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
        }
    }

    #[test]
    fn back_keeps_the_edits_and_shows_them_flattened() {
        let mut e = SnipToolbar::fitted(crop(40, 30), Size::new(1920.0, 1080.0), 1.0);
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
        let bar = toolbar::size();
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
