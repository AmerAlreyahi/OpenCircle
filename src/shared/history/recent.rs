//! Recent captures (snip): the newest `HISTORY` PNGs, only with `snip.history` on and
//! only in the user's data dir (`paths::recent`). Off = nothing on disk: the next
//! read deletes what an earlier "on" left.

use std::path::{Path, PathBuf};

use iced::widget::image;

use crate::shared::picture::Crop;

/// Captures kept on disk, and shown in the strip.
pub(crate) const HISTORY: usize = 20;
const STRIP: usize = 8;

/// `<user data dir>/opencircle/recent` (Windows: %LOCALAPPDATA%).
pub fn dir() -> Option<PathBuf> {
    crate::shared::paths::recent()
}

/// Keeps `crop` in Recent (on a thread) if the user turned history on.
pub fn remember(crop: &Crop) {
    if !crate::shared::config::current().snip.history {
        return;
    }
    let (Some(dir), crop) = (dir(), crop.clone()) else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("recent".into())
        .spawn(move || {
            if let Err(e) = keep(&dir, &crop, HISTORY) {
                crate::log!("[opencircle] recent capture not kept: {e}");
            }
        });
    if let Err(e) = spawned {
        crate::log!("[opencircle] recent capture not kept: {e}");
    }
}

/// Writes `crop` into `dir` as the newest PNG, then drops all but the newest `limit`.
pub(super) fn keep(dir: &Path, crop: &Crop, limit: usize) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    // Fixed-width names sort by time. Always after the newest kept one: a stamp freed
    // by pruning must not be reused (it would sort as the oldest).
    let newest = list(dir)
        .first()
        .and_then(|p| p.file_stem()?.to_str()?.parse::<u128>().ok());
    let stamp = newest.map_or(now, |n| now.max(n + 1));
    let path = dir.join(format!("{stamp:016}.png"));
    crate::shared::picture::write_png(&path, crop.width, crop.height, &crop.rgba)?;
    for old in list(dir).into_iter().skip(limit) {
        let _ = std::fs::remove_file(old);
    }
    // the same pass keeps the recordings (`<dir>/recordings`) to the same count
    prune_recordings(&recordings_in(dir), limit);
    Ok(path)
}

/// Where recordings that nobody saved wait in Recent: `<recent>/recordings`.
pub fn recordings_dir() -> Option<PathBuf> {
    dir().map(|d| recordings_in(&d))
}

fn recordings_in(dir: &Path) -> PathBuf {
    dir.join("recordings")
}

/// Keeps the newest `limit` files of `dir` (by their time, then name), oldest first out: the same
/// rule as the captures, so recordings never grow without a bound.
pub fn prune_recordings(dir: &Path, limit: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort();
    files.reverse(); // newest first
    for (_, old) in files.into_iter().skip(limit) {
        let _ = std::fs::remove_file(old);
    }
}

/// How many recordings wait in the Recent folder `dir`.
pub fn count_recordings(dir: &Path) -> usize {
    std::fs::read_dir(recordings_in(dir))
        .map_or(0, |d| d.flatten().filter(|e| e.path().is_file()).count())
}

/// Recent is on and recordings wait in it (the Studio's "Show recordings" row).
pub fn has_recordings() -> bool {
    crate::shared::config::current().snip.history
        && recordings_dir()
            .and_then(|d| std::fs::read_dir(d).ok())
            .is_some_and(|mut d| d.any(|e| e.is_ok_and(|e| e.path().is_file())))
}

/// The kept captures, newest first.
pub(crate) fn list(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = (entries.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    out.sort();
    out.reverse();
    out
}

/// Blocking: the Recent strip (newest `STRIP`, as thumbnails). With history off,
/// whatever an earlier "on" left is deleted instead: off = nothing on disk.
pub fn load() -> Vec<(PathBuf, image::Handle)> {
    let Some(dir) = dir() else {
        return Vec::new();
    };
    if !crate::shared::config::current().snip.history {
        clear_dir(&dir);
        return Vec::new();
    }
    (list(&dir).into_iter().take(STRIP))
        .filter_map(|path| {
            let frame = crate::shared::platform::capture::from_png(path.as_os_str()).ok()?;
            let crop = Crop {
                width: frame.width,
                height: frame.height,
                rgba: frame.rgba,
            };
            let small = crate::shared::picture::fit_long_edge(crop, 96);
            let picture = image::Handle::from_rgba(small.width, small.height, small.rgba);
            Some((path, picture))
        })
        .collect()
}

/// Blocking: a recent capture, full size.
pub fn open(path: &Path) -> Result<Crop, String> {
    let frame = crate::shared::platform::capture::from_png(path.as_os_str())?;
    Ok(Crop {
        width: frame.width,
        height: frame.height,
        rgba: frame.rgba,
    })
}

/// The Clear button: every kept capture goes.
pub fn clear() {
    if let Some(dir) = dir() {
        clear_dir(&dir);
    }
}

pub(super) fn clear_dir(dir: &Path) {
    if dir.exists()
        && let Err(e) = std::fs::remove_dir_all(dir)
    {
        crate::log!("[opencircle] recent captures not cleared: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_keeps_the_newest_twenty_and_clears() {
        let dir = std::env::temp_dir().join(format!("oc-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shot = |v: u8| Crop {
            width: 2,
            height: 2,
            rgba: vec![v; 16],
        };
        let mut kept = Vec::new();
        for i in 0..23 {
            kept.push(keep(&dir, &shot(i), HISTORY).unwrap());
        }
        let now = list(&dir);
        assert_eq!(now.len(), HISTORY);
        // Newest first; the three oldest were dropped.
        assert_eq!(now[0], kept[22]);
        assert_eq!(now[HISTORY - 1], kept[3]);
        assert!(!kept[2].exists());
        assert_eq!(open(&now[0]).unwrap().rgba, vec![22; 16]);
        clear_dir(&dir);
        assert!(!dir.exists() && list(&dir).is_empty());
    }

    /// R04-64: the recordings folder is pruned in the pictures' own pass (same count, oldest
    /// out first), and a clear takes it too.
    #[test]
    fn r04_64_recordings_are_pruned_with_the_pictures() {
        let dir = std::env::temp_dir().join(format!("oc-recent-rec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rec = recordings_in(&dir);
        std::fs::create_dir_all(&rec).unwrap();
        for n in 0..6 {
            std::fs::write(rec.join(format!("rec-{n}.mp4")), b"x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let shot = Crop {
            width: 1,
            height: 1,
            rgba: vec![1; 4],
        };
        keep(&dir, &shot, 4).unwrap(); // a picture is kept: the same pass prunes the recordings
        let left: Vec<_> = std::fs::read_dir(&rec).unwrap().flatten().collect();
        assert_eq!(left.len(), 4, "newest 4 stay");
        assert!(!rec.join("rec-0.mp4").exists() && rec.join("rec-5.mp4").exists());
        assert!(
            list(&dir).len() == 1,
            "the recordings folder is not a picture"
        );
        clear_dir(&dir);
        assert!(!rec.exists());
    }

    #[test]
    fn rapid_captures_with_pruning_stay_newest_first() {
        let dir = std::env::temp_dir().join(format!("oc-recent-burst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shot = |v: u8| Crop {
            width: 1,
            height: 1,
            rgba: vec![v; 4],
        };
        // Many captures within a few ms: pruned stamps must never be reused.
        let kept: Vec<_> = (0..40).map(|i| keep(&dir, &shot(i), 3).unwrap()).collect();
        assert_eq!(
            list(&dir),
            vec![kept[39].clone(), kept[38].clone(), kept[37].clone()]
        );
        clear_dir(&dir);
    }

    #[test]
    fn recent_is_off_by_default_and_never_written_then() {
        assert!(!crate::shared::config::Config::default().snip.history);
        // Off: `remember` writes nothing (the running config has the default).
        let before = dir().map(|d| list(&d).len());
        remember(&Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(dir().map(|d| list(&d).len()), before);
    }
}
