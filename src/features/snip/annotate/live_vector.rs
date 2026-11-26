//! Vector geometry rendering for annotations directly onto iced::widget::canvas::Frame.
//! Draws both pending items (while drawing/dragging) and the pure-vector tail of committed
//! items using the exact same paths, colours, halos, widths, caps and joins as the tiny-skia
//! rasterizers (pen, highlighter, shape, `arrow`, pointer, document), avoiding
//! per-move full-resolution pixmap allocations and full-picture flattens on commit.
use iced::widget::canvas::{Frame, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Point, Rectangle, Size, Vector};

use super::document::{Item, Shape, halo, span};
use super::{arrow, highlighter, pointer, shape};

/// Whether item can be drawn directly as canvas vector geometry.
/// Items that sample the picture (Blur, Spotlight, Magnifier), text-bearing
/// items (Text, Label, Callout, Step, labelled Arrow) and pasted
/// Pictures stay on the raster path.
pub(crate) fn supports(item: &Item) -> bool {
    match &item.shape {
        Shape::Pen(_)
        | Shape::Highlighter(_)
        | Shape::Rect { .. }
        | Shape::Ellipse { .. }
        | Shape::Redact { .. }
        | Shape::Pointer { .. } => true,
        Shape::Arrow { text, .. } => text.is_empty(),
        Shape::Text { .. }
        | Shape::Step { .. }
        | Shape::Label { .. }
        | Shape::Blur { .. }
        | Shape::Spotlight { .. }
        | Shape::Callout { .. }
        | Shape::Magnifier { .. }
        | Shape::Picture { .. } => false,
    }
}

/// Draws item onto `frame` (caller clips `frame` to the visible picture rect).
/// Points are mapped directly into display coordinates and stroke widths are scaled
/// explicitly by zoom * k because Frame::scale does not scale Stroke::width.
pub(crate) fn draw(
    frame: &mut Frame,
    item: &Item,
    view: Rectangle,
    zoom: f32,
    margin: f32,
    k: f32,
) {
    let z = zoom * k;
    let off = margin * k;
    let map = |p: Point| Point::new((p.x - view.x) * z + off, (p.y - view.y) * z + off);

    let (c, w, ring) = (item.colour, item.width, item.outline);
    match &item.shape {
        Shape::Pen(points) => {
            if let Some(path) = smooth_path(points, map) {
                frame.stroke(&path, round_stroke(c, w * z));
            }
        }
        Shape::Highlighter(points) => {
            if let Some(path) = smooth_path(points, map) {
                let hc = [c[0], c[1], c[2], highlighter::ALPHA];
                frame.stroke(&path, round_stroke(hc, w * z));
            }
        }
        Shape::Rect { a, b } => {
            if let Some(r) = norm_rect(*a, *b) {
                let top_left = map(r.position());
                let size = Size::new(r.width * z, r.height * z);
                let path = Path::rectangle(top_left, size);
                if item.fill {
                    let [r, g, b, al] = c;
                    let fill_c = [r, g, b, (f32::from(al) * shape::TINT) as u8];
                    frame.fill(&path, rgba(fill_c));
                }
                if ring {
                    frame.stroke(&path, round_stroke(halo(c), (w + 3.0) * z));
                }
                frame.stroke(&path, round_stroke(c, w * z));
            }
        }
        Shape::Ellipse { a, b } => {
            if let Some(r) = norm_rect(*a, *b) {
                let path = oval_path(r, map);
                if item.fill {
                    let [r, g, b, al] = c;
                    let fill_c = [r, g, b, (f32::from(al) * shape::TINT) as u8];
                    frame.fill(&path, rgba(fill_c));
                }
                if ring {
                    frame.stroke(&path, round_stroke(halo(c), (w + 3.0) * z));
                }
                frame.stroke(&path, round_stroke(c, w * z));
            }
        }
        Shape::Arrow {
            a, b, bend, head, ..
        } => {
            draw_arrow(frame, (*a, *b, *bend), *head, c, w, ring, z, map);
        }
        Shape::Redact { a, b, oval } => {
            let r = span(&[*a, *b]);
            if r.width > 0.0 && r.height > 0.0 {
                let path = match oval {
                    true => oval_path(r, map),
                    false => {
                        Path::rectangle(map(r.position()), Size::new(r.width * z, r.height * z))
                    }
                };
                frame.fill(&path, rgba([c[0], c[1], c[2], 255]));
            }
        }
        Shape::Pointer { at, radius } => {
            let (tail, tip) = pointer::arrow_ends(*at, *radius);
            let mid = Point::new((tail.x + tip.x) / 2.0, (tail.y + tip.y) / 2.0);
            draw_arrow(frame, (tail, tip, mid), true, c, w, true, z, map);
            if *radius > 0.0 {
                let ring_rect = Rectangle::new(
                    *at - Vector::new(*radius, *radius),
                    Size::new(*radius * 2.0, *radius * 2.0),
                );
                let circle = oval_path(ring_rect, map);
                frame.stroke(&circle, round_stroke(halo(c), (w + 3.0) * z));
                frame.stroke(&circle, round_stroke(c, w * z));
                let dot_r = (*radius * 0.28).max(1.5);
                let dot_rect = Rectangle::new(
                    *at - Vector::new(dot_r, dot_r),
                    Size::new(dot_r * 2.0, dot_r * 2.0),
                );
                frame.fill(&oval_path(dot_rect, map), rgba(c));
            }
        }
        _ => {}
    }
}

fn rgba([r, g, b, a]: [u8; 4]) -> Color {
    Color::from_rgba8(r, g, b, f32::from(a) / 255.0)
}

fn round_stroke(colour: [u8; 4], width: f32) -> Stroke<'static> {
    Stroke::default()
        .with_color(rgba(colour))
        .with_width(width)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round)
}

fn norm_rect(a: Point, b: Point) -> Option<Rectangle> {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    (x1 > x0 && y1 > y0).then(|| Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0)))
}

/// Exact quadratic midpoint smoothing matching pen::smooth.
fn smooth_path(points: &[Point], map: impl Fn(Point) -> Point) -> Option<Path> {
    let (&first, &last) = (points.first()?, points.last()?);
    Some(Path::new(|p| {
        p.move_to(map(first));
        if points.len() == 1 {
            p.line_to(map(Point::new(first.x + 0.01, first.y)));
        }
        for w in points.windows(2) {
            let mid = Point::new((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
            p.quadratic_curve_to(map(w[0]), map(mid));
        }
        p.line_to(map(last));
    }))
}

/// Replays `tiny_skia::PathBuilder::from_oval` segments verbatim so ellipses match
/// the committed rasterizer bit-for-bit.
fn oval_path(r: Rectangle, map: impl Fn(Point) -> Point) -> Path {
    let Some(rect) = tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height) else {
        return Path::new(|_| {});
    };
    let Some(skia) = tiny_skia::PathBuilder::from_oval(rect) else {
        return Path::new(|_| {});
    };
    Path::new(|p| {
        for seg in skia.segments() {
            match seg {
                tiny_skia::PathSegment::MoveTo(pt) => p.move_to(map(Point::new(pt.x, pt.y))),
                tiny_skia::PathSegment::LineTo(pt) => p.line_to(map(Point::new(pt.x, pt.y))),
                tiny_skia::PathSegment::QuadTo(c, to) => {
                    p.quadratic_curve_to(map(Point::new(c.x, c.y)), map(Point::new(to.x, to.y)));
                }
                tiny_skia::PathSegment::CubicTo(c1, c2, to) => {
                    p.bezier_curve_to(
                        map(Point::new(c1.x, c1.y)),
                        map(Point::new(c2.x, c2.y)),
                        map(Point::new(to.x, to.y)),
                    );
                }
                tiny_skia::PathSegment::Close => p.close(),
            }
        }
    })
}

/// Exact arrow / line geometry matching `arrow`::paint.
#[allow(clippy::too_many_arguments)]
fn draw_arrow(
    f: &mut Frame,
    (a, b, bend): (Point, Point, Point),
    head: bool,
    colour: [u8; 4],
    width: f32,
    ring: bool,
    z: f32,
    map: impl Fn(Point) -> Point,
) {
    let along = if bend.distance(b) > 0.5 {
        b - bend
    } else {
        b - a
    };
    let len = (along.x * along.x + along.y * along.y).sqrt();
    if len == 0.0 {
        return;
    }
    let dir = Vector::new(along.x / len, along.y / len);
    let head_len = if head {
        arrow::head_length(width).min(a.distance(b) * 0.6)
    } else {
        0.0
    };
    let end = b - dir * (head_len * 0.7);

    let shaft = Path::new(|p| {
        p.move_to(map(a));
        p.quadratic_curve_to(map(bend), map(end));
    });
    let tip = head.then(|| {
        let base = b - dir * head_len;
        let side = Vector::new(-dir.y, dir.x) * (head_len * 0.6);
        Path::new(|p| {
            p.move_to(map(b));
            p.line_to(map(base + side));
            p.line_to(map(base - side));
            p.close();
        })
    });

    let halo_c = halo(colour);
    let joint = width * 0.6;
    if ring {
        f.stroke(&shaft, round_stroke(halo_c, (width + 3.0) * z));
        if let Some(tip) = &tip {
            f.stroke(tip, round_stroke(halo_c, (joint + 3.0) * z));
        }
    }
    f.stroke(&shaft, round_stroke(colour, width * z));
    if let Some(tip) = &tip {
        f.fill(tip, rgba(colour));
        f.stroke(tip, round_stroke(colour, joint * z));
    }
}
