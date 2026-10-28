//! One file per onboarding step (`flow::STEPS` lists them).
pub mod ai;
pub mod done;
pub mod look;
pub mod screenshots;
pub mod welcome;

use iced::Element;
use iced::widget::{column, image, row};

use super::flow::Message;
use crate::ui::kit::card::card;
use crate::ui::kit::text::{sub, text};
use crate::ui::kit::tokens::{Role, space};

/// Between the logo and the card's text (`gap-4`).
const LOGO_GAP: f32 = 16.0;

/// A card with the logo (`logo` px) beside a title and a line under it
/// (Welcome's and Done's), `trailing` at its right.
fn logo_card<'a>(
    logo: f32,
    title: impl Into<String>,
    line: &'a str,
    trailing: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let handle = crate::ui::kit::logo::logo(logo as u32);
    let words = column![
        text(Role::ROW_TITLE, title.into()),
        sub(Role::ROW_DESC, line).line_height(1.5)
    ]
    .spacing(space::GAP / 2.0)
    .width(iced::Fill);
    card(
        row![image(handle).width(logo).height(logo), words]
            .push(trailing)
            .spacing(LOGO_GAP)
            .align_y(iced::Center),
    )
}
