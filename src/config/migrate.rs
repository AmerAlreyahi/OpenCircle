//! Schema migration: an older file is backed up, then rewritten as the current one.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;

/// The schema version a file says it has (missing = 1, the first release), or
/// `None` when it is not a table or says something else than a number.
pub(super) fn file_version(text: &str) -> Option<i64> {
    let root: Table = text.parse().ok()?;
    match root.get("version") {
        None => Some(1),
        Some(v) => v.as_integer(),
    }
}

/// A file written by an older schema is copied to `config.toml.bak-v<old>` and then
/// rewritten as `config` (read from that file without warnings, new keys at their
/// defaults). A failed copy leaves the file as it is: no backup, no rewrite. Every
/// later schema step adds its defaults in `parse`; this only has to carry them to disk.
pub(super) fn migrate(path: &Path, text: &str, config: &Config) {
    let Some(old) = file_version(text).filter(|v| (1..VERSION as i64).contains(v)) else {
        return;
    };
    let Some(backup) = backup_old(path, text, old) else {
        crate::log!("[opencircle] config: could not back up the v{old} file; leaving it as it is");
        return;
    };
    match config.save_to(path) {
        Ok(()) => crate::log!(
            "[opencircle] config: updated from schema {old} to {VERSION} (old file: {})",
            backup.display()
        ),
        Err(e) => crate::log!("[opencircle] config: could not write the updated file ({e})"),
    }
}

/// Writes `text` next to `path` as `<name>.bak-v<old>` (`-2`, `-3` ... if taken: an
/// existing backup is never overwritten). `None` if it cannot be written.
pub(super) fn backup_old(path: &Path, text: &str, old: i64) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    for n in 1..100 {
        let tag = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        let backup = path.with_file_name(format!("{name}.bak-v{old}{tag}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(mut f) => {
                let written = f.write_all(text.as_bytes()).and_then(|()| f.sync_all());
                return written.is_ok().then_some(backup);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
    }
    None
}
