//! Monitors as windows meet them: where a monitor sits (`Place`), putting the overlay
//! over one, the taskbar's share of its bottom, and whether windows can hide at all.

use iced::window;
use iced::{Point, Size, Task};

pub use crate::shared::platform::sys::session::can_hide_windows;

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

/// Every monitor as a `Place` (empty where the platform can't list them).
pub fn places() -> Vec<Place> {
    (crate::shared::platform::capture::monitor_list().iter())
        .map(|m| Place {
            origin: (m.rect.0, m.rect.1),
            px: (m.rect.2, m.rect.3),
            scale: m.scale.max(0.1),
        })
        .collect()
}

/// The monitor holding the middle of a window at `at` (`size`), logical px at
/// the window's own `scale` (`window::scale_factor`: it changes as the window
/// crosses to a monitor of another DPI, so ask after it stopped). THE way a
/// floating widget finds the screen it was dragged to (the Steps strip, the
/// pinned chat); `None`: none holds it, it stays where it was docked.
pub fn holding(places: &[Place], at: Point, size: Size, scale: f32) -> Option<Place> {
    let x = ((at.x + size.width / 2.0) * scale).round() as i32;
    let y = ((at.y + size.height / 2.0) * scale).round() as i32;
    places.iter().copied().find(|m| {
        let (ox, oy) = m.origin;
        (ox..ox + m.px.0 as i32).contains(&x) && (oy..oy + m.px.1 as i32).contains(&y)
    })
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
    #[cfg(test)]
    PLACED.with(|p| p.borrow_mut().push((id, place)));
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
            // landed (no scale change): a second call would only cost a frame of
            // messages per step of a resize drag (W61)
            let mut now = ::windows::Win32::Foundation::RECT::default();
            // SAFETY: as above.
            let read = unsafe {
                ::windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                    HWND(h.hwnd.get() as *mut _),
                    &mut now,
                )
            };
            let want = (x, y, x + width as i32, y + height as i32);
            if read.is_ok() && (now.left, now.top, now.right, now.bottom) == want {
                break;
            }
        }
    })
    .discard();
    // X11: one request (`shape::place`); else moved, then sized
    #[cfg(not(windows))]
    {
        let radius = crate::shared::platform::windows::FLOATING
            .lock()
            .unwrap()
            .iter()
            .find(|(f, ..)| *f == id)
            .map(|(_, r, _)| *r);
        window::run(id, move |w| {
            let handle = w.window_handle().ok()?;
            let placed = crate::shared::platform::shape::place(
                handle.as_raw(),
                place.origin,
                place.px,
                radius,
            );
            placed
                .inspect_err(|e| crate::log!("[opencircle] window not placed: {e}"))
                .ok()
        })
        .then(move |done| match done {
            Some(true) => Task::none(),
            _ => window::move_to(id, place.logical_origin()).chain(resize(id, place.size())),
        })
    }
}

/// How much of the bottom of the monitor at `origin` (`size`, logical px at
/// `scale`) the taskbar takes: the bottom bar keeps above it.
// ponytail: the bottom edge only (a taskbar on the left, right or top
// doesn't push the bar); mixed-DPI monitors map approximately.
#[cfg(windows)]
pub fn taskbar_inset(origin: Point, size: Size, scale: f32) -> f32 {
    let Some((m, w)) = monitor_rects(origin, size, scale) else {
        return 0.0;
    };
    let band = [m[0], w[3], m[2], m[3]];
    match !covered(band) {
        true => ((m[3] - w[3]) as f32 / scale).max(0.0),
        false => 0.0,
    }
}

/// The monitor holding the middle of the window at `origin` (`size`, logical px at `scale`) and
/// its work area, as `[left, top, right, bottom]` physical px; `None` when Windows won't say.
#[cfg(windows)]
fn monitor_rects(origin: Point, size: Size, scale: f32) -> Option<([i32; 4], [i32; 4])> {
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
    let rect = |r: ::windows::Win32::Foundation::RECT| [r.left, r.top, r.right, r.bottom];
    read.then(|| (rect(info.rcMonitor), rect(info.rcWork)))
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

/// The air a normal window keeps from its monitor's work area (logical px).
pub const WINDOW_MARGIN: f32 = 24.0;

/// A normal window's size, smallest size and top-left corner (logical px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub size: Size,
    pub min: Size,
    pub at: Point,
}

/// `want` (and `min`) shrunk to fit a work area of `work` at `work_at` with a
/// margin all round, centred in it. The work area is what the monitor leaves after
/// the taskbar / panels: a window taller than it opened with its top above the
/// screen (Studio, 1280x800 on a 1280x720 logical screen). Below `min` only a work area smaller
/// than `min` shrinks it (then the window is the whole work area).
pub fn fit_window(work_at: Point, work: Size, want: Size, min: Size) -> Fit {
    let room = Size::new(
        (work.width - 2.0 * WINDOW_MARGIN).max(1.0),
        (work.height - 2.0 * WINDOW_MARGIN).max(1.0),
    );
    // the margin never squeezes it below `min` while the work area itself holds `min`
    let floor = Size::new(min.width.min(work.width), min.height.min(work.height));
    let size = Size::new(
        want.width.min(room.width).max(floor.width),
        want.height.min(room.height).max(floor.height),
    );
    Fit {
        size,
        min: Size::new(min.width.min(size.width), min.height.min(size.height)),
        at: Point::new(
            work_at.x + (work.width - size.width) / 2.0,
            work_at.y + (work.height - size.height) / 2.0,
        ),
    }
}

/// What the panels take from a monitor: `[left, top, right, bottom]` of `monitor` (physical px,
/// `[left, top, right, bottom]`) that lie outside `work` (the desktop's work area, which may be
/// bigger than the monitor: only its intersection counts).
fn insets_of(monitor: [i32; 4], work: [i32; 4]) -> [i32; 4] {
    [
        (work[0] - monitor[0]).max(0),
        (work[1] - monitor[1]).max(0),
        (monitor[2] - work[2]).max(0),
        (monitor[3] - work[3]).max(0),
    ]
}

/// What the taskbar and panels take from the monitor at `origin` (`size`, logical px at
/// `scale`): `[left, top, right, bottom]` logical px. Windows: the monitor's work area (any
/// edge); X11: `_NET_WORKAREA`; Wayland (the compositor places windows) and any failure: none.
pub fn work_insets(origin: Point, size: Size, scale: f32) -> [f32; 4] {
    #[cfg(windows)]
    let px = monitor_rects(origin, size, scale).map(|(m, w)| insets_of(m, w));
    #[cfg(target_os = "linux")]
    let px = {
        let at = |v: f32| (v * scale).round() as i32;
        let m = [
            at(origin.x),
            at(origin.y),
            at(origin.x + size.width),
            at(origin.y + size.height),
        ];
        (can_hide_windows())
            .then(x11_work_area)
            .flatten()
            .map(|w| insets_of(m, w))
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    let px: Option<[i32; 4]> = {
        let _ = (origin, size);
        None
    };
    px.map_or([0.0; 4], |i| i.map(|v| v as f32 / scale))
}

/// X11's `_NET_WORKAREA` of the current desktop: `[left, top, right, bottom]` px, over every
/// monitor together (panels' struts cut it); `None` when the window manager doesn't set it.
#[cfg(target_os = "linux")]
fn x11_work_area() -> Option<[i32; 4]> {
    use xcb::{Xid, x};
    let (conn, n) = xcb::Connection::connect(None).ok()?;
    let root = conn.get_setup().roots().nth(n as usize)?.root();
    let atom = |name: &[u8]| {
        let asked = conn.send_request(&x::InternAtom {
            only_if_exists: true,
            name,
        });
        conn.wait_for_reply(asked).ok().map(|r| r.atom())
    };
    let cardinals = |property: x::Atom, length: u32| {
        let asked = conn.send_request(&x::GetProperty {
            delete: false,
            window: root,
            property,
            r#type: x::ATOM_CARDINAL,
            long_offset: 0,
            long_length: length,
        });
        conn.wait_for_reply(asked)
            .ok()
            .map(|r| r.value::<u32>().to_vec())
    };
    let desktop = atom(b"_NET_CURRENT_DESKTOP")
        .filter(|a| !a.is_none())
        .and_then(|a| cardinals(a, 1))
        .and_then(|v| v.first().copied())
        .unwrap_or(0) as usize;
    let all = cardinals(atom(b"_NET_WORKAREA").filter(|a| !a.is_none())?, 4 * 32)?;
    let w = all
        .get(4 * desktop..4 * desktop + 4)
        .or_else(|| all.get(..4))?;
    let (x, y, width, height) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    (width > 0 && height > 0).then_some([x, y, x + width, y + height])
}

#[cfg(test)]
thread_local! {
    /// Tests: every `place_window` asked (the window moved or sized).
    pub static PLACED: std::cell::RefCell<Vec<(window::Id, Place)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use super::{Place, holding, over_band};
    use iced::{Point, Size};

    /// A 4K monitor at 200 % (left of the origin) and a 1080p one at 100 %.
    fn two() -> [Place; 2] {
        [
            Place {
                origin: (0, 0),
                px: (1920, 1080),
                scale: 1.0,
            },
            Place {
                origin: (-3840, 0),
                px: (3840, 2160),
                scale: 2.0,
            },
        ]
    }

    #[test]
    fn fit_window_shrinks_to_the_work_area_and_centres_inside_it() {
        let min = Size::new(960.0, 600.0);
        // 1280x720 logical screen, 40 px taskbar at the bottom: a 1280x800 window
        let f = super::fit_window(
            Point::new(0.0, 0.0),
            Size::new(1280.0, 680.0),
            Size::new(1280.0, 800.0),
            min,
        );
        assert_eq!(f.size, Size::new(1232.0, 632.0));
        assert_eq!(f.at, Point::new(24.0, 24.0));
        assert!(f.at.y >= 0.0 && f.at.y + f.size.height <= 680.0);
        // the smallest size never exceeds what fits
        let tiny = super::fit_window(
            Point::new(0.0, 0.0),
            Size::new(800.0, 500.0),
            Size::new(1280.0, 800.0),
            min,
        );
        assert_eq!(tiny.size, Size::new(800.0, 500.0), "the whole work area");
        assert_eq!(tiny.min, Size::new(800.0, 500.0));
    }

    #[test]
    fn fit_window_on_a_roomy_monitor_keeps_the_size_and_centres_on_that_monitor() {
        // a second monitor right of the first, its work area below a top bar
        let f = super::fit_window(
            Point::new(1920.0, 30.0),
            Size::new(1920.0, 1010.0),
            Size::new(1280.0, 800.0),
            Size::new(960.0, 600.0),
        );
        assert_eq!(f.size, Size::new(1280.0, 800.0));
        assert_eq!(f.at, Point::new(1920.0 + 320.0, 30.0 + 105.0));
    }

    #[test]
    fn the_margin_never_squeezes_the_window_below_its_smallest_size() {
        let min = Size::new(960.0, 600.0);
        let want = Size::new(1280.0, 800.0);
        // the work area holds the smallest size but not with the margin: the smallest size
        let f = super::fit_window(Point::new(0.0, 0.0), Size::new(1000.0, 640.0), want, min);
        assert_eq!((f.size, f.min), (min, min));
        assert_eq!(f.at, Point::new(20.0, 20.0));
        // a work area smaller than the smallest size: the whole work area
        let f = super::fit_window(Point::new(0.0, 0.0), Size::new(900.0, 560.0), want, min);
        assert_eq!(f.size, Size::new(900.0, 560.0));
        assert_eq!(f.at, Point::new(0.0, 0.0));
    }

    #[test]
    fn insets_are_what_the_work_area_leaves_out_of_the_monitor() {
        use super::insets_of;
        // a bottom taskbar
        assert_eq!(
            insets_of([0, 0, 1920, 1080], [0, 0, 1920, 1040]),
            [0, 0, 0, 40]
        );
        // a left dock and a top bar on the second of two monitors; the work area spans both
        assert_eq!(
            insets_of([1920, 0, 3840, 1080], [64, 28, 3840, 1080]),
            [0, 28, 0, 0],
            "the dock is on the first monitor only: its left strut is outside this one"
        );
        assert_eq!(
            insets_of([0, 0, 1920, 1080], [64, 28, 3840, 1080]),
            [64, 28, 0, 0],
            "the right edge of a work area wider than the monitor takes nothing"
        );
        // a work area that is bigger or equal: nothing taken
        assert_eq!(insets_of([0, 0, 800, 600], [0, 0, 800, 600]), [0; 4]);
        assert_eq!(insets_of([0, 0, 800, 600], [-10, -10, 900, 700]), [0; 4]);
    }

    #[test]
    fn a_widget_is_on_the_monitor_holding_its_middle_at_its_own_scale() {
        let m = two();
        let size = Size::new(400.0, 300.0);
        // on the 1080p one (logical = physical)
        assert_eq!(
            holding(&m, Point::new(1000.0, 100.0), size, 1.0),
            Some(m[0])
        );
        // on the 4K one at 200 %: logical x -1500 is physical -3000
        assert_eq!(
            holding(&m, Point::new(-1500.0, 100.0), size, 2.0),
            Some(m[1])
        );
        // the same numbers at 100 % are another spot: x 1500 at 200 % is off both
        assert_eq!(holding(&m, Point::new(3000.0, 100.0), size, 2.0), None);
        // half over the bezel: the middle decides
        assert_eq!(
            holding(&m, Point::new(-250.0, 100.0), size, 2.0),
            Some(m[1])
        );
        assert_eq!(
            holding(&m, Point::new(-150.0, 100.0), size, 1.0),
            Some(m[0])
        );
        assert_eq!(holding(&m, Point::new(9000.0, 9000.0), size, 1.0), None);
    }

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
