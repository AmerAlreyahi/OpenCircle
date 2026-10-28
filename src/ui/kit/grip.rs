//! `grip(on_press)`: the design's drag grip (`DragGrip`): six dots (14 px)
//! with 8x6 px round them, `text` at 45 %, 85 % under the cursor. A press on
//! it starts the drag (a button reacts on release, too late for a drag).
use iced::widget::{container, hover, mouse_area, text};
use iced::{Element, Size, mouse};

use super::button::faded;
use super::icon::FONT;
use super::themes::c;

/// lucide "grip-vertical": two columns of three dots.
const DOTS: char = '\u{e0ee}';

/// Its whole size: the dots and the room round them.
pub const SIZE: Size = Size::new(30.0, 26.0);

pub fn grip<'a, Message: Clone + 'a>(on_press: Message) -> Element<'a, Message> {
    let dots = |alpha: f32| {
        container(text(DOTS).font(FONT).size(14).color(faded(c().text, alpha)))
            .center_x(SIZE.width)
            .center_y(SIZE.height)
    };
    // under the cursor the brighter dots are drawn over the dim ones:
    // 1 - (1 - .45)(1 - .73) = .85
    mouse_area(hover(dots(0.45), dots(0.73)))
        .on_press(on_press)
        .interaction(mouse::Interaction::Grab)
        .into()
}
