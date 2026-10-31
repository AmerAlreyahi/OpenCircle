//! Long capture by hand (N-73 point 5): the user scrolls the page under a
//! picked area themselves (wheel, scrollbar, keys); this grabs only that area,
//! about 15 times a second while there is input, and stitches what passes
//! through it (`stitch.rs`) until Done. Nothing is injected: no auto-scroll,
//! so elevated windows (UIPI) work too.
//!
//! Windows grabs the area alone with GDI (`BitBlt` from the screen; our own
//! windows are kept out with `hide_from_capture`), and only while the user is
//! giving input or the area still moves (`GetLastInputInfo`): a still screen
//! costs nothing. X11 grabs the monitor and cuts the area. Wayland has no
//! silent grab: not offered there (`available`).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::stitch::{Push, Stitcher};
use crate::actions::Crop;

/// Time between two grabs (~15 fps).
const PERIOD: Duration = Duration::from_millis(66);
/// Grabs go on this long after the last input or the last change in the area
/// (a smooth scroll finishing, a page loading under it).
const SETTLE: Duration = Duration::from_secs(1);
/// Unmatched frames in a row after which the user is told to scroll back:
/// the page jumped further than one area's height between two grabs.
const LOST_AFTER: u32 = 3;
/// The preview is made at most this often.
const PREVIEW_EVERY: Duration = Duration::from_millis(200);

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

/// Long capture can run: the area can be grabbed silently (not on Wayland).
pub fn can_scroll() -> bool {
    #[cfg(target_os = "linux")]
    if super::wayland_session() {
        return false;
    }
    true
}

/// Grabs `region` (physical desktop px: x, y, width, height) while the user
/// scrolls, until `stop` or `max` px tall; `each` gets every change, with a
/// `preview` of the newest rows at most that size (physical px). Blocking: run
/// it on a thread.
/// The image gathered so far is returned also when a later grab fails.
pub fn by_hand(
    region: (i32, i32, u32, u32),
    preview: (u32, u32),
    max: u32,
    stop: &AtomicBool,
    each: impl FnMut(Update),
) -> Result<Crop, String> {
    let mut st = Stitcher::new(region.2, region.3).limit(max);
    let grab = || grab_area(region);
    let started = Instant::now();
    match run(&mut st, grab, idle_for, stop, PERIOD, preview, each) {
        Ok(times) => crate::log!(
            "[opencircle] long capture: {} px in {} s; {} grabs, {:.1} ms each; {} stitched, {:.1} ms each; {} previews, {:.1} ms each",
            st.height(),
            started.elapsed().as_secs(),
            times.grabs,
            times.grab.as_secs_f64() * 1000.0 / f64::from(times.grabs.max(1)),
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
/// stitcher's size, `idle` the time since the user's last input. Grabs every
/// `period` while there was input or a change within `SETTLE`, and never the
/// same frame twice into the stitcher. Returns how long it all took.
fn run(
    st: &mut Stitcher,
    mut grab: impl FnMut() -> io::Result<Vec<u8>>,
    mut idle: impl FnMut() -> Duration,
    stop: &AtomicBool,
    period: Duration,
    (pw, ph): (u32, u32),
    mut each: impl FnMut(Update),
) -> io::Result<Times> {
    let mut times = Times::default();
    let mut last: Option<Vec<u8>> = None;
    let mut changed = Instant::now();
    let (mut rejects, mut lost, mut full) = (0, false, false);
    // the preview: rows added since it was last made, and when that was
    let (mut stale, mut shown) = (false, None::<Instant>);
    while !stop.load(Ordering::Relaxed) {
        let start = Instant::now();
        let busy = last.is_none() || idle() < SETTLE || changed.elapsed() < SETTLE;
        let mut news = false;
        if busy && !full {
            let frame = grab()?;
            times.grabs += 1;
            times.grab += start.elapsed();
            if last.as_deref() != Some(&frame[..]) {
                changed = Instant::now();
                let push = st.push(&frame);
                times.pushes += 1;
                times.push += changed.elapsed();
                rejects = match push {
                    Push::Rejected => rejects + 1,
                    _ => 0,
                };
                let added = matches!(push, Push::Added(_));
                full = st.is_full();
                news = added || lost != (rejects >= LOST_AFTER);
                lost = rejects >= LOST_AFTER;
                stale |= added;
                last = Some(frame);
            }
        }
        // (a preview a few times a second is enough to look alive)
        let due = stale && shown.is_none_or(|t| t.elapsed() >= PREVIEW_EVERY);
        if news || due {
            let made = Instant::now();
            let preview = due.then(|| {
                let (rgba, width, height) = st.tail(pw, ph);
                Crop {
                    width,
                    height,
                    rgba,
                }
            });
            if due {
                (stale, shown) = (false, Some(made));
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
        std::thread::sleep(period.saturating_sub(start.elapsed()));
    }
    Ok(times)
}

/// Time since the user's last keyboard or mouse input (any app). Elsewhere
/// than Windows: none known, always "just now".
fn idle_for() -> Duration {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is ours, its size set as the call needs.
        if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            // SAFETY: no arguments.
            let now = unsafe { GetTickCount() };
            return Duration::from_millis(now.wrapping_sub(info.dwTime).into());
        }
    }
    Duration::ZERO
}

/// The area alone, from the screen as shown (GDI; windows that keep out of
/// captures, ours among them, are not in it), RGBA.
#[cfg(windows)]
fn grab_area((x, y, w, h): (i32, i32, u32, u32)) -> io::Result<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
    };
    let (wi, hi) = (w as i32, h as i32);
    let mut px = vec![0u8; w as usize * h as usize * 4];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: wi,
            biHeight: -hi, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: plain GDI calls on handles made here and released before
    // returning; `px` holds exactly `h` top-down rows of 32-bit pixels.
    let lines = unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, wi, hi);
        let old = SelectObject(mem, bitmap.into());
        let copied = BitBlt(mem, 0, 0, wi, hi, Some(screen), x, y, SRCCOPY);
        SelectObject(mem, old);
        let lines = match copied {
            Ok(()) => GetDIBits(
                mem,
                bitmap,
                0,
                h,
                Some(px.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            ),
            Err(_) => 0,
        };
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        lines
    };
    if lines != hi {
        return Err(io::Error::other("the area could not be grabbed"));
    }
    for p in px.as_chunks_mut::<4>().0 {
        *p = [p[2], p[1], p[0], 255]; // BGRx -> RGBA
    }
    Ok(px)
}

/// The area cut out of its monitor's capture.
#[cfg(not(windows))]
fn grab_area(region: (i32, i32, u32, u32)) -> io::Result<Vec<u8>> {
    let (x, y, w, h) = region;
    let frame = super::monitor_of(Some(region)).map_err(io::Error::other)?;
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

    /// Runs the loop over the frames at scroll positions `at` (one per grab;
    /// the run stops after the last, or after twice as many rounds), the
    /// user busy. Returns the updates, the grabs made and the image.
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
            Ok(rig.frame(at[i.min(at.len() - 1)]).px)
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
        run(&mut st, grab, busy, &stop, Duration::ZERO, (200, 300), each).unwrap();
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
        // one update per step that added rows; the newest rows with the
        // first (then at most every PREVIEW_EVERY)
        let heights: Vec<u32> = updates.iter().map(|u| u.height).collect();
        assert_eq!(heights.len(), 7, "{heights:?}");
        assert!(heights.windows(2).all(|h| h[0] < h[1]), "{heights:?}");
        let first = updates[0].preview.as_ref().unwrap();
        assert_eq!((first.width, first.height), (200, 250), "500 rows at half");
        assert!(updates.iter().all(|u| !u.full && !u.lost));
    }

    #[test]
    fn the_preview_catches_up_once_the_user_pauses() {
        let rig = Rig::new(400, 4000, 500, 6, 40);
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        let n = Cell::new(0);
        let mut st = Stitcher::new(400, 500);
        let grab = || {
            n.set(n.get() + 1);
            Ok(rig.frame([0, 100, 200][(n.get() - 1).min(2)]).px)
        };
        let idle = || {
            if started.elapsed() > PREVIEW_EVERY * 2 {
                stop.store(true, Ordering::Relaxed);
            }
            Duration::ZERO
        };
        let updates = RefCell::new(Vec::new());
        let each = |u| updates.borrow_mut().push(u);
        let period = Duration::from_millis(10);
        run(&mut st, grab, idle, &stop, period, (200, 300), each).unwrap();
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
        // `SETTLE` after it (a still screen costs nothing).
        let rig = Rig::new(400, 4000, 500, 2, 40);
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        let last_grab = Cell::new(Duration::ZERO);
        let mut st = Stitcher::new(400, 500);
        let grab = || {
            last_grab.set(started.elapsed());
            Ok(rig.frame(0).px)
        };
        let idle = || {
            if started.elapsed() > SETTLE + Duration::from_millis(500) {
                stop.store(true, Ordering::Relaxed);
            }
            Duration::from_secs(60)
        };
        let period = Duration::from_millis(20);
        run(&mut st, grab, idle, &stop, period, (10, 10), |_| {}).unwrap();
        assert!(last_grab.get() < SETTLE + Duration::from_millis(100));
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
    fn a_grab_that_fails_keeps_what_was_gathered() {
        let rig = Rig::new(400, 4000, 500, 5, 40);
        let n = Cell::new(0);
        let mut st = Stitcher::new(400, 500);
        let grab = || {
            n.set(n.get() + 1);
            match n.get() {
                1 => Ok(rig.frame(0).px),
                2 => Ok(rig.frame(200).px),
                _ => Err(io::Error::other("gone")),
            }
        };
        let stop = AtomicBool::new(false);
        let err = run(
            &mut st,
            grab,
            || Duration::ZERO,
            &stop,
            Duration::ZERO,
            (9, 9),
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
