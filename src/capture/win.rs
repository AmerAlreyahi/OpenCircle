//! Windows capture: the screen as shown, through GDI (windows that keep out of
//! captures, ours among them, are not in it); monitors listed by xcap.
//! Positions and sizes are physical desktop px (the process is per-monitor DPI
//! aware, winit's default).
use xcap::Monitor;

use super::Frame;

pub fn monitors() -> Result<Vec<super::Monitor>, String> {
    let err = |e: xcap::XCapError| e.to_string();
    (Monitor::all().map_err(err)?.iter())
        .map(|m| {
            Ok(super::Monitor {
                rect: (
                    m.x().map_err(err)?,
                    m.y().map_err(err)?,
                    m.width().map_err(err)?,
                    m.height().map_err(err)?,
                ),
                scale: m.scale_factor().unwrap_or(1.0),
                primary: m.is_primary().unwrap_or(false),
            })
        })
        .collect()
}

pub fn grab(m: &super::Monitor) -> Result<Frame, String> {
    let (x, y, width, height) = m.rect;
    Ok(Frame {
        rgba: grab_rect(m.rect)?,
        width,
        height,
        x,
        y,
        scale: m.scale,
    })
}

/// `w` x `h` px of the desktop at (`x`, `y`), RGBA. The screen is copied
/// into a DIB section (its pixels are ours to read: no GetDIBits copy) and
/// turned into RGBA in one pass, on up to 4 cores for a screen. xcap's grab
/// (a zeroed buffer, GetDIBits into it, then a swap per pixel on one core,
/// after listing the monitors again) cost ~20 ms more at 3440x1440 on the VM.
/// The copy itself (DWM hands GDI the composed screen) is most of a freeze.
pub fn grab_rect((x, y, w, h): (i32, i32, u32, u32)) -> Result<Vec<u8>, String> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SRCCOPY, SelectObject,
    };
    if w == 0 || h == 0 {
        return Err("nothing to grab".into());
    }
    let (wi, hi) = (w as i32, h as i32);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: wi,
            biHeight: -hi, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: plain GDI calls on handles made here and released before
    // returning; the DIB section holds exactly `h` top-down rows of `w`
    // 32-bit pixels, read only while it is alive, after GdiFlush.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(dib) => {
                let old = SelectObject(mem, dib.into());
                let copied = BitBlt(mem, 0, 0, wi, hi, Some(screen), x, y, SRCCOPY);
                let _ = GdiFlush();
                let result = match copied {
                    Ok(()) if !bits.is_null() => {
                        let n = w as usize * h as usize * 4;
                        Ok(to_rgba(std::slice::from_raw_parts(bits as *const u8, n)))
                    }
                    Ok(()) => Err("no pixels for the grab".into()),
                    Err(e) => Err(e.to_string()),
                };
                SelectObject(mem, old);
                let _ = DeleteObject(dib.into());
                result
            }
            Err(e) => Err(e.to_string()),
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}

/// GDI's BGRx (the fourth byte undefined) as opaque RGBA, split between up
/// to 4 threads from a megapixel.
fn to_rgba(bgrx: &[u8]) -> Vec<u8> {
    let mut rgba = vec![0u8; bgrx.len()];
    let px = bgrx.len() / 4;
    let threads = match px >= 1 << 20 {
        true => std::thread::available_parallelism().map_or(1, |n| n.get().min(4)),
        false => 1,
    };
    let swap = |from: &[u8], to: &mut [u8]| {
        for (p, q) in (from.as_chunks::<4>().0.iter()).zip(to.as_chunks_mut::<4>().0) {
            *q = [p[2], p[1], p[0], 255];
        }
    };
    if threads == 1 {
        swap(bgrx, &mut rgba);
        return rgba;
    }
    let part = px.div_ceil(threads) * 4;
    std::thread::scope(|s| {
        for (from, to) in bgrx.chunks(part).zip(rgba.chunks_mut(part)) {
            s.spawn(move || swap(from, to));
        }
    });
    rgba
}

pub fn pointer() -> Option<(i32, i32)> {
    let mut p = windows::Win32::Foundation::POINT::default();
    // SAFETY: GetCursorPos only writes into the POINT we own.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}
