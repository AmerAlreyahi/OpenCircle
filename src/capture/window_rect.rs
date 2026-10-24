//! The windows on screen, for snip's Window mode: outer rectangles (with the
//! frame) in physical desktop pixels, topmost first. Read before the overlay
//! shows, while it is hidden (it would be the topmost window).
//!
//! Windows: `EnumWindows` (z-order) + the DWM extended frame bounds (no invisible
//! resize border). X11: `_NET_CLIENT_LIST_STACKING` + geometry + `_NET_FRAME_EXTENTS`.
//! Native Wayland has no such API: empty (the UI hides Window mode).

/// x, y, width, height: physical desktop px.
pub type WinRect = (i32, i32, u32, u32);

/// Every visible top-level window, topmost first; empty where unknown.
pub fn list() -> Vec<WinRect> {
    platform().unwrap_or_else(|e| {
        eprintln!("[opencircle] window list unavailable: {e}");
        Vec::new()
    })
}

#[cfg(target_os = "linux")]
fn platform() -> Result<Vec<WinRect>, String> {
    use xcb::x;

    if super::wayland_session() || std::env::var_os("DISPLAY").is_none() {
        return Ok(Vec::new());
    }
    let err = |e: xcb::Error| format!("{e:?}");
    let (conn, screen) = xcb::Connection::connect(None).map_err(|e| e.to_string())?;
    let root = conn
        .get_setup()
        .roots()
        .nth(screen as usize)
        .ok_or("no X11 screen")?
        .root();
    let atom = |name: &str| {
        conn.wait_for_reply(conn.send_request(&x::InternAtom {
            only_if_exists: true,
            name: name.as_bytes(),
        }))
        .map(|r| r.atom())
    };
    let (stacking, extents) = (
        atom("_NET_CLIENT_LIST_STACKING").map_err(err)?,
        atom("_NET_FRAME_EXTENTS").map_err(err)?,
    );
    if stacking == x::ATOM_NONE {
        return Ok(Vec::new()); // no EWMH window manager
    }
    let property = |window, property, r#type, long_length| {
        conn.wait_for_reply(conn.send_request(&x::GetProperty {
            delete: false,
            window,
            property,
            r#type,
            long_offset: 0,
            long_length,
        }))
    };
    let clients = property(root, stacking, x::ATOM_WINDOW, 4096).map_err(err)?;
    let mut out = Vec::new();
    // Bottom to top in the property: reversed for topmost first.
    for &window in clients.value::<x::Window>().iter().rev() {
        let viewable = conn
            .wait_for_reply(conn.send_request(&x::GetWindowAttributes { window }))
            .is_ok_and(|a| a.map_state() == x::MapState::Viewable);
        if !viewable {
            continue; // minimised or another desktop
        }
        let Ok(geometry) = conn.wait_for_reply(conn.send_request(&x::GetGeometry {
            drawable: x::Drawable::Window(window),
        })) else {
            continue;
        };
        let Ok(at) = conn.wait_for_reply(conn.send_request(&x::TranslateCoordinates {
            src_window: window,
            dst_window: root,
            src_x: 0,
            src_y: 0,
        })) else {
            continue;
        };
        // left, right, top, bottom decorations drawn by the window manager.
        let [l, r, t, b] = match property(window, extents, x::ATOM_CARDINAL, 4) {
            Ok(e) if extents != x::ATOM_NONE && e.value::<u32>().len() == 4 => {
                let v = e.value::<u32>();
                [v[0], v[1], v[2], v[3]]
            }
            _ => [0; 4],
        };
        out.push((
            i32::from(at.dst_x()) - l as i32,
            i32::from(at.dst_y()) - t as i32,
            u32::from(geometry.width()) + l + r,
            u32::from(geometry.height()) + t + b,
        ));
    }
    Ok(out)
}

#[cfg(windows)]
fn platform() -> Result<Vec<WinRect>, String> {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible,
        WS_EX_TOOLWINDOW,
    };
    use windows::core::BOOL;

    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the `&mut Vec` passed to EnumWindows below, alive for the call.
        let out = unsafe { &mut *(out.0 as *mut Vec<WinRect>) };
        // SAFETY: plain queries on a window handle EnumWindows just gave us.
        unsafe {
            let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || tool {
                return BOOL(1);
            }
            // Cloaked: on another virtual desktop, or a suspended store app.
            let mut cloaked = 0u32;
            let hidden = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast::<c_void>(),
                size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0;
            let mut r = RECT::default();
            let bounds = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&raw mut r).cast::<c_void>(),
                size_of::<RECT>() as u32,
            )
            .or_else(|_| GetWindowRect(hwnd, &mut r));
            if !hidden && bounds.is_ok() && r.right > r.left && r.bottom > r.top {
                out.push((
                    r.left,
                    r.top,
                    (r.right - r.left) as u32,
                    (r.bottom - r.top) as u32,
                ));
            }
        }
        BOOL(1)
    }

    let mut out: Vec<WinRect> = Vec::new();
    // SAFETY: `each` only uses `out` during this call.
    unsafe { EnumWindows(Some(each), LPARAM((&raw mut out) as isize)) }
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform() -> Result<Vec<WinRect>, String> {
    Ok(Vec::new())
}

/// A browser's tabs and toolbar, logical px (Edge, Chrome, Firefox at
/// 100 %: about 80; a restored window has about 8 more above the tabs).
/// ponytail: fixed; a bookmarks bar stays in (and is translated); the page's
/// own child window (Chromium's render widget) is the exact upgrade.
const BROWSER_BARS: f32 = 86.0;
/// Process (Windows) or WM_CLASS (X11) names that are browsers.
const BROWSERS: [&str; 7] = [
    "msedge", "chrome", "chromium", "firefox", "brave", "opera", "vivaldi",
];

/// The window in use, for translating it instead of the whole screen: its
/// client area (no title bar, frame or taskbar) in physical desktop px, and
/// for a browser only the page below its tabs and address bar. Our own
/// windows (the overlay, the live layer) are passed over for the next one
/// down. `None`: unknown (Wayland), or the desktop itself. `scale` = the
/// monitor's (for the browser bars).
pub fn foreground(scale: f32) -> Option<WinRect> {
    let (client, app, maximized) = active()?;
    let browser = BROWSERS.iter().any(|b| app.to_lowercase().contains(b));
    page(client, browser, maximized, scale)
}

/// [`foreground`] on a monitor whose frame starts at `origin` (desktop px)
/// and is `size` big: frame px x0, y0, x1, y1 (ends exclusive). `None`:
/// unknown, the desktop, or under 64 px of it on this monitor.
pub fn region(origin: (i32, i32), size: (u32, u32), scale: f32) -> Option<[u32; 4]> {
    let (x, y, w, h) = foreground(scale)?;
    clip((x - origin.0, y - origin.1, w, h), size)
}

fn clip((x, y, w, h): WinRect, (fw, fh): (u32, u32)) -> Option<[u32; 4]> {
    let (x0, x1) = (x.clamp(0, fw as i32), (x + w as i32).clamp(0, fw as i32));
    let (y0, y1) = (y.clamp(0, fh as i32), (y + h as i32).clamp(0, fh as i32));
    (x1 >= x0 + 64 && y1 >= y0 + 64).then_some([x0 as u32, y0 as u32, x1 as u32, y1 as u32])
}

/// The part of a client area that is content (see [`foreground`]).
fn page((x, y, w, h): WinRect, browser: bool, maximized: bool, scale: f32) -> Option<WinRect> {
    let bars = match (browser, maximized) {
        (false, _) => 0.0,
        (true, true) => BROWSER_BARS,
        (true, false) => BROWSER_BARS + 8.0,
    };
    let bars = (bars * scale).round() as u32;
    (w > 32 && h > bars + 32).then(|| (x, y + bars as i32, w, h - bars))
}

/// The active window not ours: its client rect, app name, maximised.
#[cfg(windows)]
fn active() -> Option<(WinRect, String, bool)> {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOTOWNER, GW_HWNDNEXT, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetForegroundWindow,
        GetTopWindow, GetWindow, GetWindowInfo, GetWindowLongW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, IsZoomed, WINDOWINFO, WS_EX_TOOLWINDOW,
    };
    use windows::core::PWSTR;

    // SAFETY (whole block): plain queries on window handles the system just
    // gave us; buffers outlive the calls; the process handle is closed.
    unsafe {
        let me = GetCurrentProcessId();
        let pid = |hwnd: HWND| {
            let mut p = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut p));
            p
        };
        let class = |hwnd: HWND| {
            let mut b = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut b).max(0) as usize;
            String::from_utf16_lossy(&b[..n])
        };
        let usable = |hwnd: HWND| {
            let mut cloaked = 0u32;
            let hidden = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast::<c_void>(),
                size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0;
            let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
            IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() && !tool && !hidden
        };
        let taskbar = |c: &str| c == "Shell_TrayWnd" || c == "Shell_SecondaryTrayWnd";
        let mut hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || pid(hwnd) == me {
            // ours has the focus (the overlay): the next window down
            // ponytail: another app's always-on-top window wins over the
            // focused one here
            let mut next = GetTopWindow(None).ok();
            hwnd = loop {
                let h = next?;
                if usable(h) && pid(h) != me && !taskbar(&class(h)) {
                    break h;
                }
                next = GetWindow(h, GW_HWNDNEXT).ok();
            };
        }
        // a popup (a browser's "Restore pages" bubble, a dialog): the app
        // window that owns it
        let owner = GetAncestor(hwnd, GA_ROOTOWNER);
        if !owner.is_invalid() && usable(owner) {
            hwnd = owner;
        }
        let c = class(hwnd);
        if matches!(c.as_str(), "Progman" | "WorkerW") || taskbar(&c) {
            return None; // the desktop
        }
        let mut info = WINDOWINFO {
            cbSize: size_of::<WINDOWINFO>() as u32,
            ..Default::default()
        };
        GetWindowInfo(hwnd, &mut info).ok()?;
        let r = info.rcClient;
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        let app = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid(hwnd)) {
            Ok(process) => {
                let ok = QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    PWSTR(name.as_mut_ptr()),
                    &mut len,
                )
                .is_ok();
                let _ = CloseHandle(process);
                if ok {
                    String::from_utf16_lossy(&name[..len as usize])
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        };
        let app = app.rsplit('\\').next().unwrap_or_default().to_string();
        (r.right > r.left && r.bottom > r.top).then(|| {
            let rect = (
                r.left,
                r.top,
                (r.right - r.left) as u32,
                (r.bottom - r.top) as u32,
            );
            (rect, app, IsZoomed(hwnd).as_bool())
        })
    }
}

#[cfg(target_os = "linux")]
fn active() -> Option<(WinRect, String, bool)> {
    use xcb::{Xid, XidNew, x};

    if super::wayland_session() || std::env::var_os("DISPLAY").is_none() {
        return None;
    }
    let (conn, screen) = xcb::Connection::connect(None).ok()?;
    let root = conn.get_setup().roots().nth(screen as usize)?.root();
    let atom = |name: &str| {
        let a = conn
            .wait_for_reply(conn.send_request(&x::InternAtom {
                only_if_exists: true,
                name: name.as_bytes(),
            }))
            .ok()?
            .atom();
        (a != x::ATOM_NONE).then_some(a)
    };
    let property = |window, property, r#type| {
        conn.wait_for_reply(conn.send_request(&x::GetProperty {
            delete: false,
            window,
            property,
            r#type,
            long_offset: 0,
            long_length: 4096,
        }))
        .ok()
    };
    let words = |window, name: &str, r#type| -> Vec<u32> {
        atom(name)
            .and_then(|a| property(window, a, r#type))
            .map(|r| r.value::<u32>().to_vec())
            .unwrap_or_default()
    };
    let me = std::process::id();
    let shell: Vec<u32> = ["_NET_WM_WINDOW_TYPE_DESKTOP", "_NET_WM_WINDOW_TYPE_DOCK"]
        .iter()
        .filter_map(|n| atom(n))
        .map(|a| a.resource_id())
        .collect();
    let skip = |w: x::Window| {
        let ours = words(w, "_NET_WM_PID", x::ATOM_CARDINAL).first() == Some(&me);
        let kind = words(w, "_NET_WM_WINDOW_TYPE", x::ATOM_ATOM);
        ours || kind.iter().any(|k| shell.contains(k))
    };
    // the active window, else (ours is active) the topmost of the rest
    let active = words(root, "_NET_ACTIVE_WINDOW", x::ATOM_WINDOW);
    let stacking: Vec<u32> = atom("_NET_CLIENT_LIST_STACKING")
        .and_then(|a| property(root, a, x::ATOM_WINDOW))
        .map(|r| r.value::<u32>().to_vec())
        .unwrap_or_default();
    let window = (active.into_iter().chain(stacking.into_iter().rev()))
        .filter(|&id| id != 0)
        // (ids the window manager lists as client windows)
        .map(x::Window::new)
        .find(|&w| {
            let viewable = conn
                .wait_for_reply(conn.send_request(&x::GetWindowAttributes { window: w }))
                .is_ok_and(|a| a.map_state() == x::MapState::Viewable);
            viewable && !skip(w)
        })?;
    let geometry = conn
        .wait_for_reply(conn.send_request(&x::GetGeometry {
            drawable: x::Drawable::Window(window),
        }))
        .ok()?;
    let at = conn
        .wait_for_reply(conn.send_request(&x::TranslateCoordinates {
            src_window: window,
            dst_window: root,
            src_x: 0,
            src_y: 0,
        }))
        .ok()?;
    let class = property(window, x::ATOM_WM_CLASS, x::ATOM_STRING)
        .map(|r| String::from_utf8_lossy(r.value::<u8>()).replace('\0', " "))
        .unwrap_or_default();
    let maximized = atom("_NET_WM_STATE_MAXIMIZED_VERT")
        .is_some_and(|m| words(window, "_NET_WM_STATE", x::ATOM_ATOM).contains(&m.resource_id()));
    let rect = (
        i32::from(at.dst_x()),
        i32::from(at.dst_y()),
        u32::from(geometry.width()),
        u32::from(geometry.height()),
    );
    Some((rect, class, maximized))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn active() -> Option<(WinRect, String, bool)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_browser_loses_its_bars_other_apps_keep_the_client_area() {
        let client = (0, 0, 1716, 1200);
        assert_eq!(page(client, false, true, 1.68), Some(client));
        // 86 logical px at 168 %
        assert_eq!(page(client, true, true, 1.68), Some((0, 144, 1716, 1056)));
        assert_eq!(page(client, true, false, 1.0), Some((0, 94, 1716, 1106)));
        assert_eq!(
            page((0, 0, 1716, 150), true, true, 1.68),
            None,
            "no page left"
        );
        // on the monitor: clipped to the frame, nothing if hardly on it
        assert_eq!(
            clip((-8, 144, 1732, 1056), (1716, 1271)),
            Some([0, 144, 1716, 1200])
        );
        assert_eq!(clip((1700, 0, 800, 600), (1716, 1271)), None);
    }
}
