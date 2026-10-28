//! Snip mode: its pill, timer, capture routing and scrolling capture.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use iced::widget::image;
use iced::window::{self, Mode};
use iced::{Point, Rectangle, Task, Vector};

use crate::actions::{self, Crop, Run, copy};
use crate::capture;
use crate::config::{self};
use crate::labs::{self};

use crate::ui::app::background::background;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::screens::bar::geometry as bar;
use crate::ui::screens::editor::Editing;
use crate::ui::{snip, widgets, windows};

use super::{ASK_MARGIN, App, work_area};

impl App {
    pub(in crate::ui) fn on_snip(&mut self, msg: snip::Msg) -> Task<Message> {
        // The timer ran out and the overlay is gone (hidden or closed): capture
        // again in that mode, unless something else opened meanwhile.
        if let snip::Msg::Shoot(kind) = msg {
            if self.o.frame.is_some() || self.capturing.is_some() {
                return Task::none();
            }
            self.timed = true;
            return self.capture(Instant::now(), Some(kind));
        }
        let o = &mut *self.o;
        let Purpose::Snip(snip) = &mut o.purpose else {
            return Task::none();
        };
        match msg {
            snip::Msg::Stop => {
                if let Some(scroll) = &snip.scroll {
                    scroll.stop();
                }
                Task::none()
            }
            snip::Msg::Progress(height) => {
                if let Some(scroll) = &mut snip.scroll {
                    scroll.progress(height);
                }
                Task::none()
            }
            snip::Msg::Done(stitched) => {
                snip.scroll = None;
                let visible = windows::hide_from_capture(self.overlay, false).map(Message::Windows);
                self.o.selection = None;
                self.o.stroke.clear();
                let next = match stitched {
                    Ok(crop) if crop.height > 0 => self.snip_captured(Some(crop), true),
                    Ok(_) => Task::none(),
                    Err(e) => {
                        crate::log!("scrolling capture failed: {e}");
                        Task::none()
                    }
                };
                if self.snip_pill() {
                    return visible.chain(next); // the toast keeps the window small
                }
                visible.chain(next).chain(self.show())
            }
            snip::Msg::Delay(secs) => {
                let Some(id) = snip.delay(secs) else {
                    return Task::none();
                };
                let foreground = snip.foreground;
                o.selection = None;
                o.stroke.clear();
                if !self.can_hide {
                    // Wayland: no pill (the window can't shrink or hide), so it
                    // closes, waits, and captures again.
                    let _ = snip.shoot(id);
                    let kind = snip.kind;
                    let wait = Duration::from_secs(secs.into()) + Duration::from_millis(300);
                    return self.close().chain(Self::shoot_after(wait, kind));
                }
                let ticks = background(move |tx| {
                    // The overlay is a pill now: the user's app gets the focus back.
                    std::thread::sleep(Duration::from_millis(150));
                    capture::window_rect::activate(foreground);
                    for left in (0..secs).rev() {
                        std::thread::sleep(Duration::from_secs(1));
                        if tx.unbounded_send(left).is_err() {
                            return;
                        }
                    }
                })
                .map(move |left| Message::Snip(snip::Msg::Tick(id, left)));
                self.pill_at_top().chain(ticks)
            }
            // Time's up: the overlay goes first (hidden or closed, as anything
            // closes), so the pill is in no capture.
            snip::Msg::Tick(id, left) => match snip.tick(id, left) {
                Some(true) if snip.shoot(id) => {
                    let kind = snip.kind;
                    let wait = Duration::from_millis(300);
                    self.close().chain(Self::shoot_after(wait, kind))
                }
                _ => Task::none(),
            },
            snip::Msg::Shoot(_) => Task::none(), // handled above
            snip::Msg::Cancel => self.close(),
            snip::Msg::Edit => {
                let Some(toast) = snip.take_toast() else {
                    return Task::none();
                };
                if toast.in_place {
                    let _ = self.open_editor();
                } else {
                    self.o.edit = Some(Editing::fitted(
                        toast.crop,
                        work_area(self.o.screen, self.o.work_inset),
                        self.o.scale,
                    ));
                }
                self.show()
            }
            snip::Msg::ToastDone(id) if snip.toast_showing(id) => self.close(),
            snip::Msg::CopyFailed(id) if snip.toast_showing(id) => self.on_snip(snip::Msg::Edit),
            snip::Msg::CopyFailed(_) => Task::none(),
            snip::Msg::ToastDone(_) => Task::none(),
            snip::Msg::Controls(opening, controls) => {
                if opening == self.opening {
                    snip.add_controls(controls);
                }
                Task::none()
            }
            // The pointer went to another monitor: the overlay goes with it (that
            // monitor's frame was frozen with this one), unless a drag, the
            // editor or a pill holds this one.
            snip::Msg::Left => {
                if o.drawing || o.edit.is_some() {
                    return Task::none();
                }
                let Some((place, image)) = capture::pointer().and_then(|p| snip.follow(p)) else {
                    return Task::none();
                };
                self.o.selection = None;
                self.o.stroke.clear();
                self.set_frame(image, place.scale, place.logical_origin(), place.size());
                // Full-screen stays on its monitor: windowed first.
                window::set_mode(self.overlay, Mode::Windowed).chain(self.show())
            }
            snip::Msg::Shift(held) => {
                snip.set_shift(held);
                Task::none()
            }
            snip::Msg::Recent(recent) => {
                snip.set_recent(recent);
                Task::none()
            }
            snip::Msg::OpenRecent(path) => background(move |tx| {
                let _ = tx.unbounded_send(crate::history::recent::open(&path));
            })
            .map(|read| Message::Snip(snip::Msg::Loaded(read))),
            snip::Msg::Loaded(Ok(crop)) => {
                self.o.selection = None;
                self.o.stroke.clear();
                self.snip_captured(Some(crop), false)
            }
            snip::Msg::Loaded(Err(e)) => {
                crate::log!("[opencircle] recent capture unreadable: {e}");
                Task::none()
            }
            snip::Msg::ClearRecent => {
                snip.set_recent(Vec::new());
                crate::history::recent::clear();
                Task::none()
            }
        }
    }

    /// The window is only Snip's pill (scrolling, the timer, the toast).
    pub(in crate::ui) fn snip_pill(&self) -> bool {
        matches!(&self.o.purpose, Purpose::Snip(s) if s.is_pill())
    }

    /// Snip's "Copied" toast is showing.
    pub(in crate::ui) fn snip_toast(&self) -> bool {
        matches!(&self.o.purpose, Purpose::Snip(s) if s.has_toast())
    }

    /// Shrinks the overlay to the pill at the top of the monitor (no focus taken).
    pub(in crate::ui) fn pill_at_top(&self) -> Task<Message> {
        let x = (self.o.screen.width - snip::PILL.width) / 2.0;
        self.pill_window(Point::new(x.max(0.0), ASK_MARGIN))
    }

    /// THE one way Snip's pills (toast, timer, scrolling progress) become a
    /// window: the overlay shrunk to `snip::PILL` at `at` (logical px on this
    /// monitor). Switch it to the shared floating-window helper when it lands.
    /// Wayland can't move, shrink or hide a window (`can_hide_windows`): the
    /// overlay stays full-screen there and `view` draws the pill over the frame.
    pub(in crate::ui) fn pill_window(&self, at: Point) -> Task<Message> {
        if !self.can_hide {
            return Task::none();
        }
        // never left clear by an opening that hadn't faded in yet (`show`)
        window::set_mode(self.overlay, Mode::Windowed)
            .chain(windows::opacity(self.overlay, 1.0))
            .chain(window::resize(self.overlay, snip::PILL))
            .chain(window::move_to(
                self.overlay,
                self.o.origin + Vector::new(at.x, at.y),
            ))
            // The window is exactly the pill: cut it to the pill's shape (no box).
            .chain(windows::cut(self.overlay, widgets::SURFACE_RADIUS).map(Message::Windows))
    }

    /// `Shoot(kind)` after `wait` (the overlay is gone by then).
    pub(in crate::ui) fn shoot_after(wait: Duration, kind: snip::Kind) -> Task<Message> {
        background(move |tx| {
            std::thread::sleep(wait);
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::Shoot(kind)))
    }

    /// Snip's slow extras, on threads: the Recent strip, and (Windows) the
    /// controls of each window the snip asks for (`read`, see
    /// `Snip::set_reader`), for outlines and snapping.
    pub(in crate::ui) fn snip_extras(
        &self,
        read: std::sync::mpsc::Receiver<isize>,
    ) -> Task<Message> {
        let recent = background(|tx| {
            let _ = tx.unbounded_send(crate::history::recent::load());
        })
        .map(|recent| Message::Snip(snip::Msg::Recent(recent)));
        if !cfg!(windows) {
            return recent;
        }
        let opening = self.opening;
        // Ends when the snip (the sender) is gone and every read is in.
        let controls = background(move |tx| {
            // A thread per window: a slow one (a huge page) holds up no other.
            // ponytail: a hung app's thread lives until UIA's timeout (uia.rs).
            for window in read {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let controls = (capture::uia::controls(window).into_iter())
                        .map(|(r, what)| snip::Pick::control(snip::desktop_rect(r), &what, window))
                        .collect();
                    let _ = tx.unbounded_send(controls);
                });
            }
        })
        .map(move |rects| Message::Snip(snip::Msg::Controls(opening, rects)));
        Task::batch([recent, controls])
    }

    /// A snip is taken: the selection on the frozen frame (`given: None`), or a
    /// stitched / recent capture. Kept in Recent (`remember`, if history is on),
    /// then copied (a toast offers Edit) and / or edited, as `snip.after` says.
    pub(in crate::ui) fn snip_captured(
        &mut self,
        given: Option<Crop>,
        remember: bool,
    ) -> Task<Message> {
        let in_place = given.is_none();
        let Some(crop) = given.or_else(|| self.crop()) else {
            return Task::none();
        };
        if remember {
            crate::history::recent::remember(&crop);
        }
        // No editor (Labs): a snip is only copied.
        let route = match self.o.labs.has(labs::Feature::Editor) {
            true => snip::route(config::current().snip.after),
            false => (true, false),
        };
        self.snip_routed(crop, in_place, route)
    }

    /// Copies and / or edits a snip's `crop` as `route` says; Copy gets the toast.
    pub(in crate::ui) fn snip_routed(
        &mut self,
        crop: Crop,
        in_place: bool,
        route: (bool, bool),
    ) -> Task<Message> {
        let (copy, edit) = route;
        if edit || !copy {
            if in_place {
                return self.open_editor();
            }
            self.o.edit = Some(Editing::fitted(
                crop,
                work_area(self.o.screen, self.o.work_inset),
                self.o.scale,
            ));
            return Task::none();
        }
        let Purpose::Snip(snip) = &mut self.o.purpose else {
            return Task::none();
        };
        let id = snip.toast(crop.clone(), in_place);
        let foreground = snip.foreground;
        // The copy runs on a thread (encoding a big capture takes a while);
        // if it fails, the editor opens, so the capture isn't lost.
        let copied = background(move |tx| {
            if let Run::Finish(copy) = copy::ACTION.run
                && let Err(e) = copy(crop)
            {
                crate::log!("copy failed: {e}");
                let _ = tx.unbounded_send(Message::Snip(snip::Msg::CopyFailed(id)));
            }
        });
        let done = background(move |tx| {
            // Once the pill is up, the user's app (their AI chat) gets the focus back.
            std::thread::sleep(Duration::from_millis(150));
            capture::window_rect::activate(foreground);
            std::thread::sleep(Duration::from_secs(snip::TOAST_SECS));
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::ToastDone(id)));
        Task::batch([copied, self.pill_at_top().chain(done)])
    }

    /// Where a snip pick leads: the editor on the area, or a scrolling capture.
    pub(in crate::ui) fn snip_next(&mut self, next: Option<snip::Next>) -> Task<Message> {
        match next {
            None => {
                self.o.selection = None;
                self.o.stroke.clear();
                Task::none()
            }
            Some(snip::Next::Edit(area)) => {
                self.select(area);
                self.snip_captured(None, true)
            }
        }
    }

    /// Auto-scrolls and stitches `area` on a thread. The overlay shrinks to the
    /// progress pill beside the area, so it is in none of the frames.
    pub(in crate::ui) fn start_scroll(&mut self, area: Rectangle) -> Task<Message> {
        let o = &mut *self.o;
        let (Some(image::Handle::Rgba { width, height, .. }), Purpose::Snip(snip)) =
            (&o.frame, &mut o.purpose)
        else {
            return Task::none();
        };
        let Some([x0, y0, x1, y1]) = actions::crop_bounds(*width, *height, o.scale, area) else {
            return Task::none();
        };
        let at = |v: f32, offset: u32| (v * o.scale).round() as i32 + offset as i32;
        let region = (at(o.origin.x, x0), at(o.origin.y, y0), x1 - x0, y1 - y0);
        let stop = Arc::new(AtomicBool::new(false));
        snip.scroll = Some(snip::Scroll::new(x1 - x0, stop.clone()));
        let work = background(move |tx| {
            // Let the window shrink first.
            std::thread::sleep(Duration::from_millis(150));
            let progress = |height| {
                let _ = tx.unbounded_send(snip::Msg::Progress(height));
            };
            let stitched = crate::capture::scroll::scroll_capture(region, &stop, progress);
            let _ = tx.unbounded_send(snip::Msg::Done(stitched));
        })
        .map(Message::Snip);
        // An area as tall as the screen leaves the pill over it: it is kept out of
        // the frames (`hide_from_capture`, Windows; elsewhere it stays beside the area).
        let at = bar::place(area, snip::PILL, self.o.screen);
        self.pill_window(at)
            .chain(windows::hide_from_capture(self.overlay, true).map(Message::Windows))
            .chain(window::gain_focus(self.overlay))
            .chain(work)
    }
}
