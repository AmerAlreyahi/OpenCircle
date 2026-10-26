//! Copy: the crop to the clipboard as an image only, scaled for AI chats
//! (`snip.max_edge`). Text is copied only by selecting it (or the Text panel).
use std::borrow::Cow;
use std::cell::RefCell;

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

thread_local! {
    // On Linux the copied image lives only as long as this process holds the
    // clipboard, so keep one open for the app's life (actions run on the UI thread).
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}
#[cfg(test)]
thread_local! {
    /// The last text `text` put on the clipboard on this thread (tests read it back).
    pub static COPIED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The size of the last image `run` put on the clipboard on this thread.
    pub static COPIED_IMAGE: RefCell<Option<(u32, u32)>> = const { RefCell::new(None) };
}

/// The one copy path: the picture alone. No text rides along (a browser or chat
/// would paste the text instead of the image); text is copied only by `text`.
fn run(crop: Crop) -> Result<String, String> {
    let max_edge = crate::config::current().snip.max_edge;
    let shown = super::fit_long_edge(crop, max_edge);
    #[cfg(test)]
    COPIED_IMAGE.set(Some((shown.width, shown.height)));
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
    COPIED.set(Some(text.clone()));
    with_clipboard(|clipboard| clipboard.set_text(text))
}

fn with_clipboard(
    set: impl FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error>,
) -> Result<(), String> {
    // Tests never touch the machine's clipboard (on WSL it is the Windows one).
    if cfg!(test) {
        return Ok(());
    }
    CLIPBOARD.with_borrow_mut(|slot| {
        let clipboard = match slot {
            Some(clipboard) => clipboard,
            None => slot.insert(arboard::Clipboard::new().map_err(|e| e.to_string())?),
        };
        set(clipboard).map_err(|e| e.to_string())
    })
}
