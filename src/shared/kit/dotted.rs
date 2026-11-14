//! A dotted underline under its content (the design's `border-bottom: 1.5px
//! dotted accent` on a name in the word card): iced's rich text has only a
//! solid underline, so the dots are drawn here, in the `accent`, on the
//! content's bottom edge. Built once; used wherever a word is marked as
//! "can be picked".
use iced::advanced::layout::Layout;
use iced::advanced::widget::Tree;
use iced::advanced::{Renderer as _, mouse, renderer};
use iced::{Border, Element, Rectangle, Size};

use super::themes::c;
use super::wrap::{Hook, wrap};

/// A dot's size, its pitch, and how far above the content's bottom edge the
/// line sits.
const DOT: f32 = 1.5;
const PITCH: f32 = 3.0;
const INSET: f32 = 4.0;

struct Dots;

impl<Message> Hook<Message> for Dots {
    fn draw(
        &self,
        content: &Element<'_, Message>,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        (content.as_widget()).draw(tree, renderer, theme, style, layout, cursor, viewport);
        let b = layout.bounds();
        let y = b.y + b.height - INSET;
        let mut x = b.x;
        while x + DOT <= b.x + b.width {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(iced::Point::new(x, y), Size::new(DOT, DOT)),
                    border: Border::default().rounded(DOT / 2.0),
                    ..renderer::Quad::default()
                },
                c().accent,
            );
            x += PITCH;
        }
    }
}

/// `content` with the dotted line under it.
pub fn dotted<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    wrap(content, Dots)
}
