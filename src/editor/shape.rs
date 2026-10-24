//! Rectangle and ellipse, and the smart shape: a freehand stroke that is close to
//! a rectangle, ellipse, line or arrow snaps to the clean one.
use iced::Point;
use tiny_skia::{PathBuilder, Rect};

use super::document::{Layer, Shape, span};

pub fn paint_rect(layer: &mut Layer, a: Point, b: Point, colour: [u8; 4], width: f32) {
    if let Some(path) = rect(a, b).map(PathBuilder::from_rect) {
        layer.stroke(&path, colour, width);
    }
}

pub fn paint_ellipse(layer: &mut Layer, a: Point, b: Point, colour: [u8; 4], width: f32) {
    if let Some(path) = rect(a, b).and_then(PathBuilder::from_oval) {
        layer.stroke(&path, colour, width);
    }
}

fn rect(a: Point, b: Point) -> Option<Rect> {
    Rect::from_ltrb(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

/// Fit errors (mean distance to the shape, relative to its half-size) below this snap.
const FIT: f32 = 0.12;

/// What a freehand stroke snaps to, or `None` (it stays freehand).
// ponytail: mean-error fits against the stroke's bounding box; no rotated shapes.
pub fn snap(points: &[Point]) -> Option<Shape> {
    let (&a, &b) = (points.first()?, points.last()?);
    let bbox = span(points);
    let size = bbox.width.max(bbox.height);
    if points.len() < 3 || size < 12.0 {
        return None;
    }
    let mid = |a: Point, b: Point| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);

    if straight(points, a, b, 0.06) {
        return Some(Shape::Arrow {
            a,
            b,
            bend: mid(a, b),
            head: false,
            text: String::new(),
        });
    }

    // Arrow: a straight shaft to the point farthest from the start, then a short
    // hook drawn back from that tip (the head).
    let (tip_at, &tip) = points
        .iter()
        .enumerate()
        .max_by(|(_, p), (_, q)| a.distance(**p).total_cmp(&a.distance(**q)))?;
    let shaft = a.distance(tip);
    let hook = &points[tip_at..];
    let hook_length: f32 = hook.windows(2).map(|w| w[0].distance(w[1])).sum();
    if tip_at >= 2
        && straight(&points[..=tip_at], a, tip, 0.08)
        && (0.08 * shaft..0.8 * shaft).contains(&hook_length)
        && hook.iter().all(|p| p.distance(tip) < 0.4 * shaft)
    {
        return Some(Shape::Arrow {
            a,
            b: tip,
            bend: mid(a, tip),
            head: true,
            text: String::new(),
        });
    }

    // Closed loops: whichever of ellipse / rectangle fits the stroke better.
    let (rx, ry) = (bbox.width / 2.0, bbox.height / 2.0);
    if a.distance(b) > 0.25 * size || rx.min(ry) < 6.0 {
        return None;
    }
    let c = bbox.center();
    let n = points.len() as f32;
    let ellipse = points
        .iter()
        .map(|p| (((p.x - c.x) / rx).hypot((p.y - c.y) / ry) - 1.0).abs())
        .sum::<f32>()
        / n;
    let (x0, y0, x1, y1) = (bbox.x, bbox.y, bbox.x + bbox.width, bbox.y + bbox.height);
    let rectangle = points
        .iter()
        .map(|p| (p.x - x0).min(x1 - p.x).min(p.y - y0).min(y1 - p.y).abs())
        .sum::<f32>()
        / n
        / rx.min(ry);
    let (a, b) = (bbox.position(), Point::new(x1, y1));
    if ellipse < rectangle && ellipse < FIT {
        Some(Shape::Ellipse { a, b })
    } else if rectangle <= ellipse && rectangle < FIT {
        Some(Shape::Rect { a, b })
    } else {
        None
    }
}

/// Every point stays within `stray` × |ab| of the segment a–b.
fn straight(points: &[Point], a: Point, b: Point, stray: f32) -> bool {
    let chord = a.distance(b);
    let ab = b - a;
    chord > 0.0
        && points.iter().all(|&p| {
            let ap = p - a;
            let t = ((ap.x * ab.x + ap.y * ab.y) / (chord * chord)).clamp(0.0, 1.0);
            p.distance(a + ab * t) <= stray * chord
        })
}
