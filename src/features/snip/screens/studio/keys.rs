//! The Studio's key names, once (STUDIO_V3_SPEC §7): the menus, the shortcuts
//! sheet, the status bar's tooltips and the key table's own test all read these,
//! so they can't disagree. (Key names are written in Latin in every language.)

pub const NEW_SNIP: &str = "Alt+A";
pub const OPEN: &str = "Ctrl+O";
pub const SAVE: &str = "Ctrl+S";
pub const SAVE_AS: &str = "Ctrl+Shift+S";
pub const COPY_PICTURE: &str = "Ctrl+Shift+C";
pub const PREFERENCES: &str = "Ctrl+,";
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
pub const STEPS_SIDEBAR: &str = "Ctrl+B";
/// The picture's zoom: the status bar's buttons and the View menu (the wheel
/// zooms at the pointer). Ctrl+= / Ctrl+- / Ctrl+0 scale the Studio's UI.
pub const ZOOM_IN: &str = "Ctrl+Shift+=";
pub const ZOOM_OUT: &str = "Ctrl+Shift+-";
pub const FIT: &str = "Shift+1";
pub const ACTUAL: &str = "Ctrl+1";
pub const UI_LARGER: &str = "Ctrl+=";
pub const UI_SMALLER: &str = "Ctrl+-";
pub const UI_RESET: &str = "Ctrl+0";
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
        C::Shift(c) => vec!["Shift".into(), c.to_string()],
        C::Delete => vec!["Delete".into()],
    }
}

// ---- the Studio's own commands: one table, editable in Studio preferences ----

use iced::keyboard::{Key, Modifiers, key::Code, key::Physical};

/// A command with a key the user can change (Studio preferences > Keyboard shortcuts).
/// The menus, tooltips, the shortcuts sheet and the key handler all read `of`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Open,
    SaveAs,
    Preferences,
    Shortcuts,
    StepsSidebar,
    ZoomIn,
    ZoomOut,
    Fit,
    Actual,
    UiLarger,
    UiSmaller,
    UiReset,
}

impl Cmd {
    pub const ALL: [Cmd; 12] = [
        Cmd::Open,
        Cmd::SaveAs,
        Cmd::Preferences,
        Cmd::Shortcuts,
        Cmd::StepsSidebar,
        Cmd::ZoomIn,
        Cmd::ZoomOut,
        Cmd::Fit,
        Cmd::Actual,
        Cmd::UiLarger,
        Cmd::UiSmaller,
        Cmd::UiReset,
    ];

    /// The key in `[studio.shortcuts]`.
    pub fn id(self) -> &'static str {
        match self {
            Cmd::Open => "open",
            Cmd::SaveAs => "save_as",
            Cmd::Preferences => "preferences",
            Cmd::Shortcuts => "shortcuts",
            Cmd::StepsSidebar => "steps_sidebar",
            Cmd::ZoomIn => "zoom_in",
            Cmd::ZoomOut => "zoom_out",
            Cmd::Fit => "fit",
            Cmd::Actual => "actual",
            Cmd::UiLarger => "ui_larger",
            Cmd::UiSmaller => "ui_smaller",
            Cmd::UiReset => "ui_reset",
        }
    }

    /// Its key before the user changes it.
    pub fn default_chord(self) -> &'static str {
        match self {
            Cmd::Open => OPEN,
            Cmd::SaveAs => SAVE_AS,
            Cmd::Preferences => PREFERENCES,
            Cmd::Shortcuts => SHORTCUTS,
            Cmd::StepsSidebar => STEPS_SIDEBAR,
            Cmd::ZoomIn => ZOOM_IN,
            Cmd::ZoomOut => ZOOM_OUT,
            Cmd::Fit => FIT,
            Cmd::Actual => ACTUAL,
            Cmd::UiLarger => UI_LARGER,
            Cmd::UiSmaller => UI_SMALLER,
            Cmd::UiReset => UI_RESET,
        }
    }

    /// What the key does.
    pub fn msg(self) -> super::Msg {
        use super::Msg;
        match self {
            Cmd::Open => Msg::Open,
            Cmd::SaveAs => Msg::Export,
            Cmd::Preferences => Msg::OpenPrefs,
            Cmd::Shortcuts => Msg::Shortcuts(true),
            Cmd::StepsSidebar => Msg::Library,
            Cmd::ZoomIn => Msg::ZoomIn,
            Cmd::ZoomOut => Msg::ZoomOut,
            Cmd::Fit => Msg::Fit,
            Cmd::Actual => Msg::Actual,
            Cmd::UiLarger => Msg::UiScale(1),
            Cmd::UiSmaller => Msg::UiScale(-1),
            Cmd::UiReset => Msg::UiScale(0),
        }
    }

    /// The command's name in the string table.
    pub fn name(self) -> crate::shared::app_language::S {
        use crate::shared::app_language::S;
        match self {
            Cmd::Open => S::st_open,
            Cmd::SaveAs => S::st_save_as,
            Cmd::Preferences => S::st_prefs,
            Cmd::Shortcuts => S::st_shortcuts,
            Cmd::StepsSidebar => S::st_steps,
            Cmd::ZoomIn => S::st_zoom_in,
            Cmd::ZoomOut => S::st_zoom_out,
            Cmd::Fit => S::st_fit,
            Cmd::Actual => S::st_actual,
            Cmd::UiLarger => S::st_ui_larger,
            Cmd::UiSmaller => S::st_ui_smaller,
            Cmd::UiReset => S::st_ui_reset,
        }
    }
}

/// Chords a custom key may not take: the editor's own fixed keys and Esc.
pub const FIXED: &[&str] = &[
    UNDO,
    REDO,
    PASTE,
    COPY,
    CUT,
    DUPLICATE,
    DELETE,
    SELECT_ALL,
    DESELECT,
    ADD_STEPS,
    FORWARD,
    BACKWARD,
    FRONT,
    BACK,
    COPY_PICTURE,
    SAVE,
    NEW_SNIP,
    CLOSE,
];

/// A chord that was taken: by another Studio command or by a fixed key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    Command(Cmd),
    Fixed,
}

/// The key of `cmd`: the user's (`[studio.shortcuts]`) or its default. A `&'static str`
/// so tooltips and menus can hold it (the chords the user has ever set are interned).
pub fn of(cmd: Cmd) -> &'static str {
    let config = crate::shared::config::current();
    match config.studio.shortcuts.get(cmd.id()) {
        Some(chord) => intern(chord),
        None => cmd.default_chord(),
    }
}

/// The command a chord triggers, if any (as `of` says).
pub fn command_for(chord: &str) -> Option<Cmd> {
    Cmd::ALL.into_iter().find(|c| of(*c) == chord)
}

/// Why `chord` can't be `cmd`'s key: taken by another command or a fixed key.
pub fn taken(cmd: Cmd, chord: &str) -> Option<Taken> {
    if FIXED.contains(&chord) {
        return Some(Taken::Fixed);
    }
    Cmd::ALL
        .into_iter()
        .find(|c| *c != cmd && of(*c) == chord)
        .map(Taken::Command)
}

/// ponytail: leaked once per distinct chord ever set (a few bytes each); a real intern pool
/// if keys ever change per keystroke.
fn intern(chord: &str) -> &'static str {
    use std::sync::Mutex;
    static POOL: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = pool.iter().find(|c| **c == chord) {
        return found;
    }
    let leaked: &'static str = Box::leak(chord.to_string().into_boxed_str());
    pool.push(leaked);
    leaked
}

/// The chord an event is, as `of` writes them: `Ctrl`, `Alt`, `Shift` and the key by
/// its place (digits and punctuation) or its Latin letter. `None` for a key alone with
/// no modifier that is not a function key, and for keys a chord can't end on.
pub fn chord_text(modifiers: Modifiers, key: &Key, physical: Physical) -> Option<String> {
    let Physical::Code(code) = physical else {
        return None;
    };
    let name: String = match code {
        Code::KeyA
        | Code::KeyB
        | Code::KeyC
        | Code::KeyD
        | Code::KeyE
        | Code::KeyF
        | Code::KeyG
        | Code::KeyH
        | Code::KeyI
        | Code::KeyJ
        | Code::KeyK
        | Code::KeyL
        | Code::KeyM
        | Code::KeyN
        | Code::KeyO
        | Code::KeyP
        | Code::KeyQ
        | Code::KeyR
        | Code::KeyS
        | Code::KeyT
        | Code::KeyU
        | Code::KeyV
        | Code::KeyW
        | Code::KeyX
        | Code::KeyY
        | Code::KeyZ => key.to_latin(physical)?.to_ascii_uppercase().to_string(),
        Code::Digit0 | Code::Numpad0 => "0".into(),
        Code::Digit1 | Code::Numpad1 => "1".into(),
        Code::Digit2 | Code::Numpad2 => "2".into(),
        Code::Digit3 | Code::Numpad3 => "3".into(),
        Code::Digit4 | Code::Numpad4 => "4".into(),
        Code::Digit5 | Code::Numpad5 => "5".into(),
        Code::Digit6 | Code::Numpad6 => "6".into(),
        Code::Digit7 | Code::Numpad7 => "7".into(),
        Code::Digit8 | Code::Numpad8 => "8".into(),
        Code::Digit9 | Code::Numpad9 => "9".into(),
        Code::Comma => ",".into(),
        Code::Period => ".".into(),
        Code::Slash => "/".into(),
        Code::Backslash => "\\".into(),
        Code::Equal | Code::NumpadAdd => "=".into(),
        Code::Minus | Code::NumpadSubtract => "-".into(),
        Code::BracketLeft => "[".into(),
        Code::BracketRight => "]".into(),
        Code::Semicolon => ";".into(),
        Code::Quote => "'".into(),
        Code::Backquote => "`".into(),
        Code::Enter => "Enter".into(),
        Code::Tab => "Tab".into(),
        Code::F1 => "F1".into(),
        Code::F2 => "F2".into(),
        Code::F3 => "F3".into(),
        Code::F4 => "F4".into(),
        Code::F5 => "F5".into(),
        Code::F6 => "F6".into(),
        Code::F7 => "F7".into(),
        Code::F8 => "F8".into(),
        Code::F9 => "F9".into(),
        Code::F10 => "F10".into(),
        Code::F11 => "F11".into(),
        Code::F12 => "F12".into(),
        _ => return None,
    };
    let function_key = name.starts_with('F') && name.len() > 1;
    let mut parts: Vec<&str> = Vec::new();
    if modifiers.command() {
        parts.push("Ctrl");
    }
    if modifiers.alt() {
        parts.push("Alt");
    }
    if modifiers.shift() {
        parts.push("Shift");
    }
    // a bare key types: only a function key may stand alone
    if parts.is_empty() && !function_key {
        return None;
    }
    parts.push(&name);
    Some(parts.join("+"))
}

/// The editor's own keys, shown as a reference (they can't be changed here).
pub const FIXED_ROWS: &[(crate::shared::app_language::S, &[&str])] = {
    use crate::shared::app_language::S;
    &[
        (S::st_sc_file, &[SAVE, COPY_PICTURE]),
        (S::st_sc_copy, &[COPY, CUT, PASTE]),
        (S::st_sc_dup, &[DUPLICATE, DELETE, SELECT_ALL]),
        (S::st_sc_undo, &[UNDO, REDO]),
        (S::st_sc_nudge, &[NUDGE, NUDGE_FAR]),
        (S::st_sc_order, &[FORWARD, BACKWARD, FRONT, BACK]),
        (S::st_sc_steps, &[ADD_STEPS]),
        (S::st_sc_pan, &[PAN]),
    ]
};
