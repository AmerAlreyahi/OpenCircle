//! Drawing stays inside the picture (planning #200): where the picture cannot
//! grow (a Snip area, or the Studio with "Grow the canvas" off) a drag that
//! runs past its edge stops at the last pixels, with room for the line's own
//! width, so the whole shape shows; typed text wraps before the edge. Where
//! the picture grows, nothing changes.
use iced::{Point, Rectangle};

use super::document::{Item, Shape};
use super::{Annotator, arrow, text};

/// `p` inside `view`, `by` px from its edges.
fn within(p: Point, view: Rectangle, by: f32) -> Point {
    let by = by.min(view.width / 2.0).min(view.height / 2.0).max(0.0);
    Point::new(
        p.x.clamp(view.x + by, view.x + view.width - by),
        p.y.clamp(view.y + by, view.y + view.height - by),
    )
}

/// `item` moved inside `view` so that all of it shows: each point at least
/// half its line (and a ring, and an arrow's head) from the edge.
pub fn keep_inside(item: &mut Item, view: Rectangle) {
    let half = item.width / 2.0 + 1.0;
    let ring = if item.outline {
        item.width / 2.0 + 1.0
    } else {
        0.0
    };
    let by = half + ring;
    match &mut item.shape {
        Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
            (*a, *b) = (within(*a, view, by), within(*b, view, by));
        }
        Shape::Arrow {
            a, b, bend, head, ..
        } => {
            let tip = if *head {
                by.max(arrow::head_length(item.width) / 2.0)
            } else {
                by
            };
            (*a, *b, *bend) = (
                within(*a, view, by),
                within(*b, view, tip),
                within(*bend, view, by),
            );
        }
        Shape::Pen(points) | Shape::Highlighter(points) => {
            points.iter_mut().for_each(|p| *p = within(*p, view, by));
        }
        Shape::Step { at, radius, .. } | Shape::Pointer { at, radius } => {
            *at = within(*at, view, *radius + by);
        }
        // a text's top-left: room for a word's start (it wraps at the right edge)
        Shape::Text { at, size, .. } | Shape::Label { at, size, .. } => {
            at.x =
                at.x.clamp(view.x, (view.x + view.width - *size).max(view.x));
            at.y = at.y.clamp(
                view.y,
                (view.y + view.height - text::line_height(*size)).max(view.y),
            );
        }
        // a bubble is centred on `at`
        Shape::Callout { tip, at, size, .. } => {
            *tip = within(*tip, view, by);
            *at = within(*at, view, *size * 1.5);
        }
        _ => {}
    }
}

/// A text's soft wrap: where the picture cannot grow, every line breaks before the area's right edge
/// (set when it is drawn, typed or moved: the saved text keeps no break the user did not type);
/// otherwise none.
pub fn fit_wrap(item: &mut Item, view: Rectangle, inside: bool) {
    if let Shape::Text { at, size, wrap, .. } = &mut item.shape {
        // (a box or a ring round it takes a little more)
        *wrap = inside.then(|| (view.x + view.width - at.x - *size * 0.4).max(*size));
    }
}

impl Annotator {
    /// The picture cannot grow: drawings stop at its edge.
    pub(super) fn keeps_inside(&self) -> bool {
        self.doc.crop.is_some() || !self.grow_canvas
    }

    /// The item being typed or moved: its soft wrap follows the area.
    pub(super) fn wrap_typed(&mut self) {
        let (inside, view) = (self.keeps_inside(), self.doc.view());
        if let Some(pending) = &mut self.pending {
            fit_wrap(&mut pending.item, view, inside);
        }
    }
}
