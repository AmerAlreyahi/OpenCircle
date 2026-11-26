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
use super::icon::Icon;
use super::icon_button;
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

/// Width the close button adds to a card (its size + the gap), for the hosts' size estimates.
pub const X_WIDTH: f32 = 36.0;

/// The one card: `content` with the kit's close button at its top end (mirrored in
/// right-to-left); the X, Esc (the host's) and an outside press all send `close`. Or (`x` false) the content alone: a part that is already a panel (the colour
/// picker) is shown as it is in Snip, with no second container and no X (an outside press
/// and Esc close it).
fn card_with<'a, Message: Clone + 'a>(
    content: Element<'a, Message>,
    close: Message,
    x: bool,
) -> Element<'a, Message> {
    if !x {
        return opaque(content);
    }
    let x = icon_button::square(
        Icon::Close.glyph(),
        icon_button::State::default(),
        Some(close),
    );
    opaque(
        surface(
            Material::Widget,
            radius::MENU,
            container(
                crate::dir_row![content, x]
                    .spacing(space::GAP)
                    .align_y(iced::alignment::Vertical::Top),
            )
            .padding(space::GAP + 4.0),
        )
        .padding(0),
    )
}

/// `base` with `content` in a card at `corner`, `gap` px in from the edges
/// on the side it hangs from (`top`: from the base's top).
#[cfg(test)]
pub fn over<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    corner: Corner,
    offset: Padding,
    close: Message,
) -> Element<'a, Message> {
    over_with(base, content, corner, offset, close, true)
}

fn over_with<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    corner: Corner,
    offset: Padding,
    close: Message,
    x: bool,
) -> Element<'a, Message> {
    let card = card_with(content, close.clone(), x);
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
#[cfg(test)]
pub fn place(button: Rectangle, size: Size, end: bool, window: Size) -> Point {
    place_in(button, size, end, Rectangle::new(Point::ORIGIN, window))
}

/// `place` inside `free` (the window less a side panel the card must not cover).
pub fn place_in(button: Rectangle, size: Size, end: bool, free: Rectangle) -> Point {
    const GAP: f32 = 6.0;
    let window = Size::new(free.x + free.width, free.y + free.height);
    let right_edge = end != dir::rtl();
    let x = match right_edge {
        true => button.x + button.width - size.width,
        false => button.x,
    };
    let x = x.min(window.width - size.width - GAP).max(free.x + GAP);
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
/// (`rise` px lower than it rests while it opens: the host lowers it to 0 over `motion::OPEN`;
/// nothing else redraws but the popover's own place.)
#[allow(clippy::too_many_arguments)]
pub fn anchored_rising<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    button: Rectangle,
    size: Size,
    end: bool,
    free: Rectangle,
    fallback: (Corner, Padding),
    close: Message,
    rise: f32,
) -> Element<'a, Message> {
    anchored_with(
        base, content, button, size, end, free, fallback, close, rise, true,
    )
}

/// `anchored_rising` for content that is already a panel: no card around it, no X.
#[allow(clippy::too_many_arguments)]
pub fn anchored_bare<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    button: Rectangle,
    size: Size,
    end: bool,
    free: Rectangle,
    fallback: (Corner, Padding),
    close: Message,
    rise: f32,
) -> Element<'a, Message> {
    anchored_with(
        base, content, button, size, end, free, fallback, close, rise, false,
    )
}

#[allow(clippy::too_many_arguments)]
fn anchored_with<'a, Message: Clone + 'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    button: Rectangle,
    size: Size,
    end: bool,
    free: Rectangle,
    fallback: (Corner, Padding),
    close: Message,
    rise: f32,
    x: bool,
) -> Element<'a, Message> {
    if button.width <= 0.0 {
        let lower = Padding {
            top: fallback.1.top + rise,
            ..fallback.1
        };
        return over_with(base, content, fallback.0, lower, close, x);
    }
    let mut at = place_in(button, size, end, free);
    at.y += rise;
    let card = card_with(content, close.clone(), x);
    let catch = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill),
    )
    .on_press(close.clone()) // mouse only: a press outside closes it (Esc is the host's)
    .on_right_press(close);
    stack![base, catch, pin(card).position(at)].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::shot;

    /// W90c item 1: every popover has the kit's X at its top end; X, an outside press and
    /// (the host's) Esc all send the one `close`.
    #[test]
    fn w90c_a_popover_has_an_x_and_an_outside_press_closes_it() {
        let size = Size::new(300.0, 200.0);
        let view = || -> Element<'static, u8> {
            over(
                container(iced::widget::text(""))
                    .width(Fill)
                    .height(Fill)
                    .into(),
                iced::widget::text("Position").into(),
                Corner::TopEnd,
                Padding::new(6.0),
                7,
            )
        };
        // the card hangs at the top end: its X is the last thing on its row
        let hits = (0..40)
            .filter(|i| {
                let at = iced::Point::new(size.width - 6.0 - 14.0 - *i as f32 * 0.5, 6.0 + 16.0);
                shot::click(view(), size, at) == vec![7]
            })
            .count();
        assert!(hits > 0, "the X is at the card's top end");
        assert_eq!(
            shot::click(view(), size, iced::Point::new(20.0, 150.0)),
            vec![7]
        );
    }
}
