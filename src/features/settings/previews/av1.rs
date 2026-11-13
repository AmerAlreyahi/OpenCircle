//! The previews' video: AV1 in IVF (owner, 2026-11-04), decoded by rav1d,
//! dav1d's Rust port (BSD-2, no asm: nothing to assemble on any build), behind
//! this small safe API. 8-bit 4:2:0 only (what `render-av1.py` writes), BT.709
//! limited range, turned into RGBA at the size the stage shows it.
use std::ffi::c_int;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::time::Duration;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::DAV1D_PIXEL_LAYOUT_I420;
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};

/// dav1d's "try again" (-EAGAIN; 11 on Linux and in the MSVC CRT alike).
const AGAIN: c_int = -11;

/// The frames of an IVF file, in order.
pub struct Ivf<'a> {
    rest: &'a [u8],
    /// How long each frame shows (one tick of the file's time base).
    pub frame: Duration,
}

/// `bytes` as IVF holding AV1 (`None`: anything else).
pub fn ivf(bytes: &[u8]) -> Option<Ivf<'_>> {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    if bytes.len() < 32 || &bytes[..4] != b"DKIF" || &bytes[8..12] != b"AV01" {
        return None;
    }
    let header = usize::from(u16_at(6));
    let (rate, scale) = (u32_at(16), u32_at(20));
    if header < 32 || header > bytes.len() || rate == 0 || scale == 0 {
        return None;
    }
    let frame = Duration::from_secs_f64(f64::from(scale) / f64::from(rate));
    Some(Ivf {
        rest: &bytes[header..],
        frame: frame.max(Duration::from_millis(10)),
    })
}

impl<'a> Iterator for Ivf<'a> {
    type Item = &'a [u8];

    /// The next frame's data (a cut-off one ends the clip).
    fn next(&mut self) -> Option<&'a [u8]> {
        let size = u32::from_le_bytes(self.rest.get(..4)?.try_into().ok()?) as usize;
        let data = self.rest.get(12..12 + size)?;
        self.rest = &self.rest[12 + size..];
        Some(data)
    }
}

/// One rav1d decoder (its own threads), closed when dropped.
pub struct Decoder(Dav1dContext);

// SAFETY: the context is only used through `&mut self` (one call at a time);
// dav1d contexts may move between threads.
unsafe impl Send for Decoder {}

impl Decoder {
    /// `threads`: rav1d's worker threads; one frame in flight (the least
    /// memory, pictures come out at once).
    pub fn new(threads: c_int) -> Option<Self> {
        let mut s = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: `s` is valid to write to; `dav1d_default_settings` fills it.
        let mut s = unsafe {
            dav1d_default_settings(NonNull::new(s.as_mut_ptr())?);
            s.assume_init()
        };
        s.n_threads = threads;
        s.max_frame_delay = 1;
        let mut c: Option<Dav1dContext> = None;
        // SAFETY: both pointers are valid for the call.
        let r = unsafe { dav1d_open(NonNull::new(&mut c), NonNull::new(&mut s)) };
        if r.0 != 0 {
            return None;
        }
        c.map(Decoder)
    }

    /// Decodes one IVF frame; each picture it completes goes to `out`.
    pub fn decode(&mut self, packet: &[u8], mut out: impl FnMut(&Picture)) -> Result<(), c_int> {
        let mut data = Dav1dData::default();
        // SAFETY: `data` is valid to write to; on success the returned buffer
        // holds `packet.len()` bytes, owned by `data`.
        let to = unsafe { dav1d_data_create(NonNull::new(&mut data), packet.len()) };
        if to.is_null() {
            return Err(-1);
        }
        // SAFETY: `to` has room for `packet.len()` bytes and is not `packet`.
        unsafe { std::ptr::copy_nonoverlapping(packet.as_ptr(), to, packet.len()) };
        let result = loop {
            // SAFETY: the context is open; `data` is valid to read and write.
            let sent = unsafe { dav1d_send_data(Some(self.0), NonNull::new(&mut data)) }.0;
            if sent != 0 && sent != AGAIN {
                break Err(sent);
            }
            if let Err(e) = self.pictures(&mut out) {
                break Err(e);
            }
            if data.sz == 0 {
                break Ok(());
            }
        };
        if data.sz != 0 {
            // SAFETY: `data` still owns what was not sent.
            unsafe { dav1d_data_unref(NonNull::new(&mut data)) };
        }
        result
    }

    /// Every picture ready now, to `out`.
    fn pictures(&mut self, out: &mut impl FnMut(&Picture)) -> Result<(), c_int> {
        loop {
            let mut pic = Dav1dPicture::default();
            // SAFETY: the context is open; `pic` is valid to write to.
            let r = unsafe { dav1d_get_picture(Some(self.0), NonNull::new(&mut pic)) }.0;
            match r {
                0 => out(&Picture(pic)),
                AGAIN => return Ok(()),
                e => return Err(e),
            }
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let mut c = Some(self.0);
        // SAFETY: the context came from `dav1d_open` and is closed only here.
        unsafe { dav1d_close(NonNull::new(&mut c)) };
    }
}

/// A decoded picture (released when dropped).
pub struct Picture(Dav1dPicture);

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: the picture came from `dav1d_get_picture` and is released once.
        unsafe { dav1d_picture_unref(NonNull::new(&mut self.0)) };
    }
}

impl Picture {
    pub fn size(&self) -> (usize, usize) {
        (self.0.p.w.max(0) as usize, self.0.p.h.max(0) as usize)
    }

    /// Its three planes and their strides (`None`: not 8-bit 4:2:0).
    fn planes(&self) -> Option<[(&[u8], usize); 3]> {
        let p = &self.0;
        if p.p.bpc != 8 || p.p.layout != DAV1D_PIXEL_LAYOUT_I420 {
            return None;
        }
        let (_, h) = self.size();
        let plane = |i: usize, stride: isize, rows: usize| {
            let stride = usize::try_from(stride).ok()?;
            let ptr = p.data[i]?.cast::<u8>();
            // SAFETY: dav1d's plane `i` holds `rows` rows of `stride` bytes
            // (the last one at least the picture's width), alive with `self`.
            let bytes = unsafe { std::slice::from_raw_parts(ptr.as_ptr(), stride * rows) };
            Some((bytes, stride))
        };
        Some([
            plane(0, p.stride[0], h)?,
            plane(1, p.stride[1], h.div_ceil(2))?,
            plane(2, p.stride[1], h.div_ceil(2))?,
        ])
    }

    /// RGBA at `w` x `h` (at most the picture's size; each pixel the average
    /// of the source pixels it covers, so text stays crisp when smaller).
    pub fn rgba(&self, w: usize, h: usize) -> Option<Vec<u8>> {
        let [(y, ys), (u, cs), (v, _)] = self.planes()?;
        let (sw, sh) = self.size();
        let (w, h) = (w.clamp(1, sw), h.clamp(1, sh));
        let spans = |n: usize, of: usize| -> Vec<(usize, usize)> {
            (0..n)
                .map(|i| (i * of / n, ((i + 1) * of / n).max(i * of / n + 1)))
                .collect()
        };
        let (xs, rows) = (spans(w, sw), spans(h, sh));
        let cw = sw.div_ceil(2);
        // each output row: the source rows it covers summed per column, then
        // each pixel sums its columns (one add per source byte)
        let (mut ycol, mut ucol, mut vcol) = (vec![0u32; sw], vec![0u32; cw], vec![0u32; cw]);
        let add = |sum: &mut [u32], plane: &[u8], stride: usize, rows: std::ops::Range<usize>| {
            sum.fill(0);
            for row in rows {
                let line = &plane[row * stride..row * stride + sum.len()];
                sum.iter_mut()
                    .zip(line)
                    .for_each(|(s, &b)| *s += u32::from(b));
            }
        };
        let mut out = Vec::with_capacity(w * h * 4);
        for &(y0, y1) in &rows {
            add(&mut ycol, y, ys, y0..y1);
            let (c0, c1) = (y0 / 2, y1.div_ceil(2));
            add(&mut ucol, u, cs, c0..c1);
            add(&mut vcol, v, cs, c0..c1);
            for &(x0, x1) in &xs {
                let n = ((y1 - y0) * (x1 - x0)) as f32;
                let (cx0, cx1) = (x0 / 2, x1.div_ceil(2));
                let m = ((c1 - c0) * (cx1 - cx0)) as f32;
                let sum = |s: &[u32]| s.iter().sum::<u32>() as f32;
                out.extend(bt709(
                    sum(&ycol[x0..x1]) / n,
                    sum(&ucol[cx0..cx1]) / m,
                    sum(&vcol[cx0..cx1]) / m,
                ));
            }
        }
        Some(out)
    }
}

/// BT.709 limited range to RGBA.
fn bt709(y: f32, u: f32, v: f32) -> [u8; 4] {
    let (y, u, v) = ((y - 16.0) * 1.164_384, u - 128.0, v - 128.0);
    let c = |x: f32| x.round().clamp(0.0, 255.0) as u8;
    [
        c(y + 1.792_741 * v),
        c(y - 0.213_249 * u - 0.532_909 * v),
        c(y + 2.112_402 * u),
        255,
    ]
}
