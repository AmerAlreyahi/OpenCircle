//! What kind of desktop session this is. The Wayland questions live here, one
//! name each, so no caller reads the environment itself.

/// A native Wayland session: X11 calls there only see XWayland windows.
#[cfg(target_os = "linux")]
pub fn wayland_session() -> bool {
    is_wayland(
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("XDG_SESSION_TYPE").as_deref(),
        std::env::var_os("DISPLAY").as_deref(),
    )
}

/// Wayland: a Wayland socket and a session type of `wayland`, or `tty` (an ssh login
/// into a Wayland desktop), or none and no X11 display. XDG_SESSION_TYPE=x11 wins.
/// WSLg sets both displays and no session type: not Wayland for us.
#[cfg(target_os = "linux")]
fn is_wayland(
    wayland: Option<&std::ffi::OsStr>,
    session_type: Option<&std::ffi::OsStr>,
    x11: Option<&std::ffi::OsStr>,
) -> bool {
    let set = |v: Option<&std::ffi::OsStr>| v.is_some_and(|d| !d.is_empty());
    set(wayland)
        && match session_type.and_then(|t| t.to_str()) {
            Some("wayland" | "tty") => true,
            Some("x11") => false,
            _ => !set(x11),
        }
}

/// winit's own rule (0.30): Wayland whenever either variable is set and not empty.
/// Where it holds, windows can't be hidden or cut (`ui::platform::monitor`).
#[cfg(any(target_os = "linux", test))]
pub fn winit_picks_wayland(
    display: Option<&std::ffi::OsStr>,
    socket: Option<&std::ffi::OsStr>,
) -> bool {
    [display, socket]
        .into_iter()
        .any(|v| v.is_some_and(|v| !v.is_empty()))
}

/// Whether a window can be hidden and shown again. Not on Wayland: winit's
/// `set_visible(false)` is a no-op there, so a "hidden" window stays on the
/// desktop as an empty gray box. There windows are closed instead, and opened
/// again when needed.
pub fn can_hide_windows() -> bool {
    #[cfg(target_os = "linux")]
    return !winit_picks_wayland(
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_SOCKET").as_deref(),
    );
    #[cfg(not(target_os = "linux"))]
    true
}

/// Time since the user's last keyboard or mouse input (any app). Elsewhere
/// than Windows: none known, always "just now" (Wayland gives apps no idle
/// time; only the lock says the user is away there).
pub fn idle_for() -> std::time::Duration {
    // Tests never read the desk they run on: an untouched CI machine is "away".
    #[cfg(all(windows, not(test)))]
    {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is ours, its size set as the call needs.
        if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            // SAFETY: no arguments.
            let now = unsafe { GetTickCount() };
            return std::time::Duration::from_millis(now.wrapping_sub(info.dwTime).into());
        }
    }
    std::time::Duration::ZERO
}

/// The session's screen is locked. Windows: asked of the session (WTS);
/// Linux: logind's `LockedHint`, which GNOME and KDE set while locked (asked
/// with `loginctl`; none = not locked). A few ms; call it now and then, not per frame.
pub fn locked() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::System::RemoteDesktop::{
            WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSFreeMemory,
            WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
        };
        let mut buf = windows::core::PWSTR::null();
        let mut bytes = 0u32;
        // SAFETY: WTS fills `buf` with a WTSINFOEXW it allocated (freed below);
        // it is read only when the call succeeded and it is big enough.
        unsafe {
            if WTSQuerySessionInformationW(
                Some(WTS_CURRENT_SERVER_HANDLE),
                WTS_CURRENT_SESSION,
                WTSSessionInfoEx,
                &mut buf,
                &mut bytes,
            )
            .is_err()
                || buf.is_null()
            {
                return false;
            }
            let info = buf.0 as *const WTSINFOEXW;
            let locked = bytes as usize >= size_of::<WTSINFOEXW>()
                && (*info).Level == 1
                && (*info).Data.WTSInfoExLevel1.SessionFlags as u32 == WTS_SESSIONSTATE_LOCK;
            WTSFreeMemory(buf.0 as _);
            locked
        }
    }
    #[cfg(target_os = "linux")]
    {
        let session = std::env::var("XDG_SESSION_ID").unwrap_or_else(|_| "auto".into());
        let out = std::process::Command::new("loginctl")
            .args(["show-session", &session, "-p", "LockedHint", "--value"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        out.is_ok_and(|o| o.status.success() && o.stdout.trim_ascii() == b"yes")
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_a_wayland_session() {
        use std::ffi::OsStr;
        let w = Some(OsStr::new("wayland-0"));
        let x = Some(OsStr::new(":0"));
        let t = |s| Some(OsStr::new(s));
        assert!(is_wayland(w, t("wayland"), x));
        assert!(!is_wayland(w, t("x11"), None));
        assert!(!is_wayland(w, None, x)); // WSLg: no session type, both displays
        assert!(!is_wayland(None, t("wayland"), x));
        assert!(!is_wayland(Some(OsStr::new("")), t("wayland"), x));
        // Started from ssh or systemd-run: type tty or unset, only the Wayland socket.
        assert!(is_wayland(w, t("tty"), None));
        assert!(is_wayland(w, None, None));
        assert!(is_wayland(w, t("tty"), Some(OsStr::new(""))));
        assert!(
            is_wayland(w, t("tty"), x),
            "ssh keeps DISPLAY=:0 of XWayland"
        );
        assert!(!is_wayland(None, t("tty"), x));
    }

    #[test]
    fn windows_hide_except_where_winit_runs_on_wayland() {
        use std::ffi::OsStr;
        let set = Some(OsStr::new("wayland-0"));
        let empty = Some(OsStr::new(""));
        assert!(winit_picks_wayland(set, None));
        assert!(winit_picks_wayland(None, set));
        assert!(winit_picks_wayland(empty, set));
        assert!(!winit_picks_wayland(None, None));
        assert!(!winit_picks_wayland(empty, empty));
    }
}
