use crate::core::image as raster;
use crate::core::{Rectangle, Size};
use crate::graphics;

use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::collections::hash_map;

#[derive(Debug)]
pub struct Pipeline {
    cache: RefCell<Cache>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            cache: RefCell::new(Cache::default()),
        }
    }

    pub fn load(
        &self,
        handle: &raster::Handle,
    ) -> Result<raster::Allocation, raster::Error> {
        let mut cache = self.cache.borrow_mut();
        let image = cache.allocate(handle)?;

        #[allow(unsafe_code)]
        Ok(unsafe {
            raster::allocate(handle, Size::new(image.width(), image.height()))
        })
    }

    pub fn dimensions(&self, handle: &raster::Handle) -> Option<Size<u32>> {
        let mut cache = self.cache.borrow_mut();
        let image = cache.allocate(handle).ok()?;

        Some(Size::new(image.width(), image.height()))
    }

    pub fn draw(
        &mut self,
        handle: &raster::Handle,
        filter_method: raster::FilterMethod,
        bounds: Rectangle,
        opacity: f32,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        transform: tiny_skia::Transform,
        clip_mask: Option<&tiny_skia::Mask>,
    ) {
        let mut cache = self.cache.borrow_mut();

        let Ok(image) = cache.allocate(handle) else {
            return;
        };

        let width_scale = bounds.width / image.width() as f32;
        let height_scale = bounds.height / image.height() as f32;

        let transform = transform.pre_scale(width_scale, height_scale);

        let quality = match filter_method {
            raster::FilterMethod::Linear => tiny_skia::FilterQuality::Bilinear,
            raster::FilterMethod::Nearest => tiny_skia::FilterQuality::Nearest,
        };

        pixels.draw_pixmap(
            (bounds.x / width_scale) as i32,
            (bounds.y / height_scale) as i32,
            image,
            &tiny_skia::PixmapPaint {
                quality,
                opacity,
                ..Default::default()
            },
            transform,
            clip_mask,
        );
    }

    /// OpenCircle patch (#23): `draw` for an image that lands pixel for
    /// pixel (`transform` and `bounds` scale it by exactly 1, whole-pixel
    /// offset, opacity 1), clipped to the whole pixels `clip` (x0, y0, x1, y1;
    /// what the clip mask holds, `engine::pixel_rect`): rows copied where the
    /// image is opaque, else a plain premultiplied source-over. Upstream ran
    /// tiny-skia's per-pixel pipeline with the mask (an order of magnitude
    /// slower), and every frame redraws the frozen screen under whatever moved.
    /// `false` (nothing drawn): not such an image; `draw` it.
    pub fn blit(
        &mut self,
        handle: &raster::Handle,
        bounds: Rectangle,
        transform: tiny_skia::Transform,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip: [u32; 4],
    ) -> bool {
        let mut cache = self.cache.borrow_mut();
        if cache.allocate(handle).is_err() {
            return true; // nothing to draw (as `draw`)
        }
        let Some(Some(image)) = cache.entries.get(&handle.id()) else {
            return false;
        };
        let (iw, ih) = (image.width, image.height);
        let (ws, hs) = (bounds.width / iw as f32, bounds.height / ih as f32);
        let whole = |v: f32| (v - v.round()).abs() < 1e-3;
        let t = transform;
        if t.kx != 0.0
            || t.ky != 0.0
            || (t.sx * ws - 1.0).abs() > 1e-5
            || (t.sy * hs - 1.0).abs() > 1e-5
            || !whole(t.tx)
            || !whole(t.ty)
            || !whole(bounds.x / ws)
            || !whole(bounds.y / hs)
        {
            return false;
        }
        // where `draw_pixmap` puts it
        let x = (bounds.x / ws).round() as i64 + t.tx.round() as i64;
        let y = (bounds.y / hs).round() as i64 + t.ty.round() as i64;
        let (w, h) = (pixels.width(), pixels.height());
        let Ok(dst) = bytemuck::try_cast_slice_mut::<u8, u32>(pixels.data_mut())
        else {
            return false;
        };
        let [cx0, cy0, cx1, cy1] = clip.map(i64::from);
        let x0 = x.max(cx0).max(0);
        let x1 = (x + i64::from(iw)).min(cx1).min(i64::from(w));
        let y0 = y.max(cy0).max(0);
        let y1 = (y + i64::from(ih)).min(cy1).min(i64::from(h));
        if x0 >= x1 || y0 >= y1 {
            return true;
        }
        let n = (x1 - x0) as usize;
        for row in y0..y1 {
            let s = (row - y) as usize * iw as usize + (x0 - x) as usize;
            let d = row as usize * w as usize + x0 as usize;
            let (src, dst) = (&image.pixels[s..s + n], &mut dst[d..d + n]);
            if image.opaque {
                dst.copy_from_slice(src);
            } else {
                for (d, &s) in dst.iter_mut().zip(src) {
                    match s.to_ne_bytes()[3] {
                        0 => {}
                        255 => *d = s,
                        _ => *d = over(s, *d),
                    }
                }
            }
        }
        true
    }

    pub fn trim_cache(&mut self) {
        self.cache.borrow_mut().trim();
    }
}

#[derive(Debug, Default)]
struct Cache {
    entries: FxHashMap<raster::Id, Option<Entry>>,
    hits: FxHashSet<raster::Id>,
}

impl Cache {
    pub fn allocate(
        &mut self,
        handle: &raster::Handle,
    ) -> Result<tiny_skia::PixmapRef<'_>, raster::Error> {
        let id = handle.id();

        // OpenCircle patch (#23): pixels handed over as RGBA are converted
        // straight from their bytes (upstream copied them into an image first,
        // then premultiplied pixel by pixel); opaque ones (a frozen screen, a
        // translation) only swap red and blue. The first full-screen frame
        // of a new image cost tens of ms in this conversion.
        if let (hash_map::Entry::Vacant(entry), raster::Handle::Rgba { width, height, pixels, .. }) =
            (self.entries.entry(id), handle)
            && *width > 0
            && *height > 0
            && pixels.len() == *width as usize * *height as usize * 4
        {
            let buffer: Vec<u32> = pixels
                .chunks_exact(4)
                .map(|p| match p[3] {
                    255 => u32::from_ne_bytes([p[2], p[1], p[0], 255]),
                    a => bytemuck::cast(
                        tiny_skia::ColorU8::from_rgba(p[2], p[1], p[0], a)
                            .premultiply(),
                    ),
                })
                .collect();
            let opaque = pixels.chunks_exact(4).all(|p| p[3] == 255);
            let _ = entry.insert(Some(Entry {
                width: *width,
                height: *height,
                pixels: buffer,
                opaque,
            }));
        }

        if let hash_map::Entry::Vacant(entry) = self.entries.entry(id) {
            let image = match graphics::image::load(handle) {
                Ok(image) => image,
                Err(error) => {
                    let _ = entry.insert(None);

                    return Err(error);
                }
            };

            if image.width() == 0 || image.height() == 0 {
                return Err(raster::Error::Empty);
            }

            let mut buffer =
                vec![0u32; image.width() as usize * image.height() as usize];

            for (i, pixel) in image.pixels().enumerate() {
                let [r, g, b, a] = pixel.0;

                buffer[i] = bytemuck::cast(
                    tiny_skia::ColorU8::from_rgba(b, g, r, a).premultiply(),
                );
            }

            let _ = entry.insert(Some(Entry {
                width: image.width(),
                height: image.height(),
                pixels: buffer,
                opaque: false,
            }));
        }

        let _ = self.hits.insert(id);

        Ok(self
            .entries
            .get(&id)
            .unwrap()
            .as_ref()
            .map(|entry| {
                tiny_skia::PixmapRef::from_bytes(
                    bytemuck::cast_slice(&entry.pixels),
                    entry.width,
                    entry.height,
                )
                .expect("Build pixmap from image bytes")
            })
            .expect("Image should be allocated"))
    }

    fn trim(&mut self) {
        self.entries.retain(|key, _| self.hits.contains(key));
        self.hits.clear();
    }
}

#[derive(Debug)]
struct Entry {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    /// Every pixel is opaque (OpenCircle patch: `Pipeline::blit` copies it).
    opaque: bool,
}

/// `s` over `d`, premultiplied, one byte per channel.
fn over(s: u32, d: u32) -> u32 {
    let (s, d) = (s.to_ne_bytes(), d.to_ne_bytes());
    let keep = 255 - u16::from(s[3]);
    // x / 255, rounded, exact for x <= 255 * 255
    let div = |x: u16| ((x + 128 + ((x + 128) >> 8)) >> 8) as u8;
    u32::from_ne_bytes(std::array::from_fn(|i| {
        s[i].saturating_add(div(u16::from(d[i]) * keep))
    }))
}
