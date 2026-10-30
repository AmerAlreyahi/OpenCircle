//! The setup's MSI: written to a private temp folder (`Package`), what of it is
//! installed already (`installed`), and the install through Windows Installer's
//! external UI (`install`: `MsiSetExternalUIRecord` + `MsiInstallProduct`, no
//! Windows Installer window but the UAC prompt), its progress read from the
//! progress messages (`Progress`).
// The parsing is plain code (tested everywhere); only Windows installs.
#![cfg_attr(not(windows), allow(dead_code))]

use std::fs::File;
use std::io;
use std::path::PathBuf;

use super::Installed;

/// What the install reports to the setup window.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// 0..1 of the way.
    Progress(f32),
    /// Finished: done, or why not (a sentence for the user).
    Done(Result<(), String>),
}

/// The MSI on disk while the setup runs, in a folder of its own that goes with it.
/// The file stays open with writing shared to no one, so nothing can swap the
/// package between the user's UAC consent and Windows Installer reading it.
pub struct Package {
    pub path: PathBuf,
    dir: PathBuf,
    lock: Option<File>,
}

impl Package {
    pub fn write(bytes: &[u8]) -> io::Result<Package> {
        if !super::elevated() {
            // an admin never deletes in a folder the user controls (a planted link)
            remove_left_behind(&std::env::temp_dir(), STALE);
        }
        let dir = std::env::temp_dir().join(format!("OpenCircle-Setup-{}", std::process::id()));
        std::fs::create_dir(&dir)?; // fails if it exists: never someone else's folder
        let path = dir.join(format!("OpenCircle-{}-x64.msi", env!("CARGO_PKG_VERSION")));
        std::fs::write(&path, bytes)?;
        let mut open = std::fs::OpenOptions::new();
        open.read(true);
        #[cfg(windows)]
        std::os::windows::fs::OpenOptionsExt::share_mode(&mut open, 1); // FILE_SHARE_READ
        let lock = open.open(&path)?;
        Ok(Package {
            path,
            dir,
            lock: Some(lock),
        })
    }
}

/// A setup's folder older than this is no running setup's.
const STALE: std::time::Duration = std::time::Duration::from_secs(3600);

/// The folders of setups that never got to delete theirs (killed, the PC turned
/// off mid-install: `Drop` never ran) in `temp`, untouched for `stale`. A running
/// setup's is younger, and its open MSI could not be deleted anyway.
fn remove_left_behind(temp: &std::path::Path, stale: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return;
    };
    for entry in entries.flatten() {
        let ours = (entry.file_name().to_str()).is_some_and(|n| {
            n.strip_prefix("OpenCircle-Setup-")
                .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
        });
        let old = (entry.metadata().and_then(|m| m.modified()))
            .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > stale));
        // a real folder, never a link or junction (followed, it would delete elsewhere)
        if ours
            && old
            && entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink())
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        drop(self.lock.take()); // Windows deletes no open file
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Windows Installer's progress messages (fields 1-4 of each record, see
/// "Parsing Windows Installer Messages") and action data, as one 0..1 that
/// never goes back: script generation is the first tenth, the execution the rest.
#[derive(Debug, Default)]
pub struct Progress {
    total: i64,
    done: i64,
    backward: bool,
    scripting: bool,
    step: Option<i64>,
    shown: f32,
}

impl Progress {
    /// A progress record; returns the share done.
    pub fn record(&mut self, fields: [i32; 4]) -> f32 {
        let [kind, a, b, c] = fields.map(i64::from);
        match kind {
            0 => {
                self.total = a;
                self.backward = b == 1;
                self.done = if self.backward { a } else { 0 };
                self.scripting = c == 1;
                self.step = None;
            }
            1 => self.step = (b != 0).then_some(a),
            2 => self.advance(a),
            3 => self.total += a,
            _ => {}
        }
        self.share()
    }

    /// An action-data message (one more item of the current action).
    pub fn action_data(&mut self) -> f32 {
        if let Some(step) = self.step {
            self.advance(step);
        }
        self.share()
    }

    fn advance(&mut self, by: i64) {
        self.done += if self.backward { -by } else { by };
    }

    fn share(&mut self) -> f32 {
        // a rollback counts backward: the bar stays where it was
        if self.total > 0 && !self.backward {
            let part = (self.done as f32 / self.total as f32).clamp(0.0, 1.0);
            let at = if self.scripting {
                0.1 * part
            } else {
                0.1 + 0.9 * part
            };
            self.shown = self.shown.max(at);
        }
        self.shown
    }
}

/// `MsiInstallProduct`'s answer as a sentence; `last` = the installer's last
/// error message, if it sent one.
pub fn outcome(code: u32, last: Option<&str>) -> Result<(), String> {
    match code {
        0 | 3010 | 1641 => Ok(()), // done (3010 / 1641: Windows wants a restart)
        1602 | 1223 => Err("Setup was cancelled.".into()),
        1618 => {
            Err("Another installation is running. Wait for it to finish, then try again.".into())
        }
        1625 => Err(
            "Windows did not allow the install. Try again and choose Yes when Windows asks.".into(),
        ),
        1638 => Err(
            "Another version of OpenCircle is installed. Remove it in Settings > Apps first."
                .into(),
        ),
        _ => Err(match last {
            Some(text) if !text.trim().is_empty() => text.trim().to_string(),
            _ => format!("Setup could not finish (Windows Installer error {code})."),
        }),
    }
}

#[cfg(windows)]
pub use win::{
    ask_yes_no, install, install_quiet, installed, installed_size, uninstall, uninstall_quiet,
};

#[cfg(not(windows))]
pub fn installed_size(_: &Package) -> Option<u64> {
    None
}

#[cfg(not(windows))]
pub fn installed(_: &Package) -> Option<Installed> {
    None
}

#[cfg(not(windows))]
pub fn install(_: &Package, _: &str, _: bool, report: &dyn Fn(Event)) {
    report(Event::Done(Err("The setup runs on Windows only.".into())));
}

#[cfg(not(windows))]
pub fn uninstall(_: &str, report: &dyn Fn(Event)) {
    report(Event::Done(Err("The setup runs on Windows only.".into())));
}

#[cfg(not(windows))]
pub fn install_quiet(_: &Package, _: &str, _: bool) -> u32 {
    ERROR_CALL_NOT_IMPLEMENTED
}

#[cfg(not(windows))]
pub fn uninstall_quiet(_: &Package) -> u32 {
    ERROR_CALL_NOT_IMPLEMENTED
}

/// Windows' "not implemented": what the quiet setup answers off Windows.
#[cfg(not(windows))]
const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;

#[cfg(not(windows))]
pub fn ask_yes_no(_: &str, _: &str) -> bool {
    false
}

/// The install's command line: the folder (quoted), and for the very same
/// package a full repair (every file and registry entry rewritten).
pub fn command_line(folder: &str, repair: bool) -> String {
    let mut line = format!("INSTALLDIR=\"{folder}\"");
    if repair {
        line.push_str(" REINSTALL=ALL REINSTALLMODE=amus");
    }
    line
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::path::Path;

    use windows::Win32::System::ApplicationInstallationAndServicing::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU, keybd_event,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST,
        MB_YESNO, MessageBoxW, SetForegroundWindow, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        WS_VISIBLE,
    };
    use windows::core::{HSTRING, PCWSTR, PWSTR, w};

    use super::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// A string property of an MSI handle by `get(handle, buffer, len)`.
    fn read(get: impl Fn(Option<PWSTR>, *mut u32) -> u32) -> Option<String> {
        let mut buf = vec![0u16; 2048];
        let mut len = buf.len() as u32;
        (get(Some(PWSTR(buf.as_mut_ptr())), &mut len) == 0)
            .then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }

    /// The package's `UpgradeCode` and `ProductCode`.
    fn codes(package: &Path) -> Option<(String, String)> {
        let path = HSTRING::from(package.as_os_str());
        let mut handle = MSIHANDLE::default();
        // SAFETY: `path` is a NUL-terminated wide string; the handle is closed below.
        let opened = unsafe {
            MsiOpenPackageExW(
                &path,
                MSIOPENPACKAGEFLAGS_IGNOREMACHINESTATE.0 as u32,
                &mut handle,
            )
        };
        if opened != 0 {
            return None;
        }
        let property = |name: &str| {
            let name = wide(name);
            // SAFETY: a live package handle; the buffer and its length come from `read`.
            read(|buf, len| unsafe {
                MsiGetPropertyW(handle, PCWSTR(name.as_ptr()), buf, Some(len))
            })
        };
        let codes = property("UpgradeCode").zip(property("ProductCode"));
        // SAFETY: the handle opened above.
        unsafe { MsiCloseHandle(handle) };
        codes
    }

    /// What the package puts on disk: its `File` table's sizes added up.
    pub fn installed_size(package: &Package) -> Option<u64> {
        let path = HSTRING::from(package.path.as_os_str());
        let (mut db, mut view) = (MSIHANDLE::default(), MSIHANDLE::default());
        // SAFETY: plain MSI database calls on handles opened here and closed below.
        unsafe {
            if MsiOpenDatabaseW(&path, MSIDBOPEN_READONLY, &mut db) != 0 {
                return None;
            }
            let mut total = None;
            if MsiDatabaseOpenViewW(
                db,
                windows::core::w!("SELECT `FileSize` FROM `File`"),
                &mut view,
            ) == 0
                && MsiViewExecute(view, MSIHANDLE::default()) == 0
            {
                let mut sum = 0u64;
                let mut record = MSIHANDLE::default();
                while MsiViewFetch(view, &mut record) == 0 {
                    sum += MsiRecordGetInteger(record, 1).max(0) as u64;
                    MsiCloseHandle(record);
                }
                total = Some(sum);
                MsiCloseHandle(view);
            }
            MsiCloseHandle(db);
            total
        }
    }

    /// The installed product of the package's upgrade family: its code
    /// (`{GUID}` + NUL) and the code as text.
    fn related(package: &Package) -> Option<([u16; 39], String, String)> {
        let (upgrade, product) = codes(&package.path)?;
        let upgrade = wide(&upgrade);
        let mut code = [0u16; 39];
        // SAFETY: a NUL-terminated GUID string; `code` holds a GUID and its NUL.
        let found = unsafe {
            MsiEnumRelatedProductsW(PCWSTR(upgrade.as_ptr()), None, 0, PWSTR(code.as_mut_ptr()))
        };
        (found == 0).then(|| (code, String::from_utf16_lossy(&code[..38]), product))
    }

    pub fn installed(package: &Package) -> Option<Installed> {
        let (code, installed, product) = related(package)?;
        let info = |attribute: PCWSTR| {
            // SAFETY: `code` is NUL-terminated; buffer and length from `read`.
            read(|buf, len| unsafe {
                MsiGetProductInfoW(PCWSTR(code.as_ptr()), attribute, buf, Some(len))
            })
        };
        Some(Installed {
            version: info(INSTALLPROPERTY_VERSIONSTRING)?,
            folder: info(INSTALLPROPERTY_INSTALLLOCATION).filter(|f| !f.is_empty()),
            same: installed.eq_ignore_ascii_case(&product),
        })
    }

    /// What the message handler keeps between messages.
    struct Ui<'a> {
        report: &'a dyn Fn(Event),
        progress: Progress,
        last_error: Option<String>,
    }

    /// The text of a message record.
    fn text(record: MSIHANDLE) -> Option<String> {
        // SAFETY: a record handle Windows Installer passed us; buffer from `read`.
        read(|buf, len| unsafe { MsiFormatRecordW(MSIHANDLE::default(), record, buf, Some(len)) })
    }

    unsafe extern "system" fn handler(context: *mut c_void, kind: u32, record: MSIHANDLE) -> i32 {
        // SAFETY: `context` is the `Ui` that `install` keeps alive for the whole call.
        let ui = unsafe { &mut *(context as *mut Ui) };
        let message = (kind as i32) & INSTALLMESSAGE_TYPEMASK;
        if record.is_invalid() {
            return 0;
        }
        match INSTALLMESSAGE(message) {
            INSTALLMESSAGE_PROGRESS => {
                // SAFETY: fields of the record we were handed.
                let field = |i| unsafe { MsiRecordGetInteger(record, i) };
                let share = ui.progress.record([field(1), field(2), field(3), field(4)]);
                (ui.report)(Event::Progress(share));
            }
            INSTALLMESSAGE_ACTIONDATA => (ui.report)(Event::Progress(ui.progress.action_data())),
            INSTALLMESSAGE_ERROR | INSTALLMESSAGE_FATALEXIT => ui.last_error = text(record),
            // Files in use (our app is asked to quit first): let Restart Manager close them.
            INSTALLMESSAGE_RMFILESINUSE => return 1, // IDOK
            INSTALLMESSAGE_FILESINUSE => return 5,   // IDIGNORE
            _ => {}
        }
        0
    }

    pub fn install(package: &Package, folder: &str, repair: bool, report: &dyn Fn(Event)) {
        let path = HSTRING::from(package.path.as_os_str());
        let line = HSTRING::from(command_line(folder, repair));
        // SAFETY: two NUL-terminated wide strings that outlive the call.
        with_our_ui(report, || unsafe { MsiInstallProductW(&path, &line) });
    }

    /// The silent setup (`/quiet`): the same install with Windows Installer's UI
    /// level NONE (no window, no UAC prompt: needs an elevated caller, else 1925).
    /// Windows Installer's own result code.
    pub fn install_quiet(package: &Package, folder: &str, repair: bool) -> u32 {
        let path = HSTRING::from(package.path.as_os_str());
        let line = HSTRING::from(command_line(folder, repair));
        // SAFETY: two NUL-terminated wide strings that outlive the call.
        unsafe {
            MsiSetInternalUI(INSTALLUILEVEL_NONE, None);
            MsiInstallProductW(&path, &line)
        }
    }

    /// `/uninstall /quiet`: removes the installed copy of the package's family
    /// (1605 = none is installed). The user's data stays (the MSI's quiet path).
    pub fn uninstall_quiet(package: &Package) -> u32 {
        const ERROR_UNKNOWN_PRODUCT: u32 = 1605;
        let Some((code, ..)) = related(package) else {
            return ERROR_UNKNOWN_PRODUCT;
        };
        // SAFETY: `code` is a NUL-terminated GUID string.
        unsafe {
            MsiSetInternalUI(INSTALLUILEVEL_NONE, None);
            MsiConfigureProductExW(
                PCWSTR(code.as_ptr()),
                INSTALLLEVEL_DEFAULT,
                INSTALLSTATE_ABSENT,
                &HSTRING::new(),
            )
        }
    }

    /// Removes the installed product `code` (`{GUID}`) the way Apps & features
    /// would, with our window as its only UI (the uninstall's question is ours).
    pub fn uninstall(code: &str, report: &dyn Fn(Event)) {
        let code = HSTRING::from(code);
        // SAFETY: a NUL-terminated wide string that outlives the call.
        with_our_ui(report, || unsafe {
            MsiConfigureProductExW(
                &code,
                INSTALLLEVEL_DEFAULT,
                INSTALLSTATE_ABSENT,
                &HSTRING::new(),
            )
        });
    }

    /// Runs `call` (one Windows Installer action) with no Windows Installer
    /// window but the UAC prompt; its progress and its end go to `report`.
    fn with_our_ui(report: &dyn Fn(Event), call: impl FnOnce() -> u32) {
        let mut ui = Ui {
            report,
            progress: Progress::default(),
            last_error: None,
        };
        let filter = [
            INSTALLLOGMODE_PROGRESS,
            INSTALLLOGMODE_ACTIONDATA,
            INSTALLLOGMODE_ERROR,
            INSTALLLOGMODE_FATALEXIT,
            INSTALLLOGMODE_RMFILESINUSE,
            INSTALLLOGMODE_FILESINUSE,
        ]
        .iter()
        .fold(0, |all, m| all | m.0) as u32;
        // SAFETY: the handler only touches `ui`, which outlives the call;
        // both settings are this process's, set back to none after it.
        let code = unsafe {
            MsiSetInternalUI(
                INSTALLUILEVEL(INSTALLUILEVEL_NONE.0 | INSTALLUILEVEL_UACONLY.0),
                None,
            );
            MsiSetExternalUIRecord(
                Some(handler),
                filter,
                Some(&mut ui as *mut Ui as *const c_void),
                None,
            );
            let code = call();
            MsiSetExternalUIRecord(None, 0, None, None);
            code
        };
        let result = outcome(code, ui.last_error.as_deref());
        if result.is_ok() {
            report(Event::Progress(1.0));
        }
        report(Event::Done(result));
    }

    /// A plain Yes / No question in front of everything (the uninstall's).
    /// An ownerless MessageBox from this custom action opened BEHIND Windows
    /// Installer's "Please wait while Windows configures" window (the flags
    /// alone did not make it topmost there, #135). It is owned by our own
    /// topmost 0x0 window instead: a topmost owner makes the box topmost, so
    /// the installer's non-topmost window can never cover it.
    pub fn ask_yes_no(title: &str, text: &str) -> bool {
        // SAFETY: a predefined "STATIC" class window, destroyed before return;
        // two NUL-terminated wide strings.
        let answer = unsafe {
            let owner = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!(""),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .ok();
            if let Some(owner) = owner {
                // A tap of Alt lets this process take the foreground (the
                // keyboard goes to the question, not to the window behind it).
                keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
                let _ = SetForegroundWindow(owner);
            }
            let answer = MessageBoxW(
                owner,
                &HSTRING::from(text),
                &HSTRING::from(title),
                MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
            );
            if let Some(owner) = owner {
                let _ = DestroyWindow(owner);
            }
            answer
        };
        answer == IDYES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_follows_the_installers_messages() {
        let mut p = Progress::default();
        assert_eq!(p.record([0, 100, 0, 1]), 0.0); // script generation: 100 ticks
        assert!((p.record([2, 50, 0, 0]) - 0.05).abs() < 1e-6);
        // execution: 1000 ticks; an action that moves 10 per action-data message
        assert!((p.record([0, 1000, 0, 0]) - 0.1).abs() < 1e-6);
        p.record([1, 10, 1, 0]);
        for _ in 0..50 {
            p.action_data();
        }
        assert!((p.record([2, 0, 0, 0]) - (0.1 + 0.9 * 0.5)).abs() < 1e-6);
        p.record([3, 1000, 0, 0]); // more work found: the total grows, the bar never goes back
        assert!((p.record([2, 0, 0, 0]) - 0.55).abs() < 1e-6);
        assert!((p.record([2, 1500, 0, 0]) - 1.0).abs() < 1e-6);
        assert_eq!(p.record([2, 900, 0, 0]), 1.0, "clamped at the end");
    }

    #[test]
    fn a_rollback_never_moves_the_bar_back() {
        let mut p = Progress::default();
        p.record([0, 100, 0, 0]);
        let at = p.record([2, 60, 0, 0]);
        p.record([0, 100, 1, 0]); // rollback runs backward
        assert_eq!(p.record([2, 30, 0, 0]), at);
    }

    #[test]
    fn install_results_read_as_sentences() {
        assert_eq!(outcome(0, None), Ok(()));
        assert_eq!(outcome(3010, None), Ok(()));
        assert_eq!(outcome(1602, None), Err("Setup was cancelled.".into()));
        assert!(
            outcome(1618, None)
                .unwrap_err()
                .contains("Another installation")
        );
        assert_eq!(
            outcome(
                1603,
                Some(" A newer version of OpenCircle is already installed. ")
            ),
            Err("A newer version of OpenCircle is already installed.".into())
        );
        assert!(outcome(1603, None).unwrap_err().contains("1603"));
    }

    #[test]
    fn the_command_line_quotes_the_folder_and_repairs_the_same_package() {
        assert_eq!(
            command_line(r"C:\Program Files\OpenCircle\", false),
            r#"INSTALLDIR="C:\Program Files\OpenCircle\""#
        );
        assert!(command_line(r"C:\x\", true).ends_with(" REINSTALL=ALL REINSTALLMODE=amus"));
    }

    /// A killed setup's folder (seen on the VM: two from 2026-10-27) goes at the next
    /// setup's start; a fresh one (a setup running now) and other folders stay.
    #[test]
    fn a_killed_setups_folder_goes_at_the_next_start() {
        let temp = std::env::temp_dir().join(format!("oc-setup-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let names = ["OpenCircle-Setup-348", "OpenCircle-Setup-x", "Other-1"];
        for n in names {
            std::fs::create_dir_all(temp.join(n)).unwrap();
            std::fs::write(temp.join(n).join("a.msi"), b"msi").unwrap();
        }
        remove_left_behind(&temp, std::time::Duration::from_secs(3600));
        assert!(temp.join(names[0]).exists(), "a running setup's: young");
        std::thread::sleep(std::time::Duration::from_millis(20));
        remove_left_behind(&temp, std::time::Duration::ZERO);
        assert!(!temp.join(names[0]).exists(), "left behind: gone");
        assert!(temp.join(names[1]).exists() && temp.join(names[2]).exists());
        // a link with our name (planted, pointing elsewhere): never followed
        #[cfg(unix)]
        {
            let elsewhere = temp.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            std::fs::write(elsewhere.join("keep"), b"x").unwrap();
            std::os::unix::fs::symlink(&elsewhere, temp.join("OpenCircle-Setup-77")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            remove_left_behind(&temp, std::time::Duration::ZERO);
            assert!(elsewhere.join("keep").exists());
        }
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn the_package_goes_with_the_setup() {
        let package = Package::write(b"msi").unwrap();
        let dir = package.path.parent().unwrap().to_path_buf();
        assert_eq!(std::fs::read(&package.path).unwrap(), b"msi");
        drop(package);
        assert!(!dir.exists());
    }
}
