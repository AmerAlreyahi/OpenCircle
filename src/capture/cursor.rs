//! "Capture cursor" (`snip.cursor`): the mouse pointer drawn into a captured
//! frame. Windows: `GetCursorInfo` + `DrawIconEx` over the frame's pixels (every
//! cursor kind, monochrome and animated ones too). X11: the XFixes cursor image.
//! Native Wayland: not drawn (the portal decides).
use super::Frame;

/// Draws the pointer into `frame` if it is on it; a failure only logs.
pub fn draw(frame: &mut Frame) {
    if let Err(e) = platform(frame) {
        eprintln!("[opencircle] cursor not captured: {e}");
    }
}

/// The part of a `w` x `h` image placed at `at` (frame px) that lies on a
/// `fw` x `fh` frame: image px `[x0, y0, x1, y1]`, ends exclusive.
fn overlap(fw: u32, fh: u32, at: (i32, i32), w: u32, h: u32) -> Option<[u32; 4]> {
    let clip = |a: i32, len: u32, max: u32| {
        let start = (-a).clamp(0, len as i32) as u32;
        let end = (i64::from(max) - i64::from(a)).clamp(0, i64::from(len)) as u32;
        (start, end)
    };
    let (x0, x1) = clip(at.0, w, fw);
    let (y0, y1) = clip(at.1, h, fh);
    (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
}

/// Blends a premultiplied ARGB cursor image (XFixes' format) onto the frame at `at`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn blend(frame: &mut Frame, argb: &[u32], w: u32, h: u32, at: (i32, i32)) {
    let Some([x0, y0, x1, y1]) = overlap(frame.width, frame.height, at, w, h) else {
        return;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let Some(&p) = argb.get((y * w + x) as usize) else {
                return;
            };
            let [a, r, g, b] = p.to_be_bytes().map(u32::from);
            let fx = (at.0 + x as i32) as usize;
            let fy = (at.1 + y as i32) as usize;
            let i = (fy * frame.width as usize + fx) * 4;
            let Some(dst) = frame.rgba.get_mut(i..i + 3) else {
                return;
            };
            for (d, s) in dst.iter_mut().zip([r, g, b]) {
                *d = (s + u32::from(*d) * (255 - a) / 255).min(255) as u8;
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn platform(frame: &mut Frame) -> Result<(), String> {
    use xcb::xfixes;
    if super::wayland_session() || std::env::var_os("DISPLAY").is_none() {
        return Ok(());
    }
    let err = |e: xcb::Error| format!("{e:?}");
    let (conn, _) = xcb::Connection::connect_with_extensions(None, &[xcb::Extension::XFixes], &[])
        .map_err(|e| e.to_string())?;
    // XFixes answers nothing before its version is asked.
    conn.wait_for_reply(conn.send_request(&xfixes::QueryVersion {
        client_major_version: 4,
        client_minor_version: 0,
    }))
    .map_err(err)?;
    let image = conn
        .wait_for_reply(conn.send_request(&xfixes::GetCursorImage {}))
        .map_err(err)?;
    let at = (
        i32::from(image.x()) - i32::from(image.xhot()) - frame.x,
        i32::from(image.y()) - i32::from(image.yhot()) - frame.y,
    );
    let (w, h) = (u32::from(image.width()), u32::from(image.height()));
    blend(frame, image.cursor_image(), w, h, at);
    Ok(())
}

#[cfg(windows)]
fn platform(frame: &mut Frame) -> Result<(), String> {
    use std::ffi::c_void;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetObjectW, HGDIOBJ, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, HICON,
        ICONINFO,
    };

    let os = |e: windows::core::Error| e.to_string();
    // SAFETY: plain Win32 queries and GDI drawing on objects created and freed
    // here; `bits` is the DIB section's own w*h*4 buffer while `dib` lives.
    unsafe {
        let mut info = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..CURSORINFO::default()
        };
        GetCursorInfo(&mut info).map_err(os)?;
        if info.flags.0 & CURSOR_SHOWING.0 == 0 || info.hCursor.is_invalid() {
            return Ok(()); // hidden (a game, typing)
        }
        let icon = HICON(info.hCursor.0);
        let mut icon_info = ICONINFO::default();
        GetIconInfo(icon, &mut icon_info).map_err(os)?;
        let mut mask = BITMAP::default();
        GetObjectW(
            HGDIOBJ(icon_info.hbmMask.0),
            size_of::<BITMAP>() as i32,
            Some((&raw mut mask).cast::<c_void>()),
        );
        // A monochrome cursor's mask holds the AND and the XOR halves.
        let colour = !icon_info.hbmColor.is_invalid();
        let (w, h) = (
            mask.bmWidth.max(0) as u32,
            (if colour {
                mask.bmHeight
            } else {
                mask.bmHeight / 2
            })
            .max(0) as u32,
        );
        let _ = DeleteObject(HGDIOBJ(icon_info.hbmMask.0));
        if colour {
            let _ = DeleteObject(HGDIOBJ(icon_info.hbmColor.0));
        }
        let at = (
            info.ptScreenPos.x - icon_info.xHotspot as i32 - frame.x,
            info.ptScreenPos.y - icon_info.yHotspot as i32 - frame.y,
        );
        let Some([x0, y0, x1, y1]) = overlap(frame.width, frame.height, at, w, h) else {
            return Ok(());
        };
        let dc = CreateCompatibleDC(None);
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..BITMAPINFOHEADER::default()
        };
        let bitmap = BITMAPINFO {
            bmiHeader: header,
            ..BITMAPINFO::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(dc), &bitmap, DIB_RGB_COLORS, &mut bits, None, 0);
        let dib = match dib {
            Ok(dib) if !bits.is_null() => dib,
            Ok(_) | Err(_) => {
                let _ = DeleteDC(dc);
                return Err("no DIB for the cursor".into());
            }
        };
        let old = SelectObject(dc, HGDIOBJ(dib.0));
        let px = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
        // The frame under the cursor in, as BGRA; the cursor drawn on it; back out.
        let index = |x: u32, y: u32| {
            let fx = (at.0 + x as i32) as usize;
            let fy = (at.1 + y as i32) as usize;
            (
                (fy * frame.width as usize + fx) * 4,
                ((y * w + x) * 4) as usize,
            )
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let (f, c) = index(x, y);
                let [r, g, b, _] = [0, 1, 2, 3].map(|k| frame.rgba[f + k]);
                px[c..c + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
        let drawn = DrawIconEx(dc, 0, 0, icon, w as i32, h as i32, 0, None, DI_NORMAL);
        let _ = GdiFlush();
        if drawn.is_ok() {
            for y in y0..y1 {
                for x in x0..x1 {
                    let (f, c) = index(x, y);
                    let [b, g, r, _] = [0, 1, 2, 3].map(|k| px[c + k]);
                    frame.rgba[f..f + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = DeleteDC(dc);
        drawn.map_err(os)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform(_frame: &mut Frame) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32) -> Frame {
        Frame {
            rgba: vec![100; (w * h * 4) as usize],
            width: w,
            height: h,
            x: 0,
            y: 0,
            scale: 1.0,
        }
    }

    /// `env -u WAYLAND_DISPLAY xvfb-run -a cargo test x11_pointer -- --ignored`
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs an X display (xvfb-run)"]
    fn x11_pointer_is_drawn_into_a_real_capture() {
        let mut f = crate::capture::monitor_under_cursor().unwrap();
        let before = f.rgba.clone();
        draw(&mut f);
        let changed = (f.rgba.chunks(4).zip(before.chunks(4))).filter(|(a, b)| a != b);
        assert!(changed.count() > 20, "no pointer drawn");
    }

    #[test]
    fn the_cursor_is_clipped_to_the_frame() {
        assert_eq!(overlap(10, 10, (2, 3), 4, 4), Some([0, 0, 4, 4]));
        assert_eq!(
            overlap(10, 10, (-2, 8), 4, 4),
            Some([2, 0, 4, 2]),
            "top-left off"
        );
        assert_eq!(overlap(10, 10, (9, 9), 4, 4), Some([0, 0, 1, 1]));
        assert_eq!(overlap(10, 10, (10, 0), 4, 4), None, "beside the frame");
        assert_eq!(overlap(10, 10, (-4, 0), 4, 4), None);
    }

    #[test]
    fn premultiplied_pixels_blend_over_the_frame() {
        let mut f = frame(3, 1);
        // Opaque red, half-transparent premultiplied white, fully transparent.
        let argb = [0xffff_0000, 0x8080_8080, 0x0000_0000];
        blend(&mut f, &argb, 3, 1, (0, 0));
        assert_eq!(&f.rgba[0..4], [255, 0, 0, 100]);
        // 128 + 100 * 127 / 255 = 177
        assert_eq!(&f.rgba[4..8], [177, 177, 177, 100]);
        assert_eq!(
            &f.rgba[8..12],
            [100, 100, 100, 100],
            "transparent: unchanged"
        );
        // Half off the left edge: only the pixel on the frame changes.
        let mut f = frame(2, 1);
        blend(&mut f, &argb, 3, 1, (-2, 0));
        assert_eq!(&f.rgba[..4], [100, 100, 100, 100]);
        assert_eq!(&f.rgba[4..], [100; 4]);
        blend(&mut f, &argb, 3, 1, (1, 0));
        assert_eq!(&f.rgba[4..8], [255, 0, 0, 100]);
    }
}
