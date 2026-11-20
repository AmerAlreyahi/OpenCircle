//! A popover: a small card anchored to a corner of `base`, over it; a press
//! anywhere outside the card closes it (the press is taken: nothing under it
//! acts). Esc is the host's (it sends the same `close`). For glance-only
//! things (picture info, Position, Actions); big temporary tasks use
//! `slide_over`.
use iced::advanced::layout::Layout;
use iced::advanced::widget::Tree;
use iced::widget::{container, mouse_area, opaque, pin, stack};
use iced::{Element, Fill, Padding, Point, Rectangle, Size, mouse};
use std::cell::Cell;
use std::rc::Rc;

use super::dir;
use super::surface::{Material, surface};
use super::tokens::{radius, space};

/// Which corner of the base the card hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// Under the end edge (right; left in right-to-left).
    TopEnd,
    /// Over the start edge, at the bottom.
    BottomStart,
    /// Under the start edge (left; right in right-to-left).
    TopStart,
}

/// `base` with `content` in a card at `corner`, `gap` px in from the edges
/// on the side it hangs from (`top`: from the base's top).
pub fn over<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    corner: Corner,
    offset: Padding,
    close: Message,
) -> Element<'a, Message> {
    let card = opaque(
        surface(
            Material::Widget,
            radius::MENU,
            container(content).padding(space::GAP + 4.0),
        )
        .padding(0),
    );
    let placed = container(card)
        .padding(offset)
        .width(Fill)
        .height(Fill)
        .align_x(match corner {
            Corner::TopEnd => dir::end(),
            Corner::BottomStart | Corner::TopStart => dir::start(),
        })
        .align_y(match corner {
            Corner::TopEnd | Corner::TopStart => iced::alignment::Vertical::Top,
            Corner::BottomStart => iced::alignment::Vertical::Bottom,
        });
    let catch = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill),
    )
    .on_press(close.clone()) // mouse only: a press outside closes it (Esc is the host's)
    .on_right_press(close);
    stack![base, catch, placed].into()
}

/// Where a button was last drawn (window px): `track` writes it, `anchored` reads it.
#[derive(Debug, Clone, Default)]
pub struct Anchor(Rc<Cell<Rectangle>>);

impl Anchor {
    pub fn get(&self) -> Rectangle {
        self.0.get()
    }
}

struct Tracked(Anchor);

impl<M> super::wrap::Hook<M> for Tracked {
    fn draw(
        &self,
        content: &Element<'_, M>,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.0.0.set(layout.bounds());
        (content.as_widget()).draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
}

/// `button` that remembers where it is drawn in `anchor`.
pub fn track<'a, Message: 'a>(
    button: impl Into<Element<'a, Message>>,
    anchor: &Anchor,
) -> Element<'a, Message> {
    super::wrap::wrap(button, Tracked(anchor.clone()))
}

/// The card's top-left for a card `size` hung under (or, with no room below, over) the
/// `button`, its end edge on the button's end edge (`end`) or its start edge on the
/// button's start edge, kept `GAP` inside `window`. Right-to-left mirrors the edges.
pub fn place(button: Rectangle, size: Size, end: bool, window: Size) -> Point {
    const GAP: f32 = 6.0;
    let right_edge = end != dir::rtl();
    let x = match right_edge {
        true => button.x + button.width - size.width,
        false => button.x,
    };
    let x = x.min(window.width - size.width - GAP).max(GAP);
    let below = button.y + button.height + 4.0;
    let y = match below + size.height + GAP <= window.height {
        true => below,
        false => (button.y - size.height - 4.0).max(GAP),
    };
    Point::new(x, y)
}

/// `base` with `content` in a card hung from `button` (a `track`ed rectangle): the card
/// is `size` big (an estimate: it is placed by it), `end`-aligned or start-aligned with the
/// button, inside `window`. A button not drawn yet (an empty rectangle) falls back to
/// `corner` / `offset`, as `over`.
#[allow(clippy::too_many_arguments)]
pub fn anchored<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    button: Rectangle,
    size: Size,
    end: bool,
    window: Size,
    fallback: (Corner, Padding),
    close: Message,
) -> Element<'a, Message> {
    if button.width <= 0.0 {
        return over(base, content, fallback.0, fallback.1, close);
    }
    let at = place(button, size, end, window);
    let card = opaque(
        surface(
            Material::Widget,
            radius::MENU,
            container(content).padding(space::GAP + 4.0),
        )
        .padding(0),
    );
    let catch = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill),
    )
    .on_press(close.clone()) // mouse only: a press outside closes it (Esc is the host's)
    .on_right_press(close);
    stack![base, catch, pin(card).position(at)].into()
}
