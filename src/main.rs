//! OpenCircle: resident tray app. Shortcut, tray or `opencircle capture` → frozen overlay.
mod actions;
#[allow(dead_code)] // library API, wired into the UI later
mod ai;
mod capture;
#[allow(dead_code)] // library API, wired into the UI later
mod config;
mod hotkey;
mod lens;
#[allow(dead_code)] // library API, wired into the UI later
mod secrets;
mod ui;

use std::process::ExitCode;
use std::time::Instant;

use iced::futures::channel::mpsc;

/// Something asked the app to act. `Instant` = when the trigger arrived (latency log).
#[derive(Debug, Clone, Copy)]
pub enum Trigger {
    Capture(Instant),
    Quit,
}

/// Where every trigger source (shortcut, tray, CLI socket) sends.
pub type Triggers = mpsc::UnboundedSender<Trigger>;

fn main() -> ExitCode {
    let capture_now = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("capture") => true,
        Some(_) => {
            eprintln!("usage: opencircle [capture]");
            return ExitCode::from(2);
        }
    };

    // A running instance takes the command; this launch is done.
    if hotkey::forward(if capture_now { "capture" } else { "" }).is_ok() {
        if !capture_now {
            eprintln!("OpenCircle is already running");
        }
        return ExitCode::SUCCESS;
    }

    let (tx, rx) = mpsc::unbounded();
    if let Err(e) = hotkey::listen(tx.clone()) {
        eprintln!("cannot open the command socket: {e}");
        return ExitCode::FAILURE;
    }
    let _shortcut = hotkey::start(tx.clone());
    let _tray = ui::start_tray(tx.clone());
    if capture_now {
        let _ = tx.unbounded_send(Trigger::Capture(Instant::now()));
    }

    match ui::run(rx) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
