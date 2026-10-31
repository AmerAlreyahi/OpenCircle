//! Snip mode: its pill, timer, capture routing and long capture.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use iced::widget::image;
use iced::window::{self, Mode};
use iced::{Point, Rectangle, Task, Vector};

use crate::actions::{self, Crop, Run, copy};
use crate::capture;
use crate::config::{self};

use crate::ui::app::background::background;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::screens::editor::Editing;
use crate::ui::screens::long_capture;
use crate::ui::{snip, windows};

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
        if let snip::Msg::Tool(msg) = msg {
            return self.on_tool(msg);
        }
        let o = &mut *self.o;
        let Purpose::Snip(snip) = &mut o.purpose else {
            return Task::none();
        };
        match msg {
            snip::Msg::Stop => {
                if let Some(long) = &snip.scroll {
                    long.stop();
                }
                Task::none()
            }
            snip::Msg::Cancel if snip.scroll.is_some() => {
                if let Some(long) = &mut snip.scroll {
                    long.cancelled = true;
                    long.stop();
                }
                Task::none()
            }
            snip::Msg::Progress(update) => {
                if let Some(long) = &mut snip.scroll {
                    long.update(update);
                }
                Task::none()
            }
            snip::Msg::Done(stitched) => {
                let Some(long) = snip.scroll.take() else {
                    return Task::none();
                };
                // whole again, and in screenshots again
                let whole = windows::uncut(self.overlay)
                    .chain(windows::hide_from_capture(self.overlay, false))
                    .map(Message::Windows);
                let crop = match stitched {
                    Ok(crop) if crop.height > 0 && !long.cancelled => crop,
                    Err(e) => {
                        crate::log!("long capture failed: {e}");
                        return whole.chain(self.open_editor()); // back to the picked area
                    }
                    _ => return whole.chain(self.open_editor()),
                };
                crate::history::recent::remember(&crop);
                self.o.selection = None;
                self.o.stroke.clear();
                // Done opens the editor on it
                whole.chain(self.snip_routed(crop, false, (false, true)))
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
            snip::Msg::HintGone(opening) if opening == self.opening => {
                snip.hinting = false;
                Task::none()
            }
            snip::Msg::HintGone(_) => Task::none(),
            snip::Msg::Tool(_) => Task::none(), // handled above
        }
    }

    /// Snip from main mode (the bar's Snip, E, `run edit`): the one Snip flow
    /// over the frame already frozen. `picked` (main mode's selection) starts
    /// it picked, as a pick would; none = the pick step, with the edge glow.
    /// Main mode's state (selections, words, cards) goes; the frame stays.
    pub(in crate::ui) fn snip_here(&mut self, picked: Option<Rectangle>) -> Task<Message> {
        let windows: Vec<_> = (capture::window_rect::list().into_iter())
            .map(|w| snip::Pick::window(snip::desktop_rect(w.rect), &w.title, w.handle))
            .collect();
        // the user's app: the topmost window under the overlay (ours aren't listed)
        let user = windows.first().map(|w| w.window);
        let start = self.place();
        let mut snip = snip::Snip::new(windows, start, capture::scroll::can_scroll(), user);
        let monitors = std::iter::once(Rectangle::with_size(self.o.screen)).chain(
            self.o
                .screens
                .iter()
                .map(|s| super::screens::area_of(start, s.place)),
        );
        snip.set_monitors(monitors.collect());
        let _ = snip.start(snip::Kind::Region, self.o.screen);
        let (reader, read) = std::sync::mpsc::channel();
        if cfg!(windows) {
            snip.read_later(reader, super::UIA_WINDOWS);
            snip.start_reading(); // its first frame is long up
        }
        let o = &mut *self.o;
        (o.panel, o.translate, o.song, o.toast) = (None, None, None, None);
        (o.word, o.text_mode, o.edit, o.morph) = (None, None, None, None);
        (o.bar_focus, o.grab, o.drawing) = (None, None, false);
        o.others.clear();
        (o.selection, o.stroke) = (None, Vec::new()); // no corner brackets: Snip's outline
        o.view = o.view.reset();
        o.zoom = None;
        o.purpose = Purpose::Snip(Box::new(snip));
        let extras = self.snip_extras(read);
        let Some(area) = picked else {
            return Task::batch([self.snip_next(None), extras]);
        };
        self.select(area);
        let Some(crop) = self.crop() else {
            return extras;
        };
        crate::history::recent::remember(&crop);
        // Snip pressed on a selection: its picked state (the live area and the
        // toolbar), whatever "After a capture" says a fresh pick does.
        Task::batch([self.snip_routed(crop, true, (false, true)), extras])
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
    pub(in crate::ui) fn pill_at_top(&mut self) -> Task<Message> {
        let x = (self.o.screen.width - snip::PILL.width) / 2.0;
        self.pill_window(Point::new(x.max(0.0), ASK_MARGIN))
    }

    /// THE one way Snip's pills (toast, timer, scrolling progress) become a
    /// window: the overlay shrunk to `snip::PILL` at `at` (logical px on this
    /// monitor). Switch it to the shared floating-window helper when it lands.
    /// Wayland can't move, shrink or hide a window (`can_hide_windows`): the
    /// overlay stays full-screen there and `view` draws the pill over the frame.
    /// The other screens' windows go (a pill leaves the screens to the user).
    pub(in crate::ui) fn pill_window(&mut self, at: Point) -> Task<Message> {
        if !self.can_hide {
            return Task::none();
        }
        self.o.waiting.clear();
        let others = super::screens::close_all(&std::mem::take(&mut self.o.screens));
        // never left clear by an opening that hadn't faded in yet (`show`)
        others
            .chain(window::set_mode(self.overlay, Mode::Windowed))
            .chain(windows::opacity(self.overlay, 1.0))
            .chain(window::resize(self.overlay, snip::PILL))
            .chain(window::move_to(
                self.overlay,
                self.o.origin + Vector::new(at.x, at.y),
            ))
            // The window is exactly the pill: cut it to the pill's shape (no box).
            .chain(windows::cut(self.overlay, snip::PILL_RADIUS).map(Message::Windows))
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
        // the freeze hint goes after its 2 s (no frame clock while still)
        let opening = self.opening;
        let hint = background(|tx| {
            std::thread::sleep(std::time::Duration::from_secs(snip::HINT_SECS));
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::HintGone(opening)));
        let recent = Task::batch([recent, hint]);
        if !cfg!(windows) {
            return recent;
        }
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
        let route = snip::route(config::current().snip.after);
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
            // Both (N-74): copied as well, quietly (the editor opening says it
            // went; a failure is logged and the capture is in the editor).
            let copied = match copy {
                true => copy_snip(crop.clone(), None),
                false => Task::none(),
            };
            if in_place {
                return Task::batch([copied, self.open_editor()]);
            }
            self.o.edit = Some(Editing::fitted(
                crop,
                work_area(self.o.screen, self.o.work_inset),
                self.o.scale,
            ));
            return copied;
        }
        // (a crop across screens is edited fitted from the toast: the other
        // screens' windows go with the pill)
        let in_place = in_place && !self.crop_area().is_some_and(|a| self.spans(a));
        let Purpose::Snip(snip) = &mut self.o.purpose else {
            return Task::none();
        };
        let id = snip.toast(crop.clone(), in_place);
        let foreground = snip.foreground;
        let copied = copy_snip(crop, Some(id));
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
            // the live pin quick action: no editor, the pin at once
            Some(snip::Next::Edit(area)) if matches!(&self.o.purpose, Purpose::Snip(s) if s.live_pin) =>
            {
                self.select(area);
                self.live_pin(area)
            }
            Some(snip::Next::Edit(area)) => {
                self.select(area);
                self.snip_captured(None, true)
            }
        }
    }

    /// `area` (logical px, this screen) in physical desktop px, cut to the screen.
    pub(in crate::ui) fn screen_region(
        &self,
        area: Rectangle,
    ) -> Option<crate::capture::window_rect::WinRect> {
        let o = &*self.o;
        let area = area.intersection(&Rectangle::with_size(o.screen))?;
        let Some(image::Handle::Rgba { width, height, .. }) = &o.frame else {
            return None;
        };
        let [x0, y0, x1, y1] = actions::crop_bounds(*width, *height, o.scale, area)?;
        let at = |v: f32, offset: u32| (v * o.scale).round() as i32 + offset as i32;
        Some((at(o.origin.x, x0), at(o.origin.y, y0), x1 - x0, y1 - y0))
    }

    /// Long capture of `area` (its part on this screen): the area is cut out
    /// of the overlay, so the user scrolls the app under it by hand, and a
    /// thread grabs and stitches it (`scroll::by_hand`) until Done or Cancel.
    /// What is grabbed lies inside the hole; only when the toolbar has to sit
    /// over the area (an area as big as the screen) is the overlay kept out of
    /// captures (`hide_from_capture`).
    pub(in crate::ui) fn start_scroll(&mut self, area: Rectangle) -> Task<Message> {
        let (Some(region), Some(area)) = (
            self.screen_region(area),
            area.intersection(&Rectangle::with_size(self.o.screen)),
        ) else {
            return Task::none();
        };
        let o = &*self.o;
        let layout = long_capture::layout(area, o.screen);
        let preview = layout.card.map_or((0, 0), |card| {
            let view = long_capture::viewport(card);
            (
                (view.width * o.scale) as u32,
                (view.height * o.scale) as u32,
            )
        });
        let (overlay, scale, opening) = (self.overlay, o.scale, self.opening);
        let o = &mut *self.o;
        let Purpose::Snip(snip) = &mut o.purpose else {
            return Task::none();
        };
        let stop = Arc::new(AtomicBool::new(false));
        snip.scroll = Some(long_capture::Long::new(area, stop.clone()));
        snip.hinting = true;
        o.stroke.clear(); // its outline is long capture's (dashed)
        let work = background(move |tx| {
            // Let the window be cut first.
            std::thread::sleep(Duration::from_millis(150));
            let each = |update| {
                let _ = tx.unbounded_send(snip::Msg::Progress(update));
            };
            let max = config::current().snip.long_max;
            let stitched = crate::capture::scroll::by_hand(region, preview, max, &stop, each);
            let _ = tx.unbounded_send(snip::Msg::Done(stitched));
        })
        .map(Message::Snip);
        let hint = background(|tx| {
            std::thread::sleep(Duration::from_secs(snip::HINT_SECS));
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::HintGone(opening)));
        // the toolbar stays where it is over an area that fills the screen
        let keep = Some(layout.bar).filter(|bar| bar.intersects(&area));
        let cut = windows::hide_from_capture(overlay, keep.is_some())
            .chain(windows::hole(
                overlay,
                area,
                keep.into_iter().collect(),
                scale,
            ))
            .map(Message::Windows);
        Task::batch([cut.chain(work), hint])
    }
}

/// Copies a snip on a thread (encoding a big capture takes a while), through
/// the one copy path. If it fails while toast `toast` shows, the editor opens,
/// so the capture isn't lost.
fn copy_snip(crop: Crop, toast: Option<u32>) -> Task<Message> {
    background(move |tx| {
        if let Run::Finish(copy) = copy::ACTION.run
            && let Err(e) = copy(crop)
        {
            crate::log!("copy failed: {e}");
            if let Some(id) = toast {
                let _ = tx.unbounded_send(Message::Snip(snip::Msg::CopyFailed(id)));
            }
        }
    })
}
