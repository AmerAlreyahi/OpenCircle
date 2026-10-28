//! Wayland's screenshot permission: onboarding's Screenshots step, and the
//! small window after a capture the desktop refused (`prompt_view`). Both
//! share the Allow button and its status line.
use iced::widget::tooltip::Position;
use iced::widget::{column, container};
use iced::{Element, Fill, Size};

use super::super::flow::{Message, Outcome, State};
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{error, sub, text};
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::tooltip::tip;

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
    Failed(crate::capture::CaptureError),
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
        text(Role::PAGE_TITLE, "Allow screenshots"),
        sub(
            Role::ROW_DESC,
            "Your desktop asks once whether OpenCircle may capture the screen, and only \
             while an OpenCircle window is in front. Click Allow, then choose Allow in the \
             dialog."
        ),
        control(&state.access),
    ]
    .spacing(GAP)
    .into()
}

/// The Allow button and what came of the request.
fn control(access: &Access) -> Element<'_, Message> {
    let (name, press) = match access {
        Access::Asking => ("Waiting…", None),
        Access::Allowed => ("Allowed", None),
        Access::Unasked | Access::Failed(_) => ("Allow", Some(Message::Allow)),
    };
    let allow = tip(
        button(label(name), Kind::Elevated, press),
        "Ask the desktop for screenshot permission",
        Position::Top,
    );
    let status: Element<'_, Message> = match access {
        Access::Unasked => sub(
            Role::ROW_DESC,
            "A test picture is taken and deleted at once.",
        )
        .into(),
        Access::Asking => sub(Role::ROW_DESC, "Answer the dialog your desktop shows.").into(),
        Access::Allowed => sub(Role::ROW_DESC, "OpenCircle can capture your screen now.").into(),
        Access::Failed(e) if e.is_refused() => error(
            Role::ROW_DESC,
            "Your desktop said no. Try again, or allow it later.",
        )
        .into(),
        Access::Failed(e) => error(
            Role::ROW_DESC,
            format!("Not allowed ({e}). Try again, or allow it later."),
        )
        .into(),
    };
    column![allow, status].spacing(space::GAP).into()
}

/// The small window shown after a capture the desktop refused.
pub fn prompt_view(access: &Access) -> Element<'_, Message> {
    let content = column![
        text(Role::PAGE_TITLE, "OpenCircle needs screenshot permission"),
        sub(
            Role::ROW_DESC,
            "Your desktop asks only while an OpenCircle window is in front. Click Allow \
             and accept the dialog, then press your shortcut again."
        ),
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
