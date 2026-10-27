//! Save: the crop as a PNG (or JPEG, `snip.format`) in Settings -> General's save
//! folder (else Pictures/OpenCircle), named OpenCircle-date-time. Scaled to `snip.max_edge` unless `snip.save_original`.
use std::path::{Path, PathBuf};

use super::{Action, Crop, Key};
use crate::config::{Format, SnipConfig};

pub const ACTION: Action = Action {
    id: "save",
    label: "Save",
    key: Key {
        ctrl: true,
        letter: 's',
    },
    icon: '\u{e0b6}', // lucide "download"
    run: super::Run::Finish(run),
};

/// Ctrl+S: straight into the save folder (no dialog); Ok = the pill's text.
fn run(crop: Crop) -> Result<String, String> {
    let config = crate::config::current();
    let (crop, format) = export(crop, &config.snip);
    let folder = start_folder(&config.general.save_folder).ok_or(NOT_SAVED)?;
    write_into(&folder, &crop, format)
}

fn write_into(folder: &Path, crop: &Crop, format: Format) -> Result<String, String> {
    let ext = match format {
        Format::Png => "png",
        Format::Jpeg => "jpg",
    };
    let stamp = timestamp();
    super::encode(crop, format)
        .and_then(|bytes| write_new(folder, &stamp, ext, &bytes).map_err(|e| e.to_string()))
        .map_err(|e| {
            crate::log!("save failed: {e}");
            NOT_SAVED.to_string()
        })?;
    Ok(format!("Saved to {}", short_path(folder)))
}

/// Writes `OpenCircle-<stamp>.<ext>`, or `-2`, `-3`, ... if that name is taken:
/// a second save in the same second never overwrites the first.
fn write_new(folder: &Path, stamp: &str, ext: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    for n in 1.. {
        let suffix = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        let path = folder.join(format!("OpenCircle-{stamp}{suffix}.{ext}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => return file.write_all(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// The error pill's text (the cause goes to the log).
const NOT_SAVED: &str = "Couldn't save. Pick a folder you can write to in Settings.";

/// `folder` for a pill: all of it, or its tail after an ellipsis.
fn short_path(folder: &Path) -> String {
    const MAX: usize = 36;
    let full = folder.display().to_string();
    let n = full.chars().count();
    match n > MAX {
        true => format!("…{}", full.chars().skip(n - (MAX - 1)).collect::<String>()),
        false => full,
    }
}

/// What Save writes: the crop at the export size (or original), and its format.
fn export(crop: Crop, snip: &SnipConfig) -> (Crop, Format) {
    let max = if snip.save_original { 0 } else { snip.max_edge };
    (super::fit_long_edge(crop, max), snip.format)
}

/// Where saves go when no folder is set: the Pictures folder + `OpenCircle`.
pub fn default_folder() -> Option<PathBuf> {
    dirs::picture_dir().map(|p| p.join("OpenCircle"))
}

/// The folder saves go to: the configured one if set and existing, else the
/// default folder, created on first use.
fn start_folder(configured: &str) -> Option<PathBuf> {
    start_in(configured, default_folder())
}

fn start_in(configured: &str, default: Option<PathBuf>) -> Option<PathBuf> {
    let folder = PathBuf::from(configured);
    if !configured.is_empty() && folder.is_dir() {
        return Some(folder);
    }
    let folder = default?;
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

/// Local time as `YYYY-MM-DD_HH-MM-SS` (sorts by name, valid on every filesystem).
fn timestamp() -> String {
    let (y, mo, d, h, mi, s) = local_now();
    format!("{y:04}-{mo:02}-{d:02}_{h:02}-{mi:02}-{s:02}")
}

#[cfg(unix)]
fn local_now() -> (i32, u32, u32, u32, u32, u32) {
    // SAFETY: `time(NULL)` only reads the clock; `localtime_r` writes into our `tm`.
    let tm = unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm
    };
    let field = |v: libc::c_int| v.max(0) as u32;
    (
        tm.tm_year + 1900,
        field(tm.tm_mon + 1),
        field(tm.tm_mday),
        field(tm.tm_hour),
        field(tm.tm_min),
        field(tm.tm_sec),
    )
}

#[cfg(windows)]
fn local_now() -> (i32, u32, u32, u32, u32, u32) {
    // SAFETY: plain query of the local clock.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (
        t.wYear.into(),
        t.wMonth.into(),
        t.wDay.into(),
        t.wHour.into(),
        t.wMinute.into(),
        t.wSecond.into(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_dialog_starts_in_the_configured_folder_when_it_exists() {
        let dir = std::env::temp_dir();
        let configured = dir.to_string_lossy();
        assert_eq!(super::start_folder(&configured), Some(dir.clone()));
        // Not set, missing, or a file: the default folder, created on first save.
        let want = dir
            .join(format!("oc-pictures-{}", std::process::id()))
            .join("OpenCircle");
        let gone = dir.join(format!("oc-no-such-folder-{}", std::process::id()));
        let file = dir.join(format!("oc-save-test-{}", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        assert!(!want.exists());
        for unset in ["", &*gone.to_string_lossy(), &*file.to_string_lossy()] {
            assert_eq!(
                super::start_in(unset, Some(want.clone())),
                Some(want.clone())
            );
        }
        assert!(want.is_dir());
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(want.parent().unwrap());
    }

    #[test]
    fn save_writes_into_the_folder_and_a_bad_folder_is_an_error_not_a_panic() {
        let crop = || super::Crop {
            width: 4,
            height: 4,
            rgba: vec![200; 64],
        };
        let dir = std::env::temp_dir().join(format!("oc-save-into-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let said = super::write_into(&dir, &crop(), crate::config::Format::Png).unwrap();
        assert!(said.starts_with("Saved to "), "{said}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        let said = super::write_into(&dir, &crop(), crate::config::Format::Png);
        assert_eq!(said.unwrap_err(), super::NOT_SAVED, "folder is gone");
    }

    #[test]
    fn two_saves_in_one_second_keep_both_files() {
        let dir = std::env::temp_dir().join(format!("oc-save-twice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        super::write_new(&dir, "2026-01-01_00-00-00", "png", b"first").unwrap();
        super::write_new(&dir, "2026-01-01_00-00-00", "png", b"second").unwrap();
        let read = |n: &str| std::fs::read(dir.join(n)).unwrap();
        assert_eq!(read("OpenCircle-2026-01-01_00-00-00.png"), b"first");
        assert_eq!(read("OpenCircle-2026-01-01_00-00-00-2.png"), b"second");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_folder_is_cut_to_its_tail_for_the_pill() {
        let long = std::path::Path::new("/home/someone/a/very/long/path/to/Pictures/OpenCircle");
        let short = super::short_path(long);
        assert_eq!(short.chars().count(), 36);
        assert!(short.starts_with('…') && short.ends_with("Pictures/OpenCircle"));
        assert_eq!(super::short_path(std::path::Path::new("/tmp/x")), "/tmp/x");
    }

    #[test]
    fn the_default_folder_is_pictures_opencircle() {
        if let Some(p) = super::default_folder() {
            assert!(p.ends_with("OpenCircle"));
            assert_eq!(p.parent(), dirs::picture_dir().as_deref());
        }
    }

    #[test]
    fn save_scales_unless_the_original_size_is_asked_for() {
        use crate::config::{Format, SnipConfig};
        let crop = || super::Crop {
            width: 3000,
            height: 1000,
            rgba: vec![9; 3000 * 1000 * 4],
        };
        let mut snip = SnipConfig::default();
        let (c, f) = super::export(crop(), &snip);
        assert_eq!((c.width, c.height, f), (1568, 523, Format::Png));
        snip.save_original = true;
        snip.format = Format::Jpeg;
        let (c, f) = super::export(crop(), &snip);
        assert_eq!((c.width, c.height, f), (3000, 1000, Format::Jpeg));
    }

    #[test]
    fn timestamp_is_a_date_and_time() {
        let t = super::timestamp();
        let b = t.as_bytes();
        assert_eq!(b.len(), 19, "{t}");
        assert!(t.starts_with("20"), "{t}");
        for (i, c) in t.char_indices() {
            match i {
                4 | 7 | 13 | 16 => assert_eq!(c, '-', "{t}"),
                10 => assert_eq!(c, '_', "{t}"),
                _ => assert!(c.is_ascii_digit(), "{t}"),
            }
        }
    }
}
