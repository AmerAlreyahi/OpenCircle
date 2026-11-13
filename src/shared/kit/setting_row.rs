//! `SettingRow`: a title (13 px / 500, `title`) and a description (11.5 px,
//! `text_sub`) on the left, the control on the right, a hairline under it.
//! Every row's control sits in one column (owner, 2026-10-28): the same left
//! edge and width (`size::CONTROL_COLUMN`) on every page, centred on the
//! row, which is at least `size::ROW_MIN` tall inside the same padding.
use iced::widget::text::IntoFragment;
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
/// A row's single button (`action`), for the harmony test.
pub const ACTION: &str = "setting-row-action";

/// A row's title: `ROW_TITLE` text.
pub fn title<'a, Message: 'a>(s: impl IntoFragment<'a>) -> Element<'a, Message> {
    super::text::text(Role::ROW_TITLE, s).into()
}

fn text_block<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: impl IntoFragment<'a>,
) -> iced::widget::Column<'a, Message> {
    let mut block = column![title]
        .spacing(DESC_GAP)
        .align_x(super::dir::start());
    let desc = desc.into_fragment();
    if !desc.is_empty() {
        block =
            block.push(super::text::sub(Role::ROW_DESC, desc).line_height(super::dir::line(1.6)));
    }
    block
}

fn ruled<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![container(content).padding([space::ROW_Y, 0.0]), divider()].into()
}

/// The text, then the control in the page's control column.
pub fn row<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: impl IntoFragment<'a>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let text = container(text_block(title, desc))
        .width(Fill)
        .align_x(super::dir::start())
        .padding(super::dir::pad(Padding::ZERO.right(TEXT_PAD)));
    let control = container(control)
        .id(Id::new(CONTROL))
        .width(size::CONTROL_COLUMN);
    ruled(
        // (the strut keeps a short row at the least height)
        crate::dir_row![
            Space::new().height(size::ROW_MIN),
            text,
            Space::new().width(GAP),
            control
        ]
        .align_y(iced::Center),
    )
}

/// A row whose control is one button (owner, 2026-11-11): it fills the control
/// column, as tall as the segmented control beside it, its label centred. The
/// secondary look (`Elevated`); a quiet or removing kind keeps its own look, centred.
/// Two buttons in one row: `dir_row![action(..), action(..)].spacing(space::GAP)`
/// (each takes half the column).
pub fn action<'a, Message: Clone + 'a>(
    label: &'a str,
    kind: super::button::Kind,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    action_with(super::button::label(label), kind, on_press)
}

/// `action` with its own content (a label in another colour, a glyph).
pub fn action_with<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    kind: super::button::Kind,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use super::button::{Kind, button, control};
    let h = super::segmented::TRACK_H;
    let one = match kind {
        Kind::Elevated => control(content, h, on_press),
        _ => container(button(content, kind, on_press))
            .center(Fill)
            .height(h)
            .into(),
    };
    container(one).id(Id::new(ACTION)).width(Fill).into()
}

/// A question's Cancel as a row's `action` (Elevated): focused when the
/// question opens (`focus::CANCEL`), so Enter cancels.
pub fn cancel<'a, Message: Clone + 'a>(label: &'a str, on_press: Message) -> Element<'a, Message> {
    let h = super::segmented::TRACK_H;
    let stop = super::button::control_stop(super::button::label(label), h, Some(on_press));
    container(stop.id(super::focus::CANCEL))
        .id(Id::new(ACTION))
        .width(Fill)
        .into()
}

/// A row control of a field and its buttons on one line, within the
/// control column (the field takes what the buttons leave): every "field +
/// buttons" control (the save folder, a key), so its row keeps the normal
/// height.
pub fn field_with<'a, Message: 'a>(
    field: impl Into<Element<'a, Message>>,
    buttons: impl IntoIterator<Item = Element<'a, Message>>,
) -> super::dir::DirRow<'a, Message> {
    let field: Element<'a, Message> = container(field).width(Fill).into();
    super::dir::row(std::iter::once(field).chain(buttons))
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
    ruled(
        column![text_block(title, desc), content.into()]
            .spacing(BLOCK_GAP)
            .align_x(super::dir::start()),
    )
}
