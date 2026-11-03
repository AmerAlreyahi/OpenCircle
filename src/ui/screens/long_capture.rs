//! Snip's long capture (N-73 point 5; look: owner, 2026-10-31): the picked
//! area stays outlined (dashed) and is cut out of the overlay, so the user
//! scrolls the app under it by hand; Snip's own toolbar stays docked to it
//! (its Copy / Save / Pin / Share finish the capture, close cancels it). Beside
//! the area the WHOLE stitched image so far shows as a narrow miniature (as
//! iTop's, a little narrower; owner, 2026-11-02): at most `MAX_W` wide, the
//! whole page fitted into ~80 % of the free height, so it narrows as the page
//! grows; sharp (`capture::shrink`), drawn 1:1 in physical px with only a
//! 1 px edge (no card, no title), top aligned with the area, its edge next
//! to the area's; its px count is a small chip under it (over it where the
//! toolbar is). No side with room (an area filling the screen): a small one
//! in the area's top-right corner, kept in the overlay's hole while the overlay
//! is kept out of the grabs (`corner`; owner, 2026-11-03). A calm hint near the
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
/// The picture's widest (the page's first screen; it narrows as the page
/// grows), and the least room beside the area it is shown in.
const MAX_W: f32 = 280.0;
const MIN_W: f32 = 120.0;
/// The small picture's widest, in the area's top-right corner when no side
/// has room (an area that fills the screen; like iTop, owner 2026-11-03).
const SMALL_W: f32 = 180.0;
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
    /// The toolbar was moved off the area for the capture (`bar_beside`):
    /// where it was dropped before (`None`: docked), restored after.
    pub bar_was: Option<Option<Point>>,
    /// The small picture's rect kept in the overlay's hole (`over_area`), as
    /// last cut.
    pub cut: Option<Rectangle>,
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
            bar_was: None,
            cut: None,
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

    /// Where the picture is drawn (logical px; none before its first frame or
    /// without room): its physical px at 1 / scale, at the top of its room,
    /// against the area's side of it (it grows away from the area).
    pub fn picture(&self, screen: Size, bar: Rectangle) -> Option<Rectangle> {
        let room = room(self.area, screen, bar, self.scale)?;
        let (_, w, h) = self.preview.as_ref()?;
        let size = Size::new(*w as f32 / self.scale, *h as f32 / self.scale);
        // (in the area's corner: against its right edge)
        let x = match room.x < self.area.x || room.intersects(&self.area) {
            true => room.x + room.width - size.width,
            false => room.x,
        };
        Some(Rectangle::new(Point::new(x, room.y), size))
    }

    /// The small picture over the area (`corner`), with its 1 px edge: what the
    /// overlay's hole keeps (the overlay is kept out of the grabs then); none
    /// beside the area.
    pub fn over_area(&self, screen: Size, bar: Rectangle) -> Option<Rectangle> {
        let r = self.picture(screen, bar)?;
        r.intersects(&self.area).then(|| r.expand(1.0))
    }

    /// Where the hint is drawn (none once scrolled, or without a free spot).
    pub fn hint_spot(&self, screen: Size, bar: Rectangle, rtl: bool) -> Option<Point> {
        let width = key_hint::width(&hint(rtl));
        let picture = self.picture(screen, bar);
        (!self.scrolled() && !self.lost).then(|| hint_at(bar, self.area, picture, width, screen))?
    }

    /// Where the gap note goes while the page jumped too far to match
    /// (owner, 2026-11-03: a scrollbar drag, Page Down, End): centred on the
    /// area, right above its edge, else right under it; clear of the toolbar
    /// and the picture. None where neither fits: nothing of ours goes over the
    /// area while capturing (the count chip still says "Scroll back").
    pub fn note_spot(&self, screen: Size, bar: Rectangle, rtl: bool) -> Option<Point> {
        if !self.lost || self.full {
            return None;
        }
        let (width, h) = (key_hint::width(&note(rtl)), key_hint::HEIGHT);
        let x =
            (self.area.center_x() - width / 2.0).clamp(GAP, (screen.width - GAP - width).max(GAP));
        let picture = self.picture(screen, bar);
        let (top, bottom) = (self.area.y, self.area.y + self.area.height);
        [top - 8.0 - h, bottom + 8.0]
            .into_iter()
            .map(|y| Point::new(x, y))
            .find(|p| {
                let spot = Rectangle::new(*p, Size::new(width, h));
                p.y >= 0.0
                    && p.y + h <= screen.height
                    && !spot.intersects(&self.area)
                    && !spot.intersects(&bar)
                    && picture.is_none_or(|r| !spot.intersects(&r))
            })
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
        let shown = self
            .picture(screen, bar)
            .zip(self.preview.as_ref())
            .map(|(r, (handle, ..))| (r, handle.clone()));
        let picture = shown.as_ref().map(|(r, _)| *r);
        let outline = Outline {
            area: self.area,
            look,
            solid: self.full,
            picture,
        };
        let mut layers = stack![draw(outline).width(Fill).height(Fill)];
        if let Some((r, handle)) = shown {
            let picture = image(handle)
                .width(r.width)
                .height(r.height)
                .filter_method(image::FilterMethod::Nearest);
            layers = layers.push(pin(picture).position(r.position()));
            // (wider than a narrow picture, against the area's side as it is)
            let left = r.x < self.area.x;
            let cw = r.width.max(MAX_W);
            let cx = match left {
                true => (r.x + r.width - cw).max(0.0),
                false => r.x,
            };
            let chip = container(count(self.height, self.full, self.lost, rtl)).width(cw);
            let chip = match left {
                true => chip.align_right(cw),
                false => chip.align_left(cw),
            };
            // under it; above it where the toolbar or the screen's end is;
            // over the area: on the picture's top (only the picture is kept
            // in the hole, the chip under it would sit on the frozen frame)
            let under = Rectangle::new(
                Point::new(r.x, r.y + r.height + CHIP_GAP),
                Size::new(r.width, chip::READOUT_H),
            );
            let fits = !under.intersects(&bar) && under.y + under.height <= screen.height;
            let (x, y) = match () {
                _ if r.intersects(&self.area) => (r.x + CHIP_GAP, r.y + CHIP_GAP),
                _ if fits => (cx, under.y),
                _ => (cx, r.y - CHIP_GAP - chip::READOUT_H),
            };
            if y >= 0.0 {
                layers = layers.push(pin(chip).position(Point::new(x, y)));
            }
        }
        if let Some(spot) = self.hint_spot(screen, bar, rtl) {
            layers = layers.push(pin(key_hint(&hint(rtl), rtl)).position(spot));
        }
        if let Some(spot) = self.note_spot(screen, bar, rtl) {
            layers = layers.push(pin(key_hint(&note(rtl), rtl)).position(spot));
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

/// The gap note while the page jumped too far to match.
fn note(rtl: bool) -> [key_hint::Group<'static>; 1] {
    match rtl {
        true => [(None, "قفزت الصفحة: مرّر للخلف قليلًا")],
        false => [(None, "The page jumped: scroll back a little")],
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
/// by less than half its width), top-aligned with it, `MAX_W` wide, `TALL`
/// of the free height (`screen`: the work area, above the taskbar), ending
/// above Snip's toolbar where they would
/// meet (or starting under it, where above leaves nothing); none when that leaves
/// less than `MIN_H` or a bigger overlap.
pub fn layout(area: Rectangle, screen: Size, bar: Rectangle, scale: f32) -> Option<Rectangle> {
    // room beside the area on each side (a GAP to the area and to the edge)
    let left = area.x - 2.0 * GAP;
    let right = screen.width - (area.x + area.width) - 2.0 * GAP;
    let want = MAX_W;
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
    let mut top = area.y.max(GAP);
    let tall = |top: f32| (screen.height - GAP - top) * TALL;
    let mut height = tall(top);
    // Snip's toolbar reaching into that column: the picture ends above it,
    // or starts under it when that leaves nothing (a toolbar docked inside
    // the area's top edge, as over a full-height window: owner's VM, 2026-10-31)
    let column = Rectangle::new(Point::new(x, top), Size::new(width, height));
    if column.intersects(&bar) {
        let above = height.min(bar.y - GAP - top);
        let under = bar.y + bar.height + GAP;
        (top, height) = match above >= MIN_H {
            true => (top, above),
            false => (under, tall(under)),
        };
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

/// Where the picture grows: beside the area (`layout`), else small in the
/// area's top-right corner (`corner`).
pub fn room(area: Rectangle, screen: Size, bar: Rectangle, scale: f32) -> Option<Rectangle> {
    layout(area, screen, bar, scale).or_else(|| corner(area, bar, scale))
}

/// Where the small picture grows when no side has room (an area that fills
/// the screen; owner, 2026-11-03, like iTop): inside the area's top-right
/// corner, `SMALL_W` wide, under Snip's toolbar where it reaches that corner,
/// `TALL` of the height left in the area; none under `MIN_H`.
pub fn corner(area: Rectangle, bar: Rectangle, scale: f32) -> Option<Rectangle> {
    let width = SMALL_W.min(area.width - 2.0 * GAP);
    let x = area.x + area.width - GAP - width;
    let bottom = area.y + area.height - GAP;
    let mut top = area.y + GAP;
    let column = Rectangle::new(Point::new(x, top), Size::new(width, bottom - top));
    if column.intersects(&bar) && bar.y < top + MIN_H {
        top = bar.y + bar.height + GAP;
    }
    let height = (bottom - top) * TALL;
    if width < MIN_H || height < MIN_H {
        return None;
    }
    let snap = |v: f32| (v * scale).round() / scale;
    let floor = |v: f32| (v * scale).floor() / scale;
    Some(Rectangle::new(
        Point::new(snap(x), snap(top)),
        Size::new(floor(width), floor(height)),
    ))
}

/// Where Snip's toolbar (`size`) goes for the capture when it is docked over
/// the area (an area as tall as the screen): beside it, on the roomier side
/// (the picture's, which then starts under it), top-aligned with it; none
/// where neither side holds it. Over the area the overlay has to be kept out
/// of the grabs (`hide_from_capture`), and on the owner's Windows 11 PC the
/// area then showed blurred while it was captured (photo, 2026-11-03);
/// beside it the overlay stays a plain window.
pub fn bar_beside(area: Rectangle, screen: Size, size: Size) -> Option<Point> {
    let left = area.x - 2.0 * GAP;
    let right = screen.width - (area.x + area.width) - 2.0 * GAP;
    let x = match right >= left {
        true => area.x + area.width + GAP,
        false => area.x - GAP - size.width,
    };
    let y = area.y.min(screen.height - GAP - size.height).max(GAP);
    (right.max(left) >= size.width).then_some(Point::new(x, y))
}

/// The hint beside the toolbar, on its side away from the area, else under
/// the picture (`picture`, with its count chip), else above it; `width`
/// wide; none where every spot would cover the area, the toolbar or the
/// picture, or leave the screen.
fn hint_at(
    bar: Rectangle,
    area: Rectangle,
    picture: Option<Rectangle>,
    width: f32,
    screen: Size,
) -> Option<Point> {
    let h = key_hint::HEIGHT;
    let clamp = |x: f32| x.clamp(GAP, (screen.width - GAP - width).max(GAP));
    let beside_bar = match bar.y >= area.y + area.height {
        true => bar.y + bar.height + 8.0,
        false => bar.y - 8.0 - h,
    };
    let mut spots = vec![Point::new(clamp(bar.center_x() - width / 2.0), beside_bar)];
    if let Some(r) = picture {
        let x = clamp(r.center_x() - width / 2.0);
        let under = r.y + r.height + CHIP_GAP + chip::READOUT_H + 8.0;
        spots.extend([Point::new(x, under), Point::new(x, r.y - 8.0 - h)]);
    }
    spots.into_iter().find(|p| {
        let spot = Rectangle::new(*p, Size::new(width, h));
        let inside = p.y >= 0.0 && p.y + h <= screen.height;
        inside
            && !spot.intersects(&area)
            && !spot.intersects(&bar)
            && picture.is_none_or(|r| !spot.intersects(&r))
    })
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

    /// An area that fills the screen (no side has room): the small picture
    /// sits inside its top-right corner, under the toolbar where the toolbar
    /// reaches that corner, and is the only part of it kept in the hole.
    #[test]
    fn a_full_screen_area_gets_a_small_picture_in_its_top_right_corner() {
        let full = rect(0.0, 0.0, 1280.0, 680.0);
        // a toolbar docked inside the area's top edge, centred: clear of the corner
        let centred = rect(340.0, 12.0, 600.0, 46.0);
        assert_eq!(layout(full, SCREEN, centred, 1.0), None, "no side has room");
        let r = room(full, SCREEN, centred, 1.0).expect("the corner");
        assert_eq!((r.x + r.width, r.y, r.width), (1268.0, 12.0, 180.0));
        assert!(full.contains(r.position()) && r.y + r.height <= 680.0 - 12.0);
        // a toolbar reaching the top-right corner: the picture starts under it
        let right = rect(660.0, 12.0, 600.0, 46.0);
        let r = room(full, SCREEN, right, 1.0).unwrap();
        assert_eq!((r.x + r.width, r.y), (1268.0, 12.0 + 46.0 + 12.0));
        assert!(!r.intersects(&right) && full.contains(r.position()));
        // the picture against the corner's right edge; only it is kept in the hole
        let mut long = Long::new(full, Arc::new(AtomicBool::new(false)), 1.0);
        assert_eq!(
            long.over_area(SCREEN, right),
            None,
            "nothing before a frame"
        );
        long.update(Update {
            height: 680,
            full: false,
            lost: false,
            preview: Some(crate::actions::Crop {
                width: 160,
                height: 85,
                rgba: vec![9; 160 * 85 * 4],
            }),
        });
        let p = long.picture(SCREEN, right).unwrap();
        assert_eq!((p.x + p.width, p.y, p.width), (1268.0, 70.0, 160.0));
        assert_eq!(long.over_area(SCREEN, right), Some(p.expand(1.0)));
        // beside an area with room: nothing kept over it
        let area = rect(80.0, 120.0, 560.0, 300.0);
        let mut long = Long::new(area, Arc::new(AtomicBool::new(false)), 1.0);
        long.preview = Some((image::Handle::from_rgba(1, 1, vec![0; 4]), 100, 100));
        assert_eq!(long.over_area(SCREEN, bar_under(area)), None);
    }

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
        let spot = hint_at(bar, area, None, 300.0, SCREEN).unwrap();
        assert!(spot.y > bar.y, "under the toolbar under the area");
        // a full-screen area: the toolbar is over it; no hint over the page
        let full = rect(0.0, 0.0, 1280.0, 720.0);
        let bar = rect(340.0, 600.0, 600.0, 46.0);
        assert_eq!(hint_at(bar, full, None, 300.0, SCREEN), None);
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

    /// The owner's PC (2026-11-02): the picture filled half the screen. It is
    /// a narrow column now (at most 280 px), top-aligned, its edge next to the
    /// area's whichever side it is on, growing away from the area as the
    /// (whole-page) miniature narrows or widens.
    #[test]
    fn the_picture_is_a_narrow_column_against_the_area() {
        let screen = Size::new(1720.0, 1392.0); // half a 3440 x 1440 ultrawide's work area
        let far = rect(1700.0, 1380.0, 10.0, 10.0);
        let with = |area: Rectangle, w: u32, h: u32| {
            let mut long = Long::new(area, Arc::default(), 1.0);
            long.update(Update {
                height: h,
                full: false,
                lost: false,
                preview: Some(crate::actions::Crop {
                    width: w,
                    height: h,
                    rgba: vec![0; (w * h * 4) as usize],
                }),
            });
            long.picture(screen, far).unwrap()
        };
        // the area on the right half: the column on its left, its right
        // edge GAP from the area, top-aligned
        let area = rect(860.0, 0.0, 860.0, 1392.0);
        let room = layout(area, screen, far, 1.0).unwrap();
        assert_eq!(room.width, MAX_W);
        assert_eq!(room.height, ((1392.0 - 2.0 * GAP) * TALL).floor()); // from y GAP
        for w in [280, 250, 150] {
            let p = with(area, w, 1100);
            assert_eq!((p.x + p.width + GAP, p.y), (area.x, GAP), "{w}: {p:?}");
        }
        // on the left half: the column on its right, its left edge next to it
        let area = rect(0.0, 200.0, 860.0, 1000.0);
        for w in [280, 150] {
            let p = with(area, w, 700);
            assert_eq!(
                (p.x, p.y),
                (area.x + area.width + GAP, area.y),
                "{w}: {p:?}"
            );
        }
    }

    /// The toolbar off a full-height area: on the roomier side, top-aligned,
    /// `GAP` from it; none where no side holds it (it stays over the area).
    #[test]
    fn the_toolbar_goes_beside_a_full_height_area_where_it_fits() {
        let screen = Size::new(2293.0, 960.0); // 3440 x 1440 at 1.5
        let bar = Size::new(620.0, 52.0);
        // the owner's: the left half
        let left_half = rect(0.0, 0.0, 1152.0, 960.0);
        let at = bar_beside(left_half, screen, bar).expect("room on the right");
        assert_eq!((at.x, at.y), (1152.0 + GAP, GAP));
        // the right half: on its left
        let right_half = rect(1141.0, 0.0, 1152.0, 960.0);
        let at = bar_beside(right_half, screen, bar).expect("room on the left");
        assert_eq!(at.x + bar.width + GAP, right_half.x);
        // the whole screen, or too narrow a side: no spot
        assert_eq!(bar_beside(rect(0.0, 0.0, 2293.0, 960.0), screen, bar), None);
        assert_eq!(
            bar_beside(rect(300.0, 0.0, 1700.0, 960.0), screen, bar),
            None
        );
    }

    /// A jump too far (scrollbar drag, Page Down, End): a note right above
    /// the area (else under it), never over it; gone once matched again.
    #[test]
    fn a_jump_too_far_shows_a_note_beside_the_area_until_matched_again() {
        let area = rect(300.0, 150.0, 500.0, 400.0);
        let bar = bar_under(area);
        let mut long = Long::new(area, Arc::default(), 1.0);
        let update = |height, lost| Update {
            height,
            full: false,
            lost,
            preview: None,
        };
        long.update(update(400, false));
        assert_eq!(long.note_spot(SCREEN, bar, false), None);
        long.update(update(400, true));
        let at = long.note_spot(SCREEN, bar, false).expect("a note");
        let width = key_hint::width(&note(false));
        assert_eq!(
            at.y + key_hint::HEIGHT + 8.0,
            area.y,
            "right above the area"
        );
        assert_eq!(at.x + width / 2.0, area.center_x(), "centred on it");
        assert_eq!(
            long.hint_spot(SCREEN, bar, false),
            None,
            "one note at a time"
        );
        // an area at the screen's top: under it (the toolbar elsewhere)
        let high = rect(300.0, 0.0, 500.0, 400.0);
        let mut top = Long::new(high, Arc::default(), 1.0);
        top.update(update(400, true));
        let far = rect(0.0, 700.0, 10.0, 10.0);
        let at = top.note_spot(SCREEN, far, false).unwrap();
        assert_eq!(at.y, high.height + 8.0);
        // a full-screen area: no note over the page
        let mut full = Long::new(rect(0.0, 0.0, 1280.0, 720.0), Arc::default(), 1.0);
        full.update(update(720, true));
        assert_eq!(full.note_spot(SCREEN, far, false), None);
        // matched again: gone
        long.update(update(500, false));
        assert_eq!(long.note_spot(SCREEN, bar, false), None);
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
