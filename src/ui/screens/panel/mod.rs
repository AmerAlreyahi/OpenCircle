//! Result panels (DESIGN.md Flow step 5): Search (Lens) and Ask AI (the
//! AI chat: docked, its answers as Markdown). One floating card (`kit::panel_frame::card`: on the side away from the
//! selection, a card at the bottom on narrow screens; dragged and resized by
//! its frame) over the frozen frame, so the selection stays visible. Slow work
//! runs on its own thread (`background`); its results arrive as `Msg`s.
//! Dropping a panel stops its running work. What it shows: `view`.
//!
//! The AI chat pins itself after a follow-up (or at once: `chat.stays_open`),
//! or by its pin button: the App then keeps it across overlays (`App::chat`).
//! Each answered chat is saved, text only (`store::chats`), while Keep chat
//! history is on; its drawer (`Tab`) holds History and the chat's settings.
use std::sync::Arc;
use std::time::Instant;

use iced::widget::{markdown, operation, text_editor};
use iced::{Task, task};

use crate::actions::ask::{self, Chat, PRESETS, Setup, Status};
use crate::actions::text::{self, Progress};
use crate::actions::{self, Crop, lens};
use crate::ai;
use crate::ai::providers::{self, Preset};
use crate::config::{self, Provider, StaysOpen};
use crate::store::{self, chats};
use crate::ui::background;
use crate::ui::keys;
use crate::ui::kit::markdown::Act;
use crate::ui::kit::panel_frame::{self, Event};
use crate::ui::kit::{motion, spinner};

mod view;
pub use view::{actions, card, note};

/// The follow-up input. Never focused by itself: until it is clicked, letters
/// are the overlay's action shortcuts.
pub(in crate::ui) const INPUT: &str = "panel-input";
/// The Ask AI conversation (scrolled to the end as the answer grows).
const LOG: &str = "panel-log";

/// The chat drawer's tabs (the design's "Workspace").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    History,
    Settings,
}

/// A provider the drawer's quick picker offers: Settings' own list
/// (`ai::providers::PRESETS`, by real name), after the browser. Its key is that
/// provider's own keyring entry (`providers::key_of`, read by the reload).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick(Option<Preset>);

impl std::fmt::Display for Pick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(p) => p.fmt(f),
            None => f.write_str("Browser (Google AI Mode)"),
        }
    }
}

/// The picker's choices: the browser, then every provider but Custom server
/// (its URL is typed in Settings; a Custom one shows as picked all the same).
pub(super) static PICKS: std::sync::LazyLock<Vec<Pick>> = std::sync::LazyLock::new(|| {
    let presets = providers::PRESETS.iter().filter(|p| !p.custom());
    std::iter::once(Pick(None))
        .chain(presets.map(|p| Pick(Some(*p))))
        .collect()
});

/// The pick the running config is (`None`: a value the file can't give).
pub(super) fn picked(cfg: &config::AiConfig) -> Option<Pick> {
    match cfg.provider {
        Provider::Browser => Some(Pick(None)),
        _ => Preset::of(cfg).map(|p| Pick(Some(p))),
    }
}

/// `ai` set to `pick` (as Settings' Provider list does): its provider and
/// server, and its suggested model while the model is empty or another's.
pub(super) fn apply_pick(ai: &mut config::AiConfig, pick: Pick) {
    let Some(p) = pick.0 else {
        ai.provider = Provider::Browser;
        ai.base_url.clear();
        return;
    };
    ai.provider = p.provider();
    ai.base_url = p.base_url.to_string();
    if ai.model.trim().is_empty() || providers::PRESETS.iter().any(|x| x.model == ai.model) {
        ai.model = p.model.to_string();
    }
}

/// "AI chat stays open: Always": a chat is pinned from its first question.
pub(super) fn starts_pinned(stays: StaysOpen) -> bool {
    stays == StaysOpen::Always
}

/// A model id that names a safety / moderation classifier: it rates messages
/// instead of answering them (the drawer warns).
pub fn classifier(model: &str) -> bool {
    let m = model.to_lowercase();
    ["safety", "guard", "moderation"]
        .iter()
        .any(|w| m.contains(w))
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The card's grip or corner was pressed (the App moves / resizes it).
    Card(panel_frame::Event),
    /// The Lens page is written and the browser launched (the App holds the page).
    LensOpened(Result<lens::Held, String>),
    /// An edit in the composer (typing, a new line, a paste).
    Edit(text_editor::Action),
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
    /// The header's pin: pins the chat; on a pinned one, closes it (the App).
    Pin,
    /// The drawer opens on a tab, or closes (`None`).
    Drawer(Option<Tab>),
    /// The drawer's "AI chat stays open".
    StaysOpen(StaysOpen),
    /// The drawer's quick provider picker, its model field and Enter in it.
    Pick(Pick),
    Model(String),
    SaveModel,
    /// "Open AI settings →" (the App opens Settings > AI).
    OpenAiSettings,
    /// The History tab's list of saved chats (newest first).
    Listed(Vec<chats::Entry>),
    /// History's search field: chats whose title or text hold it.
    Find(String),
    /// Star a saved chat (a favourite, listed first), or not.
    Star(String, bool),
    /// Open a saved chat, delete one, or all.
    OpenSaved(String),
    Loaded(Result<chats::Saved, String>),
    DeleteSaved(String),
    ClearHistory,
    /// A summary of the chat's older messages, written at this many messages.
    Summarized(usize, Result<String, String>),
    /// A setting was saved (redraws the drawer).
    Saved,
}

pub struct Panel {
    crop: Arc<Crop>,
    state: State,
    /// The follow-up input's text (one line, more with Shift / Ctrl+Enter).
    input: text_editor::Content,
    /// The input holds a question asked elsewhere: sent once ready.
    queued: bool,
    /// Pinned: outside clicks and Esc no longer close it (`App::chat`).
    pinned: bool,
    /// Picked up and dragged by its header (the App sets it).
    pub held: bool,
    /// The drawer's open tab, its History list (and what it was searched
    /// for) and its model field.
    drawer: Option<Tab>,
    listed: Vec<chats::Entry>,
    find: String,
    model: String,
    /// The older messages' summary being written; dropping it stops that.
    summarizing: Option<task::Handle>,
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
            input: text_editor::Content::with_text(question.as_deref().unwrap_or_default()),
            queued: question.is_some(),
            ..Panel::new(crop.clone(), State::Lens(None))
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
                panel.pinned = starts_pinned(config::current().chat.stays_open);
                return {
                    let task = panel.load();
                    (panel, task)
                };
            }
        };
        (panel, task)
    }

    /// Reads the config and key and encodes the crop (`Ready`). No key (Google
    /// AI Mode): nothing to load (no keyring, no PNG), so a question goes at
    /// once, never "Getting ready…" before it.
    fn load(&mut self) -> Task<Msg> {
        let crop = self.crop.clone();
        if config::current().ai.provider == Provider::Browser {
            let ready = ask::setup(&crop).map(Arc::new);
            return self.update(Msg::Ready(ready));
        }
        background(move |tx| {
            let _ = tx.unbounded_send(Msg::Ready(ask::setup(&crop).map(Arc::new)));
        })
    }

    /// A new capture while this chat stays open: its crop is what the next
    /// question asks about, and `question` (asked in the bar) goes into this
    /// same chat once the crop is ready. `include`: the bar's Include screen.
    pub fn ask_about(&mut self, crop: Crop, question: Option<String>, include: bool) -> Task<Msg> {
        self.crop = Arc::new(crop);
        if let State::Ask { include: on, .. } = &mut self.state {
            *on = include;
        }
        if let Some(question) = question {
            self.input = text_editor::Content::with_text(&question);
            self.queued = true;
        }
        self.drawer = None;
        // always on a thread: the chat keeps its setup until the new one is ready
        let crop = self.crop.clone();
        background(move |tx| {
            let _ = tx.unbounded_send(Msg::Ready(ask::setup(&crop).map(Arc::new)));
        })
    }

    /// The chat (tests).
    #[cfg(test)]
    pub fn chat(&self) -> Option<&Chat> {
        match &self.state {
            State::Ask { chat, .. } => Some(chat),
            State::Lens(_) => None,
        }
    }

    /// Pinned: it stays open over other apps until its pin closes it.
    pub fn pinned(&self) -> bool {
        self.pinned
    }

    /// The App's chat takes in another (the translate word card's): its turns
    /// follow this chat's (one chat only).
    pub fn append(&mut self, mut other: Chat) {
        other.cancel();
        if let State::Ask { chat, .. } = &mut self.state
            && chat.status != Status::Streaming
        {
            chat.turns.append(&mut other.turns);
        }
        self.sync();
    }

    /// A panel in `state` on `crop`, nothing typed, nothing running.
    fn new(crop: Arc<Crop>, state: State) -> Panel {
        Panel {
            crop,
            state,
            input: text_editor::Content::new(),
            queued: false,
            pinned: false,
            held: false,
            drawer: None,
            listed: Vec::new(),
            find: String::new(),
            model: String::new(),
            summarizing: None,
            work: None,
            note: None,
            spin: None,
        }
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
        std::mem::replace(&mut self.input, text_editor::Content::new()).text()
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
            (_, Msg::Edit(action)) => self.input.perform(action),
            (_, Msg::Pin) => self.pinned = true,
            (State::Ask { .. }, Msg::Card(Event::Menu)) => {
                let tab = match self.drawer {
                    Some(_) => None,
                    None => Some(Tab::History),
                };
                return self.on(Msg::Drawer(tab));
            }
            (State::Ask { .. }, Msg::Drawer(tab)) => {
                self.drawer = tab;
                self.model = config::current().ai.model;
                if tab == Some(Tab::History) {
                    return list_saved(&self.find);
                }
            }
            (_, Msg::Listed(all)) => self.listed = all,
            (_, Msg::Find(find)) => {
                self.find = find;
                return list_saved(&self.find);
            }
            (_, Msg::Star(id, on)) => {
                // at once on the card; the list sorts again once written
                if let Some(entry) = self.listed.iter_mut().find(|e| e.id == id) {
                    entry.star = on;
                }
                let Some(db) = store::path() else {
                    return Task::none();
                };
                let find = self.find.clone();
                return background(move |tx| {
                    if let Err(e) = chats::star(&db, &id, on) {
                        crate::log!("[opencircle] {e}");
                    }
                    let _ = tx.unbounded_send(Msg::Listed(chats::list(&db, &find)));
                });
            }
            (_, Msg::OpenSaved(id)) => {
                let Some(db) = store::path() else {
                    return Task::none();
                };
                return background(move |tx| {
                    let _ = tx.unbounded_send(Msg::Loaded(chats::load(&db, &id)));
                });
            }
            (
                State::Ask {
                    chat,
                    browse,
                    editing,
                    ..
                },
                Msg::Loaded(Ok(saved)),
            ) => {
                (*chat, *browse, *editing) = (Chat::from_saved(saved), Browse::Idle, None);
                (self.work, self.summarizing, self.drawer) = (None, None, None);
            }
            (_, Msg::Loaded(Err(e))) => self.note = Some(Err(e)),
            (_, Msg::DeleteSaved(id)) => {
                let Some(db) = store::path() else {
                    return Task::none();
                };
                if let State::Ask { chat, .. } = &mut self.state
                    && chat.id.as_deref() == Some(id.as_str())
                {
                    chat.id = None; // this chat, if saved again, is a new one
                }
                let find = self.find.clone();
                return background(move |tx| {
                    if let Err(e) = chats::delete(&db, &id) {
                        crate::log!("[opencircle] {e}");
                    }
                    let _ = tx.unbounded_send(Msg::Listed(chats::list(&db, &find)));
                });
            }
            (_, Msg::ClearHistory) => {
                self.listed.clear();
                if let State::Ask { chat, .. } = &mut self.state {
                    chat.id = None;
                }
                return clear_saved();
            }
            (State::Ask { .. }, Msg::StaysOpen(how)) => {
                self.pinned |= how == StaysOpen::Always;
                return save_config(move |c| c.chat.stays_open = how, None);
            }
            (State::Ask { .. }, Msg::Pick(pick)) => {
                let model = self.model.clone();
                let crop = self.crop.clone();
                return save_config(
                    move |c| {
                        c.ai.model = model; // what the field shows, then the pick's rule
                        apply_pick(&mut c.ai, pick);
                    },
                    Some(crop),
                );
            }
            (_, Msg::Model(model)) => self.model = model,
            (State::Ask { .. }, Msg::SaveModel) => {
                let model = self.model.trim().to_string();
                if model.is_empty() {
                    return Task::none();
                }
                return save_config(move |c| c.ai.model = model, Some(self.crop.clone()));
            }
            (State::Ask { chat, .. }, Msg::Summarized(at, result)) => {
                self.summarizing = None;
                match result {
                    Ok(text) if !text.is_empty() => {
                        chat.summary = Some(chats::Summary { text, at });
                        return keep(chat);
                    }
                    Ok(_) => {}
                    Err(e) => crate::log!("[opencircle] chat summary: {e}"),
                }
            }
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
                (self.work, self.summarizing, self.drawer) = (None, None, None);
                self.input = text_editor::Content::new();
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
                    self.input = text_editor::Content::with_text(&turn.question);
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
                // only a question asked elsewhere, never a draft in the composer
                if ready && std::mem::take(&mut self.queued) && !self.input.text().trim().is_empty()
                {
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
                return self.answered();
            }
            (_, Msg::Acted(note)) => self.note = Some(note),
            _ => {}
        }
        Task::none()
    }

    /// An answer came: the chat is saved (text only), and once it is long its
    /// older messages' summary is written (`ask::summary_due`) by the same
    /// provider, with the user's own key.
    fn answered(&mut self) -> Task<Msg> {
        let State::Ask {
            chat,
            setup: Some(Ok(setup)),
            ..
        } = &mut self.state
        else {
            return Task::none();
        };
        if setup.browser() {
            return Task::none(); // a hand-off to the browser: no conversation to keep
        }
        let mut tasks = vec![keep(chat)];
        if let (None, Some((at, messages))) = (&self.summarizing, chat.summary_request()) {
            let setup = setup.clone();
            let (task, handle) = background(move |tx| {
                let summary = ask::summarize(&setup, &messages);
                let _ = tx.unbounded_send(Msg::Summarized(at, summary));
            })
            .abortable();
            self.summarizing = Some(handle.abort_on_drop());
            tasks.push(task);
        }
        Task::batch(tasks)
    }

    /// Asks `question`; the answer streams in on a thread. Nothing is sent before this.
    fn send(&mut self, question: &str) -> Task<Msg> {
        // a second question in the same chat pins it
        if let State::Ask { chat, .. } = &self.state
            && !chat.turns.is_empty()
            && !question.trim().is_empty()
        {
            self.pinned = true;
        }
        let size = (self.crop.width, self.crop.height);
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
        if let Some(turn) = chat.turns.last_mut().filter(|t| t.screen) {
            turn.shot = Some(size);
        }
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

/// Saves `chat` (text only) while Keep chat history is on; nothing otherwise.
fn keep(chat: &mut Chat) -> Task<Msg> {
    keep_in(store::path(), config::current().chat.keep_history, chat)
}

/// `keep` into the database `db`, if any, when `on`.
fn keep_in(db: Option<std::path::PathBuf>, on: bool, chat: &mut Chat) -> Task<Msg> {
    let Some(db) = db.filter(|_| on) else {
        return Task::none();
    };
    let (saved, generation) = (chat.saved(), chats::generation());
    background(move |_| {
        if let Err(e) = chats::save(&db, &saved, generation) {
            crate::log!("[opencircle] {e}");
        }
    })
}

/// The saved chats holding `find` (all when blank), favourites then newest
/// first (`Listed`).
fn list_saved(find: &str) -> Task<Msg> {
    let Some(db) = store::path() else {
        return Task::none();
    };
    let find = find.to_string();
    background(move |tx| {
        let _ = tx.unbounded_send(Msg::Listed(chats::list(&db, &find)));
    })
}

/// Every saved chat goes ("Clear all history"); a save on its way writes nothing.
fn clear_saved() -> Task<Msg> {
    chats::forget_all();
    Task::done(Msg::Listed(Vec::new()))
}

/// Saves a drawer setting (`edit` on the config as it is now, on a thread);
/// with `crop`, the chat then reloads its provider and key for it (`Ready`).
fn save_config(
    edit: impl FnOnce(&mut config::Config) + Send + 'static,
    crop: Option<Arc<Crop>>,
) -> Task<Msg> {
    background(move |tx| {
        if let Err(e) = config::update(edit) {
            crate::log!("[opencircle] settings not saved: {e}");
        }
        let _ = tx.unbounded_send(Msg::Saved);
        if let Some(crop) = crop {
            let _ = tx.unbounded_send(Msg::Ready(ask::setup(&crop).map(Arc::new)));
        }
    })
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

/// Browser provider (no key): Ask AI only hands the question to Google AI Mode
/// in the user's browser, with the selection's text when Include screen is on
/// and the text reader is here; no chat, no chat history (the owner,
/// 2026-10-28). Blocking: the pill's text, or why not.
pub fn hand_off(crop: &Crop, question: Option<String>, include: bool) -> Result<String, String> {
    let target = config::current().translate.target;
    let prompt = question
        .filter(|q| !q.trim().is_empty())
        .unwrap_or_else(|| PRESETS[0].text(&target));
    let read = (include && text::models_present()).then(|| text::read(crop, |_| {}));
    let (question, _) = browser_turn(&prompt, read);
    ai::browser::ask(&question)?;
    let url = ai::browser::url(&question).0;
    crate::history::search::keep("ai", &prompt, &url, Some(crop));
    Ok("Opened in your browser".into())
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
pub(crate) mod tests;
