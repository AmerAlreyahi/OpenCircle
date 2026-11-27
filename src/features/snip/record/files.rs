//! The recording's files (R-04): the temp file while it is written, what a crash leaves, and
//! what Save does with it. A recording is too big for memory, so it is always a file:
//! `<app data>/recordings-tmp/rec-<time>.<ext>`, never in Pictures until the user saves it.
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The folder temp recordings live in.
pub fn temp_dir() -> PathBuf {
    crate::shared::paths::app_data().join("recordings-tmp")
}

/// A new temp path for a recording `ext` ("mp4", "ogg", ...): the time in ms and a counter, and
/// never one that exists (a Discard still deleting the last recording can not meet this one).
pub fn new_temp(dir: &Path, ext: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    loop {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = dir.join(format!("rec-{ms}-{n}.{ext}"));
        if !path.exists() && !marker(&path).exists() {
            return path;
        }
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The note beside `file` that says the user discarded it: if the worker deleting it is killed
/// (quit, crash), the next start deletes it instead of offering it.
fn marker(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".discard");
    PathBuf::from(name)
}

/// Marks `file` as discarded (before anything slow runs).
pub fn mark_discarded(file: &Path) {
    let _ = std::fs::write(marker(file), b"");
}

/// The discard finished: the marker goes too.
pub fn unmark(file: &Path) {
    let _ = std::fs::remove_file(marker(file));
}

/// At start: every file the user discarded (a marker beside it) is deleted with its marker, and
/// an "offered" marker whose file is gone (saved, moved) is removed.
pub fn sweep_discarded(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let m = e.path();
        let file = m.with_extension(""); // `a.mp4.discard` -> `a.mp4`
        match m.extension().and_then(|x| x.to_str()) {
            Some("discard") => {
                let _ = std::fs::remove_file(&file);
                let _ = std::fs::remove_file(&m);
            }
            Some("offered") if !file.exists() => {
                let _ = std::fs::remove_file(&m);
            }
            _ => {}
        }
    }
}

/// Recordings a crash left in `dir` (every `rec-*` file that is not marked discarded), oldest
/// first, never deleted silently. (One with an "offered" marker was shown once already.)
pub fn leftovers(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("rec-")
                && !name.ends_with(".discard")
                && !name.ends_with(".offered")
                && !marker(&e.path()).exists()
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// The note beside `file` that says it was already offered on the thumbnail once.
fn offered_marker(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".offered");
    PathBuf::from(name)
}

pub fn mark_offered(file: &Path) {
    let _ = std::fs::write(offered_marker(file), b"");
}

pub fn was_offered(file: &Path) -> bool {
    offered_marker(file).exists()
}

pub fn unmark_offered(file: &Path) {
    let _ = std::fs::remove_file(offered_marker(file));
}

/// `name` in `dir`, or `name (2)`, `name (3)` ... when taken: nothing is overwritten.
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map_or(name.into(), |s| s.to_string_lossy());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){}", ext.as_deref().unwrap_or(""))))
        .find(|p| !p.exists())
        .expect("a free name")
}

/// Moves `from` into `dir` under a free name: a rename on one volume, else a copy then the
/// original goes. The saved path.
pub fn save(from: &Path, dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = from
        .file_name()
        .ok_or_else(|| io::Error::other("no file name"))?
        .to_string_lossy();
    save_as(from, &free_name(dir, &name))
}

/// Moves `from` to exactly `to` (a free path): a rename, else (another volume) a copy beside the
/// target under a `.part` name, flushed to disk, renamed into place; only then the original goes.
/// A failure removes the partial copy and leaves the original untouched.
pub fn save_as(from: &Path, to: &Path) -> io::Result<PathBuf> {
    move_with(from, to, &|a, b| std::fs::rename(a, b))
}

fn move_with(
    from: &Path,
    to: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    if rename(from, to).is_ok() {
        return Ok(to.to_path_buf());
    }
    let mut name = to.as_os_str().to_os_string();
    name.push(".part");
    let part = PathBuf::from(name);
    let copied = (|| {
        let mut src = std::fs::File::open(from)?;
        let mut dst = std::fs::File::create(&part)?;
        io::copy(&mut src, &mut dst)?;
        dst.sync_all()?; // on the disk before the original can go
        rename(&part, to)
    })();
    match copied {
        Ok(()) => {
            if let Err(e) = std::fs::remove_file(from) {
                crate::log!("[opencircle] recording: the copy is saved, the original stays: {e}");
            }
            Ok(to.to_path_buf())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Sets `file`'s modified time to now (best effort).
pub fn touch(file: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(file) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// The default name a saved recording gets: `OpenCircle 2026-11-27 14.03.22.mp4`.
pub fn saved_name(unix: u64, ext: &str) -> String {
    let (y, m, d) = civil(unix / 86_400);
    let s = unix % 86_400;
    format!(
        "OpenCircle {y:04}-{m:02}-{d:02} {:02}.{:02}.{:02}.{ext}",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// Days since 1970-01-01 as year, month, day (UTC; Hinnant's algorithm).
fn civil(days: u64) -> (i64, u32, u32) {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = crate::shared::paths::test_dir(tag);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// R04-40: temp names are unique even within one millisecond, and never one that exists.
    #[test]
    fn r04_40_temp_names_never_meet() {
        let d = dir("rec-names");
        let first = new_temp(&d, "webm");
        std::fs::write(&first, b"x").unwrap();
        let names: std::collections::HashSet<_> = (0..500).map(|_| new_temp(&d, "webm")).collect();
        assert_eq!(names.len(), 500, "no name twice");
        assert!(!names.contains(&first));
        assert_eq!(first.extension().unwrap(), "webm");
        assert_eq!(
            saved_name(1_700_000_000, "mp4"),
            "OpenCircle 2023-11-14 22.13.20.mp4"
        );
    }

    /// R04-57: a discarded file (marker) is swept at the next start, never offered; an offered
    /// leftover's marker is not a recording, and goes when its file does.
    #[test]
    fn r04_57_discarded_files_are_swept_and_stale_offered_markers_go() {
        let d = dir("rec-sweep");
        let (gone, left, shown) = (
            d.join("rec-1.mp4"),
            d.join("rec-2.mp4"),
            d.join("rec-3.mp4"),
        );
        for f in [&gone, &left, &shown] {
            std::fs::write(f, b"x").unwrap();
        }
        mark_discarded(&gone);
        assert!(
            !leftovers(&d).contains(&gone),
            "a discarded file is not a leftover"
        );
        sweep_discarded(&d);
        assert!(
            !gone.exists() && !marker(&gone).exists(),
            "swept with its marker"
        );
        assert!(left.exists() && shown.exists(), "nothing else is touched");
        mark_offered(&shown);
        assert!(was_offered(&shown));
        let found = leftovers(&d);
        assert_eq!(found.len(), 2, "the markers are not recordings to offer");
        std::fs::remove_file(&shown).unwrap();
        sweep_discarded(&d); // its file is gone: the marker goes
        assert!(!was_offered(&shown), "no marker without its file");
        unmark(&gone);
    }

    /// R04-41: a crash's leftovers are found (only `rec-*`), oldest first, and nothing is deleted.
    #[test]
    fn r04_41_leftovers_are_found_and_kept() {
        let d = dir("rec-left");
        std::fs::write(d.join("rec-1.mp4"), b"a").unwrap();
        std::fs::write(d.join("other.txt"), b"b").unwrap();
        let found = leftovers(&d);
        assert_eq!(found, vec![d.join("rec-1.mp4")]);
        assert!(d.join("rec-1.mp4").exists() && d.join("other.txt").exists());
        assert!(leftovers(&d.join("missing")).is_empty());
    }

    /// R04-42: Save moves the file (the temp one is gone), never overwrites a file of that name.
    #[test]
    fn r04_42_save_moves_and_never_overwrites() {
        let d = dir("rec-save");
        let out = d.join("Videos");
        for i in 0..2 {
            let tmp = d.join("rec-9.mp4");
            std::fs::write(&tmp, format!("take {i}")).unwrap();
            let saved = save(&tmp, &out).unwrap();
            assert!(!tmp.exists(), "moved");
            assert_eq!(std::fs::read_to_string(saved).unwrap(), format!("take {i}"));
        }
        assert!(out.join("rec-9.mp4").exists() && out.join("rec-9 (2).mp4").exists());
    }

    /// R04-43: across drives the file is copied beside the target, flushed, renamed; the
    /// original goes only after that; a failure leaves the original and no partial copy.
    #[test]
    fn r04_43_a_move_across_drives_never_loses_the_original() {
        let d = dir("rec-cross");
        let (from, to) = (d.join("rec-5.mp4"), d.join("saved.mp4"));
        let no_same_drive = |a: &Path, b: &Path| match a == from {
            true => Err(io::Error::other("cross-device")),
            false => std::fs::rename(a, b),
        };
        std::fs::write(&from, b"film").unwrap();
        assert_eq!(move_with(&from, &to, &no_same_drive).unwrap(), to);
        assert_eq!(std::fs::read(&to).unwrap(), b"film");
        assert!(
            !from.exists(),
            "the original goes after the copy is in place"
        );
        assert!(!d.join("saved.mp4.part").exists());
        // the last step fails: the original stays, the partial copy is removed
        let (from2, to2) = (d.join("rec-6.mp4"), d.join("saved2.mp4"));
        std::fs::write(&from2, b"film2").unwrap();
        let never = |_: &Path, _: &Path| Err(io::Error::other("no"));
        assert!(move_with(&from2, &to2, &never).is_err());
        assert!(from2.exists() && !to2.exists());
        assert!(!d.join("saved2.mp4.part").exists(), "no partial file left");
        // the copy itself fails (no folder): the original stays
        let gone = d.join("missing").join("x.mp4");
        assert!(move_with(&from2, &gone, &never).is_err());
        assert_eq!(std::fs::read(&from2).unwrap(), b"film2");
    }
}
