//! OpenCircle patch: one selection over many [`Rich`] texts (an AI answer's
//! paragraphs, list items, headings and code blocks), as in a browser.
//!
//! The app wraps the texts in a widget whose `update` calls [`enter`] before
//! its content and [`leave`] after it. Inside, a drag that starts in one text
//! selects in every text it passes over (each text picks its own part from
//! the drag's two ends: above it = its start, below it = its end), and Ctrl+C
//! gives one text: every selected part in order, a line between them. Outside
//! a group each text selects alone, as upstream.
//!
//! [`Rich`]: crate::text::Rich
use std::cell::RefCell;

use crate::core::Point;

#[derive(Default)]
struct Group {
    depth: usize,
    /// Where the drag started (the texts' own coordinates), while it runs.
    anchor: Option<Point>,
    /// Ctrl+C: each selected text's part, in order.
    copied: Vec<String>,
}

thread_local! {
    static GROUP: RefCell<Group> = RefCell::default();
}

/// Before the group's content gets an event.
pub fn enter() {
    GROUP.with_borrow_mut(|g| {
        if g.depth == 0 {
            g.copied.clear();
        }
        g.depth += 1;
    });
}

/// After it: the outermost group returns what Ctrl+C copied (one write to the
/// clipboard), and a released button ends the drag (`released`).
pub fn leave(released: bool) -> Option<String> {
    GROUP.with_borrow_mut(|g| {
        g.depth = g.depth.saturating_sub(1);
        if g.depth > 0 {
            return None;
        }
        if released {
            g.anchor = None;
        }
        let parts = std::mem::take(&mut g.copied);
        (!parts.is_empty()).then(|| parts.join("\n"))
    })
}

/// Inside a group: a drag starts at `at`.
pub(crate) fn start(at: Point) -> bool {
    GROUP.with_borrow_mut(|g| {
        if g.depth > 0 {
            g.anchor = Some(at);
        }
        g.depth > 0
    })
}

/// Inside a group, while a drag runs: where it started.
pub(crate) fn anchor() -> Option<Point> {
    GROUP.with_borrow(|g| g.anchor.filter(|_| g.depth > 0))
}

/// Inside a group: `text` joins what Ctrl+C copies (back: no group).
pub(crate) fn copy(text: String) -> Result<(), String> {
    GROUP.with_borrow_mut(|g| match g.depth > 0 {
        true => {
            g.copied.push(text);
            Ok(())
        }
        false => Err(text),
    })
}
