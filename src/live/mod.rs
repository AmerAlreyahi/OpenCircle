//! The live-translate layer, platform side only: make an existing window
//! click-through, topmost and, where the OS can, invisible to screen capture.
//! The UI creates the window (borderless, transparent, monitor-sized,
//! `Level::AlwaysOnTop`) and passes its raw handle, e.g.
//! `window::run(id, |w| w.window_handle().map(|h| live::prepare_layer(h.as_raw())))`.
//! What to draw comes from `translate::Live`.

use std::sync::atomic::{AtomicU8, Ordering};

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

/// Why live translate captures nothing now (#56), as bits: the session is
/// locked, its console went to another user / was disconnected, or the
/// screen is off. Windows: kept by `win::watch` from the WTS session and
/// display notifications; never set elsewhere.
// ponytail: Linux has no watcher (logind's Lock signal / DPMS when Linux
// work resumes); there a locked screen just fails its captures.
static AWAY: AtomicU8 = AtomicU8::new(0);
const LOCKED: u8 = 1;
const DISCONNECTED: u8 = 2;
const SCREEN_OFF: u8 = 4;

/// `WM_WTSSESSION_CHANGE`'s events (its wParam), as winuser.h numbers them.
const WTS_CONSOLE_CONNECT: u32 = 1;
const WTS_CONSOLE_DISCONNECT: u32 = 2;
const WTS_REMOTE_CONNECT: u32 = 3;
const WTS_REMOTE_DISCONNECT: u32 = 4;
const WTS_SESSION_LOCK: u32 = 7;
const WTS_SESSION_UNLOCK: u32 = 8;

/// The session is away (locked, switched or disconnected, screen off): live
/// translate captures nothing until it is back. The first call starts the
/// watcher (Windows).
pub fn away() -> bool {
    #[cfg(windows)]
    {
        static WATCH: std::sync::Once = std::sync::Once::new();
        WATCH.call_once(win::watch);
    }
    AWAY.load(Ordering::SeqCst) != 0
}

/// The away bits after a session event (`WTS_*`).
fn on_session(away: u8, event: u32) -> u8 {
    match event {
        WTS_SESSION_LOCK => away | LOCKED,
        WTS_SESSION_UNLOCK => away & !LOCKED,
        WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => away | DISCONNECTED,
        WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => away & !DISCONNECTED,
        _ => away,
    }
}

/// The away bits after the display's state changed (0 off, 1 on, 2 dimmed:
/// still readable).
fn on_display(away: u8, state: u32) -> u8 {
    match state {
        0 => away | SCREEN_OFF,
        _ => away & !SCREEN_OFF,
    }
}

#[cfg(windows)]
fn update_away(change: impl Fn(u8) -> u8) {
    let was = AWAY
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |a| Some(change(a)))
        .unwrap_or_default();
    let now = change(was);
    if (was == 0) != (now == 0) {
        let what = if now == 0 { "back" } else { "away" };
        crate::log!("[opencircle] session {what} (locked / switched / screen off: {now:03b})");
    }
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
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetLayeredWindowAttributes, GetWindowLongPtrW, HWND_TOPMOST,
        LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, LWA_COLORKEY, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW,
        SetWindowPos, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    };

    /// A message-only window on its own thread for the rest of the run: WTS
    /// session changes (lock, user switch, disconnect) and the display's
    /// on/off state set `AWAY`. It costs one thread asleep in `GetMessageW`.
    pub fn watch() {
        use std::sync::atomic::AtomicIsize;
        use windows::Win32::Foundation::{HANDLE, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::Power::{
            POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
        };
        use windows::Win32::System::RemoteDesktop::{
            NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            CallWindowProcW, CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DispatchMessageW,
            GWLP_WNDPROC, GetMessageW, HWND_MESSAGE, MSG, PBT_POWERSETTINGCHANGE, WINDOW_EX_STYLE,
            WINDOW_STYLE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDPROC,
        };
        use windows::core::{GUID, w};
        /// GUID_CONSOLE_DISPLAY_STATE (SystemServices, a big feature for one GUID).
        const DISPLAY: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);
        static OLD: AtomicIsize = AtomicIsize::new(0);

        unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            match msg {
                WM_WTSSESSION_CHANGE => super::update_away(|a| super::on_session(a, w.0 as u32)),
                WM_POWERBROADCAST if w.0 as u32 == PBT_POWERSETTINGCHANGE => {
                    // SAFETY: with PBT_POWERSETTINGCHANGE, lParam points to the
                    // setting; GUID_CONSOLE_DISPLAY_STATE's data is one DWORD.
                    let state = unsafe {
                        let s = &*(l.0 as *const POWERBROADCAST_SETTING);
                        (s.PowerSetting == DISPLAY && s.DataLength >= 4)
                            .then(|| (s.Data.as_ptr() as *const u32).read_unaligned())
                    };
                    if let Some(state) = state {
                        super::update_away(|a| super::on_display(a, state));
                    }
                    return LRESULT(1);
                }
                _ => {}
            }
            // SAFETY: the STATIC class's own procedure, saved before this one
            // was installed.
            unsafe {
                let old: WNDPROC = std::mem::transmute(OLD.load(Ordering::SeqCst));
                CallWindowProcW(old, hwnd, msg, w, l)
            }
        }

        std::thread::spawn(|| {
            // SAFETY: a message-only window of this thread, alive as long as
            // the process; its procedure only reads its messages.
            unsafe {
                let Ok(hwnd) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("OpenCircle session watch"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    None,
                    None,
                ) else {
                    crate::log!("[opencircle] live translate: no session watch window");
                    return;
                };
                let old = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc as *const () as isize);
                OLD.store(old, Ordering::SeqCst);
                if let Err(e) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
                    crate::log!("[opencircle] live translate: no session notifications: {e}");
                }
                // (it sends the display's state at once)
                if let Err(e) = RegisterPowerSettingNotification(
                    HANDLE(hwnd.0),
                    &DISPLAY,
                    DEVICE_NOTIFY_WINDOW_HANDLE,
                ) {
                    crate::log!("[opencircle] live translate: no display notifications: {e}");
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    DispatchMessageW(&msg);
                }
            }
        });
    }

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
                crate::log!(
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
            crate::log!("no X11 display: skipped");
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

    /// #56: a lock, a user switch / disconnect or the screen going off each
    /// make the session away; it is back only when all of them are undone.
    #[test]
    fn lock_switch_and_screen_off_make_the_session_away() {
        let mut a = 0;
        a = on_session(a, WTS_SESSION_LOCK);
        assert_ne!(a, 0, "locked");
        a = on_display(a, 0);
        a = on_session(a, WTS_SESSION_UNLOCK);
        assert_ne!(a, 0, "unlocked, the screen still off");
        a = on_display(a, 2);
        assert_eq!(a, 0, "dimmed is on");
        a = on_session(a, WTS_CONSOLE_DISCONNECT);
        assert_ne!(a, 0, "another user on the console");
        a = on_session(a, 5); // a logon elsewhere: no change
        assert_ne!(a, 0);
        a = on_session(a, WTS_REMOTE_CONNECT);
        assert_eq!(a, 0, "back over Remote Desktop");
        a = on_session(on_session(a, WTS_REMOTE_DISCONNECT), WTS_CONSOLE_CONNECT);
        assert_eq!(a, 0);
        #[cfg(not(windows))]
        assert!(!away(), "nothing sets it here");
    }

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
