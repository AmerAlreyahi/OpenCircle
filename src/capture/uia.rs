//! Windows UI Automation: the controls of one window (buttons, fields, panes…)
//! as rectangles, so a snip's edges can snap to them. The overlay covers the
//! screen, so `ElementFromPoint` would only find it: the controls of the window
//! the user was in are read once instead, on a thread, while Snip is open.
//! Elsewhere (X11, Wayland): none; snapping uses the window rectangles.
use super::window_rect::WinRect;

/// At most this many controls (a browser page can have thousands).
#[cfg_attr(not(windows), allow(dead_code))]
const MAX: i32 = 4000;

/// The controls of window `hwnd` in physical desktop px; empty on failure.
/// Blocking (a busy app answers slowly): call it off the UI thread.
pub fn controls(hwnd: isize) -> Vec<WinRect> {
    platform(hwnd).unwrap_or_else(|e| {
        eprintln!("[opencircle] UI Automation unavailable: {e}");
        Vec::new()
    })
}

#[cfg(windows)]
fn platform(hwnd: isize) -> Result<Vec<WinRect>, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_BoundingRectanglePropertyId,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let os = |e: windows::core::Error| e.to_string();
    let hwnd = HWND(hwnd as *mut _);
    let mut pid = 0;
    // SAFETY: a plain query on a window handle.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == std::process::id() {
        return Ok(Vec::new()); // our own window: UIA would wait on our UI thread
    }
    // SAFETY: COM on this (short-lived, own) thread; every interface is dropped
    // before CoUninitialize.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(os)?;
        let read = || -> windows::core::Result<Vec<WinRect>> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let root = uia.ElementFromHandle(hwnd)?;
            let cache = uia.CreateCacheRequest()?;
            cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
            let found = root.FindAllBuildCache(
                TreeScope_Descendants,
                &uia.ControlViewCondition()?,
                &cache,
            )?;
            let mut out = Vec::new();
            for i in 0..found.Length()?.min(MAX) {
                let r = found.GetElement(i)?.CachedBoundingRectangle()?;
                if r.right > r.left && r.bottom > r.top {
                    out.push((
                        r.left,
                        r.top,
                        (r.right - r.left) as u32,
                        (r.bottom - r.top) as u32,
                    ));
                }
            }
            Ok(out)
        };
        let out = read().map_err(os);
        CoUninitialize();
        out
    }
}

#[cfg(not(windows))]
fn platform(_hwnd: isize) -> Result<Vec<WinRect>, String> {
    Ok(Vec::new())
}
