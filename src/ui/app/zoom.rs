//! The wheel zoom of the frozen screen, eased (planning #89): a notch sets a
//! target view, and the drawn one (`Opening::view`, what every mapping uses)
//! moves there from where it is now, by the clock (`motion::GLIDE`, Standard;
//! a slow frame shows it further on). A notch on the way retargets from
//! where it is, the frame pixel under the pointer staying under it. Reduced
//! motion: at once. The frame clock runs only while it moves.

use std::time::Instant;

use iced::Point;

use crate::ui::kit::motion;
use crate::ui::overlay::View;

use super::App;

/// The drawn view on its way from `from` to `to`; `start` is the first
/// frame's time (the clock the frames step by).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::ui) struct ZoomMotion {
    from: View,
    to: View,
    start: Option<Instant>,
}

impl App {
    /// A wheel's `notches` at `at` (screen px): from the target the zoom is
    /// already heading for, around the pixel under the pointer now.
    pub(in crate::ui) fn zoom_by(&mut self, at: Point, notches: f32) {
        let target = self.o.zoom.map_or(self.o.view, |z| z.to);
        let zoom = target.zoomed(at, notches).zoom();
        self.zoom_to(self.o.view.zoomed_to(at, zoom));
    }

    /// Eases from the drawn view to `to` (at once with reduced motion).
    pub(in crate::ui) fn zoom_to(&mut self, to: View) {
        if motion::reduced() || to == self.o.view {
            self.o.view = to;
            self.o.zoom = None;
            return;
        }
        if self.o.zoom.is_none() {
            self.frame_times.borrow_mut().clear(); // the log's "zoom" line
        }
        let from = self.o.view;
        self.o.zoom = Some(ZoomMotion {
            from,
            to,
            start: None,
        });
    }

    /// The view moves (the zoom easing, a pan held): the frame is drawn by
    /// `overlay::Zoomed`, Nearest; at rest with the usual filter.
    pub(in crate::ui) fn view_moving(&self) -> bool {
        self.o.zoom.is_some() || (self.o.pan.is_some() && self.o.view.crop().is_some())
    }

    /// A frame at `now`: the drawn view where the motion is by then.
    pub(in crate::ui) fn step_zoom(&mut self, now: Instant) {
        let Some(z) = &mut self.o.zoom else {
            return;
        };
        let start = *z.start.get_or_insert(now);
        let t = motion::progress(start, now, motion::GLIDE, motion::Curve::Standard);
        self.o.view = match t >= 1.0 {
            true => z.to,
            false => z.from.lerp(&z.to, t),
        };
        if t >= 1.0 {
            self.o.zoom = None;
            self.log_frames("zoom");
        }
    }
}
