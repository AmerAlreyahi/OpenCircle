//! OpenCircle: resident tray app. Shortcut, tray or `opencircle capture` → frozen overlay.
// Release builds on Windows are GUI-subsystem: no console window on double-click.
// Debug builds keep the console. `attach_parent_console` brings the logs back when
// a release exe is run from a terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod actions;
#[allow(dead_code)] // library API, wired into the UI later
mod ai;
mod capture;
#[allow(dead_code)] // library API, wired into the UI later
mod config;
#[allow(dead_code)] // the annotation editor; the UI wires it (Edit in place, Snip mode)
mod editor;
mod hotkey;
mod lens;
#[allow(dead_code)] // live-translate layer (platform calls); the UI wires it
mod live;
mod mt;
#[allow(dead_code, unused_imports)] // library for the Text/Translate actions; not wired in yet
mod ocr;
#[allow(dead_code)] // library API, wired into the UI later
mod secrets;
#[allow(dead_code)] // library for full-screen translate; the UI wires it
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
    Quick(Instant, config::QuickAction),
    /// Tray "Settings…": open the Settings window, or focus it if it is open.
    OpenSettings,
    Quit,
}

/// Where every trigger source (shortcut, tray, CLI socket) sends.
pub type Triggers = mpsc::UnboundedSender<Trigger>;

const USAGE: &str =
    "usage: opencircle [capture | run <action> [--target full_screen|circle|last_region|window]]";

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

fn main() -> ExitCode {
    #[cfg(all(windows, not(debug_assertions)))]
    attach_parent_console();
    ui::limit_malloc_arenas();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match args.first().map(String::as_str) {
        None => None,
        Some(ocr::WORKER_ARG) => return ocr::worker_main(), // hidden: spawned by the app
        Some("capture" | "run") => match hotkey::Command::from_args(&args) {
            Ok(command) => Some(command),
            Err(e) => {
                eprintln!("{e}\n{USAGE}");
                return ExitCode::from(2);
            }
        },
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    // A running instance takes the command; this launch is done.
    let line = command.as_ref().map(hotkey::Command::line);
    if hotkey::forward(line.as_deref().unwrap_or("")).is_ok() {
        if command.is_none() {
            eprintln!("OpenCircle is already running");
        }
        return ExitCode::SUCCESS;
    }

    let (tx, rx) = mpsc::unbounded();
    if let Err(e) = hotkey::listen(tx.clone()) {
        eprintln!("cannot open the command socket: {e}");
        return ExitCode::FAILURE;
    }
    let (config, warnings) = config::Config::load();
    for warning in &warnings {
        eprintln!("[opencircle] config: {warning}");
    }
    hotkey::start(tx.clone(), &config);
    config::set_current(config);
    let _tray = ui::start_tray(tx.clone());
    if let Some(command) = &command {
        let _ = tx.unbounded_send(hotkey::trigger(command, Instant::now()));
    }

    match ui::run(rx) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
