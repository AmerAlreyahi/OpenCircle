//! Studio crash recovery (STUDIO_V3_SPEC question B, board 20). Settings > Studio "Keep
//! unsaved work if OpenCircle closes unexpectedly" is OFF by default; when on and the
//! document is unsaved, the window calls [`write`] (debounced with [`Debounce`]) with
//! the picture as PNG bytes and its annotations as JSON text (the Studio has no
//! document format of its own). The copy is deleted by [`discard`] on save, discard or
//! a clean close. At the next start [`pending`] says whether a leftover copy exists;
//! the restore sheet shows it, then the window loads it (the files stay until
//! `discard` is called after the restored picture is saved or dropped) or discards it.
//! Everything lives in one folder under the app's data folder; nothing is written
//! while the switch is off, and nothing is ever written elsewhere.
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const PICTURE: &str = "picture.png";
const NOTES: &str = "annotations.json";
/// How long the document must stay quiet before a copy is written.
pub const QUIET: Duration = Duration::from_secs(3);

/// A leftover copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub png: Vec<u8>,
    /// The annotations as the window wrote them (JSON).
    pub notes: String,
    /// When the copy was last written (for "Saved 2 minutes before it closed").
    pub saved_at: SystemTime,
}

/// `<data>/OpenCircle/studio-recovery`.
#[cfg(test)]
thread_local! {
    /// A test's own folder (each test runs on its own thread).
    static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests: this thread's recovery folder.
#[cfg(test)]
pub fn use_dir(dir: PathBuf) {
    TEST_DIR.with(|d| *d.borrow_mut() = Some(dir));
}

pub fn dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = TEST_DIR.with(|d| d.borrow().clone()) {
        return dir;
    }
    crate::shared::paths::app_data().join("studio-recovery")
}

/// Deletes the copy (saved, discarded, or closed cleanly). Nothing there is fine.
pub fn discard() {
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    discard_in(&dir());
}

/// Counts the discards: a write made off the UI thread (`write_if_current`) that
/// finishes after a discard is dropped, so a copy never outlives what it kept.
static GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The discard count now (a writer takes it before it starts).
pub fn generation() -> u32 {
    GEN.load(std::sync::atomic::Ordering::SeqCst)
}

/// `write`, unless a `discard` came after `generation` was taken.
pub fn write_if_current(generation: u32, on: bool, png: &[u8], notes: &str) -> io::Result<bool> {
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if GEN.load(std::sync::atomic::Ordering::SeqCst) != generation {
        return Ok(false);
    }
    write_in(&dir(), on, png, notes)
}

/// The leftover copy of a crashed session, if a whole one exists. The caller checks
/// `studio.keep_unsaved` first: with the switch off nothing is offered (Settings
/// deletes the copy when the switch is turned off).
pub fn pending() -> Option<Pending> {
    pending_in(&dir())
}

pub fn write_in(dir: &Path, on: bool, png: &[u8], notes: &str) -> io::Result<bool> {
    if !on {
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    // Both temp files exist before anything is swapped; the old notes go first and the
    // new notes are renamed LAST, so a crash at any point leaves either no copy or a
    // whole new one, never a picture with another generation's notes.
    let (pic, note) = (dir.join(PICTURE), dir.join(NOTES));
    let (pic_tmp, note_tmp) = (pic.with_extension("tmp"), note.with_extension("tmp"));
    std::fs::write(&pic_tmp, png)?;
    std::fs::write(&note_tmp, notes.as_bytes())?;
    let _ = std::fs::remove_file(&note);
    std::fs::rename(pic_tmp, pic)?;
    std::fs::rename(note_tmp, note)?;
    Ok(true)
}

pub fn discard_in(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

pub fn pending_in(dir: &Path) -> Option<Pending> {
    let notes = std::fs::read_to_string(dir.join(NOTES)).ok()?;
    let png = std::fs::read(dir.join(PICTURE)).ok()?;
    let saved_at = std::fs::metadata(dir.join(NOTES)).ok()?.modified().ok()?;
    (!png.is_empty()).then_some(Pending {
        png,
        notes,
        saved_at,
    })
}

/// The "quiet for [`QUIET`]" rule: call `changed` on every edit and `due` on a timer;
/// `due` is true once per burst of edits.
#[derive(Debug, Default)]
pub struct Debounce(Option<Instant>);

impl Debounce {
    /// An edit is waiting for its quiet time (the clock only runs then).
    pub fn waiting(&self) -> bool {
        self.0.is_some()
    }

    pub fn changed(&mut self, now: Instant) {
        self.0 = Some(now);
    }

    pub fn due(&mut self, now: Instant) -> bool {
        match self.0 {
            Some(t) if now.duration_since(t) >= QUIET => {
                self.0 = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        crate::shared::paths::test_dir(tag).join("studio-recovery")
    }

    #[test]
    fn a_copy_is_written_restored_and_deleted() {
        let d = tmp("rec-roundtrip");
        assert!(pending_in(&d).is_none());
        assert!(write_in(&d, true, b"png-bytes", "[1]").unwrap());
        let p = pending_in(&d).unwrap();
        assert_eq!(
            (p.png.as_slice(), p.notes.as_str()),
            (&b"png-bytes"[..], "[1]")
        );
        // a newer write replaces it
        write_in(&d, true, b"newer", "[2]").unwrap();
        assert_eq!(pending_in(&d).unwrap().notes, "[2]");
        discard_in(&d);
        assert!(pending_in(&d).is_none());
        discard_in(&d); // nothing there is fine
    }

    #[test]
    fn off_writes_nothing() {
        let d = tmp("rec-off");
        assert!(!write_in(&d, false, b"x", "[]").unwrap());
        assert!(!d.exists());
    }

    #[test]
    fn a_crash_between_the_swaps_never_offers_a_mixed_copy() {
        let d = tmp("rec-mixed");
        write_in(&d, true, b"old", "[old]").unwrap();
        // the state after the new picture landed but before its notes did
        std::fs::remove_file(d.join(NOTES)).unwrap();
        std::fs::write(d.join(PICTURE), b"new").unwrap();
        assert!(pending_in(&d).is_none());
    }

    #[test]
    fn half_a_copy_is_not_pending() {
        let d = tmp("rec-half");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(PICTURE), b"png").unwrap();
        assert!(pending_in(&d).is_none());
    }

    #[test]
    fn the_copy_waits_for_a_quiet_moment() {
        let (mut b, t0) = (Debounce::default(), Instant::now());
        assert!(!b.due(t0));
        b.changed(t0);
        assert!(!b.due(t0 + Duration::from_secs(1)));
        b.changed(t0 + Duration::from_secs(2));
        assert!(!b.due(t0 + Duration::from_secs(4)));
        assert!(b.due(t0 + Duration::from_secs(5)));
        assert!(!b.due(t0 + Duration::from_secs(9)));
    }
}
