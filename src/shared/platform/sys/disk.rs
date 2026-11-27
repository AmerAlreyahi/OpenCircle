//! Free space on the volume a path is on (the recording's disk watch).
use std::path::Path;

/// Free bytes a normal user can write at `path` (a folder or a file in it); none when unknown.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let dir = match path.is_dir() {
        true => path,
        false => path.parent()?,
    };
    imp(dir)
}

#[cfg(unix)]
fn imp(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: a plain statvfs on a NUL-terminated path into a zeroed struct.
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(c.as_ptr(), &mut st) == 0).then(|| st.f_bavail as u64 * st.f_frsize as u64)
    }
}

#[cfg(windows)]
fn imp(dir: &Path) -> Option<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide = super::wide(&dir.to_string_lossy());
    let mut free = 0u64;
    // SAFETY: a NUL-terminated path and one out pointer.
    unsafe {
        GetDiskFreeSpaceExW(
            windows::core::PCWSTR(wide.as_ptr()),
            Some(&mut free),
            None,
            None,
        )
        .ok()?;
    }
    Some(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temp_folder_has_free_space_and_a_missing_one_none() {
        assert!(free_bytes(&std::env::temp_dir()).is_some_and(|b| b > 0));
        assert_eq!(free_bytes(Path::new("/definitely/not/here/x.mp4")), None);
    }
}
