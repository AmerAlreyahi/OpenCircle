//! Start OpenCircle with the OS (`general.autostart`, on by default): Windows
//! `HKCU\...\Run` value "OpenCircle" = the quoted path of the running exe;
//! Linux `~/.config/autostart/com.opencircle.app.desktop`. A bare launch is
//! already silent (tray only, no window), so the entry has no arguments.
//! `apply` writes or removes it when the setting changes and on every start
//! (an exe that moved or was updated gets its path fixed). Release builds only:
//! a dev or test build never touches the machine's entries.
use std::io;
use std::path::Path;

use crate::config::Config;

/// Brings the OS entry in line with the setting (errors are logged, never fatal).
pub fn apply(config: &Config) {
    if cfg!(any(test, debug_assertions)) {
        return;
    }
    let result = std::env::current_exe().and_then(|exe| {
        let exe = std::env::var_os("APPIMAGE").map_or(exe, Into::into); // not the mount
        sync(&Store::real(), &exe, config.general.autostart)
    });
    if let Err(e) = result {
        eprintln!("[opencircle] start with the OS: {e}");
    }
}

/// Writes the entry for `exe` when `on` and it differs, removes it when off.
fn sync(store: &Store, exe: &Path, on: bool) -> io::Result<()> {
    let want = on.then(|| store.entry(exe));
    match (want, store.read()) {
        (Some(want), have) if have.as_ref() != Some(&want) => store.write(&want),
        (None, Some(_)) => store.remove(),
        _ => Ok(()),
    }
}

use self::os::Store;

#[cfg(windows)]
mod os {
    use std::io;
    use std::path::Path;

    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows::core::PCWSTR;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "OpenCircle";

    /// A registry key under HKCU holding the value.
    pub struct Store {
        pub(super) subkey: String,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn check(e: windows::Win32::Foundation::WIN32_ERROR) -> io::Result<()> {
        match e.0 {
            0 => Ok(()),
            code => Err(io::Error::from_raw_os_error(code as i32)),
        }
    }

    impl Store {
        pub fn real() -> Store {
            Store { subkey: RUN.into() }
        }

        /// The Run value: the quoted path.
        pub fn entry(&self, exe: &Path) -> String {
            format!("\"{}\"", exe.display())
        }

        pub fn read(&self) -> Option<String> {
            let (key, name) = (wide(&self.subkey), wide(NAME));
            let mut buf = [0u16; 1024];
            let mut bytes = std::mem::size_of_val(&buf) as u32;
            // SAFETY: plain registry query into a buffer of `bytes` bytes.
            let e = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    PCWSTR(name.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut bytes),
                )
            };
            let len = (bytes as usize / 2).min(buf.len());
            (e.0 == 0).then(|| {
                String::from_utf16_lossy(&buf[..len])
                    .trim_end_matches('\0')
                    .into()
            })
        }

        pub fn write(&self, entry: &str) -> io::Result<()> {
            let (key, name, data) = (wide(&self.subkey), wide(NAME), wide(entry));
            // SAFETY: `data` is a NUL-terminated UTF-16 string of the size given.
            check(unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    PCWSTR(name.as_ptr()),
                    REG_SZ.0,
                    Some(data.as_ptr().cast()),
                    (data.len() * 2) as u32,
                )
            })
        }

        pub fn remove(&self) -> io::Result<()> {
            let (key, name) = (wide(&self.subkey), wide(NAME));
            // SAFETY: plain registry call on NUL-terminated names.
            check(unsafe {
                RegDeleteKeyValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    PCWSTR(name.as_ptr()),
                )
            })
        }
    }

    #[cfg(test)]
    pub fn temp_store() -> Store {
        // a throwaway key, never the real Run key
        Store {
            subkey: format!(r"Software\OpenCircleTest\autostart-{}", std::process::id()),
        }
    }

    #[cfg(test)]
    pub fn drop_temp(store: &Store) {
        use windows::Win32::System::Registry::RegDeleteTreeW;
        let key = wide(&store.subkey);
        // SAFETY: plain registry call on a NUL-terminated name.
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
    }
}

#[cfg(not(windows))]
mod os {
    use std::io;
    use std::path::{Path, PathBuf};

    /// The `.desktop` file in the user's autostart folder.
    pub struct Store {
        pub(super) file: PathBuf,
    }

    impl Store {
        pub fn real() -> Store {
            let dir = dirs::config_dir().unwrap_or_default().join("autostart");
            Store {
                file: dir.join("com.opencircle.app.desktop"),
            }
        }

        pub fn entry(&self, exe: &Path) -> String {
            let exe = exe
                .display()
                .to_string()
                .replace('\\', r"\\")
                .replace('"', r#"\""#);
            format!(
                "[Desktop Entry]\nType=Application\nName=OpenCircle\n\
                 Comment=Circle anything on screen\nExec=\"{exe}\"\n\
                 Icon=com.opencircle.app\nTerminal=false\n\
                 X-GNOME-Autostart-enabled=true\n"
            )
        }

        pub fn read(&self) -> Option<String> {
            std::fs::read_to_string(&self.file).ok()
        }

        pub fn write(&self, entry: &str) -> io::Result<()> {
            if let Some(dir) = self.file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&self.file, entry)
        }

        pub fn remove(&self) -> io::Result<()> {
            std::fs::remove_file(&self.file)
        }
    }

    #[cfg(test)]
    pub fn temp_store() -> Store {
        let dir = std::env::temp_dir().join(format!("oc-autostart-{}", std::process::id()));
        Store {
            file: dir.join("autostart").join("com.opencircle.app.desktop"),
        }
    }

    #[cfg(test)]
    pub fn drop_temp(store: &Store) {
        let _ = std::fs::remove_dir_all(store.file.parent().and_then(Path::parent).unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_writes_fixes_and_removes_the_entry() {
        assert!(Config::default().general.autostart, "on by default");
        let store = os::temp_store();
        let exe = Path::new(if cfg!(windows) {
            r"C:\Program Files\OpenCircle\opencircle.exe"
        } else {
            "/opt/Open Circle/opencircle"
        });
        assert_eq!(store.read(), None);
        sync(&store, exe, false).unwrap(); // off and absent: nothing to do
        assert_eq!(store.read(), None);
        sync(&store, exe, true).unwrap();
        let first = store.read().expect("written");
        assert!(first.contains(&exe.display().to_string()), "{first}");
        assert!(first.contains('"'), "the path is quoted: {first}");
        // the exe moved or was updated: the path is fixed
        let moved = Path::new(if cfg!(windows) {
            r"D:\OpenCircle\opencircle.exe"
        } else {
            "/home/me/opencircle"
        });
        sync(&store, moved, true).unwrap();
        let second = store.read().unwrap();
        assert_ne!(second, first);
        assert!(second.contains(&moved.display().to_string()));
        // off: gone; off again: still fine
        sync(&store, moved, false).unwrap();
        assert_eq!(store.read(), None);
        sync(&store, moved, false).unwrap();
        os::drop_temp(&store);
    }

    #[test]
    fn apply_never_touches_the_machine_in_tests() {
        // (a test or debug build returns first; this would write the real entry)
        let before = Store::real().read();
        apply(&Config::default());
        assert_eq!(Store::real().read(), before);
    }
}
