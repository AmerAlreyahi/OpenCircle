//! `.oc-input`: a one-line text field, the installer's small mono size (sizes
//! join with their first user).
use iced::font::Weight;
use iced::widget::text::LineHeight;
use iced::widget::text_input;
use iced::{Border, Element};

use super::text::mono;
use super::themes::c;
use super::tokens::radius;

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
