//! The one scroll area and the one scrollbar: a 5 px round scroller in `rim`
//! on no rail (the design's `::-webkit-scrollbar`), down or across. Every
//! scroll area (pages, panels, code blocks, a dropdown's menu) is one of these.
use iced::widget::scrollable::{self, Direction, Rail, Scrollbar, Scroller};
use iced::{Border, Element};

use super::themes::c;
use super::tokens::radius;

/// The scrollbar's width.
pub const WIDTH: f32 = 5.0;

/// Scrolls down. Right to left its bar is on the left (`Scroll`).
pub fn scroll<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Scroll<'a, Message> {
    let rtl = super::dir::rtl();
    let bar = match rtl {
        true => Scrollbar::hidden(),
        false => bar(),
    };
    Scroll {
        inner: iced::widget::scrollable(content)
            .direction(Direction::Vertical(bar))
            .style(style),
        rtl,
    }
}

/// iced's scroll area, which draws its bar on the right only. Left to right
/// it is just that area. Right to left the area's own bar is hidden and this
/// draws the same 5 px scroller on the left edge, where a press grabs it and
/// a drag moves the page (the wheel, the keys and the page's own redraws stay
/// the area's: a scroll redraws what it redrew before, plus the scroller).
pub struct Scroll<'a, Message> {
    inner: scrollable::Scrollable<'a, Message>,
    rtl: bool,
}

impl<'a, Message: 'a> Scroll<'a, Message> {
    pub fn width(mut self, width: impl Into<iced::Length>) -> Self {
        self.inner = self.inner.width(width);
        self
    }
    pub fn height(mut self, height: impl Into<iced::Length>) -> Self {
        self.inner = self.inner.height(height);
        self
    }
}

impl<'a, Message: 'a> From<Scroll<'a, Message>> for Element<'a, Message> {
    fn from(scroll: Scroll<'a, Message>) -> Self {
        match scroll.rtl {
            true => Element::new(scroll),
            false => scroll.inner.into(),
        }
    }
}

mod left_bar {
    use iced::advanced::layout::{self, Layout};
    use iced::advanced::widget::{Operation, Tree, operation, tree};
    use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
    use iced::widget::scrollable::AbsoluteOffset;
    use iced::{Border, Event, Rectangle, Size, Vector, mouse};

    use super::super::themes::c;
    use super::super::tokens::radius;

    type R = iced::Renderer;
    type T = iced::Theme;

    /// How far in from the area's left edge the scroller is: a page that
    /// starts at the window's left edge (Settings in Arabic) has the window's
    /// resize strip there (`header::EDGE`), on top of everything: a press on a
    /// scroller under it resized the window (seen on the Windows VM).
    const INSET: f32 = super::super::header::EDGE;

    /// How tall the content is and how far it is scrolled (from the last
    /// layout or event; the area's place comes from the layout drawn); the grab.
    #[derive(Default)]
    struct State {
        content: f32,
        down: f32,
        /// The press's distance from the scroller's top while it is held.
        grab: Option<f32>,
    }

    impl State {
        /// The scroller (as iced places it on the right, here on the left,
        /// `INSET` in).
        fn scroller(&self, area: Rectangle) -> Option<Rectangle> {
            let ratio = area.height / self.content;
            (ratio < 1.0).then(|| Rectangle {
                x: area.x + INSET,
                y: (area.y + self.down * ratio).max(0.0),
                width: super::WIDTH,
                height: (area.height * ratio).max(2.0),
            })
        }
        /// The offset that puts the scroller's top at `top`.
        fn offset_at(&self, area: Rectangle, top: f32) -> f32 {
            let room = area.height - (area.height * area.height / self.content).max(2.0);
            let share = ((top - area.y) / room.max(1.0)).clamp(0.0, 1.0);
            share * (self.content - area.height)
        }
    }

    /// Reads the area (the outermost one) and, with `to`, scrolls it there.
    struct Probe<'s>(&'s mut State, Option<f32>, bool);

    impl Operation for Probe<'_> {
        fn traverse(&mut self, _: &mut dyn FnMut(&mut dyn Operation)) {}
        fn scrollable(
            &mut self,
            _: Option<&iced::advanced::widget::Id>,
            _: Rectangle,
            content: Rectangle,
            translation: Vector,
            state: &mut dyn operation::Scrollable,
        ) {
            if std::mem::replace(&mut self.2, true) {
                return;
            }
            (self.0.content, self.0.down) = (content.height, translation.y);
            if let Some(y) = self.1 {
                state.scroll_to(AbsoluteOffset {
                    x: None,
                    y: Some(y),
                });
                self.0.down = y;
            }
        }
    }

    impl<'a, Message> super::Scroll<'a, Message> {
        fn probe(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &R, to: Option<f32>) {
            let state = tree.state.downcast_mut::<State>();
            let mut probe = Probe(state, to, false);
            self.inner
                .operate(&mut tree.children[0], layout, renderer, &mut probe);
        }
    }

    impl<'a, Message> Widget<Message, T, R> for super::Scroll<'a, Message> {
        fn size(&self) -> Size<iced::Length> {
            Widget::size(&self.inner)
        }
        fn size_hint(&self) -> Size<iced::Length> {
            self.inner.size_hint()
        }
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<State>()
        }
        fn state(&self) -> tree::State {
            tree::State::new(State::default())
        }
        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.inner as &dyn Widget<Message, T, R>)]
        }
        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(&[&self.inner as &dyn Widget<Message, T, R>]);
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &R,
            limits: &layout::Limits,
        ) -> layout::Node {
            let node = self.inner.layout(&mut tree.children[0], renderer, limits);
            self.probe(tree, Layout::new(&node), renderer, None);
            node
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
            use iced::advanced::Renderer as _;
            self.inner.draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
            let state = tree.state.downcast_ref::<State>();
            let Some(bar) = state.scroller(layout.bounds()) else {
                return;
            };
            // on its own layer: over the page, as iced draws its own bar
            renderer.with_layer(layout.bounds(), |r| {
                r.fill_quad(
                    renderer::Quad {
                        bounds: bar,
                        border: Border::default().rounded(radius::FULL),
                        ..renderer::Quad::default()
                    },
                    c().rim,
                );
            });
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &R,
            operation: &mut dyn Operation,
        ) {
            self.inner
                .operate(&mut tree.children[0], layout, renderer, operation);
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
            let area = layout.bounds();
            let state = tree.state.downcast_mut::<State>();
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    // the bar's column, as iced's: a press on it never reaches the page
                    let column = Rectangle {
                        x: area.x + INSET,
                        width: super::WIDTH,
                        ..area
                    };
                    if let (Some(at), Some(bar)) =
                        (cursor.position_over(column), state.scroller(area))
                    {
                        // on the scroller: hold it there; beside it: centre it there
                        let grab = match bar.contains(at) {
                            true => at.y - bar.y,
                            false => bar.height / 2.0,
                        };
                        state.grab = Some(grab);
                        let to = state.offset_at(area, at.y - grab);
                        self.probe(tree, layout, renderer, Some(to));
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    if let Some(grab) = state.grab {
                        let to = state.offset_at(area, position.y - grab);
                        self.probe(tree, layout, renderer, Some(to));
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                    if state.grab.take().is_some() =>
                {
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
            self.inner.update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
            // the wheel, the keys or a page change may have scrolled it
            let before = tree.state.downcast_ref::<State>().down;
            self.probe(tree, layout, renderer, None);
            if tree.state.downcast_ref::<State>().down != before {
                shell.request_redraw();
            }
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &R,
        ) -> mouse::Interaction {
            let state = tree.state.downcast_ref::<State>();
            let on_bar = (state.scroller(layout.bounds()).zip(cursor.position()))
                .is_some_and(|(b, p)| b.contains(p));
            match state.grab.is_some() || on_bar {
                true => mouse::Interaction::Idle,
                false => self.inner.mouse_interaction(
                    &tree.children[0],
                    layout,
                    cursor,
                    viewport,
                    renderer,
                ),
            }
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &R,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, T, R>> {
            self.inner.overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}

/// Scrolls down with no scrollbar (the AI chat's log: the wheel and the
/// keys scroll it; a bar on its edge read as a line in the border, owner
/// 2026-10-29).
pub fn quiet<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> scrollable::Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(Direction::Vertical(Scrollbar::hidden()))
        .style(style)
}

/// Scrolls across (a code block's long lines).
pub fn across<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> scrollable::Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(Direction::Horizontal(bar()))
        .style(style)
}

/// The scroller alone, `length` tall (a list that scrolls by rows draws its
/// own): the same 5 px round `rim` bar.
pub fn thumb<'a, Message: 'a>(length: f32) -> Element<'a, Message> {
    iced::widget::container(iced::widget::Space::new())
        .width(WIDTH)
        .height(length)
        .style(|_| iced::widget::container::Style {
            background: Some(c().rim.into()),
            border: Border::default().rounded(radius::FULL),
            ..Default::default()
        })
        .into()
}

fn bar() -> Scrollbar {
    Scrollbar::new().width(WIDTH).scroller_width(WIDTH)
}

/// The one scrollbar look, for either direction.
fn style(theme: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let rail = Rail {
        background: None,
        border: Border::default(),
        scroller: Scroller {
            background: c().rim.into(),
            border: Border::default().rounded(radius::FULL),
        },
    };
    style.vertical_rail = rail;
    style.horizontal_rail = rail;
    style
}

#[cfg(test)]
mod tests {
    use iced::mouse::{Button, Event as M, ScrollDelta};
    use iced::{Element, Event, Point, Size};

    use crate::shared::kit::testing::shot;

    const SIZE: Size = Size::new(200.0, 200.0);

    /// 40 rows in a 200 px area, built right to left or not.
    fn area(rtl: bool) -> Element<'static, ()> {
        let _dir = super::super::dir::scope(rtl);
        let rows = (0..40).map(|i| {
            crate::shared::kit::text::text(super::super::tokens::Role::ROW_DESC, format!("row {i}"))
                .into()
        });
        let column = iced::widget::column(rows)
            .width(iced::Fill)
            .padding([0, 20]);
        super::scroll(column).height(iced::Fill).into()
    }

    /// The rows of column `x` where the scroller is drawn (unlike the bare
    /// background at x = 150, right of every row's text): (top, bottom).
    fn scroller(rgba: &[u8], x: usize) -> Option<(usize, usize)> {
        let w = SIZE.width as usize;
        let at = |x: usize, y: usize| &rgba[(y * w + x) * 4..][..3];
        let rows: Vec<usize> = (0..SIZE.height as usize)
            .filter(|&y| (0..3).any(|k| at(x, y)[k].abs_diff(at(150, y)[k]) > 8))
            .collect();
        Some((*rows.first()?, *rows.last()?))
    }

    /// Right to left the scroller is on the left edge (none on the right); a
    /// press on it and a drag move the page, and so does the wheel. Left to
    /// right it is iced's, on the right.
    #[test]
    fn right_to_left_the_scroller_is_on_the_left_and_drags_and_wheels() {
        let still = shot::render(area(true), 200, 200);
        let (top, bottom) = scroller(&still, 8).expect("a scroller on the left");
        assert_eq!(top, 0);
        assert!(scroller(&still, 197).is_none(), "and none on the right");
        let ltr = shot::render(area(false), 200, 200);
        assert!(scroller(&ltr, 197).is_some() && scroller(&ltr, 8).is_none());
        // grab it in its middle and drag 60 px down
        let grab = Point::new(8.0, (bottom / 2) as f32);
        let events = [
            Event::Mouse(M::ButtonPressed(Button::Left)),
            Event::Mouse(M::CursorMoved {
                position: Point::new(8.0, grab.y + 60.0),
            }),
            Event::Mouse(M::ButtonReleased(Button::Left)),
        ];
        let dragged = shot::render_with(area(true), SIZE, Some(grab), &events);
        let (moved, _) = scroller(&dragged, 8).unwrap();
        assert!((58..=62).contains(&moved), "dragged to {moved}");
        assert_ne!(dragged, still, "the page moved");
        // the wheel
        let wheel = [Event::Mouse(M::WheelScrolled {
            delta: ScrollDelta::Pixels { x: 0.0, y: -100.0 },
        })];
        let wheeled = shot::render_with(area(true), SIZE, Some(Point::new(100.0, 100.0)), &wheel);
        assert!(scroller(&wheeled, 8).unwrap().0 > 0, "the wheel moved it");
    }
}
