//! The windows on screen, for snip (hover a window, click it; Window mode): outer
//! rectangles (with the frame) in physical desktop pixels, topmost first, with
//! their titles; none of our own. Read before the overlay shows, while it is
//! hidden (it would be the topmost window).
//!
//! Windows: `EnumWindows` (z-order) + the DWM extended frame bounds (no invisible
//! resize border). X11: `_NET_CLIENT_LIST_STACKING` + geometry + `_NET_FRAME_EXTENTS`.
//! Native Wayland has no such API: empty (the UI hides Window mode).

/// x, y, width, height: physical desktop px.
pub type WinRect = (i32, i32, u32, u32);

/// One window on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Win {
    pub rect: WinRect,
    pub title: String,
    /// Windows: its handle (for UI Automation); X11: its id.
    pub handle: isize,
}

/// Every visible top-level window, topmost first; empty where unknown.
pub fn list() -> Vec<Win> {
    platform().unwrap_or_else(|e| {
        eprintln!("[opencircle] window list unavailable: {e}");
        Vec::new()
    })
}

/// The window the user is in (Windows: its handle), read before the overlay shows.
pub fn foreground() -> Option<isize> {
    #[cfg(windows)]
    {
        // SAFETY: plain query.
        let h = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        (!h.is_invalid()).then_some(h.0 as isize)
    }
    #[cfg(not(windows))]
    None
}

/// Gives the focus back to `foreground()`'s window (the overlay shrank to a pill:
/// the user goes on in their app, e.g. pastes into a chat).
pub fn activate(window: Option<isize>) {
    #[cfg(windows)]
    if let Some(h) = window {
        // SAFETY: plain call on a handle; a stale one just fails.
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
                windows::Win32::Foundation::HWND(h as *mut _),
            )
        };
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(target_os = "linux")]
fn platform() -> Result<Vec<Win>, String> {
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
    let (name, utf8, pid) = (
        atom("_NET_WM_NAME").map_err(err)?,
        atom("UTF8_STRING").map_err(err)?,
        atom("_NET_WM_PID").map_err(err)?,
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
        let ours = pid != x::ATOM_NONE
            && property(window, pid, x::ATOM_CARDINAL, 1)
                .is_ok_and(|p| p.value::<u32>().first() == Some(&std::process::id()));
        if ours {
            continue;
        }
        let title = match property(window, name, utf8, 1024) {
            Ok(t) if name != x::ATOM_NONE && !t.value::<u8>().is_empty() => {
                String::from_utf8_lossy(t.value::<u8>()).into_owned()
            }
            _ => property(window, x::ATOM_WM_NAME, x::ATOM_STRING, 1024)
                .map(|t| String::from_utf8_lossy(t.value::<u8>()).into_owned())
                .unwrap_or_default(),
        };
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
        let rect = (
            i32::from(at.dst_x()) - l as i32,
            i32::from(at.dst_y()) - t as i32,
            u32::from(geometry.width()) + l + r,
            u32::from(geometry.height()) + t + b,
        );
        out.push(Win {
            rect,
            title,
            handle: xcb::Xid::resource_id(&window) as isize,
        });
    }
    Ok(out)
}

#[cfg(windows)]
fn platform() -> Result<Vec<Win>, String> {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetWindowLongW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_TOOLWINDOW,
    };
    use windows::core::BOOL;

    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the `&mut Vec` passed to EnumWindows below, alive for the call.
        let out = unsafe { &mut *(out.0 as *mut Vec<Win>) };
        // SAFETY: plain queries on a window handle EnumWindows just gave us.
        unsafe {
            let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let ours = pid == std::process::id();
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || tool || ours {
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
                let mut title = [0u16; 256];
                let n = GetWindowTextW(hwnd, &mut title).max(0) as usize;
                out.push(Win {
                    rect: (
                        r.left,
                        r.top,
                        (r.right - r.left) as u32,
                        (r.bottom - r.top) as u32,
                    ),
                    title: String::from_utf16_lossy(&title[..n]),
                    handle: hwnd.0 as isize,
                });
            }
        }
        BOOL(1)
    }

    let mut out: Vec<Win> = Vec::new();
    // SAFETY: `each` only uses `out` during this call.
    unsafe { EnumWindows(Some(each), LPARAM((&raw mut out) as isize)) }
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform() -> Result<Vec<Win>, String> {
    Ok(Vec::new())
}
