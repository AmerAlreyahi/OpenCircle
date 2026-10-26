//! The one focus rule for text fields (#101): a focused field (the bar's AI
//! field, a card's input or editor) always takes typed letters, and the
//! overlay's single-letter shortcuts only run when none has focus (iced's
//! fields capture a key before the overlay sees it). The two keys a field
//! would otherwise swallow are given back here: Esc leaves the field (a second
//! Esc closes), and Ctrl+C with nothing selected in it is the overlay's copy.
use iced::advanced::clipboard::{self, Clipboard};
use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, operation};
use iced::advanced::{Shell, Widget};
use iced::keyboard::{self, key::Named};
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse};

/// What a focused field gave back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyed {
    /// Esc: the field lost focus (the overlay stays open).
    Left,
    /// Ctrl+C with nothing selected in the field: copy the capture.
    Copy,
    /// A click gave the field the focus.
    Focus,
}

/// `content` (a text field, or a row holding one) with the rule above.
pub fn guard<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_key: impl Fn(Keyed) -> Message + 'a,
) -> Element<'a, Message> {
    Element::new(Guard {
        content: content.into(),
        on_key: Box::new(on_key),
    })
}

struct Guard<'a, Message> {
    content: Element<'a, Message>,
    on_key: Box<dyn Fn(Keyed) -> Message + 'a>,
}

/// Is a field inside focused?
struct Focused(bool);

impl Operation for Focused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(
        &mut self,
        _: Option<&iced::advanced::widget::Id>,
        _: Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        self.0 |= state.is_focused();
    }
}

/// The clipboard, noting whether the field wrote to it (it copied a selection).
struct Watch<'a> {
    inner: &'a mut dyn Clipboard,
    wrote: bool,
}

impl Clipboard for Watch<'_> {
    fn read(&self, kind: clipboard::Kind) -> Option<String> {
        self.inner.read(kind)
    }
    fn write(&mut self, kind: clipboard::Kind, contents: String) {
        self.wrote = true;
        self.inner.write(kind, contents);
    }
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Guard<'_, Message> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> iced::advanced::widget::tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
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
        renderer: &iced::Renderer,
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
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let key = match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) => match (key, key.to_latin(*physical_key)) {
                (keyboard::Key::Named(Named::Escape), _) => Some(Keyed::Left),
                (_, Some('c')) if modifiers.command() => Some(Keyed::Copy),
                _ => None,
            },
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => Some(Keyed::Focus),
            _ => None,
        };
        let mut focused = Focused(false);
        // (before the update for the keys it may take; after it for a click)
        if matches!(key, Some(Keyed::Left | Keyed::Copy)) {
            (self.content.as_widget_mut()).operate(tree, layout, renderer, &mut focused);
        }
        let mut watch = Watch {
            inner: clipboard,
            wrote: false,
        };
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, &mut watch, shell, viewport,
        );
        if key == Some(Keyed::Focus) {
            (self.content.as_widget_mut()).operate(tree, layout, renderer, &mut focused);
        }
        match key {
            // (a text editor lets Esc through: the overlay must not close on it)
            Some(Keyed::Left) if focused.0 => {
                shell.publish((self.on_key)(Keyed::Left));
                shell.capture_event();
            }
            Some(Keyed::Copy) if focused.0 && !watch.wrote => {
                shell.publish((self.on_key)(Keyed::Copy));
            }
            Some(Keyed::Focus) if focused.0 => shell.publish((self.on_key)(Keyed::Focus)),
            _ => {}
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        (self.content.as_widget()).mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        (self.content.as_widget_mut()).overlay(tree, layout, renderer, viewport, translation)
    }
}
