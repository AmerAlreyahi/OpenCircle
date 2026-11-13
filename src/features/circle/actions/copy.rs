//! Copy: the crop to the clipboard as an image only, scaled for AI chats
//! (`snip.max_edge`). Text is copied only by selecting it (or Copy text).
use std::borrow::Cow;
use std::sync::Mutex;

use super::{Action, Key};
use crate::shared::app_language::{S, Said};
use crate::shared::picture::Crop;

pub const ACTION: Action = Action {
    id: "copy",
    label: S::c_copy,
    key: Key {
        ctrl: true,
        letter: 'c',
    },
    icon: '\u{e0a2}', // lucide "copy"
    run: super::Run::Finish(run),
};

/// On Linux the copied image lives only as long as this process holds the
/// clipboard, so one stays open for the app's life. Copies run off the UI
/// thread (encoding a big capture takes a while), one at a time.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

/// The one copy path: the picture alone. No text rides along (a browser or chat
/// would paste the text instead of the image); text is copied only by `text`.
fn run(crop: Crop) -> Result<String, String> {
    let max_edge = crate::shared::config::current().snip.max_edge;
    let shown = crate::shared::picture::fit_long_edge(crop, max_edge);
    #[cfg(test)]
    copied::set_image((shown.width, shown.height));
    with_clipboard(|clipboard| {
        clipboard.set_image(arboard::ImageData {
            width: shown.width as usize,
            height: shown.height as usize,
            bytes: Cow::Owned(shown.rgba),
        })
    })
    .map(|()| COPIED_PILL.into())
}

/// What the user reads when the clipboard says no (its own error, e.g. "X11 server
/// connection timed out", goes to the log only).
pub const COPY_FAILED: Said = Said(S::pill_copy_failed);

/// The confirmation pill's text, for the image and for text alike.
pub const COPIED_PILL: Said = Said(S::pill_copied);

/// Puts `text` on the clipboard (Copy text); `Err` is `COPY_FAILED`.
pub fn text(text: String) -> Result<(), String> {
    #[cfg(test)]
    copied::set_text(text.clone());
    with_clipboard(|clipboard| clipboard.set_text(text))
}

/// The clipboard's text, if it holds any (Ctrl+V in the Snip editor).
pub fn read_text() -> Option<String> {
    #[cfg(test)]
    return copied::pasted();
    #[cfg(not(test))]
    {
        let mut slot = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        let clipboard = match &mut *slot {
            Some(clipboard) => clipboard,
            None => slot.insert(arboard::Clipboard::new().ok()?),
        };
        clipboard.get_text().ok()
    }
}

fn with_clipboard(
    set: impl FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error>,
) -> Result<(), String> {
    // Tests never touch the machine's clipboard (on WSL it is the Windows one).
    if cfg!(test) {
        return Ok(());
    }
    let mut slot = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    let opened = match &mut *slot {
        Some(clipboard) => Ok(clipboard),
        None => arboard::Clipboard::new().map(|c| slot.insert(c)),
    };
    opened.and_then(set).map_err(failed)
}

/// The clipboard's error, logged; the user gets `COPY_FAILED`.
fn failed(e: arboard::Error) -> String {
    crate::log!("copy failed: {e}");
    COPY_FAILED.to_string()
}

#[test]
fn a_clipboard_error_reads_plainly() {
    // #172: GNOME Wayland without XWayland showed arboard's own words
    let raw = "X11 server connection timed out because it was unreachable";
    let e = arboard::Error::Unknown {
        description: raw.into(),
    };
    assert_eq!(failed(e), COPY_FAILED.to_string());
}

/// Tests: what was put on the clipboard, per test (its thread's name; the
/// UI's background threads carry it, `app::background`).
#[cfg(test)]
pub mod copied {
    use std::collections::HashMap;
    use std::sync::Mutex;

    type Last<T> = Mutex<Option<HashMap<String, T>>>;
    static TEXT: Last<String> = Mutex::new(None);
    static IMAGE: Last<(u32, u32)> = Mutex::new(None);

    fn me() -> String {
        std::thread::current()
            .name()
            .unwrap_or_default()
            .to_string()
    }

    fn put<T>(last: &Last<T>, value: T) {
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        last.get_or_insert_default().insert(me(), value);
    }

    fn take<T>(last: &Last<T>) -> Option<T> {
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        last.as_mut()?.remove(&me())
    }

    static PASTE: Last<String> = Mutex::new(None);

    /// What `read_text` gives this test (the clipboard's text).
    pub fn set_paste(text: &str) {
        put(&PASTE, text.to_string());
    }

    pub(super) fn pasted() -> Option<String> {
        take(&PASTE)
    }

    pub(super) fn set_text(text: String) {
        put(&TEXT, text);
    }

    pub(super) fn set_image(size: (u32, u32)) {
        put(&IMAGE, size);
    }

    /// The last text this test copied (and forgets it).
    pub fn text() -> Option<String> {
        take(&TEXT)
    }

    /// The size of the last image this test copied (and forgets it).
    pub fn image() -> Option<(u32, u32)> {
        take(&IMAGE)
    }
}
