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
        Element::new(row)
    }
}

mod mirrored {
    use iced::advanced::layout::{self, Layout};
    use iced::advanced::widget::{Operation, Tree, tree};
    use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
    use iced::{Event, Point, Rectangle, Size, Vector, mouse};

    type R = iced::Renderer;
    type T = iced::Theme;

    impl<'a, Message> Widget<Message, T, R> for super::DirRow<'a, Message> {
        fn size(&self) -> Size<iced::Length> {
            Widget::size(&self.row)
        }
        fn size_hint(&self) -> Size<iced::Length> {
            self.row.size_hint()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &R,
            limits: &layout::Limits,
        ) -> layout::Node {
            let node = self.row.layout(tree, renderer, limits);
            if !self.rtl {
                return node;
            }
            // mirrored within the padded room: the first child ends at its right
            let edges = self.padding.left + node.size().width - self.padding.right;
            let children = (node.children().iter())
                .map(|c| {
                    let b = c.bounds();
                    c.clone().move_to(Point::new(edges - b.x - b.width, b.y))
                })
                .collect();
            layout::Node::with_children(node.size(), children).move_to(node.bounds().position())
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut R,
            theme: &T,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.row
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn tag(&self) -> tree::Tag {
            self.row.tag()
        }
        fn state(&self) -> tree::State {
            self.row.state()
        }
        fn children(&self) -> Vec<Tree> {
            self.row.children()
        }
        fn diff(&self, tree: &mut Tree) {
            self.row.diff(tree);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &R,
            operation: &mut dyn Operation,
        ) {
            self.row.operate(tree, layout, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &R,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.row.update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &R,
        ) -> mouse::Interaction {
            self.row
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &R,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, T, R>> {
            self.row
                .overlay(tree, layout, renderer, viewport, translation)
        }
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
