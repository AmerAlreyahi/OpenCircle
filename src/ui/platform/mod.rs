//! The OS and nothing else: windows (the one opener), their shape, monitors, the tray.
//! Imports nothing of the UI's kit, screens or app (`arch_tests::platform_is_isolated`).

pub(in crate::ui) mod monitor;
pub(in crate::ui) mod shape;
pub(in crate::ui) mod tray;
pub(in crate::ui) mod windows;
