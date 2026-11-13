//! Snip mode: its pill, timer, capture routing and long capture.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use iced::widget::image;
use iced::window::{self, Mode};
use iced::{Point, Rectangle, Size, Task, Vector};

use crate::shared::config;
use crate::shared::platform::capture;
use crate::{
    actions::{self, Crop, Run, copy, save},
    features::text_reading::text,
};

use crate::ui::app::background::background;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::screens::long_capture::{self, Finish};
use crate::ui::screens::snip_toolbar::SnipToolbar;
use crate::ui::{long_capture_popup, snip, windows};

use super::{ASK_MARGIN, App, work_area};

impl App {
    pub(crate) fn on_snip(&mut self, msg: snip::Msg) -> Task<Message> {
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
        if let snip::Msg::Progress(update) = msg {
            return self.on_progress(update);
        }
        let o = &mut *self.o;
        let Purpose::Snip(snip) = &mut o.purpose else {
            return Task::none();
        };
        match msg {
            snip::Msg::Finish(how) => {
                if let Some(long) = &mut snip.scroll {
                    long.finish(how);
                }
                Task::none()
            }
            snip::Msg::Done(stitched) => {
                let Some(long) = snip.scroll.take() else {
                    return Task::none();
                };
                // whole again, and in screenshots again (its screen's window)
                let window = long.window.unwrap_or(self.overlay);
                let whole = windows::uncut(window)
                    .chain(windows::hide_from_capture(window, false))
                    .map(Message::Windows);
                // back to the picked area (its editor kept, edits and all),
                // its toolbar where it was
                if let Some(was) = long.bar_was {
                    (o.bar_left, o.bar_now) = (was, None);
                }
                if let Some(edit) = &mut o.edit {
                    edit.annotator.capturing = false;
                }
                let crop = match stitched {
                    Ok(crop) if crop.height > 0 && !long.cancelled() => crop,
                    Err(e) => {
                        crate::log!("long capture failed: {e}");
                        return whole;
                    }
                    _ => return whole,
                };
                crate::shared::history::recent::remember(&crop);
                // the toolbar's button (Enter = Copy) acts on the stitched image
                let then = match long.then.unwrap_or(Finish::Copy) {
                    Finish::Copy | Finish::Cancel | Finish::Done => {
                        self.finish(&copy::ACTION, crop)
                    }
                    Finish::Save => self.finish(&save::ACTION, crop),
                    Finish::Pin => self.pin(crop),
                    // More's rows act on the stitched page, as Copy / Save do
                    Finish::CopyText => self.finish(&text::ACTION, crop),
                    Finish::AskAi => self.open_chat(crop, None),
                    // the share sheet comes from the editor: on the image, picked
                    Finish::Share => {
                        self.o.selection = None;
                        self.o.stroke.clear();
                        self.edit_fitted(crop);
                        Task::done(Message::Annotate(crate::snip::annotate::Message::Share))
                    }
                };
                whole.chain(then)
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
                let _ = tx.unbounded_send(crate::shared::history::recent::open(&path));
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
                crate::shared::history::recent::clear();
                Task::none()
            }
            snip::Msg::HintGone(opening) if opening == self.opening => {
                snip.hinting = false;
                Task::none()
            }
            snip::Msg::HintGone(_) => Task::none(),
            snip::Msg::Tool(_) | snip::Msg::Progress(_) => Task::none(), // handled above
        }
    }

    /// Snip from main mode (the bar's Snip, E, `run edit`): the one Snip flow
    /// over the frame already frozen. `picked` (main mode's selection) starts
    /// it picked, as a pick would; none = the pick step, with the edge glow.
    /// Main mode's state (selections, words, cards) goes; the frame stays.
    /// With a translation on screen (every entry: the bar's E, Alt+A, tray),
    /// Snip takes the page as translated (`Session::seen`) as its frame.
    pub(crate) fn snip_here(&mut self, picked: Option<Rectangle>) -> Task<Message> {
        // the windows as they were at the freeze (as Snip's own shortcut lists
        // them), not now: a late list drifts from the frozen frames
        let mut windows = std::mem::take(&mut self.o.windows);
        let seen = (self.o.translate.as_ref()).and_then(|(_, s)| s.seen());
        if let Some(seen) = seen {
            self.o.frame = Some(seen);
            // the translated page's windows aren't the picture's: none outlined
            // (drag = an area; a click or Enter = the screen)
            windows.clear();
        }
        // the user's app: the topmost titled window (the overlay wasn't up
        // yet; the taskbar has no title)
        // ponytail: an untitled app window is passed over for the next one
        let user = (windows.iter())
            .find(|w| w.label != "Window")
            .map(|w| w.window);
        let start = self.place();
        let mut snip = snip::Snip::new(windows, start, capture::scroll::can_scroll(), user);
        snip.set_monitors(self.monitors());
        let _ = snip.start(snip::Kind::Region, self.o.screen);
        let (reader, read) = std::sync::mpsc::channel();
        if cfg!(windows) {
            snip.read_later(reader, super::UIA_WINDOWS);
            snip.start_reading(); // its first frame is long up
        }
        let o = &mut *self.o;
        (o.ai_chat, o.translate, o.song, o.toast) = (None, None, None, None);
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
        crate::shared::history::recent::remember(&crop);
        // Snip pressed on a selection: its picked state (the live area and the
        // toolbar), whatever "After a capture" says a fresh pick does.
        Task::batch([self.snip_routed(crop, true, false), extras])
    }

    /// The window is only Snip's pill (scrolling, the timer).
    pub(crate) fn snip_pill(&self) -> bool {
        matches!(&self.o.purpose, Purpose::Snip(s) if s.is_pill())
    }

    /// Shrinks the overlay to the pill at the top of the monitor (no focus taken).
    pub(crate) fn pill_at_top(&mut self) -> Task<Message> {
        let x = (self.o.screen.width - snip::PILL.width) / 2.0;
        self.pill_window(Point::new(x.max(0.0), ASK_MARGIN))
    }

    /// THE one way Snip's pills (timer, scrolling progress) become a
    /// window: the overlay shrunk to `snip::PILL` at `at` (logical px on this
    /// monitor). Switch it to the shared floating-window helper when it lands.
    /// Wayland can't move, shrink or hide a window (`can_hide_windows`): the
    /// overlay stays full-screen there and `view` draws the pill over the frame.
    /// The other screens' windows go (a pill leaves the screens to the user).
    pub(crate) fn pill_window(&mut self, at: Point) -> Task<Message> {
        if !self.can_hide {
            return Task::none();
        }
        self.o.waiting.clear();
        let others = super::screens::close_all(&std::mem::take(&mut self.o.screens));
        // never left clear by an opening that hadn't faded in yet (`show`)
        others
            .chain(windows::show(self.overlay, Mode::Windowed))
            .chain(windows::opacity(self.overlay, 1.0))
            .chain(crate::shared::platform::monitor::resize(
                self.overlay,
                snip::PILL,
            ))
            .chain(window::move_to(
                self.overlay,
                self.o.origin + Vector::new(at.x, at.y),
            ))
            // The window is exactly the pill: cut it to the pill's shape (no box).
            .chain(windows::cut(self.overlay, snip::PILL_RADIUS).map(Message::Windows))
    }

    /// `Shoot(kind)` after `wait` (the overlay is gone by then).
    pub(crate) fn shoot_after(wait: Duration, kind: snip::Kind) -> Task<Message> {
        background(move |tx| {
            std::thread::sleep(wait);
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::Shoot(kind)))
    }

    /// Snip's slow extras, on threads: the Recent strip, and (Windows) the
    /// controls of each window the snip asks for (`read`, see
    /// `Snip::set_reader`), for outlines and snapping.
    pub(crate) fn snip_extras(&self, read: std::sync::mpsc::Receiver<isize>) -> Task<Message> {
        let recent = background(|tx| {
            let _ = tx.unbounded_send(crate::shared::history::recent::load());
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
    /// then picked with the toolbar, and copied unless `snip.after` is "editor".
    pub(crate) fn snip_captured(&mut self, given: Option<Crop>, remember: bool) -> Task<Message> {
        let in_place = given.is_none();
        let Some(crop) = given.or_else(|| self.crop()) else {
            return Task::none();
        };
        if remember {
            crate::shared::history::recent::remember(&crop);
        }
        let copy = snip::copies(config::current().snip.after);
        self.snip_routed(crop, in_place, copy)
    }

    /// Picks a snip's `crop` (the Snip toolbar, always), also copied if `copy`.
    pub(crate) fn snip_routed(&mut self, crop: Crop, in_place: bool, copy: bool) -> Task<Message> {
        // Copied quietly (the toolbar opening says it went; a failure is logged
        // and the capture is in the editor).
        let copied = match copy {
            true => copy_snip(crop.clone()),
            false => Task::none(),
        };
        if in_place {
            return Task::batch([copied, self.open_snip_toolbar()]);
        }
        self.edit_fitted(crop);
        copied
    }

    /// A capture of its own (stitched, recent, a toast's across screens) in
    /// Snip's picked state on this screen: the live area is its whole image,
    /// with Snip's toolbar docked to it (`SnipToolbar::fitted`).
    fn edit_fitted(&mut self, crop: Crop) {
        let mut edit = SnipToolbar::fitted(
            crop,
            work_area(self.o.screen, self.o.work_inset),
            self.o.scale,
        );
        edit.annotator.steps = self.windows.steps();
        let area = edit.canvas_rect();
        self.o.ai_chat = None;
        self.o.view = self.o.view.reset(); // the editor works at 1x
        self.o.zoom = None;
        (self.o.bar_now, self.o.bar_glide, self.o.bar_left) = (None, None, None); // it rises from the area
        self.o.edit = Some(edit);
        self.select(area); // the overlay's dim and outline follow it
    }

    /// Where a snip pick leads: the editor on the area, or a scrolling capture.
    pub(crate) fn snip_next(&mut self, next: Option<snip::Next>) -> Task<Message> {
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
    pub(crate) fn screen_region(
        &self,
        area: Rectangle,
    ) -> Option<crate::shared::platform::capture::window_rect::WinRect> {
        let o = &*self.o;
        let area = area.intersection(&Rectangle::with_size(o.screen))?;
        let Some(image::Handle::Rgba { width, height, .. }) = &o.frame else {
            return None;
        };
        let [x0, y0, x1, y1] = actions::crop_bounds(*width, *height, o.scale, area)?;
        let at = |v: f32, offset: u32| (v * o.scale).round() as i32 + offset as i32;
        Some((at(o.origin.x, x0), at(o.origin.y, y0), x1 - x0, y1 - y0))
    }

    /// Long capture's news; the small picture over the area (`Long::over_area`)
    /// is kept in its screen's hole as it grows.
    fn on_progress(
        &mut self,
        update: crate::shared::platform::capture::scroll::Update,
    ) -> Task<Message> {
        let on = self.long_on();
        let bar = self.long_bar_on(&on);
        let Purpose::Snip(snip) = &mut self.o.purpose else {
            return Task::none();
        };
        let Some(long) = &mut snip.scroll else {
            return Task::none();
        };
        long.update(update);
        let picture = long.over_area(on.work, bar);
        if picture == long.cut {
            return Task::none();
        }
        long.cut = picture;
        let keep = [Some(bar).filter(|b| b.intersects(&long.area)), picture];
        windows::hole(
            on.window,
            long.area,
            keep.into_iter().flatten().collect(),
            on.place.scale,
        )
        .map(Message::Windows)
    }

    /// The screen the running long capture is on (the starting one without one).
    pub(crate) fn long_on(&self) -> super::screens::On {
        let window = self.scroll().and_then(|l| l.window);
        self.on_window(window.unwrap_or(self.overlay))
    }

    /// Snip's toolbar during a long capture, as its picture and hint keep
    /// clear of it: in the logical px of the long capture's screen.
    pub(crate) fn long_bar(&self) -> Rectangle {
        self.long_bar_on(&self.long_on())
    }

    /// Snip's toolbar in the logical px of the screen `on` (its size there).
    fn long_bar_on(&self, on: &super::screens::On) -> Rectangle {
        let size =
            (self.o.edit.as_ref()).map_or_else(SnipToolbar::toolbar_size, SnipToolbar::bar_size);
        let at = on.to_local(Rectangle::new(self.bar_at(), Size::ZERO));
        Rectangle::new(at.position(), size)
    }

    /// Long capture of `area` (its part on the screen holding the larger part
    /// of it, `on_area`; any screen, any scale): the area is cut out of that
    /// screen's window, so the user scrolls the app under it by hand, and a
    /// thread grabs and stitches it (`scroll::by_hand`: that monitor's
    /// duplication, the wheel paced there) until Done or Cancel. Its picture,
    /// hint and note are drawn there too. What is grabbed lies inside the hole;
    /// only when the toolbar has to sit over the area (an area as big as the
    /// screen) is the window kept out of captures (`hide_from_capture`).
    pub(crate) fn start_scroll(&mut self, area: Rectangle) -> Task<Message> {
        let on = self.on_area(area);
        let Some((area, region)) = self.region_on(on, area) else {
            return Task::none();
        };
        if !self.can_hide {
            return self.start_cast(on, area, region);
        }
        // the toolbar stays where it is, but off the area where it fits
        // beside it: over the area the window must be kept out of the grabs
        let mut bar = self.long_bar_on(&on);
        let work = on.work;
        let mut bar_was = None;
        if bar.intersects(&area)
            && let Some(at) = long_capture::bar_beside(area, work, bar.size())
        {
            bar_was = Some(self.o.bar_left);
            (self.o.bar_left, self.o.bar_now) = (Some(on.to_start(at)), None);
            bar = Rectangle::new(at, bar.size());
        }
        let scale = on.place.scale;
        let room = long_capture::room(area, work, bar, scale);
        let preview = room.map_or((0, 0), |r| physical(r.size(), scale));
        let o = &mut *self.o;
        let Purpose::Snip(snip) = &mut o.purpose else {
            return Task::none();
        };
        let stop = Arc::new(AtomicBool::new(false));
        let mut long = long_capture::Long::new(area, stop.clone(), scale);
        (long.bar_was, long.window) = (bar_was, Some(on.window));
        // (let the window be cut first)
        let work = stitch(region, preview, (0, 0), Duration::from_millis(150), stop);
        snip.scroll = Some(long);
        if let Some(edit) = &mut o.edit {
            edit.annotator.capturing = true;
            edit.annotator.focus = None;
            edit.annotator.menu = None;
        }
        o.stroke.clear(); // its outline is long capture's (dashed)
        // the toolbar stays where it is over an area that fills the screen,
        // and the small picture sits in its corner (`on_progress`): kept in the
        // hole, out of every grab (no other window of ours over the area)
        let keep = Some(bar).filter(|bar| bar.intersects(&area));
        let over = keep.is_some() || room.is_some_and(|r| r.intersects(&area));
        crate::log!(
            "[opencircle] long capture of {region:?} on the screen at {:?} ({}x): toolbar {}",
            on.place.origin,
            scale,
            match (keep.is_some(), bar_was.is_some()) {
                (true, _) => "over the area (kept out of captures)",
                (false, true) => "moved beside the area",
                (false, false) => "beside the area",
            }
        );
        let window = on.window;
        let cut = windows::hide_from_capture(window, over)
            .chain(windows::no_backdrop(window))
            .chain(windows::hole(
                window,
                area,
                keep.into_iter().collect(),
                scale,
            ))
            .map(Message::Windows);
        cut.chain(work)
    }

    /// Wayland's long capture of `area` (logical px on `on`; `region` its
    /// physical desktop px): the overlay can't be cut there (`can_hide`), so
    /// it goes, and only then is the screen streamed (none of the overlay, its
    /// dim or its fade in a frame: `capture::LEAVE_FRAME`, as a grab waits).
    /// The user scrolls the page by hand; the pop-up beside the area (inside
    /// its top-right corner where no side has room: `popup_room`) shows the
    /// picture so far with Copy / Save / Done / Cancel (`long_capture_popup`,
    /// XWayland), and the tray says it is capturing (`tray::capturing`, the
    /// one sign without XWayland). Any shortcut, the pop-up or the tray ends it.
    fn start_cast(
        &mut self,
        on: super::screens::On,
        area: Rectangle,
        region: (i32, i32, u32, u32),
    ) -> Task<Message> {
        let scale = on.place.scale;
        let room = long_capture::popup_room(area, on.work, scale);
        let preview = room.map_or((0, 0), |(r, _)| physical(r, scale));
        let inside = room.is_some_and(|(_, inside)| inside);
        let cover = long_capture::popup_cover(area, on.work, scale);
        let stop = Arc::new(AtomicBool::new(false));
        let origin = Point::new(
            on.place.origin.0 as f32 / scale,
            on.place.origin.1 as f32 / scale,
        );
        let mut cast = Cast {
            stop: stop.clone(),
            then: None,
            popup: None,
            area,
            work: on.work,
            origin: Vector::new(origin.x, origin.y),
            scale,
            picture: Size::ZERO,
            label: String::new(),
            labelled: None,
            inside,
            first: region.3,
        };
        let clicks = match room.is_some() {
            true => match long_capture_popup::Popup::start() {
                Some((popup, clicks)) => {
                    cast.popup = Some(popup);
                    clicks.map(|how| Message::Trigger(crate::Trigger::LongCapture(how)))
                }
                None => Task::none(),
            },
            false => {
                crate::log!(
                    "[opencircle] long capture: no room for the pop-up, beside or in the area"
                );
                Task::none()
            }
        };
        crate::log!(
            "[opencircle] long capture of {region:?} (Wayland): pop-up {}",
            match (cast.popup.is_some(), inside) {
                (true, false) => "beside the area".to_string(),
                (true, true) => format!("in the area's corner (left out: {cover:?})"),
                (false, _) => "none (tray only)".to_string(),
            }
        );
        cast.show(0, false, false, None);
        self.cast = Some(cast);
        // GNOME's close animation can outlast 250 ms: a frame of the fading overlay once
        // slipped into the page (W40). Frames are read only after it has surely gone.
        let wait = Duration::from_millis(500);
        Task::batch([
            self.close(),
            stitch(region, preview, cover, wait, stop),
            clicks,
        ])
    }

    /// Wayland: the long capture going on without its overlay, so far.
    pub(crate) fn cast_progress(
        &mut self,
        u: crate::shared::platform::capture::scroll::Update,
    ) -> Task<Message> {
        if let Some(cast) = &mut self.cast {
            cast.show(u.height, u.full, u.lost, u.preview);
        }
        Task::none()
    }

    /// Wayland: the long capture ends so (the pop-up, the tray, a shortcut =
    /// Done); the first end asked for wins.
    pub(crate) fn cast_finish(&mut self, how: Finish) -> Task<Message> {
        if let Some(cast) = &mut self.cast {
            crate::log!("[opencircle] long capture: {how:?}");
            cast.then.get_or_insert(how);
            cast.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Task::none()
    }

    /// Wayland: the long capture that went on without its overlay ended. Copy /
    /// Save act on the stitched page at once (the pop-up then says how it went
    /// and goes); Done, a shortcut or the stream stopping: Snip opens on a
    /// fresh frame with the stitched page picked (its toolbar's Copy / Save /
    /// drag, Ctrl+C); Cancel drops it.
    pub(crate) fn cast_done(&mut self, stitched: Result<Crop, String>) -> Task<Message> {
        let Some(cast) = self.cast.take() else {
            return Task::none();
        };
        crate::shared::platform::tray::capturing(None);
        let crop = match stitched {
            Ok(crop) if crop.height > 0 && cast.then != Some(Finish::Cancel) => crop,
            Ok(_) => return Task::none(),
            Err(e) => {
                crate::log!("long capture failed: {e}");
                return Task::none();
            }
        };
        crate::shared::history::recent::remember(&crop);
        let action = match cast.then {
            Some(Finish::Copy) => &copy::ACTION,
            Some(Finish::Save) => &save::ACTION,
            _ => {
                if let Some(popup) = cast.popup {
                    popup.close(); // out of the fresh frame
                }
                self.picked_next = Some((None, crop));
                self.next = Purpose::Search;
                return self.capture(Instant::now(), Some(snip::Kind::Region));
            }
        };
        let (Run::Finish(run), id, popup) = (action.run, action.id, cast.popup) else {
            return Task::none();
        };
        background(move |_| {
            let said = run(crop).unwrap_or_else(|e| {
                crate::log!("{id} failed: {e}");
                e
            });
            if let Some(popup) = popup {
                popup.end(&said);
            }
        })
    }
}

/// Wayland's long capture going on without its overlay (`App::cast`).
pub(crate) struct Cast {
    stop: Arc<AtomicBool>,
    /// How it ends (the first asked for).
    pub(crate) then: Option<Finish>,
    /// Its pop-up (none without XWayland or room beside the area).
    pub(crate) popup: Option<long_capture_popup::Popup>,
    /// The area and the work area of its screen (logical px), that screen's
    /// origin (logical desktop px) and scale.
    area: Rectangle,
    work: Size,
    origin: Vector,
    scale: f32,
    /// The picture's logical size so far.
    picture: Size,
    /// The tray's label as last shown, and when (`LABEL_EVERY`).
    label: String,
    labelled: Option<Instant>,
    /// The pop-up sits inside the area (`popup_room`): it shows only once
    /// the first frame is read, with the whole-screen tip until the page
    /// grows past it (`first`: the area's height, physical px).
    inside: bool,
    first: u32,
}

/// The tray's label changes at most this often: it may be in the stream (the
/// top bar, over a whole-screen area), and each change is a D-Bus round.
const LABEL_EVERY: Duration = Duration::from_millis(500);

impl Cast {
    /// The pop-up and the tray after a change (`preview`: the new picture).
    fn show(&mut self, height: u32, full: bool, lost: bool, preview: Option<Crop>) {
        let label = format!("● {}", long_capture::px(height));
        // ponytail: the last change within LABEL_EVERY waits for the next
        // update (the pop-up has the exact count); a timer if it matters
        let due = self.labelled.is_none_or(|t| t.elapsed() >= LABEL_EVERY);
        if label != self.label && due {
            crate::shared::platform::tray::capturing(Some(label.clone()));
            (self.label, self.labelled) = (label, Some(Instant::now()));
        }
        let Some(popup) = &self.popup else { return };
        if self.inside && height == 0 {
            return; // not in the first frame
        }
        let tip = self.inside && height <= self.first;
        let picture = preview.filter(|p| p.width > 0 && p.height > 0).map(|p| {
            let size = Size::new(p.width as f32, p.height as f32) * (1.0 / self.scale);
            (p, size)
        });
        if let Some((_, size)) = &picture {
            self.picture = *size;
        }
        let Some((window, left)) =
            long_capture::popup_at(self.area, self.work, self.picture, tip, self.scale)
        else {
            return;
        };
        popup.frame(long_capture_popup::Frame {
            window: window + self.origin,
            left,
            inside: self.inside,
            tip,
            count: (height, full, lost),
            picture,
        });
    }

    #[cfg(test)]
    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// `size` (logical px) in physical px at `scale`.
fn physical(size: Size, scale: f32) -> (u32, u32) {
    let px = |v: f32| (v * scale).round() as u32;
    (px(size.width), px(size.height))
}

/// Long capture's thread: after `wait`, `region` grabbed and stitched
/// (`scroll::by_hand`, `cover` its pop-up's corner) until `stop`, then `Msg::Done`.
fn stitch(
    region: (i32, i32, u32, u32),
    preview: (u32, u32),
    cover: (u32, u32),
    wait: Duration,
    stop: Arc<AtomicBool>,
) -> Task<Message> {
    background(move |tx| {
        std::thread::sleep(wait);
        let each = |update| {
            let _ = tx.unbounded_send(snip::Msg::Progress(update));
        };
        let max = config::current().snip.long_max;
        let stitched = crate::shared::platform::capture::scroll::by_hand(
            region, preview, cover, max, &stop, each,
        );
        let _ = tx.unbounded_send(snip::Msg::Done(stitched));
    })
    .map(Message::Snip)
}

/// Copies a snip on a thread (encoding a big capture takes a while), through
/// the one copy path.
fn copy_snip(crop: Crop) -> Task<Message> {
    background(move |_| {
        if let Run::Finish(copy) = copy::ACTION.run
            && let Err(e) = copy(crop)
        {
            crate::log!("copy failed: {e}");
        }
    })
}
