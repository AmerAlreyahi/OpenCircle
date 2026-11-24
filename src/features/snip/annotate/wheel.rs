//! Ctrl+wheel over the selection resizes it, round its centre (owner,
//! 2026-11-24, #202). The ONE path for the Studio and the Snip editor.
use std::time::{Duration, Instant};

use iced::{Point, Rectangle, Size};

use super::Annotator;

/// One wheel notch in pixels (touchpads send pixels): the Studio's and Snip's.
pub const WHEEL_PX: f32 = 40.0;
/// One notch changes the size by this factor.
const STEP: f32 = 1.06;
/// Notches this close are one gesture: one undo step.
const BURST: Duration = Duration::from_millis(600);
/// The smallest side a resize may leave (image px).
const SMALLEST: f32 = 4.0;

/// A gesture in progress: when it last turned, the history state it left, the
/// selection's box before it and the whole factor so far.
#[derive(Clone, Copy)]
pub(super) struct Burst {
    at: Instant,
    state: u64,
    from: Rectangle,
    k: f32,
}

impl Annotator {
    /// `at` (image px) is over the selection: where Ctrl+wheel resizes.
    pub fn wheel_over(&self, at: Point) -> bool {
        self.selection_bounds().is_some_and(|b| b.contains(at))
    }

    /// Ctrl+wheel at `at` (image px): resizes the selection when `at` is over
    /// it (`false`: not over it, nothing changed, the caller keeps the wheel).
    pub fn wheel_resize(&mut self, at: Point, notches: f32, now: Instant) -> bool {
        let Some(bounds) = self.selection_bounds().filter(|_| self.wheel_over(at)) else {
            return false;
        };
        let sel = self.doc.selection();
        let more = self.wheel.filter(|b| {
            now.saturating_duration_since(b.at) <= BURST && b.state == self.doc.state()
        });
        // a gesture goes on: its last step is taken back and redone whole
        let (from, k) = match more {
            Some(b) => {
                self.doc.undo();
                self.doc.select(&sel);
                (b.from, b.k)
            }
            None => (bounds, 1.0),
        };
        let floor = SMALLEST / from.width.min(from.height).max(1.0);
        let k = (k * STEP.powf(notches)).max(floor);
        let size = from.size() * k;
        let to = Rectangle::new(
            Point::new(
                from.center_x() - size.width / 2.0,
                from.center_y() - size.height / 2.0,
            ),
            Size::new(size.width, size.height),
        );
        let before = self.doc.state();
        self.set_selection_bounds(to);
        if self.doc.state() == before {
            // refused (the canvas may not grow): put the taken step back
            if more.is_some() {
                self.doc.redo();
                self.doc.select(&sel);
            }
            self.wheel = None;
            return true;
        }
        self.wheel = Some(Burst {
            at: now,
            state: self.doc.state(),
            from,
            k,
        });
        true
    }
}
