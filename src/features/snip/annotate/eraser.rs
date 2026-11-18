//! Eraser (Studio round 2, key X): takes drawings off, never the picture. A pen or
//! highlighter stroke loses only the part under the brush ("Whole drawing" off);
//! any other drawing it touches goes whole.
// ponytail: shapes, arrows, texts and blurs go whole even with "Whole drawing" off:
// they are objects, not ink; cutting a part of one needs a per-shape split.
use iced::Point;

use super::document::{Item, Shape};
use super::select;

/// The brush's sizes (display px, as seen on screen): its presets; the default is the middle one.
pub const SIZES: [f32; 3] = [8.0, 16.0, 32.0];

/// Erases what a brush of radius `r` (image px) moved from `a` to `b` touches in
/// `items`. `true` = something went.
pub(super) fn erase(items: &mut Vec<Item>, a: Point, b: Point, r: f32, whole: bool) -> bool {
    let along = path_through(&[a, b], r / 2.0);
    let mut changed = false;
    let mut kept = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if !along.iter().any(|&p| select::touches(&item, p, r)) {
            kept.push(item);
            continue;
        }
        changed = true;
        if let (Shape::Pen(points) | Shape::Highlighter(points), false) = (&item.shape, whole) {
            let near = r + item.width / 2.0;
            let dense = path_through(points, near / 2.0);
            let under = |q: &Point| along.iter().any(|p| p.distance(*q) <= near);
            for run in dense.split(under).filter(|run| run.len() >= 2) {
                let mut piece = item.clone();
                piece.shape = match item.shape {
                    Shape::Pen(_) => Shape::Pen(run.to_vec()),
                    _ => Shape::Highlighter(run.to_vec()),
                };
                kept.push(piece);
            }
        }
    }
    *items = kept;
    changed
}

/// `points` with points added so that none is more than `step` from the next
/// (a fast drag erases the whole way, a long segment is cut where the brush is).
fn path_through(points: &[Point], step: f32) -> Vec<Point> {
    let step = step.max(0.5);
    let mut out = Vec::with_capacity(points.len());
    for (i, &p) in points.iter().enumerate() {
        if let Some(&q) = i.checked_sub(1).and_then(|j| points.get(j)) {
            let n = (q.distance(p) / step).ceil() as usize;
            out.extend((1..n).map(|k| {
                let t = k as f32 / n as f32;
                Point::new(q.x + (p.x - q.x) * t, q.y + (p.y - q.y) * t)
            }));
        }
        out.push(p);
    }
    out
}
