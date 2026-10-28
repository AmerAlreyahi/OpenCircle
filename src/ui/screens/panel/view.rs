//! What the result cards show (the design's app-states cards: Recognised
//! text, Browser hand-off, Ask AI without a key), composed from kit parts
//! only, in the one result card frame (`kit::panel_frame::card`).
use std::sync::Arc;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, image, row};
use iced::{Center, ContentFit, Element, Fill, Rectangle, Size};

use super::{Browse, LOG, Msg, Panel, Reading, State};
use crate::actions;
use crate::actions::ask::{Chat, PRESETS, Setup, Status};
use crate::ui::keys;
use crate::ui::kit::bubble::bubble;
use crate::ui::kit::button::{Kind, button, cta_label, label};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::panel_frame::{self as frame, Event, Header};
use crate::ui::kit::scroll::scroll;
use crate::ui::kit::text::{error, sub, text};
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::{chip, field, progress};
use crate::ui::screens::model_download::{self, On, Stage};
use crate::ui::widgets::{self, Why};

/// A card's body and its footer.
type Part<'a> = (Element<'a, Msg>, Option<Element<'a, Msg>>);

/// The crop's thumbnail in the Ask AI card.
const THUMB_HEIGHT: f32 = 112.0;
/// Between a card's blocks (`.as-note` margins, the textarea's top).
const STACK: f32 = 10.0;
/// Between the actions of a row (`.as-actions` gap 8; a call to action
/// and quiet text, as the update card).
const ACTIONS_GAP: f32 = 16.0;
/// Between chips (`gap-2`).
const CHIPS_GAP: f32 = 8.0;

/// The result card for `header` at `at` on a `screen`: the kit's frame with
/// its close button (Esc). `hover`: the pointer is on its resize edge.
#[allow(clippy::too_many_arguments)]
pub fn card<'a, M: Clone + 'a>(
    header: Header<'a>,
    body: impl Into<Element<'a, M>>,
    footer: Option<Element<'a, M>>,
    at: Rectangle,
    screen: Size,
    hover: bool,
    on: impl Fn(Event) -> M,
) -> Element<'a, M> {
    let close = icon_button(
        Icon::Close.glyph(),
        icon_button::State::default(),
        Some(on(Event::Close)),
    );
    let close = widgets::hint(close, "Close", Why::Keys("Esc"), Position::Bottom);
    frame::card(header, close, body, footer, at, screen, hover, on)
}

/// A card's note (`.as-note`): 12 px secondary text.
pub fn note<'a>(line: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    sub(Role::NOTE, line).line_height(1.65)
}

/// The call to action and the other choices of a card (`.as-actions`): the
/// first fills the row in the accent, the rest are quiet text.
pub fn actions<'a, M: Clone + 'a>(
    main: (&'a str, M),
    others: impl IntoIterator<Item = (&'a str, M)>,
) -> Element<'a, M> {
    let mut line = row![button(cta_label(main.0, None), Kind::Accent, Some(main.1))]
        .spacing(ACTIONS_GAP)
        .align_y(Center);
    for (name, msg) in others {
        line = line.push(button(label(name), Kind::Quiet, Some(msg)));
    }
    line.into()
}

/// The OCR download card (`model_download`) for the Text card's download
/// question, progress, done and failure; `None` once it reads (or failed to
/// read with the models here).
fn download_card(reading: &Reading) -> Option<Element<'_, Msg>> {
    let stage = match reading {
        Reading::Consent => Stage::Ask,
        Reading::Downloading(done) => Stage::at(*done),
        Reading::Installed => Stage::Done,
        Reading::Failed(e) if !crate::actions::text::models_present() => Stage::Failed(e),
        _ => return None,
    };
    let on = On {
        download: Msg::Download,
        dismiss: Some(Msg::Cancel),
        provider: None,
    };
    let bytes = crate::ocr::models::full_size();
    Some(model_download::card(&model_download::OCR, bytes, stage, on))
}

/// A download's line, its bar and its share (`.as-progress-label` +
/// `.as-progress`): the Ask card's OCR download for its question.
fn downloading<'a, M: 'a>(what: &'a str, done: f32) -> Element<'a, M> {
    let label = row![
        container(note(what)).width(Fill),
        note(format!("{:.0} %", done * 100.0))
    ]
    .spacing(STACK);
    column![label, progress::bar(done)].into()
}

impl Panel {
    /// The panel as the floating result card at `at` on a `screen`;
    /// `hover`: the pointer is on its resize edge.
    pub fn view(&self, at: Rectangle, screen: Size, hover: bool) -> Element<'_, Msg> {
        if let State::Text(reading) = &self.state
            && let Some(card) = download_card(reading)
        {
            return model_download::placed(card, at, screen);
        }
        let (glyph, title, (body, footer)) = match &self.state {
            State::Lens(status) => (
                actions::lens::ACTION.icon,
                actions::lens::ACTION.label,
                self.lens_view(status.as_ref()),
            ),
            State::Text(reading) => (
                actions::text::ACTION.icon,
                "Text from your selection",
                self.text_view(reading),
            ),
            State::Ask {
                thumb,
                setup,
                chat,
                browse,
            } => (
                actions::ask::ACTION.icon,
                actions::ask::ACTION.label,
                self.ask_view(thumb, setup.as_ref(), chat, browse),
            ),
        };
        let chip = match &self.state {
            State::Ask {
                setup: Some(Ok(setup)),
                ..
            } if setup.browser() => Some("Google AI Mode"),
            State::Ask {
                setup: Some(Ok(setup)),
                ..
            } => Some(setup.cfg.model.as_str()),
            _ => None,
        };
        let header = Header { glyph, title, chip };
        // the Text card's download question, progress and error are short
        // cards, as tall as their text and buttons, not a slab (#127)
        let at = match &self.state {
            State::Text(Reading::Failed(e)) => frame::short(at, screen, e.chars().count(), 0.0),
            // thumbnail, question bubble, buttons and the privacy line under the text
            State::Ask {
                browse: Browse::Consent(q),
                ..
            } => {
                let chars = 165 + q.chars().count();
                frame::short(at, screen, chars, THUMB_HEIGHT + 190.0)
            }
            _ => at,
        };
        card(header, body, footer, at, screen, hover, Msg::Card)
    }

    /// Body and footer.
    fn lens_view<'a>(&'a self, status: Option<&'a Result<(), String>>) -> Part<'a> {
        let only = note("Only this selection goes to Google Lens.");
        let body = match status {
            None => column![note("Opening Google Lens in your browser…"), only],
            Some(Ok(())) => column![note("Opened in your browser"), only],
            // nothing went (e.g. a blank selection): no note about what goes
            Some(Err(e)) => column![error(Role::NOTE, e)],
        }
        .spacing(STACK);
        (body.into(), Some(self.follow_up("Ask AI about it…", false)))
    }

    fn ask_view<'a>(
        &'a self,
        thumb: &'a image::Handle,
        setup: Option<&'a Result<Arc<Setup>, String>>,
        chat: &'a Chat,
        browse: &'a Browse,
    ) -> Part<'a> {
        let thumb = container(
            image(thumb.clone())
                .height(THUMB_HEIGHT)
                .content_fit(ContentFit::Contain),
        )
        .align_right(Fill);
        let Some(Ok(_)) = setup else {
            let line: Element<'a, Msg> = match setup {
                Some(Err(steps)) => scroll(note(steps.as_str())).height(Fill).into(),
                _ => note("Getting ready…").into(),
            };
            return (column![thumb, line].spacing(STACK).into(), None);
        };

        let browser = matches!(setup, Some(Ok(s)) if s.browser());
        let mut log = column![].spacing(STACK).width(Fill);
        for (i, turn) in chat.turns.iter().enumerate() {
            log = log.push(bubble(&turn.question));
            let last = i + 1 == chat.turns.len();
            if !turn.answer.is_empty() {
                log = log.push(text(Role::CHAT, &turn.answer));
            } else if last && chat.status == Status::Streaming {
                log = log.push(match browse {
                    Browse::Downloading(done) => {
                        downloading("Downloading the text-recognition model…", *done)
                    }
                    _ if browser => note("Opening Google AI Mode…").into(),
                    _ => note("Thinking…").into(),
                });
            }
        }
        // Questions about the picture itself: Lens takes the crop (browser provider).
        if browser && chat.status == Status::Done {
            let lens = button(
                label("Search the image with Lens"),
                Kind::Quiet,
                Some(Msg::Search),
            );
            log = log.push(lens);
        }
        match &chat.status {
            Status::Failed(e) => log = log.push(error(Role::NOTE, e)),
            Status::Cancelled => log = log.push(note("Stopped.")),
            _ => {}
        }
        if let Browse::Consent(question) = browse {
            // Waiting for the user's choice: the question is theirs, not yet a turn.
            log = log.push(bubble(question));
        } else if chat.turns.is_empty() {
            let prompts = PRESETS
                .iter()
                .enumerate()
                .map(|(i, p)| chip::action(p.label.to_string(), Msg::Preset(i)));
            log = log.push(row(prompts).spacing(CHIPS_GAP).wrap());
        }
        let input: Element<'a, Msg> = if matches!(browse, Browse::Consent(_)) {
            column![
                note(
                    "To add the text of your selection to the question, OpenCircle downloads \
                     the text-recognition model once (about 20 MB, from GitHub). \
                     Nothing else is sent."
                ),
                actions(
                    ("Download", Msg::Download),
                    [("Send without the text", Msg::SendWithoutText)]
                ),
            ]
            .spacing(STACK)
            .into()
        } else {
            self.follow_up("Ask a follow-up…", chat.status == Status::Streaming)
        };
        let privacy = if browser {
            "When you send, your question and the text read from the selection go to Google \
             AI Mode. The image is not uploaded unless you choose Lens."
        } else {
            "Only this selection is sent, and only to the provider you chose."
        };
        let body = scroll(column![thumb, log].spacing(STACK))
            .id(LOG)
            .height(Fill);
        let footer = column![input, note(privacy)].spacing(STACK);
        (body.into(), Some(footer.into()))
    }

    /// The field with its send (or, while streaming, stop) button.
    fn follow_up(&self, placeholder: &str, streaming: bool) -> Element<'_, Msg> {
        let top = Position::Top;
        let button = match streaming {
            true => widgets::hint(
                icon_button(
                    Icon::Stop.glyph(),
                    icon_button::State::default(),
                    Some(Msg::Stop),
                ),
                "Stop",
                Why::Icon,
                top,
            ),
            false => widgets::hint(
                icon_button(
                    Icon::Send.glyph(),
                    icon_button::State::default(),
                    Some(Msg::Submit),
                ),
                "Send",
                Why::Keys("Enter"),
                top,
            ),
        };
        let input = field::input(placeholder, &self.input, false, false)
            .id(super::INPUT)
            .on_input(Msg::Input)
            .on_submit(Msg::Submit)
            .width(Fill);
        row![keys::guard(input, Msg::Key), button]
            .spacing(space::GAP)
            .align_y(Center)
            .into()
    }

    /// The Text card (`Recognised text`): the note, the text to edit, then
    /// Copy, Search and Translate.
    fn text_view<'a>(&'a self, reading: &'a Reading) -> Part<'a> {
        let body = match reading {
            // (the download card shows these: `download_card`)
            Reading::Consent | Reading::Downloading(_) | Reading::Installed | Reading::Busy => {
                note("Reading the text…").into()
            }
            Reading::Done(content) => {
                let edit = keys::guard(field::editor(content, Msg::Edit), Msg::Key);
                let mut body =
                    column![note("Review or edit before using this text"), edit].spacing(STACK);
                body = match &self.translation {
                    _ if self.work.is_some() => body.push(note("Translating…")),
                    Some(Ok(text)) => {
                        body.push(scroll(sub(Role::CHAT, text.as_str()).width(Fill)).height(Fill))
                    }
                    Some(Err(e)) => body.push(error(Role::NOTE, e)),
                    None => body,
                };
                if !self.entities.is_empty() {
                    let chips = (self.entities.iter().enumerate())
                        .map(|(i, e)| chip::action(e.chip(), Msg::Chip(i)));
                    body = body.push(row(chips).spacing(CHIPS_GAP).wrap());
                }
                body = match &self.note {
                    Some(Ok(done)) => body.push(note(*done)),
                    Some(Err(e)) => body.push(error(Role::NOTE, e)),
                    None => body,
                };
                let buttons = actions(
                    ("Copy", Msg::CopyText),
                    [
                        ("Search", Msg::Search),
                        (actions::translate::ACTION.label, Msg::Translate),
                    ],
                );
                return (body.into(), Some(buttons));
            }
            Reading::Failed(e) => error(Role::NOTE, e).into(),
        };
        (body, None)
    }
}
