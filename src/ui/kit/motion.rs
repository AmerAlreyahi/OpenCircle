//! Widget motion, one place for every animation: the overlay's opening, the
//! bar's glide and its turn into the editing toolbar, a snapped selection's
//! morph, a window's or a widget's open and close (`Shown`), the toast's fade
//! and rise. The durations and curves are the new design's
//! (`core/motion.js`, DESIGN_BEHAVIOR 1.5 and 12), always by the time since
//! the motion began (never a step per frame, so a slow frame shows the motion
//! further on, it never jumps nor slows it down). With the OS's animations
//! off (Windows "Show animations"), every motion is instant.
//!
//! Cost: the CPU renderer has no alpha for a group of widgets, and a fade of
//! anything big redraws all of it every frame. So a whole window fades by
//! its opacity (the OS composites it: `windows::opacity`), and widgets inside
//! the overlay move (rise, glide, scale) and redraw only round themselves.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A widget or window opening (`setWidgetOpen`, opacity 0 -> 1, scale .93 -> 1).
pub const OPEN: Duration = Duration::from_millis(210);
/// Closing: a little faster, eased in.
pub const CLOSE: Duration = Duration::from_millis(150);
/// The bar's body swapped in place (actions <-> editor).
pub const MORPH: Duration = Duration::from_millis(190);
/// Something gliding to a new place (a thumb, a snapped box).
pub const GLIDE: Duration = Duration::from_millis(200);
/// A dragged widget let go: it settles into its place.
pub const DOCK: Duration = Duration::from_millis(240);
/// The toast pill showing or hiding (opacity and `TOAST_RISE` px).
pub const TOAST: Duration = Duration::from_millis(200);
pub const TOAST_RISE: f32 = 8.0;
/// A widget's scale when closed (it grows to 1 as it opens).
pub const OPEN_SCALE: f32 = 0.93;

/// The design's easing curves (cubic Béziers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// Expo-out (open, page rise): a fast start, a soft landing.
    Out,
    /// Standard-out (glide, dock, drawer, card).
    Standard,
    /// Ease-in (close): a soft start, a quick leave.
    In,
    /// The plain ease (the toast's fade and rise).
    Ease,
    /// `animate-pulse`'s in-out (the setup's logo while installing).
    Pulse,
}

impl Curve {
    /// The control points of its cubic Bézier (x1, y1, x2, y2).
    fn points(self) -> [f32; 4] {
        match self {
            Curve::Out => [0.16, 1.0, 0.3, 1.0],
            Curve::Standard => [0.22, 1.0, 0.36, 1.0],
            Curve::In => [0.4, 0.0, 1.0, 1.0],
            Curve::Ease => [0.4, 0.0, 0.2, 1.0],
            Curve::Pulse => [0.4, 0.0, 0.6, 1.0],
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

/// How a thing opens and closes: each way's length and curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    pub open: (Duration, Curve),
    pub close: (Duration, Curve),
}

/// Widgets and windows (`setWidgetOpen`): 210 ms expo-out, 150 ms ease-in.
pub const WIDGET: Pace = Pace {
    open: (OPEN, Curve::Out),
    close: (CLOSE, Curve::In),
};
/// The toast pill: 200 ms ease both ways.
pub const TOAST_PACE: Pace = Pace {
    open: (TOAST, Curve::Ease),
    close: (TOAST, Curve::Ease),
};

/// A thing opening or closing by `Pace`: 0 = closed .. 1 = open (its
/// opacity), from wherever it was when it turned (a close during the opening
/// goes back from there, no jump).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pace: Pace,
    open: bool,
    from: f32,
    start: Instant,
}

impl Shown {
    /// Starting to open at `now`, from closed.
    pub fn opening(pace: Pace, now: Instant) -> Shown {
        Shown {
            pace,
            open: true,
            from: 0.0,
            start: now,
        }
    }

    /// Starting to close at `now`, from open.
    pub fn closing(pace: Pace, now: Instant) -> Shown {
        let mut shown = Shown {
            pace,
            open: true,
            from: 1.0,
            start: now,
        };
        shown.close(now);
        shown
    }

    /// Turns to close at `now` (nothing if closing already).
    pub fn close(&mut self, now: Instant) {
        if self.open {
            *self = Shown {
                open: false,
                from: self.at(now),
                start: now,
                ..*self
            };
        }
    }

    /// How open it is at `now` (0..1, eased).
    pub fn at(&self, now: Instant) -> f32 {
        let (length, curve) = match self.open {
            true => self.pace.open,
            false => self.pace.close,
        };
        let to = if self.open { 1.0 } else { 0.0 };
        self.from + (to - self.from) * progress(self.start, now, length, curve)
    }

    /// Still moving at `now` (a window's fade steps on until it isn't).
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    pub fn moving(&self, now: Instant) -> bool {
        let to = if self.open { 1.0 } else { 0.0 };
        self.at(now) != to
    }
}

/// The frame clock's beat: 16 ms (62.5 a second; the frame budget, #23).
pub const FRAME: Duration = Duration::from_millis(16);

/// One message per `FRAME` (its time) while subscribed: the clock every motion
/// on the overlay steps by. A thread ticks on deadlines; a tick the app is
/// still busy with is dropped (no backlog). Not iced's `window::frames`:
/// with no vsync on the CPU renderer every message redraws at once, so those
/// came as fast as a frame could be drawn (500 a second for a small motion,
/// a core busy for nothing).
pub fn frames() -> iced::Subscription<Instant> {
    iced::Subscription::run(ticks)
}

fn ticks() -> impl iced::futures::Stream<Item = Instant> {
    iced::stream::channel(1, async |mut out| {
        std::thread::spawn(move || {
            let mut next = Instant::now();
            loop {
                next += FRAME;
                let now = Instant::now();
                match next.checked_duration_since(now) {
                    Some(wait) => std::thread::sleep(wait),
                    None => next = now, // behind: start again from now
                }
                // the subscription ended (nothing moves): the clock stops
                if out
                    .try_send(Instant::now())
                    .is_err_and(|e| e.is_disconnected())
                {
                    return;
                }
            }
        });
        std::future::pending::<()>().await;
    })
}

/// The OS asks for no animations (read by `refresh`).
static REDUCED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// Tests: this thread's "Show animations" (CI runners turn it off).
    static TEST_REDUCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tests: the OS setting as `reduced` for this thread.
#[cfg(test)]
pub fn set_reduced(reduced: bool) {
    TEST_REDUCED.set(reduced);
}

/// Reads the OS's animation setting again (when the overlay or a window opens).
pub fn refresh() {
    REDUCED.store(!os_animates(), Ordering::Relaxed);
}

/// The OS asks for no animations: motions are instant.
pub fn reduced() -> bool {
    #[cfg(test)]
    return TEST_REDUCED.get();
    #[cfg(not(test))]
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

    /// Every curve starts at 0, ends at 1 and only ever goes forward; the
    /// outs are ahead of the clock (fast start), ease-in behind it.
    #[test]
    fn curves_are_monotonic_and_bounded() {
        for curve in [Curve::Out, Curve::Standard, Curve::In, Curve::Ease] {
            let mut last = 0.0;
            for i in 0..=100 {
                let v = curve.ease(i as f32 / 100.0);
                assert!((0.0..=1.0).contains(&v) && v >= last, "{curve:?} at {i}");
                last = v;
            }
            assert_eq!((curve.ease(0.0), curve.ease(1.0)), (0.0, 1.0));
        }
        assert!(Curve::Out.ease(0.3) > 0.8);
        assert!(Curve::Standard.ease(0.3) > 0.6);
        assert!(Curve::In.ease(0.3) < 0.2);
        // cubic-bezier(0.4, 0, 0.2, 1) at half time (CSS: 0.78)
        assert!((Curve::Ease.ease(0.5) - 0.78).abs() < 0.02);
    }

    /// The design's tokens (core/motion.js, ui-kit.css, app.js).
    #[test]
    fn the_tokens_are_the_designs() {
        let ms = |d: Duration| d.as_millis();
        assert_eq!(
            [
                ms(OPEN),
                ms(CLOSE),
                ms(MORPH),
                ms(GLIDE),
                ms(DOCK),
                ms(TOAST)
            ],
            [210, 150, 190, 200, 240, 200]
        );
        assert_eq!((TOAST_RISE, OPEN_SCALE), (8.0, 0.93));
        assert_eq!(WIDGET.open, (OPEN, Curve::Out));
        assert_eq!(WIDGET.close, (CLOSE, Curve::In));
        assert_eq!(TOAST_PACE.open, (TOAST, Curve::Ease));
    }

    /// Progress is the time since the start, whatever the frames: one slow
    /// frame lands where the clock is (no jump past it, no lag behind), and
    /// it is done exactly at its length.
    #[test]
    fn progress_follows_the_clock_not_the_frames() {
        let t0 = Instant::now();
        let at = |ms| progress(t0, t0 + Duration::from_millis(ms), GLIDE, Curve::Out);
        // 60 fps or one 100 ms frame: the same place at 100 ms
        let smooth = (0..=6).map(|i| at(i * 16)).next_back().unwrap();
        assert!((smooth - at(96)).abs() < 1e-6);
        assert!(at(100) > at(96) && at(100) < 1.0);
        assert_eq!(at(200), 1.0);
        assert_eq!(at(5000), 1.0);
        assert_eq!(done(t0, t0 - Duration::from_millis(5), GLIDE), 0.0);
    }

    /// Open: 210 ms eased out to 1; a close half-way turns back from where
    /// it is (no jump) and is gone 150 ms later.
    #[test]
    fn shown_opens_and_closes_from_where_it_is() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut s = Shown::opening(WIDGET, t0);
        assert_eq!(s.at(t0), 0.0);
        assert!(s.at(ms(50)) > 0.5 && s.moving(ms(50)), "eased out");
        assert_eq!(s.at(ms(210)), 1.0);
        assert!(!s.moving(ms(210)));
        let mid = s.at(ms(60));
        s.close(ms(60));
        assert_eq!(s.at(ms(60)), mid, "no jump");
        assert!(s.at(ms(100)) < mid && s.moving(ms(100)));
        assert_eq!(s.at(ms(210)), 0.0);
        assert!(!s.moving(ms(210)));
        s.close(ms(300)); // closing already: nothing
        assert_eq!(s.at(ms(300)), 0.0);
        // a close from open: 150 ms eased in
        let c = Shown::closing(WIDGET, t0);
        assert_eq!(c.at(t0), 1.0);
        assert!(c.at(ms(40)) > 0.85, "soft start");
        assert_eq!(c.at(ms(150)), 0.0);
    }

    /// The OS asks for no animation: every motion is there at once.
    #[test]
    fn reduced_motion_is_instant() {
        let t0 = Instant::now();
        set_reduced(true);
        let s = Shown::opening(WIDGET, t0);
        assert_eq!((s.at(t0), s.moving(t0)), (1.0, false));
        assert_eq!(progress(t0, t0, DOCK, Curve::Standard), 1.0);
        let mut c = s;
        c.close(t0);
        assert_eq!((c.at(t0), c.moving(t0)), (0.0, false));
        set_reduced(false);
        assert_eq!(Shown::opening(WIDGET, t0).at(t0), 0.0);
    }
}
