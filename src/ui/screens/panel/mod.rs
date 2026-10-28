//! Result panels (DESIGN.md Flow step 5): Search (Lens) and Ask AI (the
//! AI chat: docked, its answers as Markdown). One floating card (`kit::panel_frame::card`: on the side away from the
//! selection, a card at the bottom on narrow screens; dragged and resized by
//! its frame) over the frozen frame, so the selection stays visible. Slow work
//! runs on its own thread (`background`); its results arrive as `Msg`s.
//! Dropping a panel stops its running work. What it shows: `view`.
use std::sync::Arc;
use std::time::Instant;

use iced::widget::{markdown, operation};
use iced::{Task, task};

use crate::actions::ask::{self, Chat, PRESETS, Setup, Status};
use crate::actions::text::{self, Progress};
use crate::actions::{self, Crop, lens};
use crate::ai;
use crate::ui::background;
use crate::ui::keys;
use crate::ui::kit::markdown::Act;
use crate::ui::kit::panel_frame;
use crate::ui::kit::{motion, spinner};

mod view;
pub use view::{actions, card, note};

/// The follow-up input. Never focused by itself: until it is clicked, letters
/// are the overlay's action shortcuts.
pub(in crate::ui) const INPUT: &str = "panel-input";
/// The Ask AI conversation (scrolled to the end as the answer grows).
const LOG: &str = "panel-log";

#[derive(Debug, Clone)]
pub enum Msg {
    /// The card's grip or corner was pressed (the App moves / resizes it).
    Card(panel_frame::Event),
    /// The Lens page is written and the browser launched (the App holds the page).
    LensOpened(Result<lens::Held, String>),
    Input(String),
    /// Enter / send in the follow-up input.
    Submit,
    /// A suggestion chip (index into `PRESETS`).
    Preset(usize),
    Stop,
    Ready(Result<Arc<Setup>, String>),
    Delta(String),
    Answered(Result<(), String>),
    /// The user agreed to the model download.
    Download,
    /// Browser provider: send the question without the selection's text.
    SendWithoutText,
    Progress(Progress),
    /// Search the crop with Lens (the chat's "Search the image with Lens").
    Search,
    /// What a link's click did (its note).
    Acted(Result<&'static str, String>),
    /// A focused field gave a key back (`keys`); the App handles it.
    Key(keys::Keyed),
    /// Ask AI: send the selection with the next question, or not.
    IncludeScreen,
    /// Ask AI: an empty chat (the running answer stops).
    NewChat,
    /// A click in an answer: a link, or a code block's Copy.
    Answer(Act),
    /// Copy turn `i`'s answer (`false`: its question).
    CopyTurn(usize, bool),
    /// Put turn `i`'s question back in the input; sending replaces it and
    /// the turns after it.
    EditTurn(usize),
    /// Ask the last question again (also Retry after an error).
    Regenerate,
}

pub struct Panel {
    crop: Arc<Crop>,
    state: State,
    /// The follow-up input's text.
    input: String,
    /// The running stream; dropping the handle aborts it.
    work: Option<task::Handle>,
    /// What the last link click did (or why it could not).
    note: Option<Result<&'static str, String>>,
    /// The logo spinner while the AI has not answered yet: since when, and
    /// how far through its turn at the latest frame (`tick`).
    spin: Option<(Instant, f32)>,
}

enum State {
    /// `None` while the page is being written.
    Lens(Option<Result<(), String>>),
    Ask {
        /// `None` while loading the config and key.
        setup: Option<Result<Arc<Setup>, String>>,
        chat: Chat,
        browse: Browse,
        /// Each turn's answer parsed as Markdown, and how much of it.
        answers: Vec<(usize, markdown::Content)>,
        /// The selection goes with the next question (on by default).
        include: bool,
        /// The question being edited (its turn).
        editing: Option<usize>,
    },
}

/// Browser provider (Google AI Mode): adding the selection's text to the
/// question needs the OCR model, so it asks before the first download.
#[derive(Default)]
enum Browse {
    #[default]
    Idle,
    /// No models yet: this question waits for the user's choice.
    Consent(String),
    Downloading(f32),
}

impl Panel {
    /// Opens `kind` for `crop`. `question` (Ask AI only) is sent once ready:
    /// the user already pressed Enter on it elsewhere (Lens follow-up, the
    /// bar's AI field); `include`: its Include screen starts on.
    /// Search (Lens) hands the crop to the user's browser at once: the user
    /// picked it.
    pub fn open(
        kind: actions::Panel,
        crop: Crop,
        question: Option<String>,
        include: bool,
    ) -> (Panel, Task<Msg>) {
        let crop = Arc::new(crop);
        let mut panel = Panel {
            crop: crop.clone(),
            state: State::Lens(None),
            input: question.unwrap_or_default(),
            work: None,
            note: None,
            spin: None,
        };
        let task = match kind {
            actions::Panel::Lens => background(move |tx| {
                let _ = tx.unbounded_send(Msg::LensOpened(lens::open(&crop)));
            }),
            actions::Panel::Ask => {
                panel.state = State::ask(Chat::default());
                if let State::Ask { include: on, .. } = &mut panel.state {
                    *on = include;
                }
                // No key (Google AI Mode): nothing to load (no keyring, no PNG), so
                // the question goes at once, never "Getting ready…" before it.
                if crate::config::current().ai.provider == crate::config::Provider::Browser {
                    let ready = ask::setup(&crop).map(Arc::new);
                    let task = panel.update(Msg::Ready(ready));
                    return (panel, task);
                }
                background(move |tx| {
                    let _ = tx.unbounded_send(Msg::Ready(ask::setup(&crop).map(Arc::new)));
                })
            }
        };
        (panel, task)
    }

    /// Ask AI, continuing `chat` (e.g. the translate word card's explanation).
    pub fn open_chat(crop: Crop, mut chat: Chat, include: bool) -> (Panel, Task<Msg>) {
        chat.cancel(); // a stream still running belonged to the card; it stopped
        let (mut panel, task) = Panel::open(actions::Panel::Ask, crop, None, include);
        if let State::Ask { chat: open, .. } = &mut panel.state {
            *open = chat;
        }
        panel.sync();
        (panel, task)
    }

    /// Ask AI's Include screen (`None`: not Ask AI).
    pub fn include(&self) -> Option<bool> {
        match self.state {
            State::Ask { include, .. } => Some(include),
            State::Lens(_) => None,
        }
    }

    pub fn kind(&self) -> actions::Panel {
        match self.state {
            State::Lens(_) => actions::Panel::Lens,
            State::Ask { .. } => actions::Panel::Ask,
        }
    }

    /// Asked, and the AI has not written anything yet (thinking, or waiting
    /// for its first words): the logo spinner shows.
    pub fn waiting(&self) -> bool {
        matches!(&self.state, State::Ask { chat, setup: Some(Ok(setup)), .. }
            if !setup.browser()
                && chat.status == Status::Streaming
                && chat.turns.last().is_some_and(|t| t.answer.is_empty()))
    }

    /// The spinner turns (frames wanted): waiting, and motion not reduced.
    pub fn spinning(&self) -> bool {
        self.waiting() && !motion::reduced()
    }

    /// A frame at `now`: the spinner's turn (one per `spinner::TURN`,
    /// steady), or none once the answer streams.
    pub fn tick(&mut self, now: Instant) {
        self.spin = match (self.spinning(), self.spin) {
            (false, _) => None,
            (true, None) => Some((now, 0.0)),
            (true, Some((start, _))) => {
                Some((start, spinner::turn(now.saturating_duration_since(start))))
            }
        };
    }

    /// Empties the follow-up input and returns what was in it.
    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    /// Messages for the panel itself (the App handles Search
    /// and the Lens follow-up first).
    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.on(msg);
        self.sync();
        task
    }

    /// The answers' Markdown follows the chat: new text parsed on, a turn
    /// taken back or replaced parsed anew.
    fn sync(&mut self) {
        let State::Ask { chat, answers, .. } = &mut self.state else {
            return;
        };
        answers.truncate(chat.turns.len());
        for (i, turn) in chat.turns.iter().enumerate() {
            if answers.len() == i {
                answers.push((0, markdown::Content::new()));
            }
            let (parsed, md) = &mut answers[i];
            if turn.answer.len() < *parsed {
                (*parsed, *md) = (0, markdown::Content::new());
            }
            md.push_str(&turn.answer[*parsed..]);
            *parsed = turn.answer.len();
        }
    }

    fn on(&mut self, msg: Msg) -> Task<Msg> {
        match (&mut self.state, msg) {
            (State::Lens(status), Msg::LensOpened(result)) => *status = Some(result.map(drop)),
            (_, Msg::Input(value)) => self.input = value,
            // Not ready or still answering: the question stays in the input.
            (
                State::Ask {
                    chat,
                    setup: Some(Ok(_)),
                    browse: Browse::Idle,
                    ..
                },
                Msg::Submit,
            ) if chat.status != Status::Streaming => {
                let question = self.take_input();
                if let State::Ask { chat, editing, .. } = &mut self.state
                    && let Some(i) = editing.take()
                    && !question.trim().is_empty()
                {
                    chat.take_back(i);
                }
                return self.send(&question);
            }
            (State::Ask { include, .. }, Msg::IncludeScreen) => *include = !*include,
            (
                State::Ask {
                    chat,
                    browse,
                    editing,
                    ..
                },
                Msg::NewChat,
            ) => {
                (*chat, *browse, *editing) = (Chat::default(), Browse::Idle, None);
                self.work = None;
                self.input.clear();
                self.note = None;
            }
            (State::Ask { .. }, Msg::Answer(act)) => {
                return match act {
                    // Only on a click, and only a web link (`open_link`).
                    Act::Link(url) => background(move |tx| {
                        let done = crate::sys::open::open_link(&url)
                            .map(|()| "Opened in your browser.")
                            .map_err(|e| format!("Could not open the link: {e}"));
                        let _ = tx.unbounded_send(Msg::Acted(done));
                    }),
                    Act::Copy(code) => copy(code),
                };
            }
            (State::Ask { chat, .. }, Msg::CopyTurn(i, answer)) => {
                if let Some(turn) = chat.turns.get(i) {
                    return copy(match answer {
                        true => turn.answer.clone(),
                        false => turn.question.clone(),
                    });
                }
            }
            (State::Ask { chat, editing, .. }, Msg::EditTurn(i))
                if chat.status != Status::Streaming =>
            {
                if let Some(turn) = chat.turns.get(i) {
                    self.input = turn.question.clone();
                    *editing = Some(i);
                    return operation::focus(INPUT);
                }
            }
            (State::Ask { chat, .. }, Msg::Regenerate) => {
                if let Some(question) = chat
                    .turns
                    .len()
                    .checked_sub(1)
                    .and_then(|i| chat.take_back(i))
                {
                    return self.send(&question);
                }
            }
            (State::Ask { .. }, Msg::Preset(i)) => {
                if let Some(preset) = PRESETS.get(i) {
                    return self.send(&preset.text(&crate::config::current().translate.target));
                }
            }
            (
                State::Ask {
                    browse: Browse::Consent(_),
                    ..
                },
                Msg::Download,
            ) => return self.resume(true),
            (
                State::Ask {
                    browse: Browse::Consent(_),
                    ..
                },
                Msg::SendWithoutText,
            ) => return self.resume(false),
            (
                State::Ask {
                    browse: browse @ Browse::Downloading(_),
                    ..
                },
                Msg::Progress(Progress::Download(done)),
            ) => *browse = Browse::Downloading(done),
            (State::Ask { browse, .. }, Msg::Progress(Progress::Reading)) => *browse = Browse::Idle,
            (State::Ask { chat, browse, .. }, Msg::Stop) => {
                chat.cancel();
                *browse = Browse::Idle;
                self.work = None;
            }
            (State::Ask { setup, .. }, Msg::Ready(result)) => {
                let ready = result.is_ok();
                *setup = Some(result);
                if ready && !self.input.trim().is_empty() {
                    let question = self.take_input();
                    return self.send(&question);
                }
            }
            (State::Ask { chat, .. }, Msg::Delta(piece)) => {
                chat.delta(&piece);
                // Follow the answer. (anchor_bottom() hid short content in iced 0.14.)
                return operation::snap_to_end(LOG);
            }
            (State::Ask { chat, browse, .. }, Msg::Answered(result)) => {
                chat.finish(result);
                *browse = Browse::Idle;
                self.work = None;
            }
            (_, Msg::Acted(note)) => self.note = Some(note),
            _ => {}
        }
        Task::none()
    }

    /// Asks `question`; the answer streams in on a thread. Nothing is sent before this.
    fn send(&mut self, question: &str) -> Task<Msg> {
        let State::Ask {
            setup: Some(Ok(setup)),
            chat,
            browse,
            include,
            ..
        } = &mut self.state
        else {
            return Task::none();
        };
        if setup.browser() {
            let question = question.trim();
            if question.is_empty() || chat.status == Status::Streaming {
                return Task::none();
            }
            if !*include {
                return self.browse(question.into(), false); // the question only
            }
            if !text::models_present() {
                *browse = Browse::Consent(question.into());
                return Task::none();
            }
            return self.browse(question.into(), true);
        }
        // Include screen off: the question goes without the picture.
        let png: &[u8] = if *include { &setup.png } else { &[] };
        let Some(messages) = chat.ask(question, png) else {
            return Task::none();
        };
        let setup = setup.clone();
        let (task, handle) = background(move |tx| {
            let result = ask::stream(&setup, &messages, |piece| {
                tx.unbounded_send(Msg::Delta(piece.into())).is_ok()
            });
            let _ = tx.unbounded_send(Msg::Answered(result));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }

    /// The user chose about the text model (Browser provider): go on with the waiting question.
    fn resume(&mut self, read_text: bool) -> Task<Msg> {
        let State::Ask { browse, .. } = &mut self.state else {
            return Task::none();
        };
        let Browse::Consent(question) = std::mem::take(browse) else {
            return Task::none();
        };
        if read_text {
            *browse = Browse::Downloading(0.0);
        }
        self.browse(question, read_text)
    }

    /// Browser provider: read the selection's text (`read_text`; the user agreed
    /// to the model), then open `prompt` + that text in Google AI Mode.
    fn browse(&mut self, prompt: String, read_text: bool) -> Task<Msg> {
        let State::Ask {
            setup: Some(Ok(_)),
            chat,
            ..
        } = &mut self.state
        else {
            return Task::none();
        };
        chat.ask(&prompt, &[]); // the turn (and "busy"); nothing is sent by this
        let crop = self.crop.clone();
        let (task, handle) = background(move |tx| {
            let read = read_text.then(|| {
                text::read(&crop, |progress| {
                    let _ = tx.unbounded_send(Msg::Progress(progress));
                })
            });
            let (question, note) = browser_turn(&prompt, read);
            // The user pressed Send: AI Mode opens in their own browser.
            let line = match ai::browser::ask(&question) {
                Ok(opened) => {
                    let url = ai::browser::url(&question).0;
                    crate::history::search::keep("ai", &prompt, &url, Some(&crop));
                    format!("{opened}{note}")
                }
                Err(e) => e,
            };
            let _ = tx.unbounded_send(Msg::Delta(line));
            let _ = tx.unbounded_send(Msg::Answered(Ok(())));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }
}

impl State {
    /// A new Ask AI chat (`chat`), getting ready.
    fn ask(chat: Chat) -> State {
        State::Ask {
            setup: None,
            chat,
            browse: Browse::Idle,
            answers: Vec::new(),
            include: true,
            editing: None,
        }
    }
}

/// Puts `text` on the clipboard (on a thread); its note says so.
fn copy(text: String) -> Task<Msg> {
    background(move |tx| {
        let done = actions::copy::text(text)
            .map(|()| "Copied.")
            .map_err(|e| format!("Could not copy: {e}"));
        let _ = tx.unbounded_send(Msg::Acted(done));
    })
}

/// Browser provider: the question to open in AI Mode and the line to add to the
/// answer. `read` is the OCR result (`None` = the user declined to read text).
fn browser_turn(prompt: &str, read: Option<Result<String, String>>) -> (String, String) {
    let alone = |why: String| {
        (
            ai::browser::question(prompt, None),
            format!("\n{why} Only your question was sent."),
        )
    };
    match read {
        Some(Ok(text)) if !text.trim().is_empty() => (
            ai::browser::question(prompt, Some(&text)),
            "\nSent with the text read from your selection.".into(),
        ),
        Some(Ok(_)) => alone("No text was found in the selection.".into()),
        Some(Err(e)) => alone(format!("Could not read the text ({e}).")),
        None => alone("The image itself cannot be passed to AI Mode.".into()),
    }
}

#[cfg(test)]
mod tests;
