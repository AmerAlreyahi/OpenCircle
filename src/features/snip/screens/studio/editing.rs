//! The Studio's right-click menus (STUDIO_V3_SPEC S1): items with their
//! enabled state. The key -> message table is `Annotator::studio_key` (the
//! backend may not import this UI folder); the editor does all the work.
use crate::features::snip::annotate::{Annotator, Message, Order};

/// A right-click menu row: what it does, its key (for the row's right
/// side) and whether it can be used now. Labels are the view's (string table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    Forward,
    Backward,
    Front,
    Back,
    SelectAll,
    CopyPicture,
    Fit,
    Shortcuts,
}

/// A row's key, drawn as key caps by the view (no words here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    Ctrl(char),
    CtrlShift(char),
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub action: Action,
    pub keys: Chord,
    pub enabled: bool,
    /// A line is drawn above this row.
    pub divider: bool,
}

impl Action {
    /// The editor message for it; `None` = the Studio's host does it (Paste
    /// reads the system clipboard, Fit zooms, Shortcuts opens the sheet).
    pub fn message(self) -> Option<Message> {
        Some(match self {
            Action::Cut => Message::CutSelection,
            Action::Copy => Message::CopySelection,
            Action::Duplicate => Message::Duplicate,
            Action::Delete => Message::Delete,
            Action::Forward => Message::Order(Order::Forward),
            Action::Backward => Message::Order(Order::Backward),
            Action::Front => Message::Order(Order::Front),
            Action::Back => Message::Order(Order::Back),
            Action::SelectAll => Message::SelectAll,
            Action::CopyPicture => Message::Copy,
            Action::Paste | Action::Fit | Action::Shortcuts => return None,
        })
    }
}

/// The menu for a right-click on an object (which selects it first when it
/// was not selected): Cut, Copy, Paste, Duplicate, Delete, then the order.
pub fn object_menu(e: &Annotator) -> Vec<Entry> {
    let sel = e.selection();
    let items = e.doc.items.len();
    let row = |action, keys, enabled, divider| Entry {
        action,
        keys,
        enabled,
        divider,
    };
    // order moves are no-ops at the ends
    let (top, bottom) = (
        sel.last().is_some_and(|&i| i + 1 == items) && sel.len() == 1,
        sel.first() == Some(&0) && sel.len() == 1,
    );
    vec![
        row(Action::Cut, Chord::Ctrl('X'), !sel.is_empty(), false),
        row(Action::Copy, Chord::Ctrl('C'), !sel.is_empty(), false),
        row(Action::Paste, Chord::Ctrl('V'), true, false),
        row(Action::Duplicate, Chord::Ctrl('D'), !sel.is_empty(), false),
        row(Action::Delete, Chord::Delete, !sel.is_empty(), false),
        row(
            Action::Forward,
            Chord::Ctrl(']'),
            !sel.is_empty() && !top,
            true,
        ),
        row(
            Action::Backward,
            Chord::Ctrl('['),
            !sel.is_empty() && !bottom,
            false,
        ),
        row(
            Action::Front,
            Chord::CtrlShift(']'),
            !sel.is_empty() && !top,
            false,
        ),
        row(
            Action::Back,
            Chord::CtrlShift('['),
            !sel.is_empty() && !bottom,
            false,
        ),
    ]
}

/// The menu for a right-click on the empty picture.
pub fn picture_menu(e: &Annotator) -> Vec<Entry> {
    let row = |action, keys, enabled, divider| Entry {
        action,
        keys,
        enabled,
        divider,
    };
    vec![
        row(Action::Paste, Chord::Ctrl('V'), true, false),
        row(
            Action::SelectAll,
            Chord::Ctrl('A'),
            !e.doc.items.is_empty(),
            false,
        ),
        row(Action::CopyPicture, Chord::CtrlShift('C'), true, true),
        row(Action::Fit, Chord::Ctrl('0'), true, false),
        row(Action::Shortcuts, Chord::Ctrl('/'), true, true),
    ]
}
