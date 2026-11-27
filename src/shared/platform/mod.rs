//! The OS and nothing else: windows (the one opener), their shape, monitors, the tray.
//! Imports nothing of the UI's kit, screens or app (`arch_tests::platform_is_isolated`).

pub(crate) mod alpha_window;
pub(crate) mod audio;
pub(crate) mod autostart;
pub(crate) mod capture;
pub(crate) mod dcomp;
pub(crate) mod hotkey;
pub(crate) mod layer_shell;
pub(crate) mod monitor;
pub(crate) mod shape;
pub(crate) mod sys;
pub(crate) mod tray;
pub(crate) mod windows;

/// Whether this OS can keep our own windows out of a whole-screen capture or recording (Windows:
/// the display affinity). Where it cannot (Linux), the recording's controls live in the tray when
/// the whole screen is recorded (owner, Q1; `snip::record::placement::surface`). The one place
/// that says so: no platform checks in feature files.
pub const CAN_KEEP_WINDOWS_OUT_OF_CAPTURE: bool = cfg!(windows);
