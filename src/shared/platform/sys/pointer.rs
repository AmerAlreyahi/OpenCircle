//! The pointer, the Ctrl key and a window's place read from anywhere on the desktop, for a live
//! pin that lets clicks pass through (it gets no input of its own then). Windows only (live
//! pins capture a Windows window): `GetCursorPos`, `GetAsyncKeyState`, `GetWindowRect`, all in
//! physical px. Elsewhere nothing can be read (`can_probe` is false) and the pin keeps its own
//! toggle and the tray row.

/// What the desktop says right now (physical desktop px).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub ctrl: bool,
    pub at: (i32, i32),
}

/// Whether `probe` can answer in this session.
pub const fn can_probe() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
pub fn probe() -> Option<Probe> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT::default();
    // SAFETY: plain out-parameter; the key read has no pointers
    unsafe { GetCursorPos(&mut p).ok()? };
    let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0; // high bit = down now
    Some(Probe {
        ctrl,
        at: (p.x, p.y),
    })
}

/// The window `hwnd`'s rectangle `[left, top, right, bottom]`; `None` once it is gone.
#[cfg(windows)]
pub fn window_rect(hwnd: isize) -> Option<[i32; 4]> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut r = RECT::default();
    // SAFETY: plain out-parameter; a stale handle just fails
    unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut r).ok()? };
    Some([r.left, r.top, r.right, r.bottom])
}

#[cfg(not(windows))]
pub fn probe() -> Option<Probe> {
    None
}

#[cfg(not(windows))]
pub fn window_rect(_: isize) -> Option<[i32; 4]> {
    None
}
