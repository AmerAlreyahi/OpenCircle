//! `SettingRow`: a title (13 px / 500, `title`) and a description (11.5 px,
//! `text_sub`) on the left, the control on the right, a hairline under it.
//! `wide` (a long segmented control) puts the control under the text, at the
//! right, as the design's row wraps it.
use iced::widget::{Space, column, container};
use iced::{Element, Fill, Length, Padding};

use super::divider::divider;
use super::tokens::{Role, space};

/// The description's gap under the title (`mt-0.5`).
const DESC_GAP: f32 = 2.0;
/// Between the text and the control (`gap-x-4`), and its right pad (`pr-2`).
const GAP: f32 = 16.0;
const TEXT_PAD: f32 = 8.0;
/// A wide row's gap between the text and the control under it (`gap-y-2.5`).
const WIDE_GAP: f32 = 10.0;
/// A block's gap between the text and its cards (`space-y-2`).
const BLOCK_GAP: f32 = 8.0;

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

fn ruled<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    pad: f32,
) -> Element<'a, Message> {
    column![container(content).padding([pad, 0.0]), divider()].into()
}

/// A row 10 px top and bottom.
pub fn row<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: &'a str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let text = container(text_block(title, desc))
        .width(Fill)
        .padding(Padding::ZERO.right(TEXT_PAD));
    ruled(
        iced::widget::row![text, control.into()]
            .spacing(GAP)
            .align_y(iced::Center),
        space::ROW_Y,
    )
}

/// A block of cards (`py-2.5 space-y-2`): the text, then `content` across
/// the row under it.
pub fn block<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    ruled(
        column![text_block(title, desc), content.into()].spacing(BLOCK_GAP),
        space::ROW_Y,
    )
}

/// A row 12 px top and bottom with the control under the text, at the right.
pub fn wide<'a, Message: 'a>(
    title: Element<'a, Message>,
    desc: &'a str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let control = iced::widget::row![Space::new().width(Fill), control.into()];
    ruled(
        column![text_block(title, desc), control]
            .spacing(WIDE_GAP)
            .width(Length::Fill),
        space::ROW_Y_WIDE,
    )
}
