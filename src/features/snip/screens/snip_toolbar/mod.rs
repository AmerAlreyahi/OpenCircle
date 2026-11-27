//! The Snip toolbar and its canvas: the annotation editor (`crate::features::snip::annotate`) over
//! the frozen frame, its toolbar docked where the bottom bar is (the host
//! places it: same anchors, same drag).
use crate::t;
use iced::mouse;
use iced::widget::{container, image, mouse_area, pin, stack};
use iced::{ContentFit, Element, Point, Rectangle, Size};

pub mod canvas;
pub mod toolbar;

use crate::features::circle::screens::bar::geometry::{BOTTOM_GAP, GAP};
use crate::features::snip::annotate::{self, Annotator, Image, Message, Output};
use crate::shared::kit::chip;
use crate::shared::kit::key_hint::{self, key_hint};
use crate::shared::kit::surface::Join;
use crate::shared::picture::Crop;

/// Snip's keyboard hint (after an arrow key) and its position readout's width.
fn nudge_hint() -> [key_hint::Group<'static>; 5] {
    [
        (Some("← ↑ → ↓"), t!(sn_nudge_px)),
        (Some("Shift"), t!(sn_nudge_10)),
        (Some("Ctrl"), t!(sn_nudge_resize)),
        (Some("Enter"), t!(sn_nudge_copy)),
        (Some("Esc"), t!(sn_nudge_back)),
    ]
}
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
        use crate::shared::kit::motion;
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
                // an arrow in the options row: a Tab step (Left / Up back; mirrored in Arabic)
                Message::RowStep(dx, dy) => {
                    let back = match dx {
                        0 => dy < 0,
                        _ => (dx < 0) != crate::shared::kit::dir::rtl(),
                    };
                    return self.update(Message::Focus(back));
                }
                Message::Focus(back) => {
                    let n = toolbar::stops(&self.annotator).len();
                    let next = match (self.annotator.focus, back) {
                        (None, false) => 0,
                        (None, true) => n - 1,
                        (Some(i), false) => (i + 1) % n,
                        (Some(i), true) => (i + n - 1) % n,
                    };
                    self.annotator.focus = Some(next);
                    // R-10: Tab onto Pin opens its row (sticky, like the pointer); its two cells
                    // are the next stops
                    if toolbar::stops(&self.annotator).get(next) == Some(&Message::PinButton) {
                        self.annotator
                            .open_pin_row(crate::features::snip::capture::live::offered());
                    }
                    (self.annotator.row_at, self.annotator.row_end) =
                        toolbar::row_stops_span(&self.annotator);
                    return None;
                }
                // More's rows: the arrow keys walk the live ones
                Message::MenuStep(back) => {
                    let rows = toolbar::more_rows(&self.annotator);
                    let live: Vec<usize> = (0..rows.len())
                        .filter(|&i| rows[i].message.is_some())
                        .collect();
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
                Message::MenuEdge(last) => {
                    let rows = toolbar::more_rows(&self.annotator);
                    let mut live = (0..rows.len()).filter(|&i| rows[i].message.is_some());
                    self.annotator.row = if last { live.next_back() } else { live.next() };
                    return None;
                }
                Message::Activate if self.annotator.menu == Some(annotate::Menu::More) => {
                    self.take_row()?
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

    /// Enter in More: closes the menu and gives the keyboard's row's message
    /// (`None`: no row, or a dimmed one).
    pub fn take_row(&mut self) -> Option<Message> {
        let rows = toolbar::more_rows(&self.annotator);
        let row = self.annotator.row.and_then(|i| rows.into_iter().nth(i));
        self.annotator.menu = None;
        row?.message
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

    /// What the toolbar docks with: the toolbar and the room under it for the
    /// options row (UX C4), always the same, so the toolbar never jumps when a
    /// tool has no options.
    pub fn dock_size(&self) -> Size {
        let bar = self.bar_size();
        Size::new(bar.width, bar.height + toolbar::ROW_ROOM)
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
            let hint = key_hint(&nudge_hint());
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
            // the options row (C4, W107): one piece with the toolbar, centred on it (the same right to
            // left), shifted inward only to stay inside the screen's margins
            let (x0, x1) = toolbar::row_span(
                at.x,
                size.width,
                screen.width,
                toolbar::row_need(&self.annotator),
            );
            // (the row's tooltips open away from the toolbar: down under it, up above it)
            let row_above_bar = row_above(at.y, size.height, toolbar::ROW_ROOM, screen.height);
            let join = join_of(row_above_bar);
            let row =
                toolbar::options_row_tips(&self.annotator, x1 - x0, row_tips(row_above_bar), join);
            // (no row: the toolbar's edges stay round)
            let join = if row.is_some() { join } else { Join::None };
            let room = if row.is_some() {
                toolbar::ROW_ROOM
            } else {
                0.0
            };
            // under the toolbar; above it when the screen has no room under it
            let above = row.is_some() && row_above_bar;
            // its tooltips and panels away from the area: above it when docked above
            let side = match at.y + size.height + room <= canvas.y {
                true => iced::widget::tooltip::Position::Top,
                false => iced::widget::tooltip::Position::Bottom,
            };
            // its tooltips never fall on the row: on the toolbar's other side
            let tips = toolbar_tips(row.is_some(), above, side);
            // (panels below clear the row)
            let held = Rectangle::new(at, Size::new(size.width, size.height + room));
            let (toolbar, popover) = toolbar::view_at(&self.annotator, held, side, tips, join);
            let toolbar = mouse_area(toolbar.map(map))
                .on_press(grab.clone())
                .interaction(mouse::Interaction::Move);
            layers = layers.push(pin(toolbar).position(at));
            if let Some(row) = row {
                let row = mouse_area(row.map(map))
                    .on_press(grab)
                    .interaction(mouse::Interaction::Move);
                // (only the row takes presses, not the strip around it)
                let row = container(row).center_x(x1 - x0);
                let y = row_y(at.y, size.height, above);
                layers = layers.push(pin(row).position(Point::new(x0, y)));
            }
            layers = layers.extend(popover.map(|p| p.map(map)));
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

/// The row goes above the toolbar when the screen has no room under it (`room`: the row's).
fn row_above(bar_y: f32, bar_h: f32, room: f32, screen_h: f32) -> bool {
    bar_y + bar_h + room > screen_h
}

/// The toolbar's edge the row is attached to (the row's own is the opposite one).
fn join_of(row_above_bar: bool) -> Join {
    match row_above_bar {
        true => Join::Above,
        false => Join::Below,
    }
}

/// The row's top: under the toolbar, or above it.
fn row_y(bar_y: f32, bar_h: f32, above: bool) -> f32 {
    match above {
        false => bar_y + bar_h + toolbar::ROW_GAP - toolbar::JOIN_OVERLAP,
        true => bar_y - toolbar::ROW_GAP - toolbar::ROW_H + toolbar::JOIN_OVERLAP,
    }
}

/// Where the row's tooltips open: away from the toolbar.
fn row_tips(above: bool) -> iced::widget::tooltip::Position {
    match above {
        true => iced::widget::tooltip::Position::Top,
        false => iced::widget::tooltip::Position::Bottom,
    }
}

/// Where the toolbar's tooltips open: never on its row, so on its other side; with no row, `side`.
fn toolbar_tips(
    row: bool,
    above: bool,
    side: iced::widget::tooltip::Position,
) -> iced::widget::tooltip::Position {
    use iced::widget::tooltip::Position::{Bottom, Top};
    match (row, above) {
        (true, false) => Top,
        (true, true) => Bottom,
        _ => side,
    }
}

/// Where an image of `size` px goes on the work area `screen` and its zoom (at
/// most `max_zoom`): centred, scaled down to leave room for Snip's toolbar
/// docked under it.
fn fit(size: Size, screen: Size, max_zoom: f32) -> (Point, f32) {
    let toolbar = toolbar::size().height + toolbar::ROW_ROOM;
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

    /// C4-6: the toolbar docks with the room of its options row under it, the
    /// same for every tool (it never jumps), and the row sits inside that room.
    #[test]
    fn c4_6_the_dock_has_room_for_the_row() {
        let mut e = SnipToolbar::fitted(crop(40, 30), Size::new(1920.0, 1080.0), 1.0);
        let room = e.dock_size();
        assert_eq!(room.height, e.bar_size().height + toolbar::ROW_ROOM);
        e.annotator.tool = annotate::Tool::Pen;
        assert_eq!(e.dock_size(), room, "the same for every tool");
        // a picture is fitted with the row's room too
        let (at, zoom) = fit(Size::new(780.0, 6000.0), Size::new(1920.0, 1080.0), 1.0);
        assert!(at.y + 6000.0 * zoom + GAP + room.height <= 1080.0 - BOTTOM_GAP + 1e-3);
    }

    /// C4-7: on a 1280 x 720 screen Hide's row (the widest) is lined up with the toolbar and reaches no
    /// further than the screen's margin, whether the toolbar is at the left or at the right edge (it moves
    /// over to fit); right to left the same mirrored. Goldens show it.
    #[test]
    fn c4_7_hides_row_fits_a_1280_screen_wherever_the_toolbar_is() {
        use crate::shared::kit::testing::golden;
        let screen = Size::new(1280.0, 720.0);
        let mut failed = Vec::new();
        for rtl in [false, true] {
            let _ar = rtl.then(crate::shared::kit::testing::arabic);
            for (name, x) in [("left", 64.0), ("right", 1280.0 - 16.0 - 676.0)] {
                let mut e = SnipToolbar::fitted(crop(300, 200), screen, 1.0);
                e.annotator
                    .update(annotate::Message::Tool(annotate::Tool::Blur));
                let at = Point::new(x, 560.0);
                let dir = if rtl { "-ar" } else { "" };
                let file = format!("snip-row-fit-{name}{dir}-sapphire");
                failed.extend(golden::check("screens", &file, screen, &[1.0], || {
                    e.view(
                        screen,
                        Some(at),
                        canvas::ScreenMap::ONE,
                        |m| m,
                        annotate::Message::Grab,
                    )
                }));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// W107-1: the toolbar and its options row as the user sees them (goldens: Shapes, Pen, Hide;
    /// English and Arabic, both themes), the toolbar centred near the bottom as in the owner's shot.
    #[test]
    fn w107_1_toolbar_and_row_goldens() {
        use crate::shared::kit::testing::golden;
        use crate::shared::kit::themes::{self, DAYLIGHT, SAPPHIRE};
        let mut failed = Vec::new();
        // (a 1600 px screen, and a 1280 px one: every row fits it, W107-5)
        for (screen, tag) in [
            (Size::new(1600.0, 420.0), ""),
            (Size::new(1280.0, 420.0), "1280-"),
        ] {
            for theme in [&SAPPHIRE, &DAYLIGHT] {
                themes::set(theme);
                for rtl in [false, true] {
                    let _ar = rtl.then(crate::shared::kit::testing::arabic);
                    for (name, tool) in [
                        ("shapes", annotate::Tool::Rect),
                        ("pen", annotate::Tool::Pen),
                        ("text", annotate::Tool::Text),
                        ("hide", annotate::Tool::Blur),
                    ] {
                        if tag.is_empty() && name == "text" {
                            continue;
                        }
                        let mut e = SnipToolbar::fitted(crop(300, 200), screen, 1.0);
                        e.annotator.update(annotate::Message::Tool(tool));
                        let bar = e.bar_size();
                        let at = Point::new((screen.width - bar.width) / 2.0, 320.0 - bar.height);
                        let dir = if rtl { "-ar" } else { "" };
                        // owner 2026-11-27: the same toolbar at the screen's bottom, the row ABOVE it
                        // (joined along the toolbar's top edge)
                        if tag.is_empty() && name == "shapes" {
                            let low = Point::new(at.x, screen.height - bar.height - 4.0);
                            assert!(row_above(
                                low.y,
                                bar.height,
                                toolbar::ROW_ROOM,
                                screen.height
                            ));
                            let file = format!("snip-dock-above-{name}{dir}-{}", theme.id);
                            failed.extend(golden::check("screens", &file, screen, &[1.0], || {
                                e.view(
                                    screen,
                                    Some(low),
                                    canvas::ScreenMap::ONE,
                                    |m| m,
                                    annotate::Message::Grab,
                                )
                            }));
                        }
                        let file = format!("snip-dock-{tag}{name}{dir}-{}", theme.id);
                        failed.extend(golden::check("screens", &file, screen, &[1.0], || {
                            e.view(
                                screen,
                                Some(at),
                                canvas::ScreenMap::ONE,
                                |m| m,
                                annotate::Message::Grab,
                            )
                        }));
                    }
                }
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// Owner 2026-11-27: the toolbar and its row are one piece, in both directions: the joined edge of
    /// each tray is nearly square, the far edges stay round.
    #[test]
    fn the_row_is_joined_to_the_toolbar_below_and_above() {
        use crate::shared::kit::surface::Join;
        assert_eq!(
            join_of(false),
            Join::Below,
            "row under: the toolbar's bottom edge"
        );
        assert_eq!(
            join_of(true),
            Join::Above,
            "row over: the toolbar's top edge"
        );
        assert_eq!(join_of(false).facing(), Join::Above, "the row's top edge");
        assert_eq!(join_of(true).facing(), Join::Below, "the row's bottom edge");
    }

    /// W107 fix 2: the row goes ABOVE the toolbar when the toolbar is near the screen's bottom (the row's
    /// top, the tooltips' side), under it otherwise; in the drawn view the row is where the rule says.
    #[test]
    fn w107_the_row_goes_above_a_toolbar_near_the_screen_bottom() {
        use iced::widget::tooltip::Position::{Bottom, Top};
        let room = toolbar::ROW_ROOM;
        let bar_h = toolbar::size().height;
        assert!(!row_above(300.0, bar_h, room, 720.0));
        assert!(row_above(720.0 - bar_h - 4.0, bar_h, room, 720.0));
        assert_eq!(
            row_y(300.0, bar_h, false),
            300.0 + bar_h - toolbar::JOIN_OVERLAP
        );
        assert!(row_y(600.0, bar_h, true) + toolbar::ROW_H <= 600.0 + toolbar::JOIN_OVERLAP);
        // the tooltips go to the other side of the row; with no row, where they were
        assert_eq!(toolbar_tips(true, false, Bottom), Top);
        assert_eq!(toolbar_tips(true, true, Top), Bottom);
        assert_eq!(toolbar_tips(false, false, Bottom), Bottom);
        // the row's own tooltips: down under the toolbar, up above it; the row builds with the host's side
        assert_eq!((row_tips(false), row_tips(true)), (Bottom, Top));
        let e = SnipToolbar::fitted(crop(300, 200), Size::new(1280.0, 720.0), 1.0);
        let _ = toolbar::options_row_tips(&e.annotator, 900.0, Top, Join::None);
        assert_eq!(crate::shared::kit::option_bar::tips(), Top);
        // as drawn: the row's name sits under the toolbar, or above it near the bottom
        let screen = Size::new(1280.0, 720.0);
        for (y, above) in [(400.0, false), (720.0 - bar_h - 4.0, true)] {
            let mut e = SnipToolbar::fitted(crop(300, 200), screen, 1.0);
            e.annotator
                .update(annotate::Message::Tool(annotate::Tool::Rect));
            let at = Point::new(300.0, y);
            let view = e.view(
                screen,
                Some(at),
                canvas::ScreenMap::ONE,
                |m| m,
                annotate::Message::Grab,
            );
            let name = crate::shared::kit::testing::shot::texts_at(view, screen)
                .into_iter()
                .find(|(_, t)| t == "Rectangle")
                .expect("the row's tool name")
                .0;
            assert_eq!(name.y < y, above, "toolbar at {y}: row text at {}", name.y);
        }
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
