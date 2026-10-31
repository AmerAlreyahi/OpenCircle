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
        opacity: f32,
        under: Option<u32>,
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
        if t.kx != 0.0 || t.ky != 0.0 {
            return false;
        }
        if opacity < 1.0
            || (t.sx * ws - 1.0).abs() > 1e-5
            || (t.sy * hs - 1.0).abs() > 1e-5
            || !whole(t.tx)
            || !whole(t.ty)
        {
            return image.opaque
                && scaled(image, bounds, t, pixels, clip, opacity, under);
        }
        // where `draw` puts it: its position truncated to whole pixels (as
        // `draw` hands it to `draw_pixmap`), so the frozen tiles and the edge
        // strips, placed a quarter pixel in against float error, are blitted
        // too (#134: they went through the pipeline, ~20 ms a frame of the
        // opening on the 4-core VM)
        let x = (bounds.x / ws) as i32 as i64 + t.tx.round() as i64;
        let y = (bounds.y / hs) as i32 as i64 + t.ty.round() as i64;
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
        // (#89: on all cores when big, the edge glow's strips under a pan)
        rows(dst, w as usize, n, y0 as usize..y1 as usize, |row, dst| {
            let s = (row as i64 - y) as usize * iw as usize + (x0 - x) as usize;
            let (src, dst) = (&image.pixels[s..s + n], &mut dst[x0 as usize..x0 as usize + n]);
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
        });
        true
    }

    /// OpenCircle patch (#89): an image's size and whether it is opaque,
    /// loaded now if it isn't (`None`: it can't be).
    pub fn loaded(&self, handle: &raster::Handle) -> Option<(u32, u32, bool)> {
        let mut cache = self.cache.borrow_mut();
        let _ = cache.allocate(handle).ok()?;
        let image = cache.entries.get(&handle.id())?.as_ref()?;
        Some((image.width, image.height, image.opaque))
    }

    pub fn trim_cache(&mut self) {
        self.cache.borrow_mut().trim();
    }
}

/// OpenCircle patch (#89): an opaque image scaled by any amount and placed
/// at any fraction of a pixel, Nearest: each device pixel takes the image
/// pixel under its middle (tiny-skia's Nearest, `nearest`), a column table
/// per draw. The overlay's zoom redraws the whole frozen screen every frame
/// of its ease (tiny-skia's pipeline: ~75 ms a 4K frame). At `opacity` below
/// 1 it is drawn over `under` (the one opaque colour beneath it, known to
/// the caller: the dim folded into the picture, one pass), and needs it.
/// `false`: not drawable so (a flipped image, a faded one over unknown pixels).
fn scaled(
    image: &Entry,
    bounds: Rectangle,
    t: tiny_skia::Transform,
    pixels: &mut tiny_skia::PixmapMut<'_>,
    clip: [u32; 4],
    opacity: f32,
    under: Option<u32>,
) -> bool {
    let (iw, ih) = (image.width as usize, image.height as usize);
    let Some((ox, sx, oy, sy)) = placed(image.width, image.height, bounds, t) else {
        return false;
    };
    let under = match under {
        _ if opacity >= 1.0 => None,
        Some(under) => Some(under),
        None => return false,
    };
    let (w, h) = (pixels.width(), pixels.height());
    let Ok(dst) = bytemuck::try_cast_slice_mut::<u8, u32>(pixels.data_mut()) else {
        return false;
    };
    let [x0, y0, x1, y1] = [clip[0], clip[1], clip[2].min(w), clip[3].min(h)];
    let (mut cols, mut first) = (Vec::with_capacity((x1.saturating_sub(x0)) as usize), None);
    for x in x0..x1 {
        if let Some(u) = nearest(x, ox, sx, iw) {
            let _ = first.get_or_insert(x as usize);
            cols.push(u as u32);
        }
    }
    let Some(xa) = first else {
        return true; // nothing of it in the clip
    };
    let n = cols.len();
    // faded: each byte p * o + under * (1 - o), rounded, 8 pixels at a time
    // (the compiler turns the fixed arrays into vector code)
    let o = (opacity.clamp(0.0, 1.0) * 255.0).round() as u16;
    let lift: Option<[u16; 32]> = under.map(|u| {
        std::array::from_fn(|i| u16::from(u.to_ne_bytes()[i % 4]) * (255 - o))
    });
    let fade = |d: &mut [u8], l: &[u16]| {
        for (d, &l) in d.iter_mut().zip(l) {
            let t = u16::from(*d) * o + l + 128;
            *d = ((t + (t >> 8)) >> 8) as u8;
        }
    };
    // only the rows the image covers (drawn within the clip, the clip is the
    // whole window: a 9x loupe cell walked all 1271 rows, and on all cores)
    let ys = (y0..y1).filter(|&y| nearest(y, oy, sy, ih).is_some());
    let (Some(ya), Some(yb)) = (ys.clone().next(), ys.last()) else {
        return true;
    };
    rows(dst, w as usize, n, ya as usize..yb as usize + 1, |y, row| {
        let Some(v) = nearest(y as u32, oy, sy, ih) else {
            return;
        };
        let at = &image.pixels[v * iw..(v + 1) * iw];
        let row = &mut row[xa..xa + n];
        for (p, &u) in row.iter_mut().zip(&cols) {
            *p = at[u as usize];
        }
        if let Some(lift) = &lift {
            let bytes: &mut [u8] = bytemuck::cast_slice_mut(row);
            let mut chunks = bytes.chunks_exact_mut(32);
            for chunk in &mut chunks {
                let chunk: &mut [u8; 32] = chunk.try_into().expect("32 bytes");
                fade(chunk, lift);
            }
            fade(chunks.into_remainder(), lift);
        }
    });
    true
}

/// OpenCircle patch (#89): where `scaled` puts an `iw` x `ih` image drawn
/// in `bounds` by `t`: (x of its corner, device px per image px across, y,
/// down). `None`: flipped or skewed.
pub(crate) fn placed(
    iw: u32,
    ih: u32,
    bounds: Rectangle,
    t: tiny_skia::Transform,
) -> Option<(f64, f64, f64, f64)> {
    let sx = f64::from(t.sx) * f64::from(bounds.width) / f64::from(iw);
    let sy = f64::from(t.sy) * f64::from(bounds.height) / f64::from(ih);
    let ox = f64::from(t.sx) * f64::from(bounds.x) + f64::from(t.tx);
    let oy = f64::from(t.sy) * f64::from(bounds.y) + f64::from(t.ty);
    (t.kx == 0.0 && t.ky == 0.0 && sx > 0.0 && sy > 0.0).then_some((ox, sx, oy, sy))
}

/// OpenCircle patch (#89): the image px (of `n`) Nearest puts in device px
/// `d`, for an image whose corner is at `o` with `s` device px per image px:
/// the one under the pixel's middle. `None`: outside the image.
pub(crate) fn nearest(d: u32, o: f64, s: f64, n: usize) -> Option<usize> {
    let u = ((f64::from(d) + 0.5 - o) / s).floor();
    (u >= 0.0 && u < n as f64).then_some(u as usize)
}

/// OpenCircle patch (#89): `each(y, row)` over the rows `ys` of a `w` px wide
/// image, split between the CPU's cores when there are many pixels (a whole
/// screen each frame of the wheel zoom); in order on one core when few.
/// `px`: the pixels `each` works on in a row (the threads are started for
/// those, not for the row's width: a loupe's strip of a few hundred px
/// started them on each draw, ~0.2 ms here, more on a VM).
pub(crate) fn rows(
    data: &mut [u32],
    w: usize,
    px: usize,
    ys: std::ops::Range<usize>,
    each: impl Fn(usize, &mut [u32]) + Sync,
) {
    let data = &mut data[ys.start * w..ys.end * w];
    if !threaded(ys.len(), px) {
        for (i, row) in data.chunks_exact_mut(w).enumerate() {
            each(ys.start + i, row);
        }
        return;
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    let band = ys.len().div_ceil(cores).max(1);
    SPLIT.set(SPLIT.get() + 1);
    if cores == 1 {
        for (i, row) in data.chunks_exact_mut(w).enumerate() {
            each(ys.start + i, row);
        }
        return;
    }
    std::thread::scope(|scope| {
        for (b, chunk) in data.chunks_mut(band * w).enumerate() {
            let each = &each;
            let _ = scope.spawn(move || {
                for (i, row) in chunk.chunks_exact_mut(w).enumerate() {
                    each(ys.start + b * band + i, row);
                }
            });
        }
    });
}

thread_local! {
    /// OpenCircle patch (#89): how often `rows` split its work between the
    /// cores on this thread (tests: a small redraw never does).
    pub static SPLIT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// OpenCircle patch (#89): `rows` splits `n` rows of `px` pixels between the
/// cores from a megapixel (a zoomed screen; starting the threads costs more
/// than copying a damaged strip on one core, ~0.2 ms here, more on a VM).
pub(crate) fn threaded(n: usize, px: usize) -> bool {
    n * px >= 1 << 20
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
