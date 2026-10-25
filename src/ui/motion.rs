//! Widget motion, one place for every animation (the overlay's opening, the
//! bar's glide and its turn into the editing toolbar, a snapped selection's
//! morph, the results card's slide and dock): durations on one scale, enter
//! eased out and exit eased in, always by the time since the motion began
//! (never a step per frame, so a slow frame shows the motion further on, it
//! never jumps nor slows it down). With the OS's animations off (Windows
//! "Show animations"), every motion is instant.
//!
//! Std only: the results card's helper (`src/sheet/helper`) includes this
//! file too.
#![allow(dead_code)] // each includer uses its part

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Something appearing (the opening, a slide-in, the bar turning into the toolbar).
pub const ENTER: Duration = Duration::from_millis(200);
/// Something leaving or turning back (exit is a little faster than enter).
pub const EXIT: Duration = Duration::from_millis(150);
/// Something moving to a new place (the bar docking, a card docking, a morph).
pub const MOVE: Duration = Duration::from_millis(200);

/// Enter / move: fast start, soft landing. Exit: soft start, quick leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Out,
    In,
}

impl Curve {
    /// The control points of its cubic Bézier (x1, y1, x2, y2).
    fn points(self) -> [f32; 4] {
        match self {
            Curve::Out => [0.16, 1.0, 0.3, 1.0],
            Curve::In => [0.7, 0.0, 0.84, 0.0],
        }
    }

    /// `t` (0..1 of the time) eased: 0..1 of the way, never past it.
    pub fn ease(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 || t == 1.0 {
            return t;
        }
        let [x1, y1, x2, y2] = self.points();
        let at = |a: f32, b: f32, s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
        };
        // the curve's parameter where its x is t (x rises with s: bisect)
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            match at(x1, x2, mid) < t {
                true => lo = mid,
                false => hi = mid,
            }
        }
        at(y1, y2, (lo + hi) / 2.0).clamp(0.0, 1.0)
    }
}

/// The OS asks for no animations (read by `refresh`).
static REDUCED: AtomicBool = AtomicBool::new(false);

/// Reads the OS's animation setting again (when the overlay or a card opens).
pub fn refresh() {
    REDUCED.store(!os_animates(), Ordering::Relaxed);
}

/// The OS asks for no animations: motions are instant.
pub fn reduced() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

/// How much of a motion `length` long begun at `start` is done at `now`
/// (0..1 of the time, linear; 1 at once when motion is reduced).
pub fn done(start: Instant, now: Instant, length: Duration) -> f32 {
    if reduced() || length.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(start).as_secs_f32() / length.as_secs_f32()).min(1.0)
}

/// How far a motion `length` long begun at `start` is at `now` (0..1, `curve`).
pub fn progress(start: Instant, now: Instant, length: Duration, curve: Curve) -> f32 {
    curve.ease(done(start, now, length))
}

/// The OS's "show animations" setting (Windows); elsewhere motion is on.
#[cfg(windows)]
fn os_animates() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut on = windows::core::BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL into `on`.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut windows::core::BOOL as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || on.as_bool()
}

#[cfg(not(windows))]
fn os_animates() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both curves start at 0, end at 1 and only ever go forward; enter is
    /// ahead of the clock (fast start), exit behind it (soft start).
    #[test]
    fn curves_are_monotonic_and_bounded() {
        for curve in [Curve::Out, Curve::In] {
            let mut last = 0.0;
            for i in 0..=100 {
                let v = curve.ease(i as f32 / 100.0);
                assert!((0.0..=1.0).contains(&v) && v >= last, "{curve:?} at {i}");
                last = v;
            }
            assert_eq!((curve.ease(0.0), curve.ease(1.0)), (0.0, 1.0));
        }
        assert!(Curve::Out.ease(0.3) > 0.6);
        assert!(Curve::In.ease(0.3) < 0.1);
    }

    /// Progress is the time since the start, whatever the frames: one slow
    /// frame lands where the clock is (no jump past it, no lag behind), and
    /// it is done exactly at its length.
    #[test]
    fn progress_follows_the_clock_not_the_frames() {
        let t0 = Instant::now();
        let at = |ms| progress(t0, t0 + Duration::from_millis(ms), MOVE, Curve::Out);
        // 60 fps or one 100 ms frame: the same place at 100 ms
        let smooth = (0..=6).map(|i| at(i * 16)).next_back().unwrap();
        assert!((smooth - at(96)).abs() < 1e-6);
        assert!(at(100) > at(96) && at(100) < 1.0);
        assert_eq!(at(200), 1.0);
        assert_eq!(at(5000), 1.0);
        assert_eq!(done(t0, t0 - Duration::from_millis(5), MOVE), 0.0);
        // every token on the scale
        for d in [ENTER, EXIT, MOVE] {
            assert!([100, 150, 200, 300].contains(&d.as_millis()));
        }
        assert!(EXIT < ENTER);
    }
}
