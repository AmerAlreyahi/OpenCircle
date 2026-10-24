//! Copy: the crop to the clipboard as an image, scaled for AI chats
//! (`snip.max_edge`). On Windows its OCR text follows on the same clipboard
//! entry, so a chat app pastes the image and a text editor the text.
use std::borrow::Cow;
use std::cell::RefCell;

use super::{Action, Crop, Key};

pub const ACTION: Action = Action {
    id: "copy",
    label: "Copy",
    tip: "Copy the selection as an image",
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

fn run(crop: Crop) -> Result<(), String> {
    let max_edge = crate::config::current().snip.max_edge;
    #[cfg(windows)]
    let original = crop.clone();
    let shown = super::fit_long_edge(crop, max_edge);
    with_clipboard(|clipboard| {
        clipboard.set_image(arboard::ImageData {
            width: shown.width as usize,
            height: shown.height as usize,
            bytes: Cow::Owned(shown.rgba),
        })
    })?;
    #[cfg(windows)]
    win::add_text_later(original);
    Ok(())
}

/// Puts `text` on the clipboard (the Text panel's Copy).
pub fn text(text: String) -> Result<(), String> {
    with_clipboard(|clipboard| clipboard.set_text(text))
}

fn with_clipboard(
    set: impl FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error>,
) -> Result<(), String> {
    CLIPBOARD.with_borrow_mut(|slot| {
        let clipboard = match slot {
            Some(clipboard) => clipboard,
            None => slot.insert(arboard::Clipboard::new().map_err(|e| e.to_string())?),
        };
        set(clipboard).map_err(|e| e.to_string())
    })
}

// ponytail: Linux gets the image only. arboard writes one format per call and an
// X11 selection has one owner; text beside it needs our own selection owner
// (TARGETS + INCR) or an arboard multi-format API.
#[cfg(windows)]
mod win {
    use clipboard_win::{Clipboard, options::NoClear, raw};

    use crate::actions::{Crop, text};

    /// Reads `crop` (original size, through the warm OCR helper while one is
    /// held) and adds the text to the image just copied, unless anything else
    /// was copied meanwhile. No OCR models: image only (a copy never asks).
    pub fn add_text_later(crop: Crop) {
        if !text::models_present() {
            return;
        }
        let Some(copied) = raw::seq_num() else { return };
        let hold = crate::ocr::hold();
        let spawned = std::thread::Builder::new()
            .name("copy text".into())
            .spawn(move || {
                let _hold = hold;
                let read = crate::ocr::recognize_in_child(&crop.rgba, crop.width, crop.height);
                let words = match read {
                    Ok(blocks) => text::join_lines(&blocks),
                    Err(e) => return eprintln!("[opencircle] copy: no text added ({e})"),
                };
                if words.trim().is_empty() {
                    return;
                }
                let Ok(_open) = Clipboard::new_attempts(10) else {
                    return eprintln!("[opencircle] copy: clipboard busy, no text added");
                };
                // Still our image (nothing copied since): one entry, two formats.
                if raw::seq_num() == Some(copied)
                    && let Err(e) = raw::set_string_with(&words, NoClear)
                {
                    eprintln!("[opencircle] copy: no text added ({e})");
                }
            });
        if let Err(e) = spawned {
            eprintln!("[opencircle] copy: no text added ({e})");
        }
    }
}
