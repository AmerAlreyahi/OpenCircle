//! Magnifier: a round or rounded-square lens showing what is under it, zoomed.
use iced::{Point, Rectangle, Size};
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, Pixmap, Stroke, Transform};

use super::document::{Layer, halo, rounded, solid};

/// The lens shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Circle,
    RoundedSquare,
}

/// Offered magnification factors and their labels (one source for UI, config and clamping).
pub const ZOOMS: [(u8, &str); 3] = [(2, "2×"), (3, "3×"), (4, "4×")];
pub const DEFAULT_ZOOM: u8 = ZOOMS[0].0;

pub fn has_zoom(z: u8) -> bool {
    ZOOMS.iter().any(|&(v, _)| v == z)
}

pub fn clamp_zoom(z: f32) -> u8 {
    let lo = ZOOMS[0].0;
    let hi = ZOOMS[ZOOMS.len() - 1].0;
    (z.round() as u8).clamp(lo, hi)
}

pub fn default_zoom() -> f32 {
    f32::from(DEFAULT_ZOOM)
}

/// Old saved magnifiers had no rim flag and always drew their rim; default to true on load.
pub fn default_rim() -> bool {
    true
}

/// Corner radius of a rounded-square lens with half-size radius.
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

/// Premultiplied RGBA8 pixels to magnify from, with top-left origin in image px.
pub struct Source<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub origin: Point,
}

/// Geometry and style of one magnifier lens.
pub struct Lens {
    pub at: Point,
    pub radius: f32,
    pub zoom: f32,
    pub kind: Kind,
    pub colour: [u8; 4],
    pub width: f32,
    pub outline: bool,
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn sample_row_bilinear(
    row0: &[u8],
    row1: &[u8],
    x0: usize,
    x1: usize,
    wx0: u32,
    wx1: u32,
    wy0: u32,
    wy1: u32,
) -> [u8; 4] {
    let w00 = wx0 * wy0;
    let w10 = wx1 * wy0;
    let w01 = wx0 * wy1;
    let w11 = wx1 * wy1;
    let i0 = x0 * 4;
    let i1 = x1 * 4;
    let r = (u32::from(row0[i0]) * w00
        + u32::from(row0[i1]) * w10
        + u32::from(row1[i0]) * w01
        + u32::from(row1[i1]) * w11
        + 32768)
        >> 16;
    let g = (u32::from(row0[i0 + 1]) * w00
        + u32::from(row0[i1 + 1]) * w10
        + u32::from(row1[i0 + 1]) * w01
        + u32::from(row1[i1 + 1]) * w11
        + 32768)
        >> 16;
    let b = (u32::from(row0[i0 + 2]) * w00
        + u32::from(row0[i1 + 2]) * w10
        + u32::from(row1[i0 + 2]) * w01
        + u32::from(row1[i1 + 2]) * w11
        + 32768)
        >> 16;
    let a = (u32::from(row0[i0 + 3]) * w00
        + u32::from(row0[i1 + 3]) * w10
        + u32::from(row1[i0 + 3]) * w01
        + u32::from(row1[i1 + 3]) * w11
        + 32768)
        >> 16;
    [r as u8, g as u8, b as u8, a as u8]
}

#[inline(always)]
fn blend_premul(dst: &mut [u8], src: [u8; 4], cov: u32, overwrite_empty: bool) {
    if cov == 255 && (overwrite_empty || src[3] == 255) {
        dst.copy_from_slice(&src);
        return;
    }
    let sr = (u32::from(src[0]) * cov + 127) / 255;
    let sg = (u32::from(src[1]) * cov + 127) / 255;
    let sb = (u32::from(src[2]) * cov + 127) / 255;
    let sa = (u32::from(src[3]) * cov + 127) / 255;
    if overwrite_empty {
        dst[0] = sr as u8;
        dst[1] = sg as u8;
        dst[2] = sb as u8;
        dst[3] = sa as u8;
        return;
    }
    let inv = 255 - sa;
    dst[0] = (sr + (u32::from(dst[0]) * inv + 127) / 255).min(255) as u8;
    dst[1] = (sg + (u32::from(dst[1]) * inv + 127) / 255).min(255) as u8;
    dst[2] = (sb + (u32::from(dst[2]) * inv + 127) / 255).min(255) as u8;
    dst[3] = (sa + (u32::from(dst[3]) * inv + 127) / 255).min(255) as u8;
}

#[inline(always)]
fn lens_sdf(rel_x: f32, rel_y: f32, radius: f32, kind: Kind, cr: f32, inner: f32) -> f32 {
    match kind {
        Kind::Circle => rel_x.hypot(rel_y) - radius,
        Kind::RoundedSquare => {
            let qx = rel_x.abs() - inner;
            let qy = rel_y.abs() - inner;
            if qx > 0.0 && qy > 0.0 {
                qx.hypot(qy) - cr
            } else {
                qx.max(qy) - cr
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_lens(
    src: &Source<'_>,
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    dst_origin: Point,
    scale_x: f32,
    scale_y: f32,
    at: Point,
    radius: f32,
    zoom: f32,
    kind: Kind,
    overwrite_empty: bool,
) {
    if src.width == 0 || src.height == 0 || dst_w == 0 || dst_h == 0 || radius <= 0.0 {
        return;
    }
    let zoom = zoom.max(1.0);
    let inv_sx = 1.0 / scale_x;
    let inv_sy = 1.0 / scale_y;
    let px_step = inv_sx.max(inv_sy);
    let half_px = 0.5 * px_step;
    let cr = corner_radius(radius);
    let inner = radius - cr;

    let src_stride = src.width as usize * 4;
    let dst_stride = dst_w as usize * 4;
    let max_sx = (src.width - 1) as i64;
    let max_sy = (src.height - 1) as i32;
    let src_max_x = (src.width - 1) as usize;
    let src_max_y = (src.height - 1) as usize;

    let step_x_fp = ((inv_sx / zoom) * 65536.0).round() as i64;
    let base_sx_f = (at.x - src.origin.x) + (dst_origin.x + 0.5 * inv_sx - at.x) / zoom - 0.5;
    let base_sx_fp = (base_sx_f * 65536.0).round() as i64;

    let dy_start =
        (((at.y - radius - half_px - dst_origin.y) * scale_y).floor() as i32).max(0) as u32;
    let dy_end = (((at.y + radius + half_px - dst_origin.y) * scale_y).ceil() as i32)
        .min(dst_h as i32)
        .max(0) as u32;

    for dy in dy_start..dy_end {
        let iy = dst_origin.y + (dy as f32 + 0.5) * inv_sy;
        let rel_y = iy - at.y;
        let abs_y = rel_y.abs();
        let (outer_hx, inner_hx) = match kind {
            Kind::Circle => {
                let r_out = radius + half_px;
                if abs_y >= r_out {
                    continue;
                }
                let ohx = (r_out * r_out - rel_y * rel_y).max(0.0).sqrt();
                let r_in = (radius - half_px).max(0.0);
                let ihx = if abs_y < r_in {
                    (r_in * r_in - rel_y * rel_y).max(0.0).sqrt()
                } else {
                    -1.0
                };
                (ohx, ihx)
            }
            Kind::RoundedSquare => {
                let r_out = radius + half_px;
                if abs_y >= r_out {
                    continue;
                }
                if abs_y <= inner {
                    (r_out, (radius - half_px).max(0.0))
                } else {
                    let dy_c = abs_y - inner;
                    let cr_out = cr + half_px;
                    if dy_c >= cr_out {
                        continue;
                    }
                    let ohx = inner + (cr_out * cr_out - dy_c * dy_c).max(0.0).sqrt();
                    let cr_in = (cr - half_px).max(0.0);
                    let ihx = if dy_c < cr_in {
                        inner + (cr_in * cr_in - dy_c * dy_c).max(0.0).sqrt()
                    } else {
                        -1.0
                    };
                    (ohx, ihx)
                }
            }
        };

        let x_out_lo =
            (((at.x - outer_hx - dst_origin.x) * scale_x - 0.5).floor() as i32).max(0) as usize;
        let x_out_hi = (((at.x + outer_hx - dst_origin.x) * scale_x - 0.5).ceil() as i32)
            .min(dst_w as i32 - 1);
        if x_out_hi < x_out_lo as i32 {
            continue;
        }
        let x_out_hi = x_out_hi as usize;

        let sy = (at.y - src.origin.y) + rel_y / zoom - 0.5;
        let sy_clamped = sy.clamp(0.0, max_sy as f32);
        let y0 = sy_clamped.floor() as usize;
        let y1 = (y0 + 1).min(src_max_y);
        let wy1 = (((sy_clamped - y0 as f32) * 256.0 + 0.5) as u32).min(256);
        let wy0 = 256 - wy1;
        let row0 = &src.rgba[y0 * src_stride..(y0 + 1) * src_stride];
        let row1 = &src.rgba[y1 * src_stride..(y1 + 1) * src_stride];
        let dst_row = &mut dst[dy as usize * dst_stride..(dy as usize + 1) * dst_stride];

        let (x_in_lo, x_in_hi) = if inner_hx > 0.0 {
            let lo = (((at.x - inner_hx - dst_origin.x) * scale_x - 0.5).ceil() as i32)
                .max(x_out_lo as i32) as usize;
            let hi = (((at.x + inner_hx - dst_origin.x) * scale_x - 0.5).floor() as i32)
                .min(x_out_hi as i32);
            if hi >= lo as i32 {
                (lo, hi as usize)
            } else {
                (x_out_hi + 1, x_out_hi)
            }
        } else {
            (x_out_hi + 1, x_out_hi)
        };

        let paint_edge = |dst_row: &mut [u8], dx: usize| {
            let ix = dst_origin.x + (dx as f32 + 0.5) * inv_sx;
            let d = lens_sdf(ix - at.x, rel_y, radius, kind, cr, inner);
            if d >= half_px {
                return;
            }
            let cov = if d <= -half_px {
                255u32
            } else {
                ((0.5 - d / px_step).clamp(0.0, 1.0) * 255.0 + 0.5) as u32
            };
            if cov == 0 {
                return;
            }
            let sx_fp = (base_sx_fp + dx as i64 * step_x_fp).clamp(0, max_sx << 16);
            let x0 = (sx_fp >> 16) as usize;
            let x1 = (x0 + 1).min(src_max_x);
            let wx1 = ((sx_fp >> 8) & 0xff) as u32;
            let wx0 = 256 - wx1;
            let px = sample_row_bilinear(row0, row1, x0, x1, wx0, wx1, wy0, wy1);
            blend_premul(&mut dst_row[dx * 4..dx * 4 + 4], px, cov, overwrite_empty);
        };

        let left_end = x_in_lo.min(x_out_hi + 1);
        for dx in x_out_lo..left_end {
            paint_edge(dst_row, dx);
        }

        if x_in_lo <= x_in_hi {
            let mut sx_fp = base_sx_fp + x_in_lo as i64 * step_x_fp;
            let max_fp = max_sx << 16;
            for dx in x_in_lo..=x_in_hi {
                let clamped = sx_fp.clamp(0, max_fp);
                let x0 = (clamped >> 16) as usize;
                let x1 = (x0 + 1).min(src_max_x);
                let wx1 = ((clamped >> 8) & 0xff) as u32;
                let wx0 = 256 - wx1;
                let px = sample_row_bilinear(row0, row1, x0, x1, wx0, wx1, wy0, wy1);
                blend_premul(&mut dst_row[dx * 4..dx * 4 + 4], px, 255, overwrite_empty);
                sx_fp += step_x_fp;
            }
            for dx in (x_in_hi + 1)..=x_out_hi {
                paint_edge(dst_row, dx);
            }
        }
    }
}

/// Paints a magnifier onto a committed Layer (1:1 image px).
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
    if radius <= 0.0 {
        return;
    }
    let zoom = zoom.max(1.0);
    let c = at - (layer.origin - Point::ORIGIN);
    let r = radius / zoom + 2.0;
    let x0 = (c.x - r).floor().max(0.0) as u32;
    let y0 = (c.y - r).floor().max(0.0) as u32;
    let x1 = (c.x + r).ceil().min(layer.pm.width() as f32) as u32;
    let y1 = (c.y + r).ceil().min(layer.pm.height() as f32) as u32;
    if x1 > x0 && y1 > y0 {
        let sw = x1 - x0;
        let sh = y1 - y0;
        let pm_stride = layer.pm.width() as usize * 4;
        let src_stride = sw as usize * 4;
        let mut copy = vec![0u8; src_stride * sh as usize];
        let pm_data = layer.pm.data();
        for row in 0..sh as usize {
            let s = (y0 as usize + row) * pm_stride + x0 as usize * 4;
            let d = row * src_stride;
            copy[d..d + src_stride].copy_from_slice(&pm_data[s..s + src_stride]);
        }
        let src = Source {
            rgba: &copy,
            width: sw,
            height: sh,
            origin: Point::new(layer.origin.x + x0 as f32, layer.origin.y + y0 as f32),
        };
        let (dw, dh) = (layer.pm.width(), layer.pm.height());
        fill_lens(
            &src,
            layer.pm.data_mut(),
            dw,
            dh,
            layer.origin,
            1.0,
            1.0,
            at,
            radius,
            zoom,
            kind,
            false,
        );
    }
    if outline && let Some(lens) = lens_path(at, radius, kind) {
        layer.stroke(&lens, halo(colour), width + 3.0);
        layer.stroke(&lens, colour, width);
    }
}

/// Paints a live magnifier onto a fresh transparent Pixmap (at display scale when shrunk),
/// reading directly from src with zero copy of the backdrop.
pub fn paint_live(
    src: &Source<'_>,
    dst_origin: Point,
    dst_w: u32,
    dst_h: u32,
    scale_x: f32,
    scale_y: f32,
    lens: &Lens,
) -> Pixmap {
    let mut pm = Pixmap::new(dst_w.max(1), dst_h.max(1)).expect("non-empty live lens");
    let (pw, ph) = (pm.width(), pm.height());
    fill_lens(
        src,
        pm.data_mut(),
        pw,
        ph,
        dst_origin,
        scale_x,
        scale_y,
        lens.at,
        lens.radius,
        lens.zoom,
        lens.kind,
        true,
    );
    if lens.outline
        && let Some(path) = lens_path(lens.at, lens.radius, lens.kind)
    {
        let t = Transform::from_scale(scale_x, scale_y).pre_translate(-dst_origin.x, -dst_origin.y);
        for (colour, width) in [
            (halo(lens.colour), lens.width + 3.0),
            (lens.colour, lens.width),
        ] {
            let stroke = Stroke {
                width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pm.stroke_path(&path, &solid(colour), &stroke, t, None);
        }
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{FilterQuality, IntRect, Paint, Pattern, SpreadMode};

    fn textured_pixmap(w: u32, h: u32) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        for y in 0..h {
            for x in 0..w {
                let diag = ((x + y) / 7) % 2 == 0;
                let bar = (y / 11) % 3 == 0 && (x / 6) % 4 != 0;
                let px = if bar {
                    [45u8, 52, 72, 255]
                } else if diag {
                    [
                        (120 + x * 100 / w) as u8,
                        (150 + y * 80 / h) as u8,
                        220,
                        255,
                    ]
                } else {
                    [(190 + x * 50 / w) as u8, (170 + y * 60 / h) as u8, 140, 255]
                };
                let o = ((y * w + x) * 4) as usize;
                pm.data_mut()[o..o + 4].copy_from_slice(&px);
            }
        }
        pm
    }

    fn paint_bicubic_ref(pm: &mut Pixmap, at: Point, radius: f32, zoom: f32, kind: Kind) {
        let lens = lens_path(at, radius, kind).unwrap();
        let r = radius / zoom + 1.0;
        let x0 = (at.x - r).floor().max(0.0);
        let y0 = (at.y - r).floor().max(0.0);
        let x1 = (at.x + r).ceil().min(pm.width() as f32);
        let y1 = (at.y + r).ceil().min(pm.height() as f32);
        let rect =
            IntRect::from_xywh(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32).unwrap();
        let source = pm.clone_rect(rect).unwrap();
        let to_user = Transform::from_row(
            zoom,
            0.0,
            0.0,
            zoom,
            at.x + (x0 - at.x) * zoom,
            at.y + (y0 - at.y) * zoom,
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
        pm.fill_path(
            &lens,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    #[test]
    fn w117_review_sampler_stays_within_tolerance_of_bicubic_at_2x_3x_4x() {
        let base = textured_pixmap(200, 200);
        let at = Point::new(100.0, 100.0);
        let radius = 56.0;
        for kind in [Kind::Circle, Kind::RoundedSquare] {
            for &(z, _) in &ZOOMS {
                let zoom = f32::from(z);
                let mut fast = Layer {
                    pm: base.clone(),
                    origin: Point::ORIGIN,
                };
                paint(
                    &mut fast,
                    at,
                    radius,
                    zoom,
                    kind,
                    [255, 0, 0, 255],
                    4.0,
                    false,
                );
                let mut bicubic = base.clone();
                paint_bicubic_ref(&mut bicubic, at, radius, zoom, kind);

                let cr = corner_radius(radius);
                let inner = radius - cr;
                let (mut count, mut sum_diff, mut over_tol) = (0usize, 0u64, 0usize);
                for y in 40..160u32 {
                    for x in 40..160u32 {
                        let d = lens_sdf(
                            x as f32 + 0.5 - at.x,
                            y as f32 + 0.5 - at.y,
                            radius,
                            kind,
                            cr,
                            inner,
                        );
                        if d > -2.0 {
                            continue;
                        }
                        count += 1;
                        let o = ((y * 200 + x) * 4) as usize;
                        let a = &fast.pm.data()[o..o + 4];
                        let b = &bicubic.data()[o..o + 4];
                        let max_c = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
                        sum_diff += u64::from(max_c);
                        if max_c > 24 {
                            over_tol += 1;
                        }
                    }
                }
                let mean = sum_diff as f64 / count as f64;
                assert!(count > 5000, "{kind:?} {z}x: too few interior px {count}");
                assert!(
                    mean < 4.5,
                    "{kind:?} {z}x: mean channel diff {mean:.2} too high vs bicubic"
                );
                assert!(
                    over_tol * 50 <= count,
                    "{kind:?} {z}x: {over_tol}/{count} interior px differed > 24 vs bicubic"
                );
            }
        }
    }
}
