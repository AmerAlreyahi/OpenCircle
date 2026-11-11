//! The layout direction (left to right, or right to left for Arabic): the
//! ONE place kit parts read it. A window's view sets it for the time it
//! builds its elements (`scope`); every kit part that lays things side by
//! side, aligns text or pads one side reads it while it is built, so screens
//! write their children in reading order and never branch on the language.
//!
//! What mirrors and what does not: ARABIC_UI_SPEC.md §3. Key caps, numbers,
//! paths, URLs, the logo, colour bars and preview stages are never mirrored
//! (their parts don't ask).
use std::cell::Cell;

use iced::alignment::Horizontal;
use iced::widget::Row;
use iced::{Element, Padding};

thread_local! {
    static RTL: Cell<bool> = const { Cell::new(false) };
}

/// Right to left now (while a view that set it is being built).
pub fn rtl() -> bool {
    RTL.get()
}

/// The direction for as long as the guard lives (a view's whole build): the
/// one before comes back when it drops, so views that don't set it (the
/// overlay, until it speaks Arabic) stay left to right.
#[must_use]
pub struct Scope(bool);

pub fn scope(rtl: bool) -> Scope {
    Scope(RTL.replace(rtl))
}

impl Drop for Scope {
    fn drop(&mut self) {
        RTL.set(self.0);
    }
}

/// `children` (in reading order) side by side: from the right in RTL.
pub fn row<'a, Message: 'a>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Row<'a, Message> {
    Row::with_children(order(children.into_iter().collect()))
}

/// `items` in reading order, laid out left to right: reversed in RTL.
pub fn order<T>(mut items: Vec<T>) -> Vec<T> {
    if rtl() {
        items.reverse();
    }
    items
}

/// Where a line starts: the left, or the right in RTL.
pub fn start() -> Horizontal {
    match rtl() {
        true => Horizontal::Right,
        false => Horizontal::Left,
    }
}

/// Where a line ends.
pub fn end() -> Horizontal {
    match rtl() {
        true => Horizontal::Left,
        false => Horizontal::Right,
    }
}

/// `p` with its left and right swapped in RTL (a pad written for the left
/// to right layout).
pub fn pad(p: Padding) -> Padding {
    match rtl() {
        true => Padding {
            left: p.right,
            right: p.left,
            ..p
        },
        false => p,
    }
}

/// A text role's size: Arabic is drawn 1 px larger (ARABIC_UI_SPEC.md §4).
pub fn size(px: f32) -> f32 {
    px + if rtl() { 1.0 } else { 0.0 }
}

/// A wrapped line's height (times its size): more room in Arabic for the
/// dots and marks above and below (1.6 -> 1.75, 1.5 -> 1.7).
pub fn line(height: f32) -> f32 {
    match rtl() {
        true => height + if height >= 1.6 { 0.15 } else { 0.2 },
        false => height,
    }
}

/// `row!` in reading order: `dir_row![a, b]` puts `a` at the start side.
#[macro_export]
macro_rules! dir_row {
    ($($x:expr),* $(,)?) => {
        $crate::ui::kit::dir::row([$(iced::Element::from($x)),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_sets_and_restores_the_direction() {
        assert!(!rtl());
        {
            let _ar = scope(true);
            assert!(rtl());
            assert_eq!(order(vec![1, 2, 3]), [3, 2, 1]);
            assert_eq!(pad(Padding::ZERO.left(8.0)).right, 8.0);
            assert_eq!(start(), Horizontal::Right);
            {
                let _en = scope(false);
                assert!(!rtl());
            }
            assert!(rtl());
        }
        assert!(!rtl());
        assert_eq!(order(vec![1, 2]), [1, 2]);
    }
}
