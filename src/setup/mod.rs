//! OpenCircle's own Windows setup, `OpenCircle-Setup-<ver>-x64.exe`: this same
//! program built with the release MSI inside (build.rs embeds the file named by
//! `OPENCIRCLE_SETUP_MSI`; release.yml builds it after the MSI). Started with no
//! arguments it shows the setup window (`ui::setup`) and installs the MSI through
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
/// entry is hidden, ARPSYSTEMCOMPONENT): our uninstall window (`ui::setup`).
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

/// `opencircle --uninstall-ui {ProductCode}`, from Apps & features. The
/// installed exe can't remove itself while it runs, so it copies itself to a
/// temp folder of its own, starts the copy and exits; the copy shows the
/// window (`ui::setup::run_uninstall`), then has its folder deleted once it
/// has exited.
pub fn uninstall_ui(code: Option<&str>) -> std::process::ExitCode {
    use std::process::ExitCode;
    let Some(code) = code.filter(|c| is_product_code(c)) else {
        return ExitCode::from(2);
    };
    let Ok(exe) = std::env::current_exe() else {
        return ExitCode::FAILURE;
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
        return match started {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                crate::log!("[opencircle] uninstall: start the copy: {e}");
                let _ = std::fs::remove_dir_all(&dir);
                ExitCode::FAILURE
            }
        };
    };
    let folder = std::env::var("OPENCIRCLE_INSTALLED").unwrap_or_default();
    if let Err(e) = crate::ui::setup::run_uninstall(code.to_string(), folder) {
        crate::log!("[opencircle] uninstall: {e}");
    }
    delete_when_gone(dir);
    ExitCode::SUCCESS
}

/// Deletes `dir` (this exe's own folder) a moment after this process exits:
/// a hidden `cmd` waits two seconds, then removes it.
fn delete_when_gone(dir: &Path) {
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

/// What the setup build was asked to do without a window.
#[derive(Debug, PartialEq, Eq)]
pub enum Silent {
    /// Install or update (a repair when the same version is there); `dir` = a
    /// fresh install's folder, checked and tidied.
    Install {
        dir: Option<String>,
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
    let (mut quiet, mut uninstall, mut dir) = (false, false, None);
    for arg in args {
        let flag = arg.trim_start_matches(['/', '-']);
        let (name, value) = flag.split_once('=').unwrap_or((flag, ""));
        match name.to_ascii_lowercase().as_str() {
            "quiet" | "qn" | "silent" => quiet = true,
            "uninstall" => uninstall = true,
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
    Ok(Some(Silent::Install { dir }))
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
        Silent::Install { dir } => {
            let installed = msi::installed(&package);
            let folder = silent_folder(installed.as_ref(), dir);
            let repair = installed.as_ref().is_some_and(|i| i.same);
            msi::install_quiet(&package, &folder, repair)
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
pub fn default_folder() -> String {
    let programs = std::env::var("ProgramW6432")
        .or_else(|_| std::env::var("ProgramFiles"))
        .unwrap_or_else(|_| r"C:\Program Files".into());
    format!("{}\\OpenCircle\\", programs.trim_end_matches('\\'))
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
    // SAFETY: a plain query of this process's token.
    if unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool() {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        return std::process::Command::new(Path::new(&windir).join("explorer.exe"))
            .arg(exe)
            .spawn()
            .map(drop);
    }
    crate::spawn_detached(exe)
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
    [dirs::config_dir(), dirs::data_local_dir()]
        .into_iter()
        .flatten()
        .map(|d| d.join("opencircle"))
        .collect()
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
    for dir in user_data() {
        if dir.exists()
            && let Err(e) = std::fs::remove_dir_all(&dir)
        {
            crate::log!("[opencircle] remove {}: {e}", dir.display());
        }
    }
    use crate::config::Provider;
    for provider in [Provider::Anthropic, Provider::OpenaiCompatible] {
        let _ = crate::secrets::delete(provider);
    }
    let _ = crate::secrets::by_name::delete(crate::music::SERVICE.key_entry());
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Ok(Some(Silent::Install { dir: None }))
        );
        assert_eq!(
            parse_silent(&args("/QUIET /norestart")),
            Ok(Some(Silent::Install { dir: None }))
        );
        assert_eq!(
            parse_silent(&args(r#"--silent /dir="d:/Apps/OC""#)),
            Ok(Some(Silent::Install {
                dir: Some(r"D:\Apps\OC\".into())
            }))
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

    #[test]
    fn only_the_setup_build_carries_an_msi() {
        assert_eq!(payload().is_some(), cfg!(setup_payload));
    }
}
