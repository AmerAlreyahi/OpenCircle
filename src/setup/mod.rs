//! OpenCircle's own Windows setup, `OpenCircle-Setup-<ver>-x64.exe`: this same
//! program built with the release MSI inside (build.rs embeds the file named by
//! `OPENCIRCLE_SETUP_MSI`; release.yml builds it after the MSI). Started with no
//! arguments it shows the setup window (`ui::screens::setup`) and installs the MSI through
//! Windows Installer's external UI (`msi`), so upgrade, repair, the uninstall in
//! Apps & features and silent IT installs (`msiexec /i … /qn INSTALLDIR=…`) stay
//! the MSI's own. The MSI's uninstall runs `opencircle.exe --forget-user-data`
//! (`FORGET_ARG`): the question whether the user's data goes too.
//!
//! Silent mode (`silent`: IT, winget): `/quiet` installs or updates with no UI,
//! `/dir="C:\path"` for a fresh install, `/uninstall /quiet` removes; the exit
//! code is Windows Installer's. The MSI is no release asset: this exe is the one
//! installer. Why embedded and not beside it: one file to download and run.
//! Why this program and not a second one: the
//! setup's window reuses the app's renderer, fonts and theme (no new toolkit);
//! the price is the app's size once more in the setup exe.

pub mod msi;

use std::path::{Path, PathBuf};

/// The hidden argument the MSI's uninstall starts the installed exe with.
pub const FORGET_ARG: &str = "--forget-user-data";

/// The hidden argument of our Apps & features entry's uninstall
/// (`opencircle.exe --uninstall-ui {ProductCode}`, written by the MSI; its own
/// entry is hidden, ARPSYSTEMCOMPONENT): our uninstall window (`ui::screens::setup`).
pub const UNINSTALL_ARG: &str = "--uninstall-ui";

/// The temp folder a copy of the exe runs the uninstall window from.
const UNINSTALL_COPY: &str = "OpenCircle-Uninstall-";

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`: a product code, nothing else.
pub fn is_product_code(code: &str) -> bool {
    let b = code.as_bytes();
    b.len() == 38
        && b[0] == b'{'
        && b[37] == b'}'
        && b[1..37].iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// What `opencircle --uninstall-ui` does next.
pub enum Uninstall {
    /// Nothing more: exit with this code (a bad argument, or the hand-over to the copy).
    Exit(std::process::ExitCode),
    /// This is the copy: `main` shows the uninstall window (`ui::setup::run_uninstall`
    /// with `code` and the installed `folder`), then calls `delete_when_gone(&dir)`.
    Window {
        code: String,
        folder: String,
        dir: PathBuf,
    },
}

/// `opencircle --uninstall-ui {ProductCode}`, from Apps & features. The
/// installed exe can't remove itself while it runs, so it copies itself to a
/// temp folder of its own, starts the copy and exits; the copy shows the
/// window (`Uninstall::Window`), then has its folder deleted once it has exited.
pub fn uninstall_ui(code: Option<&str>) -> Uninstall {
    use std::process::ExitCode;
    let Some(code) = code.filter(|c| is_product_code(c)) else {
        return Uninstall::Exit(ExitCode::from(2));
    };
    let Ok(exe) = std::env::current_exe() else {
        return Uninstall::Exit(ExitCode::FAILURE);
    };
    let copy_dir = exe.parent().filter(|d| {
        d.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(UNINSTALL_COPY))
    });
    let Some(dir) = copy_dir else {
        // the installed exe: hand over to a copy
        let dir = std::env::temp_dir().join(format!("{UNINSTALL_COPY}{}", std::process::id()));
        let copy = dir.join("opencircle.exe");
        let started = std::fs::create_dir(&dir)
            .and_then(|()| std::fs::copy(&exe, &copy))
            .and_then(|_| {
                std::process::Command::new(&copy)
                    .args([UNINSTALL_ARG, code])
                    .env("OPENCIRCLE_INSTALLED", exe.parent().unwrap_or(&dir))
                    .spawn()
            });
        return Uninstall::Exit(match started {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                crate::log!("[opencircle] uninstall: start the copy: {e}");
                let _ = std::fs::remove_dir_all(&dir);
                ExitCode::FAILURE
            }
        });
    };
    Uninstall::Window {
        code: code.to_string(),
        folder: std::env::var("OPENCIRCLE_INSTALLED").unwrap_or_default(),
        dir: dir.to_path_buf(),
    }
}

/// Deletes `dir` (this exe's own folder) a moment after this process exits:
/// a hidden `cmd` waits two seconds, then removes it.
pub fn delete_when_gone(dir: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let line = format!(
            "ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"",
            dir.display()
        );
        let _ = std::process::Command::new("cmd")
            .arg("/c")
            .raw_arg(line)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = dir;
}

/// `/sha256=<hex>`: the app's update (`update::SETUP_ARGS`) started this setup
/// elevated from the user's own (writable) download folder. Before anything runs
/// as admin, the setup copies itself into a fresh folder only admins can write,
/// checks the copy against that hash there (a file swapped meanwhile is refused)
/// and runs the copy, with its temp folder (where the MSI is unpacked) in there
/// too. Why this design: the elevated side never trusts a path the user can write,
/// and the hash comes from the app's signed checksums. `/relocated` marks the copy.
pub const SHA256_ARG: &str = "/sha256=";
const RELOCATED_ARG: &str = "/relocated";

/// Runs the relocated copy when asked (`SHA256_ARG`) and returns its exit code;
/// `None` = carry on here (not asked, or this is the copy).
pub fn relocate(args: &[String]) -> Option<u32> {
    let hash = args.iter().find_map(|a| a.strip_prefix(SHA256_ARG))?;
    if args.iter().any(|a| a == RELOCATED_ARG) {
        return None;
    }
    let Some(want) = crate::update::verify::parse_hex(hash) else {
        return Some(ERROR_INVALID_COMMAND_LINE);
    };
    let dir = admin_root().join(format!(
        "OpenCircle-Update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let (copy, held) = match std::env::current_exe().and_then(|me| copy_verified(&me, &dir, &want))
    {
        Ok(copy) => copy,
        Err(e) => {
            crate::log!("[opencircle] setup: not run: {e}");
            let _ = std::fs::remove_dir_all(&dir);
            return Some(1);
        }
    };
    let child = std::process::Command::new(&copy)
        .args(args)
        .arg(RELOCATED_ARG)
        .env("TEMP", &dir)
        .env("TMP", &dir)
        .spawn();
    drop(held); // it runs: Windows keeps a running exe's file from changing
    let code = child
        .and_then(|mut c| c.wait())
        .map_or(1, |s| s.code().unwrap_or(1) as u32);
    let _ = std::fs::remove_dir_all(&dir);
    Some(code)
}

/// Where the copy goes: `<Windows folder>\Temp` (its own new folder, admin-only, below).
fn admin_root() -> PathBuf {
    windows_dir().join("Temp")
}

/// The Windows folder from Windows itself, never from the environment: an elevated
/// process inherits the user's (HKCU\Environment may set WINDIR, SystemRoot, PATH).
#[cfg(windows)]
pub fn windows_dir() -> PathBuf {
    let mut buf = [0u16; 260];
    // SAFETY: writes at most `buf.len()` wide chars into `buf`.
    let n = unsafe {
        windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW(Some(&mut buf))
    };
    match n as usize {
        1..260 => PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])),
        _ => PathBuf::from(r"C:\Windows"),
    }
}

/// This process runs with admin rights.
pub fn elevated() -> bool {
    #[cfg(windows)]
    // SAFETY: a plain query of this process's token.
    return unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool();
    #[cfg(not(windows))]
    false
}

#[cfg(not(windows))]
pub fn windows_dir() -> PathBuf {
    PathBuf::from(r"C:\Windows")
}

/// `exe` copied into the new folder `dir` (made admin-only: `admin_only_dir`; fails if
/// it exists), the copy's SHA-256 checked there against `want`. `Err` = refused. The
/// copy comes back held open, writing and deleting shared with no one: keep it until
/// the copy has started, so the verified bytes are the ones that run.
fn copy_verified(
    exe: &Path,
    dir: &Path,
    want: &[u8; 32],
) -> std::io::Result<(PathBuf, std::fs::File)> {
    admin_only_dir(dir)?;
    let copy = dir.join("OpenCircle-Setup.exe");
    std::fs::copy(exe, &copy)?;
    let mut open = std::fs::OpenOptions::new();
    open.read(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut open, 1); // FILE_SHARE_READ
    let file = open.open(&copy)?;
    if crate::update::verify::sha256(&file)? != *want {
        drop(file);
        let _ = std::fs::remove_file(&copy);
        return Err(std::io::Error::other(
            "the setup is not the verified download",
        ));
    }
    Ok((copy, file))
}

/// A new folder that SYSTEM and Administrators may change and users only read and
/// run (a protected DACL, nothing inherited from `%windir%\Temp`, where users may
/// create files). Fails if `dir` exists (a folder someone made in advance).
#[cfg(windows)]
fn admin_only_dir(dir: &Path) -> std::io::Result<()> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::CreateDirectoryW;
    use windows::core::{HSTRING, w};
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a constant SDDL string; `sd` is freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;BU)"),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )?;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    // SAFETY: a NUL-terminated path; `attributes` points at the live descriptor.
    let made = unsafe { CreateDirectoryW(&HSTRING::from(dir.as_os_str()), Some(&attributes)) };
    // SAFETY: the descriptor the conversion allocated.
    unsafe { LocalFree(Some(HLOCAL(sd.0))) };
    made.map_err(std::io::Error::from)
}

#[cfg(not(windows))]
fn admin_only_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o755).create(dir)
}

/// What the setup build was asked to do without a window.
#[derive(Debug, PartialEq, Eq)]
pub enum Silent {
    /// Install or update (a repair when the same version is there); `dir` = a
    /// fresh install's folder, checked and tidied.
    Install {
        dir: Option<String>,
        /// `/launch` (hidden, the app's own update: `update::SETUP_ARGS`): start the
        /// installed app afterwards (the new one, or the old one if it failed).
        launch: bool,
    },
    Uninstall,
}

/// Windows Installer's "invalid command line", for a bad `/dir`.
const ERROR_INVALID_COMMAND_LINE: u32 = 1639;

/// The setup build's arguments: `None` = show the window. Only `/quiet` (or
/// `/qn`, `/silent`) asks for silence; `/uninstall`, `/dir=...` and `/norestart`
/// (nothing here restarts) matter only with it. Flags are case-insensitive and
/// may start with `/`, `-` or `--`. `Err` = the exit code for a bad `/dir`.
pub fn parse_silent(args: &[String]) -> Result<Option<Silent>, u32> {
    let (mut quiet, mut uninstall, mut dir, mut launch) = (false, false, None, false);
    for arg in args {
        let flag = arg.trim_start_matches(['/', '-']);
        let (name, value) = flag.split_once('=').unwrap_or((flag, ""));
        match name.to_ascii_lowercase().as_str() {
            "quiet" | "qn" | "silent" => quiet = true,
            "uninstall" => uninstall = true,
            "launch" => launch = true,
            "dir" => dir = Some(value.trim_matches('"')),
            _ => {}
        }
    }
    if !quiet {
        return Ok(None);
    }
    if uninstall {
        return Ok(Some(Silent::Uninstall));
    }
    let dir = dir
        .map(|d| check_folder(d).map_err(|_| ERROR_INVALID_COMMAND_LINE))
        .transpose()?;
    Ok(Some(Silent::Install { dir, launch }))
}

/// Where a silent install goes: an update or repair stays where the app is
/// (like the window's), a fresh install takes `/dir`, else the default.
fn silent_folder(installed: Option<&Installed>, dir: Option<String>) -> String {
    installed
        .and_then(|i| i.folder.as_deref())
        .and_then(|f| check_folder(f).ok())
        .or(dir)
        .unwrap_or_else(default_folder)
}

/// Runs a silent request against the embedded MSI; returns the process exit
/// code: 0 = done, else Windows Installer's error code (3010 = done, restart wanted).
pub fn silent(msi_bytes: &'static [u8], what: Silent) -> u32 {
    let package = match msi::Package::write(msi_bytes) {
        Ok(p) => p,
        Err(e) => {
            crate::log!("[opencircle] setup: cannot unpack the installer: {e}");
            return 1; // ERROR_INVALID_FUNCTION: nothing better fits
        }
    };
    quit_running_app();
    let code = match what {
        Silent::Uninstall => msi::uninstall_quiet(&package),
        Silent::Install { dir, launch } => {
            let installed = msi::installed(&package);
            let folder = silent_folder(installed.as_ref(), dir);
            let repair = installed.as_ref().is_some_and(|i| i.same);
            let code = msi::install_quiet(&package, &folder, repair);
            if launch && let Err(e) = self::launch(&app_in(&folder), false) {
                crate::log!("[opencircle] setup: {e}");
            }
            code
        }
    };
    if code != 0 {
        crate::log!("[opencircle] setup: Windows Installer error {code}");
    }
    code
}

/// The MSI this exe carries: only the setup build has one.
pub fn payload() -> Option<&'static [u8]> {
    #[cfg(setup_payload)]
    return Some(include_bytes!(env!("SETUP_MSI")));
    #[cfg(not(setup_payload))]
    None
}

/// Why a typed install folder is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadFolder {
    Empty,
    /// Not `X:\…` (relative, a network path, another OS's path).
    NotLocal,
    /// A drive's root (`C:\`): the app gets a folder of its own.
    DriveRoot,
    /// A character Windows forbids in a name (`<>"|?*`, a second `:`, control).
    BadCharacter,
    /// A name Windows reserves (`CON`, `NUL`, `COM1`, …) or one ending in a dot or space.
    ReservedName,
    TooLong,
}

/// Longest install folder we accept (MAX_PATH less room for the files below it).
const MAX_FOLDER: usize = 200;

/// The install folder the user typed or picked, checked and tidied: an absolute
/// local path (`X:\…`) below a drive's root, `/` read as `\`, doubled
/// separators dropped, with one trailing `\` (as Windows Installer's INSTALLDIR
/// wants it).
pub fn check_folder(input: &str) -> Result<String, BadFolder> {
    let input = input.trim();
    if input.is_empty() {
        return Err(BadFolder::Empty);
    }
    let path = input.replace('/', "\\");
    let mut chars = path.chars();
    let (drive, colon, slash) = (chars.next(), chars.next(), chars.next());
    if !(drive.is_some_and(|c| c.is_ascii_alphabetic())
        && colon == Some(':')
        && slash == Some('\\'))
    {
        return Err(BadFolder::NotLocal);
    }
    let names: Vec<&str> = path[3..].split('\\').filter(|n| !n.is_empty()).collect();
    if names.is_empty() {
        return Err(BadFolder::DriveRoot);
    }
    for name in &names {
        if name
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
        {
            return Err(BadFolder::BadCharacter);
        }
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit()
                && stem.as_bytes()[3] != b'0');
        if reserved || name.ends_with('.') || name.ends_with(' ') {
            return Err(BadFolder::ReservedName);
        }
    }
    let drive = drive.unwrap_or_default().to_ascii_uppercase();
    let tidy = format!("{drive}:\\{}\\", names.join("\\"));
    if tidy.chars().count() > MAX_FOLDER {
        return Err(BadFolder::TooLong);
    }
    Ok(tidy)
}

/// Where a fresh install goes: `%ProgramFiles%\OpenCircle` (the MSI is per machine).
/// Asked of Windows (the known folder), not the environment, which an elevated setup
/// inherits from the user.
pub fn default_folder() -> String {
    let programs = program_files().unwrap_or_else(|| r"C:\Program Files".into());
    format!("{}\\OpenCircle\\", programs.trim_end_matches('\\'))
}

#[cfg(windows)]
fn program_files() -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: a known folder id; the string Windows allocates is freed below.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        text
    }
}

#[cfg(not(windows))]
fn program_files() -> Option<String> {
    None
}

/// The installed app in `folder`.
pub fn app_in(folder: &str) -> PathBuf {
    Path::new(folder).join("opencircle.exe")
}

/// How long a launched app has to answer on its socket.
const START_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// The setup's "Launch OpenCircle", run off the UI thread: an OpenCircle still
/// running (the old version, still on its way out after the upgrade asked it
/// to quit) quits first, or the new one would hand itself to it and exit, and
/// then the old one goes too: nothing left running. Then the installed app
/// starts (`start`) and is waited for until it answers. `Err` = why not.
/// After an update or repair (`settings`) the app has nothing new to show on its
/// own, so Launch opens its Settings, as a second launch does (a fresh install
/// has its first-run onboarding). Done through the running app's socket, not an
/// argument: an elevated setup starts the app through Explorer, which passes none.
pub fn launch(exe: &Path, settings: bool) -> Result<(), String> {
    let show = || {
        let _ = crate::hotkey::forward(&crate::hotkey::Command::OpenSettings.line());
    };
    launch_with(
        exe,
        quit_running_app,
        start,
        crate::hotkey::running,
        START_WAIT,
        settings.then_some(&show as &dyn Fn()),
    )
}

fn launch_with(
    exe: &Path,
    quit: impl FnOnce() -> bool,
    start: impl FnOnce(&Path) -> std::io::Result<()>,
    running: impl Fn() -> bool,
    wait: std::time::Duration,
    show_settings: Option<&dyn Fn()>,
) -> Result<(), String> {
    if !quit() {
        crate::log!("[opencircle] setup: the running OpenCircle did not quit");
    }
    start(exe).map_err(|e| {
        crate::log!("[opencircle] setup: launch {}: {e}", exe.display());
        format!("OpenCircle did not start ({e}). Start it from the Start menu.")
    })?;
    let deadline = std::time::Instant::now() + wait;
    while !running() {
        if std::time::Instant::now() >= deadline {
            crate::log!("[opencircle] setup: {} did not answer", exe.display());
            return Err("OpenCircle did not start. Start it from the Start menu.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if let Some(show) = show_settings {
        show();
    }
    Ok(())
}

/// Starts `exe` as the signed-in user and returns at once: never a child of
/// the setup (no handles, no console, nothing waits for it). A setup that runs
/// elevated (started "as administrator") hands the start to Explorer, so the
/// app does not run elevated too.
fn start(exe: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    if elevated() {
        // absolute, from Windows (not the user's PATH / WINDIR); `exe` is the MSI's
        // install folder (Windows Installer's record, admin-written) + opencircle.exe
        return std::process::Command::new(windows_dir().join("explorer.exe"))
            .arg(exe)
            .spawn()
            .map(drop);
    }
    crate::sys::spawn::spawn_detached(exe)
}

/// What is installed already (the MSI's upgrade family).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    /// Where it is (`None` when Windows Installer doesn't know).
    pub folder: Option<String>,
    /// The very same package: installing it again repairs it.
    pub same: bool,
}

/// Asks a running OpenCircle to quit and waits up to 10 s for it (its files can
/// then be replaced or removed). `true` = none is running any more.
pub fn quit_running_app() -> bool {
    use crate::hotkey;
    if hotkey::forward(&hotkey::Command::Quit.line()).is_err() {
        return true; // none running
    }
    for _ in 0..50 {
        if !hotkey::running() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

/// The user's data: the settings folder and the local data (models, recent
/// captures, histories, the update cache), and the keys in the keyring.
fn user_data() -> Vec<PathBuf> {
    crate::paths::user_data()
}

/// `opencircle --forget-user-data [--quiet]`, run by the MSI's uninstall (not
/// for an upgrade), before Windows Installer looks for files in use: quits a
/// running OpenCircle, removes this copy's "Start with Windows" entry, then
/// (not `quiet`, i.e. not a silent uninstall) asks whether the user's settings,
/// downloaded models and saved keys go too.
pub fn forget_user_data(quiet: bool) -> std::process::ExitCode {
    quit_running_app();
    if let Ok(exe) = std::env::current_exe()
        && let Err(e) = crate::autostart::forget(&exe)
    {
        crate::log!("[opencircle] start with Windows: {e}");
    }
    if !quiet
        && msi::ask_yes_no(
            "Uninstall OpenCircle",
            "Also remove your OpenCircle settings, downloaded models and saved keys?\n\nChoose No to keep them for a later install.",
        )
    {
        remove_user_data();
    }
    std::process::ExitCode::SUCCESS
}

/// The user's settings, downloaded models, histories and saved keys: gone.
pub fn remove_user_data() {
    remove_dirs(&user_data());
    let owners = (crate::ai::providers::PRESETS.iter()).map(|p| crate::secrets::AiKey(p.id));
    for owner in owners.chain([crate::secrets::AiKey::SHARED]) {
        let _ = crate::secrets::delete(owner);
    }
    let _ = crate::secrets::by_name::delete(crate::music::SERVICE.key_entry());
}

fn remove_dirs(dirs: &[PathBuf]) {
    for dir in dirs {
        if dir.exists()
            && let Err(e) = std::fs::remove_dir_all(dir)
        {
            crate::log!("[opencircle] remove {}: {e}", dir.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forget-my-data removes the settings folder and BOTH data folders (the
    /// capitalised one holds the models, logs and search history), nothing else.
    #[test]
    fn forgetting_user_data_removes_both_data_folders() {
        let home = std::env::temp_dir().join(format!("oc-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let (data, config) = (home.join("data"), home.join("config"));
        let names = [
            config.join("opencircle"),
            data.join("opencircle"),
            data.join("OpenCircle"),
        ];
        let other = data.join("somebody-else");
        for d in names.iter().chain([&other]) {
            std::fs::create_dir_all(d).unwrap();
        }
        let found = crate::paths::user_data_in(Some(&data), Some(&config));
        remove_dirs(&found);
        // (a case-insensitive disk is one folder: gone either way)
        assert!(names.iter().all(|d| !d.exists()), "{found:?}");
        assert!(other.exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_good_folder_is_tidied() {
        assert_eq!(
            check_folder(r"  c:\Program Files\OpenCircle  "),
            Ok(r"C:\Program Files\OpenCircle\".into())
        );
        assert_eq!(check_folder("D:/Apps//OC/"), Ok(r"D:\Apps\OC\".into()));
        assert_eq!(
            check_folder(r"E:\Tools\Open.Circle"),
            Ok(r"E:\Tools\Open.Circle\".into())
        );
        assert_eq!(
            check_folder(r"C:\برامج\OpenCircle"),
            Ok(r"C:\برامج\OpenCircle\".into())
        );
    }

    #[test]
    fn a_bad_folder_says_why() {
        use BadFolder::*;
        for (input, why) in [
            ("", Empty),
            ("   ", Empty),
            (r"Program Files\OpenCircle", NotLocal),
            (r"\\server\share\OpenCircle", NotLocal),
            ("/opt/opencircle", NotLocal),
            ("C:", NotLocal),
            (r"C:OpenCircle", NotLocal),
            (r"C:\", DriveRoot),
            (r"C:\\", DriveRoot),
            (r"C:\Open|Circle", BadCharacter),
            (r"C:\Open?Circle", BadCharacter),
            (r"C:\a:b", BadCharacter),
            (r"C:\Apps\CON", ReservedName),
            (r"C:\Apps\nul.txt", ReservedName),
            (r"C:\Apps\COM1", ReservedName),
            (r"C:\Apps\lpt9\x", ReservedName),
            (r"C:\Apps\OpenCircle.", ReservedName),
            (r"C:\Apps\OpenCircle \x", ReservedName),
        ] {
            assert_eq!(check_folder(input), Err(why), "{input:?}");
        }
        // COM0 / CONSOLE are ordinary names
        assert!(check_folder(r"C:\Apps\COM0").is_ok());
        assert!(check_folder(r"C:\Apps\CONSOLE").is_ok());
        let long = format!(r"C:\{}", "a".repeat(MAX_FOLDER));
        assert_eq!(check_folder(&long), Err(TooLong));
    }

    #[test]
    fn the_default_folder_passes_its_own_check() {
        let folder = default_folder();
        assert_eq!(check_folder(&folder), Ok(folder.clone()), "{folder}");
        assert!(folder.ends_with(r"\OpenCircle\"));
    }

    /// Launch after an upgrade: the old copy quits BEFORE the new one starts
    /// (else the new one hands itself to the old one and exits), and Launch
    /// waits until the new one answers.
    #[test]
    fn launch_quits_the_old_copy_first_and_waits_for_the_new_one() {
        use std::cell::RefCell;
        use std::time::Duration;
        let steps = RefCell::new(Vec::new());
        let polls = RefCell::new(0);
        let result = launch_with(
            Path::new("app.exe"),
            || {
                steps.borrow_mut().push("quit");
                true
            },
            |_| {
                steps.borrow_mut().push("start");
                Ok(())
            },
            || {
                *polls.borrow_mut() += 1;
                *polls.borrow() > 2 // answers on the third look
            },
            Duration::from_secs(5),
            None,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(*steps.borrow(), ["quit", "start"]);
        assert_eq!(*polls.borrow(), 3);

        let silent = launch_with(
            Path::new("app.exe"),
            || true,
            |_| Ok(()),
            || false,
            Duration::from_millis(250),
            None,
        );
        assert!(silent.unwrap_err().contains("did not start"));
        let refused = launch_with(
            Path::new("app.exe"),
            || true,
            |_| Err(std::io::Error::other("blocked")),
            || true,
            Duration::from_secs(1),
            None,
        );
        assert!(refused.unwrap_err().contains("blocked"));
    }

    /// After an update or repair Launch opens Settings once the app answers (and
    /// not before); after a fresh install it asks for nothing (onboarding shows).
    #[test]
    fn launch_opens_settings_after_an_update_only() {
        use std::cell::Cell;
        use std::time::Duration;
        for (settings, shown) in [(true, 1), (false, 0)] {
            let (up, opened) = (Cell::new(false), Cell::new(0));
            let show = || {
                assert!(up.get(), "Settings is asked for once the app answers");
                opened.set(opened.get() + 1);
            };
            let result = launch_with(
                Path::new("app.exe"),
                || true,
                |_| {
                    up.set(true);
                    Ok(())
                },
                || up.get(),
                Duration::from_secs(1),
                settings.then_some(&show as &dyn Fn()),
            );
            assert_eq!(result, Ok(()));
            assert_eq!(opened.get(), shown);
        }
    }

    #[test]
    fn the_uninstall_takes_only_a_product_code() {
        assert!(is_product_code("{046BC980-BD0C-45D2-B990-202898DD0BAD}"));
        for bad in [
            "",
            "046BC980-BD0C-45D2-B990-202898DD0BAD",
            "{046BC980-BD0C-45D2-B990-202898DD0BA}",
            "{046BC980-BD0C-45D2-B990-202898DD0BAX}",
            "{046BC980BBD0C-45D2-B990-202898DD0BAD}",
            "{046BC980-BD0C-45D2-B990-202898DD0BAD} /qn",
            "/x {046BC980-BD0C-45D2-B990-202898DD0BAD}",
        ] {
            assert!(!is_product_code(bad), "{bad:?}");
        }
    }

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn silent_flags_are_read_and_only_quiet_hides_the_window() {
        assert_eq!(parse_silent(&[]), Ok(None));
        assert_eq!(parse_silent(&args(r"/uninstall /dir=C:\x")), Ok(None));
        assert_eq!(
            parse_silent(&args("/quiet")),
            Ok(Some(Silent::Install {
                dir: None,
                launch: false
            }))
        );
        assert_eq!(
            parse_silent(&args("/QUIET /norestart")),
            Ok(Some(Silent::Install {
                dir: None,
                launch: false
            }))
        );
        assert_eq!(
            parse_silent(&args(r#"--silent /dir="d:/Apps/OC""#)),
            Ok(Some(Silent::Install {
                dir: Some(r"D:\Apps\OC\".into()),
                launch: false
            }))
        );
        // the app's own update (`update::SETUP_ARGS`): install, then start the app
        let update: Vec<String> = (crate::update::SETUP_ARGS.split(' '))
            .map(String::from)
            .collect();
        assert_eq!(
            parse_silent(&update),
            Ok(Some(Silent::Install {
                dir: None,
                launch: true
            }))
        );
        assert_eq!(
            parse_silent(&args("/launch")),
            Ok(None),
            "never without /quiet"
        );
        assert_eq!(
            parse_silent(&args("/uninstall /quiet")),
            Ok(Some(Silent::Uninstall))
        );
        assert_eq!(
            parse_silent(&args("/quiet /dir=relative")),
            Err(ERROR_INVALID_COMMAND_LINE)
        );
    }

    #[test]
    fn a_silent_update_stays_where_the_app_is() {
        let at = |f: Option<&str>| Installed {
            version: "0.1.0".into(),
            folder: f.map(String::from),
            same: false,
        };
        let dir = || Some(r"D:\New\".to_string());
        assert_eq!(
            silent_folder(Some(&at(Some(r"C:\Old\"))), dir()),
            r"C:\Old\"
        );
        assert_eq!(silent_folder(Some(&at(None)), dir()), r"D:\New\");
        assert_eq!(silent_folder(None, dir()), r"D:\New\");
        assert_eq!(silent_folder(None, None), default_folder());
    }

    /// The elevated setup runs only a copy it verified in a folder it made itself.
    #[test]
    fn the_update_setup_runs_only_its_verified_copy() {
        let base = std::env::temp_dir().join(format!("oc-relocate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let exe = base.join("OpenCircle-Setup-9.9.9-x64.exe");
        std::fs::write(&exe, b"setup").unwrap();
        let good = crate::update::verify::sha256(&b"setup"[..]).unwrap();
        let (copy, _held) = copy_verified(&exe, &base.join("ok"), &good).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"setup");
        assert_eq!(copy.parent(), Some(base.join("ok").as_path()));
        // swapped after the app verified it: refused, nothing left to run
        std::fs::write(&exe, b"evil").unwrap();
        let err = copy_verified(&exe, &base.join("bad"), &good).unwrap_err();
        assert!(err.to_string().contains("not the verified"), "{err}");
        assert!(!base.join("bad").join("OpenCircle-Setup.exe").exists());
        // a folder that is there already (made in advance by someone): refused
        assert!(copy_verified(&exe, &base.join("ok"), &good).is_err());
        let _ = std::fs::remove_dir_all(&base);

        let args = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert_eq!(relocate(&args("/quiet")), None, "not asked: runs here");
        assert_eq!(
            relocate(&args("/quiet /sha256=nothex")),
            Some(ERROR_INVALID_COMMAND_LINE),
            "a bad hash never runs unrelocated"
        );
        let hex = "ab".repeat(32);
        assert_eq!(
            relocate(&args(&format!("/quiet /sha256={hex} /relocated"))),
            None,
            "the copy itself"
        );
        assert!(admin_root().ends_with("Temp"));
    }

    /// The elevated setup inherits the user's environment (HKCU\Environment): the
    /// folders it trusts never come from it.
    #[test]
    fn the_elevated_setup_never_takes_folders_from_the_environment() {
        let planted = r"C:\Users\u\AppData\Local\Planted";
        // SAFETY: no other test reads these variables.
        unsafe {
            for var in ["WINDIR", "SystemRoot", "ProgramW6432", "ProgramFiles"] {
                std::env::set_var(var, planted);
            }
        }
        for path in [
            admin_root().display().to_string(),
            windows_dir().join("explorer.exe").display().to_string(),
            default_folder(),
        ] {
            assert!(!path.contains("Planted"), "{path}");
        }
    }

    #[test]
    fn only_the_setup_build_carries_an_msi() {
        assert_eq!(payload().is_some(), cfg!(setup_payload));
    }
}
