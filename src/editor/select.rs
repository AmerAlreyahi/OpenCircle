//! Select: pick the topmost item under the pointer, move it, resize it from a
//! corner handle, bend an arrow from its middle handle.
use iced::{Point, Vector};

use super::arrow;
use super::document::{Item, Shape};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    /// Frame corner: top-left, top-right, bottom-right, bottom-left.
    Corner(usize),
    /// An arrow's middle (bend) handle.
    Bend,
}

/// The item's handles and where they are (image px).
pub fn handles(item: &Item) -> Vec<(Handle, Point)> {
    let mut all: Vec<_> = corners(item)
        .into_iter()
        .enumerate()
        .map(|(i, p)| (Handle::Corner(i), p))
        .collect();
    if let Shape::Arrow { a, b, bend, .. } = item.shape {
        all.push((Handle::Bend, arrow::middle(a, b, bend)));
    }
    all
}

fn corners(item: &Item) -> [Point; 4] {
    let f = item.frame();
    let (x1, y1) = (f.x + f.width, f.y + f.height);
    [
        f.position(),
        Point::new(x1, f.y),
        Point::new(x1, y1),
        Point::new(f.x, y1),
    ]
}

/// The handle of `item` within `reach` of `p`.
pub fn handle_at(item: &Item, p: Point, reach: f32) -> Option<Handle> {
    handles(item)
        .into_iter()
        .find(|(_, at)| at.distance(p) <= reach)
        .map(|(h, _)| h)
}

/// The topmost item whose painted area (plus `reach`) contains `p`.
// ponytail: bounding-box hit test; thin diagonal strokes are easy to hit near,
// test against the path if that picks the wrong item in practice.
pub fn hit(items: &[Item], p: Point, reach: f32) -> Option<usize> {
    items
        .iter()
        .rposition(|item| item.bounds().expand(reach).contains(p))
}

pub fn moved(item: &Item, by: Vector) -> Item {
    let mut item = item.clone();
    item.shape.transform(|p| p + by, 1.0);
    item
}

/// `original` with corner `corner` of its frame dragged to `p` (the opposite
/// corner stays; dragging past it flips).
pub fn resized(original: &Item, corner: usize, p: Point) -> Item {
    let c = corners(original);
    let (grab, fixed) = (c[corner % 4], c[(corner + 2) % 4]);
    let scale = |to: f32, from: f32, at: f32| {
        if (from - at).abs() < 1.0 {
            1.0
        } else {
            (to - at) / (from - at)
        }
    };
    let (sx, sy) = (scale(p.x, grab.x, fixed.x), scale(p.y, grab.y, fixed.y));
    let mut item = original.clone();
    item.shape.transform(
        |q| {
            Point::new(
                fixed.x + (q.x - fixed.x) * sx,
                fixed.y + (q.y - fixed.y) * sy,
            )
        },
        sy.abs().max(0.05),
    );
    item
}

/// An arrow bent so its middle passes through `p`.
pub fn bent(original: &Item, p: Point) -> Item {
    let mut item = original.clone();
    if let Shape::Arrow { a, b, bend, .. } = &mut item.shape {
        *bend = arrow::bend_through(*a, *b, p);
    }
    item
}
