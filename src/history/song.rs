//! Recent song searches, on this PC only and only when the user turned history
//! on (`song.history`, off by default). Turning it off deletes the file on the
//! next search.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::music::Song;

/// Entries kept.
const KEEP: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub title: String,
    pub artist: String,
}

pub fn path() -> Option<PathBuf> {
    crate::paths::song_history()
}

/// Newest first; nothing when the file is missing or unreadable.
pub fn read(path: &Path) -> Vec<Entry> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Adds `song` at the front (once: a repeat moves up) and keeps `KEEP`.
pub fn add(path: &Path, song: &Song) -> std::io::Result<()> {
    let entry = Entry {
        title: song.title.clone(),
        artist: song.artist.clone(),
    };
    let mut list = read(path);
    list.retain(|e| *e != entry);
    list.insert(0, entry);
    list.truncate(KEEP);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec(&list)?)
}

/// What a finished search does with history: `enabled` adds the song (if any)
/// and returns the list; off deletes any old file and returns nothing.
pub fn after_search(path: &Path, enabled: bool, song: Option<&Song>) -> Vec<Entry> {
    if !enabled {
        let _ = std::fs::remove_file(path);
        return Vec::new();
    }
    if let Some(song) = song
        && let Err(e) = add(path, song)
    {
        crate::log!("[opencircle] song history not saved: {e}");
    }
    read(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(t: &str) -> Song {
        Song {
            title: t.into(),
            artist: "A".into(),
            ..Song::default()
        }
    }

    #[test]
    fn history_is_off_by_default_and_off_deletes_it() {
        assert!(!crate::config::Config::default().song.history);
        let dir = std::env::temp_dir().join(format!("oc-song-hist-{}", std::process::id()));
        let path = dir.join("h.json");
        let _ = std::fs::remove_dir_all(&dir);
        // Off: nothing written.
        assert!(after_search(&path, false, Some(&song("x"))).is_empty());
        assert!(!path.exists());
        // On: newest first, a repeat moves up.
        after_search(&path, true, Some(&song("one")));
        after_search(&path, true, Some(&song("two")));
        let list = after_search(&path, true, Some(&song("one")));
        let titles: Vec<_> = list.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["one", "two"]);
        // Turned off: the file goes.
        assert!(after_search(&path, false, None).is_empty());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
