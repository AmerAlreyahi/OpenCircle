//! A click-through, always-on-top window showing one picture with per-pixel
//! alpha (Windows: `UpdateLayeredWindow`), for what our CPU renderer can't
//! draw with partial alpha (Live translate's edge glow over a game): drawn
//! once, faded in on a thread of its own, faded out and closed when dropped.
//! It costs nothing while it shows (the system keeps its picture). Captures
//! show it: Windows refuses `WDA_EXCLUDEFROMCAPTURE` for such a window.
//! Part of the UI (a window; `app::arch_tests`).

use iced::window::raw_window_handle::RawWindowHandle;

/// The window (dropping it fades it out and closes it).
#[derive(Debug)]
pub struct AlphaWindow {
    #[cfg(windows)]
    hwnd: isize,
}

impl Drop for AlphaWindow {
    fn drop(&mut self) {
        #[cfg(windows)]
        win::close(self.hwnd);
    }
}

/// The window over `at` (x, y, w, h, physical desktop px) showing `strips`
/// (rectangles in window px, RGBA straight alpha), just under the window
/// `under_of`; `None` where there is no such window (only Windows has one;
/// elsewhere a window of ours blends by itself: X11 with a compositor) or it
/// couldn't be made (logged).
pub fn show(
    under_of: RawWindowHandle,
    at: (i32, i32, u32, u32),
    strips: Vec<([u32; 4], Vec<u8>)>,
) -> Option<AlphaWindow> {
    match under_of {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => {
            win::open(Some(h.hwnd.get()), at, strips).map(|hwnd| AlphaWindow { hwnd })
        }
        _ => {
            let _ = (at, strips);
            None
        }
    }
}

/// `show`, but over every window (no window to sit under): a notice pill
/// (W72 prototype, `OPENCIRCLE_TEST_CRISP_PILL`). `None` off Windows.
pub fn show_top(at: (i32, i32, u32, u32), strips: Vec<([u32; 4], Vec<u8>)>) -> Option<AlphaWindow> {
    #[cfg(windows)]
    return win::open(None, at, strips).map(|hwnd| AlphaWindow { hwnd });
    #[cfg(not(windows))]
    {
        let _ = (at, strips);
        None
    }
}

/// `strips` (monitor px, RGBA straight alpha) into a `w`-wide BGRA picture,
/// premultiplied (what `UpdateLayeredWindow` takes).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(super) fn premultiplied_bgra(px: &mut [u8], w: u32, strips: &[([u32; 4], Vec<u8>)]) {
    for ([x0, y0, sw, _], rgba) in strips {
        for (i, p) in rgba.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (x0 + i as u32 % sw, y0 + i as u32 / sw);
            let j = ((y * w + x) * 4) as usize;
            let a = u32::from(p[3]);
            let pre = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
            if let Some(out) = px.get_mut(j..j + 4) {
                out.copy_from_slice(&[pre(p[2]), pre(p[1]), pre(p[0]), p[3]]);
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    };

    /// The message closing the window (fade out, destroy).
    const CLOSE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

    /// `super::open`: the window on a thread of its own (it waits in its
    /// message loop for `close`); its handle once it is drawn.
    pub fn open(
        layer: Option<isize>,
        at: (i32, i32, u32, u32),
        strips: Vec<([u32; 4], Vec<u8>)>,
    ) -> Option<isize> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || match draw(layer, at, &strips) {
            Ok(hwnd) => {
                drop(strips);
                let _ = tx.send(Some(hwnd.0 as isize));
                run(hwnd);
            }
            Err(e) => {
                crate::log!("[opencircle] alpha window: {e}");
                let _ = tx.send(None);
            }
        });
        rx.recv_timeout(std::time::Duration::from_secs(3))
            .ok()
            .flatten()
    }

    pub fn close(hwnd: isize) {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        // SAFETY: posting to a window of this process (its own thread destroys it).
        let _ = unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), CLOSE, WPARAM(0), LPARAM(0)) };
    }

    /// The window, drawn once (still invisible: constant alpha 0), just under `under_of`.
    fn draw(
        under_of: Option<isize>,
        (x, y, w, h): (i32, i32, u32, u32),
        strips: &[([u32; 4], Vec<u8>)],
    ) -> Result<HWND, String> {
        use windows::Win32::Foundation::{POINT, SIZE};
        use windows::Win32::Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
        };
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SW_SHOWNOACTIVATE,
            ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSW, WS_POPUP,
        };
        use windows::core::w;
        static CLASS: std::sync::Once = std::sync::Once::new();
        unsafe extern "system" fn proc(
            hwnd: HWND,
            msg: u32,
            w: windows::Win32::Foundation::WPARAM,
            l: windows::Win32::Foundation::LPARAM,
        ) -> windows::Win32::Foundation::LRESULT {
            // SAFETY: the system's default handling of this window's messages.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        let os = |e: windows::core::Error| e.to_string();
        // SAFETY: a window of this thread; its picture is made, copied into it by
        // UpdateLayeredWindow (the system keeps its own copy) and freed here.
        unsafe {
            let instance = GetModuleHandleW(None).map_err(os)?;
            CLASS.call_once(|| {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(proc),
                    hInstance: instance.into(),
                    lpszClassName: w!("OpenCircleAlpha"),
                    ..Default::default()
                };
                RegisterClassW(&class);
            });
            let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
            let hwnd = CreateWindowExW(
                ex | WS_EX_TOOLWINDOW,
                w!("OpenCircleAlpha"),
                w!("OpenCircle"),
                WS_POPUP,
                x,
                y,
                w as i32,
                h as i32,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(os)?;
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let drawn = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(bitmap) if !bits.is_null() => {
                    let px = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
                    px.fill(0);
                    super::premultiplied_bgra(px, w, strips);
                    let old = SelectObject(dc, bitmap.into());
                    let size = SIZE {
                        cx: w as i32,
                        cy: h as i32,
                    };
                    let done = UpdateLayeredWindow(
                        hwnd,
                        Some(screen),
                        Some(&POINT { x, y }),
                        Some(&size),
                        Some(dc),
                        Some(&POINT::default()),
                        COLORREF(0),
                        Some(&blend(0)),
                        ULW_ALPHA,
                    );
                    SelectObject(dc, old);
                    let _ = DeleteObject(bitmap.into());
                    done.map_err(os)
                }
                Ok(_) => Err("no bitmap".into()),
                Err(e) => Err(os(e)),
            };
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            if let Err(e) = drawn {
                let _ = DestroyWindow(hwnd);
                return Err(e);
            }
            // just under `under_of`, topmost with it (else topmost over all)
            if let Some(under_of) = under_of {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND(under_of as *mut _)),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            crate::shared::platform::capture::window_rect::no_pick(hwnd.0 as isize);
            // W72: whether it can stay out of captures (W12 found it can't)
            let seen =
                crate::shared::platform::sys::test_hook(crate::shared::platform::sys::CAPTURABLE);
            if under_of.is_none() && seen.is_none() {
                use windows::Win32::UI::WindowsAndMessaging::{
                    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
                };
                let out = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
                crate::log!(
                    "[opencircle] crisp pill: exclude from capture {:?}",
                    out.is_ok()
                );
            }
            Ok(hwnd)
        }
    }

    fn blend(alpha: u8) -> windows::Win32::Graphics::Gdi::BLENDFUNCTION {
        use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};
        BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        }
    }

    /// The fade in, the wait for `CLOSE`, the fade out, the end.
    fn run(hwnd: HWND) {
        use crate::shared::kit::motion::{FRAME, Shown, WIDGET};
        use std::time::Instant;
        use windows::Win32::UI::WindowsAndMessaging::{
            DestroyWindow, DispatchMessageW, GetMessageW, MSG, ULW_ALPHA, UpdateLayeredWindow,
        };
        let fade = |shown: Shown| {
            let mut next = Instant::now();
            loop {
                let now = Instant::now();
                let a = (shown.at(now) * 255.0).round() as u8;
                // SAFETY: this thread's window; only its constant alpha changes
                // (no source: the picture drawn once stays).
                let _ = unsafe {
                    UpdateLayeredWindow(
                        hwnd,
                        None,
                        None,
                        None,
                        None,
                        None,
                        COLORREF(0),
                        Some(&blend(a)),
                        ULW_ALPHA,
                    )
                };
                if !shown.moving(now) {
                    break;
                }
                next += FRAME;
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        };
        fade(Shown::opening(WIDGET, Instant::now()));
        let mut msg = MSG::default();
        // SAFETY: this thread's message loop; its window is destroyed on it.
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == CLOSE {
                    break;
                }
                DispatchMessageW(&msg);
            }
        }
        fade(Shown::closing(WIDGET, Instant::now()));
        // SAFETY: as above.
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The glow window's picture: each strip at its place, BGRA, colours
    /// premultiplied by their alpha, clear elsewhere.
    #[test]
    fn the_glow_window_takes_its_strips_premultiplied() {
        let mut px = vec![0u8; 4 * 3 * 4];
        let strips = vec![([1, 1, 2, 1], vec![200, 100, 50, 128, 10, 20, 30, 255])];
        premultiplied_bgra(&mut px, 4, &strips);
        let at = |x: usize, y: usize| px[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].to_vec();
        assert_eq!(at(1, 1), [25, 50, 100, 128]);
        assert_eq!(at(2, 1), [30, 20, 10, 255]);
        assert!([at(0, 0), at(3, 1), at(1, 2)].iter().all(|p| p == &[0; 4]));
    }
}
