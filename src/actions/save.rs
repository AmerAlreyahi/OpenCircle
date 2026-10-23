//! Save: the crop as a PNG, wherever the user picks in the native save dialog.
use super::{Action, Crop, Key, write_png};

pub const ACTION: Action = Action {
    id: "save",
    label: "Save",
    tip: "Save the selection as a PNG file",
    key: Key {
        ctrl: true,
        letter: 's',
    },
    icon: '\u{e0b6}', // lucide "download"
    run,
};

fn run(crop: Crop) -> Result<(), String> {
    let name = format!("OpenCircle-{}.png", timestamp());
    // The dialog blocks until the user answers: keep it off the UI thread.
    std::thread::Builder::new()
        .name("save dialog".into())
        .spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Save selection")
                .set_file_name(name)
                .add_filter("PNG image", &["png"])
                .save_file()
            else {
                return; // cancelled
            };
            if let Err(e) = write_png(&path, crop.width, crop.height, &crop.rgba) {
                // ponytail: log only; show it in a status pill once one exists.
                eprintln!("save failed: {e}");
            }
        })
        .map(drop)
        .map_err(|e| e.to_string())
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
