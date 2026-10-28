//! The one loading mark (`UI.LogoSpinner`, `core/motion.js` `ocLogoSpin`):
//! the OpenCircle logo turning one full turn every `TURN`, steady (linear),
//! at any size; reduced motion = the logo at rest. No generic spinner or AI
//! glyph. `turn` (0..1 of a turn) comes from the caller's frame clock, so
//! nothing here reads a clock; only the logo's square changes between frames.
use std::f32::consts::TAU;
use std::time::Duration;

use iced::border::Radius;
use iced::widget::{container, image, row};
use iced::{Center, Element, Fill, Radians, Rotation};

use super::text::sub;
use super::themes::c;
use super::tokens::{Role, radius, size};

/// One turn of the logo (`logoSpinnerDuration`).
pub const TURN: Duration = Duration::from_millis(1200);

/// The chat's thinking row's logo (`LogoSpinner(16)`).
const THINKING: f32 = size::ICON;
/// The thinking row's padding (`px-3 py-2`).
const ROW_PAD: [f32; 2] = [8.0, 12.0];
/// Between the logo and the label (`gap-2`).
const ROW_GAP: f32 = 8.0;

/// How far through its turn the logo is `elapsed` after it began (linear).
pub fn turn(elapsed: Duration) -> f32 {
    (elapsed.as_secs_f32() / TURN.as_secs_f32()).fract()
}

/// The logo `px` square, `turn` of the way round; `still`: at rest.
pub fn spinner<'a, Message: 'a>(
    logo: image::Handle,
    px: f32,
    turn: f32,
    still: bool,
) -> Element<'a, Message> {
    let angle = if still { 0.0 } else { turn * TAU };
    image(logo)
        .width(px)
        .height(px)
        .rotation(Rotation::Floating(Radians(angle)))
        .into()
}

/// The AI chat's thinking row (`renderAiThinkingHtml`): the 16 px spinner and
/// `label` (12 px `text_sub`) on `hover`, radius 12.
pub fn thinking<'a, Message: 'a>(
    logo: image::Handle,
    turn: f32,
    still: bool,
    label: &'a str,
) -> Element<'a, Message> {
    container(
        row![spinner(logo, THINKING, turn, still), sub(Role::NOTE, label)]
            .spacing(ROW_GAP)
            .align_y(Center),
    )
    .padding(ROW_PAD)
    .width(Fill)
    .style(|_| container::Style {
        background: Some(c().hover.into()),
        border: iced::Border {
            radius: Radius::new(radius::CARD),
            ..iced::Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_steady_turn_every_turn_length() {
        use super::{TURN, turn};
        assert_eq!(turn(TURN / 4), 0.25);
        assert_eq!(turn(TURN / 2), 0.5);
        assert!(turn(TURN + TURN / 4) - 0.25 < 1e-5, "it goes round again");
    }
}
