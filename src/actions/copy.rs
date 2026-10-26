//! Copy: the crop to the clipboard as an image only, scaled for AI chats
//! (`snip.max_edge`). Text is copied only by selecting it (or the Text panel).
use std::borrow::Cow;
use std::sync::Mutex;

use super::{Action, Crop, Key};

pub const ACTION: Action = Action {
    id: "copy",
    label: "Copy",
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
    let max_edge = crate::config::current().snip.max_edge;
    let shown = super::fit_long_edge(crop, max_edge);
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
    .map_err(|e| {
        eprintln!("copy failed: {e}");
        "Couldn't copy. Close what holds the clipboard and try again.".into()
    })
}

/// The confirmation pill's text, for the image and for text alike.
pub const COPIED_PILL: &str = "Copied";

/// Puts `text` on the clipboard (the Text panel's Copy).
pub fn text(text: String) -> Result<(), String> {
    #[cfg(test)]
    copied::set_text(text.clone());
    with_clipboard(|clipboard| clipboard.set_text(text))
}

fn with_clipboard(
    set: impl FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error>,
) -> Result<(), String> {
    // Tests never touch the machine's clipboard (on WSL it is the Windows one).
    if cfg!(test) {
        return Ok(());
    }
    let mut slot = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    let clipboard = match &mut *slot {
        Some(clipboard) => clipboard,
        None => slot.insert(arboard::Clipboard::new().map_err(|e| e.to_string())?),
    };
    set(clipboard).map_err(|e| e.to_string())
}

/// Tests: what was put on the clipboard, per test (its thread's name; the
/// UI's background threads carry it, `ui::background`).
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
