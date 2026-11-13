//! A widget that wraps another and forwards everything to it: a part that only
//! changes how its content is laid out (`layout`) or what it does with an event
//! (`update`) writes a `Hook` with just those, instead of the dozen forwarding
//! methods of `Widget`. The tree, size, drawing, operations, cursor and overlay
//! are the content's own.
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::{Element, Event, Rectangle, Size, Vector, mouse};

type R = iced::Renderer;
type T = iced::Theme;

/// What a wrapper changes. Both defaults hand the call to the content.
pub trait Hook<Message> {
    fn layout(
        &mut self,
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        content.as_widget_mut().layout(tree, renderer, limits)
    }

    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &R,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }
}

/// `content` with `hook` in front of it.
pub fn wrap<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    hook: impl Hook<Message> + 'a,
) -> Element<'a, Message> {
    Element::new(Wrap {
        content: content.into(),
        hook,
    })
}

struct Wrap<'a, Message, H> {
    content: Element<'a, Message>,
    hook: H,
}

impl<Message, H: Hook<Message>> Widget<Message, T, R> for Wrap<'_, Message, H> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<iced::Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<iced::Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) -> layout::Node {
        self.hook.layout(&mut self.content, tree, renderer, limits)
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
        (self.content.as_widget()).draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        (self.content.as_widget_mut()).operate(tree, layout, renderer, operation);
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
        self.hook.update(
            &mut self.content,
            tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
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
        (self.content.as_widget()).mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &R,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, T, R>> {
        (self.content.as_widget_mut()).overlay(tree, layout, renderer, viewport, translation)
    }
}
