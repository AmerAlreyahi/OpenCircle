//! Wheel pacing for long capture (owner, 2026-11-03): while it runs, a wheel
//! (or touchpad) scroll DOWN over the area reaches the page no faster than the
//! stitcher sees it move, so a fast spin can't skip content. The first step
//! goes through untouched (normal scrolling never waits); the next ones are
//! held and replayed one notch at a time, each after the grab loop saw the
//! page move (`moved`) or `WAIT` passed (the page's end: nothing moves).
//! Scrolling up is never held (what it shows is stitched already). Held notches
//! are replayed only while the pointer is still over the area: what is held when
//! it leaves, or when the capture ends, is dropped (replayed at the pointer, it
//! would scroll whatever window is under it now).
//! Windows only (a low-level mouse hook, `win`); elsewhere nothing is hooked.

// (the pacer itself is Windows-only outside its tests)
#![cfg_attr(not(windows), allow(dead_code))]

use std::time::{Duration, Instant};

/// Most wheel units replayed in one step: one notch.
const STEP: i32 = 120;
/// A held step goes anyway this long after the last one (the page didn't move:
/// its end, or a page that doesn't scroll).
const WAIT: Duration = Duration::from_millis(150);

/// The held wheel: `pass` decides on each event, `next` gives what to replay.
/// Wheel units as Windows counts them: + up, - down, 120 a notch (smooth /
/// high-resolution wheels and touchpads send less).
#[derive(Debug, Default)]
pub struct Pacer {
    /// Held, not yet replayed (summed: nothing is lost while over the area).
    held: i32,
    /// When the last step down reached the page, until the page is seen moving.
    sent: Option<Instant>,
}

impl Pacer {
    /// An event of `delta`: true = let it through now, false = held.
    pub fn pass(&mut self, delta: i32, now: Instant) -> bool {
        if self.held == 0 && (delta > 0 || self.ready(now)) {
            if delta < 0 {
                self.sent = Some(now);
            }
            return true;
        }
        self.held = self.held.saturating_add(delta);
        false
    }

    /// The page was seen moving: the next step may go.
    pub fn moved(&mut self) {
        self.sent = None;
    }

    /// What to replay now, if anything: all of a net scroll up at once, else
    /// one step down once the last one was seen (or `WAIT` passed). `over`: the
    /// pointer is still over the area; off it, everything held is dropped.
    pub fn next(&mut self, now: Instant, over: bool) -> Option<i32> {
        if !over {
            self.held = 0;
        }
        let step = match self.held {
            0 => return None,
            up if up > 0 => up,
            _ if !self.ready(now) => return None,
            down => {
                self.sent = Some(now);
                down.max(-STEP)
            }
        };
        self.held -= step;
        Some(step)
    }

    /// When `next` may give something (none: nothing held, or ready now).
    pub fn due(&self) -> Option<Instant> {
        (self.held < 0).then_some(self.sent?.checked_add(WAIT)?)
    }

    fn ready(&self, now: Instant) -> bool {
        self.sent.is_none_or(|t| now >= t + WAIT)
    }
}

/// Paces the wheel over `region` (physical desktop px) until dropped; `None`
/// where it can't (not Windows, a window under the area we may not send input
/// to, the hook refused).
#[cfg(windows)]
pub fn over(region: (i32, i32, u32, u32)) -> Option<win::Paced> {
    win::Paced::start(region)
}

#[cfg(not(windows))]
pub fn over(_region: (i32, i32, u32, u32)) -> Option<Paced> {
    None
}

/// Elsewhere than Windows: never made (`over` gives none).
#[cfg(not(windows))]
pub struct Paced;

#[cfg(not(windows))]
impl Paced {
    pub fn moved(&self) {}
}

#[cfg(windows)]
pub mod win {
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread::JoinHandle;
    use std::time::Instant;

    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GA_ROOT, GetAncestor, GetCursorPos, GetMessageW, GetWindowThreadProcessId,
        HHOOK, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        WH_MOUSE_LL, WM_MOUSEWHEEL, WM_QUIT, WindowFromPoint,
    };

    use super::Pacer;

    /// Our replays carry this, so the hook lets them through.
    const OURS: usize = 0x4F43_5743; // "OCWC"

    struct Shared {
        pacer: Mutex<(Pacer, bool)>, // (the pacer, stopping)
        wake: Condvar,
        region: (i32, i32, u32, u32),
    }

    /// The one paced capture the hook serves.
    static ACTIVE: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

    /// The hook and its replayer, until dropped (also on a panic's unwind;
    /// a crash unhooks with the process).
    pub struct Paced {
        shared: Arc<Shared>,
        hook_thread: u32,
        threads: Vec<JoinHandle<()>>,
    }

    impl Paced {
        pub fn start(region: (i32, i32, u32, u32)) -> Option<Paced> {
            let (x, y, w, h) = region;
            let centre = POINT {
                x: x + w as i32 / 2,
                y: y + h as i32 / 2,
            };
            if !may_send_to(centre) {
                crate::log!(
                    "[opencircle] long capture: the wheel is not paced (an elevated window)"
                );
                return None;
            }
            let shared = Arc::new(Shared {
                pacer: Mutex::new((Pacer::default(), false)),
                wake: Condvar::new(),
                region,
            });
            *ACTIVE.lock().ok()? = Some(shared.clone());
            let (tx, rx) = std::sync::mpsc::channel();
            let hook = std::thread::spawn(move || {
                // SAFETY: a hook on this thread, served by its message loop
                // below and removed before the thread ends.
                unsafe {
                    let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook), None, 0);
                    let _ = tx.send(hook.is_ok().then(|| GetCurrentThreadId()));
                    let Ok(hook) = hook else { return };
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}
                    let _ = UnhookWindowsHookEx(hook);
                }
            });
            let Ok(Some(hook_thread)) = rx.recv() else {
                *ACTIVE.lock().ok()? = None;
                let _ = hook.join();
                crate::log!("[opencircle] long capture: the wheel hook was refused");
                return None;
            };
            let replay = {
                let shared = shared.clone();
                std::thread::spawn(move || replayer(&shared))
            };
            Some(Paced {
                shared,
                hook_thread,
                threads: vec![hook, replay],
            })
        }

        /// The page was seen moving (the stitcher added rows).
        pub fn moved(&self) {
            if let Ok(mut p) = self.shared.pacer.lock() {
                p.0.moved();
                self.shared.wake.notify_one();
            }
        }
    }

    impl Drop for Paced {
        fn drop(&mut self) {
            if let Ok(mut a) = ACTIVE.lock() {
                *a = None;
            }
            // SAFETY: a plain message to the hook's thread, ending its loop.
            let _ = unsafe { PostThreadMessageW(self.hook_thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
            if let Ok(mut p) = self.shared.pacer.lock() {
                p.1 = true;
                self.shared.wake.notify_one();
            }
            for t in self.threads.drain(..) {
                let _ = t.join();
            }
        }
    }

    /// Replays what the pacer lets go while the pointer is over the area; what
    /// is held once stopping is dropped.
    fn replayer(shared: &Shared) {
        let Ok(mut guard) = shared.pacer.lock() else {
            return;
        };
        loop {
            let (pacer, stopping) = &mut *guard;
            if *stopping {
                return;
            }
            let now = Instant::now();
            let mut at = POINT::default();
            // SAFETY: GetCursorPos only writes into the POINT we own.
            let over = unsafe { GetCursorPos(&mut at) }.is_ok() && inside(at, shared.region);
            if let Some(step) = pacer.next(now, over) {
                drop(guard);
                wheel(step);
                guard = match shared.pacer.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                continue;
            }
            let wait = pacer.due().map(|t| t.saturating_duration_since(now));
            guard = match wait {
                Some(d) => match shared.wake.wait_timeout(guard, d) {
                    Ok((g, _)) => g,
                    Err(_) => return,
                },
                None => match shared.wake.wait(guard) {
                    Ok(g) => g,
                    Err(_) => return,
                },
            };
        }
    }

    /// One wheel event of `delta` at the pointer, marked ours.
    fn wheel(delta: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    mouseData: delta as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    dwExtraInfo: OURS,
                    ..Default::default()
                },
            },
        };
        // SAFETY: one INPUT of ours, its size given.
        unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
    }

    /// Low-level mouse hook: a wheel over the area goes through the pacer.
    unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && wparam.0 as u32 == WM_MOUSEWHEEL {
            // SAFETY: for WH_MOUSE_LL, lparam points at an MSLLHOOKSTRUCT.
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let delta = (info.mouseData >> 16) as u16 as i16 as i32;
            if info.dwExtraInfo != OURS
                && delta != 0
                && let Ok(active) = ACTIVE.lock()
                && let Some(shared) = active.as_ref()
                && inside(info.pt, shared.region)
                && let Ok(mut p) = shared.pacer.lock()
                && !p.0.pass(delta, Instant::now())
            {
                shared.wake.notify_one();
                return LRESULT(1); // held: replayed by `replayer`
            }
        }
        // SAFETY: passing the event on, as every hook must.
        unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) }
    }

    fn inside(p: POINT, (x, y, w, h): (i32, i32, u32, u32)) -> bool {
        p.x >= x && p.y >= y && p.x < x + w as i32 && p.y < y + h as i32
    }

    /// Our replays reach the window under `at`: not an elevated one while we
    /// aren't (Windows drops input to it silently; it is then not held).
    fn may_send_to(at: POINT) -> bool {
        // SAFETY: plain queries on handles opened and closed here.
        unsafe {
            let window = GetAncestor(WindowFromPoint(at), GA_ROOT);
            if window == HWND::default() {
                return false;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(window, Some(&mut pid));
            let ours = elevated(GetCurrentProcess());
            if ours == Some(true) {
                return true;
            }
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let theirs = elevated(process);
            let _ = windows::Win32::Foundation::CloseHandle(process);
            theirs == Some(false)
        }
    }

    /// Whether `process` runs elevated; none when its token can't be read.
    unsafe fn elevated(process: HANDLE) -> Option<bool> {
        let mut token = HANDLE::default();
        // SAFETY: the token is ours to close; TOKEN_ELEVATION is what
        // TokenElevation writes, its size given.
        unsafe {
            OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
            let mut e = TOKEN_ELEVATION::default();
            let mut len = 0;
            let read = GetTokenInformation(
                token,
                TokenElevation,
                Some((&raw mut e).cast()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
            let _ = windows::Win32::Foundation::CloseHandle(token);
            read.ok()?;
            Some(e.TokenIsElevated != 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fast_spin_is_replayed_one_notch_per_move_and_nothing_is_lost() {
        let t = Instant::now();
        let mut p = Pacer::default();
        // the first notch goes through at once (normal scrolling never waits)
        assert!(p.pass(-120, t));
        // a fast spin: nine more notches and a smooth wheel's small steps
        let spin = [-120; 9].into_iter().chain([-30, -30, -40]);
        let total: i32 = spin.clone().sum();
        for d in spin {
            assert!(!p.pass(d, t), "held while the first isn't seen");
        }
        assert_eq!(
            p.next(t, true),
            None,
            "nothing until the page is seen moving"
        );
        let mut out = Vec::new();
        for _ in 0..20 {
            p.moved();
            if let Some(d) = p.next(t, true) {
                assert!((-STEP..0).contains(&d), "{d}");
                out.push(d);
                assert_eq!(p.next(t, true), None, "one step per move");
            }
        }
        assert_eq!(out.iter().sum::<i32>(), total, "{out:?}");
        assert_eq!(p.held, 0);
    }

    #[test]
    fn a_page_that_stops_moving_still_gets_its_wheel_after_a_wait() {
        let t = Instant::now();
        let mut p = Pacer::default();
        assert!(p.pass(-120, t));
        assert!(!p.pass(-120, t));
        assert_eq!(p.due(), Some(t + WAIT));
        assert_eq!(p.next(t + WAIT / 2, true), None);
        assert_eq!(p.next(t + WAIT, true), Some(-120));
        // ready again later: a lone notch passes untouched
        assert!(p.pass(-120, t + WAIT * 3));
    }

    #[test]
    fn up_is_never_held_and_cancels_what_is_held_without_loss() {
        let t = Instant::now();
        let mut p = Pacer::default();
        assert!(p.pass(120, t), "up, nothing held: through");
        assert!(p.pass(-120, t));
        assert!(!p.pass(-240, t));
        // the user turns back up: summed with what is held
        assert!(!p.pass(360, t));
        assert_eq!(p.next(t, true), Some(120), "the net scroll up at once");
        assert_eq!(p.next(t, true), None);
    }

    #[test]
    fn what_is_held_when_the_pointer_leaves_is_dropped() {
        let t = Instant::now();
        let mut p = Pacer::default();
        assert!(p.pass(-120, t));
        assert!(!p.pass(-600, t));
        p.moved();
        // the pointer went to another window (or the toolbar): not replayed there
        assert_eq!(p.next(t, false), None);
        assert_eq!(p.held, 0, "dropped, not kept for later");
        p.moved();
        assert_eq!(p.next(t, true), None, "nothing comes back over the area");
    }
}
