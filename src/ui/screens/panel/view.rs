//! What the result cards show (the design's app-states cards: Recognised
//! text, Browser hand-off, Ask AI without a key), composed from kit parts
//! only, in the one result card frame (`kit::panel_frame::card`).
use std::sync::Arc;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, markdown as md, row};
use iced::{Center, Element, Fill, Rectangle, Right, Size};

use super::{Browse, LOG, Msg, Panel, State};
use crate::actions;
use crate::actions::ask::{Chat, PRESETS, Setup, Status, Turn};
use crate::ui::keys;
use crate::ui::kit::bubble::bubble;
use crate::ui::kit::button::{Kind, button, cta_label, label};
use crate::ui::kit::chip::Tone;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::markdown;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame::{self as frame, Event, Header};
use crate::ui::kit::scroll::scroll;
use crate::ui::kit::spinner;
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::{chip, field, progress};
use crate::ui::widgets::{self, Why};

/// A card's body and its footer.
type Part<'a> = (Element<'a, Msg>, Option<Element<'a, Msg>>);

/// The AI chat's title (the design's header).
const CHAT_TITLE: &str = "OpenCircle AI";
/// Between a chat's turns (`space-y-4`).
const TURN_GAP: f32 = 16.0;
/// Between a card's blocks (`.as-note` margins, the textarea's top).
const STACK: f32 = 10.0;
/// Between the actions of a row (`.as-actions` gap 8; a call to action
/// and quiet text, as the update card).
const ACTIONS_GAP: f32 = 16.0;
/// Between chips (`gap-2`).
const CHIPS_GAP: f32 = 8.0;

/// The result card for `header` at `at` on a `screen`: the kit's frame, with
/// no close button (Esc, or a click outside it). `hover`: the pointer is on
/// its resize edge.
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
    frame::card(header, None, body, footer, at, screen, hover, on)
}

/// A small quiet text button (a message's Copy, Edit, Regenerate).
fn quiet<'a>(name: &'a str, msg: Msg) -> Element<'a, Msg> {
    button(label(name), Kind::Quiet, Some(msg))
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
        let (glyph, title, (body, footer)) = match &self.state {
            State::Lens(status) => (
                actions::lens::ACTION.icon,
                actions::lens::ACTION.label,
                self.lens_view(status.as_ref()),
            ),
            State::Ask {
                setup,
                chat,
                browse,
                answers,
                include,
                ..
            } => (
                actions::ask::ACTION.icon,
                CHAT_TITLE,
                self.ask_view(setup.as_ref(), chat, browse, answers, *include),
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
        let chatting = matches!(self.state, State::Ask { .. });
        let header = Header {
            glyph,
            title,
            chip,
            docked: chatting,
            logo: chatting.then(widgets::chrome::logo_handle),
        };
        if chatting {
            // the header's end: a new chat (no close button: Esc closes)
            let new = button(label("+ New"), Kind::Elevated, Some(Msg::NewChat));
            let new = widgets::hint(new, "New chat", Why::Icon, Position::Bottom);
            return frame::card(
                header,
                Some(new),
                body,
                footer,
                at,
                screen,
                hover,
                Msg::Card,
            );
        }
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

    #[allow(clippy::too_many_arguments)]
    fn ask_view<'a>(
        &'a self,
        setup: Option<&'a Result<Arc<Setup>, String>>,
        chat: &'a Chat,
        browse: &'a Browse,
        answers: &'a [(usize, md::Content)],
        include: bool,
    ) -> Part<'a> {
        let Some(Ok(_)) = setup else {
            let line: Element<'a, Msg> = match setup {
                Some(Err(steps)) => scroll(note(steps.as_str())).height(Fill).into(),
                _ => note("Getting ready…").into(),
            };
            return (line, None);
        };

        let browser = matches!(setup, Some(Ok(s)) if s.browser());
        let streaming = chat.status == Status::Streaming;
        let mut log = column![].spacing(TURN_GAP).width(Fill);
        for (i, turn) in chat.turns.iter().enumerate() {
            let last = i + 1 == chat.turns.len();
            log = log.push(self.question(i, turn, !streaming && !browser));
            let mut answer = column![].spacing(STACK).width(Fill);
            if let Some(thought) = turn.thought {
                let line = format!("Thought for {:.1} s", thought.as_secs_f32());
                answer = answer.push(chip::logo(line, widgets::chrome::logo_handle()));
            }
            let busy = last && streaming;
            if let (false, Some((_, parsed))) = (turn.answer.is_empty(), answers.get(i)) {
                answer = answer.push(markdown::markdown(parsed.items()).map(Msg::Answer));
            } else if busy {
                answer = answer.push(match browse {
                    Browse::Downloading(done) => {
                        downloading("Downloading the text-recognition model…", *done)
                    }
                    _ if browser => note("Opening Google AI Mode…").into(),
                    _ => spinner::thinking(
                        widgets::chrome::logo_handle(),
                        self.spin.map_or(0.0, |(_, turn)| turn),
                        motion::reduced(),
                        "Thinking…",
                    ),
                });
            }
            if !turn.answer.is_empty() && !busy && !browser {
                let mut tools = row![quiet("Copy", Msg::CopyTurn(i, true))].spacing(ACTIONS_GAP);
                if last {
                    tools = tools.push(quiet("Regenerate", Msg::Regenerate));
                }
                answer = answer.push(tools);
            }
            log = log.push(answer);
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
            Status::Failed(e) if browser => log = log.push(error(Role::NOTE, e)),
            Status::Failed(e) => {
                let retry = quiet("Retry", Msg::Regenerate);
                log = log.push(column![error(Role::NOTE, e), retry].spacing(STACK));
            }
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
            log = log.push(
                column![
                    note("Circle anything on your screen or ask a question below."),
                    row(prompts).spacing(CHIPS_GAP).wrap()
                ]
                .spacing(STACK),
            );
        }
        log = match &self.note {
            Some(Ok(done)) => log.push(note(*done)),
            Some(Err(e)) => log.push(error(Role::NOTE, e)),
            None => log,
        };
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
        } else if browser {
            self.follow_up("Ask a follow-up…", streaming)
        } else {
            self.composer(include, streaming)
        };
        let privacy = match (browser, include) {
            (true, true) => {
                "When you send, your question and the text read from the selection go to \
                 Google AI Mode. The image is not uploaded unless you choose Lens."
            }
            (true, false) => "When you send, only your question goes to Google AI Mode.",
            (false, true) => "Only this selection is sent, and only to the provider you chose.",
            (false, false) => "Only your question is sent, to the provider you chose.",
        };
        let body = scroll(log).id(LOG).height(Fill);
        let footer = column![input, note(privacy)].spacing(STACK);
        (body.into(), Some(footer.into()))
    }

    /// The user's turn `i`: the selection's chip when it went along, the
    /// bubble, and Copy / Edit (`editable`: not while an answer streams).
    fn question<'a>(&'a self, i: usize, turn: &'a Turn, editable: bool) -> Element<'a, Msg> {
        let mut side = column![].spacing(STACK).align_x(Right).width(Fill);
        if turn.screen {
            side = side.push(chip::status(
                self.crop_size("Selected screen region"),
                Tone::Busy,
            ));
        }
        side = side.push(bubble(&turn.question));
        let mut tools = row![quiet("Copy", Msg::CopyTurn(i, false))].spacing(ACTIONS_GAP);
        if editable {
            tools = tools.push(quiet("Edit", Msg::EditTurn(i)));
        }
        side.push(tools).into()
    }

    /// "`what` · W×H px" (the crop, physical px).
    fn crop_size(&self, what: &str) -> String {
        format!("{what} · {}×{} px", self.crop.width, self.crop.height)
    }

    /// The chat's composer: the attached selection's chip (while Include
    /// screen is on), the field with Send (Stop while answering), and the
    /// Include screen switch.
    fn composer(&self, include: bool, streaming: bool) -> Element<'_, Msg> {
        let toggle = button(
            row![chip::switch_dot(include), label("Include screen")]
                .spacing(space::GAP)
                .align_y(Center),
            Kind::Elevated,
            Some(Msg::IncludeScreen),
        );
        let toggle = widgets::hint(
            toggle,
            "Send the selection with your question",
            Why::Icon,
            Position::Top,
        );
        let mut parts = column![].spacing(STACK);
        if include {
            let attached = self.crop_size("Screen selection attached");
            parts = parts.push(chip::status(attached, Tone::Busy));
        }
        parts
            .push(self.follow_up("Message OpenCircle AI…", streaming))
            .push(toggle)
            .into()
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
}
