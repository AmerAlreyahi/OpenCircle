//! Drag-out: drag the flattened image from the editor (or a pin) into another
//! app, e.g. a browser's AI chat. The image goes as a temp PNG file (what web
//! pages and file targets take) plus a bitmap (what WordPad or Paint take).
//!
//! Windows: the shell's own data object for the file (CF_HDROP and friends) with
//! a CF_DIB added, dragged by `SHDoDragDrop` (default drop source: no COM objects
//! of ours). Linux: not yet. X11 needs a whole XDND source (own the selection,
//! track the windows under the pointer, answer their position/status messages)
//! beside winit's connection; Wayland gives drags only to the window's own
//! toolkit. Until then the toolbar shows no drag handle there (`SUPPORTED`).
//!
//! The editor sits on the full-screen overlay, which would be the only window
//! under the pointer: `start(.., hide_source)` hides the window the drag starts
//! in (the overlay) for the drag and shows it again after it; the host then keeps
//! the editor (nothing took the drop) or closes the overlay the normal way.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::features::circle::actions::Crop;

/// The platform can drag an image out.
pub const SUPPORTED: bool = cfg!(windows);

/// A temp PNG of the image being dragged; deleted when dropped (the editor or
/// pin that made it closes, or the next drag replaces it). Once an app took the
/// drop it may still be reading the file (a browser uploads it after the drop),
/// so then it goes `GRACE` later. Quitting within `GRACE` deletes it at once
/// (`clear_own`); a crash leaves it to the next start's sweep
/// (`search_in_browser::page::sweep_stale`).
pub struct DragFile {
    path: PathBuf,
    /// An app took it: delete after `GRACE`.
    pub dropped: bool,
}

/// How long a dropped file stays for the app that took it.
pub const GRACE: std::time::Duration = std::time::Duration::from_secs(30);

impl DragFile {
    pub fn write(crop: &Crop) -> Result<Self, String> {
        Self::temp(crop, "drag")
    }

    /// `crop` as `opencircle-<what>-<pid>-<n>.png` in the temp folder (the
    /// share sheet's file too: same life, same grace).
    pub fn temp(crop: &Crop, what: &str) -> Result<Self, String> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("opencircle-{what}-{}-{n}.png", std::process::id());
        let path = std::env::temp_dir().join(name);
        crate::features::circle::actions::write_png(&path, crop.width, crop.height, &crop.rgba)?;
        Ok(DragFile {
            path,
            dropped: false,
        })
    }

    #[cfg_attr(not(windows), allow(dead_code))] // read by the Windows drag-out and share
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Deletes every drag-out / Share PNG of this process now, waiting ones too.
/// Call when the app quits (the grace threads die with it).
pub fn clear_own() {
    clear_own_in(&std::env::temp_dir());
}

fn clear_own_in(dir: &Path) {
    let pid = std::process::id();
    let ours = ["drag", "share"].map(|what| format!("opencircle-{what}-{pid}-"));
    crate::features::circle::actions::search_in_browser::page::sweep_dir(
        dir,
        &ours,
        std::time::Duration::ZERO,
    );
}

impl Drop for DragFile {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.path);
        if self.dropped {
            std::thread::spawn(move || {
                std::thread::sleep(GRACE);
                let _ = std::fs::remove_file(path);
            });
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A packed bottom-up 32-bit DIB (BITMAPINFOHEADER + BGRA rows) of `crop`.
#[cfg_attr(not(windows), allow(dead_code))] // Windows clipboard/drag format
pub fn dib(crop: &Crop) -> Vec<u8> {
    let (w, h) = (crop.width as usize, crop.height as usize);
    let mut out = Vec::with_capacity(40 + w * h * 4);
    for v in [40u32, crop.width, crop.height] {
        out.extend_from_slice(&v.to_le_bytes()); // biSize, biWidth, biHeight (+ = bottom-up)
    }
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&((w * h * 4) as u32).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&[0; 16]); // resolution, colours used / important
    for row in crop.rgba.chunks_exact(w * 4).rev() {
        for px in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    out
}

#[test]
fn quit_deletes_only_this_process_files() {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("oc-dragout-quit-{pid}"));
    std::fs::create_dir_all(&dir).unwrap();
    let mk = |name: String| {
        let p = dir.join(name);
        std::fs::write(&p, b"png").unwrap();
        p
    };
    let drag = mk(format!("opencircle-drag-{pid}-7.png"));
    let share = mk(format!("opencircle-share-{pid}-7.png"));
    let other = mk(format!("opencircle-drag-{}-7.png", pid + 1)); // a live instance's
    clear_own_in(&dir);
    assert!(!drag.exists() && !share.exists(), "ours go, grace or not");
    assert!(other.exists(), "another process's file stays");
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(test)]
thread_local! {
    /// What the next `start` answers in a test (the OS drag needs a real mouse).
    pub static TEST_DROP: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Runs the OS drag of `file` (and `crop` as a bitmap) until the drop. Call it
/// on the UI thread while the left button is down; it returns after the drop:
/// `true` = an app took it (`file` is marked dropped). `hide_source`: the active
/// window hides for the drag (the overlay) and comes back after it.
pub fn start(file: &mut DragFile, crop: &Crop, hide_source: bool) -> Result<bool, String> {
    #[cfg(test)]
    if let Some(dropped) = TEST_DROP.take() {
        file.dropped = dropped;
        return Ok(dropped);
    }
    drag(file, crop, hide_source)
}

#[cfg(windows)]
fn drag(file: &mut DragFile, crop: &Crop, hide_source: bool) -> Result<bool, String> {
    use std::mem::ManuallyDrop;
    use windows::Win32::Foundation::{HGLOBAL, POINT};
    use windows::Win32::System::Com::{
        DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{
        CF_DIB, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropSource, OleInitialize, OleUninitialize,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{DragDetect, GetActiveWindow};
    use windows::Win32::UI::Shell::{
        BHID_DataObject, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SW_HIDE, SW_SHOW, ShowWindow};
    use windows::core::HSTRING;

    if cfg!(test) {
        // The OS drag waits for a real mouse: never in a test run (CI hangs).
        return Err("drag-out: no OS drag in tests".into());
    }
    let e =
        |what: &'static str| move |err: windows::core::Error| format!("drag-out: {what}: {err}");
    // winit already initialised OLE on this thread (it is a drop target); this
    // only counts up, and is balanced below.
    let ole = unsafe { OleInitialize(None) }.is_ok();
    let result = (|| unsafe {
        let path = HSTRING::from(file.path().as_os_str());
        let item: IShellItem = SHCreateItemFromParsingName(&path, None).map_err(e("file"))?;
        let data: IDataObject = item
            .BindToHandler(None, &BHID_DataObject)
            .map_err(e("data object"))?;

        let bits = dib(crop);
        let global: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bits.len()).map_err(e("memory"))?;
        let dst = GlobalLock(global) as *mut u8;
        if !dst.is_null() {
            std::ptr::copy_nonoverlapping(bits.as_ptr(), dst, bits.len());
            let _ = GlobalUnlock(global);
            let format = FORMATETC {
                cfFormat: CF_DIB.0,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            };
            let medium = STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: global },
                pUnkForRelease: ManuallyDrop::new(None),
            };
            // fRelease: the data object owns the memory from here. Without the
            // bitmap the file alone still drags.
            if let Err(err) = data.SetData(&format, &medium, true) {
                crate::log!("[opencircle] drag-out: no bitmap ({err})");
            }
        }
        let window = GetActiveWindow();
        let window = (!window.is_invalid()).then_some(window);
        // A press that lets go before the system drag distance (SM_CXDRAG /
        // SM_CYDRAG, 4 px at 100 %) is a click: nothing is dragged (a drop
        // would land in the app under the hidden overlay).
        if let Some(w) = window {
            let mut at = POINT::default();
            if GetCursorPos(&mut at).is_ok() && !DragDetect(w, at).as_bool() {
                return Ok(false);
            }
        }
        // The drag's own window and every other screen's frozen overlay: a screen
        // we left visible would swallow a drop meant for the app behind it.
        let hidden = if hide_source {
            window.into_iter().chain(other_overlays(window)).collect()
        } else {
            Vec::new()
        };
        for w in &hidden {
            let _ = ShowWindow(*w, SW_HIDE);
        }
        let effect =
            SHDoDragDrop(window, &data, None::<&IDropSource>, DROPEFFECT_COPY).map_err(e("drag"));
        let dropped = effect.as_ref().is_ok_and(|e| *e != DROPEFFECT_NONE);
        // Always back: iced and winit still count the window as shown, and the host
        // decides what happens next (the overlay stays up, or closes by its own
        // steps, which assume a visible window; a window hidden behind their back
        // was left half-hidden with its toolbar on screen).
        for w in hidden {
            let _ = ShowWindow(w, SW_SHOW);
        }
        effect.map(|_| dropped)
    })();
    if ole {
        unsafe { OleUninitialize() };
    }
    if result == Ok(true) {
        file.dropped = true;
    }
    result
}

/// The visible windows of this thread that fill their whole monitor (the
/// frozen overlays), except `skip`.
#[cfg(windows)]
fn other_overlays(
    skip: Option<windows::Win32::Foundation::HWND>,
) -> Vec<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetWindowRect, IsWindowVisible,
    };
    use windows::core::BOOL;

    unsafe extern "system" fn each(w: HWND, out: LPARAM) -> BOOL {
        unsafe {
            let (mut r, mut mi) = (RECT::default(), MONITORINFO::default());
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            let m = MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST);
            if IsWindowVisible(w).as_bool()
                && GetWindowRect(w, &mut r).is_ok()
                && GetMonitorInfoW(m, &mut mi).as_bool()
                && covers(
                    [r.left, r.top, r.right, r.bottom],
                    [
                        mi.rcMonitor.left,
                        mi.rcMonitor.top,
                        mi.rcMonitor.right,
                        mi.rcMonitor.bottom,
                    ],
                )
            {
                (*(out.0 as *mut Vec<HWND>)).push(w);
            }
        }
        BOOL(1)
    }
    let mut found = Vec::new();
    unsafe {
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(each),
            LPARAM(&mut found as *mut _ as isize),
        );
    }
    found.retain(|w| Some(*w) != skip);
    found
}

/// `win` (l, t, r, b) fills `monitor` (a frozen overlay, not a toolbar).
#[cfg_attr(not(windows), allow(dead_code))]
fn covers(win: [i32; 4], monitor: [i32; 4]) -> bool {
    win[0] <= monitor[0] && win[1] <= monitor[1] && win[2] >= monitor[2] && win[3] >= monitor[3]
}

#[test]
fn only_a_window_filling_its_screen_counts_as_an_overlay() {
    let screen = [1920, 0, 3840, 1080];
    assert!(covers([1920, 0, 3840, 1080], screen));
    assert!(covers([1919, -1, 3841, 1081], screen)); // a pixel of slack
    assert!(!covers([2000, 900, 2400, 1000], screen)); // a toolbar
}

#[cfg(not(windows))]
fn drag(_file: &mut DragFile, _crop: &Crop, _hide_source: bool) -> Result<bool, String> {
    Err("drag-out is Windows-only for now".into())
}
