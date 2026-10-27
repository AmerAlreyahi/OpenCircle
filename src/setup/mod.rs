//! OpenCircle's own Windows setup, `OpenCircle-Setup-<ver>-x64.exe`: this same
//! program built with the release MSI inside (build.rs embeds the file named by
//! `OPENCIRCLE_SETUP_MSI`; release.yml builds it after the MSI). Started with no
//! arguments it shows the setup window (`ui::setup`) and installs the MSI through
//! Windows Installer's external UI (`msi`), so upgrade, repair, the uninstall in
//! Apps & features and silent IT installs (`msiexec /i … /qn INSTALLDIR=…`) stay
//! the MSI's own. The MSI's uninstall runs `opencircle.exe --forget-user-data`
//! (`FORGET_ARG`): the question whether the user's data goes too.
//!
//! Why embedded and not beside it: one file to download and run; the MSI stays a
//! separate release asset for IT. Why this program and not a second one: the
//! setup's window reuses the app's renderer, fonts and theme (no new toolkit);
//! the price is the app's size once more in the setup exe.

pub mod msi;

use std::path::{Path, PathBuf};

/// The hidden argument the MSI's uninstall starts the installed exe with.
pub const FORGET_ARG: &str = "--forget-user-data";

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
        eprintln!("[opencircle] start with Windows: {e}");
    }
    if quiet
        || !msi::ask_yes_no(
            "Uninstall OpenCircle",
            "Also remove your OpenCircle settings, downloaded models and saved keys?\n\nChoose No to keep them for a later install.",
        )
    {
        return std::process::ExitCode::SUCCESS;
    }
    for dir in user_data() {
        if dir.exists()
            && let Err(e) = std::fs::remove_dir_all(&dir)
        {
            eprintln!("[opencircle] remove {}: {e}", dir.display());
        }
    }
    use crate::config::Provider;
    for provider in [Provider::Anthropic, Provider::OpenaiCompatible] {
        let _ = crate::secrets::delete(provider);
    }
    let _ = crate::secrets::by_name::delete(crate::music::SERVICE.key_entry());
    std::process::ExitCode::SUCCESS
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

    #[test]
    fn only_the_setup_build_carries_an_msi() {
        assert_eq!(payload().is_some(), cfg!(setup_payload));
    }
}
