//! Monitors as windows meet them: where a monitor sits (`Place`), putting the overlay
//! over one, the taskbar's share of its bottom, and whether windows can hide at all.

use iced::window;
use iced::{Point, Size, Task};

pub use crate::sys::session::can_hide_windows;

/// A monitor as the overlay shows it: its origin and size in physical desktop
/// px, and physical px per logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub origin: (i32, i32),
    pub px: (u32, u32),
    pub scale: f32,
}

impl Place {
    /// Its logical size (the overlay's).
    pub fn size(&self) -> Size {
        Size::new(self.px.0 as f32, self.px.1 as f32) * (1.0 / self.scale)
    }

    /// Its origin in logical px (the overlay window's position).
    pub fn logical_origin(&self) -> Point {
        Point::new(
            self.origin.0 as f32 / self.scale,
            self.origin.1 as f32 / self.scale,
        )
    }
}

/// THE way to resize one of our windows at run time (a test forbids
/// `window::resize` elsewhere). Wayland: winit applies a program's resize without
/// a `Resized` event, so iced keeps drawing the old size and the compositor
/// stretches it; a window whose content depends on its size is replaced by a new
/// one instead (`Windows::place_steps`, `update_card`). Every other caller goes
/// through here so the next one has one place to fix.
pub fn resize<T: Send + 'static>(id: window::Id, size: Size) -> Task<T> {
    // Wayland: our windows aren't resizable, which pins the compositor's min and
    // max size to the old one (xdg_toplevel), so a bigger buffer is clipped or
    // scaled back; the hints move to the new size first.
    if !can_hide_windows() {
        return window::set_min_size(id, Some(size))
            .chain(window::set_max_size(id, Some(size)))
            .chain(window::resize(id, size));
    }
    window::resize(id, size)
}

/// Puts the overlay window over the monitor at `place`, so full-screen (which
/// covers the window's own monitor) lands there. Windows: in physical px; iced
/// moves in logical px at the window's current scale, another monitor's when
/// their scales differ.
pub fn place_window<T: Send + 'static>(id: window::Id, place: Place) -> Task<T> {
    #[cfg(windows)]
    return window::run(id, move |w| {
        use ::windows::Win32::Foundation::HWND;
        use ::windows::Win32::UI::WindowsAndMessaging::{
            SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        };
        use iced::window::raw_window_handle::RawWindowHandle;
        let Ok(handle) = w.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(h) = handle.as_raw() else {
            return;
        };
        let ((x, y), (width, height)) = (place.origin, place.px);
        // Twice: a move onto a monitor of another scale makes winit resize the
        // window for it (WM_DPICHANGED); the second move, at the new scale, sticks.
        for _ in 0..2 {
            // SAFETY: `h` is this window's live handle (an iced window of ours).
            let moved = unsafe {
                SetWindowPos(
                    HWND(h.hwnd.get() as *mut _),
                    None,
                    x,
                    y,
                    width as i32,
                    height as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            if let Err(e) = moved {
                crate::log!("[opencircle] overlay not moved: {e}");
            }
        }
    })
    .discard();
    #[cfg(not(windows))]
    window::move_to(id, place.logical_origin()).chain(resize(id, place.size()))
}

/// How much of the bottom of the monitor at `origin` (`size`, logical px at
/// `scale`) the taskbar takes: the bottom bar keeps above it.
// ponytail: the bottom edge only (a taskbar on the left, right or top
// doesn't push the bar); mixed-DPI monitors map approximately.
#[cfg(windows)]
pub fn taskbar_inset(origin: Point, size: Size, scale: f32) -> f32 {
    use ::windows::Win32::Foundation::POINT;
    use ::windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    let centre = POINT {
        x: ((origin.x + size.width / 2.0) * scale) as i32,
        y: ((origin.y + size.height / 2.0) * scale) as i32,
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: plain Win32 queries; `info` is sized as cbSize says.
    let read = unsafe {
        let monitor = MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info).as_bool()
    };
    let band = [
        info.rcMonitor.left,
        info.rcWork.bottom,
        info.rcMonitor.right,
        info.rcMonitor.bottom,
    ];
    match read && !covered(band) {
        true => ((info.rcMonitor.bottom - info.rcWork.bottom) as f32 / scale).max(0.0),
        false => 0.0,
    }
}

/// The taskbar's band (`[left, top, right, bottom]`, physical px) is under
/// another app's window: a game or a picture full screen (F11) covers it, so
/// the frame has no taskbar there (one-shot translate left a game's bottom
/// row out as "the taskbar", Win10 VM). The top window over the band that
/// isn't ours decides: the taskbar itself, or one spanning the whole band.
#[cfg(windows)]
fn covered(band: [i32; 4]) -> bool {
    use ::windows::Win32::Foundation::RECT;
    use ::windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use ::windows::Win32::System::Threading::GetCurrentProcessId;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GW_HWNDNEXT, GetClassNameW, GetTopWindow, GetWindow, GetWindowRect,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    };
    if band[3] <= band[1] {
        return false;
    }
    // SAFETY (whole block): plain queries on handles the system just gave
    // us; the buffers outlive the calls.
    unsafe {
        let me = GetCurrentProcessId();
        let mut next = GetTopWindow(None).ok();
        while let Some(h) = next {
            next = GetWindow(h, GW_HWNDNEXT).ok();
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            let mut r = RECT::default();
            // (a suspended Store app's window is "visible" but cloaked)
            let mut cloaked = 0u32;
            let cloak = DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast::<std::ffi::c_void>(),
                size_of::<u32>() as u32,
            );
            let shown = IsWindowVisible(h).as_bool()
                && !IsIconic(h).as_bool()
                && !(cloak.is_ok() && cloaked != 0);
            if pid == me || !shown || GetWindowRect(h, &mut r).is_err() {
                continue;
            }
            let mut b = [0u16; 32];
            let n = GetClassNameW(h, &mut b).max(0) as usize;
            let class = String::from_utf16_lossy(&b[..n]);
            if let Some(hides) = over_band(&class, [r.left, r.top, r.right, r.bottom], band) {
                return hides;
            }
        }
        false
    }
}

/// One window (top first, `rect` = `[left, top, right, bottom]`) against the
/// taskbar's band: `Some(false)` = the taskbar is on top, `Some(true)` = a
/// window spanning the band hides it, `None` = not over it, or smaller (a
/// tooltip, a helper): the next window down decides.
#[cfg_attr(not(windows), allow(dead_code))]
fn over_band(class: &str, rect: [i32; 4], band: [i32; 4]) -> Option<bool> {
    let [l, t, r, b] = rect;
    if !(l < band[2] && r > band[0] && t < band[3] && b > band[1]) {
        return None;
    }
    if matches!(class, "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return Some(false);
    }
    (l <= band[0] && r >= band[2] && t <= band[1] && b >= band[3]).then_some(true)
}

/// Elsewhere the desktop's panels aren't known: the bar keeps its own gap.
#[cfg(not(windows))]
pub fn taskbar_inset(_: Point, _: Size, _: f32) -> f32 {
    0.0
}

#[cfg(test)]
mod tests {
    use super::over_band;

    /// A 1920x1080 monitor, a 40 px taskbar band at the bottom.
    const BAND: [i32; 4] = [0, 1040, 1920, 1080];

    #[test]
    fn a_full_screen_window_hides_the_taskbar() {
        // a game or a picture at F11 over the band (Win10 VM): no taskbar
        // in the frame, the bottom row is the game's
        let game = over_band("WindowsForms10", [0, 0, 1920, 1080], BAND);
        assert_eq!(game, Some(true));
        let bigger = over_band("UnrealWindow", [-8, -8, 1928, 1088], BAND);
        assert_eq!(bigger, Some(true));
        // the taskbar on top of it all, as on a desktop
        let bar = over_band("Shell_TrayWnd", [0, 1040, 1920, 1080], BAND);
        assert_eq!(bar, Some(false));
        // a maximised window ends above the band; a tooltip is too small
        let max = over_band("Chrome_WidgetWin_1", [0, 0, 1920, 1040], BAND);
        assert_eq!(max, None);
        let tip = over_band("tooltips_class32", [900, 1030, 1100, 1060], BAND);
        assert_eq!(tip, None);
    }
}
