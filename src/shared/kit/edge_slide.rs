//! A panel that slides in over its window from one edge (Studio's Steps panel): `slide_in`
//! shows `content` (`width` wide) `t` of the way in (0 = out of sight, 1 = in place), the
//! rest cut off by the window's edge. Drawn from the state at each frame (the caller's
//! clock, `motion::progress`), never a step per frame; no fade (the CPU renderer cannot).
use iced::advanced::{layout, widget::Tree};
use iced::widget::{button, column, container, text};
use iced::{Element, Length, Vector};

use super::icon::{FONT, Icon};
use super::themes::c;
use super::tokens::{radius, size};
use super::wrap::{Hook, wrap};

/// The handle's width and height.
const HANDLE: (f32, f32) = (22.0, 64.0);

/// The closed panel's handle: a slim tab on the window's edge (`on_left`: the left one) with an
/// arrow toward the window and a count when there is one; a press sends `press`.
pub fn handle<'a, Message: Clone + 'a>(
    count: usize,
    on_left: bool,
    press: Message,
) -> Element<'a, Message> {
    let arrow = match on_left {
        true => Icon::Forward,
        false => Icon::Back,
    };
    let mut body = column![text(arrow.glyph()).font(FONT).size(size::ICON - 2.0)]
        .spacing(6)
        .align_x(iced::Center);
    if count > 0 {
        body = body.push(super::chip::count(count));
    }
    // the corners toward the window are round; the edge side is flat against the window's edge
    let r = radius::CONTROL;
    let corners = match on_left {
        true => iced::border::Radius {
            top_right: r,
            bottom_right: r,
            ..Default::default()
        },
        false => iced::border::Radius {
            top_left: r,
            bottom_left: r,
            ..Default::default()
        },
    };
    let tab = button(container(body).center(Length::Fill))
        .width(HANDLE.0)
        .height(HANDLE.1)
        .padding(0)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(c().elevated.into()),
                text_color: if hot { c().text } else { c().text_sub },
                border: iced::Border {
                    color: c().elevated_border,
                    width: 1.0,
                    radius: corners,
                },
                ..button::Style::default()
            }
        });
    super::focus::press(tab, r, Some(press)).into()
}

/// Moves the content's own node `dx` px: the clip is the outer container's.
struct Shift(f32);

impl<Message> Hook<Message> for Shift {
    fn layout(
        &mut self,
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = content.as_widget_mut().layout(tree, renderer, limits);
        let moved = (node.children().iter())
            .map(|c| c.clone().translate(Vector::new(self.0, 0.0)))
            .collect();
        layout::Node::with_children(node.size(), moved)
    }
}

/// `content` `t` of the way in from the right edge (`from_right`) or the left edge.
pub fn slide_in<'a, Message: 'a>(
    content: Element<'a, Message>,
    width: f32,
    t: f32,
    from_right: bool,
) -> Element<'a, Message> {
    let hidden = (1.0 - t.clamp(0.0, 1.0)) * width;
    let dx = if from_right { hidden } else { -hidden };
    let inner = container(content)
        .width(Length::Fixed(width))
        .height(Length::Fill);
    container(wrap(inner, Shift(dx)))
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .clip(true)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    /// The panel's text is at its place at t = 1, a whole width out at t = 0, and halfway
    /// at 0.5, from either edge.
    #[test]
    fn the_panel_comes_in_from_its_edge_by_the_time() {
        use crate::shared::kit::testing::shot;
        crate::shared::kit::text::load();
        let size = Size::new(200.0, 40.0);
        let x_at = |t: f32, right: bool| -> f32 {
            let word: Element<'static, ()> = iced::widget::text("steps").width(200.0).into();
            let texts = shot::texts_at(slide_in(word, 200.0, t, right), size);
            texts.into_iter().find(|(_, s)| s == "steps").unwrap().0.x
        };
        for right in [true, false] {
            assert_eq!(x_at(1.0, right), 0.0, "in place, right {right}");
            let sign = if right { 1.0 } else { -1.0 };
            assert_eq!(x_at(0.0, right), sign * 200.0, "out, right {right}");
            assert_eq!(x_at(0.5, right), sign * 100.0, "halfway, right {right}");
        }
    }
}
