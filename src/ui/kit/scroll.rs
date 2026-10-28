//! The one scroll area: vertical, a 5 px round scroller in `rim` on no rail
//! (the design's `::-webkit-scrollbar`).
use iced::widget::scrollable::{self, Direction, Rail, Scrollbar, Scroller};
use iced::{Border, Element};

use super::themes::c;
use super::tokens::radius;

const WIDTH: f32 = 5.0;

pub fn scroll<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> scrollable::Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(Direction::Vertical(
            Scrollbar::new().width(WIDTH).scroller_width(WIDTH),
        ))
        .style(|theme, status| {
            let mut style = scrollable::default(theme, status);
            style.vertical_rail = Rail {
                background: None,
                border: Border::default(),
                scroller: Scroller {
                    background: c().rim.into(),
                    border: Border::default().rounded(radius::FULL),
                },
            };
            style
        })
}
