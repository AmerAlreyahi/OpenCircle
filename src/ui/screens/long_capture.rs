//! Snip's long capture (N-73 point 5; look: owner, 2026-10-31): the picked
//! area stays outlined (dashed) and is cut out of the overlay, so the user
//! scrolls the app under it by hand; Snip's own toolbar stays docked to it
//! (its Copy / Save / Pin / Share finish the capture, close cancels it). Beside
//! the area the stitched image itself grows, sharp (`capture::shrink`) and
//! drawn 1:1 in physical px, with only a 1 px edge (no card, no title): top
//! aligned with the area, at most ~80 % of the free height, pinned to its
//! newest rows; its px count is a small chip under it (over it where the
//! toolbar is). A calm hint near the
//! area until the first scroll. The grabbing and stitching run on a thread
//! (`capture::scroll::by_hand`).
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::widget::canvas::{self, Frame, Geometry};
use iced::widget::{canvas as draw, container, image, pin, stack};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::capture::scroll::Update;
use crate::ui::kit::area;
use crate::ui::kit::chip;
use crate::ui::kit::key_hint::{self, key_hint};
use crate::ui::snip::Msg;

/// Between the area, the picture and the screen's edge (logical px).
const GAP: f32 = 12.0;
/// The picture's width: this share of the area's (readable text, about half
/// size), at most `MAX_SHARE` of the screen, at least `MIN_W` (else none).
const SCALE: f32 = 0.5;
const MAX_SHARE: f32 = 0.3;
const MIN_W: f32 = 120.0;
/// Its tallest: this share of the free height under the area's top (a little
/// shorter than the whole free height, owner 2026-10-31), at least `MIN_H`.
const TALL: f32 = 0.8;
const MIN_H: f32 = 80.0;
/// The count chip under the picture.
const CHIP_GAP: f32 = 6.0;

/// How the capture ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    /// Esc / close: back to the picked area.
    Cancel,
    /// Enter / Copy, Save, Pin, Share: that, on the stitched image.
    Copy,
    Save,
    Pin,
    Share,
}

/// The running capture (`Snip::scroll`).
pub struct Long {
    stop: Arc<AtomicBool>,
    /// How it ends (the first finish asked for wins); the thread then hands
    /// over the image.
    pub then: Option<Finish>,
    /// The area, logical px on the overlay's screen.
    pub area: Rectangle,
    /// The overlay's scale: the preview is drawn 1:1 in physical px.
    scale: f32,
    height: u32,
    /// The first frame's height: more = the page was scrolled.
    first: Option<u32>,
    full: bool,
    lost: bool,
    preview: Option<(image::Handle, u32, u32)>,
}

impl Long {
    /// Capturing `area`; `stop` is shared with the grabbing thread.
    pub fn new(area: Rectangle, stop: Arc<AtomicBool>, scale: f32) -> Self {
        Long {
            stop,
            then: None,
            area,
            scale,
            height: 0,
            first: None,
            full: false,
            lost: false,
            preview: None,
        }
    }

    /// Ends the grabbing, then `how` (unless an end was already asked for).
    pub fn finish(&mut self, how: Finish) {
        self.then.get_or_insert(how);
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Ends the grabbing as it is (the overlay closes).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.then == Some(Finish::Cancel)
    }

    /// The page moved under the area (the hint goes).
    pub fn scrolled(&self) -> bool {
        self.first.is_some_and(|f| self.height > f)
    }

    pub fn update(&mut self, u: Update) {
        (self.height, self.full, self.lost) = (u.height, u.full, u.lost);
        self.first.get_or_insert(u.height);
        if let Some(p) = u.preview.filter(|p| p.width > 0 && p.height > 0) {
            let handle = image::Handle::from_rgba(p.width, p.height, p.rgba);
            self.preview = Some((handle, p.width, p.height));
        }
    }

    /// The outline, the picture with its count and the hint on a `screen`
    /// big overlay whose toolbar is at `bar`; `look` = the active look's
    /// pair, for the outline. `rtl`: the count's corner and wording mirrored.
    pub fn view(
        &self,
        screen: Size,
        bar: Rectangle,
        look: (Color, Color),
        rtl: bool,
    ) -> Element<'_, Msg> {
        let room = layout(self.area, screen, bar, self.scale);
        // the picture as drawn: its physical px at 1 / scale, from the top
        let shown = room
            .zip(self.preview.as_ref())
            .map(|(room, (handle, w, h))| {
                let size = Size::new(*w as f32 / self.scale, *h as f32 / self.scale);
                (Rectangle::new(room.position(), size), handle.clone())
            });
        let outline = Outline {
            area: self.area,
            look,
            solid: self.full,
            picture: shown.as_ref().map(|(r, _)| *r),
        };
        let mut layers = stack![draw(outline).width(Fill).height(Fill)];
        if let Some((r, handle)) = shown {
            let picture = image(handle)
                .width(r.width)
                .height(r.height)
                .filter_method(image::FilterMethod::Nearest);
            layers = layers.push(pin(picture).position(r.position()));
            let chip = container(count(self.height, self.full, self.lost, rtl)).width(r.width);
            let chip = match rtl {
                true => chip.align_left(r.width),
                false => chip.align_right(r.width),
            };
            // under it; above it where the toolbar or the screen's end is
            let under = Rectangle::new(
                Point::new(r.x, r.y + r.height + CHIP_GAP),
                Size::new(r.width, chip::READOUT_H),
            );
            let fits = !under.intersects(&bar) && under.y + under.height <= screen.height;
            let y = match fits {
                true => under.y,
                false => r.y - CHIP_GAP - chip::READOUT_H,
            };
            if y >= 0.0 {
                layers = layers.push(pin(chip).position(Point::new(r.x, y)));
            }
        }
        if !self.scrolled() {
            let hint = hint(rtl);
            let spot = hint_at(bar, self.area, key_hint::width(&hint), screen);
            if let Some(spot) = spot {
                layers = layers.push(pin(key_hint(&hint, rtl)).position(spot));
            }
        }
        layers.into()
    }
}

/// The hint until the first scroll.
fn hint(rtl: bool) -> [key_hint::Group<'static>; 1] {
    match rtl {
        true => [(None, "مرّر لالتقاط الصفحة")],
        false => [(None, "Scroll to capture the page")],
    }
}

/// The count chip: `3,240 px`, with "Limit reached" / "Scroll back".
pub fn count<'a>(height: u32, full: bool, lost: bool, rtl: bool) -> Element<'a, Msg> {
    let ar = |latin: &'a str, arabic: &'a str| if rtl { arabic } else { latin };
    let note = match () {
        _ if full => Some(ar("Limit reached", "بلغت الحد")),
        _ if lost => Some(ar("Scroll back", "مرّر للخلف")),
        _ => None,
    };
    let label = match rtl {
        true => format!("{} بكسل", group(height)),
        false => px(height),
    };
    chip::readout(label, false, note)
}

/// Where the picture may grow (logical px, on the physical px grid): right
/// beside the area (`GAP`), on the side it fits (the roomier when both or
/// neither do; narrower to fit, down to `MIN_W`, then over the area's edge
/// by less than half its width), top-aligned with it, `SCALE` of its width,
/// `TALL` of the free height, ending above Snip's toolbar where they would
/// meet; none when that leaves less than `MIN_H` or a bigger overlap.
pub fn layout(area: Rectangle, screen: Size, bar: Rectangle, scale: f32) -> Option<Rectangle> {
    // room beside the area on each side (a GAP to the area and to the edge)
    let left = area.x - 2.0 * GAP;
    let right = screen.width - (area.x + area.width) - 2.0 * GAP;
    let want = (area.width * SCALE)
        .min(screen.width * MAX_SHARE)
        .max(MIN_W);
    // the side it fits on (the roomier one when both do), else the roomier
    // side, narrower; never across the screen, never past the toolbar
    let on_right = match (right >= want, left >= want) {
        (true, false) => true,
        (false, true) => false,
        _ => right >= left,
    };
    let room = if on_right { right } else { left };
    let width = want.min(room.max(MIN_W));
    // too little room even for MIN_W: overlap the area's edge a little
    let x = match on_right {
        true => (area.x + area.width + GAP).min(screen.width - GAP - width),
        false => (area.x - GAP - width).max(GAP),
    };
    let overlap = match on_right {
        true => area.x + area.width + GAP - x,
        false => x + width + GAP - area.x,
    };
    if overlap > width / 2.0 {
        return None;
    }
    let top = area.y.max(GAP);
    let mut height = (screen.height - GAP - top) * TALL;
    // Snip's toolbar reaching into that column: the picture ends above it
    let column = Rectangle::new(Point::new(x, top), Size::new(width, height));
    if column.intersects(&bar) && bar.y > top {
        height = height.min(bar.y - GAP - top);
    }
    if height < MIN_H {
        return None;
    }
    // whole physical px: the picture is drawn 1:1, never resampled
    let snap = |v: f32| (v * scale).round() / scale;
    let floor = |v: f32| (v * scale).floor() / scale;
    Some(Rectangle::new(
        Point::new(snap(x), snap(top)),
        Size::new(floor(width), floor(height)),
    ))
}

/// The hint beside the toolbar, on its side away from the area, `width`
/// wide; none where it would cover the area or leave the screen.
fn hint_at(bar: Rectangle, area: Rectangle, width: f32, screen: Size) -> Option<Point> {
    let h = key_hint::HEIGHT;
    let y = match bar.y >= area.y + area.height {
        true => bar.y + bar.height + 8.0,
        false => bar.y - 8.0 - h,
    };
    let x = (bar.center_x() - width / 2.0).clamp(GAP, (screen.width - GAP - width).max(GAP));
    let spot = Rectangle::new(Point::new(x, y), Size::new(width, h));
    let inside = y >= 0.0 && y + h <= screen.height;
    (inside && !spot.intersects(&area)).then_some(spot.position())
}

/// `3,240 px`.
fn px(height: u32) -> String {
    format!("{} px", group(height))
}

/// `3240` as `3,240`.
fn group(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The area's outline (dashed while capturing, solid at the limit; outside
/// the area: the live screen shows inside) and the picture's edge.
struct Outline {
    area: Rectangle,
    look: (Color, Color),
    solid: bool,
    picture: Option<Rectangle>,
}

impl canvas::Program<Msg> for Outline {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        match self.solid {
            true => area::outline(&mut frame, self.area.expand(area::LINE), self.look),
            false => area::dashed(&mut frame, self.area, self.look),
        }
        if let Some(r) = self.picture {
            area::edge(&mut frame, r);
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size::new(1280.0, 720.0);

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    /// A toolbar docked under `area`, centred, 600 x 46.
    fn bar_under(area: Rectangle) -> Rectangle {
        rect(
            area.center_x() - 300.0,
            area.y + area.height + 12.0,
            600.0,
            46.0,
        )
    }

    #[test]
    fn the_picture_takes_the_roomier_side_top_aligned_and_never_covers_the_area() {
        // area on the left: the picture on its right, half its width
        let area = rect(80.0, 120.0, 560.0, 300.0);
        let room = layout(area, SCREEN, bar_under(area), 1.0).unwrap();
        assert!(!room.intersects(&area) && room.x >= area.x + area.width);
        assert_eq!((room.width, room.y), (280.0, 120.0));
        // 80 % of the free height (no toolbar in the way)
        let far = rect(0.0, 700.0, 10.0, 10.0);
        let room = layout(area, SCREEN, far, 1.0).unwrap();
        assert_eq!(room.height, ((720.0 - 12.0 - 120.0) * 0.8f32).floor());
        // area on the right: the picture on its left
        let area = rect(700.0, 100.0, 500.0, 400.0);
        let room = layout(area, SCREEN, bar_under(area), 1.0).unwrap();
        assert!(room.x + room.width <= area.x);
        // the toolbar under the area reaching the picture's side: above it
        let area = rect(80.0, 120.0, 560.0, 300.0);
        let bar = bar_under(area);
        let room = layout(area, SCREEN, bar, 1.0).unwrap();
        assert!(!room.intersects(&bar), "{room:?} {bar:?}");
        // no side wide enough: none
        let area = rect(10.0, 50.0, 1200.0, 500.0);
        assert_eq!(layout(area, SCREEN, bar_under(area), 1.0), None);
        let full = rect(0.0, 0.0, 1280.0, 720.0);
        assert_eq!(layout(full, SCREEN, bar_under(full), 1.0), None);
    }

    /// The owner's VM (2026-10-31): an area at the right edge got its picture
    /// at the far left (past the wide toolbar). It goes right beside the area,
    /// `GAP` away, top-aligned, wherever the area is.
    #[test]
    fn the_picture_is_right_beside_the_area_at_the_edges_and_centre() {
        let screen = Size::new(1716.0, 1271.0);
        let wide_bar = |area: Rectangle| {
            rect(
                area.center_x() - 590.0,
                area.y + area.height + 12.0,
                1180.0,
                46.0,
            )
        };
        let room_for = |area: Rectangle| {
            let room = layout(area, screen, wide_bar(area), 1.0).unwrap();
            assert_eq!(room.y, area.y, "top-aligned: {room:?}");
            assert!(room.x >= 0.0 && room.x + room.width <= screen.width);
            room
        };
        // right edge (the VM's case): on its left, GAP away
        let area = rect(866.0, 150.0, 830.0, 400.0);
        let room = room_for(area);
        assert_eq!(room.x + room.width + GAP, area.x);
        // left edge: on its right
        let area = rect(20.0, 150.0, 830.0, 400.0);
        assert_eq!(room_for(area).x, area.x + area.width + GAP);
        // centre: the roomier side, adjacent
        let area = rect(600.0, 200.0, 500.0, 400.0);
        assert_eq!(room_for(area).x, area.x + area.width + GAP);
        // neither side fits the full width: narrower, still adjacent
        let area = rect(150.0, 100.0, 1416.0, 600.0);
        let room = room_for(area);
        assert_eq!(room.x, area.x + area.width + GAP);
        assert!(room.width >= MIN_W && !room.intersects(&area));
        // no room at all: over the area's edge, by less than half its width
        let area = rect(60.0, 100.0, 1560.0, 600.0);
        let room = room_for(area);
        let over = (area.x + area.width - room.x).max(0.0);
        assert!(over > 0.0 && over <= room.width / 2.0, "{room:?}");
    }

    #[test]
    fn the_picture_sits_on_whole_physical_pixels() {
        let area = rect(80.3, 120.7, 561.0, 300.0);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let room = layout(area, SCREEN, bar_under(area), scale).unwrap();
            for v in [room.x, room.y, room.width, room.height] {
                let phys = v * scale;
                assert!((phys - phys.round()).abs() < 1e-3, "{v} at {scale}");
            }
        }
    }

    #[test]
    fn the_hint_sits_beside_the_toolbar_away_from_the_area() {
        let area = rect(80.0, 120.0, 560.0, 400.0);
        let bar = bar_under(area);
        let spot = hint_at(bar, area, 300.0, SCREEN).unwrap();
        assert!(spot.y > bar.y, "under the toolbar under the area");
        // a full-screen area: the toolbar is over it; no hint over the page
        let full = rect(0.0, 0.0, 1280.0, 720.0);
        let bar = rect(340.0, 600.0, 600.0, 46.0);
        assert_eq!(hint_at(bar, full, 300.0, SCREEN), None);
    }

    #[test]
    fn counts_read_with_thousands() {
        assert_eq!(px(0), "0 px");
        assert_eq!(px(3240), "3,240 px");
        assert_eq!(px(20_000), "20,000 px");
        assert_eq!(group(1_234_567), "1,234,567");
        let texts = crate::ui::kit::testing::shot::texts(count(20_000, true, false, false));
        assert!(texts.iter().any(|t| t == "Limit reached"), "{texts:?}");
    }

    #[test]
    fn the_first_finish_wins_and_the_hint_goes_after_a_scroll() {
        let mut long = Long::new(rect(0.0, 0.0, 10.0, 10.0), Arc::default(), 1.0);
        let update = |height| Update {
            height,
            full: false,
            lost: false,
            preview: None,
        };
        long.update(update(300));
        assert!(!long.scrolled());
        long.update(update(420));
        assert!(long.scrolled());
        long.finish(Finish::Save);
        long.finish(Finish::Cancel);
        assert_eq!(long.then, Some(Finish::Save));
        assert!(long.stop.load(Ordering::Relaxed));
    }
}
