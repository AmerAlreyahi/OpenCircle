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

use iced::widget::{canvas, image, stack};
use iced::window::{self, Mode};
use iced::{Element, Fill, Point, Rectangle, Size, Task, Vector};

use crate::config;
use crate::ui::app::capture::overlay_window;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::app::word;
use crate::ui::platform::monitor::{Place, place_window};
use crate::ui::{overlay, windows};

use super::App;

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
#[cfg_attr(not(test), allow(dead_code))] // (41d)
pub(in crate::ui) fn to_desktop(start: Place, u: Point) -> Point {
    Point::new(
        start.origin.0 as f32 + u.x * start.scale,
        start.origin.1 as f32 + u.y * start.scale,
    )
}

/// A desktop physical px point in the starting screen's image px.
#[cfg_attr(not(test), allow(dead_code))] // (41d)
pub(in crate::ui) fn from_desktop(start: Place, d: Point) -> Point {
    Point::new(
        (d.x - start.origin.0 as f32) / start.scale,
        (d.y - start.origin.1 as f32) / start.scale,
    )
}

/// The monitor at `place` in the starting screen's image px.
#[cfg_attr(not(test), allow(dead_code))] // (41d)
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

/// Closing: every other monitor's window goes at once (cleared now, so no
/// frame of it shows while it closes). Their frames go with the opening.
// ponytail: opened and closed per opening, no pool of hidden windows; add one
// if opening the second screen's window ever shows as late.
pub(in crate::ui) fn close_all<T: Send + 'static>(screens: &[Screen]) -> Task<T> {
    Task::batch(screens.iter().map(|s| {
        windows::clear_now(s.window);
        windows::forget(s.window);
        window::close(s.window)
    }))
}

impl App {
    /// Another monitor's frame of capture `n` landed: its window opens over it
    /// (no focus: the keys stay with the starting screen). Dropped when it is
    /// late (closed, another capture's) or this opening isn't Circle to Search.
    pub(in crate::ui) fn add_screen(
        &mut self,
        n: u32,
        place: Place,
        frame: image::Handle,
    ) -> Task<Message> {
        let start = self.place();
        let known = self.o.screens.iter().any(|s| s.place == place);
        if self.o.capture != n
            || !matches!(self.o.purpose, Purpose::Search)
            || place.origin == start.origin
            || known
        {
            return Task::none();
        }
        let (id, open) = windows::screen(overlay_window(None));
        self.o.screens.push(Screen::new(id, start, place, frame));
        crate::log!(
            "[opencircle] screen {}x{} at {:?} frozen too",
            place.px.0,
            place.px.1,
            place.origin
        );
        open.discard()
            .chain(place_window(id, place))
            .chain(window::set_mode(id, Mode::Fullscreen))
            .chain(window::gain_focus(self.overlay))
    }

    /// An event from another monitor's window: its points go on in the
    /// starting screen's px (where `on_overlay` takes them), so a stroke, box
    /// or text drag carries on across screens. Zoom and pan are the starting
    /// screen's own.
    pub(in crate::ui) fn on_screen_event(
        &mut self,
        window: window::Id,
        event: overlay::Event,
    ) -> Task<Message> {
        let Some(view) =
            (self.o.screens.iter()).find_map(|s| (s.window == window).then_some(s.view))
        else {
            return Task::none();
        };
        let start = self.o.view;
        let on_start = |p: Point| start.to_screen(view.to_image(p));
        let event = match event {
            overlay::Event::Press { at, shift } => overlay::Event::Press {
                at: on_start(at),
                shift,
            },
            overlay::Event::Move(p) => overlay::Event::Move(on_start(p)),
            overlay::Event::Hover(p) => overlay::Event::Hover(on_start(p)),
            overlay::Event::Zoom { .. }
            | overlay::Event::PanStart(_)
            | overlay::Event::PanMove(_)
            | overlay::Event::PanEnd
            | overlay::Event::ZoomReset => return Task::none(),
            event => event,
        };
        let release = matches!(event, overlay::Event::Release);
        let done = self.on_overlay(event);
        match release {
            // the click gave that window the focus: back to the starting screen
            true if self.o.is_some() => done.chain(window::gain_focus(self.overlay)),
            _ => done,
        }
    }

    /// Another monitor's window: its frozen frame, dimmed, and whatever of the
    /// selection is on it. No edge glow, bar, card, toast or hint: those stay
    /// on the starting screen.
    pub(in crate::ui) fn screen_view<'a>(&'a self, screen: &'a Screen) -> Element<'a, Message> {
        let picked: Vec<Rectangle> = (self.o.text_mode.as_ref())
            .and_then(|m| m.picked.as_ref())
            .map_or_else(Vec::new, word::Picked::rects);
        let overlay = overlay::Overlay {
            stroke: &self.o.stroke,
            selection: self.o.selection,
            others: &self.o.others,
            view: screen.view,
            zooming: false,
            handles: self.handles(),
            opening: self.opening,
            first_draw: &screen.first_draw,
            frame_times: &screen.frame_times,
            hover: false,
            dim: true,
            live_drag: false,
            look: config::look(),
            morph: (self.o.morph.as_ref()).map(|(from, at)| (from.as_slice(), *at)),
            words: (self.o.text_mode.as_ref()).map_or(&[], |m| m.boxes.as_slice()),
            picked,
            text: Vec::new(),
        };
        let frozen = overlay::Frozen {
            frame: &screen.frame,
            scale: screen.place.scale,
        };
        let window = screen.window;
        stack![
            canvas(frozen).width(Fill).height(Fill),
            Element::from(canvas(overlay).width(Fill).height(Fill))
                .map(move |event| Message::ScreenEvent(window, event)),
        ]
        .into()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::Trigger;
    use crate::ui::app::message::Shot;
    use crate::ui::widgets::shot;

    fn place(origin: (i32, i32), px: (u32, u32), scale: f32) -> Place {
        Place { origin, px, scale }
    }

    fn solid(place: Place, grey: u8) -> image::Handle {
        let (w, h) = place.px;
        image::Handle::from_rgba(w, h, vec![grey; (w * h * 4) as usize])
    }

    /// The starting screen: 640x400 at 100 % at the desktop's origin.
    const START: Place = Place {
        origin: (0, 0),
        px: (640, 400),
        scale: 1.0,
    };

    /// Circle to Search opened on `START`; its capture's number.
    fn opened() -> (App, u32) {
        let mut app = App::idle(window::Id::unique());
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let n = app.capturing.expect("pending");
        let shot = Shot {
            image: solid(START, 200),
            scale: START.scale,
            origin: START.logical_origin(),
            size: START.size(),
            snip: None,
        };
        let _ = app.update(Message::Captured(n, Instant::now(), Ok(shot)));
        assert!(app.o.is_some());
        (app, n)
    }

    #[test]
    fn every_other_monitor_gets_its_own_window_without_the_glow_or_the_bar() {
        let (mut app, n) = opened();
        // right of it at 150 %, and left of it, higher, at 100 %
        let right = place((640, 0), (480, 300), 1.5);
        let left = place((-320, -100), (320, 200), 1.0);
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        let _ = app.update(Message::Screen(n, left, solid(left, 90)));
        let _ = app.update(Message::Screen(n, left, solid(left, 90))); // twice: one window
        let _ = app.update(Message::Screen(n, START, solid(START, 90))); // its own
        let ids: Vec<_> = app.o.screens.iter().map(|s| s.window).collect();
        assert_eq!(ids.len(), 2, "one window per other screen");
        assert!(ids[0] != ids[1] && !ids.contains(&app.overlay));
        // the other screens: the frozen frame under the dim, nothing else
        for s in &app.o.screens {
            let size = s.place.size();
            let rgba = shot::render(app.view(s.window), size.width as u32, size.height as u32);
            let (pixels, _) = rgba.as_chunks::<4>();
            assert!(pixels.iter().all(|p| *p == pixels[0]), "only the dim");
            assert!(pixels[0][0] < 90, "dimmed: {:?}", pixels[0]);
        }
        // the starting screen: its edge glow and bar
        let rgba = shot::render(app.view(app.overlay), 640, 400);
        let (pixels, _) = rgba.as_chunks::<4>();
        assert!(pixels.iter().any(|p| *p != pixels[320 * 640 + 200]));
    }

    #[test]
    fn esc_closes_every_screen_at_once_and_a_late_frame_is_dropped() {
        let (mut app, n) = opened();
        let right = place((640, 0), (480, 300), 1.5);
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        let other = app.o.screens[0].window;
        windows::CLEARED.with(|c| c.borrow_mut().clear());
        let _ = app.update(Message::Escape);
        assert!(app.o.is_none(), "closed, every frame freed with it");
        let cleared = windows::CLEARED.with(|c| std::mem::take(&mut *c.borrow_mut()));
        assert!(cleared.contains(&other), "its window went with it");
        // a frame landing after Esc, then one of the previous capture: dropped
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        assert!(app.o.is_none());
        let (mut app, m) = opened();
        let _ = app.update(Message::Screen(m.wrapping_sub(1), right, solid(right, 90)));
        assert!(app.o.screens.is_empty(), "another capture's frame");
        let _ = app.update(Message::Screen(m, right, solid(right, 90)));
        assert_eq!(app.o.screens.len(), 1);
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
