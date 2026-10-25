//! Windows UI Automation: the controls of one window (buttons, fields, panes…)
//! as rectangles, so a snip can outline them and its edges snap to them. The
//! overlay covers the screen, so `ElementFromPoint` would only find it: the
//! controls of each window under the pointer are read once instead, on a
//! thread, while Snip is open.
//! Elsewhere (X11, Wayland): none; snapping uses the window rectangles.
use super::window_rect::WinRect;

/// At most this many controls (a browser page can have thousands).
#[cfg_attr(not(windows), allow(dead_code))]
const MAX: i32 = 4000;
/// A window that hasn't answered in this long gives none (a hung app, a huge
/// page): its outline stops at the window. UIA's own default is 20 s.
#[cfg_attr(not(windows), allow(dead_code))]
const TIMEOUT_MS: u32 = 3000;

/// The controls of window `hwnd` in physical desktop px, each with what it is
/// ("button: Save"); empty on failure. Blocking (a busy app answers slowly):
/// call it off the UI thread.
pub fn controls(hwnd: isize) -> Vec<(WinRect, String)> {
    platform(hwnd).unwrap_or_else(|e| {
        eprintln!("[opencircle] no controls read from a window (UI Automation: {e})");
        Vec::new()
    })
}

#[cfg(windows)]
fn platform(hwnd: isize) -> Result<Vec<(WinRect, String)>, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomation2, TreeScope_Descendants,
        UIA_BoundingRectanglePropertyId, UIA_IsOffscreenPropertyId,
        UIA_LocalizedControlTypePropertyId, UIA_NamePropertyId,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    use windows::core::Interface;

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
        let read = || -> windows::core::Result<Vec<(WinRect, String)>> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            if let Ok(uia2) = uia.cast::<IUIAutomation2>() {
                uia2.SetConnectionTimeout(TIMEOUT_MS)?;
                uia2.SetTransactionTimeout(TIMEOUT_MS)?;
            }
            let root = uia.ElementFromHandle(hwnd)?;
            let cache = uia.CreateCacheRequest()?;
            cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
            cache.AddProperty(UIA_NamePropertyId)?;
            cache.AddProperty(UIA_LocalizedControlTypePropertyId)?;
            cache.AddProperty(UIA_IsOffscreenPropertyId)?;
            let found = root.FindAllBuildCache(
                TreeScope_Descendants,
                &uia.ControlViewCondition()?,
                &cache,
            )?;
            let mut out = Vec::new();
            for i in 0..found.Length()?.min(MAX) {
                let element = found.GetElement(i)?;
                let r = element.CachedBoundingRectangle()?;
                // scrolled out of view, a closed menu's items: not on screen
                let offscreen = element.CachedIsOffscreen().is_ok_and(|b| b.as_bool());
                if r.right > r.left && r.bottom > r.top && !offscreen {
                    let kind = element.CachedLocalizedControlType().map(|b| b.to_string());
                    let name = element.CachedName().map(|b| b.to_string());
                    let rect = (
                        r.left,
                        r.top,
                        (r.right - r.left) as u32,
                        (r.bottom - r.top) as u32,
                    );
                    let what = label(&kind.unwrap_or_default(), &name.unwrap_or_default());
                    out.push((rect, what));
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
fn platform(_hwnd: isize) -> Result<Vec<(WinRect, String)>, String> {
    Ok(Vec::new())
}

/// What a control is, for the outline's label: "button: Save", "tool bar",
/// a long name cut short; only the name when the kind is unknown (the outline
/// already says "Element: ").
#[cfg_attr(not(windows), allow(dead_code))]
fn label(kind: &str, name: &str) -> String {
    let name = name.lines().next().unwrap_or_default().trim();
    let name = match name.chars().count() {
        n if n > 32 => format!("{}…", name.chars().take(31).collect::<String>()),
        _ => name.to_string(),
    };
    match (kind, name.is_empty()) {
        ("", true) => "element".to_string(),
        ("", false) => name,
        (kind, true) => kind.to_string(),
        (kind, false) => format!("{kind}: {name}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels_say_what_the_control_is() {
        assert_eq!(super::label("button", "Save"), "button: Save");
        assert_eq!(super::label("tool bar", ""), "tool bar");
        assert_eq!(super::label("", ""), "element");
        assert_eq!(super::label("", "Home"), "Home", "not 'element: Home'");
        let long = super::label("document", &"x".repeat(50));
        assert_eq!(long.chars().count(), "document: ".len() + 32);
        assert_eq!(super::label("edit", "first\nsecond"), "edit: first");
    }
}
