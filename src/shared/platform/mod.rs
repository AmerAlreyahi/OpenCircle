//! The OS and nothing else: windows (the one opener), their shape, monitors, the tray.
//! Imports nothing of the UI's kit, screens or app (`arch_tests::platform_is_isolated`).

pub(crate) mod autostart;
pub(crate) mod capture;
pub(crate) mod hotkey;
pub(crate) mod monitor;
pub(crate) mod shape;
pub(crate) mod sys;
pub(crate) mod tray;
pub(crate) mod windows;
