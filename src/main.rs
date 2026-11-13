//! OpenCircle: resident tray app. Shortcut, tray or `opencircle capture` → frozen overlay.
// Release builds on Windows are GUI-subsystem: no console window on double-click.
// Debug builds keep the console. `attach_parent_console` brings the logs back when
// a release exe is run from a terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod actions;
mod ai;
mod features;
mod live;
mod mt;
mod ocr;
mod shared;
mod snip;
mod translate;
mod ui;

use std::process::ExitCode;
use std::time::Instant;

use iced::futures::channel::mpsc;

/// Something asked the app to act. `Instant` = when the trigger arrived (latency log).
#[derive(Debug, Clone)]
pub enum Trigger {
    Capture(Instant),
    /// A quick action (its shortcut or `opencircle run`): action id + target + live flag.
    Quick(Instant, crate::shared::config::QuickAction),
    /// Tray "Settings…": open the Settings window, or focus it if it is open.
    OpenSettings,
    /// Linux: no tray icon can show (no host, library or GTK): open a window instead (first run: the onboarding).
    NoTray,
    /// Windows, a right-click on the tray icon: open our tray menu at the icon
    /// (`[left, top, right, bottom]`, physical desktop px).
    TrayMenu([i32; 4]),
    /// From the update thread (`update::Event`).
    Update(crate::shared::update::Event),
    /// Restart now (the update pill, the tray menu's first row, Settings):
    /// install the waiting update.
    RestartToUpdate,
    /// Wayland: the running long capture ends so (its pop-up or the tray's
    /// capturing menu).
    LongCapture(crate::shared::platform::capture::scroll::Finish),
    Quit,
}

/// Where every trigger source (shortcut, tray, CLI socket) sends.
pub type Triggers = mpsc::UnboundedSender<Trigger>;

const USAGE: &str = "usage: opencircle [capture | quit | run <action> [--target full_screen|circle|last_region|window]]";

/// A GUI-subsystem exe has no stdout/stderr. When started from a terminal, attach
/// to its console so `eprintln!` shows; a redirect (file, pipe) is already valid
/// and is left alone. Limit: cmd.exe does not wait for a GUI exe, so the prompt
/// may come back before the output (use `start /wait` or `| more`).
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    use std::os::windows::io::IntoRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        SetStdHandle,
    };
    // SAFETY: plain Win32 calls; the opened console handle is leaked on purpose.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            return; // double-click or service: no console to join
        }
        for std_handle in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if GetStdHandle(std_handle).is_ok_and(|h| !h.0.is_null()) {
                continue;
            }
            if let Ok(console) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                let _ = SetStdHandle(std_handle, HANDLE(console.into_raw_handle()));
            }
        }
    }
}

/// Starts this exe with no arguments as the resident app, detached from the
/// terminal (own process group, no console, no stdio), and returns at once.
fn start_detached() -> std::io::Result<()> {
    crate::shared::platform::sys::spawn::spawn_detached(&std::env::current_exe()?)
}

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    crate::shared::platform::sys::init_x11_threads();
    #[cfg(all(windows, not(debug_assertions)))]
    attach_parent_console();
    crate::shared::platform::sys::mem::limit_malloc_arenas();
    crate::shared::platform::sys::fix_appimage_env();
    // The setup build (OpenCircle-Setup-*.exe) is the setup, whatever it is asked.
    if let Some(msi) = crate::shared::setup::payload() {
        // Elevated: DLLs it loads later come from System32 only, never from its folder.
        #[cfg(windows)]
        // SAFETY: a process-wide setting with no pointers.
        let _ = unsafe {
            windows::Win32::System::LibraryLoader::SetDefaultDllDirectories(
                windows::Win32::System::LibraryLoader::LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        let args: Vec<String> = std::env::args().skip(1).collect();
        if crate::shared::setup::elevated()
            || args
                .iter()
                .any(|a| a.starts_with(crate::shared::setup::SHA256_ARG))
        {
            // run elevated (the app's update, winget, IT): no file of the user's is
            // written as admin (a planted link in the log folder); the app logs the
            // outcome of its update in the user's own log
            crate::shared::log::memory_only();
        }
        if let Some(code) = crate::shared::setup::relocate(&args) {
            std::process::exit(code as i32);
        }
        match crate::shared::setup::parse_silent(&args) {
            Ok(Some(what)) => std::process::exit(crate::shared::setup::silent(msi, what) as i32),
            Err(code) => std::process::exit(code as i32),
            Ok(None) => {}
        }
        return match ui::setup::run(msi) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                crate::log!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let relaunched = args.first().map(String::as_str) == Some(crate::shared::update::RELAUNCH_ARG);
    let command = match args.first().map(String::as_str) {
        None => None,
        Some(ocr::WORKER_ARG) => return ocr::worker_main(), // hidden: spawned by the app
        // hidden: Wayland's long capture pop-up, spawned by the app on XWayland
        Some(ui::long_capture_popup::ARG) => return ui::long_capture_popup::main(),
        Some(crate::shared::setup::FORGET_ARG) => {
            // hidden: the MSI's uninstall
            return crate::shared::setup::forget_user_data(
                args.get(1).is_some_and(|a| a == "--quiet"),
            );
        }
        // hidden: our Apps & features entry's uninstall
        Some(crate::shared::setup::UNINSTALL_ARG) => {
            return match crate::shared::setup::uninstall_ui(args.get(1).map(String::as_str)) {
                crate::shared::setup::Uninstall::Exit(code) => code,
                crate::shared::setup::Uninstall::Window { code, folder, dir } => {
                    if let Err(e) = ui::setup::run_uninstall(code, folder) {
                        crate::log!("[opencircle] uninstall: {e}");
                    }
                    crate::shared::setup::delete_when_gone(&dir);
                    ExitCode::SUCCESS
                }
            };
        }
        Some(crate::shared::update::VERSION_ARG) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        // hidden: started by an update, once the old process has exited
        Some(crate::shared::update::RELAUNCH_ARG)
            if crate::shared::update::wait_for_old(crate::shared::platform::hotkey::running) =>
        {
            None
        }
        Some(crate::shared::update::RELAUNCH_ARG) => return ExitCode::FAILURE,
        Some("quit") => {
            // Never starts the app: with none running there is nothing to quit.
            let _ = crate::shared::platform::hotkey::forward(
                &crate::shared::platform::hotkey::Command::Quit.line(),
            );
            return ExitCode::SUCCESS;
        }
        Some("capture" | "run") => match crate::shared::platform::hotkey::Command::from_args(&args)
        {
            Ok(command) => Some(command),
            Err(e) => {
                crate::log!("{e}\n{USAGE}");
                return ExitCode::from(2);
            }
        },
        Some(_) => {
            crate::log!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    // A running instance takes the command; this launch is done. A command with
    // none running starts the app in the background first, so the terminal is
    // not taken over (a bare launch, e.g. a double-click, is the app itself).
    let line = command
        .as_ref()
        .map(crate::shared::platform::hotkey::Command::line);
    if let Some(line) = &line {
        return match crate::shared::platform::hotkey::forward_or_start(line, start_detached) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                crate::log!("OpenCircle did not start: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let ours = env!("CARGO_PKG_VERSION");
    match crate::shared::platform::hotkey::running_version() {
        // An older copy ignores what we forward: ask it to quit and start in its place.
        Ok(theirs) if crate::shared::platform::hotkey::is_older(theirs.as_deref(), ours) => {
            let was = theirs.as_deref().unwrap_or("0.1.x");
            if !take_over() {
                crate::log!("OpenCircle v{was} is running and did not quit");
                return ExitCode::FAILURE;
            }
            crate::log!("[opencircle] took over from v{was}");
        }
        // Started by an update, or right after one (the setup's own start, Windows
        // restarting what it closed): the running copy is the new one; open nothing.
        Ok(_) if relaunched || crate::shared::update::just_updated() => {
            crate::log!("OpenCircle is already running (just updated); nothing opened");
            return ExitCode::SUCCESS;
        }
        Ok(_)
            if crate::shared::platform::hotkey::forward(
                &crate::shared::platform::hotkey::Command::OpenSettings.line(),
            )
            .is_ok() =>
        {
            crate::log!("OpenCircle is already running; opened its Settings");
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    crate::shared::log::install_panic_hook(crate::shared::log::dir());
    crate::log!("[opencircle] {} starting", env!("CARGO_PKG_VERSION"));
    // Hidden test hook (the CLI is frozen, so an env var): a thread panics, the crash file appears.
    if crate::shared::platform::sys::test_hook("OPENCIRCLE_TEST_CRASH").is_some() {
        let _ = std::thread::spawn(|| panic!("OPENCIRCLE_TEST_CRASH")).join();
    }
    let (tx, rx) = mpsc::unbounded();
    if let Err(e) = crate::shared::platform::hotkey::listen(tx.clone()) {
        crate::log!("cannot open the command socket: {e}");
        return ExitCode::FAILURE;
    }
    let (config, warnings) = crate::shared::config::Config::load();
    for warning in &warnings {
        crate::log!("[opencircle] config: {warning}");
    }
    crate::shared::platform::hotkey::start(tx.clone(), &config);
    crate::shared::platform::autostart::apply(&config);
    #[cfg(target_os = "linux")]
    crate::shared::platform::capture::portal::repair_user_launcher();
    // before any helper can have a model file open: old-named model folders get the OptimaMT name
    mt::adopt_old_folders();
    std::thread::spawn(|| remove_old_webview(&crate::shared::paths::app_data()));
    std::thread::spawn(|| remove_search_history(crate::shared::paths::search_history().as_deref()));
    // once: the key all OpenAI-compatible providers shared goes to the one chosen
    let ai = config.ai.clone();
    std::thread::spawn(move || {
        if let Err(e) =
            crate::shared::secrets::migrate_shared(&crate::shared::secrets::Keyring, &ai)
        {
            crate::log!("[opencircle] keyring: {e}");
        }
    });
    crate::shared::config::set_current(config);
    let _tray = ui::start_tray(tx.clone());
    crate::shared::update::start(tx.clone());

    let ran = ui::run(rx);
    crate::shared::platform::hotkey::stop();
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            crate::log!("{e}");
            if relaunched || crate::shared::update::pending() {
                crate::shared::update::roll_back(); // a new version that can't start: the old one again
            }
            ExitCode::FAILURE
        }
    }
}

/// Asks the running copy to quit and waits (5 s) until it lets go of the socket.
fn take_over() -> bool {
    let _ = crate::shared::platform::hotkey::forward(
        &crate::shared::platform::hotkey::Command::Quit.line(),
    );
    for _ in 0..50 {
        if !crate::shared::platform::hotkey::running() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// v0.1.0's web view left its profile (cache, cookies) in `<data>/webview`; the
/// web view is gone (#50, #130). Removes only that folder (a symlink is unlinked,
/// never followed); a failure is logged and retried next start.
fn remove_old_webview(data: &std::path::Path) {
    match std::fs::remove_dir_all(data.join("webview")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            crate::log!("[opencircle] cannot remove the old web view data: {e}");
        }
        _ => {}
    }
}

/// The Labs search history is gone (owner, 2026-11-08): its file, data nobody can
/// see, is deleted at start. Logged once (the file is then gone); a failure is
/// logged and retried next start.
fn remove_search_history(file: Option<&std::path::Path>) {
    let Some(file) = file else { return };
    match std::fs::remove_file(file) {
        Ok(()) => crate::log!("[opencircle] removed the old search history file"),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            crate::log!("[opencircle] cannot remove the old search history: {e}");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_old_search_history_file_is_removed_and_nothing_else() {
        let d = std::env::temp_dir().join(format!("oc-sh-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (old, other) = (d.join("search_history.json"), d.join("song_history.json"));
        std::fs::write(&old, "[]").unwrap();
        std::fs::write(&other, "[]").unwrap();
        super::remove_search_history(Some(&old));
        assert!(!old.exists() && other.exists());
        super::remove_search_history(Some(&old)); // gone: no error
        super::remove_search_history(None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn old_webview_data_is_removed_and_nothing_else() {
        let d = std::env::temp_dir().join(format!("oc-wv-{}", std::process::id()));
        for sub in ["webview/EBWebView", "ocr", "mt"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        super::remove_old_webview(&d);
        assert!(!d.join("webview").exists());
        assert!(d.join("ocr").is_dir() && d.join("mt").is_dir());
        super::remove_old_webview(&d); // no folder: no error
        std::fs::remove_dir_all(&d).unwrap();
    }
}
