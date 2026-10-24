//! Select: pick the topmost item under the pointer, move it, resize it from a
//! corner handle, bend an arrow from its middle handle. `grab` = what a drawing
//! tool moves instead of drawing (only on what is painted, so a box can still be
//! drawn inside a box).
use iced::{Point, Rectangle, Size, Vector};

use super::document::{Item, Shape, span};
use super::{arrow, label};

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
    items.iter().rposition(|item| {
        // A blur's bounds include the far area its blur reads; its stroke is the frame.
        let area = match item.shape {
            Shape::Blur { .. } => item.frame(),
            _ => item.bounds(),
        };
        area.expand(reach).contains(p)
    })
}

/// The topmost item painted at `p` (strokes, outlines and arrow lines within
/// `reach`; solid items anywhere on them).
pub fn grab(items: &[Item], p: Point, reach: f32) -> Option<usize> {
    items.iter().rposition(|item| touches(item, p, reach))
}

fn touches(item: &Item, p: Point, reach: f32) -> bool {
    let near = reach + item.width / 2.0;
    let line = |points: &[Point], near: f32| match points {
        [one] => one.distance(p) <= near,
        _ => points.windows(2).any(|s| to_segment(p, s[0], s[1]) <= near),
    };
    match &item.shape {
        Shape::Rect { a, b } | Shape::Spotlight { a, b } => {
            let f = span(&[*a, *b]);
            f.expand(near).contains(p) && !shrink(f, near).is_some_and(|i| i.contains(p))
        }
        Shape::Ellipse { a, b } => {
            let f = span(&[*a, *b]);
            let (c, rx, ry) = (f.center(), f.width / 2.0, f.height / 2.0);
            let inside = |rx: f32, ry: f32| {
                rx > 0.0
                    && ry > 0.0
                    && ((p.x - c.x) / rx).powi(2) + ((p.y - c.y) / ry).powi(2) <= 1.0
            };
            inside(rx + near, ry + near) && !inside(rx - near, ry - near)
        }
        Shape::Arrow {
            a, b, bend, text, ..
        } => {
            let curve: Vec<Point> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    let u = 1.0 - t;
                    Point::new(
                        u * u * a.x + 2.0 * u * t * bend.x + t * t * b.x,
                        u * u * a.y + 2.0 * u * t * bend.y + t * t * b.y,
                    )
                })
                .collect();
            line(&curve, near.max(arrow::head_length(item.width) / 3.0))
                || (!text.is_empty() && {
                    let size = label::arrow_size(item.width);
                    label::bubble(label::centred(*a, text, size), text, size)
                        .expand(reach)
                        .contains(p)
                })
        }
        Shape::Pen(points) | Shape::Highlighter(points) => line(points, near),
        Shape::Blur { points, size, .. } => line(points, reach + size / 2.0),
        _ => item.frame().expand(reach).contains(p),
    }
}

/// `r` with `by` taken off every side (`None` once nothing is left).
fn shrink(r: Rectangle, by: f32) -> Option<Rectangle> {
    (r.width > 2.0 * by && r.height > 2.0 * by).then(|| {
        Rectangle::new(
            Point::new(r.x + by, r.y + by),
            Size::new(r.width - 2.0 * by, r.height - 2.0 * by),
        )
    })
}

/// Distance from `p` to the segment `a`–`b`.
fn to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (d, v) = (b - a, p - a);
    let len2 = d.x * d.x + d.y * d.y;
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((v.x * d.x + v.y * d.y) / len2).clamp(0.0, 1.0)
    };
    p.distance(a + d * t)
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
