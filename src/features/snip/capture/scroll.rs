//! Long capture by hand (N-73 point 5): the user scrolls the page under a
//! picked area themselves (wheel, scrollbar, keys); this grabs only that area,
//! about 15 times a second while there is input, and stitches what passes
//! through it (`stitch.rs`) until Done. Nothing is injected: no auto-scroll,
//! so elevated windows (UIPI) work too.
//!
//! Windows reads every frame the screen shows from a Desktop Duplication of
//! the area's monitor, kept open only while the capture runs (it waits for a
//! new frame: a still screen costs nothing), and paces the wheel over the
//! area to what was seen (`pace`: a fast spin can't skip the page; owner,
//! 2026-11-03). Where duplication can't run (an area across monitors, a
//! rotated one, a remote session) or stops, it grabs the area alone with GDI
//! (`BitBlt`), ~15 times a second and only while the user is giving input or
//! the area still moves (`GetLastInputInfo`). Our own windows are kept out
//! with `hide_from_capture` either way. X11 grabs the monitor and cuts the
//! area (no pacing). Wayland has no silent grab: there the frames come from a
//! ScreenCast stream of the area's monitor (`screencast::Cast`, the restore
//! token: no dialog once allowed), open only while the capture runs; no pacing
//! (no input injection there). Without that portal or GStreamer's PipeWire
//! source it isn't offered.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::stitch::{Push, Stitcher};
use crate::actions::Crop;

/// Time between two GDI grabs (~15 fps).
const PERIOD: Duration = Duration::from_millis(66);
/// Longest wait for a duplicated frame (the stop flag is read in between).
#[cfg(windows)]
const FRAME_WAIT_MS: u32 = 100;
#[cfg(target_os = "linux")]
const CAST_WAIT: Duration = Duration::from_millis(100);
/// Grabs go on this long after the last input or the last change in the area
/// (a smooth scroll finishing, a page loading under it).
const SETTLE: Duration = Duration::from_secs(1);
/// Unmatched frames in a row after which the user is told to scroll back:
/// the page jumped further than one area's height between two grabs.
const LOST_AFTER: u32 = 3;
/// The preview is made at most this often.
const PREVIEW_EVERY: Duration = Duration::from_millis(200);

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
    /// More's Copy text / Ask AI: finish, then read the page's text and copy
    /// it / ask the AI about the stitched page.
    CopyText,
    AskAi,
    /// Wayland (the pop-up's or the tray's Done, any shortcut): Snip opens
    /// with the stitched page picked.
    Done,
}

/// What the capture has so far, sent after each change.
#[derive(Clone)]
pub struct Update {
    /// The stitched height, physical px.
    pub height: u32,
    /// The height limit is reached: nothing more is added.
    pub full: bool,
    /// The page moved too far between two grabs to match: scroll back a little.
    pub lost: bool,
    /// The newest rows, scaled to the preview's size (when they changed).
    pub preview: Option<Crop>,
}

impl std::fmt::Debug for Update {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Update({} px, full {}, lost {})",
            self.height, self.full, self.lost
        )
    }
}

/// Long capture can run: the area can be grabbed silently, or (Wayland)
/// streamed by the ScreenCast portal.
pub fn can_scroll() -> bool {
    #[cfg(target_os = "linux")]
    if crate::shared::platform::capture::wayland_session() {
        let stream = || {
            crate::shared::platform::capture::portal::session_bus()
                .is_ok_and(|c| crate::shared::platform::capture::screencast::can_stream(&c))
        };
        return offered(true, stream);
    }
    true
}

/// Long capture is offered: always where the screen can be grabbed silently,
/// on Wayland only where it can be streamed (`can_stream`, asked only there).
#[cfg(any(target_os = "linux", test))]
fn offered(wayland: bool, can_stream: impl FnOnce() -> bool) -> bool {
    !wayland || can_stream()
}

/// Grabs `region` (physical desktop px: x, y, width, height) while the user
/// scrolls, until `stop` or `max` px tall; `each` gets every change, with a
/// `preview` of the newest rows at most that size (physical px); `cover`: our
/// own pop-up's corner over the area's top right after the first frame
/// (`Stitcher::cover`; zero: none). Blocking: run it on a thread.
/// The image gathered so far is returned also when a later grab fails.
pub fn by_hand(
    region: (i32, i32, u32, u32),
    preview: (u32, u32),
    cover: (u32, u32),
    max: u32,
    stop: &AtomicBool,
    each: impl FnMut(Update),
) -> Result<Crop, String> {
    let mut st = Stitcher::new(region.2, region.3)
        .limit(max)
        .cover(cover.0, cover.1);
    let started = Instant::now();
    let source = std::cell::RefCell::new(Source::open(region));
    // (unhooked when it goes, as this returns: what it held is replayed)
    let paced = super::pace::over(region);
    let grab = || source.borrow_mut().next(region);
    // duplication waits for frames itself: no input check, no period
    let idle = || match source.borrow().fast() {
        true => Duration::ZERO,
        false => crate::shared::platform::sys::session::idle_for(),
    };
    let period = || match source.borrow().fast() {
        true => Duration::ZERO,
        false => PERIOD,
    };
    let moved = || {
        if let Some(p) = &paced {
            p.moved();
        }
    };
    let ran = run(
        &mut st,
        grab,
        idle,
        Instant::now,
        stop,
        period,
        preview,
        moved,
        each,
    );
    let source = source.into_inner();
    let secs = started.elapsed().as_secs_f64().max(0.001);
    match ran {
        Ok(times) => crate::log!(
            "[opencircle] long capture: {} px in {:.0} s; {}; {} grabs, {:.1} ms each, {:.1}/s; {} stitched, {:.1} ms each; {} previews, {:.1} ms each",
            st.height(),
            secs,
            source.describe(),
            times.grabs,
            source.ms_each(times.grab, times.grabs),
            f64::from(times.grabs) / secs,
            times.pushes,
            times.push.as_secs_f64() * 1000.0 / f64::from(times.pushes.max(1)),
            times.previews,
            times.preview.as_secs_f64() * 1000.0 / f64::from(times.previews.max(1)),
        ),
        Err(e) if st.height() == 0 => return Err(e.to_string()),
        Err(e) => crate::log!("[opencircle] long capture stopped early: {e}"),
    }
    let (rgba, width, height) = st.finish();
    Ok(Crop {
        width,
        height,
        rgba,
    })
}

/// Where frames come from: a Desktop Duplication (Windows) while it works,
/// a screen cast (Wayland), else a grab of the area each time.
struct Source {
    #[cfg(windows)]
    dup: Option<crate::shared::platform::capture::win::Duplication>,
    #[cfg(target_os = "linux")]
    cast: Option<crate::shared::platform::capture::screencast::Cast>,
    /// Why there is no duplication (it never started, or stopped).
    why: Option<String>,
    first: bool,
}

impl Source {
    fn open(region: (i32, i32, u32, u32)) -> Source {
        #[cfg(windows)]
        let (dup, why) = match crate::shared::platform::capture::win::Duplication::start(region) {
            Ok(dup) => (Some(dup), None),
            Err(e) => (None, Some(e)),
        };
        #[cfg(target_os = "linux")]
        let (cast, why) = match crate::shared::platform::capture::wayland_session() {
            true => match crate::shared::platform::capture::screencast::Cast::start(region) {
                Ok(cast) => (Some(cast), None),
                Err(e) => (None, Some(e)),
            },
            false => (None, None),
        };
        #[cfg(not(any(windows, target_os = "linux")))]
        let (_, why) = (region, None);
        Source {
            #[cfg(windows)]
            dup,
            #[cfg(target_os = "linux")]
            cast,
            why,
            first: true,
        }
    }

    /// Frames come as the screen shows them (no input check, no period).
    fn fast(&self) -> bool {
        #[cfg(windows)]
        return self.dup.is_some();
        #[cfg(target_os = "linux")]
        return self.cast.is_some();
        #[cfg(not(any(windows, target_os = "linux")))]
        false
    }

    /// The next frame; none when nothing new was shown. The first is a grab
    /// (duplication only says what changes).
    fn next(&mut self, region: (i32, i32, u32, u32)) -> io::Result<Option<Vec<u8>>> {
        let first = std::mem::take(&mut self.first);
        #[cfg(windows)]
        if !first && let Some(dup) = &mut self.dup {
            match dup.next(FRAME_WAIT_MS) {
                Ok(frame) => return Ok(frame),
                Err(e) => {
                    crate::log!("[opencircle] long capture: screen frames stopped ({e}); grabs");
                    (self.why, self.dup) = (Some(e), None);
                }
            }
        }
        let _ = first;
        #[cfg(target_os = "linux")]
        if let Some(cast) = &mut self.cast {
            return cast.next(CAST_WAIT).map_err(io::Error::other);
        }
        #[cfg(target_os = "linux")]
        if crate::shared::platform::capture::wayland_session() {
            // (a screenshot each time would be a portal session each, or a dialog)
            let why = self.why.as_deref().unwrap_or("no screen cast");
            return Err(io::Error::other(why.to_string()));
        }
        grab_area(region).map(Some)
    }

    /// For the log line.
    fn describe(&self) -> String {
        #[cfg(windows)]
        if let Some(dup) = &self.dup {
            return format!("screen frames ({} read)", dup.frames);
        }
        #[cfg(target_os = "linux")]
        if let Some(cast) = &self.cast {
            return format!("screen cast ({} frames read)", cast.frames);
        }
        match &self.why {
            Some(why) => format!("grabs (no screen frames: {why})"),
            None => "grabs".into(),
        }
    }

    /// Ms a frame took to read: the copy alone for duplicated frames (the
    /// loop's time includes waiting for them).
    fn ms_each(&self, total: Duration, n: u32) -> f64 {
        #[cfg(windows)]
        if let Some(dup) = &self.dup {
            return dup.copy.as_secs_f64() * 1000.0 / f64::from(dup.frames.max(1));
        }
        total.as_secs_f64() * 1000.0 / f64::from(n.max(1))
    }
}

/// How long the grabs and the stitching took (the log line).
#[derive(Debug, Default)]
struct Times {
    grabs: u32,
    grab: Duration,
    /// Frames handed to the stitcher (new ones: an unchanged frame is not)
    /// and the time it took.
    pushes: u32,
    push: Duration,
    previews: u32,
    preview: Duration,
}

/// The grab loop over any source: `grab` returns one RGBA frame of the
/// stitcher's size (none: nothing new shown), `idle` the time since the
/// user's last input, `now` the clock the settle and preview times are read
/// on (a test's own, so a slow machine can't change what happens). Grabs
/// every `period` while there was input or a change within `SETTLE`, and
/// never the same frame twice into the stitcher; `moved` hears of each frame
/// that added rows (the wheel's pacer). Returns how long it all took.
#[allow(clippy::too_many_arguments)]
fn run(
    st: &mut Stitcher,
    mut grab: impl FnMut() -> io::Result<Option<Vec<u8>>>,
    mut idle: impl FnMut() -> Duration,
    now: impl Fn() -> Instant,
    stop: &AtomicBool,
    period: impl Fn() -> Duration,
    (pw, ph): (u32, u32),
    mut moved: impl FnMut(),
    mut each: impl FnMut(Update),
) -> io::Result<Times> {
    let mut times = Times::default();
    let debug = Debug::from_env(st);
    let mut last: Option<Vec<u8>> = None;
    let mut changed = now();
    let (mut rejects, mut lost, mut full) = (0, false, false);
    // the preview: rows added since it was last made, and when that was
    let (mut stale, mut shown) = (false, None::<Instant>);
    while !stop.load(Ordering::Relaxed) {
        let start = Instant::now();
        let busy = last.is_none() || idle() < SETTLE || now() - changed < SETTLE;
        let mut news = false;
        if busy && !full {
            let frame = grab()?;
            if frame.is_some() {
                times.grabs += 1;
                times.grab += start.elapsed();
            }
            if let Some(frame) = frame.filter(|f| last.as_deref() != Some(&f[..])) {
                changed = now();
                let pushed = Instant::now();
                let push = st.push(&frame);
                times.pushes += 1;
                times.push += pushed.elapsed();
                debug.frame(times.pushes, &frame, st, push, pushed.elapsed());
                rejects = match push {
                    Push::Rejected => rejects + 1,
                    _ => 0,
                };
                let added = matches!(push, Push::Added(_));
                if added || push == Push::Moved {
                    moved();
                }
                full = st.is_full();
                news = added || lost != (rejects >= LOST_AFTER);
                lost = rejects >= LOST_AFTER;
                stale |= added;
                last = Some(frame);
            } else if rejects > 0 && !lost {
                // one jump, then the page stopped: no frame comes to count the
                // rest, and the user waits for a chip that never shows
                (lost, news) = (true, true);
            }
        }
        // (a preview a few times a second is enough to look alive)
        let due = stale && shown.is_none_or(|t| now() - t >= PREVIEW_EVERY);
        if news || due {
            let made = Instant::now();
            let preview = due.then(|| {
                let (rgba, width, height) = st.whole(pw, ph);
                Crop {
                    width,
                    height,
                    rgba,
                }
            });
            if due {
                (stale, shown) = (false, Some(now()));
                times.previews += 1;
                times.preview += made.elapsed();
            }
            each(Update {
                height: st.height(),
                full,
                lost,
                preview,
            });
        }
        std::thread::sleep(period().saturating_sub(start.elapsed()));
    }
    Ok(times)
}

/// For field reports, off unless asked for: `OPENCIRCLE_LONG_LOG` set = a
/// log line per frame stitched (what the stitcher decided and how long it
/// took); `OPENCIRCLE_LONG_DUMP=<folder>` = every such frame saved there as
/// a PNG too (a replay fixture's source, `stitch::replay`). Captures stay on
/// this PC, in the folder the user named, and nowhere else: only an absolute
/// path counts (empty or relative is ignored), and the log says it is on.
struct Debug {
    log: bool,
    dump: Option<std::path::PathBuf>,
    size: (u32, u32),
}

impl Debug {
    fn from_env(st: &Stitcher) -> Debug {
        let dump = dump_dir(std::env::var_os("OPENCIRCLE_LONG_DUMP"));
        if let Some(dir) = &dump {
            crate::log!(
                "[opencircle] long capture: saving frames to {}",
                dir.display()
            );
        }
        Debug {
            log: std::env::var_os("OPENCIRCLE_LONG_LOG").is_some(),
            dump,
            size: st.size(),
        }
    }

    fn frame(&self, i: u32, rgba: &[u8], st: &Stitcher, push: Push, took: Duration) {
        if self.log {
            let ms = took.as_secs_f64() * 1000.0;
            crate::log!(
                "[opencircle] long capture frame {i}: {push:?}, {}, {ms:.1} ms",
                st.step
            );
        }
        if let Some(dir) = &self.dump {
            let path = dir.join(format!("{i:05}.png"));
            let (w, h) = self.size;
            if let Err(e) = crate::actions::write_png(&path, w, h, rgba) {
                crate::log!("[opencircle] long capture: frame not saved ({e})");
            }
        }
    }
}

/// `OPENCIRCLE_LONG_DUMP`'s folder: an absolute path only (empty would be the
/// working folder, a relative one depends on it).
fn dump_dir(value: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    value
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The area alone, from the screen as shown (GDI; windows that keep out of
/// captures, ours among them, are not in it), RGBA.
#[cfg(windows)]
fn grab_area(region: (i32, i32, u32, u32)) -> io::Result<Vec<u8>> {
    crate::shared::platform::capture::win::grab_rect(region)
        .map_err(|_| io::Error::other("the area could not be grabbed"))
}

/// The area cut out of its monitor's capture.
#[cfg(not(windows))]
fn grab_area(region: (i32, i32, u32, u32)) -> io::Result<Vec<u8>> {
    let (x, y, w, h) = region;
    let frame =
        crate::shared::platform::capture::monitor_of(Some(region)).map_err(io::Error::other)?;
    cut(&frame.rgba, frame.width, (x - frame.x, y - frame.y), (w, h))
        .ok_or_else(|| io::Error::other("the area left the monitor"))
}

/// `size` pixels at `at` out of a `width`-wide RGBA frame; `None` if not inside it.
#[cfg(any(not(windows), test))]
fn cut(rgba: &[u8], width: u32, at: (i32, i32), size: (u32, u32)) -> Option<Vec<u8>> {
    let (x, y) = (u32::try_from(at.0).ok()?, u32::try_from(at.1).ok()?);
    let height = rgba.len() as u32 / 4 / width.max(1);
    if x + size.0 > width || y + size.1 > height {
        return None;
    }
    let row = |r: u32| {
        let start = (((y + r) * width + x) * 4) as usize;
        &rgba[start..start + size.0 as usize * 4]
    };
    Some((0..size.1).flat_map(row).copied().collect())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::super::stitch::MAX_HEIGHT;
    use super::super::stitch::synth::Rig;
    use super::*;

    /// #151: on Wayland long capture is offered where the screen can be
    /// streamed, hidden where not; elsewhere always (nothing asked).
    #[test]
    fn long_capture_is_offered_on_wayland_where_the_screen_can_be_streamed() {
        assert!(offered(false, || panic!("not asked off Wayland")));
        assert!(
            offered(true, || true),
            "ScreenCast portal + PipeWire source"
        );
        assert!(!offered(true, || false), "no portal or no PipeWire source");
    }

    /// #151: the screen cast's frames (whole BGRx screens, rows padded) cut
    /// to the area feed the same stitcher as every other source.
    #[cfg(target_os = "linux")]
    #[test]
    fn screen_cast_frames_cut_to_the_area_are_stitched() {
        let rig = Rig::new(400, 4000, 500, 7, 40);
        // the area at (30, 50) on a 480 x 600 screen, 16 bytes of padding a row
        let (sw, sh, at) = (480usize, 600usize, (30usize, 50usize));
        let stride = sw * 4 + 16;
        let screen = |s: usize| {
            let area = rig.frame(s).px;
            let mut bgrx = vec![9u8; stride * sh];
            for (y, row) in area.as_chunks::<{ 400 * 4 }>().0.iter().enumerate() {
                for (x, p) in row.as_chunks::<4>().0.iter().enumerate() {
                    let i = (at.1 + y) * stride + (at.0 + x) * 4;
                    bgrx[i..i + 4].copy_from_slice(&[p[2], p[1], p[0], 0]);
                }
            }
            bgrx
        };
        let at_ = [0, 0, 120, 240, 240, 360, 480, 600];
        let n = std::cell::Cell::new(0);
        let stop = AtomicBool::new(false);
        let grab = || {
            let i = n.get();
            n.set(i + 1);
            if i + 1 >= at_.len() {
                stop.store(true, Ordering::Relaxed);
            }
            let area = (at.0 as i32, at.1 as i32, 400, 500);
            let cut = crate::shared::platform::capture::screencast::cut_bgrx(
                &screen(at_[i]),
                sw as u32,
                sh as u32,
                area,
            );
            Ok(Some(cut.map_err(io::Error::other)?))
        };
        let mut st = Stitcher::new(400, 500);
        run(
            &mut st,
            grab,
            || Duration::ZERO,
            Instant::now,
            &stop,
            || Duration::ZERO,
            (0, 0),
            || {},
            |_| {},
        )
        .unwrap();
        let truth = rig.truth(600);
        let (rgba, _, height) = st.finish();
        assert_eq!(height as usize, truth.h);
        assert!(rgba == truth.px, "no gap, nothing twice");
    }

    #[test]
    fn frames_are_dumped_only_to_an_absolute_folder() {
        assert_eq!(dump_dir(None), None);
        assert_eq!(dump_dir(Some("".into())), None, "not the working folder");
        assert_eq!(dump_dir(Some("frames".into())), None, "relative");
        let abs = std::env::temp_dir().join("frames");
        assert_eq!(dump_dir(Some(abs.clone().into())), Some(abs));
    }

    /// Runs the loop over the frames at scroll positions `at` (one per grab;
    /// the run stops after the last, or after twice as many rounds), the
    /// user busy and the clock still (so only the first preview is due).
    /// Returns the updates, the grabs made and the image.
    fn drive(rig: &Rig, at: &[usize]) -> (Vec<Update>, usize, Crop) {
        let stop = AtomicBool::new(false);
        let grabs = Cell::new(0);
        let updates = RefCell::new(Vec::new());
        let mut st = Stitcher::new(rig.page.w as u32, rig.vh as u32);
        let grab = || {
            let i = grabs.get();
            grabs.set(i + 1);
            if i + 1 >= at.len() {
                stop.store(true, Ordering::Relaxed);
            }
            Ok(Some(rig.frame(at[i.min(at.len() - 1)]).px))
        };
        let each = |u| updates.borrow_mut().push(u);
        let rounds = Cell::new(0);
        let busy = || {
            rounds.set(rounds.get() + 1);
            if rounds.get() > 2 * at.len() {
                stop.store(true, Ordering::Relaxed); // (nothing grabbed once full)
            }
            Duration::ZERO
        };
        let t0 = Instant::now();
        let still = || t0;
        run(
            &mut st,
            grab,
            busy,
            still,
            &stop,
            || Duration::ZERO,
            (200, 300),
            || {},
            each,
        )
        .unwrap();
        let (rgba, width, height) = st.finish();
        let image = Crop {
            width,
            height,
            rgba,
        };
        (updates.into_inner(), grabs.get(), image)
    }

    #[test]
    fn what_the_user_scrolls_is_stitched_with_a_preview_after_each_step() {
        let rig = Rig::new(400, 4000, 500, 1, 40);
        // still, down, still, back up, down again: no repeats
        let at = [0, 0, 80, 160, 160, 240, 120, 40, 240, 400, 600, 800];
        let (updates, grabs, image) = drive(&rig, &at);
        assert_eq!(grabs, at.len());
        let truth = rig.truth(800);
        assert_eq!(
            (image.width as usize, image.height as usize),
            (400, truth.h)
        );
        assert!(image.rgba == truth.px);
        // one update per step that added rows; the newest rows with the first
        // (the clock stands still: no later preview is due)
        let heights: Vec<u32> = updates.iter().map(|u| u.height).collect();
        assert_eq!(heights.len(), 7, "{heights:?}");
        assert!(heights.windows(2).all(|h| h[0] < h[1]), "{heights:?}");
        let first = updates[0].preview.as_ref().unwrap();
        assert_eq!((first.width, first.height), (200, 250), "500 rows at half");
        assert!(updates.iter().all(|u| !u.full && !u.lost));
    }

    #[test]
    fn the_preview_catches_up_once_the_user_pauses() {
        // rows come in the first three grabs, 10 ms apart on the test's clock:
        // the first preview at once, the catch-up one PREVIEW_EVERY later
        let rig = Rig::new(400, 4000, 500, 6, 40);
        let stop = AtomicBool::new(false);
        let t0 = Instant::now();
        let clock = Cell::new(t0);
        let n = Cell::new(0);
        let mut st = Stitcher::new(400, 500);
        let grab = || {
            n.set(n.get() + 1);
            Ok(Some(rig.frame([0, 100, 200][(n.get() - 1).min(2)]).px))
        };
        let idle = || {
            clock.set(clock.get() + Duration::from_millis(10)); // one round
            if clock.get() - t0 > PREVIEW_EVERY * 2 {
                stop.store(true, Ordering::Relaxed);
            }
            Duration::ZERO
        };
        let updates = RefCell::new(Vec::new());
        let each = |u| updates.borrow_mut().push(u);
        let now = || clock.get();
        run(
            &mut st,
            grab,
            idle,
            now,
            &stop,
            || Duration::ZERO,
            (200, 300),
            || {},
            each,
        )
        .unwrap();
        let updates = updates.into_inner();
        let last = updates.last().unwrap();
        assert_eq!(last.height as usize, rig.truth(200).h);
        assert!(
            last.preview.is_some(),
            "the rows added after the first preview"
        );
        assert_eq!(updates.iter().filter(|u| u.preview.is_some()).count(), 2);
    }

    #[test]
    fn nothing_is_grabbed_while_the_user_does_nothing() {
        // No input, and the area unchanged since the first grab: grabs stop
        // `SETTLE` after it (a still screen costs nothing). 20 ms a round on
        // the test's clock.
        let rig = Rig::new(400, 4000, 500, 2, 40);
        let stop = AtomicBool::new(false);
        let t0 = Instant::now();
        let clock = Cell::new(t0);
        let last_grab = Cell::new(Duration::ZERO);
        let mut st = Stitcher::new(400, 500);
        let grab = || {
            last_grab.set(clock.get() - t0);
            Ok(Some(rig.frame(0).px))
        };
        let idle = || {
            clock.set(clock.get() + Duration::from_millis(20));
            if clock.get() - t0 > SETTLE + Duration::from_millis(500) {
                stop.store(true, Ordering::Relaxed);
            }
            Duration::from_secs(60)
        };
        let now = || clock.get();
        run(
            &mut st,
            grab,
            idle,
            now,
            &stop,
            || Duration::ZERO,
            (10, 10),
            || {},
            |_| {},
        )
        .unwrap();
        let last = last_grab.get();
        assert!(
            last + Duration::from_millis(20) >= SETTLE && last <= SETTLE,
            "{last:?}"
        );
        assert_eq!(st.height(), 500);
    }

    #[test]
    fn the_height_limit_stops_adding_and_says_so() {
        let rig = Rig::new(100, 26_000, 700, 3, 40);
        let at: Vec<usize> = (0..100).map(|i| i * 300).collect();
        let (updates, grabs, image) = drive(&rig, &at);
        assert_eq!(image.height, MAX_HEIGHT);
        let first_full = updates.iter().position(|u| u.full).unwrap();
        assert_eq!(first_full, updates.len() - 1, "full is the last news");
        assert!(grabs < at.len(), "no grabs once full");
    }

    #[test]
    fn a_jump_too_far_asks_to_scroll_back_then_goes_on() {
        let rig = Rig::new(400, 4000, 500, 4, 40);
        // 0 -> 1500 is far past the 400 px body: lost until back at 300
        let at = [0, 1500, 1600, 1700, 1800, 300, 500];
        let (updates, _, image) = drive(&rig, &at);
        let lost: Vec<bool> = updates.iter().map(|u| u.lost).collect();
        // the first frame, the jump (lost), back in step, on
        assert_eq!(lost, [false, true, false, false], "{updates:?}");
        let truth = rig.truth(500);
        assert!(image.height as usize == truth.h && image.rgba == truth.px);
    }

    #[test]
    fn one_jump_too_far_then_still_asks_to_scroll_back() {
        let rig = Rig::new(400, 4000, 500, 4, 40);
        // one fast flick far past the body, then the page rests there
        let at = [0, 1500, 1500, 1500, 300, 500];
        let (updates, _, image) = drive(&rig, &at);
        let lost: Vec<bool> = updates.iter().map(|u| u.lost).collect();
        assert_eq!(lost, [false, true, false, false], "{updates:?}");
        let truth = rig.truth(500);
        assert!(image.height as usize == truth.h && image.rgba == truth.px);
    }

    /// The owner's Windows 11 PC (2026-11-03): after a fast flick the capture
    /// stayed stuck; scrolling back up past where it stopped and on down
    /// again never resumed (a step down from above that point was checked
    /// against where the page was last seen, which never moved on).
    #[test]
    fn scrolling_back_past_the_stop_point_resumes_without_a_gap_or_a_repeat() {
        let rig = Rig::new(400, 4000, 500, 4, 40);
        // stitched to 200, a flick far past the 400 px body, back up past 200
        // (to 150), then on down in steps of half an area
        let at = [0, 100, 200, 1500, 1400, 900, 150, 350, 550, 750, 950];
        let (updates, _, image) = drive(&rig, &at);
        let truth = rig.truth(950);
        assert_eq!(image.height as usize, truth.h, "{updates:?}");
        assert!(image.rgba == truth.px, "no gap, nothing twice");
        assert!(
            updates.iter().any(|u| u.lost),
            "the flick asked to scroll back"
        );
        assert!(
            !updates.last().unwrap().lost,
            "the note goes once it goes on"
        );
    }

    /// A jump back up further than the search around the position reaches
    /// (Page Up twice), then on down over what is captured and past it: found
    /// in the page at once, so no "Scroll back" note (it showed while the
    /// user scrolled over what was captured) and no gap or repeat.
    #[test]
    fn a_jump_back_up_into_the_page_is_found_at_once() {
        let rig = Rig::new(400, 4000, 500, 4, 40);
        let mut at: Vec<usize> = (0..=10).map(|i| i * 150).collect();
        at.extend((0..10).map(|i| 600 + i * 150));
        let (updates, _, image) = drive(&rig, &at);
        assert!(updates.iter().all(|u| !u.lost), "{updates:?}");
        let truth = rig.truth(1950);
        assert_eq!(image.height as usize, truth.h);
        assert!(image.rgba == truth.px, "no gap, nothing twice");
    }

    #[test]
    fn a_grab_that_fails_keeps_what_was_gathered() {
        let rig = Rig::new(400, 4000, 500, 5, 40);
        let n = Cell::new(0);
        let mut st = Stitcher::new(400, 500);
        let grab = || {
            n.set(n.get() + 1);
            match n.get() {
                1 => Ok(Some(rig.frame(0).px)),
                2 => Ok(Some(rig.frame(200).px)),
                _ => Err(io::Error::other("gone")),
            }
        };
        let stop = AtomicBool::new(false);
        let err = run(
            &mut st,
            grab,
            || Duration::ZERO,
            Instant::now,
            &stop,
            || Duration::ZERO,
            (9, 9),
            || {},
            |_| {},
        );
        assert_eq!(err.unwrap_err().to_string(), "gone");
        assert_eq!(st.height() as usize, rig.truth(200).h);
    }

    #[test]
    fn cut_takes_an_area_inside_the_frame_only() {
        // 3x2 frame, pixel (x, y) = [x, y, 0, 255].
        let frame: Vec<u8> = (0..2u8)
            .flat_map(|y| (0..3u8).flat_map(move |x| [x, y, 0, 255]))
            .collect();
        assert_eq!(
            cut(&frame, 3, (1, 1), (2, 1)),
            Some(vec![1, 1, 0, 255, 2, 1, 0, 255])
        );
        assert_eq!(cut(&frame, 3, (2, 0), (2, 1)), None);
        assert_eq!(cut(&frame, 3, (-1, 0), (1, 1)), None);
    }
}
