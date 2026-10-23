//! Copy: the crop to the clipboard as an image.
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
    run,
};

thread_local! {
    // On Linux the copied image lives only as long as this process holds the
    // clipboard, so keep one open for the app's life (actions run on the UI thread).
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

fn run(crop: Crop) -> Result<(), String> {
    CLIPBOARD.with_borrow_mut(|slot| {
        let clipboard = match slot {
            Some(clipboard) => clipboard,
            None => slot.insert(arboard::Clipboard::new().map_err(|e| e.to_string())?),
        };
        clipboard
            .set_image(arboard::ImageData {
                width: crop.width as usize,
                height: crop.height as usize,
                bytes: Cow::Owned(crop.rgba),
            })
            .map_err(|e| e.to_string())
    })
}
