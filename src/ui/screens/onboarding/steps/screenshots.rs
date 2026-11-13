//! Wayland's screenshot permission: onboarding's Screenshots step, and the
//! small window after a capture the desktop refused (`prompt_view`). Both
//! share the Allow button and its status line.
use crate::t;
use iced::widget::tooltip::Position;
use iced::widget::{column, container};
use iced::{Element, Fill, Size};

use super::super::flow::{Message, Outcome, State};
use crate::shared::kit::button::{Kind, button, label};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{error, sub, text};
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::tooltip::tip;

/// Logical size of the prompt shown after a refused capture.
pub const PROMPT_SIZE: Size = Size::new(500.0, 270.0);
/// Between the prompt's title, words and button.
const GAP: f32 = 14.0;

/// The desktop's screenshot permission (Wayland), as far as we asked. Onboarding's
/// step and the prompt after a refused capture share it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Access {
    #[default]
    Unasked,
    /// A request is out: the desktop's dialog is up.
    Asking,
    Allowed,
    Failed(crate::shared::platform::capture::CaptureError),
}

/// The Allow button (or Allow again after a failure) and the screenshot request's
/// answer: `Outcome::AskScreenshot` when a request should go out.
pub fn ask(access: &mut Access, message: Message) -> Outcome {
    match message {
        Message::Allow if *access != Access::Asking => {
            *access = Access::Asking;
            Outcome::AskScreenshot
        }
        Message::Allowed(answer) => {
            *access = match answer {
                Ok(()) => Access::Allowed,
                Err(e) => Access::Failed(e),
            };
            Outcome::None
        }
        _ => Outcome::None,
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    column![
        text(Role::PAGE_TITLE, t!(o_shots)),
        sub(Role::ROW_DESC, t!(o_shots_desc)),
        control(&state.access),
    ]
    .spacing(GAP)
    .into()
}

/// The Allow button and what came of the request.
fn control(access: &Access) -> Element<'_, Message> {
    let (name, press) = match access {
        Access::Asking => (t!(o_waiting), None),
        Access::Allowed => (t!(o_allowed), None),
        Access::Unasked | Access::Failed(_) => (t!(o_allow), Some(Message::Allow)),
    };
    let allow = tip(
        button(label(name), Kind::Elevated, press),
        t!(o_allow_tip),
        Position::Top,
    );
    let status: Element<'_, Message> = match access {
        Access::Unasked => sub(Role::ROW_DESC, t!(o_once)).into(),
        Access::Asking => sub(Role::ROW_DESC, t!(o_answer)).into(),
        Access::Allowed => sub(Role::ROW_DESC, t!(o_can_capture)).into(),
        Access::Failed(e) if e.is_refused() => error(Role::ROW_DESC, t!(o_said_no)).into(),
        Access::Failed(e) => error(
            Role::ROW_DESC,
            crate::app_language::fmt(crate::app_language::S::o_not_allowed, &[&e]),
        )
        .into(),
    };
    column![allow, status].spacing(space::GAP).into()
}

/// The small window shown after a capture the desktop refused.
pub fn prompt_view(access: &Access) -> Element<'_, Message> {
    let content = column![
        text(Role::PAGE_TITLE, t!(o_needs_shots)),
        sub(Role::ROW_DESC, t!(o_shots_again)),
        control(access),
    ]
    .spacing(GAP);
    surface(
        Material::Window,
        0.0,
        container(content)
            .padding(space::PAGE)
            .width(Fill)
            .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .into()
}
