//! Cuts a floating window to the rounded rectangle of its content, so no pixel
//! outside it exists (no white or black box behind a pill). Our CPU renderer
//! (softbuffer) has no per-pixel alpha on any platform, so the window itself is
//! cut: Windows `SetWindowRgn`, X11 the SHAPE extension (with or without a
//! compositor). Wayland has no way to cut a window, so there floating contents
//! are drawn square (`radius`) and fill the window: no stray pixel either.
//! Called by `platform/windows.rs` after a floating window opens and on every resize.
//! Also `opacity`: a whole window faded by the OS (Windows), for every open
//! and close (`motion`), so the CPU never redraws a fade.
//! Also `no_caption`: on Windows no window of ours ever gets the system's
//! classic title bar painted over its top (#126).
use iced::window::raw_window_handle::RawWindowHandle;

/// Whether a floating window can be cut to a rounded shape here (not on Wayland).
pub fn available() -> bool {
    super::monitor::can_hide_windows()
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

/// A window px rectangle: x, y, width, height.
pub type Px = (i32, i32, u32, u32);

/// A `w` x `h` window less `hole`, but with `keep` (bars over the hole) as
/// rectangles: the bands above, below, left and right of the hole, then `keep`.
pub fn holed(w: u32, h: u32, hole: Px, keep: &[Px]) -> Vec<Px> {
    let (wi, hi) = (w as i32, h as i32);
    let (x0, y0) = (hole.0.clamp(0, wi), hole.1.clamp(0, hi));
    let (x1, y1) = (
        (hole.0 + hole.2 as i32).clamp(x0, wi),
        (hole.1 + hole.3 as i32).clamp(y0, hi),
    );
    let bands = [
        (0, 0, wi, y0),
        (0, y1, wi, hi - y1),
        (0, y0, x0, y1 - y0),
        (x1, y0, wi - x1, y1 - y0),
    ];
    (bands.into_iter())
        .filter(|b| b.2 > 0 && b.3 > 0)
        .map(|(x, y, w, h)| (x, y, w as u32, h as u32))
        .chain(keep.iter().copied().filter(|k| k.2 > 0 && k.3 > 0))
        .collect()
}

/// Shapes the window behind `handle` to `holed`: clicks and the wheel in the
/// hole reach the window under it (Snip's long capture). `Ok` also where
/// nothing can be cut (Wayland).
pub fn hole(handle: RawWindowHandle, hole: Px, keep: &[Px]) -> Result<(), String> {
    match handle {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => win::rects(h.hwnd.get(), |w, h| holed(w, h, hole, keep)),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(h) => x11::rects(h.window as u32, |w, h| holed(w, h, hole, keep)),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(h) => x11::rects(h.window.get(), |w, h| holed(w, h, hole, keep)),
        _ => {
            let _ = (hole, keep);
            Ok(())
        }
    }
}

/// Whether a whole window can fade here (`opacity`): Windows (DWM composites
/// a layered window's alpha on the GPU, free for our CPU renderer).
// ponytail: Windows only. X11 has _NET_WM_WINDOW_OPACITY (compositor-only),
// Wayland nothing; there windows show and go at once (Linux test round).
pub fn can_fade() -> bool {
    cfg!(windows)
}

/// The window behind `handle` drawn at `alpha` (0..1) of its opacity, as a
/// whole (a fade without redrawing a pixel). Nothing where `can_fade` is false.
pub fn opacity(handle: RawWindowHandle, alpha: f32) -> Result<(), String> {
    match handle {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => win::opacity(h.hwnd.get(), alpha),
        _ => {
            let _ = alpha;
            Ok(())
        }
    }
}

/// The window behind `handle` as `fade` takes it (a plain number, for a
/// thread); `None` where windows can't fade.
pub fn fadeable(handle: RawWindowHandle) -> Option<isize> {
    match handle {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// Fades the window `fadeable` gave along `shown` (its opacity), one step per
/// screen refresh, and returns when `shown` is done; `rise` (logical px): it
/// also sits that much lower while closed (rises as it opens, sinks as it
/// closes). The fade is the window's latest from this call (an older one
/// stops; a later `clear_now` stops this one); the returned run blocks: call
/// it on a thread of its own (`windows::fade`).
pub fn fade(
    window: isize,
    shown: crate::ui::kit::motion::Shown,
    rise: f32,
) -> impl FnOnce() + Send {
    #[cfg(windows)]
    return win::fade(window, shown, rise);
    #[cfg(not(windows))]
    move || {
        let _ = (window, shown, rise);
    }
}

/// Windows: the system never paints its classic title bar over the window behind
/// `handle` (`win::no_caption`). Called once by `platform/windows.rs` for every window it
/// opens. Elsewhere nothing.
pub fn no_caption(handle: RawWindowHandle) {
    #[cfg(windows)]
    if let RawWindowHandle::Win32(h) = handle {
        win::no_caption(h.hwnd.get());
    }
    #[cfg(not(windows))]
    let _ = handle;
}

/// Windows: the window `fadeable` gave, clear at once, from the calling thread,
/// its fade (if any) stopped: nothing it draws shows until its opacity is set
/// again. The overlay's close: its last frames (the empty view, the restore from
/// full-screen) are drawn before it hides, and none of them may show. Nothing
/// where windows can't fade.
pub fn clear_now(window: isize) {
    #[cfg(windows)]
    win::clear_now(window);
    #[cfg(not(windows))]
    let _ = window;
}

/// Windows: the window behind `handle` never gets a taskbar button: a tool
/// window (`WS_EX_TOOLWINDOW`, not `WS_EX_APPWINDOW`) now and after every style
/// winit writes (`win::wndproc`). winit's `skip_taskbar` only deletes the
/// button once; the window's next show (or leaving full-screen) brings it back.
/// Needs `no_caption` first (its subclass). Elsewhere nothing.
pub fn no_taskbar(handle: RawWindowHandle) {
    #[cfg(windows)]
    if let RawWindowHandle::Win32(h) = handle {
        win::no_taskbar(h.hwnd.get());
    }
    #[cfg(not(windows))]
    let _ = handle;
}

#[cfg(windows)]
mod win {
    use super::corner;
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWL_EXSTYLE, GWL_STYLE, GWLP_WNDPROC, GetPropW, GetWindowLongPtrW,
        GetWindowRect, RemovePropW, STYLESTRUCT, SetPropW, SetWindowLongPtrW, WM_NCACTIVATE,
        WM_NCDESTROY, WM_SETICON, WM_SETTEXT, WM_STYLECHANGING, WNDPROC, WS_EX_LAYERED, WS_VISIBLE,
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
    /// still sees every activation. The other ways the system paints that caption
    /// (Windows 11 draws its themed one through them): the theme engine's
    /// WM_NCUAHDRAWCAPTION / WM_NCUAHDRAWFRAME (swallowed), and DefWindowProc's
    /// WM_SETTEXT / WM_SETICON on a visible window (run with WS_VISIBLE off for
    /// the call, so nothing is painted; the text and icon are still set).
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
                let _ = RemovePropW(h, LAYERED);
                let _ = RemovePropW(h, TOOL);
            }
            // winit rewrites the extended style whole on every show, hide or
            // full-screen change: a faded window (`opacity`) stays layered, its
            // alpha kept (else it would flash in opaque before its fade)
            if msg == WM_STYLECHANGING
                && w.0 as i32 == GWL_EXSTYLE.0
                && !GetPropW(h, LAYERED).is_invalid()
            {
                let style = &mut *(l.0 as *mut STYLESTRUCT);
                style.styleNew |= WS_EX_LAYERED.0;
            }
            // ... and a tool window stays one (no taskbar button, `no_taskbar`)
            if msg == WM_STYLECHANGING
                && w.0 as i32 == GWL_EXSTYLE.0
                && !GetPropW(h, TOOL).is_invalid()
            {
                let style = &mut *(l.0 as *mut STYLESTRUCT);
                style.styleNew = tool_style(style.styleNew);
            }
            match msg {
                WM_NCUAHDRAWCAPTION | WM_NCUAHDRAWFRAME => LRESULT(0),
                WM_NCACTIVATE => CallWindowProcW(next, h, msg, w, LPARAM(-1)),
                WM_SETTEXT | WM_SETICON => {
                    let style = GetWindowLongPtrW(h, GWL_STYLE);
                    let shown = style & WS_VISIBLE.0 as isize != 0;
                    if shown {
                        SetWindowLongPtrW(h, GWL_STYLE, style & !(WS_VISIBLE.0 as isize));
                    }
                    let result = CallWindowProcW(next, h, msg, w, l);
                    if shown {
                        let now = GetWindowLongPtrW(h, GWL_STYLE);
                        SetWindowLongPtrW(h, GWL_STYLE, now | WS_VISIBLE.0 as isize);
                    }
                    result
                }
                _ => CallWindowProcW(next, h, msg, w, l),
            }
        }
    }

    /// Undocumented: the theme engine asks the window to paint its caption / frame.
    pub(super) const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
    pub(super) const WM_NCUAHDRAWFRAME: u32 = 0x00AF;

    /// Set on a window `opacity` made layered (`wndproc` keeps it so).
    const LAYERED: PCWSTR = w!("OpenCircle.layered");
    /// Set on a window `no_taskbar` made a tool window (`wndproc` keeps it so).
    const TOOL: PCWSTR = w!("OpenCircle.tool");

    /// An extended style with no taskbar button.
    pub(super) fn tool_style(ex: u32) -> u32 {
        use windows::Win32::UI::WindowsAndMessaging::{WS_EX_APPWINDOW, WS_EX_TOOLWINDOW};
        (ex | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0
    }

    pub fn no_taskbar(hwnd: isize) {
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: `hwnd` is a live window of this thread; a property of ours and
        // its extended style (which our `wndproc` then keeps).
        unsafe {
            if SetPropW(hwnd, TOOL, Some(HANDLE(std::ptr::dangling_mut()))).is_ok() {
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, tool_style(ex as u32) as isize);
            }
        }
    }

    pub fn clear_now(hwnd: isize) {
        // under the lock: no fade step lands after it (`fade`)
        let mut latest = LATEST.lock().unwrap();
        latest.retain(|(w, _)| *w != hwnd);
        if let Err(e) = opacity(hwnd, 0.0) {
            crate::log!("[opencircle] window clear: {e}");
        }
    }

    /// A layered window (style set once) at `alpha`; GDI still paints into it
    /// as before (the system keeps its picture) and its region still cuts it.
    /// A colour key already set (the live controller) stays.
    pub fn opacity(hwnd: isize, alpha: f32) -> Result<(), String> {
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetLayeredWindowAttributes, LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, LWA_COLORKEY,
            SetLayeredWindowAttributes,
        };
        let hwnd = HWND(hwnd as *mut _);
        let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        // SAFETY: `hwnd` is a live window of this process (the UI's raw handle);
        // these calls only read and change its extended style, its alpha and
        // a property of ours.
        unsafe {
            if GetPropW(hwnd, LAYERED).is_invalid() {
                SetPropW(hwnd, LAYERED, Some(HANDLE(std::ptr::dangling_mut())))
                    .map_err(|e| e.to_string())?;
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let (mut key, mut flags) = (COLORREF(0), LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
            if ex & WS_EX_LAYERED.0 as isize == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
            } else if GetLayeredWindowAttributes(hwnd, Some(&mut key), None, Some(&mut flags))
                .is_err()
            {
                flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
            }
            let flags = LWA_ALPHA | (flags & LWA_COLORKEY);
            SetLayeredWindowAttributes(hwnd, key, alpha, flags).map_err(|e| e.to_string())
        }
    }

    /// The latest fade of each window: an older one still running stops (a
    /// close during the opening, an overlay opened again).
    static LATEST: std::sync::Mutex<Vec<(isize, u64)>> = std::sync::Mutex::new(Vec::new());

    /// `super::fade`: a step per beat of the frame clock (`motion::FRAME`):
    /// the window changes once a frame, by the clock.
    pub fn fade(
        hwnd: isize,
        shown: crate::ui::kit::motion::Shown,
        rise: f32,
    ) -> impl FnOnce() + Send {
        // never reused (a number reused after a `clear_now` emptied the list
        // would let a stopped fade run on)
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let me = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        {
            let mut latest = LATEST.lock().unwrap();
            latest.retain(|(w, _)| *w != hwnd);
            latest.push((hwnd, me));
        }
        move || steps(hwnd, me, shown, rise)
    }

    fn steps(hwnd: isize, me: u64, shown: crate::ui::kit::motion::Shown, rise: f32) {
        use std::time::{Duration, Instant};
        use windows::Win32::UI::WindowsAndMessaging::{
            SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        };
        let mine = || LATEST.lock().unwrap().contains(&(hwnd, me));
        let h = HWND(hwnd as *mut _);
        let mut rest = RECT::default();
        // SAFETY: `h` is a live window of this process; reading its place and
        // DPI, and moving it (asynchronously: never waits on the UI thread).
        let px = rise * unsafe { GetDpiForWindow(h) }.max(96) as f32 / 96.0;
        // the steps' gaps, for the log (the VM frame budget, #23)
        let (mut steps, mut last, mut worst) = (0, Instant::now(), Duration::ZERO);
        let mut next = Instant::now();
        if px == 0.0 || unsafe { GetWindowRect(h, &mut rest) }.is_ok() {
            // until done, or a newer fade took over
            while mine() {
                let step = Instant::now();
                (steps, worst, last) = (steps + 1, worst.max(step - last), step);
                let a = shown.at(step);
                // checked and stepped under the lock: never after a `clear_now`
                // (the window is already layered, `appear` made it so on the UI
                // thread: no style change here, so nothing waits on that thread)
                let latest = LATEST.lock().unwrap();
                if !latest.contains(&(hwnd, me)) {
                    break;
                }
                if let Err(e) = opacity(hwnd, a) {
                    crate::log!("[opencircle] window fade: {e}");
                    break;
                }
                drop(latest);
                if px != 0.0 {
                    let y = rest.top + ((1.0 - a) * px).round() as i32;
                    let how = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
                    // SAFETY: as above.
                    let _ = unsafe { SetWindowPos(h, None, rest.left, y, 0, 0, how) };
                }
                if !shown.moving(step) {
                    break;
                }
                // the frame clock's beat (`motion::FRAME`), on deadlines
                next += crate::ui::kit::motion::FRAME;
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        }
        LATEST.lock().unwrap().retain(|e| *e != (hwnd, me));
        crate::log!(
            "[opencircle] window fade: {steps} steps, max {} ms apart",
            worst.as_millis()
        );
    }

    pub fn clear(hwnd: isize) -> Result<(), String> {
        // SAFETY: `hwnd` is a live window of this process; no region = the whole window.
        unsafe { SetWindowRgn(HWND(hwnd as *mut _), None, true) };
        Ok(())
    }

    /// Shapes the window to the union of `of(width, height)`'s rectangles.
    pub fn rects(hwnd: isize, of: impl FnOnce(u32, u32) -> Vec<super::Px>) -> Result<(), String> {
        let hwnd = HWND(hwnd as *mut _);
        let mut rect = RECT::default();
        // SAFETY: `hwnd` is a live window of this process. Each part region is
        // deleted after it is merged; the whole one is handed to the system.
        unsafe {
            GetWindowRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            let region = CreateRectRgn(0, 0, 0, 0);
            for (x, y, rw, rh) in of(w.max(0) as u32, h.max(0) as u32) {
                let part = CreateRectRgn(x, y, x + rw as i32, y + rh as i32);
                CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
                let _ = DeleteObject(part.into());
            }
            if SetWindowRgn(hwnd, Some(region), true) == 0 {
                return Err("SetWindowRgn failed".into());
            }
        }
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
        rects(window, |w, h| {
            let r = corner(max_radius, w as f32, h as f32).round() as u32;
            round_rect(w, h, r)
        })
    }

    /// Shapes the window to the union of `of(width, height)`'s rectangles.
    pub fn rects(window: u32, of: impl FnOnce(u32, u32) -> Vec<super::Px>) -> Result<(), String> {
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
        let rects: Vec<x::Rectangle> = of(geometry.width().into(), geometry.height().into())
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

    /// Long capture's window: everything but the area, its toolbar kept even
    /// over the area; a hole at the window's edge leaves no empty band.
    #[test]
    fn a_holed_window_is_all_but_the_hole_plus_what_is_kept() {
        let inside = |r: &[Px], (px, py): (i32, i32)| {
            (r.iter())
                .any(|&(x, y, w, h)| px >= x && py >= y && px < x + w as i32 && py < y + h as i32)
        };
        let bands = holed(100, 80, (20, 10, 50, 40), &[(30, 40, 20, 5)]);
        assert_eq!(area(&bands[..4]), 100 * 80 - 50 * 40);
        assert!(!inside(&bands, (20, 10)) && !inside(&bands, (69, 49)));
        assert!(inside(&bands, (19, 10)) && inside(&bands, (70, 49)) && inside(&bands, (20, 50)));
        assert!(inside(&bands, (35, 42)), "the toolbar over the area stays");
        // the whole window: only what is kept
        assert_eq!(holed(100, 80, (0, 0, 100, 80), &[]), vec![]);
        assert_eq!(
            holed(100, 80, (-5, 0, 200, 10), &[]),
            vec![(0, 10, 100, 70)]
        );
    }

    /// #126: DefWindowProc must get WM_NCACTIVATE with lParam -1 (no caption
    /// painted), never the theme engine's caption / frame paints, and WM_SETTEXT /
    /// WM_SETICON only while the window is not visible (visible again after);
    /// every other message reaches the window's own procedure unchanged, and
    /// installing twice changes nothing.
    #[cfg(windows)]
    #[test]
    fn the_system_never_paints_the_caption() {
        use std::sync::atomic::{AtomicIsize, Ordering};
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GWL_STYLE, GWLP_WNDPROC, GetWindowLongPtrW,
            HWND_MESSAGE, SendMessageW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE,
            WM_NCACTIVATE, WM_SETTEXT, WM_USER, WS_VISIBLE,
        };
        use windows::core::w;

        // [NCACTIVATE lParam, WM_USER lParam, theme paints seen, visible during SETTEXT]
        static SEEN: [AtomicIsize; 4] = [const { AtomicIsize::new(0) }; 4];
        unsafe extern "system" fn own(h: HWND, msg: u32, _: WPARAM, l: LPARAM) -> LRESULT {
            match msg {
                WM_NCACTIVATE => SEEN[0].store(l.0, Ordering::SeqCst),
                WM_USER => SEEN[1].store(l.0, Ordering::SeqCst),
                win::WM_NCUAHDRAWCAPTION | win::WM_NCUAHDRAWFRAME => {
                    SEEN[2].fetch_add(1, Ordering::SeqCst);
                }
                WM_SETTEXT => {
                    // SAFETY: reads the style of the live test window.
                    let style = unsafe { GetWindowLongPtrW(h, GWL_STYLE) };
                    SEEN[3].store(style & WS_VISIBLE.0 as isize, Ordering::SeqCst);
                }
                _ => {}
            }
            LRESULT(1)
        }
        // SAFETY: a message-only window of this thread, destroyed at the end.
        let visible_after = unsafe {
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
            SendMessageW(hwnd, win::WM_NCUAHDRAWCAPTION, None, None);
            SendMessageW(hwnd, win::WM_NCUAHDRAWFRAME, None, None);
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style | WS_VISIBLE.0 as isize);
            SEEN[3].store(-1, Ordering::SeqCst);
            SendMessageW(
                hwnd,
                WM_SETTEXT,
                None,
                Some(LPARAM(w!("new").as_ptr() as isize)),
            );
            let after = GetWindowLongPtrW(hwnd, GWL_STYLE) & WS_VISIBLE.0 as isize != 0;
            let _ = DestroyWindow(hwnd);
            after
        };
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
        assert_eq!(SEEN[2].load(Ordering::SeqCst), 0, "theme caption painted");
        assert_eq!(
            SEEN[3].load(Ordering::SeqCst),
            0,
            "title set on a visible window"
        );
        assert!(visible_after, "the window stays visible");
    }

    /// A hidden top-level test window of this thread (winit's default style:
    /// overlapped, `WS_EX_APPWINDOW`), with our subclass; destroyed by the caller.
    #[cfg(windows)]
    fn test_window() -> windows::Win32::Foundation::HWND {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
        };
        use windows::core::w;
        // SAFETY: a plain window of this thread, never shown.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_APPWINDOW,
                w!("STATIC"),
                w!("style test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        };
        win::no_caption(hwnd.0 as isize);
        hwnd
    }

    /// The owner's taskbar flash: winit rewrites the whole extended style (with
    /// `WS_EX_APPWINDOW`) on every show, hide and full-screen change, so the
    /// overlay got a taskbar button whenever it was shown windowed (leaving
    /// full-screen as it closed). A `no_taskbar` window stays a tool window
    /// through any such rewrite; another window is left as it is.
    #[cfg(windows)]
    #[test]
    fn a_window_off_the_taskbar_stays_off_it() {
        use windows::Win32::UI::WindowsAndMessaging::{
            DestroyWindow, GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_ACCEPTFILES,
            WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };
        let (tool, app) = (test_window(), test_window());
        win::no_taskbar(tool.0 as isize);
        let rewrite = (WS_EX_APPWINDOW | WS_EX_ACCEPTFILES).0 as isize;
        // SAFETY: the test's own windows, destroyed at the end.
        let (tool_ex, app_ex) = unsafe {
            let first = GetWindowLongPtrW(tool, GWL_EXSTYLE) as u32;
            assert!(
                first & WS_EX_TOOLWINDOW.0 != 0,
                "not a tool window: {first:x}"
            );
            SetWindowLongPtrW(tool, GWL_EXSTYLE, rewrite); // winit, on a show
            SetWindowLongPtrW(app, GWL_EXSTYLE, rewrite);
            let ex = |h| GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            let got = (ex(tool), ex(app));
            let _ = DestroyWindow(tool);
            let _ = DestroyWindow(app);
            got
        };
        assert!(
            tool_ex & WS_EX_TOOLWINDOW.0 != 0,
            "tool style dropped: {tool_ex:x}"
        );
        assert!(
            tool_ex & WS_EX_APPWINDOW.0 == 0,
            "app window again: {tool_ex:x}"
        );
        assert!(
            tool_ex & WS_EX_ACCEPTFILES.0 != 0,
            "the rest of winit's style kept"
        );
        assert_eq!(app_ex & WS_EX_TOOLWINDOW.0, 0, "another window untouched");
    }

    /// The owner's gray flash, the fade's part: `clear_now` leaves the window
    /// clear, and a fade asked before it (the overlay's fade-in, Esc during the
    /// opening) never steps it visible again, even one whose thread had not
    /// started yet.
    #[cfg(windows)]
    #[test]
    fn a_cleared_window_stays_clear() {
        use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, GetLayeredWindowAttributes};
        let hwnd = test_window();
        let raw = hwnd.0 as isize;
        // a session without a desktop (a service, SSH) has no layered windows
        if let Err(e) = win::opacity(raw, 1.0) {
            eprintln!("skipped: no layered windows in this session ({e})");
            // SAFETY: the test's own window.
            let _ = unsafe { DestroyWindow(hwnd) };
            return;
        }
        let now = std::time::Instant::now();
        let fade_in = fade(
            raw,
            crate::ui::kit::motion::Shown::opening(crate::ui::kit::motion::WIDGET, now),
            0.0,
        );
        clear_now(raw);
        fade_in(); // its thread, late
        let mut alpha = 255u8;
        // SAFETY: the test's own window, destroyed at the end.
        unsafe {
            GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), None).unwrap();
            let _ = DestroyWindow(hwnd);
        }
        assert_eq!(alpha, 0, "shown again after the clear");
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
            crate::log!("no X11 display: skipped");
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
