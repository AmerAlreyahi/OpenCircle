//! The one scroll area and the one scrollbar: a 5 px round scroller in `rim`
//! on no rail (the design's `::-webkit-scrollbar`), down or across. Every
//! scroll area (pages, panels, code blocks, a dropdown's menu) is one of these.
use iced::widget::scrollable::{self, Direction, Rail, Scrollbar, Scroller};
use iced::{Border, Element};

use super::themes::c;
use super::tokens::radius;

/// The scrollbar's width.
pub const WIDTH: f32 = 5.0;

/// Scrolls down.
pub fn scroll<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> scrollable::Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(Direction::Vertical(bar()))
        .style(style)
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
