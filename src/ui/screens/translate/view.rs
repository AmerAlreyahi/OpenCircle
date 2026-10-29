//! What translate in place shows over the frame (the design's "Translate
//! screen" states), from kit parts only: the language pill (source → target,
//! the status), and the result card for its prompts, errors and the word card.
use iced::widget::{column, container, opaque, pin, row, stack};
use iced::{Center, Element, Fill, Rectangle, Size};

use super::{Card, Msg, PICKER_LIST, Phase, Session};
use crate::actions::ask::Status;
use crate::actions::{self};
use crate::ui::kit::markdown;
use crate::ui::kit::panel_frame::{self as frame, Header};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{error, text as label};
use crate::ui::kit::tokens::{Role, radius};
use crate::ui::panel::{actions, card, note};
use crate::ui::picker;
use crate::ui::screens::model_download::{self, OCR, Stage, TRANSLATION};

/// The pill's padding and its gap (`.as-language-pill`: 10 px, gap 10), and
/// its distance to the top of the screen.
const PILL_PAD: f32 = 10.0;
const PILL_TOP: f32 = 24.0;
/// Between a card's blocks.
const STACK: f32 = 10.0;

impl Session {
    /// The layer over the frame: status and the language picker, and the
    /// floating result card (at `at`, like every result card) for prompts,
    /// errors and the word card. `hover`: on the card's resize edge.
    pub fn view<'a>(&'a self, screen: Size, at: Rectangle, hover: bool) -> Element<'a, Msg> {
        // a prompt is as tall as its text and buttons, not a full-height slab
        let card = |body: Element<'a, Msg>,
                    footer: Option<Element<'a, Msg>>,
                    fit: Option<(usize, f32)>,
                    title: Option<&'a str>| {
            let at = fit.map_or(at, |(chars, below)| frame::short(at, screen, chars, below));
            let title = title.unwrap_or(actions::translate::ACTION.label);
            let header = Header {
                glyph: actions::translate::ACTION.icon,
                title,
                chip: None,
                menu: false,
                held: false,
                logo: None,
            };
            card(header, body, footer, at, screen, hover, Msg::Card)
        };
        // the pill stays beside the card
        let with_pill = |card: Element<'a, Msg>| -> Element<'a, Msg> {
            let free = frame::free(screen, Some(at));
            let pill = container(self.languages(None)).width(free.width);
            stack![pin(pill).position(free.position()), card].into()
        };
        // the download card (`model_download`): asks, its progress, why it failed
        let download = |model, bytes, stage, provider| {
            let on = model_download::On {
                download: Msg::Download,
                dismiss: Some(Msg::Close),
                provider,
            };
            let card = model_download::card(model, bytes, stage, on);
            with_pill(model_download::placed(card, at, screen))
        };
        // what is (being) fetched: the pair's models, else OCR's
        let fetched = |stage| match self.offline {
            Some(pair) => download(&TRANSLATION, pair.full_size(), stage, None),
            None => download(&OCR, crate::ocr::models::full_size(), stage, None),
        };
        match &self.phase {
            Phase::Consent(None) => fetched(Stage::Ask),
            Phase::Consent(Some((_, bytes))) => {
                download(&TRANSLATION, *bytes, Stage::Ask, Some(Msg::OpenSettings))
            }
            Phase::Downloading(done) => fetched(Stage::at(*done)),
            Phase::DownloadFailed(why) => fetched(Stage::Failed(why)),
            Phase::Translating => self.languages(Some("Translating…")),
            Phase::Failed(problem) => {
                let buttons = match problem.setup {
                    true => actions(("Set up AI", Msg::OpenSettings), [("Close", Msg::Close)]),
                    false => actions(("Close", Msg::Close), []),
                };
                // the picker stays: another pair may work offline
                let body = error(Role::NOTE, problem.message.clone()).into();
                let fit = Some((problem.message.chars().count(), 77.0));
                with_pill(card(body, Some(buttons), fit, None))
            }
            Phase::Shown(_) => match &self.card {
                Some(word) => {
                    let (body, footer) = card_view(word);
                    with_pill(card(body, Some(footer), None, Some(&word.title)))
                }
                None => self.languages(None),
            },
        }
    }

    /// The pill at the top (`.as-language-pill`): source → target (the
    /// picker), and `status`.
    fn languages(&self, status: Option<&'static str>) -> Element<'_, Msg> {
        let mut top = row![self.picker.chips(picker::CHIP_HEIGHT).map(Msg::Picker)]
            .spacing(PILL_PAD)
            .align_y(Center);
        if let Some(status) = status {
            top = top.push(note(status));
        }
        let mut body = column![top].spacing(picker::SPACING);
        if let Some(list) = self.picker.list(PICKER_LIST) {
            body = body.push(list.map(Msg::Picker));
        }
        let pill = surface(Material::Widget, radius::DRAWER, body).padding(PILL_PAD);
        container(opaque(pill))
            .center_x(Fill)
            .padding(PILL_TOP)
            .into()
    }
}

/// The word card (the design's "Word explanation", titled "translation ·
/// original"): its body (the translation, the AI's explanation) and actions
/// (Explain = E, Ask AI = A).
fn card_view(card: &Card) -> (Element<'_, Msg>, Element<'_, Msg>) {
    // Mixed lines ("«name» ...") would each pick their own side: a
    // right-to-left target's lines are laid at the right as a block. (A
    // `Fill` text aligned right drew nothing in the card.)
    let side = |line: iced::widget::Text<'static>| -> Element<'_, Msg> {
        match card.rtl {
            true => container(line).align_right(Fill).into(),
            false => line.width(Fill).into(),
        }
    };
    let mut body = column![
        note(card.block.original.clone()).width(Fill),
        side(label(Role::CHAT, card.block.translation.clone())),
    ]
    .spacing(STACK);
    if let Some(turn) = card.chat.turns.last() {
        body = body.push(
            match turn.answer.is_empty() && card.chat.status == Status::Streaming {
                true => note("Thinking…").width(Fill).into(),
                // Markdown, as the chat shows it (lines shape by their own direction)
                false => markdown::markdown(card.answer.items()).map(Msg::Answer),
            },
        );
    }
    if let Status::Failed(e) = &card.chat.status {
        body = body.push(error(Role::NOTE, e.clone()));
    }
    let ask = &actions::ask::ACTION;
    let buttons = actions(("Explain", Msg::Explain), [(ask.label, Msg::Ask)]);
    (body.into(), buttons)
}
