//! Auto-scroll for scrolling capture: one wheel injection per platform plus the
//! pure capture loop that drives it.
//!
//! Windows: `SendInput` mouse wheel (needs the target not to be elevated, UIPI
//! drops input to elevated windows silently). X11: XTest buttons 4/5. Wayland
//! forbids synthetic input: `Unsupported`, the UI falls back to manual scrolling.

use std::cell::Cell;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::stitch::{MAX_HEIGHT, Push, Stitcher};
use crate::actions::Crop;

/// Scrolls the content under the pointer at `at` (physical desktop pixels) by
/// `notches` wheel notches: positive scrolls down (towards the end of the page),
/// negative up. Moves the pointer to `at` first.
#[cfg(windows)]
pub fn scroll_once(at: (i32, i32), notches: i32) -> io::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    };
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

    // SAFETY: plain Win32 calls with valid, fully initialised arguments.
    unsafe { SetCursorPos(at.0, at.1) }.map_err(io::Error::other)?;
    // Wheel: positive delta is away from the user (up), so down is negative.
    let delta = (if notches > 0 { -120i32 } else { 120 }) as u32;
    let wheel = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: delta,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // One SendInput per notch, a beat apart: in one batch, Notepad applied only
    // some of the notches (2 of 7) while our window had the focus.
    for i in 0..notches.unsigned_abs().min(100) {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(15));
        }
        // SAFETY: a valid one-element slice of INPUT and its matching size.
        if unsafe { SendInput(&[wheel], size_of::<INPUT>() as i32) } != 1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn scroll_once(at: (i32, i32), notches: i32) -> io::Result<()> {
    use xcb::xtest::FakeInput;

    const BUTTON_PRESS: u8 = 4;
    const BUTTON_RELEASE: u8 = 5;
    const MOTION_NOTIFY: u8 = 6;

    // XTest into XWayland never reaches native Wayland windows.
    if !crate::sys::session::x11_only() {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    let (conn, screen) =
        xcb::Connection::connect_with_extensions(None, &[xcb::Extension::Test], &[])
            .map_err(io::Error::other)?;
    let root = conn
        .get_setup()
        .roots()
        .nth(screen as usize)
        .ok_or_else(|| io::Error::other("no X11 screen"))?
        .root();
    let fake = |ty: u8, detail: u8, x: i16, y: i16| FakeInput {
        r#type: ty,
        detail,
        time: 0,
        root,
        root_x: x,
        root_y: y,
        deviceid: 0,
    };
    let clamp = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    conn.send_and_check_request(&fake(MOTION_NOTIFY, 0, clamp(at.0), clamp(at.1)))
        .map_err(io::Error::other)?;
    let button = if notches > 0 { 5 } else { 4 }; // 4 = wheel up, 5 = wheel down
    for _ in 0..notches.unsigned_abs().min(100) {
        conn.send_request(&fake(BUTTON_PRESS, button, 0, 0));
        conn.send_request(&fake(BUTTON_RELEASE, button, 0, 0));
    }
    conn.flush().map_err(io::Error::other)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn scroll_once(_at: (i32, i32), _notches: i32) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Tuning of [`run`].
pub struct Params {
    /// Pause before each settle check and between two of them.
    pub poll: Duration,
    /// Settle checks per scroll before the latest frame is taken as it is
    /// (animated pages never hold still). `poll * settle_tries` is the timeout.
    pub settle_tries: u32,
    /// Consecutive rejected frames after which the run gives up.
    pub max_rejects: u32,
    /// Notches for the first scroll; later ones are tuned to half a viewport.
    pub first_notches: i32,
    /// Settle checks that must pass with the frame still as before the scroll
    /// before that counts as "did not move": the target may repaint late.
    pub quiet_tries: u32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            poll: Duration::from_millis(80),
            settle_tries: 25,
            max_rejects: 4,
            first_notches: 3,
            quiet_tries: 4,
        }
    }
}

/// Why [`run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Two settled frames in a row without movement: the end of the page.
    EndOfPage,
    /// The stitcher reached `MAX_HEIGHT`.
    Cap,
    /// The stop flag was set.
    Cancelled,
    /// `max_rejects` frames in a row could not be matched (video, animation, jumpy page).
    Unmatched,
}

/// Captures until two consecutive captures are identical (the page stopped
/// moving), or `settle_tries` checks passed, then returns the latest capture.
/// While the frame still equals `before` (the frame before the scroll), it
/// keeps waiting for `quiet_tries` checks: the target may not have repainted yet.
fn settle(
    capture: &mut impl FnMut() -> io::Result<Vec<u8>>,
    stop: &AtomicBool,
    p: &Params,
    before: Option<&[u8]>,
) -> io::Result<Vec<u8>> {
    // The first pause lets the scroll start: capturing at once would see the old page twice.
    std::thread::sleep(p.poll);
    let mut prev = capture()?;
    for i in 0..p.settle_tries {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(p.poll);
        let next = capture()?;
        if next == prev && (before != Some(&next[..]) || i + 1 >= p.quiet_tries) {
            return Ok(next);
        }
        prev = next;
    }
    Ok(prev)
}

/// Scrolls and captures into `st` until the end of the page, the height cap, a
/// stop request or too many unmatched frames. `capture` returns one RGBA frame
/// of the stitcher's size, `scroll(notches)` scrolls down, `progress` gets the
/// stitched height after each added frame. `st` keeps everything gathered, so a
/// cancelled run can still be finished.
pub fn run(
    st: &mut Stitcher,
    mut capture: impl FnMut() -> io::Result<Vec<u8>>,
    mut scroll: impl FnMut(i32) -> io::Result<()>,
    stop: &AtomicBool,
    p: &Params,
    mut progress: impl FnMut(u32),
) -> io::Result<Stop> {
    let mut last = settle(&mut capture, stop, p, None)?;
    if st.push(&last) == Push::Rejected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame does not match the stitcher size",
        ));
    }
    let (mut notches, mut per_notch, mut still, mut rejects) = (p.first_notches.max(1), 0f32, 0, 0);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(Stop::Cancelled);
        }
        if st.height() >= MAX_HEIGHT {
            return Ok(Stop::Cap);
        }
        scroll(notches)?;
        let frame = settle(&mut capture, stop, p, Some(&last))?;
        match st.push(&frame) {
            Push::Added(px) => {
                (still, rejects) = (0, 0);
                // Pixels per notch depend on the app and the system, and a notch
                // can get lost: trust the largest step seen (a short step is safe,
                // one past the overlap is not).
                per_notch = per_notch.max(px as f32 / notches as f32);
                let target = (st.frame_size().1 / 2) as f32;
                notches = ((target / per_notch).round() as i32).clamp(1, 40);
                progress(st.height());
            }
            Push::NoMovement => {
                (still, rejects) = (still + 1, 0);
                if still >= 2 {
                    return Ok(Stop::EndOfPage);
                }
            }
            Push::Rejected => {
                (still, rejects) = (0, rejects + 1);
                if rejects >= p.max_rejects {
                    return Ok(Stop::Unmatched);
                }
                // Most likely scrolled past the overlap: go back, take smaller steps.
                scroll(-notches)?;
                notches = (notches / 2).max(1);
                continue;
            }
        }
        last = frame;
    }
}

/// Test hook: a tall PNG stands in for the scrolled page (WSLg/CI can't scroll).
const TEST_SCROLL: &str = "OPENCIRCLE_TEST_SCROLL";

/// Auto-scroll can run: synthetic wheel input works (not on native Wayland).
pub fn can_scroll() -> bool {
    #[cfg(target_os = "linux")]
    if super::wayland_session() {
        return false;
    }
    true
}

/// Auto-scrolls `region` (physical desktop px: x, y, width, height) and stitches
/// it until the end of the page, the height cap or `stop`. Blocking: run it on a
/// thread. `progress` gets the stitched height after each new frame.
pub fn scroll_capture(
    region: (i32, i32, u32, u32),
    stop: &AtomicBool,
    progress: impl FnMut(u32),
) -> Result<Crop, String> {
    let (x, y, w, h) = region;
    if let Some(path) = std::env::var_os(TEST_SCROLL) {
        let page = super::from_png(&path)?;
        let fake = FakePage::new(page.rgba, page.width, page.height);
        let params = Params {
            poll: std::time::Duration::from_millis(30),
            ..Params::default()
        };
        return gather(
            w,
            h,
            || fake.frame(w, h),
            |n| fake.scroll(n, h),
            stop,
            &params,
            progress,
        );
    }
    let centre = (x + w as i32 / 2, y + h as i32 / 2);
    let grab = || {
        let frame = super::monitor_of(Some(region)).map_err(io::Error::other)?;
        let crop = cut(&frame.rgba, frame.width, (x - frame.x, y - frame.y), (w, h));
        crop.ok_or_else(|| io::Error::other("the area left the monitor"))
    };
    let scroll = |n| scroll_once(centre, n);
    gather(w, h, grab, scroll, stop, &Params::default(), progress)
}

/// The capture loop over any source; a failed or stopped run keeps what it gathered.
fn gather(
    w: u32,
    h: u32,
    grab: impl FnMut() -> io::Result<Vec<u8>>,
    scroll: impl FnMut(i32) -> io::Result<()>,
    stop: &AtomicBool,
    params: &Params,
    progress: impl FnMut(u32),
) -> Result<Crop, String> {
    let mut st = Stitcher::new(w, h);
    match run(&mut st, grab, scroll, stop, params, progress) {
        Ok(reason) => crate::log!("[opencircle] scrolling capture: {reason:?}"),
        Err(e) if st.height() == 0 => return Err(e.to_string()),
        Err(e) => crate::log!("[opencircle] scrolling capture stopped early: {e}"),
    }
    let (rgba, width, height) = st.finish();
    Ok(Crop {
        width,
        height,
        rgba,
    })
}

/// `size` pixels at `at` out of a `width`-wide RGBA frame; `None` if not inside it.
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

/// A tall page seen through a viewport that scrolls 100 px per notch.
struct FakePage {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    top: Cell<u32>,
}

impl FakePage {
    fn new(rgba: Vec<u8>, width: u32, height: u32) -> Self {
        FakePage {
            rgba,
            width,
            height,
            top: Cell::new(0),
        }
    }

    fn frame(&self, w: u32, h: u32) -> io::Result<Vec<u8>> {
        cut(&self.rgba, self.width, (0, self.top.get() as i32), (w, h))
            .ok_or_else(|| io::Error::other("the test page is smaller than the area"))
    }

    fn scroll(&self, notches: i32, h: u32) -> io::Result<()> {
        let to = self.top.get() as i64 + i64::from(notches) * 100;
        self.top
            .set(to.clamp(0, i64::from(self.height.saturating_sub(h))) as u32);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::super::stitch::synth::{Rig, Rng};
    use super::*;

    const PX_PER_NOTCH: usize = 100;

    fn fast() -> Params {
        Params {
            poll: Duration::ZERO,
            settle_tries: 5,
            ..Params::default()
        }
    }

    /// Runs the loop on a fake page of `page_h` rows; returns the stop reason,
    /// the scroll calls and the stitched image. `lossy`: the first two scrolls
    /// apply one notch only, as Notepad did on the Windows VM.
    fn drive(
        w: usize,
        page_h: usize,
        stop: &AtomicBool,
        stop_after: Option<usize>,
        lossy: bool,
    ) -> (Stop, usize, (Vec<u8>, u32, u32), Rig) {
        let rig = Rig::new(w, page_h, 700, 5, 40);
        let pos = Cell::new(0usize);
        let calls = Cell::new(0usize);
        let mut st = Stitcher::new(w as u32, 700);
        let reason = run(
            &mut st,
            || Ok(rig.frame(pos.get()).px),
            |n| {
                calls.set(calls.get() + 1);
                let n = if lossy && calls.get() <= 2 {
                    n.signum()
                } else {
                    n
                };
                let to = pos.get() as i64 + n as i64 * PX_PER_NOTCH as i64;
                pos.set((to.max(0) as usize).min(rig.max_scroll()));
                if Some(calls.get()) == stop_after {
                    stop.store(true, Ordering::Relaxed);
                }
                Ok(())
            },
            stop,
            &fast(),
            |_| {},
        )
        .unwrap();
        (reason, calls.get(), st.finish(), rig)
    }

    #[test]
    fn stops_at_end_of_page_with_exact_result() {
        let stop = AtomicBool::new(false);
        let (reason, calls, (px, w, h), rig) = drive(780, 6000, &stop, None, false);
        assert_eq!(reason, Stop::EndOfPage);
        let truth = rig.truth(rig.max_scroll());
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        assert!(px == truth.px);
        // First scroll 3 notches (300 px), then tuned to ~half a viewport (3 notches),
        // so ~20 moves to reach the end, plus the 2 confirming scrolls.
        assert!((10..40).contains(&calls), "scroll calls: {calls}");
    }

    #[test]
    fn lost_notches_do_not_scroll_past_the_overlap() {
        // 3 notches measured as 100 px used to tune the next scroll to 11 notches
        // (1100 px, past the 600 px body): every later frame was rejected.
        let stop = AtomicBool::new(false);
        let (reason, _, (px, w, h), rig) = drive(780, 6000, &stop, None, true);
        assert_eq!(reason, Stop::EndOfPage);
        let truth = rig.truth(rig.max_scroll());
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        assert!(px == truth.px);
    }

    #[test]
    fn settle_waits_for_a_late_repaint() {
        // The target repaints only on the 4th capture after the scroll: settling on
        // the old frame (twice the same) would read as "did not move".
        let n = Cell::new(0);
        let mut cap = || {
            n.set(n.get() + 1);
            Ok(vec![if n.get() < 4 { 0 } else { 1 }; 4])
        };
        let f = settle(&mut cap, &AtomicBool::new(false), &fast(), Some(&[0; 4])).unwrap();
        assert_eq!(f, vec![1; 4]);
        // A page that really did not move is let go after `quiet_tries` checks.
        n.set(0);
        let mut still = || {
            n.set(n.get() + 1);
            Ok(vec![0; 4])
        };
        let f = settle(&mut still, &AtomicBool::new(false), &fast(), Some(&[0; 4])).unwrap();
        assert_eq!((f, n.get()), (vec![0; 4], 1 + fast().quiet_tries));
    }

    #[test]
    fn stops_at_the_height_cap() {
        let stop = AtomicBool::new(false);
        let (reason, _, (_, w, h), _) = drive(100, 36_000, &stop, None, false);
        assert_eq!(reason, Stop::Cap);
        assert_eq!((w, h), (100, MAX_HEIGHT));
    }

    #[test]
    fn stops_on_the_stop_flag_and_keeps_the_partial_result() {
        let stop = AtomicBool::new(false);
        let (reason, calls, (_, _, h), _) = drive(780, 6000, &stop, Some(4), false);
        assert_eq!(reason, Stop::Cancelled);
        assert_eq!(calls, 4);
        assert!(h > 700);
    }

    #[test]
    fn stop_flag_set_before_start_scrolls_nothing() {
        let stop = AtomicBool::new(true);
        let (reason, calls, (_, _, h), _) = drive(780, 6000, &stop, None, false);
        assert_eq!((reason, calls, h), (Stop::Cancelled, 0, 700));
    }

    #[test]
    fn settle_gives_up_on_a_page_that_never_holds_still() {
        // Every capture differs (a video filling the region): settle must time out
        // by count, and the run must end through rejected frames, not hang.
        let stop = AtomicBool::new(false);
        let captures = Cell::new(0usize);
        let rng = RefCell::new(Rng::new(1));
        let (w, h) = (780usize, 700usize);
        let mut st = Stitcher::new(w as u32, h as u32);
        let reason = run(
            &mut st,
            || {
                captures.set(captures.get() + 1);
                let mut r = rng.borrow_mut();
                // First frame ok-ish noise too: the stitcher only needs the right size.
                Ok((0..w * h * 4).map(|_| r.range(0, 256) as u8).collect())
            },
            |_| Ok(()),
            &stop,
            &fast(),
            |_| {},
        )
        .unwrap();
        assert_eq!(reason, Stop::Unmatched);
        // 1 initial settle + max_rejects settles, each at most 1 + settle_tries captures.
        assert!(
            captures.get() <= (1 + 4) * (1 + 5),
            "captures: {}",
            captures.get()
        );
        assert!(captures.get() > 5, "settle should have retried");
    }

    #[test]
    fn settle_returns_as_soon_as_two_captures_match() {
        let n = Cell::new(0);
        let mut cap = || {
            n.set(n.get() + 1);
            // frames: 0, 1, 2, 2, ...
            Ok(vec![n.get().min(3) as u8; 4])
        };
        let f = settle(&mut cap, &AtomicBool::new(false), &fast(), None).unwrap();
        assert_eq!(f, vec![3; 4]);
        assert_eq!(n.get(), 4);
    }

    #[test]
    fn capture_errors_propagate() {
        let mut st = Stitcher::new(4, 4);
        let err = run(
            &mut st,
            || Err(io::Error::other("gone")),
            |_| Ok(()),
            &AtomicBool::new(false),
            &fast(),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "gone");
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    /// A page with a distinct block pattern on every row (the stitcher matches rows).
    fn page(w: u32, h: u32) -> Vec<u8> {
        let mut seed = 0x9e37_79b9_u32;
        let blocks: Vec<u8> = (0..(w / 8 + 1) * (h / 4 + 1))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let v = blocks[((y / 4) * (w / 8 + 1) + x / 8) as usize];
                [v, v.wrapping_mul(3), 255 - v, 255]
            })
            .collect()
    }

    fn fast_two() -> Params {
        Params {
            poll: std::time::Duration::ZERO,
            settle_tries: 2,
            ..Params::default()
        }
    }

    #[test]
    fn scrolling_runs_to_the_end_of_the_page_with_progress() {
        let (w, h, vh) = (320, 3000, 600);
        let fake = FakePage::new(page(w, h), w, h);
        let mut seen = Vec::new();
        let stop = AtomicBool::new(false);
        let out = gather(
            w,
            vh,
            || fake.frame(w, vh),
            |n| fake.scroll(n, vh),
            &stop,
            &fast_two(),
            |height| seen.push(height),
        )
        .unwrap();
        assert_eq!((out.width, out.height), (w, h));
        assert!(out.rgba == fake.rgba, "stitched page differs");
        assert!(seen.windows(2).all(|p| p[0] < p[1]), "{seen:?}");
        assert_eq!(seen.last(), Some(&h));
    }

    #[test]
    fn stop_keeps_what_was_gathered() {
        let (w, h, vh) = (320, 6000, 600);
        let fake = FakePage::new(page(w, h), w, h);
        let stop = AtomicBool::new(false);
        let mut seen = Vec::new();
        let out = gather(
            w,
            vh,
            || fake.frame(w, vh),
            |n| fake.scroll(n, vh),
            &stop,
            &fast_two(),
            |height| {
                seen.push(height);
                if seen.len() == 3 {
                    stop.store(true, Ordering::Relaxed); // the Stop button
                }
            },
        )
        .unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(out.height, seen[2]);
        assert!(vh < out.height && out.height < h);
        // The part gathered is the top of the page, exactly.
        let n = (w * out.height * 4) as usize;
        assert!(out.rgba[..] == fake.rgba[..n]);
    }

    #[test]
    fn a_source_that_fails_at_once_is_an_error() {
        let err = gather(
            4,
            4,
            || Err(io::Error::other("no screen")),
            |_| Ok(()),
            &AtomicBool::new(false),
            &fast_two(),
            |_| {},
        );
        assert_eq!(err.unwrap_err(), "no screen");
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
