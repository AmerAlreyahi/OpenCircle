//! Crops of the frozen frame: cutting, scaling for export, encoding, stacking.

use super::encode_png;
use iced::Rectangle;

/// A crop of the frozen frame, in physical pixels.
#[derive(Clone)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Its size only (a log line, not megabytes of pixels).
impl std::fmt::Debug for Crop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Crop({}x{})", self.width, self.height)
    }
}

/// Cuts `area` (logical px) out of an RGBA frame whose pixels are `scale` times the
/// logical size. Edges round outwards; `None` if nothing is left on the frame.
pub fn crop(rgba: &[u8], width: u32, height: u32, scale: f32, area: Rectangle) -> Option<Crop> {
    let [x0, y0, x1, y1] = crop_bounds(width, height, scale, area)?;
    if rgba.len() < width as usize * height as usize * 4 {
        return None;
    }
    let row = |y: u32| {
        let start = ((y * width + x0) * 4) as usize;
        &rgba[start..start + ((x1 - x0) * 4) as usize]
    };
    Some(Crop {
        width: x1 - x0,
        height: y1 - y0,
        rgba: (y0..y1).flat_map(row).copied().collect(),
    })
}

/// The frame pixels `crop` takes for `area`: `[x0, y0, x1, y1]`, ends exclusive.
pub fn crop_bounds(width: u32, height: u32, scale: f32, area: Rectangle) -> Option<[u32; 4]> {
    let edge =
        |v: f32, max: u32, round: fn(f32) -> f32| (round(v * scale).max(0.0) as u32).min(max);
    let (x0, y0) = (
        edge(area.x, width, f32::floor),
        edge(area.y, height, f32::floor),
    );
    let x1 = edge(area.x + area.width, width, f32::ceil);
    let y1 = edge(area.y + area.height, height, f32::ceil);
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

/// `crop` scaled down (bicubic, so text stays sharp) until its long edge is at
/// most `max_edge` px; as it is when it already fits or `max_edge` is 0.
pub fn fit_long_edge(crop: Crop, max_edge: u32) -> Crop {
    let long = crop.width.max(crop.height);
    if max_edge == 0 || long <= max_edge {
        return crop;
    }
    let shrink =
        |v: u32| ((u64::from(v) * u64::from(max_edge)).div_ceil(u64::from(long)) as u32).max(1);
    let (width, height) = (shrink(crop.width), shrink(crop.height));
    let Some(source) = image::RgbaImage::from_raw(crop.width, crop.height, crop.rgba) else {
        unreachable!("a Crop's buffer is width * height * 4 bytes");
    };
    let scaled = image::imageops::resize(&source, width, height, image::imageops::CatmullRom);
    Crop {
        width,
        height,
        rgba: scaled.into_raw(),
    }
}

/// `crop` as file bytes: PNG, or JPEG at quality 85 (no alpha: opaque screenshots).
pub fn encode(crop: &Crop, format: crate::config::Format) -> Result<Vec<u8>, String> {
    match format {
        crate::config::Format::Png => encode_png(crop.width, crop.height, &crop.rgba),
        crate::config::Format::Jpeg => {
            let (pixels, _) = crop.rgba.as_chunks::<4>();
            let rgb: Vec<u8> = pixels.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
                .encode(
                    &rgb,
                    crop.width,
                    crop.height,
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| e.to_string())?;
            Ok(out)
        }
    }
}

/// Between stacked crops (several selections sent as one image).
pub const STACK_GAP: u32 = 16;

/// `crops` one under the other, left-aligned, `STACK_GAP` px of white between
/// them and right of the narrower ones. `None` for no crops.
pub fn stack_crops(crops: &[Crop]) -> Option<Crop> {
    let width = crops.iter().map(|c| c.width).max()?;
    let gaps = STACK_GAP * (crops.len() as u32 - 1);
    let height = crops.iter().map(|c| c.height).sum::<u32>() + gaps;
    let mut rgba = vec![255; width as usize * height as usize * 4];
    let mut y = 0;
    for c in crops {
        let row = c.width as usize * 4;
        for (i, line) in c.rgba.chunks_exact(row).enumerate() {
            let at = (y + i) * width as usize * 4;
            rgba[at..at + row].copy_from_slice(line);
        }
        y += (c.height + STACK_GAP) as usize;
    }
    Some(Crop {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Size};

    /// 4x3 frame, pixel (x, y) = [x, y, 0, 255].
    fn frame() -> Vec<u8> {
        (0..3u8)
            .flat_map(|y| (0..4u8).flat_map(move |x| [x, y, 0, 255]))
            .collect()
    }

    #[test]
    fn crop_is_in_physical_pixels() {
        // Scale 2: logical (0.5, 0.5) 1x1 = physical (1, 1) 2x2.
        let area = Rectangle::new(Point::new(0.5, 0.5), Size::new(1.0, 1.0));
        let c = crop(&frame(), 4, 3, 2.0, area).unwrap();
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(
            c.rgba,
            [1, 1, 0, 255, 2, 1, 0, 255, 1, 2, 0, 255, 2, 2, 0, 255]
        );

        // Scale 1.5, fractional edges round outwards: x 1.2..2.1 → 1..3, y 0..1.5 → 0..2.
        let area = Rectangle::new(Point::new(0.8, 0.0), Size::new(0.6, 1.0));
        let c = crop(&frame(), 4, 3, 1.5, area).unwrap();
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(&c.rgba[..4], [1, 0, 0, 255]);
    }

    fn solid(width: u32, height: u32) -> Crop {
        Crop {
            width,
            height,
            rgba: (0..width * height)
                .flat_map(|_| [200, 100, 50, 255])
                .collect(),
        }
    }

    #[test]
    fn export_scales_the_long_edge_down_only() {
        // 4K landscape -> 1568 wide, aspect kept (rounded up, never 0).
        let c = fit_long_edge(solid(3840, 2160), 1568);
        assert_eq!((c.width, c.height), (1568, 882));
        assert_eq!(c.rgba.len(), 1568 * 882 * 4);
        // Flat colour stays that colour (no ringing on a flat area).
        assert!(
            c.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [200, 100, 50, 255])
        );
        // Tall: the height is the long edge.
        let c = fit_long_edge(solid(500, 3000), 1568);
        assert_eq!((c.width, c.height), (262, 1568));
        // Already small, or 0 = original: untouched (never enlarged).
        let c = fit_long_edge(solid(800, 600), 1568);
        assert_eq!((c.width, c.height), (800, 600));
        let c = fit_long_edge(solid(3840, 2160), 0);
        assert_eq!((c.width, c.height), (3840, 2160));
        // A sliver keeps at least one pixel.
        let c = fit_long_edge(solid(5000, 1), 1000);
        assert_eq!((c.width, c.height), (1000, 1));
    }

    #[test]
    fn export_writes_png_or_jpeg() {
        use crate::config::Format;
        let c = solid(40, 30);
        let png = encode(&c, Format::Png).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let jpeg = encode(&c, Format::Jpeg).unwrap();
        assert_eq!(&jpeg[..3], [0xff, 0xd8, 0xff], "JPEG magic");
        let back = image::load_from_memory(&jpeg).unwrap().into_rgb8();
        assert_eq!(back.dimensions(), (40, 30));
        let [r, g, b] = back.get_pixel(20, 15).0;
        let near = |a: u8, b: u8| a.abs_diff(b) <= 4;
        assert!(near(r, 200) && near(g, 100) && near(b, 50), "{r} {g} {b}");
    }

    #[test]
    fn crop_clamps_to_the_frame() {
        let area = Rectangle::new(Point::new(-5.0, 1.0), Size::new(100.0, 100.0));
        let c = crop(&frame(), 4, 3, 1.0, area).unwrap();
        assert_eq!((c.width, c.height), (4, 2));
        assert_eq!(c.rgba.len(), 4 * 2 * 4);
        let off = Rectangle::new(Point::new(10.0, 10.0), Size::new(5.0, 5.0));
        assert!(crop(&frame(), 4, 3, 1.0, off).is_none());
    }

    #[test]
    fn stacked_crops_keep_their_pixels() {
        let red = Crop {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 255, 0, 0, 255],
        };
        let blue = Crop {
            width: 1,
            height: 2,
            rgba: vec![0, 0, 255, 255, 0, 0, 255, 255],
        };
        let s = stack_crops(&[red, blue]).unwrap();
        assert_eq!((s.width, s.height), (2, 1 + STACK_GAP + 2));
        let px = |x: u32, y: u32| &s.rgba[((y * 2 + x) * 4) as usize..][..4];
        assert_eq!(px(1, 0), [255, 0, 0, 255]);
        assert_eq!(px(0, 1), [255; 4], "the gap");
        assert_eq!(px(0, 1 + STACK_GAP), [0, 0, 255, 255]);
        assert_eq!(px(1, 1 + STACK_GAP), [255; 4], "beside the narrow one");
        assert!(stack_crops(&[]).is_none());
    }
}
