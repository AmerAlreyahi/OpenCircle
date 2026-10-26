//! OpenCircle search history (a Labs feature): the searches the user handed to
//! their browser (Search in browser, Ask AI without a key). Opt-in
//! (`general.search_history`, off by default), only in the user's data dir:
//! the query text, a small thumbnail of the crop, the time and the kind.
//! Turning it off deletes it.
// ponytail: recorded only; a list to browse lands with the AI chat (N-05).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::actions::Crop;

/// Searches kept (newest first).
pub const KEEP: usize = 50;
/// The thumbnail's long edge (px).
const THUMB: u32 = 96;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub time: u64,
    /// "lens" (an image) | "text" (a text search) | "ai".
    pub kind: String,
    /// The question or searched text; empty for an image.
    pub query: String,
    /// The page the browser got (https; "" for Lens, which is a form post).
    pub url: String,
    /// A small PNG of the crop, base64 ("" = none).
    pub thumb: String,
}

impl Entry {
    /// A search of `kind` for `query` on `crop` that opened `url`.
    pub fn new(kind: &str, query: &str, url: &str, crop: Option<&Crop>) -> Entry {
        Entry {
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            kind: kind.into(),
            query: query.into(),
            url: url.into(),
            thumb: crop.and_then(thumb).unwrap_or_default(),
        }
    }
}

/// `<data dir>/OpenCircle/search_history.json`.
pub fn path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("OpenCircle").join("search_history.json"))
}

/// The user turned history on, and its Labs switch is on (hidden: nothing is
/// recorded or shown, and the saved list is left alone).
pub fn enabled() -> bool {
    let c = crate::config::current();
    c.general.search_history && c.labs.has(crate::labs::Feature::History)
}

/// Newest first; nothing when the file is missing or unreadable.
pub fn read(path: &Path) -> Vec<Entry> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Adds `entry` at the front (a repeat of the same page moves up), keeps `KEEP`.
pub fn add(path: &Path, entry: Entry) -> std::io::Result<()> {
    let mut list = read(path);
    list.retain(|e| entry.url.is_empty() || e.url != entry.url);
    list.insert(0, entry);
    list.truncate(KEEP);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec(&list)?)
}

/// Deletes the history (Clear, or history turned off).
pub fn clear(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Keeps `entry` when history is on (`enabled`); off deletes any old history.
pub fn record(path: &Path, enabled: bool, entry: Entry) {
    if !enabled {
        return clear(path);
    }
    if let Err(e) = add(path, entry) {
        eprintln!("[opencircle] search history not saved: {e}");
    }
}

/// Keeps this search when the user turned history on (`enabled`); turned off
/// deletes the old list; hidden by Labs leaves it alone.
pub fn keep(kind: &str, query: &str, url: &str, crop: Option<&Crop>) {
    let Some(path) = path().filter(|_| !cfg!(test)) else {
        return; // tests: never the user's file
    };
    if enabled() {
        record(&path, true, Entry::new(kind, query, url, crop));
    } else if !crate::config::current().general.search_history {
        clear(&path);
    }
}

/// [`clear`] at the usual place (Settings turned history off, or Clear).
pub fn clear_default() {
    if let Some(p) = path() {
        clear(&p);
    }
}

/// A `THUMB`-sized PNG of `crop`, base64.
fn thumb(crop: &Crop) -> Option<String> {
    let (w, h) = (crop.width, crop.height);
    if w == 0 || h == 0 || crop.rgba.len() < (w * h * 4) as usize {
        return None;
    }
    let k = (THUMB as f32 / w.max(h) as f32).min(1.0);
    let (tw, th) = (
        ((w as f32 * k) as u32).max(1),
        ((h as f32 * k) as u32).max(1),
    );
    // ponytail: nearest pixel, enough for a 96 px reminder.
    let mut px = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        for x in 0..tw {
            let i = (((y * h / th) * w + x * w / tw) * 4) as usize;
            px.extend_from_slice(&crop.rgba[i..i + 4]);
        }
    }
    let png = crate::actions::encode_png(tw, th, &px).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(png))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str) -> Entry {
        Entry {
            time: 100,
            kind: "ai".into(),
            query: format!("q {url}"),
            url: url.into(),
            thumb: String::new(),
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-search-hist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("h.json")
    }

    #[test]
    fn history_is_off_by_default_and_off_deletes_it() {
        assert!(!crate::config::Config::default().general.search_history);
        let path = scratch("off");
        record(&path, true, entry("https://www.google.com/a"));
        assert_eq!(read(&path).len(), 1);
        record(&path, false, entry("https://www.google.com/b"));
        assert!(!path.exists(), "turning it off deletes it");
        assert!(read(&path).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn newest_first_repeats_move_up_and_only_keep_are_kept() {
        let path = scratch("ring");
        for i in 0..KEEP + 7 {
            add(&path, entry(&format!("https://www.google.com/{i}"))).unwrap();
        }
        let list = read(&path);
        assert_eq!(list.len(), KEEP);
        assert_eq!(list[0].url, format!("https://www.google.com/{}", KEEP + 6));
        add(&path, entry("https://www.google.com/30")).unwrap();
        let list = read(&path);
        assert_eq!(
            (list.len(), list[0].url.as_str()),
            (KEEP, "https://www.google.com/30")
        );
        assert_eq!(list.iter().filter(|e| e.url.ends_with("/30")).count(), 1);
        clear(&path);
        assert!(read(&path).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn thumbnails_are_small_pngs() {
        let crop = Crop {
            width: 400,
            height: 200,
            rgba: vec![200; 400 * 200 * 4],
        };
        let png = base64::engine::general_purpose::STANDARD
            .decode(thumb(&crop).unwrap())
            .unwrap();
        let info = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        assert_eq!((info.info().width, info.info().height), (96, 48));
        let empty = Crop {
            width: 0,
            height: 0,
            rgba: vec![],
        };
        assert!(thumb(&empty).is_none());
    }
}
