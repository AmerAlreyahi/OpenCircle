//! The Studio's editing model on the keyboard and in the right-click menu
//! (STUDIO_V3_SPEC S1, S7): key -> editor message, and the context menus'
//! items with their enabled state. The editor itself (`annotate`) does the
//! work; this file is only the tables the view and the canvas read.
#![allow(dead_code)] // the menus are drawn by the Studio view (W87)
use iced::keyboard::{Key, Modifiers, key::Named};

use crate::features::snip::annotate::{Annotator, Message, Order, Tool};

/// What a Studio key does in the editor (typing and open menus are handled
/// before this): `None` = not a Studio editing key, the editor's own table
/// goes on. Esc ladder here: selection -> tool back to Select -> nothing.
pub fn key(e: &Annotator, key: &Key, latin: Option<char>, m: Modifiers) -> Option<Message> {
    let ch = latin.or(match key {
        Key::Character(s) => s.chars().next(),
        _ => None,
    });
    let ch = ch.map(|c| c.to_ascii_lowercase());
    let sel = !e.selection().is_empty();
    if let Key::Named(named) = key {
        return match named {
            Named::Escape if e.picking || e.menu.is_some() => None,
            Named::Escape if sel => Some(Message::Deselect),
            Named::Escape if e.tool != Tool::Select => Some(Message::Tool(Tool::Select)),
            Named::Escape => Some(Message::Deselect),
            Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown
                if sel && !m.command() =>
            {
                let arrow = match named {
                    Named::ArrowLeft => (-1, 0),
                    Named::ArrowRight => (1, 0),
                    Named::ArrowUp => (0, -1),
                    _ => (0, 1),
                };
                Some(Message::Shove(arrow, m.shift()))
            }
            _ => None,
        };
    }
    if !m.command() {
        return None;
    }
    match (ch?, m.shift()) {
        ('c', false) => Some(Message::CopySelection),
        ('c', true) => Some(Message::Copy),
        ('x', false) => Some(Message::CutSelection),
        ('d', false) => Some(Message::Duplicate),
        ('a', false) => Some(Message::SelectAll),
        (']' | '}', shift) => Some(Message::Order(if shift { Order::Front } else { Order::Forward })),
        ('[' | '{', shift) => Some(Message::Order(if shift { Order::Back } else { Order::Backward })),
        _ => None,
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub action: Action,
    pub keys: &'static str,
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
        row(Action::Cut, "Ctrl+X", !sel.is_empty(), false),
        row(Action::Copy, "Ctrl+C", !sel.is_empty(), false),
        row(Action::Paste, "Ctrl+V", true, false),
        row(Action::Duplicate, "Ctrl+D", !sel.is_empty(), false),
        row(Action::Delete, "Delete", !sel.is_empty(), false),
        row(Action::Forward, "Ctrl+]", !sel.is_empty() && !top, true),
        row(Action::Backward, "Ctrl+[", !sel.is_empty() && !bottom, false),
        row(Action::Front, "Ctrl+Shift+]", !sel.is_empty() && !top, false),
        row(Action::Back, "Ctrl+Shift+[", !sel.is_empty() && !bottom, false),
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
        row(Action::Paste, "Ctrl+V", true, false),
        row(Action::SelectAll, "Ctrl+A", !e.doc.items.is_empty(), false),
        row(Action::CopyPicture, "Ctrl+Shift+C", true, true),
        row(Action::Fit, "Ctrl+0", true, false),
        row(Action::Shortcuts, "Ctrl+/", true, true),
    ]
}
