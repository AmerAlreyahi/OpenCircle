//! Search: the crop in Google Lens, in the results card (`crate::sheet`) or the
//! user's own browser (see `crate::lens`).
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Action, Crop, Key, Panel, Run};
use crate::lens::LensPage;

pub const ACTION: Action = Action {
    id: "lens",
    label: "Search",
    tip: "Search the selection with Google Lens",
    key: Key {
        ctrl: false,
        letter: 'l',
    },
    icon: '\u{e154}', // lucide "search"
    run: Run::Panel(Panel::Lens),
};

/// The browser reads the page a while after we launch it (a cold browser start
/// can take seconds): never delete a page younger than this.
const MIN_LIFE: Duration = Duration::from_secs(10);

/// The Lens page for `crop`, in memory, for the results card (`crate::sheet`).
/// Blocking (PNG encode): call off the UI thread.
pub fn page(crop: &Crop) -> Result<String, String> {
    let png = super::encode_png(crop.width, crop.height, &crop.rgba)?;
    crate::lens::page(&png, crop.width, crop.height).map_err(|e| e.to_string())
}

/// Writes the Lens page for `crop` and opens it in the default browser.
/// Blocking (PNG encode, file write, process spawn): call off the UI thread.
pub fn open(crop: &Crop) -> Result<Held, String> {
    let png = super::encode_png(crop.width, crop.height, &crop.rgba)?;
    let page = crate::lens::open(&png, crop.width, crop.height).map_err(|e| e.to_string())?;
    Ok(Held {
        _page: Arc::new(page),
        opened: Instant::now(),
    })
}

/// An open Lens page; the file goes when the last clone is dropped.
#[derive(Debug, Clone)]
pub struct Held {
    /// Only held: its Drop deletes the file.
    _page: Arc<LensPage>,
    opened: Instant,
}

impl Held {
    /// The overlay is done with the page: delete it now, or once it is `MIN_LIFE` old.
    pub fn release(self) {
        let Some(wait) = linger(self.opened.elapsed()) else {
            return; // dropped here
        };
        // A failed spawn drops the closure, and the page with it: deleted early
        // rather than leaked (the next start's sweep would get it anyway).
        let _ = std::thread::Builder::new()
            .name("lens page".into())
            .spawn(move || {
                std::thread::sleep(wait);
                drop(self);
            });
    }
}

/// How much longer a page `age` old must stay; `None` = delete it now.
fn linger(age: Duration) -> Option<Duration> {
    Some(MIN_LIFE.saturating_sub(age)).filter(|wait| !wait.is_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_lives_at_least_min_life() {
        let s = Duration::from_secs;
        assert_eq!(linger(s(0)), Some(MIN_LIFE));
        assert_eq!(linger(s(3)), Some(MIN_LIFE - s(3)));
        assert_eq!(
            linger(MIN_LIFE - Duration::from_millis(1)),
            Some(Duration::from_millis(1))
        );
        assert_eq!(linger(MIN_LIFE), None);
        assert_eq!(linger(s(600)), None);
        assert!(MIN_LIFE >= s(10), "the browser needs time to read the page");
    }
}
