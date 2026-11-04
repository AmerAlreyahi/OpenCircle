//! What translate in place shows over the frame (the design's "Translate
//! screen" states), from kit parts only: the language pill (source → target,
//! the status), and the result card for its prompts, errors and the word card.
use iced::widget::{column, container, opaque, pin, row, stack};
use iced::{Center, Element, Fill, Rectangle, Size};

use super::{Card, Msg, PICKER_LIST, Phase, Session};
use crate::actions::ask::Status;
use crate::actions::{self};
use crate::ui::ai_chat::{actions, card, note};
use crate::ui::kit::button::{Kind, button, label as button_label};
use crate::ui::kit::markdown;
use crate::ui::kit::panel_frame::{self as frame, Header};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{error, text as label};
use crate::ui::kit::tokens::{Role, radius};
use crate::ui::picker;
use crate::ui::screens::model_download::{self, Model, OCR, SCRIPTS, Stage, TRANSLATION};

/// The pill's padding and its gap (`.as-language-pill`: 10 px, gap 10), and
/// its distance to the top of the screen.
const PILL_PAD: f32 = 10.0;
const PILL_TOP: f32 = 24.0;
/// Between a card's blocks.
const STACK: f32 = 10.0;
/// The note under the pill (`surface::hint`'s padding).
const NOTE_PAD: [f32; 2] = [8.0, 14.0];

/// A problem's first sentence: the rest is how-to the "Set up AI" action covers.
fn gist(message: &str) -> &str {
    let end = ([". ", " ("].iter().filter_map(|m| message.find(m)))
        .min()
        .unwrap_or(message.len());
    message[..end].trim_end_matches('.')
}

impl Session {
    /// The layer over the frame: status and the language picker, and the
    /// floating result card (at `at`, like every result card) for prompts,
    /// errors and the word card. `hover`: on the card's resize edge.
    pub fn view<'a>(&'a self, screen: Size, at: Rectangle, hover: bool) -> Element<'a, Msg> {
        let card =
            |body: Element<'a, Msg>, footer: Option<Element<'a, Msg>>, title: Option<&'a str>| {
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
        let download = |model: &Model, bytes, stage, provider| {
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
            None => match &self.readers {
                // only the readers that aren't installed
                Some((bytes, names)) => {
                    let does = format!("Read {names} text");
                    download(
                        &Model {
                            does: &does,
                            ..SCRIPTS
                        },
                        *bytes,
                        stage,
                        None,
                    )
                }
                None => download(&OCR, crate::ocr::models::full_size(), stage, None),
            },
        };
        match &self.phase {
            Phase::Consent(None) => fetched(Stage::Ask),
            Phase::Consent(Some((_, bytes))) => {
                download(&TRANSLATION, *bytes, Stage::Ask, Some(Msg::OpenSettings))
            }
            Phase::Downloading(done) => fetched(Stage::at(*done)),
            Phase::DownloadFailed(why) => fetched(Stage::Failed(why)),
            Phase::Translating => self.languages(Some("Translating…")),
            // Already in the target, no offline model, an AI error: one soft line
            // under the pill, never a card, and the pill stays put (owner,
            // 2026-11-04). The picker stays: another pair may work offline.
            Phase::Failed(problem) => {
                let mut line = row![note(gist(&problem.message))]
                    .spacing(PILL_PAD)
                    .align_y(Center);
                if problem.setup {
                    let set_up = button_label("Set up AI");
                    line = line.push(button(set_up, Kind::Quiet, Some(Msg::OpenSettings)));
                }
                let below = surface(Material::Widget, radius::FULL, line).padding(NOTE_PAD);
                self.pill(None, None, Some(below.into()))
            }
            // lines kept in their own language: a quiet offer in the pill
            Phase::Shown(shown) if self.card.is_none() && shown.offer.is_some() => {
                self.offer(shown.offer.as_ref().expect("just checked"))
            }
            Phase::Shown(_) => match &self.card {
                Some(word) => {
                    let (body, footer) = card_view(word);
                    with_pill(card(body, Some(footer), Some(&word.title)))
                }
                None => self.languages(None),
            },
        }
    }

    /// The pill with the offer: "Spanish text found" and its Get button.
    fn offer<'a>(&'a self, o: &'a super::Offer) -> Element<'a, Msg> {
        let get = button(button_label(&o.get), Kind::Elevated, Some(Msg::GetHeld));
        self.pill(Some(&o.found), Some(get), None)
    }

    /// The pill at the top (`.as-language-pill`): source → target (the
    /// picker), and `status`.
    fn languages<'a>(&'a self, status: Option<&'a str>) -> Element<'a, Msg> {
        self.pill(status, None, None)
    }

    /// `below`: a note under the pill; the pill keeps its place.
    fn pill<'a>(
        &'a self,
        status: Option<&'a str>,
        then: Option<Element<'a, Msg>>,
        below: Option<Element<'a, Msg>>,
    ) -> Element<'a, Msg> {
        let mut top = row![self.picker.chips(picker::CHIP_HEIGHT).map(Msg::Picker)]
            .spacing(PILL_PAD)
            .align_y(Center);
        if let Some(status) = status {
            top = top.push(note(status));
        }
        top = top.push(then);
        let mut body = column![top].spacing(picker::SPACING);
        if let Some(list) = self.picker.list(PICKER_LIST) {
            body = body.push(list.map(Msg::Picker));
        }
        let pill = surface(Material::Widget, radius::DRAWER, body).padding(PILL_PAD);
        let shown = column![opaque(pill)]
            .push(below.map(opaque))
            .spacing(STACK)
            .align_x(Center);
        container(shown).center_x(Fill).padding(PILL_TOP).into()
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
