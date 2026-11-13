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
use iced::{Element, Padding, Point};

use super::wrap::{Hook, wrap};

thread_local! {
    /// A view's own direction (`scope`); none = the app language's.
    static RTL: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Right to left now: what a view set for its build (`scope`), else the app
/// language's direction (so a bar or a card measured outside any view agrees
/// with the one drawn).
pub fn rtl() -> bool {
    RTL.get().unwrap_or_else(crate::shared::app_language::rtl)
}

/// The direction for as long as the guard lives (a view's whole build): the
/// one before comes back when it drops.
#[must_use]
pub struct Scope(Option<bool>);

pub fn scope(rtl: bool) -> Scope {
    Scope(RTL.replace(Some(rtl)))
}

impl Drop for Scope {
    fn drop(&mut self) {
        RTL.set(self.0);
    }
}

/// `children` (in reading order) side by side: from the right in RTL.
pub fn row<'a, Message: 'a>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> DirRow<'a, Message> {
    DirRow {
        row: Row::with_children(children),
        rtl: rtl(),
        padding: Padding::ZERO,
    }
}

/// A row in reading order. Right to left it is laid out as left to right
/// (the first child gets its room first, as in English) and then mirrored
/// inside its padding: a row reversed instead gave its LAST child the room
/// first, and a long Arabic label squeezed the icon beside it to nothing.
pub struct DirRow<'a, Message> {
    row: Row<'a, Message>,
    rtl: bool,
    /// As given (its left is the left on screen).
    padding: Padding,
}

impl<'a, Message: 'a> DirRow<'a, Message> {
    pub fn spacing(mut self, amount: impl Into<iced::Pixels>) -> Self {
        self.row = self.row.spacing(amount);
        self
    }
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self.row = self.row.padding(self.padding);
        self
    }
    pub fn width(mut self, width: impl Into<iced::Length>) -> Self {
        self.row = self.row.width(width);
        self
    }
    pub fn height(mut self, height: impl Into<iced::Length>) -> Self {
        self.row = self.row.height(height);
        self
    }
    pub fn align_y(mut self, align: impl Into<iced::alignment::Vertical>) -> Self {
        self.row = self.row.align_y(align);
        self
    }
    /// `child` after the others in reading order (at the left in RTL).
    pub fn push(mut self, child: impl Into<Element<'a, Message>>) -> Self {
        self.row = self.row.push(child);
        self
    }
}

impl<'a, Message: 'a> From<DirRow<'a, Message>> for Element<'a, Message> {
    fn from(row: DirRow<'a, Message>) -> Self {
        let mirror = Mirror {
            rtl: row.rtl,
            padding: row.padding,
        };
        wrap(row.row, mirror)
    }
}

/// Right to left: the laid-out row mirrored inside its padded room (the first
/// child ends at the right).
struct Mirror {
    rtl: bool,
    padding: Padding,
}

impl<Message> Hook<Message> for Mirror {
    fn layout(
        &mut self,
        content: &mut Element<'_, Message>,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let node = content.as_widget_mut().layout(tree, renderer, limits);
        if !self.rtl {
            return node;
        }
        let edges = self.padding.left + node.size().width - self.padding.right;
        let children = (node.children().iter())
            .map(|c| {
                let b = c.bounds();
                c.clone().move_to(Point::new(edges - b.x - b.width, b.y))
            })
            .collect();
        iced::advanced::layout::Node::with_children(node.size(), children)
            .move_to(node.bounds().position())
    }
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

/// A letter of a right-to-left script (Hebrew, Arabic and their presentation forms).
pub fn rtl_letter(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// `text` reads right to left: its first letter is of a right-to-left script
/// (the first-strong rule: a message or a paragraph has the direction of its
/// own content, whatever the UI's direction is).
pub fn first_strong_rtl(text: &str) -> bool {
    text.chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(rtl_letter)
}

/// Where `text`'s lines start: by its own content (`first_strong_rtl`).
pub fn content_start(text: &str) -> Horizontal {
    match first_strong_rtl(text) {
        true => Horizontal::Right,
        false => Horizontal::Left,
    }
}

/// `row!` in reading order: `dir_row![a, b]` puts `a` at the start side.
#[macro_export]
macro_rules! dir_row {
    ($($x:expr),* $(,)?) => {
        $crate::shared::kit::dir::row([$(iced::Element::from($x)),*])
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

    /// A paragraph takes the direction of its first letter, not of the UI.
    #[test]
    fn a_text_has_the_direction_of_its_first_strong_letter() {
        assert!(first_strong_rtl("مرحبا hello"));
        assert!(
            first_strong_rtl("  123 «مرحبا»"),
            "digits and signs are neutral"
        );
        assert!(!first_strong_rtl("hello مرحبا"));
        assert!(!first_strong_rtl("1234"), "no letter: the UI decides");
        assert_eq!(content_start("שלום"), Horizontal::Right);
        assert_eq!(content_start("Hello"), Horizontal::Left);
    }
}
