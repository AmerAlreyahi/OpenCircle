//! Edit: the action bar turns into the editing toolbar over the selection
//! (`ui/edit.rs` places it; `crate::editor` is the editor).
use super::{Action, Key, Run};

pub const ACTION: Action = Action {
    id: "edit",
    label: "Edit",
    key: Key {
        ctrl: false,
        letter: 'e',
    },
    icon: '\u{e1f8}', // lucide "pencil"
    run: Run::Edit,
};
