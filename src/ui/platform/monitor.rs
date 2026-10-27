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

    pub(in crate::ui) fn contains(&self, (x, y): (i32, i32)) -> bool {
        let (ox, oy) = (i64::from(self.origin.0), i64::from(self.origin.1));
        (ox..ox + i64::from(self.px.0)).contains(&i64::from(x))
            && (oy..oy + i64::from(self.px.1)).contains(&i64::from(y))
    }
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
    window::move_to(id, place.logical_origin()).chain(window::resize(id, place.size()))
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
    match read {
        true => ((info.rcMonitor.bottom - info.rcWork.bottom) as f32 / scale).max(0.0),
        false => 0.0,
    }
}

/// Elsewhere the desktop's panels aren't known: the bar keeps its own gap.
#[cfg(not(windows))]
pub fn taskbar_inset(_: Point, _: Size, _: f32) -> f32 {
    0.0
}
