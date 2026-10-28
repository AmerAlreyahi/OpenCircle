//! What the result cards show (the design's app-states cards: Recognised
//! text, Browser hand-off, Ask AI without a key), composed from kit parts
//! only, in the one result card frame (`kit::panel_frame::card`).
use std::sync::Arc;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, markdown as md, row, stack};
use iced::{Center, Element, Fill, Rectangle, Right, Size};

use super::{Browse, LOG, Msg, PICKS, Panel, State, Tab, classifier, picked};
use crate::actions;
use crate::actions::ask::{Chat, PRESETS, Setup, Status, Turn};
use crate::config::{self, StaysOpen};
use crate::ui::keys;
use crate::ui::kit::bubble::bubble;
use crate::ui::kit::button::{Kind, button, cta_label, label};
use crate::ui::kit::chip::Tone;
use crate::ui::kit::dropdown::dropdown;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::markdown;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame::{self as frame, Event, Header};
use crate::ui::kit::scroll::scroll;
use crate::ui::kit::segmented::{self, segmented};
use crate::ui::kit::spinner;
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::text_area::text_area;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::tooltip::{Why, hint};
use crate::ui::kit::{chip, field, progress};

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
            logo: chatting.then(|| crate::ui::kit::logo::logo(16)),
            menu: chatting,
            held: self.held,
        };
        if chatting {
            // the header's end: a new chat and the pin (no close button: Esc
            // or a click outside closes an unpinned chat; a pinned one, its pin)
            let new = button(label("+ New"), Kind::Elevated, Some(Msg::NewChat));
            let new = hint(new, "New chat", Why::Icon, Position::Bottom);
            let state = icon_button::State {
                active: self.pinned,
                ..Default::default()
            };
            // pinned: "pin-off" on the filled (active) button, so it reads at a glance
            let (glyph, tip) = match self.pinned {
                true => (Icon::Unpin, "Unpin and close"),
                false => (Icon::Pin, "Pin chat"),
            };
            let pin = icon_button(glyph.glyph(), state, Some(Msg::Pin));
            let pin = hint(pin, tip, Why::Icon, Position::Bottom);
            let ends = row![new, pin].spacing(space::GAP).align_y(Center);
            let card = frame::card(
                header,
                Some(ends.into()),
                body,
                footer,
                at,
                screen,
                hover,
                Msg::Card,
            );
            return match self.drawer {
                Some(tab) => {
                    let inside = self.drawer_view(tab, frame::drawer_room(at));
                    stack![card, frame::drawer(inside, at)]
                        .width(Fill)
                        .height(Fill)
                        .into()
                }
                None => card,
            };
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
        (
            body.into(),
            Some(self.follow_up("Ask AI about it…", false, None, None)),
        )
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
        let Some(Ok(ready)) = setup else {
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
                answer = answer.push(chip::logo(line, crate::ui::kit::logo::logo(14)));
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
                        crate::ui::kit::logo::logo(16),
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
        } else {
            let placeholder = placeholder(chat);
            match browser {
                true => self.follow_up(placeholder, streaming, None, None),
                false => {
                    let attached = include && chat.takes(&ready.png);
                    self.composer(placeholder, include, attached, streaming)
                }
            }
        };
        let body = scroll(log).id(LOG).height(Fill);
        (body.into(), Some(input))
    }

    /// The user's turn `i`: the selection's chip when it went along, the
    /// bubble, and Copy / Edit (`editable`: not while an answer streams).
    fn question<'a>(&'a self, i: usize, turn: &'a Turn, editable: bool) -> Element<'a, Msg> {
        let mut side = column![].spacing(STACK).align_x(Right).width(Fill);
        if turn.screen {
            side = side.push(chip::status(
                match turn.shot {
                    Some((w, h)) => format!("Selected screen region · {w}×{h} px"),
                    None => "Screenshot not kept".to_string(),
                },
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

    /// The chat's composer (the design's, solid): the attached screen's chip
    /// while a picture goes with the next message (`attached`), the field
    /// (more lines with Shift / Ctrl+Enter), then Include screen and Send
    /// (Stop while answering). Web search's Auto / On / Off joins it in 0.2.0.
    fn composer<'a>(
        &'a self,
        placeholder: &'a str,
        include: bool,
        attached: bool,
        streaming: bool,
    ) -> Element<'a, Msg> {
        let size = format!("{}×{} px", self.crop.width, self.crop.height);
        let top = attached.then(|| chip::attached("UI", "Screen selection attached", size));
        let switch = crate::ui::kit::button::switch("Include screen", include, Msg::IncludeScreen);
        self.follow_up(placeholder, streaming, top, Some(switch))
    }

    /// The composer box: `top` over the field, `extra` at the start of its
    /// controls row (the Include screen switch) and its send (or, while
    /// streaming, stop) button at the end.
    fn follow_up<'a>(
        &'a self,
        placeholder: &'a str,
        streaming: bool,
        top: Option<Element<'a, Msg>>,
        extra: Option<Element<'a, Msg>>,
    ) -> Element<'a, Msg> {
        use crate::ui::kit::button::{COMPACT_H, compact};
        let round =
            |icon: Icon, msg| compact(icon.glyph(), None, COMPACT_H, (false, false), Some(msg));
        let send = match streaming {
            true => hint(
                round(Icon::Stop, Msg::Stop),
                "Stop",
                Why::Icon,
                Position::Top,
            ),
            false => hint(
                round(Icon::Send, Msg::Submit),
                "Send",
                Why::Keys("Enter"),
                Position::Top,
            ),
        };
        let controls = row![]
            .push(extra)
            .push(iced::widget::space::horizontal())
            .push(send)
            .spacing(space::GAP)
            .align_y(Center);
        let composer = text_area(
            super::INPUT,
            placeholder,
            &self.input,
            Msg::Edit,
            Msg::Submit,
            top,
            controls.into(),
        );
        keys::guard(composer, Msg::Key)
    }

    /// The drawer (the design's "Workspace"), `width` inside: back, its title
    /// and the tabs, then the tab.
    fn drawer_view(&self, tab: Tab, width: f32) -> Element<'_, Msg> {
        let back = icon_button(
            Icon::Back.glyph(),
            icon_button::State::default(),
            Some(Msg::Drawer(None)),
        );
        let back = hint(back, "Back to the chat", Why::Icon, Position::Bottom);
        let tabs = segmented::inline(
            &[(Tab::History, "History"), (Tab::Settings, "Settings")],
            tab,
            tab,
            1.0,
            |t| Msg::Drawer(Some(t)),
        );
        let top = row![
            back,
            container(crate::ui::kit::text::text(Role::CARD_HEADING, "Workspace")).width(Fill),
            tabs
        ]
        .spacing(space::GAP)
        .align_y(Center);
        let parts = match tab {
            Tab::History => self.history_view(width),
            Tab::Settings => vec![scroll(self.settings_view()).height(Fill).into()],
        };
        column![top].extend(parts).spacing(STACK).into()
    }

    /// This PC's saved chats, favourites then newest first: the search field
    /// and a new chat, a card per chat (star, open, delete), and the footer.
    fn history_view(&self, width: f32) -> Vec<Element<'_, Msg>> {
        let search = field::input("Search chats…", &self.find, false, false)
            .on_input(Msg::Find)
            .width(Fill);
        let new = button(label("+ New"), Kind::Elevated, Some(Msg::NewChat));
        let top = row![search, new].spacing(space::GAP).align_y(Center);
        if !config::current().chat.keep_history {
            let off = note("Chat history is off (Settings > AI). Nothing is saved.");
            return vec![top.into(), off.into()];
        }
        let open = match &self.state {
            State::Ask { chat, .. } => chat.id.clone(),
            State::Lens(_) => None,
        };
        let now = crate::store::chats::now();
        let mut cards = column![].spacing(space::GAP).width(Fill);
        for entry in &self.listed {
            cards = cards.push(crate::ui::kit::card::chat(crate::ui::kit::card::Chat {
                title: &entry.title,
                when: ago(entry.updated, now),
                crop: entry.crop.map(|(w, h)| format!("{w}×{h} px")),
                starred: entry.star,
                selected: open.as_deref() == Some(entry.id.as_str()),
                width,
                open: Msg::OpenSaved(entry.id.clone()),
                star: Msg::Star(entry.id.clone(), !entry.star),
                delete: Msg::DeleteSaved(entry.id.clone()),
            }));
        }
        let list: Element<'_, Msg> = match self.listed.is_empty() {
            true if self.find.trim().is_empty() => note("No saved chats yet.").into(),
            true => container(note("No matching conversations"))
                .center_x(Fill)
                .into(),
            false => scroll(cards).height(Fill).into(),
        };
        let clear = button(label("Clear all"), Kind::Remove, Some(Msg::ClearHistory));
        let footer = row![
            container(crate::ui::kit::text::muted(
                Role::CARD_DESC,
                "Local on this PC"
            ))
            .width(Fill),
            clear
        ]
        .align_y(Center);
        let list = container(list).height(Fill);
        vec![top.into(), list.into(), footer.into()]
    }

    /// "AI chat stays open", the quick provider / model picker, and the way to
    /// the full AI settings.
    fn settings_view(&self) -> Element<'_, Msg> {
        let cfg = config::current();
        let stays = segmented(
            &[
                (StaysOpen::FollowUp, "After a follow-up"),
                (StaysOpen::Always, "Always"),
            ],
            cfg.chat.stays_open,
            cfg.chat.stays_open,
            1.0,
            Msg::StaysOpen,
        );
        let provider = dropdown(&PICKS[..], picked(&cfg.ai), "Choose a provider", Msg::Pick);
        let mut part = column![
            sub(Role::LABEL, "AI chat stays open"),
            stays,
            sub(Role::LABEL, "Provider"),
            provider,
        ]
        .spacing(STACK)
        .width(Fill);
        if cfg.ai.provider != config::Provider::Browser {
            let model = field::input("Model id", &self.model, true, false)
                .on_input(Msg::Model)
                .on_submit(Msg::SaveModel);
            part = part.push(sub(Role::LABEL, "Model")).push(model);
            if classifier(&self.model) {
                part = part.push(error(
                    Role::NOTE,
                    "This looks like a safety classifier: it rates messages instead of \
                     answering them. Pick a chat model.",
                ));
            }
        }
        part.push(crate::ui::kit::button::fill(
            label("Open AI & model settings →"),
            Some(Msg::OpenAiSettings),
        ))
        .into()
    }
}

/// The composer's placeholder: a first message, then a follow-up.
pub(super) fn placeholder(chat: &Chat) -> &'static str {
    match chat.turns.is_empty() {
        true => "Message OpenCircle AI…",
        false => "Ask a follow-up…",
    }
}

/// When a saved chat last changed, as History says it (`now`: unix seconds):
/// "Just now", "18m ago", "3h ago", "Yesterday", then its date.
pub(super) fn ago(then: u64, now: u64) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    match now.saturating_sub(then) {
        s if s < MIN => "Just now".into(),
        s if s < HOUR => format!("{}m ago", s / MIN),
        s if s < DAY => format!("{}h ago", s / HOUR),
        s if s < 2 * DAY => "Yesterday".into(),
        _ => crate::log::date(then),
    }
}
