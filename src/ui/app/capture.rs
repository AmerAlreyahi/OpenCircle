//! Opening an overlay: the capture, the frame, showing and closing the window.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::image;
use iced::window::{self, Level, Mode};
use iced::{Point, Size, Task};

use crate::actions::{self, Action, Run, text};
use crate::capture::{self, CaptureError};
use crate::config::{self};
use crate::labs::BarButton;
use crate::ocr::{self};

use crate::ui::app::background::background;
use crate::ui::app::message::{BarKeys, Hiding, Message, Purpose, Shot};
use crate::ui::app::opening::Opening;
use crate::ui::app::quick;
use crate::ui::app::quick::{Quick, Start};
use crate::ui::kit::motion;
use crate::ui::platform::monitor::{place_window, taskbar_inset};
use crate::ui::platform::shape;
use crate::ui::snip::Snip;
use crate::ui::{overlay, snip, windows};

use super::{App, CAPTURE_TIMEOUT, UIA_WINDOWS};

impl App {
    /// Captures on a thread: a Wayland portal may wait minutes for the user's OK.
    /// `snip`: Snip mode in this kind; also lists the windows, while the overlay is
    /// still hidden.
    /// After `CAPTURE_TIMEOUT` the capture is given up (the next trigger works) and
    /// its answer, if it still comes, is dropped.
    pub(in crate::ui) fn capture(
        &mut self,
        at: Instant,
        snip: Option<snip::Kind>,
    ) -> Task<Message> {
        self.captures = self.captures.wrapping_add(1);
        let n = self.captures;
        self.capturing = Some(n);
        let timeout = background(|tx| {
            std::thread::sleep(CAPTURE_TIMEOUT);
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::CaptureTimedOut(n));
        let shot = background(move |tx| {
            let shot = match snip {
                Some(kind) => Self::snip_shot(kind),
                // The monitor of the window the user is in (else the pointer's).
                None => capture::active_monitor().map(|frame| {
                    let (place, image) = snip::screen_of(frame);
                    Shot {
                        origin: place.logical_origin(),
                        size: place.size(),
                        image,
                        scale: place.scale,
                        snip: None,
                    }
                }),
            };
            let _ = tx.unbounded_send(shot);
        })
        .map(move |shot| Message::Captured(n, at, shot));
        Task::batch([shot, timeout])
    }

    /// Snip: every monitor at once (the overlay shows the pointer's, then follows
    /// the pointer) and every window, while the overlay is still hidden.
    pub(in crate::ui) fn snip_shot(kind: snip::Kind) -> Result<Shot, CaptureError> {
        let capture::snip::Snapshot {
            frames,
            on,
            windows,
            foreground,
        } = capture::snip::snapshot(config::current().snip.cursor)?;
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
        })
    }

    pub(in crate::ui) fn open(
        &mut self,
        at: Instant,
        shot: Result<Shot, CaptureError>,
    ) -> Task<Message> {
        self.capturing = None;
        let shot = match shot {
            Ok(shot) => shot,
            Err(e) => {
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
            ..Opening::default()
        });
        self.set_frame(shot.image, shot.scale, shot.origin, size);
        self.o.chat_hole = (self.chat.as_ref())
            .filter(|c| c.window.is_some())
            .map(|c| iced::Rectangle::new(c.at - (shot.origin - Point::ORIGIN), c.size));
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
        if let Some((kind, windows, foreground, screens)) = shot.snip {
            let mut snip = Snip::new(
                windows,
                self.place(),
                crate::capture::scroll::can_scroll(),
                foreground,
            );
            snip.set_screens(screens);
            first = snip.start(kind, size); // Full = the whole screen at once
            let wait = config::current().snip.delay;
            if !std::mem::take(&mut self.timed) && wait > 0 && first.is_none() {
                timer = Some(wait); // Settings > Snip > Timer: hide, count down, capture again
            }
            let (reader, read) = std::sync::mpsc::channel();
            if cfg!(windows) {
                snip.set_reader(reader, UIA_WINDOWS); // else dropped: no controls
            }
            self.o.purpose = Purpose::Snip(Box::new(snip));
            extras = self.snip_extras(read);
        }
        let Purpose::Quick(quick, target) = self.o.purpose else {
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
            return Task::batch([next.chain(self.show()), extras, text]);
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
    pub(in crate::ui) fn set_frame(
        &mut self,
        image: image::Handle,
        scale: f32,
        origin: Point,
        size: Size,
    ) {
        let px = |v: f32| (v * scale).round() as u32;
        self.o.view = overlay::View::new(px(size.width), px(size.height), size);
        self.o.zoom = None;
        self.o.tiles = overlay::tiles(&image).map(|t| (image.id(), t));
        self.o.frame = Some(image);
        self.o.scale = scale;
        self.o.screen = size;
        self.o.origin = origin;
        self.o.work_inset = taskbar_inset(origin, size, scale);
    }

    /// The shown monitor in physical desktop px.
    pub(in crate::ui) fn place(&self) -> snip::Place {
        let px = |v: f32| (v * self.o.scale).round();
        snip::Place {
            origin: (px(self.o.origin.x) as i32, px(self.o.origin.y) as i32),
            px: (
                px(self.o.screen.width) as u32,
                px(self.o.screen.height) as u32,
            ),
            scale: self.o.scale,
        }
    }

    /// Shows the (hidden or pill-sized) window over the whole monitor, or opens
    /// it full-screen where it was closed (see `can_hide_windows`).
    pub(in crate::ui) fn show(&mut self) -> Task<Message> {
        if !self.overlay_open {
            let (id, open) = windows::screen(overlay_window(Some(self.o.screen)));
            crate::log!("[opencircle] overlay window opened");
            (self.overlay, self.overlay_open) = (id, true);
            let chat = self.unfloat_chat();
            return open.discard().chain(window::gain_focus(id)).chain(chat);
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
        // Place and size the window to cover the monitor BEFORE set_mode shows it:
        // iced makes it visible before going full-screen, so whatever is shown
        // first must already cover the monitor (no small black box flash).
        let show = windows::uncut(self.overlay)
            .map(Message::Windows)
            .chain(clear)
            .chain(place_window(self.overlay, self.place()))
            .chain(window::set_mode(self.overlay, Mode::Fullscreen))
            .chain(window::gain_focus(self.overlay))
            .chain(chat);
        // A hand-off to the browser drops the overlay out of the topmost band
        // behind iced's back (`lens::hand_off_foreground`): Normal then
        // AlwaysOnTop makes it apply again.
        #[cfg(windows)]
        let show = window::set_level(self.overlay, Level::Normal)
            .chain(window::set_level(self.overlay, Level::AlwaysOnTop))
            .chain(show);
        show
    }

    /// Shows `message` over the frame for a moment, then closes the overlay.
    pub(in crate::ui) fn toast_then_close(&mut self, message: &'static str) -> Task<Message> {
        self.o.toast = Some(message);
        background(|tx| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = tx.unbounded_send(());
        })
        .map(|()| Message::ToastDone)
    }

    /// Test hook: shoot what the overlay shows after the next frame, then close.
    pub(in crate::ui) fn test_shot(&self) -> Task<Message> {
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
    pub(in crate::ui) fn close(&mut self) -> Task<Message> {
        if let Some(scroll) = self.scroll() {
            scroll.stop();
        }
        // Everything the opening held goes at once (the panel, translate and
        // song stop; the next show clears the window again).
        // a pinned chat goes on in its own window
        let chat = self.float_chat();
        let Some(opening) = self.o.take() else {
            return Task::none(); // closed already
        };
        opening.finish();
        // hidden or closed alike: the editor's glyphs go with the opening
        crate::editor::free_glyphs();
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
        crate::sys::mem::trim_heap();
        chat.chain(window::close(self.overlay))
    }
}

/// The overlay window: hidden (pre-created), or full-screen at `size`. Never on
/// the taskbar (Windows): it leaves full-screen visible while it closes (`Hiding`).
pub(in crate::ui) fn overlay_window(shown: Option<Size>) -> window::Settings {
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
pub(in crate::ui) const TEST_SHOT: &str = "OPENCIRCLE_TEST_SHOT";

pub(in crate::ui) fn save_test_shot(shot: &window::Screenshot) {
    let Some(path) = std::env::var_os(TEST_SHOT) else {
        return;
    };
    let result = actions::write_png(path.as_ref(), shot.size.width, shot.size.height, &shot.rgba);
    if let Err(e) = result {
        crate::log!("test shot failed: {e}");
    }
}
