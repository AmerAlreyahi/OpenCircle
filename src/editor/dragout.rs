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
//! in (the overlay) for the drag, and shows it again if nothing took the drop.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::actions::Crop;

/// The platform can drag an image out.
pub const SUPPORTED: bool = cfg!(windows);

/// A temp PNG of the image being dragged; deleted when dropped (the editor or
/// pin that made it closes, or the next drag replaces it). Once an app took the
/// drop it may still be reading the file (a browser uploads it after the drop),
/// so then it goes `GRACE` later.
// ponytail: a crash, or quitting within GRACE, leaves the file in the temp
// folder; sweep `opencircle-drag-*` at start if that ever matters.
pub struct DragFile {
    path: PathBuf,
    /// An app took it: delete after `GRACE`.
    pub dropped: bool,
}

/// How long a dropped file stays for the app that took it.
pub const GRACE: std::time::Duration = std::time::Duration::from_secs(30);

impl DragFile {
    pub fn write(crop: &Crop) -> Result<Self, String> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("opencircle-drag-{}-{n}.png", std::process::id());
        let path = std::env::temp_dir().join(name);
        crate::actions::write_png(&path, crop.width, crop.height, &crop.rgba)?;
        Ok(DragFile {
            path,
            dropped: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
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

/// Runs the OS drag of `file` (and `crop` as a bitmap) until the drop. Call it
/// on the UI thread while the left button is down; it returns after the drop:
/// `true` = an app took it (`file` is marked dropped). `hide_source`: the active
/// window hides for the drag (the overlay), and comes back if nothing took it.
#[cfg(windows)]
pub fn start(file: &mut DragFile, crop: &Crop, hide_source: bool) -> Result<bool, String> {
    use std::mem::ManuallyDrop;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::{
        DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{
        CF_DIB, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropSource, OleInitialize, OleUninitialize,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::Shell::{
        BHID_DataObject, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, ShowWindow};
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
                eprintln!("[opencircle] drag-out: no bitmap ({err})");
            }
        }
        let window = GetActiveWindow();
        let window = (!window.is_invalid()).then_some(window);
        let hidden = window.filter(|_| hide_source);
        if let Some(w) = hidden {
            let _ = ShowWindow(w, SW_HIDE);
        }
        let effect =
            SHDoDragDrop(window, &data, None::<&IDropSource>, DROPEFFECT_COPY).map_err(e("drag"));
        let dropped = effect.as_ref().is_ok_and(|e| *e != DROPEFFECT_NONE);
        if let Some(w) = hidden.filter(|_| !dropped) {
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

#[cfg(not(windows))]
pub fn start(_file: &mut DragFile, _crop: &Crop, _hide_source: bool) -> Result<bool, String> {
    Err("drag-out is Windows-only for now".into())
}
