//! Auto-scroll for scrolling capture: one wheel injection per platform plus the
//! pure capture loop that drives it.
//!
//! Windows: `SendInput` mouse wheel (needs the target not to be elevated, UIPI
//! drops input to elevated windows silently). X11: XTest buttons 4/5. Wayland
//! forbids synthetic input: `Unsupported`, the UI falls back to manual scrolling.
#![allow(dead_code)] // wired by the snip UI later; the tests exercise the loop

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::stitch::{MAX_HEIGHT, Push, Stitcher};

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
    // One event per notch: some apps only honour a single notch per event.
    let inputs = vec![wheel; notches.unsigned_abs().min(100) as usize];
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: `inputs` is a valid slice of INPUT and the size matches.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
pub fn scroll_once(at: (i32, i32), notches: i32) -> io::Result<()> {
    use xcb::xtest::FakeInput;

    const BUTTON_PRESS: u8 = 4;
    const BUTTON_RELEASE: u8 = 5;
    const MOTION_NOTIFY: u8 = 6;

    // XTest into XWayland never reaches native Wayland windows.
    if std::env::var_os("DISPLAY").is_none() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
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
}

impl Default for Params {
    fn default() -> Self {
        Params {
            poll: Duration::from_millis(80),
            settle_tries: 25,
            max_rejects: 4,
            first_notches: 3,
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
fn settle(
    capture: &mut impl FnMut() -> io::Result<Vec<u8>>,
    stop: &AtomicBool,
    p: &Params,
) -> io::Result<Vec<u8>> {
    // The first pause lets the scroll start: capturing at once would see the old page twice.
    std::thread::sleep(p.poll);
    let mut prev = capture()?;
    for _ in 0..p.settle_tries {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(p.poll);
        let next = capture()?;
        if next == prev {
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
    let first = settle(&mut capture, stop, p)?;
    if st.push(&first) == Push::Rejected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame does not match the stitcher size",
        ));
    }
    let (mut notches, mut tuned, mut still, mut rejects) = (p.first_notches.max(1), false, 0, 0);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(Stop::Cancelled);
        }
        if st.height() >= MAX_HEIGHT {
            return Ok(Stop::Cap);
        }
        scroll(notches)?;
        let frame = settle(&mut capture, stop, p)?;
        match st.push(&frame) {
            Push::Added(px) => {
                (still, rejects) = (0, 0);
                if !tuned {
                    // The pixels per notch depend on the app and the system: measure once.
                    tuned = true;
                    let target = (st.frame_size().1 / 2) as f32;
                    let per_notch = (px as f32 / notches as f32).max(1.0);
                    notches = ((target / per_notch).round() as i32).clamp(1, 40);
                }
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
            }
        }
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
    /// the scroll calls and the stitched image.
    fn drive(
        w: usize,
        page_h: usize,
        stop: &AtomicBool,
        stop_after: Option<usize>,
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
                pos.set((pos.get() + n as usize * PX_PER_NOTCH).min(rig.max_scroll()));
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
        let (reason, calls, (px, w, h), rig) = drive(780, 6000, &stop, None);
        assert_eq!(reason, Stop::EndOfPage);
        let truth = rig.truth(rig.max_scroll());
        assert_eq!((w as usize, h as usize), (truth.w, truth.h));
        assert!(px == truth.px);
        // First scroll 3 notches (300 px), then tuned to ~half a viewport (3 notches),
        // so ~20 moves to reach the end, plus the 2 confirming scrolls.
        assert!((10..40).contains(&calls), "scroll calls: {calls}");
    }

    #[test]
    fn stops_at_the_height_cap() {
        let stop = AtomicBool::new(false);
        let (reason, _, (_, w, h), _) = drive(100, 36_000, &stop, None);
        assert_eq!(reason, Stop::Cap);
        assert_eq!((w, h), (100, MAX_HEIGHT));
    }

    #[test]
    fn stops_on_the_stop_flag_and_keeps_the_partial_result() {
        let stop = AtomicBool::new(false);
        let (reason, calls, (_, _, h), _) = drive(780, 6000, &stop, Some(4));
        assert_eq!(reason, Stop::Cancelled);
        assert_eq!(calls, 4);
        assert!(h > 700);
    }

    #[test]
    fn stop_flag_set_before_start_scrolls_nothing() {
        let stop = AtomicBool::new(true);
        let (reason, calls, (_, _, h), _) = drive(780, 6000, &stop, None);
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
        let f = settle(&mut cap, &AtomicBool::new(false), &fast()).unwrap();
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
