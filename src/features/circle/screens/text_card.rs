//! The right-click menu's Translate (owner, 2026-11-25): the SELECTED TEXT's translation in a
//! small card beside the selection, with Copy. The overlay stays open. While it works the card
//! says so; a failure is the one error card (F12). `App` places it and runs the work
//! (`app::text_card`).
use iced::widget::{column, container};
use iced::{Element, Fill, Rectangle};

use crate::shared::app_language::{S, tr};
use crate::shared::kit::button::regular;
use crate::shared::kit::error_card::{ErrorCard, error_card};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, own};
use crate::shared::kit::tokens::{Role, radius};

pub const WIDTH: f32 = 360.0;
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Translating,
    Done(String),
    Failed(String),
}

/// The open card: which request it answers, what it shows, and the selection it sits by (screen px).
#[derive(Debug, Clone, PartialEq)]
pub struct TextCard {
    pub id: u32,
    pub state: State,
    pub near: Rectangle,
    pub copied: bool,
    pub details: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Copy,
    Details,
}

impl TextCard {
    pub fn new(id: u32, near: Rectangle) -> Self {
        TextCard {
            id,
            state: State::Translating,
            near,
            copied: false,
            details: false,
        }
    }

    /// About how tall the card is (to keep it on the screen).
    pub fn height(&self) -> f32 {
        let chars = match &self.state {
            State::Translating => 12,
            State::Done(t) | State::Failed(t) => t.chars().count(),
        };
        let lines = (chars as f32 * 7.0 / (WIDTH - 2.0 * PAD))
            .ceil()
            .clamp(1.0, 12.0);
        2.0 * PAD + 36.0 + lines * 20.0
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let body: Element<'_, Msg> = match &self.state {
            State::Translating => muted(Role::NOTE, tr(S::tp_translating)).into(),
            State::Done(text) => {
                let label = tr(if self.copied {
                    S::pill_copied
                } else {
                    S::c_copy
                });
                column![
                    own(Role::ROW_TITLE, text.as_str()).width(Fill),
                    container(regular(label, false, Some(Msg::Copy)))
                        .align_x(crate::shared::kit::dir::start()),
                ]
                .spacing(GAP)
                .into()
            }
            State::Failed(message) => {
                let first = message.lines().next().unwrap_or_default().to_string();
                let more = (first.len() < message.trim().len()).then(|| message.clone());
                error_card(ErrorCard {
                    headline: tr(S::tp_failed).into(),
                    hint: first,
                    detail: more.unwrap_or_default(),
                    width: WIDTH - 2.0 * PAD,
                    open: self.details,
                    on_details: Msg::Details,
                    fix: None,
                    retry: None,
                    words: crate::shared::theme::error_words(),
                })
            }
        };
        surface(Material::Widget, radius::CARD, body)
            .padding(PAD)
            .width(WIDTH)
            .into()
    }
}
