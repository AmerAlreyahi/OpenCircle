//! Cuts a floating window to the rounded rectangle of its content, so no pixel
//! outside it exists (no white or black box behind a pill). Our CPU renderer
//! (softbuffer) has no per-pixel alpha on any platform, so the window itself is
//! cut: Windows `SetWindowRgn`, X11 the SHAPE extension (with or without a
//! compositor). Wayland has no way to cut a window, so there floating contents
//! are drawn square (`radius`) and fill the window: no stray pixel either.
//! Called by `windows.rs` after a floating window opens and on every resize.
//! Also `no_caption`: on Windows no window of ours ever gets the system's
//! classic title bar painted over its top (#126).
use iced::window::raw_window_handle::RawWindowHandle;

/// Whether a floating window can be cut to a rounded shape here (not on Wayland).
pub fn available() -> bool {
    super::can_hide_windows()
}

/// The corner radius a floating window's content should draw: `r` where the window
/// is cut to match, else 0 (square: a rounded corner would leave stray pixels).
pub fn radius(r: f32) -> f32 {
    if available() { r } else { 0.0 }
}

/// The corner radius a `w` x `h` window can take: `max`, but never more than half
/// its shorter side (so a pill's huge radius becomes a full round end).
pub fn corner(max: f32, w: f32, h: f32) -> f32 {
    max.min(w.min(h) / 2.0).max(0.0)
}

/// A `w` x `h` rounded rectangle of corner radius `r` (pixels) as `(x, y, w, h)`
/// bands: one rectangle per corner row, one for the straight middle.
// ponytail: hard (aliased) edge; X11 without a compositor has no better.
#[cfg(any(target_os = "linux", test))]
pub fn round_rect(w: u32, h: u32, r: u32) -> Vec<(i32, i32, u32, u32)> {
    let r = r.min(w / 2).min(h / 2);
    let mut bands = Vec::new();
    for row in 0..r {
        // Distance from the circle's centre row to this row's middle.
        let dy = r as f32 - row as f32 - 0.5;
        let inset = (r as f32 - (r as f32 * r as f32 - dy * dy).max(0.0).sqrt()).ceil() as u32;
        let inset = inset.min(w / 2);
        let width = w - 2 * inset;
        bands.push((inset as i32, row as i32, width, 1));
        bands.push((inset as i32, (h - 1 - row) as i32, width, 1));
    }
    if h > 2 * r {
        bands.push((0, r as i32, w, h - 2 * r));
    }
    bands
}

/// Removes the shape: the window is whole again (the overlay went full-screen).
pub fn clear(handle: RawWindowHandle) -> Result<(), String> {
    match handle {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => win::clear(h.hwnd.get()),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(h) => x11::clear(h.window as u32),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(h) => x11::clear(h.window.get()),
        _ => Ok(()),
    }
}

/// Shapes the window behind `handle` to its rounded rectangle (`max_radius` in
/// logical px, see `corner`). `Ok` also when this window system needs nothing.
pub fn apply(handle: RawWindowHandle, max_radius: f32) -> Result<(), String> {
    match handle {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => win::apply(h.hwnd.get(), max_radius),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(h) => x11::apply(h.window as u32, max_radius),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(h) => x11::apply(h.window.get(), max_radius),
        _ => {
            let _ = max_radius;
            Ok(())
        }
    }
}

/// Windows: the system never paints its classic title bar over the window behind
/// `handle` (`win::no_caption`). Called once by `windows.rs` for every window it
/// opens. Elsewhere nothing.
pub fn no_caption(handle: RawWindowHandle) {
    #[cfg(windows)]
    if let RawWindowHandle::Win32(h) = handle {
        win::no_caption(h.hwnd.get());
    }
    #[cfg(not(windows))]
    let _ = handle;
}

#[cfg(windows)]
mod win {
    use super::corner;
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, CreateRoundRectRgn, SetWindowRgn};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWLP_WNDPROC, GetPropW, GetWindowLongPtrW, GetWindowRect, RemovePropW,
        SetPropW, SetWindowLongPtrW, WM_NCACTIVATE, WM_NCDESTROY, WNDPROC,
    };
    use windows::core::{PCWSTR, w};

    /// The window's own procedure (winit's), kept while ours sits in front of it.
    const NEXT: PCWSTR = w!("OpenCircle.next_wndproc");

    /// winit keeps an undecorated window's style overlapped (it hides the caption
    /// with its WM_NCCALCSIZE) and hands WM_NCACTIVATE on to DefWindowProc. Where
    /// DWM does not draw the frame (the full-screen overlay, windows cut by a
    /// region) DefWindowProc then paints the classic caption, "OpenCircle", over
    /// the top 22 px of our content at every (de)activation (the Start menu, a
    /// click on another app), and nothing paints over it: only damage is redrawn.
    /// lParam -1 tells DefWindowProc not to repaint the non-client area; winit
    /// still sees every activation.
    pub fn no_caption(hwnd: isize) {
        let hwnd = HWND(hwnd as *mut _);
        let ours = wndproc as *const () as isize;
        // SAFETY: `hwnd` is a live window of this thread. The next procedure is
        // stored before ours is installed, so `wndproc` always finds it.
        unsafe {
            let next = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
            if next == ours || SetPropW(hwnd, NEXT, Some(HANDLE(next as *mut _))).is_err() {
                return;
            }
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours);
        }
    }

    unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: the property holds the procedure `no_caption` replaced, a valid
        // WNDPROC of this window (never null: it is set before ours goes in).
        unsafe {
            let next: WNDPROC = std::mem::transmute(GetPropW(h, NEXT).0);
            if msg == WM_NCDESTROY {
                let _ = RemovePropW(h, NEXT);
            }
            let l = if msg == WM_NCACTIVATE { LPARAM(-1) } else { l };
            CallWindowProcW(next, h, msg, w, l)
        }
    }

    pub fn clear(hwnd: isize) -> Result<(), String> {
        // SAFETY: `hwnd` is a live window of this process; no region = the whole window.
        unsafe { SetWindowRgn(HWND(hwnd as *mut _), None, true) };
        Ok(())
    }

    pub fn apply(hwnd: isize, max_radius: f32) -> Result<(), String> {
        let hwnd = HWND(hwnd as *mut _);
        let mut rect = RECT::default();
        // SAFETY: `hwnd` is a live window of this process (the UI's raw handle). The
        // region is handed to the system, which owns it after a successful call.
        unsafe {
            GetWindowRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            let r = (corner(max_radius * scale, w as f32, h as f32)).round() as i32;
            let region = if r <= 0 {
                CreateRectRgn(0, 0, w, h)
            } else {
                // The ellipse is 2r wide; the +1 is GDI's exclusive far edge.
                CreateRoundRectRgn(0, 0, w + 1, h + 1, 2 * r, 2 * r)
            };
            if SetWindowRgn(hwnd, Some(region), true) == 0 {
                return Err("SetWindowRgn failed".into());
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::{corner, round_rect};
    use xcb::{XidNew, shape, x};

    pub fn clear(window: u32) -> Result<(), String> {
        let (conn, _) =
            xcb::Connection::connect_with_extensions(None, &[xcb::Extension::Shape], &[])
                .map_err(|e| format!("X11: {e:?}"))?;
        conn.send_and_check_request(&shape::Mask {
            operation: shape::So::Set,
            destination_kind: shape::Sk::Bounding,
            destination_window: x::Window::new(window),
            x_offset: 0,
            y_offset: 0,
            source_bitmap: xcb::Xid::none(),
        })
        .map_err(|e| format!("X11: {e:?}"))
    }

    pub fn apply(window: u32, max_radius: f32) -> Result<(), String> {
        let os = |e: &dyn std::fmt::Debug| format!("X11: {e:?}");
        let (conn, _) =
            xcb::Connection::connect_with_extensions(None, &[xcb::Extension::Shape], &[])
                .map_err(|e| os(&e))?;
        let window = x::Window::new(window);
        let geometry = conn
            .wait_for_reply(conn.send_request(&x::GetGeometry {
                drawable: x::Drawable::Window(window),
            }))
            .map_err(|e| os(&e))?;
        let (w, h) = (geometry.width(), geometry.height());
        let r = corner(max_radius, w as f32, h as f32).round() as u32;
        let rects: Vec<x::Rectangle> = round_rect(w as u32, h as u32, r)
            .into_iter()
            .map(|(x, y, w, h)| x::Rectangle {
                x: x as i16,
                y: y as i16,
                width: w as u16,
                height: h as u16,
            })
            .collect();
        conn.send_and_check_request(&shape::Rectangles {
            operation: shape::So::Set,
            destination_kind: shape::Sk::Bounding,
            ordering: x::ClipOrdering::Unsorted,
            destination_window: window,
            x_offset: 0,
            y_offset: 0,
            rectangles: &rects,
        })
        .map_err(|e| os(&e))
    }

    #[cfg(test)]
    pub fn bounding(window: u32) -> Vec<(i32, i32, u32, u32)> {
        let (conn, _) =
            xcb::Connection::connect_with_extensions(None, &[xcb::Extension::Shape], &[]).unwrap();
        let reply = conn
            .wait_for_reply(conn.send_request(&shape::GetRectangles {
                window: x::Window::new(window),
                source_kind: shape::Sk::Bounding,
            }))
            .unwrap();
        reply
            .rectangles()
            .iter()
            .map(|r| (r.x as i32, r.y as i32, r.width as u32, r.height as u32))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(bands: &[(i32, i32, u32, u32)]) -> u32 {
        bands.iter().map(|b| b.2 * b.3).sum()
    }

    /// #126: DefWindowProc must get WM_NCACTIVATE with lParam -1 (no caption
    /// painted); every other message reaches the window's own procedure unchanged,
    /// and installing twice changes nothing.
    #[cfg(windows)]
    #[test]
    fn activation_never_paints_the_caption() {
        use std::sync::atomic::{AtomicIsize, Ordering};
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GWLP_WNDPROC, HWND_MESSAGE, SendMessageW,
            SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_NCACTIVATE, WM_USER,
        };
        use windows::core::w;

        static SEEN: [AtomicIsize; 2] = [AtomicIsize::new(0), AtomicIsize::new(0)];
        unsafe extern "system" fn own(_: HWND, msg: u32, _: WPARAM, l: LPARAM) -> LRESULT {
            match msg {
                WM_NCACTIVATE => SEEN[0].store(l.0, Ordering::SeqCst),
                WM_USER => SEEN[1].store(l.0, Ordering::SeqCst),
                _ => {}
            }
            LRESULT(1)
        }
        // SAFETY: a message-only window of this thread, destroyed at the end.
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("caption test"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .unwrap();
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, own as *const () as isize);
            no_caption(RawWindowHandle::Win32(
                iced::window::raw_window_handle::Win32WindowHandle::new(
                    std::num::NonZeroIsize::new(hwnd.0 as isize).unwrap(),
                ),
            ));
            win::no_caption(hwnd.0 as isize); // twice: no loop, no second hop
            SendMessageW(hwnd, WM_NCACTIVATE, Some(WPARAM(0)), Some(LPARAM(0)));
            SendMessageW(hwnd, WM_USER, Some(WPARAM(0)), Some(LPARAM(7)));
            let _ = DestroyWindow(hwnd);
        }
        assert_eq!(
            SEEN[0].load(Ordering::SeqCst),
            -1,
            "caption repaint not skipped"
        );
        assert_eq!(
            SEEN[1].load(Ordering::SeqCst),
            7,
            "other messages unchanged"
        );
    }

    #[test]
    fn corner_is_capped_by_the_shorter_side() {
        assert_eq!(
            corner(999.0, 300.0, 36.0),
            18.0,
            "a pill ends in a half circle"
        );
        assert_eq!(corner(20.0, 300.0, 200.0), 20.0);
        assert_eq!(corner(20.0, 28.0, 28.0), 14.0, "a dot is a circle");
        assert_eq!(corner(-1.0, 28.0, 28.0), 0.0);
    }

    #[test]
    fn a_rounded_rectangle_has_no_pixel_in_its_corners() {
        let (w, h, r) = (200, 80, 20);
        let bands = round_rect(w, h, r);
        // The first and last pixel rows are inset; the middle is the full width.
        let top = bands.iter().find(|b| b.1 == 0).unwrap();
        assert!(top.0 > 0 && top.2 < w, "{top:?}");
        assert!(bands.iter().any(|b| *b == (0, r as i32, w, h - 2 * r)));
        // Less than the full rectangle, more than the rectangle minus four squares.
        let full = w * h;
        assert!(
            area(&bands) < full && area(&bands) > full - 4 * r * r,
            "{}",
            area(&bands)
        );
        // Symmetric top and bottom.
        for b in bands.iter().filter(|b| b.1 < r as i32) {
            let mirror = (b.0, (h as i32) - 1 - b.1, b.2, b.3);
            assert!(bands.contains(&mirror), "{b:?}");
        }
        // No rounding: one rectangle.
        assert_eq!(round_rect(50, 20, 0), vec![(0, 0, 50, 20)]);
        // A full pill: the radius is capped at half the height.
        let pill = round_rect(100, 20, 999);
        assert!(pill.iter().all(|b| b.0 + b.2 as i32 <= 100));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_window_gets_the_shape() {
        use xcb::{Xid, x};
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("no X11 display: skipped");
            return;
        }
        let (conn, screen) = xcb::Connection::connect(None).unwrap();
        let root = conn.get_setup().roots().nth(screen as usize).unwrap();
        let window: x::Window = conn.generate_id();
        conn.send_and_check_request(&x::CreateWindow {
            depth: x::COPY_FROM_PARENT as u8,
            wid: window,
            parent: root.root(),
            x: 0,
            y: 0,
            width: 120,
            height: 40,
            border_width: 0,
            class: x::WindowClass::InputOutput,
            visual: root.root_visual(),
            value_list: &[],
        })
        .unwrap();
        x11::apply(window.resource_id(), 999.0).unwrap();
        let got = x11::bounding(window.resource_id());
        let want = round_rect(120, 40, 20);
        assert_eq!(area(&got), area(&want), "{got:?}");
        // A corner pixel is outside every band, the centre inside one.
        let inside = |px: i32, py: i32| {
            got.iter()
                .any(|b| px >= b.0 && px < b.0 + b.2 as i32 && py >= b.1 && py < b.1 + b.3 as i32)
        };
        assert!(!inside(0, 0) && !inside(119, 39) && inside(60, 20));
        // Cleared: the whole window again.
        x11::clear(window.resource_id()).unwrap();
        assert_eq!(area(&x11::bounding(window.resource_id())), 120 * 40);
    }
}
