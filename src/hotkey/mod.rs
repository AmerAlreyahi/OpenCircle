//! Triggers from outside the UI: the global shortcut and the CLI socket.
#[cfg(target_os = "linux")]
mod portal;
mod socket;

pub use socket::{forward, listen};

use std::time::Instant;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::{Trigger, Triggers};

/// Starts the capture shortcut: the desktop portal on Wayland (no key grabs there),
/// a key grab elsewhere. Keep the returned manager alive.
pub fn start(tx: Triggers) -> Option<GlobalHotKeyManager> {
    #[cfg(target_os = "linux")]
    if crate::capture::wayland_session() {
        portal::bind(tx);
        return None;
    }
    register(tx)
        .map_err(|e| {
            eprintln!("global shortcut unavailable ({e}); bind `opencircle capture` instead")
        })
        .ok()
}

/// Registers Ctrl+Shift+Space. Keep the returned manager alive.
// ponytail: fixed shortcut; read it from the config once Settings → Shortcuts exists.
fn register(tx: Triggers) -> Result<GlobalHotKeyManager, String> {
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    manager.register(hotkey).map_err(|e| e.to_string())?;
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed {
            let _ = tx.unbounded_send(Trigger::Capture(Instant::now()));
        }
    }));
    Ok(manager)
}
