//! Snip (frozen id "edit"): the action bar turns into the Snip toolbar over the selection
//! (`ui/app/snip_toolbar.rs` opens it; `crate::snip::annotate` is the editor).
use super::{Action, Key, Run};
use crate::shared::app_language::S;

pub const ACTION: Action = Action {
    id: "edit",
    label: S::nav_snip,
    key: Key {
        ctrl: false,
        letter: 'e',
    },
    icon: '\u{e151}', // lucide "scissors"
    run: Run::Snip,
};
