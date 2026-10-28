//! Every monitor at once (UI plan step 41): the starting screen is the overlay
//! (`Opening`'s frame and view); each other monitor gets a `Screen`, its own
//! window over its own frozen frame.
//!
//! One unit for every screen: whatever the user draws (stroke, selection,
//! words, lines, OCR bands) is kept in the starting screen's image px, which
//! are desktop physical px moved to the starting screen's origin and divided by
//! its scale once (`to_desktop` / `from_desktop`). It is one affine map of the
//! desktop for all screens, so a stroke crossing screens of mixed scaling stays
//! one straight line, and on one screen nothing changes. Each screen converts
//! only to draw (`Screen::view`).

use std::cell::{Cell, RefCell};
use std::time::Instant;

use iced::widget::image;
use iced::{Point, Rectangle, Size, Vector, window};

use crate::ui::overlay;
use crate::ui::platform::monitor::Place;

/// Another monitor's overlay while the overlay is open.
pub(in crate::ui) struct Screen {
    /// Its overlay window.
    pub(in crate::ui) window: window::Id,
    /// Where the monitor sits (physical desktop px) and its scale.
    pub(in crate::ui) place: Place,
    /// Its frozen frame: what it shows (one image, `overlay::Frozen`) and what crops take.
    pub(in crate::ui) frame: image::Handle,
    /// The starting screen's image px <-> this window's logical px.
    pub(in crate::ui) view: overlay::View,
    /// Its own draw log, so the starting screen's frame budget stays its own.
    pub(in crate::ui) first_draw: Cell<Option<(Instant, Size)>>,
    pub(in crate::ui) frame_times: RefCell<Vec<Instant>>,
}

impl Screen {
    /// The monitor at `place` with its `frame`, beside the starting screen `start`.
    pub(in crate::ui) fn new(
        window: window::Id,
        start: Place,
        place: Place,
        frame: image::Handle,
    ) -> Screen {
        Screen {
            window,
            place,
            frame,
            view: view_of(start, place),
            first_draw: Cell::new(None),
            frame_times: RefCell::default(),
        }
    }
}

/// A point in the starting screen's image px (`start`) in desktop physical px.
pub(in crate::ui) fn to_desktop(start: Place, u: Point) -> Point {
    Point::new(
        start.origin.0 as f32 + u.x * start.scale,
        start.origin.1 as f32 + u.y * start.scale,
    )
}

/// A desktop physical px point in the starting screen's image px.
pub(in crate::ui) fn from_desktop(start: Place, d: Point) -> Point {
    Point::new(
        (d.x - start.origin.0 as f32) / start.scale,
        (d.y - start.origin.1 as f32) / start.scale,
    )
}

/// The monitor at `place` in the starting screen's image px.
pub(in crate::ui) fn area_of(start: Place, place: Place) -> Rectangle {
    let at = from_desktop(
        start,
        Point::new(place.origin.0 as f32, place.origin.1 as f32),
    );
    Rectangle::new(
        at,
        Size::new(place.px.0 as f32, place.px.1 as f32) * (1.0 / start.scale),
    )
}

/// How the window over the monitor at `place` shows the starting screen's
/// image px: `u` at `u * start.scale / place.scale + (start - place origin) /
/// place.scale` of its logical px, i.e. through the desktop px both share.
pub(in crate::ui) fn view_of(start: Place, place: Place) -> overlay::View {
    let k = start.scale / place.scale;
    let at = Vector::new(
        (start.origin.0 - place.origin.0) as f32 / place.scale,
        (start.origin.1 - place.origin.1) as f32 / place.scale,
    );
    overlay::View::mapped(place.size(), k, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(origin: (i32, i32), px: (u32, u32), scale: f32) -> Place {
        Place { origin, px, scale }
    }

    fn near(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
    }

    #[test]
    fn every_scale_maps_desktop_px_both_ways_through_one_unit() {
        // (scale, origin): every scale the owner's screens use, origins left of
        // and above the primary (negative) and right of it.
        let scales = [1.0, 1.25, 1.5, 2.0, 2.5];
        let origins = [(0, 0), (-1920, -150), (3840, 200), (-2560, 1080)];
        for &s1 in &scales {
            for &o1 in &origins {
                let start = place(o1, (3840, 2160), s1);
                for &s2 in &scales {
                    for &o2 in &origins {
                        let other = place(o2, (1920, 1080), s2);
                        let view = view_of(start, other);
                        for d in [
                            Point::new(o2.0 as f32, o2.1 as f32),
                            Point::new(o2.0 as f32 + 1919.0, o2.1 as f32 + 1079.0),
                            Point::new(o2.0 as f32 + 333.5, o2.1 as f32 + 77.25),
                        ] {
                            let u = from_desktop(start, d);
                            assert!(near(to_desktop(start, u), d), "{s1} {o1:?} {d:?}");
                            // in the other window: (desktop - its origin) / its scale
                            let local =
                                Point::new((d.x - o2.0 as f32) / s2, (d.y - o2.1 as f32) / s2);
                            let shown = view.to_screen(u);
                            assert!(near(shown, local), "{s1}->{s2} {o1:?}->{o2:?}: {shown:?}");
                            assert!(near(view.to_image(local), u), "back: {s1}->{s2}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn one_screen_is_the_identity_and_screens_meet_at_their_seam() {
        // the laptop at 250 % and, right of it, a 1080p screen at 100 %
        let laptop = place((0, 0), (3840, 2400), 2.5);
        let monitor = place((3840, 300), (1920, 1080), 1.0);
        let own = view_of(laptop, laptop);
        let p = Point::new(123.4, 567.8);
        assert!(near(own.to_screen(p), p) && near(own.to_image(p), p));
        // the monitor's left edge is the laptop's right edge, in one unit
        let area = area_of(laptop, monitor);
        assert!(near(area.position(), Point::new(1536.0, 120.0)), "{area:?}");
        assert!((area.width - 768.0).abs() < 0.01 && (area.height - 432.0).abs() < 0.01);
        assert_eq!(laptop.size().width, area.x);
        // a laptop image px is 2.5 monitor px
        let v = view_of(laptop, monitor);
        let a = v.to_screen(Point::new(1536.0, 120.0));
        let b = v.to_screen(Point::new(1537.0, 120.0));
        assert!(near(a, Point::ORIGIN) && (b.x - a.x - 2.5).abs() < 0.01);
    }
}
