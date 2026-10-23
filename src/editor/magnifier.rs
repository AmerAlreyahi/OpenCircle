//! Magnifier: a round lens showing what is under it, zoomed.
use iced::Point;
use tiny_skia::{FilterQuality, IntRect, Paint, PathBuilder, Pattern, SpreadMode, Transform};

use super::document::{Layer, halo};

pub const ZOOM: f32 = 2.0;

pub fn paint(layer: &mut Layer, at: Point, radius: f32, colour: [u8; 4], width: f32) {
    let Some(lens) = PathBuilder::from_circle(at.x, at.y, radius) else {
        return;
    };
    // The source: the circle of radius/ZOOM around the centre, in pixmap px.
    let c = at - (layer.origin - Point::ORIGIN);
    let r = radius / ZOOM + 1.0;
    let (x0, y0) = ((c.x - r).floor().max(0.0), (c.y - r).floor().max(0.0));
    let x1 = (c.x + r).ceil().min(layer.pm.width() as f32);
    let y1 = (c.y + r).ceil().min(layer.pm.height() as f32);
    let source = IntRect::from_xywh(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
        .and_then(|rect| layer.pm.clone_rect(rect));
    if let Some(source) = source {
        // Pattern px q → image px at + (x0 + q - c) * ZOOM (the fill's transform
        // then maps image px to the pixmap, shader included).
        let to_user = Transform::from_row(
            ZOOM,
            0.0,
            0.0,
            ZOOM,
            at.x + (x0 - c.x) * ZOOM,
            at.y + (y0 - c.y) * ZOOM,
        );
        let paint = Paint {
            shader: Pattern::new(
                source.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bicubic,
                1.0,
                to_user,
            ),
            anti_alias: true,
            ..Paint::default()
        };
        let t = layer.t();
        layer
            .pm
            .fill_path(&lens, &paint, tiny_skia::FillRule::Winding, t, None);
    }
    layer.stroke(&lens, halo(colour), width + 3.0);
    layer.stroke(&lens, colour, width);
}
