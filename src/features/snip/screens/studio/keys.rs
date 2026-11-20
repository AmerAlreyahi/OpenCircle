//! The Studio's key names, once (STUDIO_V3_SPEC §7): the menus, the shortcuts
//! sheet, the status bar's tooltips and the key table's own test all read these,
//! so they can't disagree. (Key names are written in Latin in every language.)

pub const NEW_SNIP: &str = "Alt+A";
pub const OPEN: &str = "Ctrl+O";
pub const SAVE: &str = "Ctrl+S";
pub const SAVE_AS: &str = "Ctrl+Shift+S";
pub const COPY_PICTURE: &str = "Ctrl+Shift+C";
pub const SETTINGS: &str = "Ctrl+,";
pub const SHORTCUTS: &str = "Ctrl+/";
pub const CLOSE: &str = "Ctrl+W";
pub const UNDO: &str = "Ctrl+Z";
pub const REDO: &str = "Ctrl+Y";
pub const PASTE: &str = "Ctrl+V";
pub const COPY: &str = "Ctrl+C";
pub const CUT: &str = "Ctrl+X";
pub const DUPLICATE: &str = "Ctrl+D";
pub const DELETE: &str = "Delete";
pub const SELECT_ALL: &str = "Ctrl+A";
pub const DESELECT: &str = "Esc";
pub const ADD_STEPS: &str = "Ctrl+Enter";
pub const LEFT_PANEL: &str = "Ctrl+B";
pub const RIGHT_PANEL: &str = "Ctrl+Alt+B";
pub const ALL_PANELS: &str = "Ctrl+\\";
pub const ZOOM_IN: &str = "Ctrl+=";
pub const ZOOM_OUT: &str = "Ctrl+-";
pub const FIT: &str = "Ctrl+0";
pub const ACTUAL: &str = "Ctrl+1";
pub const NUDGE: &str = "←↑→↓";
pub const NUDGE_FAR: &str = "Shift+←↑→↓";
pub const FORWARD: &str = "Ctrl+]";
pub const BACKWARD: &str = "Ctrl+[";
pub const FRONT: &str = "Ctrl+Shift+]";
pub const BACK: &str = "Ctrl+Shift+[";
pub const PAN: &str = "Space";

/// A right-click row's key as key names (`Ctrl`, `Shift`, the key).
pub fn chord_caps(k: super::editing::Chord) -> Vec<String> {
    use super::editing::Chord as C;
    match k {
        C::Ctrl(c) => vec!["Ctrl".into(), c.to_string()],
        C::CtrlShift(c) => vec!["Ctrl".into(), "Shift".into(), c.to_string()],
        C::Delete => vec!["Delete".into()],
    }
}
