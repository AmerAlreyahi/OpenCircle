//! A live pin's click-through rule (R-12): clicks pass to the app under it, unless Ctrl is held
//! with the pointer over the pin. The rule runs on its own thread (`Watch`) against the pin's
//! real window rectangle, both in physical px (so a pin dragged between screens of different
//! scales stays right), and sends a message only when what the pin shows changes: a beat
//! costs a few syscalls and no redraw.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc;
use iced::window;

use crate::shared::platform::sys::pointer;

/// How far round a pin (physical px) the pointer counts as "near": the watch beats fast there.
const NEAR: i32 = 120;
const FAR_BEAT: Duration = Duration::from_millis(300);
const NEAR_BEAT: Duration = Duration::from_millis(40);
/// The pointer rests this long over a pin (no Ctrl) before the hint shows, once per pin.
const DWELL: Duration = Duration::from_millis(700);

/// What a pin does and shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    /// Clicks pass through to the app under it.
    pub through: bool,
    /// "Hold Ctrl to move or close" shows.
    pub hint: bool,
}

/// The pin's rectangle `[l, t, r, b]` and the pointer (both physical px): over it, and near it.
pub fn place(rect: [i32; 4], at: (i32, i32)) -> (bool, bool) {
    let has = |m: i32| {
        (rect[0] - m..rect[2] + m).contains(&at.0) && (rect[1] - m..rect[3] + m).contains(&at.1)
    };
    (has(0), has(NEAR))
}

/// One pin's rule over time.
pub struct Tracker {
    sent: Verdict,
    /// Since when the pointer rests over it without Ctrl.
    since: Option<Instant>,
    /// The hint showed once already.
    hinted: bool,
}

impl Default for Tracker {
    fn default() -> Self {
        Tracker {
            sent: Verdict {
                through: true,
                hint: false,
            },
            since: None,
            hinted: false,
        }
    }
}

impl Tracker {
    /// A look at Ctrl and the pointer: the new verdict when it differs from the last one.
    pub fn step(&mut self, ctrl: bool, over: bool, now: Instant) -> Option<Verdict> {
        let resting = over && !ctrl;
        match resting {
            true => {
                self.since.get_or_insert(now);
            }
            false => self.since = None,
        }
        let waited = self.since.is_some_and(|s| now.duration_since(s) >= DWELL);
        let hint = resting && (self.sent.hint || (waited && !self.hinted));
        self.hinted |= hint;
        let v = Verdict {
            through: !(ctrl && over),
            hint,
        };
        (v != self.sent).then(|| {
            self.sent = v;
            v
        })
    }

    /// The pin was put back to passing clicks through (the hint stays used up).
    fn reset(&mut self) {
        *self = Tracker {
            hinted: self.hinted,
            ..Tracker::default()
        };
    }
}

struct Entry {
    id: window::Id,
    hwnd: isize,
    active: bool,
    tracker: Tracker,
}

type Shared = Arc<(Mutex<Vec<Entry>>, Condvar)>;

/// The thread that watches Ctrl and the pointer for every live pin on the rule. It sleeps (no
/// wake-ups) while none is active.
pub struct Watch {
    shared: Shared,
    tx: mpsc::UnboundedSender<(window::Id, Verdict)>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<(window::Id, Verdict)>>>,
    started: std::sync::Once,
}

impl Default for Watch {
    fn default() -> Self {
        let (tx, rx) = mpsc::unbounded();
        Watch {
            shared: Shared::default(),
            tx,
            rx: Mutex::new(Some(rx)),
            started: std::sync::Once::new(),
        }
    }
}

impl Watch {
    /// The verdicts, once: the caller turns them into messages.
    pub fn stream(&self) -> Option<mpsc::UnboundedReceiver<(window::Id, Verdict)>> {
        self.rx.lock().ok()?.take()
    }

    /// Starts watching the pin `id` (its window `hwnd`), passing clicks through.
    pub fn add(&self, id: window::Id, hwnd: isize) {
        let (list, cvar) = &*self.shared;
        let mut list = list.lock().unwrap_or_else(|e| e.into_inner());
        list.retain(|e| e.id != id);
        list.push(Entry {
            id,
            hwnd,
            active: true,
            tracker: Tracker::default(),
        });
        cvar.notify_one();
        drop(list);
        self.started.call_once(|| {
            let (shared, tx) = (self.shared.clone(), self.tx.clone());
            if let Err(e) = std::thread::Builder::new()
                .name("pin-watch".into())
                .spawn(move || run(&shared, &tx))
            {
                crate::log!("cannot start a thread: {e}");
            }
        });
    }

    /// The pin is closed: its handle is never read again.
    pub fn remove(&self, id: window::Id) {
        let (list, cvar) = &*self.shared;
        list.lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|e| e.id != id);
        cvar.notify_one();
    }

    #[cfg(test)]
    fn watching(&self) -> usize {
        self.shared.0.lock().unwrap().len()
    }

    /// The pin follows the rule (`true`, passing through again) or not (latched clickable).
    pub fn active(&self, id: window::Id, on: bool) {
        let (list, cvar) = &*self.shared;
        let mut list = list.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = list.iter_mut().find(|e| e.id == id) {
            e.active = on;
            e.tracker.reset();
        }
        cvar.notify_one();
    }
}

fn run(shared: &Shared, tx: &mpsc::UnboundedSender<(window::Id, Verdict)>) {
    let (list, cvar) = &**shared;
    let mut list = list.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        list.retain(|e| e.hwnd != 0);
        while !list.iter().any(|e| e.active) {
            list = cvar.wait(list).unwrap_or_else(|e| e.into_inner());
        }
        let mut near = false;
        if let Some(p) = pointer::probe() {
            let now = Instant::now();
            for e in list.iter_mut().filter(|e| e.active) {
                let Some(rect) = pointer::window_rect(e.hwnd) else {
                    e.hwnd = 0; // closed: dropped
                    e.active = false;
                    continue;
                };
                let (over, close) = place(rect, p.at);
                near |= close;
                if let Some(v) = e.tracker.step(p.ctrl, over, now)
                    && tx.unbounded_send((e.id, v)).is_err()
                {
                    return;
                }
            }
        }
        let beat = if near { NEAR_BEAT } else { FAR_BEAT };
        list = cvar
            .wait_timeout(list, beat)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: &mut Tracker, ctrl: bool, over: bool, ms: u64, t0: Instant) -> Option<Verdict> {
        t.step(ctrl, over, t0 + Duration::from_millis(ms))
    }

    /// R12-1: clicks pass through; Ctrl with the pointer over the pin makes it clickable and
    /// only then; messages come only when something changes.
    #[test]
    fn r12_1_ctrl_over_the_pin_makes_it_clickable() {
        let (mut t, t0) = (Tracker::default(), Instant::now());
        assert_eq!(
            at(&mut t, false, false, 0, t0),
            None,
            "starts through: nothing to say"
        );
        assert_eq!(at(&mut t, true, false, 10, t0), None, "Ctrl away from it");
        let click = at(&mut t, true, true, 20, t0).unwrap();
        assert!(!click.through && !click.hint);
        assert_eq!(at(&mut t, true, true, 30, t0), None, "no repeat per beat");
        assert!(
            at(&mut t, false, true, 40, t0).unwrap().through,
            "release: through"
        );
        assert!(!at(&mut t, true, true, 50, t0).unwrap().through);
        assert!(
            at(&mut t, true, false, 60, t0).unwrap().through,
            "pointer leaves"
        );
    }

    /// R12-4: the hint comes after the pointer rests ~700 ms over the pin, once per pin.
    #[test]
    fn r12_4_the_hint_waits_for_a_rest_and_shows_once() {
        let (mut t, t0) = (Tracker::default(), Instant::now());
        assert_eq!(
            at(&mut t, false, true, 0, t0),
            None,
            "just arrived: no hint"
        );
        assert_eq!(at(&mut t, false, true, 600, t0), None);
        assert!(at(&mut t, false, true, 700, t0).unwrap().hint, "rested");
        assert_eq!(
            at(&mut t, false, true, 900, t0),
            None,
            "stays up while resting"
        );
        assert!(
            !at(&mut t, false, false, 1000, t0).unwrap().hint,
            "leaving hides it"
        );
        assert_eq!(at(&mut t, false, true, 1100, t0), None);
        assert_eq!(at(&mut t, false, true, 2500, t0), None, "never twice");
        // Ctrl held before the rest ends: no hint at all on a fresh pin
        let mut fresh = Tracker::default();
        let _ = at(&mut fresh, false, true, 0, t0);
        assert!(!at(&mut fresh, true, true, 800, t0).unwrap().hint);
    }

    /// R12-6: a closed pin's entry is dropped, so its handle is never watched again.
    #[test]
    fn r12_6_a_closed_pin_is_forgotten() {
        let (w, a, b) = (Watch::default(), window::Id::unique(), window::Id::unique());
        w.add(a, 0x10);
        w.add(b, 0x20);
        assert_eq!(w.watching(), 2);
        w.remove(a);
        assert_eq!(w.watching(), 1);
        w.remove(a);
        assert_eq!(w.watching(), 1, "twice is harmless");
    }

    /// R12-5: pointer and window are compared in physical px, so a pin that moved from a 150 %
    /// screen to a 100 % one is judged by the rectangle it has now.
    #[test]
    fn r12_5_the_rectangle_is_physical_whatever_the_scale() {
        let pointer = (500, 250);
        let at150 = [100, 100, 700, 400]; // 400x200 logical at 150 %: 600x300 px
        let at100 = [100, 100, 500, 300]; // dragged to a 100 % screen: 400x200 px
        assert_eq!(place(at150, pointer), (true, true));
        assert_eq!(place(at100, pointer), (false, true), "outside, near");
        assert_eq!(place(at100, (900, 900)), (false, false));
        assert_eq!(
            place(at100, (300, 900)),
            (false, false),
            "right column, far below"
        );
    }
}
