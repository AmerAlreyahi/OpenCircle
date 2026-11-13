//! Opening an overlay: the capture, the frame, showing and closing the window.

use crate::shared::app_language::Said;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::image;
use iced::window::{self, Level, Mode};
use iced::{Point, Size, Task};

use crate::features::settings::labs::BarButton;
use crate::features::text_reading as ocr;
use crate::features::{
    circle::actions::{self, Action, Run},
    text_reading::text,
};
use crate::shared::config;
use crate::shared::platform::capture::{self, CaptureError};

use crate::app::background::background;
use crate::app::message::{BarKeys, Hiding, Message, Purpose, Shot};
use crate::app::opening::Opening;
use crate::features::circle::app::quick;
use crate::features::circle::app::quick::{Quick, Start};
use crate::features::snip::screens::snip::Snip;
use crate::shared::kit::motion;
use crate::shared::platform::monitor::{place_window, taskbar_inset};
use crate::shared::platform::shape;
use crate::{
    app::windows,
    features::{circle::screens::overlay, snip::screens::snip},
};

use super::{App, CAPTURE_TIMEOUT, UIA_WINDOWS};
use crate::shared::platform::monitor::can_hide_windows;

/// How long the grab waits for the screen to redraw without our floating
/// windows (`App::leave_frame`): GNOME animates a closing window for ~150 ms
/// (at 80 ms the Ubuntu VM's portal shot still held a fading Steps strip).
const LEAVE_FRAME: Duration = Duration::from_millis(250);

impl App {
    /// Captures on a thread: a Wayland portal may wait minutes for the user's OK.
    /// `snip`: Snip mode in this kind; also lists the windows, while the overlay is
    /// still hidden.
    /// After `CAPTURE_TIMEOUT` the capture is given up (the next trigger works) and
    /// its answer, if it still comes, is dropped.
    pub(crate) fn capture(&mut self, at: Instant, snip: Option<snip::Kind>) -> Task<Message> {
        self.captures = self.captures.wrapping_add(1);
        let n = self.captures;
        self.capturing = Some(n);
        let timeout = background(|tx| {
            std::thread::sleep(CAPTURE_TIMEOUT);
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::CaptureTimedOut(n));
        #[cfg(target_os = "linux")]
        let timeout = Task::batch([
            timeout,
            background(|tx| {
                std::thread::sleep(super::CAPTURE_SLOW);
                let _ = tx.unbounded_send(());
            })
            .map(move |()| Message::CaptureSlow(n)),
        ]);
        // Circle to Search freezes every monitor (`app::screens`).
        let all = snip.is_none() && matches!(self.next, Purpose::Search);
        // Snip picking an area at once: the overlay gets ready meanwhile (`pre_show`).
        let early = snip.is_some_and(|k| k.picks()) && config::current().snip.delay == 0;
        // (`background` starts its thread at once: made only when the grab may run)
        let shot = move || {
            background(move |tx| {
                // (a closure: its returns end the grab, then `Done`)
                (|| {
                    let Some(kind) = snip else {
                        // The monitor the mouse is on first: its overlay opens on it;
                        // then the others.
                        // The windows as frozen, listed beside the grab (the overlay
                        // isn't up yet): the bar's Snip outlines them (`snip_here`).
                        let mut listing = Some(std::thread::spawn(capture::window_rect::list));
                        let mut first = true;
                        capture::mouse_first(all, |frame| {
                            let grabbed = match (std::mem::take(&mut first), frame) {
                                (true, frame) => Grabbed::Active(frame.map(|frame| {
                                    let (place, image) = snip::screen_of(frame);
                                    let windows = (listing.take().map(|l| l.join()))
                                        .and_then(Result::ok)
                                        .unwrap_or_default();
                                    Shot {
                                        origin: place.logical_origin(),
                                        size: place.size(),
                                        image,
                                        scale: place.scale,
                                        snip: None,
                                        windows: (windows.into_iter())
                                            .map(|w| {
                                                let rect = snip::desktop_rect(w.rect);
                                                snip::Pick::window(rect, &w.title, w.handle)
                                            })
                                            .collect(),
                                    }
                                })),
                                (false, Ok(frame)) => {
                                    let (place, image) = snip::screen_of(frame);
                                    Grabbed::Other(place, image)
                                }
                                (false, Err(_)) => return, // (`mouse_first` hands over none)
                            };
                            let _ = tx.unbounded_send(grabbed);
                        });
                        return;
                    };
                    let chosen = |m: &capture::Monitor| {
                        if !early {
                            return;
                        }
                        let (x, y, w, h) = m.rect;
                        let place = snip::Place {
                            origin: (x, y),
                            px: (w, h),
                            scale: m.scale,
                        };
                        let _ = tx.unbounded_send(Grabbed::Chosen(place));
                    };
                    let _ = tx.unbounded_send(Grabbed::Active(Self::snip_shot(kind, chosen)));
                })();
                let _ = tx.unbounded_send(Grabbed::Done);
            })
            .map(move |grabbed| match grabbed {
                Grabbed::Active(shot) => Message::Captured(n, at, shot),
                Grabbed::Other(place, image) => Message::Screen(n, place, image),
                Grabbed::Chosen(place) => Message::Chosen(n, place),
                Grabbed::Done => Message::Grabbed(n),
            })
        };
        // our windows leave the frame first (Linux), the screen redraws; Snip's
        // grab has them in, to pick one (owner, 2026-11-10): they leave once it
        // is over, before its overlay shows (`Message::Captured`)
        let out = match snip {
            Some(_) => None,
            None => self.leave_frame(n),
        };
        let wait = grab_wait(out.is_some(), self.overlay_gone, Instant::now());
        let shot = match wait.is_zero() {
            true => shot(),
            false => {
                let mut shot = Some(shot);
                let redrawn = background(move |tx| {
                    std::thread::sleep(wait);
                    let _ = tx.unbounded_send(());
                });
                (out.unwrap_or_else(Task::none).discard().chain(redrawn))
                    .then(move |()| shot.take().map_or_else(Task::none, |shot| shot()))
            }
        };
        Task::batch([shot, timeout])
    }

    /// Linux: our floating windows out of capture `n`'s grab (`Windows::leave_frame`;
    /// Windows keeps them out of every capture). The chat's own window hides
    /// till the grab is over (X11, `grabbed`: then it is live above the
    /// overlay), or goes where windows can't hide (the overlay draws the chat).
    /// `None`: none was there.
    pub(crate) fn leave_frame(&mut self, n: u32) -> Option<Task<Message>> {
        let out = (self.windows.leave_frame(n)).map(|out| out.map(Message::Windows));
        let own = self.own_window();
        let gone = match self.chat_win.as_mut() {
            _ if cfg!(windows) => None,
            Some(w) if own && !w.hidden => {
                (w.out, w.hidden) = (true, true);
                Some(window::set_mode(w.id, Mode::Hidden))
            }
            // ponytail: a capture that then fails leaves the chat for the next overlay.
            Some(_) if !own && !can_hide_windows() => Some(self.unfloat_chat()),
            _ => None,
        };
        match (out, gone) {
            (None, None) => None,
            (out, gone) => Some(Task::batch([
                out.unwrap_or_else(Task::none),
                gone.unwrap_or_else(Task::none),
            ])),
        }
    }

    /// Capture `n`'s grab is done: what left its frame comes back unless the
    /// overlay is up (then when it closes, `close`: no window over it).
    pub(crate) fn grabbed(&mut self, n: u32) -> Task<Message> {
        if let Some(w) = &mut self.chat_win {
            w.out = false; // (shown again by `sync_chat_window`)
        }
        match self.o.is_some() {
            true => Task::none(),
            false => self.windows.back_in_frame(Some(n)).map(Message::Windows),
        }
    }

    /// Snip: every monitor at once (the overlay shows the pointer's, then follows
    /// the pointer) and every window; `chosen` gets the pointer's monitor first
    /// (`App::pre_show`).
    pub(crate) fn snip_shot(
        kind: snip::Kind,
        chosen: impl FnOnce(&capture::Monitor),
    ) -> Result<Shot, CaptureError> {
        let crate::features::snip::capture::snip::Snapshot {
            frames,
            on,
            windows,
            foreground,
        } = crate::features::snip::capture::snip::snapshot(config::current().snip.cursor, chosen)?;
        let windows = (windows.into_iter())
            .map(|w| snip::Pick::window(snip::desktop_rect(w.rect), &w.title, w.handle))
            .collect();
        let screens: Vec<_> = frames.into_iter().map(snip::screen_of).collect();
        let (place, image) = screens.get(on).cloned().ok_or("no monitor found")?;
        Ok(Shot {
            origin: place.logical_origin(),
            size: place.size(),
            image,
            scale: place.scale,
            snip: Some((kind, windows, foreground, screens)),
            windows: Vec::new(),
        })
    }

    pub(crate) fn open(&mut self, at: Instant, shot: Result<Shot, CaptureError>) -> Task<Message> {
        self.capturing = None;
        let shot = match shot {
            Ok(shot) => shot,
            Err(e) => {
                self.unshow();
                crate::log!("capture failed: {e}");
                self.next = Purpose::Search;
                if !e.is_refused() {
                    return Task::none();
                }
                // Wayland: the desktop asks for permission only while we are in front.
                return self
                    .windows
                    .open(windows::Kind::Permission)
                    .map(Message::Windows);
            }
        };
        crate::log!(
            "[opencircle] capture {}x{} took {} ms",
            shot.size.width * shot.scale,
            shot.size.height * shot.scale,
            at.elapsed().as_millis()
        );
        let size = shot.size;
        let purpose = std::mem::replace(&mut self.next, Purpose::Search);
        self.o.start(Opening {
            purpose,
            windows: shot.windows,
            ..Opening::default()
        });
        self.set_frame(shot.image, shot.scale, shot.origin, size);
        self.find_chat(shot.origin);
        self.o.helper = Some(Arc::new(ocr::hold()));
        self.o.ocr_ready = text::models_present();
        self.frame_times.borrow_mut().clear();
        self.opening = self.opening.wrapping_add(1);
        self.first_draw.set(Some((at, size)));
        self.hiding = Hiding::No;
        let cfg = config::current();
        self.o.appear = None; // its first frame is t = 0
        self.o.pill = BarButton::Search;
        self.o.bar_anchor = cfg.bar.anchor;
        self.o.bar_now = None;
        self.o.bar_glide = None;
        motion::refresh();
        self.o.behaviour = cfg.selection;
        self.o.bar_keys = BarKeys::of(&cfg);
        self.o.labs = cfg.labs;
        let mut first = None;
        let mut extras = Task::none();
        let mut timer = None;
        let mut step = None;
        if let Some((kind, windows, foreground, screens)) = shot.snip {
            let mut snip = Snip::new(
                windows,
                self.place(),
                crate::features::snip::capture::scroll::can_scroll(),
                foreground,
            );
            // Every other monitor gets its window over its frame once the
            // opening is up (`app::screens`, as Circle to Search): one area may
            // span them.
            let start = self.place();
            self.o.waiting = (screens.into_iter())
                .filter(|(p, _)| p.origin != start.origin)
                .collect();
            snip.set_monitors(self.monitors());
            first = snip.start(kind, size); // Full = the whole screen at once
            let wait = config::current().snip.delay;
            let tool = self.tool_next.take();
            step = self.picked_next.take();
            if step.is_some() {
                (first, self.timed) = (None, true); // no pick, no timer: the step is picked
            }
            if !std::mem::take(&mut self.timed) && wait > 0 && first.is_none() && tool.is_none() {
                timer = Some(wait); // Settings > Snip > Timer: hide, count down, capture again
            }
            let (reader, read) = std::sync::mpsc::channel();
            if cfg!(windows) {
                snip.read_later(reader, UIA_WINDOWS); // else dropped: no controls
            }
            snip.live_pin = std::mem::take(&mut self.live_next);
            self.o.purpose = Purpose::Snip(Box::new(snip));
            extras = self.snip_extras(read);
            if let Some(tool) = tool {
                extras = Task::batch([extras, self.start_tool(tool)]);
            }
        }
        let Purpose::Quick(quick, target) = self.o.purpose else {
            // a step of the Steps strip to update: picked at once ("Update
            // step N"); a long capture finished on Wayland: picked as well
            if let Some((i, crop)) = step {
                let _ = self.snip_routed(crop, false, false);
                if let Some(edit) = &mut self.o.edit {
                    edit.annotator.updating = i;
                }
            }
            let next = match (first, timer) {
                (Some(next), _) => self.snip_next(Some(next)),
                (None, Some(secs)) => self.on_snip(snip::Msg::Delay(secs)),
                (None, None) => Task::none(),
            };
            if self.snip_pill() {
                return Task::batch([next, extras]); // the toast is the window
            }
            self.note_reader();
            // Windows OCR reads the whole screen while the overlay comes up
            // (#62: ready ~0.1 s after its first frame); the helper waits (#123).
            let text = match self.system_text() {
                true => self.read_more(),
                false => Task::none(),
            };
            return Task::batch([next.chain(self.shown()), extras, text]);
        };
        if let Quick::Song = quick {
            let song = self.run_quick(quick); // listens at once, no area
            return self.show().chain(song);
        }
        let Start::Area(area) = quick::start(target, size, self.last_region()) else {
            return self.show(); // the first selection runs it
        };
        self.select(area);
        if let Quick::Action(
            action @ Action {
                run: Run::Finish(_),
                ..
            },
        ) = quick
        {
            // Copy, Save: nothing to show; the window stays hidden.
            let done = self.run(action);
            self.hiding = Hiding::No;
            return done;
        }
        let run = self.run_quick(quick);
        self.show().chain(run)
    }

    /// The frozen frame of the monitor at `origin` (logical px, `size`) becomes
    /// what the overlay shows.
    pub(crate) fn set_frame(
        &mut self,
        image: image::Handle,
        scale: f32,
        origin: Point,
        size: Size,
    ) {
        let px = |v: f32| (v * scale).round() as u32;
        self.o.view = overlay::View::new(px(size.width), px(size.height), size);
        self.o.zoom = None;
        self.o.frame = Some(image);
        self.o.scale = scale;
        self.o.screen = size;
        self.o.origin = origin;
        self.o.work_inset = taskbar_inset(origin, size, scale);
    }

    /// The shown monitor in physical desktop px.
    pub(crate) fn place(&self) -> snip::Place {
        self.o.place()
    }

    /// Shows the (hidden or pill-sized) window over the whole monitor, or opens
    /// it full-screen where it was closed (see `can_hide_windows`).
    pub(crate) fn show(&mut self) -> Task<Message> {
        if !self.overlay_open {
            windows::activate_next();
            let (id, open) = windows::screen(overlay_window(Some(self.o.screen)));
            crate::log!("[opencircle] overlay window opened");
            (self.overlay, self.overlay_open) = (id, true);
            let chat = self.unfloat_chat();
            return open.discard().chain(windows::foreground(id)).chain(chat);
        }
        // Shown fully clear: it fades in as a whole once its first full frame
        // is drawn (`Message::Frame`), so the CPU never redraws a fade.
        let clear = match shape::can_fade() {
            true => {
                self.o.reveal = Some(Instant::now());
                if self.first_draw.get().is_none() {
                    self.first_draw.set(Some((Instant::now(), self.o.screen)));
                }
                windows::opacity(self.overlay, 0.0)
            }
            // no fade to wait for: the chat's window goes once the overlay is up
            false => Task::none(),
        };
        let chat = match shape::can_fade() {
            true => Task::none(), // at the reveal (`Message::Frame`)
            false => self.unfloat_chat(),
        };
        self.show_steps(self.place(), clear, chat)
    }

    /// Snip: the overlay shows on the monitor it will open on, fully clear,
    /// while the screen is grabbed (`Message::Chosen`): showing the window
    /// (~130 ms on the VM, most of it going full-screen) overlaps the grab
    /// instead of following it. Only a ready hidden window that fades (Windows):
    /// a clear layered window is in no GDI grab and our windows are never
    /// listed. `open` then only fades it in; a capture that ends without
    /// opening hides it again (`unshow`).
    pub(crate) fn pre_show(&mut self, place: snip::Place) -> Task<Message> {
        let ready = self.can_hide
            && shape::can_fade()
            && self.overlay_open
            && self.hiding == Hiding::No
            && self.o.is_none()
            && !self.pre_shown;
        if !ready {
            return Task::none();
        }
        self.pre_shown = true;
        let clear = windows::opacity(self.overlay, 0.0);
        self.show_steps(place, clear, Task::none())
    }

    /// `show` for the opening, unless `pre_show` did already: then it only
    /// waits for its first frame to fade in.
    fn shown(&mut self) -> Task<Message> {
        if !std::mem::take(&mut self.pre_shown) {
            return self.show();
        }
        self.o.reveal = Some(Instant::now());
        Task::none()
    }

    /// A capture ended without opening: the window `pre_show` showed is hidden
    /// again (the steps `close` takes).
    pub(crate) fn unshow(&mut self) {
        if std::mem::take(&mut self.pre_shown) {
            windows::clear_now(self.overlay);
            self.hiding = Hiding::Empty;
        }
    }

    /// The steps that show the overlay over the monitor at `place`: `clear`
    /// (its opacity) first, `chat` last.
    fn show_steps(
        &self,
        place: snip::Place,
        clear: Task<Message>,
        chat: Task<Message>,
    ) -> Task<Message> {
        // Place and size the window to cover the monitor BEFORE set_mode shows it:
        // iced makes it visible before going full-screen, so whatever is shown
        // first must already cover the monitor (no small black box flash).
        let show = windows::uncut(self.overlay)
            .map(Message::Windows)
            .chain(clear)
            .chain(place_window(self.overlay, place))
            .chain(windows::show(self.overlay, Mode::Fullscreen))
            .chain(windows::foreground(self.overlay));
        // (Windows: topmost again once it fades in, `relevel`)
        show.chain(chat)
    }

    /// A hand-off to the browser drops the overlay out of the topmost band
    /// behind iced's back (`search_in_browser::hand_off_foreground`): Normal then
    /// AlwaysOnTop makes it apply again. At the fade-in, not in `show_steps`:
    /// each step is a round trip the first frame waited for (~30 ms on the VM).
    pub(crate) fn relevel(&self) -> Task<Message> {
        #[cfg(windows)]
        return window::set_level(self.overlay, Level::Normal)
            .chain(window::set_level(self.overlay, Level::AlwaysOnTop));
        #[cfg(not(windows))]
        Task::none()
    }

    /// Shows `message` over the frame for a moment, then closes the overlay.
    pub(crate) fn toast_then_close(&mut self, message: Said) -> Task<Message> {
        self.o.toast = Some(message);
        Self::hold_then_close()
    }

    /// `ToastDone` once a pill over the frame has been read (it then closes).
    pub(crate) fn hold_then_close() -> Task<Message> {
        background(|tx| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = tx.unbounded_send(());
        })
        .map(|()| Message::ToastDone)
    }

    /// Test hook: shoot what the overlay shows after the next frame, then close.
    pub(crate) fn test_shot(&self) -> Task<Message> {
        // No message (= no relayout) in between: a relayout drops the text
        // editor's drawn text.
        let overlay = self.overlay;
        background(|tx| {
            std::thread::sleep(Duration::from_millis(150));
            let _ = tx.unbounded_send(());
        })
        .then(move |()| window::screenshot(overlay))
        .map(Message::TestShot)
    }

    /// Frees the frame and drops the panel (stopping its work); Lens pages go
    /// once old enough. The window is shrunk and hidden in steps (see `Hiding`).
    pub(crate) fn close(&mut self) -> Task<Message> {
        // Wayland: a pinned chat drawn on the overlay: its own window comes up
        // over the same spot first (no blank beat); the overlay goes when it is
        // up. Elsewhere the chat has its window already: nothing changes for it.
        if self.o.is_some() && self.chat.is_some() && self.chat_win.is_none() && !self.own_window()
        {
            return self.float_chat().chain(Task::done(Message::ChatHandedOver));
        }
        // a long capture ends with it; the window shows in screenshots again
        let mut chat = Task::none();
        if let Some(long) = self.scroll() {
            long.stop();
            chat = windows::hide_from_capture(self.overlay, false).map(Message::Windows);
        }
        // Everything the opening held goes at once (the panel, translate and
        // song stop; the next show clears the window again).
        // a pinned chat goes on in its own window
        let chat = chat.chain(self.float_chat());
        let Some(opening) = self.o.take() else {
            return Task::none(); // closed already
        };
        if !cfg!(windows) {
            self.overlay_gone = Some(Instant::now()); // its windows fade out (`grab_wait`)
        }
        // the other monitors' windows go with it, at once; what left the frame is back
        let back = self.windows.back_in_frame(None).map(Message::Windows);
        let chat = Task::batch([
            chat,
            crate::app::monitors::close_all(&opening.screens),
            back,
        ]);
        drop(opening); // the panel, translate, song, helper and frame stop or free themselves
        // hidden or closed alike: the editor's glyphs go with the opening
        crate::features::snip::annotate::free_glyphs();
        if self.can_hide {
            // Gone at once: the hide steps draw the empty view (the theme's
            // background) full-screen, then restore the window from full-screen,
            // all while it is still shown. Unseen, at opacity 0 (`show` clears
            // and fades it in again).
            windows::clear_now(self.overlay);
            self.hiding = Hiding::Empty;
            return chat;
        }
        if !std::mem::take(&mut self.overlay_open) {
            return chat; // never shown (Copy, Save)
        }
        crate::log!("[opencircle] overlay window closed");
        // the window's buffers are freed after the close: trim then, not now
        crate::shared::platform::sys::mem::trim_heap_soon();
        chat.chain(window::close(self.overlay))
    }
}

/// How long a grab waits for the screen to redraw without our windows: the
/// whole `LEAVE_FRAME` when some just left it (`left`), else what is left of it
/// since the last opening's windows went (`gone`, Linux: an Esc then the shortcut
/// at once froze the closing overlay, its chips and all, into the next frame).
pub(crate) fn grab_wait(left: bool, gone: Option<Instant>, now: Instant) -> Duration {
    match (left, gone) {
        (true, _) => LEAVE_FRAME,
        (false, Some(gone)) => LEAVE_FRAME.saturating_sub(now.saturating_duration_since(gone)),
        (false, None) => Duration::ZERO,
    }
}

/// What a capture's thread hands over: the overlay's monitor, then each other
/// one (snip: before them, where the overlay will open).
enum Grabbed {
    /// The grab is over (every monitor's).
    Done,
    Chosen(snip::Place),
    Active(Result<Shot, CaptureError>),
    Other(snip::Place, image::Handle),
}

/// The overlay window: hidden (pre-created), or full-screen at `size`. Never on
/// the taskbar (Windows): it leaves full-screen visible while it closes (`Hiding`).
pub(crate) fn overlay_window(shown: Option<Size>) -> window::Settings {
    #[cfg(windows)]
    let platform_specific = window::settings::PlatformSpecific {
        skip_taskbar: true,
        ..Default::default()
    };
    #[cfg(not(windows))]
    let platform_specific = Default::default();
    window::Settings {
        platform_specific,
        size: shown.unwrap_or(window::Settings::default().size),
        visible: shown.is_some(),
        fullscreen: shown.is_some(),
        decorations: false,
        resizable: false,
        level: Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..window::Settings::default()
    }
}

/// Test hook: on Esc, save what the overlay shows before closing (WSLg can't grab it).
pub(crate) const TEST_SHOT: &str = "OPENCIRCLE_TEST_SHOT";

pub(crate) fn save_test_shot(shot: &window::Screenshot) {
    let Some(path) = crate::shared::platform::sys::test_hook(TEST_SHOT) else {
        return;
    };
    let result = actions::write_png(path.as_ref(), shot.size.width, shot.size.height, &shot.rgba);
    if let Err(e) = result {
        crate::log!("test shot failed: {e}");
    }
}
