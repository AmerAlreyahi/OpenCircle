//! Putting a verified file in place of the running program, and taking it back.
//!
//! `replace`: the new file is copied beside the target, the target is renamed to
//! `<target>.old` (Windows allows renaming a running exe; Linux keeps the running
//! AppImage's inode), then the copy is renamed onto the target. The old file stays
//! until the new program has started (`Marker`); `restore` puts it back.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `<target>.old`: the previous program while an update is unconfirmed.
pub fn backup_of(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(".old");
    name.into()
}

fn sibling(target: &Path, ext: &str) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(ext);
    name.into()
}

/// Puts `new` at `target`, keeping the old file as `backup_of(target)`. The copy
/// beside the target must hash to `sha256` (the verified bytes), else nothing
/// changes. On any failure `target` is as before.
pub fn replace(target: &Path, new: &Path, sha256: &[u8; 32]) -> io::Result<PathBuf> {
    let staged = sibling(target, ".new");
    let backup = backup_of(target);
    let _ = std::fs::remove_file(&staged);
    std::fs::copy(new, &staged)?;
    let copied = std::fs::File::open(&staged).and_then(super::verify::sha256);
    if copied.as_ref().ok() != Some(sha256) {
        let _ = std::fs::remove_file(&staged);
        return Err(io::Error::other("the file changed after it was verified"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = std::fs::remove_file(&backup); // a confirmed update's leftover
    if let Err(e) = std::fs::rename(target, &backup) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&staged, target) {
        let _ = std::fs::rename(&backup, target);
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    Ok(backup)
}

/// The old program back at `target` (the new one, maybe running, moves aside to
/// `<target>.bad` and is deleted when it can be).
pub fn restore(target: &Path, backup: &Path) -> io::Result<()> {
    let bad = sibling(target, ".bad");
    let _ = std::fs::remove_file(&bad);
    if target.exists() {
        std::fs::rename(target, &bad)?;
    }
    std::fs::rename(backup, target)?;
    let _ = std::fs::remove_file(&bad);
    Ok(())
}

/// What the old process leaves for the new one: the update to confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub version: String,
    pub target: PathBuf,
    pub backup: PathBuf,
    pub notes: String,
    pub url: String,
}

impl Marker {
    /// `<local data dir>/opencircle/update.json`.
    pub fn path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("opencircle").join("update.json"))
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec(self).map_err(io::Error::other)?)
    }

    pub fn read(path: &Path) -> Option<Marker> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
}

/// The new program started: the marker and the old program go. Returns the
/// marker (the notes to show), or `None` when no update was waiting.
pub fn confirm(path: &Path) -> Option<Marker> {
    let marker = Marker::read(path)?;
    let _ = std::fs::remove_file(path);
    // The old process may still be exiting (Windows locks a running exe): a
    // leftover `.old` is deleted by the next update.
    let _ = std::fs::remove_file(&marker.backup);
    Some(marker)
}

/// The new program could not start: the old one back, the marker gone.
pub fn roll_back(path: &Path) -> Option<Marker> {
    let marker = Marker::read(path)?;
    let _ = std::fs::remove_file(path);
    match restore(&marker.target, &marker.backup) {
        Ok(()) => crate::log!("[opencircle] update {}: rolled back", marker.version),
        Err(e) => crate::log!(
            "[opencircle] update {}: roll back failed: {e}",
            marker.version
        ),
    }
    Some(marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::release::tests::temp;

    #[test]
    fn replace_keeps_the_old_file_and_restore_brings_it_back() {
        let dir = temp("swap");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("opencircle.exe");
        let new = dir.join("download.exe");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        let hash = crate::update::verify::sha256(&b"new"[..]).unwrap();

        let backup = replace(&target, &new, &hash).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        assert!(!sibling(&target, ".new").exists());

        let marker_path = dir.join("update.json");
        let marker = Marker {
            version: "9.9.9".into(),
            target: target.clone(),
            backup: backup.clone(),
            notes: "n".into(),
            url: "u".into(),
        };
        marker.write(&marker_path).unwrap();
        assert_eq!(roll_back(&marker_path), Some(marker.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), b"old", "rolled back");
        assert!(!backup.exists() && !marker_path.exists());
        assert_eq!(roll_back(&marker_path), None, "nothing waiting");

        // A confirmed update: the old file and the marker go, the new one stays.
        let backup = replace(&target, &new, &hash).unwrap();
        marker.write(&marker_path).unwrap();
        assert_eq!(
            confirm(&marker_path).map(|m| m.version),
            Some("9.9.9".into())
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!backup.exists() && !marker_path.exists());
        assert_eq!(confirm(&marker_path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_replace_leaves_the_target_alone() {
        let dir = temp("swapfail");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("opencircle.exe");
        std::fs::write(&target, b"old").unwrap();
        assert!(replace(&target, &dir.join("missing"), &[0; 32]).is_err());
        // changed after it was verified: refused, nothing moved
        let new = dir.join("download.exe");
        std::fs::write(&new, b"swapped").unwrap();
        let verified = crate::update::verify::sha256(&b"verified"[..]).unwrap();
        let err = replace(&target, &new, &verified).unwrap_err();
        assert!(err.to_string().contains("changed after"), "{err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(!sibling(&target, ".new").exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(!backup_of(&target).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
