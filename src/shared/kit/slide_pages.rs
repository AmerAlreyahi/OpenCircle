//! Two pages of one panel, one sliding in as the other slides out (a drill-in panel: the
//! Steps list and its Export page). `pages(first, second, width, t, rtl)`: `t` 0 shows
//! `first`, 1 shows `second`, between them both move by `width * t`; the pair is clipped
//! to `width`. Drawn from the state at each frame (the caller's clock, `motion::progress`),
//! never a step per frame.
use iced::advanced::{layout, widget::Tree};
use iced::widget::{container, row};
use iced::{Element, Length, Size, Vector};

use super::wrap::{Hook, wrap};

struct Slide {
    width: f32,
    /// How far the pair has moved toward its second page (0..1).
    t: f32,
    rtl: bool,
}

impl<Message> Hook<Message> for Slide {
    fn layout(
        &mut self,
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // the pair is two pages wide whatever room the parent gives
        let wide =
            layout::Limits::new(Size::ZERO, Size::new(2.0 * self.width, limits.max().height));
        let node = content.as_widget_mut().layout(tree, renderer, &wide);
        let h = node.size().height;
        // left to right: first, second; right to left they swap sides
        let x = match self.rtl {
            false => -self.t * self.width,
            true => -(1.0 - self.t) * self.width,
        };
        // (the row's own node, its children moved: the row draws its pages from there)
        let pages = (node.children().iter())
            .map(|c| c.clone().translate(Vector::new(x, 0.0)))
            .collect();
        layout::Node::with_children(Size::new(self.width, h), pages)
    }
}

/// The pair, `width` wide, `t` of the way from `first` to `second` (see the module).
pub fn pages<'a, Message: 'a>(
    first: Element<'a, Message>,
    second: Element<'a, Message>,
    width: f32,
    t: f32,
    rtl: bool,
) -> Element<'a, Message> {
    let (a, b) = (
        container(first).width(Length::Fixed(width)),
        container(second).width(Length::Fixed(width)),
    );
    let both = match rtl {
        false => row![a, b],
        true => row![b, a],
    }
    .width(Length::Fixed(2.0 * width));
    container(wrap(both, Slide { width, t, rtl }))
        .width(Length::Fixed(width))
        .clip(true)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W90d.8: the first page is at the panel's edge at t = 0, the second at t = 1, both
    /// halfway between (English and Arabic).
    #[test]
    fn the_pages_slide_by_the_time() {
        use crate::shared::kit::testing::shot;
        crate::shared::kit::text::load();
        let size = Size::new(200.0, 40.0);
        let page = |s: &'static str| -> Element<'static, ()> {
            iced::widget::text(s).width(Length::Fixed(200.0)).into()
        };
        let texts = |t: f32, rtl: bool| -> Vec<(String, f32)> {
            let view = pages(page("one"), page("two"), 200.0, t, rtl);
            (shot::texts_at(view, size).into_iter())
                .map(|(r, s)| (s, r.x))
                .collect()
        };
        let at = |v: &[(String, f32)], w: &str| v.iter().find(|(s, _)| s == w).map(|(_, x)| *x);
        for rtl in [false, true] {
            assert_eq!(at(&texts(0.0, rtl), "one"), Some(0.0), "rtl {rtl}");
            assert_eq!(at(&texts(1.0, rtl), "two"), Some(0.0), "rtl {rtl}");
            let mid = texts(0.5, rtl);
            let (one, two) = (at(&mid, "one").unwrap(), at(&mid, "two").unwrap());
            assert_eq!((one - two).abs(), 200.0, "{one} {two}");
            assert_eq!(one.abs().min(two.abs()), 100.0, "halfway: {one} {two}");
        }
    }
}
