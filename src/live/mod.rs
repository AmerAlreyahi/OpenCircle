//! The live-translate layer, platform side only: make an existing window
//! click-through, topmost and, where the OS can, invisible to screen capture.
//! The UI creates the window (borderless, transparent, monitor-sized,
//! `Level::AlwaysOnTop`) and passes its raw handle, e.g.
//! `window::run(id, |w| w.window_handle().map(|h| live::prepare_layer(h.as_raw())))`.
//! What to draw comes from `translate::Live`.

use iced::window::raw_window_handle::RawWindowHandle;

/// Windows: the layer is colour-keyed. Paint every pixel that is not a patch
/// in this colour; it becomes transparent (and click-through).
pub const COLOUR_KEY: [u8; 3] = [255, 0, 255];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerError {
    /// No layer here (Wayland): live mode falls back to one-shot per press.
    Unsupported(&'static str),
    /// The window system refused a call.
    Os(String),
}

impl std::fmt::Display for LayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LayerError::Unsupported(why) => write!(f, "live translate is not available: {why}"),
            LayerError::Os(e) => write!(f, "could not set up the live layer: {e}"),
        }
    }
}

impl std::error::Error for LayerError {}

/// What the platform could do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    /// False on X11 and on Windows before 10 2004: captures show the layer,
    /// and only `translate::Live`'s read-back guard keeps it from being read.
    pub excluded_from_capture: bool,
}

/// The live controller's small window: topmost, takes clicks without taking the
/// focus from the game, colour-keyed like the layer, and invisible to capture
/// where the OS can (Windows). `Ok(excluded from capture)`.
pub fn prepare_controller(handle: RawWindowHandle) -> Result<bool, LayerError> {
    match handle {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => {
            win::prepare(h.hwnd.get(), false).map(|l| l.excluded_from_capture)
        }
        // X11: topmost from the window's level; captures show it (the step masks it).
        _ => Ok(false),
    }
}

/// Where this app's own windows show in a capture (desktop px, x0, y0, x1,
/// y1): every visible top-level window of this process, except the ones
/// captures leave out (excluded from capture) and the click-through layer
/// (its patches have their own guard). A step blanks these out, so our own
/// UI (a menu, a card, a pill, a toast) is never read or translated.
// ponytail: Windows only; X11 needs a walk of the window tree by _NET_WM_PID,
// so there only the controller's rect is masked (`ui::live_layer`).
pub fn own_windows() -> Vec<[i32; 4]> {
    #[cfg(windows)]
    return win::own_windows();
    #[cfg(not(windows))]
    Vec::new()
}

pub fn prepare_layer(handle: RawWindowHandle) -> Result<Layer, LayerError> {
    match handle {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => win::prepare(h.hwnd.get(), true),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(h) => x11::prepare(h.window as u32),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(h) => x11::prepare(h.window.get()),
        RawWindowHandle::Wayland(_) => Err(LayerError::Unsupported(
            "Wayland does not let an app keep a click-through layer over other apps",
        )),
        _ => Err(LayerError::Unsupported("unknown window system")),
    }
}

#[cfg(windows)]
mod win {
    use super::{COLOUR_KEY, Layer, LayerError};
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetLayeredWindowAttributes, GetWindowLongPtrW, HWND_TOPMOST,
        LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, LWA_COLORKEY, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW,
        SetWindowPos, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    };

    pub fn own_windows() -> Vec<[i32; 4]> {
        use windows::Win32::Foundation::{LPARAM, RECT};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowDisplayAffinity, GetWindowRect, GetWindowThreadProcessId,
            IsWindowVisible,
        };
        unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> windows::core::BOOL {
            // SAFETY: `out` is the Vec below, alive for the whole EnumWindows call;
            // the calls only read the window's state.
            unsafe {
                let out = &mut *(out.0 as *mut Vec<[i32; 4]>);
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let mut affinity = 0;
                let _ = GetWindowDisplayAffinity(hwnd, &mut affinity);
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                let mut r = RECT::default();
                if pid == std::process::id()
                    && IsWindowVisible(hwnd).as_bool()
                    && affinity != WDA_EXCLUDEFROMCAPTURE.0
                    && ex & WS_EX_TRANSPARENT.0 == 0
                    && GetWindowRect(hwnd, &mut r).is_ok()
                    && r.right > r.left
                    && r.bottom > r.top
                {
                    out.push([r.left, r.top, r.right, r.bottom]);
                }
            }
            true.into()
        }
        let mut out: Vec<[i32; 4]> = Vec::new();
        // SAFETY: the callback only touches `out`, which outlives the call.
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize)) };
        out
    }

    /// `click_through`: the layer (clicks go to the game); the controller takes them.
    pub fn prepare(hwnd: isize, click_through: bool) -> Result<Layer, LayerError> {
        let hwnd = HWND(hwnd as *mut _);
        let os = |e: windows::core::Error| LayerError::Os(e.to_string());
        let ex = WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOPMOST;
        let ex = if click_through {
            ex | WS_EX_TRANSPARENT
        } else {
            ex
        };
        let [r, g, b] = COLOUR_KEY.map(u32::from);
        // SAFETY: `hwnd` is a live window of this process (the UI's raw
        // handle); these calls only change its styles, z-order and affinity.
        unsafe {
            let old = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            // tool window: no taskbar button, not in Alt+Tab
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, old | (ex | WS_EX_TOOLWINDOW).0 as isize);
            // a fade under way (`ui::windows::fade`, the controller) keeps its alpha
            let (mut alpha, mut flags) = (255, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
            let fading = old & WS_EX_LAYERED.0 as isize != 0
                && GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags))
                    .is_ok()
                && flags.contains(LWA_ALPHA);
            let (alpha, flags) = match fading {
                true => (alpha, LWA_COLORKEY | LWA_ALPHA),
                false => (255, LWA_COLORKEY),
            };
            SetLayeredWindowAttributes(hwnd, COLORREF(r | g << 8 | b << 16), alpha, flags)
                .map_err(os)?;
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .map_err(os)?;
            // Not WDA_MONITOR as a fallback: it blacks the window out of
            // captures, and a monitor-sized layer would black out the frame.
            let excluded = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_ok();
            if !excluded {
                eprintln!(
                    "[opencircle] live layer: no capture exclusion (needs Windows 10 2004+); \
                     relying on the read-back guard"
                );
            }
            Ok(Layer {
                excluded_from_capture: excluded,
            })
        }
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::{Layer, LayerError};
    use xcb::{XidNew, shape, x, xfixes};

    /// An empty input shape: every click goes to the window below. X11 has no
    /// capture exclusion; topmost comes from the window's `AlwaysOnTop` level.
    pub fn prepare(window: u32) -> Result<Layer, LayerError> {
        let os = |e: &dyn std::fmt::Debug| LayerError::Os(format!("X11: {e:?}"));
        let (conn, _) = xcb::Connection::connect_with_extensions(
            None,
            &[xcb::Extension::XFixes, xcb::Extension::Shape],
            &[],
        )
        .map_err(|e| os(&e))?;
        conn.wait_for_reply(conn.send_request(&xfixes::QueryVersion {
            client_major_version: 5,
            client_minor_version: 0,
        }))
        .map_err(|e| os(&e))?;
        let region: xfixes::Region = conn.generate_id();
        conn.send_and_check_request(&xfixes::CreateRegion {
            region,
            rectangles: &[],
        })
        .map_err(|e| os(&e))?;
        let set = conn.send_and_check_request(&xfixes::SetWindowShapeRegion {
            dest: x::Window::new(window),
            dest_kind: shape::Sk::Input,
            x_offset: 0,
            y_offset: 0,
            region,
        });
        let _ = conn.send_and_check_request(&xfixes::DestroyRegion { region });
        set.map_err(|e| os(&e))?;
        Ok(Layer {
            excluded_from_capture: false,
        })
    }

    #[test]
    fn empty_input_shape_on_a_real_window() {
        use xcb::Xid;
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("no X11 display: skipped");
            return;
        }
        let (conn, screen) =
            xcb::Connection::connect_with_extensions(None, &[xcb::Extension::Shape], &[]).unwrap();
        let root = conn.get_setup().roots().nth(screen as usize).unwrap();
        let window: x::Window = conn.generate_id();
        conn.send_and_check_request(&x::CreateWindow {
            depth: x::COPY_FROM_PARENT as u8,
            wid: window,
            parent: root.root(),
            x: 0,
            y: 0,
            width: 64,
            height: 64,
            border_width: 0,
            class: x::WindowClass::InputOutput,
            visual: root.root_visual(),
            value_list: &[],
        })
        .unwrap();
        let input = || {
            conn.wait_for_reply(conn.send_request(&shape::GetRectangles {
                window,
                source_kind: shape::Sk::Input,
            }))
            .unwrap()
            .rectangles()
            .len()
        };
        assert_eq!(input(), 1, "a new window takes input everywhere");
        let layer = prepare(window.resource_id()).unwrap();
        assert!(!layer.excluded_from_capture);
        assert_eq!(input(), 0, "click-through: no input area left");
        conn.send_and_check_request(&x::DestroyWindow { window })
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::window::raw_window_handle::WaylandWindowHandle;
    use std::ptr::NonNull;

    #[test]
    fn wayland_is_unsupported() {
        // never dereferenced: Wayland is refused before any call
        let surface = NonNull::<u8>::dangling().cast();
        let handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(surface));
        assert!(matches!(
            prepare_layer(handle),
            Err(LayerError::Unsupported(_))
        ));
    }
}
