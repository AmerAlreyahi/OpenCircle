//! Share (Snip's toolbar): the capture through Windows' share sheet
//! (DataTransferManager, anchored to our window). The shared item is the
//! capture alone, as a temp PNG; nothing leaves the PC unless the user picks a
//! target in the sheet. The file goes with the editor (the overlay closing),
//! a grace period later once a target took it (`dragout::DragFile`).
//! Linux: not yet (an xdg portal has no share sheet); the button is hidden
//! there (`SUPPORTED`).
use iced::futures::channel::oneshot;

use super::Crop;
use crate::editor::dragout::DragFile;

/// The platform has a share sheet.
pub const SUPPORTED: bool = cfg!(windows);

/// A share in progress: its PNG, and (Windows) the sheet's event handlers,
/// removed when this goes.
pub struct Shared {
    pub file: DragFile,
    #[cfg(all(windows, not(test)))]
    _sheet: Option<sheet::Sheet>,
}

/// Writes `crop` as the temp PNG and opens the share sheet for the active
/// window (call it on the UI thread). The receiver fires once the user picked
/// a target (the file is then theirs to read: `chosen`); it is cancelled if
/// the sheet goes without one and this `Shared` is dropped.
pub fn start(crop: &Crop) -> Result<(Shared, oneshot::Receiver<()>), String> {
    let file = DragFile::temp(crop, "share")?;
    let (tx, rx) = oneshot::channel::<()>();
    #[cfg(test)]
    {
        TEST_SHEET.set(Some((file.path().to_path_buf(), tx)));
        Ok((Shared { file }, rx))
    }
    #[cfg(all(windows, not(test)))]
    {
        let sheet = sheet::show(file.path(), tx)?;
        Ok((
            Shared {
                file,
                _sheet: Some(sheet),
            },
            rx,
        ))
    }
    #[cfg(all(not(windows), not(test)))]
    {
        let _ = (file, tx, rx); // never called: no Share button here
        Err("share: no share sheet on this platform yet".into())
    }
}

impl Shared {
    /// A target took the file: it stays `GRACE` after the editor goes (the
    /// app may still be reading it).
    pub fn chosen(&mut self) {
        self.file.dropped = true;
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: the file the last `start` would have handed to the sheet, and
    /// the sender a picked target fires.
    pub static TEST_SHEET: std::cell::Cell<Option<(std::path::PathBuf, oneshot::Sender<()>)>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(all(windows, not(test)))]
mod sheet {
    use std::path::Path;
    use std::sync::Mutex;

    use iced::futures::channel::oneshot;
    use windows::ApplicationModel::DataTransfer::{
        DataRequestedEventArgs, DataTransferManager, TargetApplicationChosenEventArgs,
    };
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::{IStorageItem, StorageFile};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::Shell::IDataTransferManagerInterop;
    use windows::core::{AgileReference, HSTRING, Interface, Ref};
    use windows_collections::IIterable;

    /// The sheet's title (Windows requires one).
    const TITLE: &str = "OpenCircle capture";

    /// The handlers on our window's DataTransferManager (one share at a time).
    pub struct Sheet {
        manager: DataTransferManager,
        requested: i64,
        chosen: i64,
    }

    impl Drop for Sheet {
        fn drop(&mut self) {
            let _ = self.manager.RemoveDataRequested(self.requested);
            let _ = self.manager.RemoveTargetApplicationChosen(self.chosen);
        }
    }

    pub fn show(path: &Path, tx: oneshot::Sender<()>) -> Result<Sheet, String> {
        let e =
            |what: &'static str| move |err: windows::core::Error| format!("share: {what}: {err}");
        // SAFETY: plain query of this thread's active window.
        let window = unsafe { GetActiveWindow() };
        if window.is_invalid() {
            return Err("share: no window".into());
        }
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
            .map_err(e("file"))?
            .join()
            .map_err(e("file"))?;
        // (the handler may run on another thread: COM objects go agile)
        let file = AgileReference::new(&file).map_err(e("file"))?;
        let interop = windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()
            .map_err(e("interop"))?;
        // SAFETY: `window` is ours and alive (this thread's active window).
        let manager: DataTransferManager =
            unsafe { interop.GetForWindow(window) }.map_err(e("manager"))?;
        let requested = manager
            .DataRequested(&TypedEventHandler::new(
                move |_, args: Ref<DataRequestedEventArgs>| {
                    if let Some(args) = args.as_ref() {
                        let data = args.Request()?.Data()?;
                        data.Properties()?.SetTitle(&HSTRING::from(TITLE))?;
                        // the capture alone, read-only
                        let item: IStorageItem = file.resolve()?.cast()?;
                        let items = IIterable::<IStorageItem>::from(vec![Some(item)]);
                        data.SetStorageItemsReadOnly(&items)?;
                    }
                    Ok(())
                },
            ))
            .map_err(e("handler"))?;
        let tx = Mutex::new(Some(tx));
        let chosen = manager
            .TargetApplicationChosen(&TypedEventHandler::new(
                move |_, _: Ref<TargetApplicationChosenEventArgs>| {
                    let tx = tx.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Some(tx) = tx {
                        let _ = tx.send(());
                    }
                    Ok(())
                },
            ))
            .map_err(e("handler"))?;
        let sheet = Sheet {
            manager,
            requested,
            chosen,
        };
        // SAFETY: as above.
        unsafe { interop.ShowShareUIForWindow(window) }.map_err(e("sheet"))?;
        Ok(sheet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sheet gets one PNG of the capture, nothing else; it is deleted
    /// when the share goes, unless a target took it (then after the grace).
    #[test]
    fn the_share_is_the_capture_as_a_png_deleted_with_the_editor() {
        let crop = Crop {
            width: 3,
            height: 2,
            rgba: (0..24).collect(),
        };
        let (shared, _) = start(&crop).unwrap();
        let (path, _) = TEST_SHEET.take().expect("handed to the sheet");
        assert_eq!(path, shared.file.path());
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("opencircle-share-") && name.ends_with(".png"));
        let png = png::Decoder::new(std::io::Cursor::new(std::fs::read(&path).unwrap()));
        let mut png = png.read_info().unwrap();
        let mut px = vec![0; png.output_buffer_size().unwrap()];
        let info = png.next_frame(&mut px).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(px, crop.rgba, "the capture, pixel for pixel");
        drop(shared);
        assert!(!path.exists(), "gone with the share");

        // a target took it: still there right after the share goes
        let (mut shared, _) = start(&crop).unwrap();
        let (path, _) = TEST_SHEET.take().unwrap();
        shared.chosen();
        drop(shared);
        assert!(path.exists(), "the target may still be reading it");
        let _ = std::fs::remove_file(path);
    }
}
