//! Magnifier: a round or rounded-square lens showing what is under it, zoomed.
use iced::{Point, Rectangle, Size};
use tiny_skia::{FilterQuality, IntRect, Paint, Path, PathBuilder, Pattern, SpreadMode, Transform};

use super::document::{Layer, halo, rounded};

/// The lens shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Circle,
    RoundedSquare,
}

/// Offered magnification factors (the options bar and config contract).
pub const ZOOMS: [u8; 3] = [2, 3, 4];
pub const DEFAULT_ZOOM: u8 = 2;

pub fn default_zoom() -> f32 {
    f32::from(DEFAULT_ZOOM)
}

/// Corner radius of a rounded-square lens with half-size `radius`.
pub fn corner_radius(radius: f32) -> f32 {
    (radius * 0.28).clamp(4.0, radius)
}

pub fn lens_path(at: Point, radius: f32, kind: Kind) -> Option<Path> {
    if radius <= 0.0 {
        return None;
    }
    match kind {
        Kind::Circle => PathBuilder::from_circle(at.x, at.y, radius),
        Kind::RoundedSquare => rounded(
            Rectangle::new(
                Point::new(at.x - radius, at.y - radius),
                Size::new(radius * 2.0, radius * 2.0),
            ),
            corner_radius(radius),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn paint(
    layer: &mut Layer,
    at: Point,
    radius: f32,
    zoom: f32,
    kind: Kind,
    colour: [u8; 4],
    width: f32,
    outline: bool,
) {
    let Some(lens) = lens_path(at, radius, kind) else {
        return;
    };
    let zoom = zoom.max(1.0);
    // The source: the region of half-size radius/zoom around the centre, in pixmap px.
    let c = at - (layer.origin - Point::ORIGIN);
    let r = radius / zoom + 1.0;
    let (x0, y0) = ((c.x - r).floor().max(0.0), (c.y - r).floor().max(0.0));
    let x1 = (c.x + r).ceil().min(layer.pm.width() as f32);
    let y1 = (c.y + r).ceil().min(layer.pm.height() as f32);
    let source = IntRect::from_xywh(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
        .and_then(|rect| layer.pm.clone_rect(rect));
    if let Some(source) = source {
        // Pattern px q -> image px at + (x0 + q - c) * zoom (the fill's transform
        // then maps image px to the pixmap, shader included).
        let to_user = Transform::from_row(
            zoom,
            0.0,
            0.0,
            zoom,
            at.x + (x0 - c.x) * zoom,
            at.y + (y0 - c.y) * zoom,
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
    if outline {
        layer.stroke(&lens, halo(colour), width + 3.0);
        layer.stroke(&lens, colour, width);
    }
}
