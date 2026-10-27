//! OpenCircle: resident tray app. Shortcut, tray or `opencircle capture` → frozen overlay.
// Release builds on Windows are GUI-subsystem: no console window on double-click.
// Debug builds keep the console. `attach_parent_console` brings the logs back when
// a release exe is run from a terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod actions;
#[allow(dead_code)] // library API, wired into the UI later
mod ai;
mod autostart;
mod capture;
#[allow(dead_code)] // library API, wired into the UI later
mod config;
mod diagnostics;
#[allow(dead_code)] // the annotation editor; the UI wires it (Edit in place, Snip mode)
mod editor;
mod hotkey;
mod labs;
mod lens;
#[allow(dead_code)] // live-translate layer (platform calls); the UI wires it
mod live;
mod log;
mod mt;
mod music;
#[allow(dead_code, unused_imports)] // library for the Text/Translate actions; not wired in yet
mod ocr;
mod search_history;
#[allow(dead_code)] // library API, wired into the UI later
mod secrets;
mod setup;
#[allow(dead_code)] // library for full-screen translate; the UI wires it
mod translate;
mod ui;
mod update;

use std::process::ExitCode;
use std::time::Instant;

use iced::futures::channel::mpsc;

/// Something asked the app to act. `Instant` = when the trigger arrived (latency log).
#[derive(Debug, Clone)]
pub enum Trigger {
    Capture(Instant),
    /// A quick action (its shortcut or `opencircle run`): action id + target + live flag.
    Quick(Instant, config::QuickAction),
    /// Tray "Settings…": open the Settings window, or focus it if it is open.
    OpenSettings,
    /// From the update thread (`update::Event`).
    Update(update::Event),
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
    spawn_detached(&std::env::current_exe()?)
}

/// Starts `exe` with no arguments, detached (see `start_detached`), and returns at once.
#[cfg(unix)]
pub fn spawn_detached(exe: &std::path::Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut app = Command::new(exe);
    app.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    std::os::unix::process::CommandExt::process_group(&mut app, 0); // Ctrl-C / terminal hang-up skip it
    app.spawn().map(drop) // never waited for: it runs until Quit
}

/// Same on Windows, by `CreateProcessW` without handle inheritance: std's
/// `Command` passes on every inheritable handle (cmd.exe keeps spare copies of
/// its output pipe), so a script capturing this command's output would wait
/// until the app quits.
#[cfg(windows)]
pub fn spawn_detached(exe: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    use windows::core::{PCWSTR, PWSTR};

    let mut cmdline: Vec<u16> = std::iter::once(b'"' as u16)
        .chain(exe.as_os_str().encode_wide())
        .chain([b'"' as u16, 0])
        .collect();
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: `cmdline` is a writable NUL-terminated buffer; `startup` and `info`
    // live through the call; the two handles it returns are closed right away.
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            false,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            None,
            PCWSTR::null(),
            &startup,
            &mut info,
        )?;
        let _ = CloseHandle(info.hThread);
        let _ = CloseHandle(info.hProcess);
    }
    Ok(())
}

fn main() -> ExitCode {
    #[cfg(all(windows, not(debug_assertions)))]
    attach_parent_console();
    ui::limit_malloc_arenas();
    // The setup build (OpenCircle-Setup-*.exe) is the setup, whatever it is asked.
    if let Some(msi) = setup::payload() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match setup::parse_silent(&args) {
            Ok(Some(what)) => std::process::exit(setup::silent(msi, what) as i32),
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
    let relaunched = args.first().map(String::as_str) == Some(update::RELAUNCH_ARG);
    let command = match args.first().map(String::as_str) {
        None => None,
        Some(ocr::WORKER_ARG) => return ocr::worker_main(), // hidden: spawned by the app
        Some(setup::FORGET_ARG) => {
            // hidden: the MSI's uninstall
            return setup::forget_user_data(args.get(1).is_some_and(|a| a == "--quiet"));
        }
        // hidden: our Apps & features entry's uninstall
        Some(setup::UNINSTALL_ARG) => {
            return setup::uninstall_ui(args.get(1).map(String::as_str));
        }
        Some(update::VERSION_ARG) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        // hidden: started by an update, once the old process has exited
        Some(update::RELAUNCH_ARG) if update::wait_for_old(hotkey::running) => None,
        Some(update::RELAUNCH_ARG) => return ExitCode::FAILURE,
        Some("quit") => {
            // Never starts the app: with none running there is nothing to quit.
            let _ = hotkey::forward(&hotkey::Command::Quit.line());
            return ExitCode::SUCCESS;
        }
        Some("capture" | "run") => match hotkey::Command::from_args(&args) {
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
    let line = command.as_ref().map(hotkey::Command::line);
    if let Some(line) = &line {
        return match hotkey::forward_or_start(line, start_detached) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                crate::log!("OpenCircle did not start: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if hotkey::forward(&hotkey::Command::OpenSettings.line()).is_ok() {
        crate::log!("OpenCircle is already running; opened its Settings");
        return ExitCode::SUCCESS;
    }

    log::install_panic_hook(log::dir());
    crate::log!("[opencircle] {} starting", env!("CARGO_PKG_VERSION"));
    // Hidden test hook (the CLI is frozen, so an env var): a thread panics, the crash file appears.
    if std::env::var_os("OPENCIRCLE_TEST_CRASH").is_some() {
        let _ = std::thread::spawn(|| panic!("OPENCIRCLE_TEST_CRASH")).join();
    }
    let (tx, rx) = mpsc::unbounded();
    if let Err(e) = hotkey::listen(tx.clone()) {
        crate::log!("cannot open the command socket: {e}");
        return ExitCode::FAILURE;
    }
    let (config, warnings) = config::Config::load();
    for warning in &warnings {
        crate::log!("[opencircle] config: {warning}");
    }
    hotkey::start(tx.clone(), &config);
    autostart::apply(&config);
    std::thread::spawn(|| remove_old_webview(&app_data_dir()));
    config::set_current(config);
    let _tray = ui::start_tray(tx.clone());
    update::start(tx.clone());

    match ui::run(rx) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            crate::log!("{e}");
            if relaunched {
                update::roll_back(); // a new version that can't start: the old one again
            }
            ExitCode::FAILURE
        }
    }
}

fn app_data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenCircle")
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

#[cfg(test)]
mod tests {
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
