//! Spotlight: dims everything except the chosen areas (all spotlights together).
use iced::Rectangle;
use tiny_skia::{FillRule, Mask, Transform};

use super::document::{Layer, rounded};

/// The dim outside the spotlights.
pub const DIM: [u8; 4] = [0, 0, 0, 140];
/// The corners of a hole, image px (the live dim round a hole uses it too).
pub const RADIUS: f32 = 10.0;

#[inline(always)]
fn div255(x: u32) -> u32 {
    (x + 255) >> 8
}

/// Exact tiny-skia SourceOver blend of [0, 0, 0, sa] over premultiplied pixel p.
#[inline(always)]
pub(crate) fn dim_pixel_alpha(p: &mut [u8], sa: u32) {
    let inv = 255 - sa;
    p[0] = div255(u32::from(p[0]) * inv) as u8;
    p[1] = div255(u32::from(p[1]) * inv) as u8;
    p[2] = div255(u32::from(p[2]) * inv) as u8;
    p[3] = (sa + div255(u32::from(p[3]) * inv)) as u8;
}

#[inline(always)]
pub(crate) fn dim_row(row: &mut [u8]) {
    const SA: u32 = DIM[3] as u32;
    const INV: u32 = 255 - SA;
    for p in row.as_chunks_mut::<4>().0 {
        p[0] = div255(u32::from(p[0]) * INV) as u8;
        p[1] = div255(u32::from(p[1]) * INV) as u8;
        p[2] = div255(u32::from(p[2]) * INV) as u8;
        p[3] = (SA + div255(u32::from(p[3]) * INV)) as u8;
    }
}

/// Clamped bounding box of the union of all holes in layer-local pixels, plus a tight
/// inverted coverage mask for just that union box (only 1-10 % of an 8K canvas instead of 33 MB).
pub(crate) struct HoleMask {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
    pub mask: Mask,
}

pub(crate) fn build_hole_mask(
    w: u32,
    h: u32,
    origin: iced::Point,
    holes: &[Rectangle],
) -> Option<HoleMask> {
    let mut ux0 = w;
    let mut uy0 = h;
    let mut ux1 = 0u32;
    let mut uy1 = 0u32;
    for hole in holes {
        let hx0 = (hole.x - origin.x).floor().clamp(0.0, w as f32) as u32;
        let hy0 = (hole.y - origin.y).floor().clamp(0.0, h as f32) as u32;
        let hx1 = (hole.x + hole.width - origin.x).ceil().clamp(0.0, w as f32) as u32;
        let hy1 = (hole.y + hole.height - origin.y)
            .ceil()
            .clamp(0.0, h as f32) as u32;
        if hx0 < hx1 && hy0 < hy1 {
            ux0 = ux0.min(hx0);
            uy0 = uy0.min(hy0);
            ux1 = ux1.max(hx1);
            uy1 = uy1.max(hy1);
        }
    }
    if ux0 >= ux1 || uy0 >= uy1 {
        return None;
    }
    let mw = ux1 - ux0;
    let mh = uy1 - uy0;
    let mut mask = Mask::new(mw, mh)?;
    let t = Transform::from_translate(-(origin.x + ux0 as f32), -(origin.y + uy0 as f32));
    for hole in holes {
        if let Some(path) = rounded(*hole, RADIUS) {
            mask.fill_path(&path, FillRule::Winding, true, t);
        }
    }
    mask.invert();
    Some(HoleMask {
        x0: ux0 as usize,
        y0: uy0 as usize,
        x1: ux1 as usize,
        y1: uy1 as usize,
        mask,
    })
}

#[inline]
pub(crate) fn apply_dim_to_row(y: usize, row: &mut [u8], hm: Option<&HoleMask>) {
    let Some(hm) = hm else {
        dim_row(row);
        return;
    };
    if y < hm.y0 || y >= hm.y1 {
        dim_row(row);
        return;
    }
    dim_row(&mut row[..hm.x0 * 4]);
    let mw = hm.x1 - hm.x0;
    let mrow = &hm.mask.data()[(y - hm.y0) * mw..(y - hm.y0 + 1) * mw];
    let mid = &mut row[hm.x0 * 4..hm.x1 * 4];
    for (p, &m) in mid.as_chunks_mut::<4>().0.iter_mut().zip(mrow) {
        if m == 0 {
            continue;
        }
        if m == 255 {
            dim_pixel_alpha(p, u32::from(DIM[3]));
        } else {
            let sa = div255(u32::from(DIM[3]) * u32::from(m));
            if sa > 0 {
                dim_pixel_alpha(p, sa);
            }
        }
    }
    dim_row(&mut row[hm.x1 * 4..]);
}

pub fn paint(layer: &mut Layer, holes: &[Rectangle]) {
    let (w, h) = (layer.pm.width(), layer.pm.height());
    if w == 0 || h == 0 {
        return;
    }
    let hm = build_hole_mask(w, h, layer.origin, holes);
    let stride = w as usize * 4;
    let pixels = layer.pm.data_mut();
    let threads = if (w as usize) * (h as usize) >= 262_144 {
        std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(1)
    } else {
        1
    };
    if threads <= 1 {
        for (y, row) in pixels.chunks_exact_mut(stride).enumerate() {
            apply_dim_to_row(y, row, hm.as_ref());
        }
    } else {
        let rows_per = (h as usize).div_ceil(threads);
        let hm_ref = hm.as_ref();
        std::thread::scope(|scope| {
            for (chunk_i, chunk) in pixels.chunks_mut(rows_per * stride).enumerate() {
                let y0 = chunk_i * rows_per;
                scope.spawn(move || {
                    for (dy, row) in chunk.chunks_exact_mut(stride).enumerate() {
                        apply_dim_to_row(y0 + dy, row, hm_ref);
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::document::solid;
    use super::*;
    use iced::{Point, Size};
    use tiny_skia::{Pixmap, Rect};

    fn paint_reference(layer: &mut Layer, holes: &[Rectangle]) {
        let (w, h) = (layer.pm.width(), layer.pm.height());
        let mut mask = Mask::new(w, h).unwrap();
        let t = layer.t();
        for hole in holes {
            if let Some(path) = rounded(*hole, RADIUS) {
                mask.fill_path(&path, FillRule::Winding, true, t);
            }
        }
        mask.invert();
        let all = Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap();
        layer
            .pm
            .fill_rect(all, &solid(DIM), Transform::identity(), Some(&mask));
    }

    #[test]
    fn fast_spotlight_matches_tiny_skia_bit_for_bit() {
        let (w, h) = (256u32, 192u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let a = if y < 128 { 255 } else { (x as u8).max(1) };
                let premul = |c: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
                data[o] = premul(x as u8);
                data[o + 1] = premul((y.wrapping_mul(3)) as u8);
                data[o + 2] = premul((x ^ y) as u8);
                data[o + 3] = a;
            }
        }
        let holes = [
            Rectangle::new(Point::new(30.5, 25.25), Size::new(80.0, 60.0)),
            Rectangle::new(Point::new(90.0, 50.0), Size::new(110.0, 90.0)),
        ];
        let mut a = Layer {
            pm: Pixmap::from_vec(data.clone(), tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap(),
            origin: Point::ORIGIN,
        };
        let mut b = Layer {
            pm: Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap(),
            origin: Point::ORIGIN,
        };
        paint(&mut a, &holes);
        paint_reference(&mut b, &holes);
        let mut diff = 0usize;
        let mut max_d = 0u8;
        for (pa, pb) in
            a.pm.data()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(b.pm.data().as_chunks::<4>().0)
        {
            let d = (0..4).map(|c| pa[c].abs_diff(pb[c])).max().unwrap();
            if d > 0 {
                diff += 1;
                max_d = max_d.max(d);
            }
        }
        assert_eq!(diff, 0, "spotlight mismatch: {diff} px (max={max_d})");
    }
}
