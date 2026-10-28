//! `SettingRow`: a title (13 px / 500, `title`) and a description (11.5 px,
//! `text_sub`) on the left, the control on the right, a hairline under it.
//! Every row's control sits in one column (owner, 2026-10-28): the same left
//! edge and width (`size::CONTROL_COLUMN`) on every page, centred on the
//! row, which is at least `size::ROW_MIN` tall inside the same padding.
use iced::widget::{Id, Space, column, container};
use iced::{Element, Fill, Padding};

use super::divider::divider;
use super::tokens::{Role, size, space};

/// The description's gap under the title (`mt-0.5`).
const DESC_GAP: f32 = 2.0;
/// Between the text and the control (`gap-x-4`), and its right pad (`pr-2`).
const GAP: f32 = 16.0;
const TEXT_PAD: f32 = 8.0;
/// A block's gap between the text and its cards (`space-y-2`).
const BLOCK_GAP: f32 = 8.0;
/// The control column's container (the layout test finds every row's).
pub const CONTROL: &str = "setting-row-control";

/// A row's title: `ROW_TITLE` text.
pub fn title<'a, Message: 'a>(s: &'a str) -> Element<'a, Message> {
    super::text::text(Role::ROW_TITLE, s).into()
}

fn text_block<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: &'a str,
) -> iced::widget::Column<'a, Message> {
    let mut block = column![title].spacing(DESC_GAP);
    if !desc.is_empty() {
        block = block.push(super::text::sub(Role::ROW_DESC, desc).line_height(1.6));
    }
    block
}

fn ruled<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![container(content).padding([space::ROW_Y, 0.0]), divider()].into()
}

/// The text, then the control in the page's control column.
pub fn row<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: &'a str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let text = container(text_block(title, desc))
        .width(Fill)
        .padding(Padding::ZERO.right(TEXT_PAD));
    let control = container(control)
        .id(Id::new(CONTROL))
        .width(size::CONTROL_COLUMN);
    ruled(
        // (the strut keeps a short row at the least height)
        iced::widget::row![
            Space::new().height(size::ROW_MIN),
            text,
            Space::new().width(GAP),
            control
        ]
        .align_y(iced::Center),
    )
}

/// A row control of a field and its buttons on one line, within the
/// control column (the field takes what the buttons leave): every "field +
/// buttons" control (the save folder, a key), so its row keeps the normal
/// height.
pub fn field_with<'a, Message: 'a>(
    field: impl Into<Element<'a, Message>>,
    buttons: impl IntoIterator<Item = Element<'a, Message>>,
) -> iced::widget::Row<'a, Message> {
    iced::widget::row![container(field).width(Fill)]
        .extend(buttons)
        .spacing(super::tokens::space::GAP)
        .align_y(iced::Center)
}

/// A block of cards (`py-2.5 space-y-2`): the text, then `content` across
/// the row under it.
pub fn block<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    ruled(column![text_block(title, desc), content.into()].spacing(BLOCK_GAP))
}
