//! `.oc-input`: a one-line text field. `input` is the design's (Settings'
//! URL, model and key fields), `field` the installer's small mono one, `slots`
//! the bar's AI field.
use iced::font::Weight;
use iced::widget::text::LineHeight;
use iced::widget::text_input;
use iced::{Border, Element};

use super::button::faded;
use super::text::mono;
use super::themes::c;
use super::tokens::radius;

/// `.oc-input`: 12 px (`mono`: the mono font) on `track`, 6x11 px, radius 10,
/// a `rim` edge that turns `accent` while focused and `DANGER` when `invalid`
/// (the design's 2 px focus ring is left out: a text field can't draw outside
/// itself). The caller adds `on_input`, `secure`, `on_submit` and its width.
pub fn input<'a, Message: Clone + 'a>(
    placeholder: &str,
    value: &str,
    mono_font: bool,
    invalid: bool,
) -> text_input::TextInput<'a, Message> {
    use super::tokens::{Role, fixed};
    let role = Role::NOTE;
    let font = match mono_font {
        true => mono(role.weight),
        false => super::text::font(role),
    };
    text_input(placeholder, value)
        .font(font)
        .size(role.size)
        .line_height(1.5)
        .padding([6, 11])
        .style(move |_, status| text_input::Style {
            background: c().track.into(),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(match status {
                    _ if invalid => fixed::DANGER,
                    text_input::Status::Focused { .. } => c().accent,
                    _ => c().rim,
                }),
            icon: c().text_sub,
            placeholder: c().text_muted,
            value: c().text,
            selection: c().accent_soft,
        })
}

/// `.as-textarea`: `.oc-input` over several lines (the Text card's text):
/// 12 px on `track`, line 1.65, 12 px in, radius 10, a `rim` edge that turns
/// `accent` while focused; as tall as its place.
pub fn editor<'a, Message: Clone + 'a>(
    content: &'a iced::widget::text_editor::Content,
    on_action: impl Fn(iced::widget::text_editor::Action) -> Message + 'a,
) -> Element<'a, Message> {
    use super::tokens::Role;
    use iced::widget::text_editor;
    text_editor(content)
        .on_action(on_action)
        .font(super::text::font(Role::NOTE))
        .size(Role::NOTE.size)
        .line_height(1.65)
        .padding(12)
        .height(iced::Fill)
        .style(|_, status| text_editor::Style {
            background: c().track.into(),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(match status {
                    text_editor::Status::Focused { .. } => c().accent,
                    _ => c().rim,
                }),
            placeholder: c().text_muted,
            value: c().text,
            selection: c().accent_soft,
        })
        .into()
}

/// Mono 10.5 px on `track`, a rim edge that turns `accent` while focused. No
/// `on_input` = read-only.
pub fn field<'a, Message: Clone + 'a>(
    value: &str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
) -> Element<'a, Message> {
    let mut input = text_input("", value)
        .font(mono(Weight::Normal))
        .size(10.5)
        .line_height(LineHeight::Absolute(16.into()))
        .padding([4, 8])
        .on_submit(on_submit)
        .style(|_, status| text_input::Style {
            background: c().track.into(),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(match status {
                    text_input::Status::Focused { .. } => c().accent,
                    _ => c().rim,
                }),
            icon: c().text_sub,
            placeholder: c().text_muted,
            value: match status {
                text_input::Status::Disabled => c().text_sub,
                _ => c().text,
            },
            selection: c().accent_soft,
        });
    if let Some(on_input) = on_input {
        input = input.on_input(on_input);
    }
    input.into()
}

/// The slotted field's height, the focus ring round it, and its text's room
/// left and right of the slots (`pl-8 pr-14`).
pub const SLOTS_H: f32 = 30.0;
pub const RING: f32 = 2.0;
pub const SLOTS_LEFT: f32 = 32.0;
pub const SLOTS_RIGHT: f32 = 56.0;
/// Its text size.
pub const SLOTS_TEXT: f32 = 12.0;

/// The field with slots (the bar's AI field, `.oc-input` in the bar): 12 px
/// text on `track`, a rim edge, radius 12, `width` wide with its ring (the
/// field itself `2 * RING` less), `SLOTS_H` tall; `left` and `right` (a logo
/// button, a badge) sit on it, 8 and 6 px in from its ends. `focused` (or iced's
/// own focus) = an `accent` edge and a 2 px `accent_soft` ring round it; the
/// ring's room is kept at rest, so focus never moves anything.
#[allow(clippy::too_many_arguments)]
pub fn slots<'a, Message: Clone + 'a>(
    id: &'static str,
    placeholder: &str,
    value: &str,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
    width: f32,
    focused: bool,
    (left, right): (Element<'a, Message>, Element<'a, Message>),
) -> Element<'a, Message> {
    use iced::widget::{container, row, space, stack};
    use iced::{Center, Padding};
    // Arabic with all its marks inside the box (the UI's `LINE`)
    const LINE: f32 = 1.7;
    let inner = width - 2.0 * RING;
    let input = text_input(placeholder, value)
        .id(id)
        .on_input(on_input)
        .on_submit(on_submit)
        .size(SLOTS_TEXT)
        .font(super::text::weight(Weight::Normal))
        .line_height(LINE)
        .width(inner)
        .padding(
            Padding::new((SLOTS_H - SLOTS_TEXT * LINE) / 2.0)
                .left(SLOTS_LEFT)
                .right(SLOTS_RIGHT),
        )
        .style(move |_, status| {
            let on = focused || matches!(status, text_input::Status::Focused { .. });
            text_input::Style {
                background: c().track.into(),
                border: Border::default()
                    .rounded(radius::CARD)
                    .width(1)
                    .color(if on { c().accent } else { c().rim }),
                icon: c().text_sub,
                placeholder: c().text_sub,
                value: c().text,
                selection: c().accent_soft,
            }
        });
    let over = row![left, space::horizontal(), right]
        .padding(Padding::ZERO.left(8).right(6))
        .width(inner)
        .height(SLOTS_H)
        .align_y(Center);
    container(stack![input, over])
        .padding(RING)
        .style(move |_| container::Style {
            border: Border::default()
                .rounded(radius::CARD + RING)
                .width(RING)
                .color(match focused {
                    true => c().accent_soft,
                    false => iced::Color::TRANSPARENT,
                }),
            ..container::Style::default()
        })
        .into()
}

/// A slot's logo button (`#mainBarOpenAiBtn`): `logo` at 16 px in 20x20,
/// radius 6, `hover` under the cursor; Tab's focus an `accent` edge.
pub fn logo<'a, Message: Clone + 'a>(
    logo: iced::widget::image::Handle,
    focused: bool,
    on_press: Message,
) -> Element<'a, Message> {
    use iced::widget::{button, container, image};
    let picture = image(logo).width(16).height(16);
    button(container(picture).center(20))
        .padding(0)
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(c().hover.into()),
                _ => None,
            },
            border: slot_edge(focused),
            ..button::Style::default()
        })
        .into()
}

/// A slot's badge (`#mainBarAiSubmitBadge`): "AI ↵" (the arrow at 75 %),
/// mono 10 px / 600 in `accent` on `accent_soft`, 2x6 px, radius 6; stronger
/// under the cursor; Tab's focus an `accent` edge.
pub fn badge<'a, Message: Clone + 'a>(
    label: &'a str,
    focused: bool,
    on_press: Message,
) -> Element<'a, Message> {
    use iced::widget::{button, row, text};
    let font = mono(Weight::Semibold);
    let line = row![
        text(label).size(10).font(font),
        super::icon::icon(super::icon::Icon::Enter, 10.0).color(faded(c().accent, 0.75)),
    ]
    .spacing(4);
    button(line)
        .padding([2, 6])
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => faded(c().accent, 0.28),
                    _ => c().accent_soft,
                }
                .into(),
            ),
            text_color: c().accent,
            border: slot_edge(focused),
            ..button::Style::default()
        })
        .into()
}

/// A slot's corners (6 px) and its edge: `accent` while Tab is on it.
fn slot_edge(focused: bool) -> Border {
    Border::default().rounded(6).width(1).color(match focused {
        true => c().accent,
        false => iced::Color::TRANSPARENT,
    })
}
