//! Translate in place (DESIGN.md "Full-screen translate"): the frozen frame, or
//! the selection in it, is translated on a thread (`translate::pipeline`) and
//! swapped in where it was, top first as the blocks are ready (#27).
//! Hold Alt = the original. Click a translated block = its word card, whose
//! Explain (an explicit click) asks the AI; `A` continues as an Ask AI chat.
use std::sync::Arc;

use iced::widget::{image, markdown};
use iced::{Point, Rectangle, Size, Task, task};

use crate::app::background::background;
use crate::features::ask_ai::AiError;
use crate::features::ask_ai::ask::{self, Chat, Status};

use crate::features::circle::actions;
use crate::features::text_reading::models;
use crate::features::text_reading::screens::model_download;
use crate::features::translate::mt::Pair;
use crate::features::translate::pipeline::{self, Cache, Deps, Error, Translation};
use crate::features::translate::screens::language_picker::{self as picker, Picker};
use crate::shared::app_language::{S, fmt};
use crate::shared::kit::panel_frame as card;
use crate::shared::picture::{self, Crop};
use crate::t;

/// Context around a block in the image the word card sends (physical px).
const CONTEXT: u32 = 8;

#[derive(Debug, Clone)]
pub enum Msg {
    /// The user agreed to the OCR (or offline translation) download.
    Download,
    /// Translation will be offline and this pair's models are missing
    /// (bytes): ask.
    Offline(&'static Pair, u64),
    /// The language picker; a new source or target translates again.
    Picker(picker::Msg),
    /// The glossary button's list; another glossary translates again.
    Glossary(glossary::GlossaryMsg),
    /// The word card's Add to glossary (or its key, G), and its form.
    AddStart,
    /// A dotted name of the card's original clicked: the form for that name.
    AddFor(String),
    Add(glossary::AddMsg),
    /// Declined the download, or closed an error: the overlay closes.
    Close,
    /// The pill's Snip button: Snip takes the page as translated (the App
    /// does what Alt+A does).
    Snip,
    /// Model download, 0..=1.
    Progress(f32),
    /// The download failed (why).
    DownloadFailed(String),
    /// The models are in place; translating now.
    Translating,
    /// What is translated so far (the rest still shows the original).
    Partial(Part),
    Done(Result<Shown, Problem>),
    /// Alt went down (true) or up.
    Peek(bool),
    /// The word card's Explain (or its key, E).
    Explain,
    Delta(String),
    Explained(Result<(), AiError>),
    /// The error card's Show / Hide details.
    Details,
    /// A click in the explanation: a link, or a code block's Copy.
    Answer(crate::shared::kit::markdown::Act),
    /// The word card's Ask AI (or `A`): continue as a chat.
    Ask,
    /// The AI is not set up: open Settings.
    OpenSettings,
    /// Get the pair of the lines kept in their own language (`Offer`).
    GetHeld,
    /// The page read as noise: offer the readers for other scripts (bytes).
    Readers(u64, String),
    /// The card's frame (`kit::panel_frame`): the App moves, resizes and
    /// closes it, as for the other result cards.
    Card(card::Event),
    /// The used pairs' model update (`mt::update`), 0..=1 ...
    ModelUpdate(f32),
    /// ... and whether it finished.
    ModelUpdated(bool),
}

/// What is translated so far: just the translated area, drawn over the frozen
/// frame at `area` (frame px: x0, y0, x1, y1). Never a copy of the whole frame
/// per partial (8 MB at 1080p, up to every 300 ms: audit 12).
#[derive(Debug, Clone)]
pub struct Part {
    image: image::Handle,
    area: [u32; 4],
}

/// The translated frame and its blocks (frame pixels).
#[derive(Debug, Clone)]
pub struct Shown {
    image: image::Handle,
    blocks: Vec<Translation>,
    /// Lines certainly in another language whose pair isn't here, kept as
    /// they were: offered quietly in the pill, never waited for.
    offer: Option<Box<Offer>>,
    /// The pairs it used whose decoder is still the older v1 one: updated
    /// now, in the background (`Session::update_models`).
    outdated: Vec<&'static Pair>,
}

#[derive(Debug, Clone)]
pub struct Offer {
    pair: &'static Pair,
    /// The kept lines as sent (`Real::held`).
    lines: Vec<String>,
    /// "Spanish text found", "Get Spanish → Arabic".
    found: String,
    get: String,
}

impl Offer {
    fn new((pair, lines): (&'static Pair, Vec<String>), source: &str, target: &str) -> Offer {
        use crate::shared::app_language::language_name as name;
        Offer {
            pair,
            lines,
            found: fmt(S::tp_found, &[&name(source)]),
            get: fmt(S::tp_get, &[&name(source), &name(target)]),
        }
    }
}

/// Why nothing was translated. The original frame stays as it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub message: String,
    /// The AI is not set up (no key, provider, model): offer Settings.
    pub setup: bool,
}

#[derive(Debug)]
enum Phase {
    /// No OCR models for the source yet: ask first (the download
    /// card). `Some`: an offline pair's models (and OCR's, if missing)
    /// instead, with the bytes to download.
    Consent(Option<(&'static Pair, u64)>),
    Downloading(f32),
    /// The download stopped (why); the card offers it again.
    DownloadFailed(String),
    Translating,
    Shown(Shown),
    Failed(Problem),
}

pub struct Session {
    /// The original frame (shared bytes, never copied whole here).
    frame: image::Handle,
    /// What is translated, frame pixels: x0, y0, x1, y1 (ends exclusive).
    area: [u32; 4],
    /// The whole frame was asked for (no selection to outline).
    whole: bool,
    phase: Phase,
    peek: bool,
    card: Option<Card>,
    cache: Arc<Cache>,
    /// The download / translation; dropping it stops the thread's messages.
    work: Option<task::Handle>,
    /// The offline pair the user was asked for (and agreed to download).
    offline: Option<&'static Pair>,
    /// Source → target (the config's at first).
    picker: Picker,
    /// The monitor's scale (`Real::scale`).
    scale: f32,
    /// While translating: what is translated so far.
    partial: Option<Part>,
    /// The OCR download asked for is the readers for other scripts (bytes).
    readers: Option<(u64, String)>,
    /// The glossary button and its list.
    glossary: glossary::Menu,
    /// What the last "Add to glossary" saved, a quiet tag beside the pill.
    tag: Option<String>,
    /// Snip's own shortcut, for its button's tooltip.
    snip_keys: String,
    /// The used pairs' model update: its bytes and share so far; `None` =
    /// none running. Its thread goes on when the overlay closes.
    updating: Option<(u64, f32)>,
    update_work: Option<task::Handle>,
    /// The update stopped (offline): said once per run of the app.
    update_paused: bool,
}

/// An update that stopped was said (`tp_update_paused`) in this run.
static PAUSE_SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The card of one clicked block.
struct Card {
    block: Translation,
    /// The block (with a little context) from the original frame: what Explain sends.
    crop: Arc<Crop>,
    chat: Chat,
    /// The AI's explanation parsed as Markdown (it shares the chat's system
    /// prompt, which allows it).
    answer: markdown::Content,
    work: Option<task::Handle>,
    /// The target language is right-to-left: the answer is right-aligned.
    rtl: bool,
    /// Its heading: "translation · original".
    title: String,
    /// Its error card shows the whole detail.
    details: bool,
    /// The Add to glossary form is open.
    add: Option<glossary::AddForm>,
}

impl Session {
    /// Translates `area` of `frame` from the configured source into the
    /// configured target. Asks first for what is missing: the OCR models
    /// (`models` = present), or, when translation will be offline, its
    /// models with OCR's in one download. The whole frame is all of it
    /// (full screen means the full screen; only live reads the window),
    /// but the OS taskbar (`taskbar` frame px under the monitor's work area)
    /// is the desktop's chrome: its clock was translated ("PM" -> "مساءً").
    // ponytail: a bottom taskbar only (`taskbar_inset`); a side or top one,
    // or GNOME's top bar (Wayland tells no work area), is still read.
    pub fn start(
        frame: image::Handle,
        area: [u32; 4],
        scale: f32,
        cache: Arc<Cache>,
        models: bool,
        taskbar: u32,
    ) -> (Session, Task<Msg>) {
        let cfg = crate::shared::config::current().translate;
        let offline = crate::features::translate::action::will_be_offline();
        let size = match &frame {
            image::Handle::Rgba { width, height, .. } => (*width, *height),
            _ => (0, 0),
        };
        let whole = area == [0, 0, size.0, size.1];
        let area = match whole && taskbar < size.1 {
            true => [0, 0, size.0, size.1 - taskbar],
            false => area,
        };
        let mut session = Session {
            frame,
            area,
            whole,
            phase: Phase::Consent(None),
            peek: false,
            card: None,
            cache,
            work: None,
            offline: None,
            picker: Picker::new(&cfg.source, &cfg.target, offline),
            scale,
            partial: None,
            readers: None,
            tag: None,
            glossary: glossary::Menu::load(
                &cfg.glossary,
                crate::shared::platform::capture::window_rect::captured_title().as_deref(),
            ),
            snip_keys: (crate::shared::config::current().quick_actions.into_iter())
                .find(|q| q.action == crate::features::snip::action::ACTION.id)
                .map(|q| q.shortcut)
                .unwrap_or_default(),
            updating: None,
            update_work: None,
            update_paused: false,
        };
        let task = session.begin(models);
        (session, task)
    }

    /// Asks for missing models, or translates. A few file sizes: cheap
    /// enough here. A missing API key is only found on the thread
    /// (keyring), which then asks (`Msg::Offline`).
    fn begin(&mut self, models: bool) -> Task<Msg> {
        let (source, target) = (&self.picker.source, &self.picker.target);
        let offline = (crate::features::translate::action::will_be_offline())
            .then(|| asked_first(source, target))
            .flatten()
            .map(|p| (p, p.missing_bytes()))
            .filter(|&(_, b)| b > 0);
        self.card = None;
        self.partial = None;
        self.readers = None;
        if models && offline.is_none() {
            self.phase = Phase::Translating;
            self.run(false)
        } else {
            self.work = None;
            self.phase = Phase::Consent(offline);
            Task::none()
        }
    }

    /// The whole frame is translated: no selection
    /// to show.
    pub fn full(&self) -> bool {
        self.whole
    }

    /// What to show instead of the original frame: the translation, unless
    /// Alt is held or it isn't ready.
    pub fn image(&self) -> Option<&image::Handle> {
        match &self.phase {
            _ if self.peek => None,
            Phase::Shown(shown) => Some(&shown.image),
            _ => None,
        }
    }

    /// The frame as the translation shows it (full frame px, never scaled):
    /// what Snip takes (owner, 2026-11-10). The translated page, or while
    /// translating the original with what is translated so far pasted in;
    /// `None` = nothing translated on screen yet. Alt (the peek) doesn't
    /// count: Alt+A's own Alt is down when Snip asks.
    pub fn seen(&self) -> Option<image::Handle> {
        match &self.phase {
            Phase::Shown(shown) => Some(shown.image.clone()),
            Phase::Translating => {
                let part = self.partial.as_ref()?;
                let (
                    image::Handle::Rgba {
                        width,
                        height,
                        pixels,
                        ..
                    },
                    image::Handle::Rgba { pixels: rgba, .. },
                ) = (&self.frame, &part.image)
                else {
                    return None;
                };
                let mut full = pixels.to_vec();
                paste(&mut full, *width, part.area, rgba);
                Some(image::Handle::from_rgba(*width, *height, full))
            }
            _ => None,
        }
    }

    /// Tests elsewhere: the translation `image` is shown.
    #[cfg(test)]
    pub(crate) fn show(&mut self, image: image::Handle) {
        let blocks = Vec::new();
        self.phase = Phase::Shown(Shown {
            image,
            blocks,
            offer: None,
            outdated: Vec::new(),
        });
    }

    /// What is translated now and where it goes (frame px: x0, y0, x1, y1);
    /// `true`: the image is the whole frame (the finished translation), else
    /// only the area (what is ready so far). Snip's More pastes it into its
    /// picture. Alt (the peek) doesn't count.
    pub fn translated(&self) -> Option<(&image::Handle, [u32; 4], bool)> {
        match &self.phase {
            Phase::Shown(shown) => Some((&shown.image, self.area, true)),
            Phase::Translating => self.partial.as_ref().map(|p| (&p.image, p.area, false)),
            _ => None,
        }
    }

    /// While translating: the area translated so far and where it goes in the
    /// frame (px), to draw over the original frame, unless Alt is held.
    pub fn part(&self) -> Option<(&image::Handle, [u32; 4])> {
        match &self.phase {
            _ if self.peek => None,
            Phase::Translating => self.partial.as_ref().map(|p| (&p.image, p.area)),
            _ => None,
        }
    }

    /// The App handles `Close`, `OpenSettings` and `Ask` first.
    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Download if matches!(self.phase, Phase::Consent(_) | Phase::DownloadFailed(_)) => {
                // Try again fetches what was asked for before
                self.offline = match self.phase {
                    Phase::Consent(Some((pair, _))) => Some(pair),
                    Phase::Consent(None) => None,
                    _ => self.offline,
                };
                self.phase = Phase::Downloading(0.0);
                return self.run(true);
            }
            Msg::Readers(bytes, names) if matches!(self.phase, Phase::Translating) && bytes > 0 => {
                self.work = None;
                self.readers = Some((bytes, names));
                self.offline = None;
                self.phase = Phase::Consent(None);
            }
            Msg::Offline(pair, bytes) if matches!(self.phase, Phase::Translating) => {
                self.work = None;
                // the readers (if they were asked for) are installed by now:
                // Download fetches the models, not the readers again (#163)
                self.readers = None;
                self.phase = Phase::Consent(Some((pair, bytes)));
            }
            Msg::GetHeld => {
                if let Phase::Shown(Shown { offer: Some(o), .. }) = &self.phase {
                    // the kept lines were cached as "as they were"
                    let target = &self.picker.target;
                    o.lines.iter().for_each(|l| self.cache.forget(l, target));
                    self.offline = Some(o.pair);
                    self.card = None;
                    self.phase = Phase::Downloading(0.0);
                    return self.run(true);
                }
            }
            Msg::AddStart => self.start_add(None),
            Msg::AddFor(word) => self.start_add(Some(word)),
            Msg::Add(msg) => return self.add(msg),
            Msg::Glossary(msg) => {
                self.picker.close();
                // (New / Edit open Settings: the App takes `Glossary(Edit)` first)
                if let Some(true) = self.glossary.update(msg) {
                    let source = &self.picker.source;
                    let models = models::installed(models::recognizer_for(source));
                    return self.begin(models);
                }
            }
            Msg::Picker(msg) => {
                let opens = matches!(
                    msg,
                    picker::Msg::Open(_) | picker::Msg::Key(picker::Nav::Tab)
                );
                self.glossary.close();
                if self.picker.update(msg) {
                    let source = &self.picker.source;
                    let models = models::installed(models::recognizer_for(source));
                    return self.begin(models);
                }
                // typing filters at once
                if opens && self.picker.is_open() {
                    return iced::widget::operation::focus(picker::SEARCH);
                }
            }
            Msg::Progress(done) if matches!(self.phase, Phase::Downloading(_)) => {
                self.phase = Phase::Downloading(done);
            }
            Msg::DownloadFailed(why) if matches!(self.phase, Phase::Downloading(_)) => {
                self.work = None;
                self.phase = Phase::DownloadFailed(why);
            }
            Msg::Translating => self.phase = Phase::Translating,
            Msg::Partial(shown) if matches!(self.phase, Phase::Translating) => {
                self.partial = Some(shown);
            }
            Msg::Done(result) if matches!(self.phase, Phase::Translating) => {
                self.work = None;
                self.partial = None;
                let update = (result.as_ref().ok()).map(|s| s.outdated.clone());
                self.phase = match result {
                    Ok(shown) => Phase::Shown(shown),
                    Err(problem) => Phase::Failed(problem),
                };
                if let Some(pairs) = update {
                    return self.update_models(pairs);
                }
            }
            Msg::ModelUpdate(share) => {
                if let Some((_, at)) = &mut self.updating {
                    *at = share;
                }
            }
            Msg::ModelUpdated(done) => {
                self.updating = None;
                self.update_work = None;
                // quietly: once per run, then a later Translate just tries again
                self.update_paused =
                    !done && !PAUSE_SAID.swap(true, std::sync::atomic::Ordering::Relaxed);
            }
            Msg::Peek(down) => self.peek = down,
            Msg::Explain => return self.explain(),
            Msg::Delta(piece) => {
                if let Some(card) = &mut self.card {
                    card.chat.delta(&piece);
                    card.answer.push_str(&piece);
                }
            }
            Msg::Answer(act) => {
                use crate::shared::kit::markdown::Act;
                // Only on a click, and only a web link (`open_link`).
                return match act {
                    Act::Link(url) => background(
                        move |_: &iced::futures::channel::mpsc::UnboundedSender<Msg>| {
                            if let Err(e) = crate::shared::platform::sys::open::open_link(&url) {
                                crate::log!("explanation link not opened: {e}");
                            }
                        },
                    )
                    .discard(),
                    Act::Copy(code) => background(
                        move |_: &iced::futures::channel::mpsc::UnboundedSender<Msg>| {
                            if let Err(e) = actions::copy::text(code) {
                                crate::log!("copy failed: {e}");
                            }
                        },
                    )
                    .discard(),
                };
            }
            Msg::Details => {
                if let Some(card) = &mut self.card {
                    card.details = !card.details;
                }
            }
            Msg::Explained(result) => {
                if let Some(card) = &mut self.card {
                    card.details = false;
                    card.chat.finish(result);
                    card.work = None;
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// A tap at `p` (frame pixels): the translated block there gets its card;
    /// anywhere else closes the card.
    pub fn click(&mut self, p: Point) {
        self.card = None;
        self.tag = None;
        if self.picker.is_open() || self.glossary.is_open() {
            self.picker.close();
            self.glossary.close();
            return;
        }
        let (
            Phase::Shown(shown),
            image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            },
        ) = (&self.phase, &self.frame)
        else {
            return;
        };
        let Some(i) = hit(&shown.blocks, p) else {
            return;
        };
        let block = shown.blocks[i].clone();
        let [x, y, w, h] = block.bounds;
        let pad = CONTEXT as f32;
        let area = Rectangle::new(
            Point::new(x - pad, y - pad),
            Size::new(w + 2.0 * pad, h + 2.0 * pad),
        );
        if let Some(crop) = picture::crop(pixels, *width, *height, 1.0, area) {
            self.card = Some(Card {
                title: format!("{} · {}", block.translation, block.original),
                block,
                crop: Arc::new(crop),
                chat: Chat::default(),
                answer: markdown::Content::new(),
                work: None,
                rtl: pipeline::is_rtl(&self.picker.target),
                details: false,
                add: None,
            });
        }
    }

    /// A list (the languages', the glossary's) or the Add to glossary form is
    /// open: Esc closes it first.
    pub fn picker_open(&self) -> bool {
        self.picker.is_open() || self.glossary.is_open() || self.adding()
    }

    pub fn close_picker(&mut self) {
        match self.adding() {
            true => self.card.iter_mut().for_each(|c| c.add = None),
            false => {
                self.picker.close();
                self.glossary.close();
            }
        }
    }

    /// The Add to glossary form is open (the card's keys are typing).
    pub fn adding(&self) -> bool {
        self.card.as_ref().is_some_and(|c| c.add.is_some())
    }

    pub fn has_card(&self) -> bool {
        self.card.is_some()
    }

    /// Hands the card's crop and conversation to an Ask AI panel.
    pub fn take_chat(&mut self) -> Option<(Crop, Chat)> {
        let card = self.card.take()?;
        Some((Arc::unwrap_or_clone(card.crop), card.chat))
    }

    /// The Add to glossary form: a field changes, Cancel closes it, Save
    /// writes the term and translates again (the cache stamp makes only the
    /// lines holding it new).
    fn add(&mut self, msg: glossary::AddMsg) -> Task<Msg> {
        use glossary::AddMsg;
        let Some(form) = self.card.as_mut().and_then(|c| c.add.as_mut()) else {
            return Task::none();
        };
        match msg {
            AddMsg::Term(s) => form.term = s,
            AddMsg::Keep(keep) => form.keep = keep,
            AddMsg::Always(s) => {
                form.always = s;
                form.keep = false;
            }
            AddMsg::Into(p) => form.into = p.0,
            AddMsg::Name(s) => form.name = s,
            AddMsg::Cancel => self.card.iter_mut().for_each(|c| c.add = None),
            AddMsg::Save => {
                let form = form.clone();
                let dir = crate::features::translate::glossary_store::dir();
                match form.save(&dir, &self.picker.target) {
                    Ok((slug, name)) => {
                        self.glossary.activate(&slug);
                        let models = models::installed(models::recognizer_for(&self.picker.source));
                        let task = self.begin(models);
                        self.tag = Some(fmt(S::gl_save_tag, &[&form.term.trim(), &name]));
                        return task;
                    }
                    Err(e) => crate::log!("[opencircle] glossary term not saved: {e}"),
                }
            }
        }
        Task::none()
    }

    /// Explain: ONE streamed AI call with the block's text and image.
    fn explain(&mut self) -> Task<Msg> {
        let Some(card) = &mut self.card else {
            return Task::none();
        };
        if !matches!(card.chat.status, Status::Idle | Status::Failed(_)) {
            return Task::none(); // streaming, or explained already
        }
        let question = explain_question(&card.block, &self.picker.target);
        // The card keeps the turn; the thread sends it with the image.
        card.chat.ask(&question, &[]);
        card.answer = markdown::Content::new();
        let crop = card.crop.clone();
        let (task, handle) = background(move |tx| {
            let result = ask::setup(&crop)
                .map_err(AiError::NotConfigured)
                .and_then(|setup| {
                    let messages = Chat::default()
                        .ask(&question, &setup.png)
                        .unwrap_or_default();
                    ask::stream(&setup, &messages, |piece| {
                        tx.unbounded_send(Msg::Delta(piece.into())).is_ok()
                    })
                });
            let _ = tx.unbounded_send(Msg::Explained(result));
        })
        .abortable();
        card.work = Some(handle.abort_on_drop());
        task
    }

    /// The translation just shown used `pairs` with the older v1 decoder:
    /// their v2 decoders download now, after it (never slowing it), shown
    /// as a quiet tag beside the pill. One that is already here only waits
    /// to be swapped in (no tag). The helper keeps the v1 file loaded; the
    /// swap comes when it ends (`mt::update`).
    fn update_models(&mut self, pairs: Vec<&'static Pair>) -> Task<Msg> {
        let bytes: u64 = pairs.iter().map(|p| p.update_bytes()).sum();
        if bytes == 0 || self.updating.is_some() {
            return Task::none();
        }
        self.updating = Some((bytes, 0.0));
        self.update_paused = false;
        let (task, handle) = background(move |tx| {
            let progress = |done: u64, total: u64| {
                let _ = tx.unbounded_send(Msg::ModelUpdate(done as f32 / total.max(1) as f32));
            };
            let got = crate::features::translate::mt::update(&pairs, progress);
            if let Err(e) = &got {
                crate::log!("[opencircle] translation model update stopped: {e}");
            }
            let _ = tx.unbounded_send(Msg::ModelUpdated(got.is_ok()));
        })
        .abortable();
        self.update_work = Some(handle.abort_on_drop());
        task
    }

    /// The download (if `download`) and the translation, on a thread.
    /// Offline translation's models are fetched only if the user was asked
    /// for them (`self.offline`); otherwise the thread asks (`Msg::Offline`),
    /// also when the text turned out to need another pair ("auto").
    fn run(&mut self, download: bool) -> Task<Msg> {
        let (frame, area, cache) = (self.frame.clone(), self.area, self.cache.clone());
        let (source, target) = (self.picker.source.clone(), self.picker.target.clone());
        let (agreed, scale, readers) = (self.offline, self.scale, self.readers.is_some());
        let (task, handle) = background(move |tx| {
            let deps = crate::features::translate::action::deps_for(&source, &target).map(
                |(mut real, target)| {
                    real.scale = scale;
                    (real, target)
                },
            );
            let offline = deps.as_ref().is_ok_and(|(real, _)| real.offline());
            if download {
                let progress = |done: u64, total: u64| {
                    let _ = tx.unbounded_send(Msg::Progress(done as f32 / total.max(1) as f32));
                };
                let got = match readers {
                    true => crate::features::translate::action::download_readers(progress),
                    false => {
                        crate::features::translate::action::download(&source, agreed, progress)
                    }
                };
                if let Err(e) = got {
                    let _ = tx.unbounded_send(Msg::DownloadFailed(model_download::why(&e)));
                    return;
                }
                let _ = tx.unbounded_send(Msg::Translating);
            }
            let ask = |pair: &'static Pair| {
                let missing = pair.missing_bytes();
                (missing > 0).then(|| {
                    crate::log!(
                        "[opencircle] translate asks for {} ({missing} bytes)",
                        pair.label()
                    );
                    tx.unbounded_send(Msg::Offline(pair, missing))
                })
            };
            let expected = offline.then(|| asked_first(&source, &target)).flatten();
            if expected.and_then(ask).is_some() {
                return;
            }
            let partial = |shown| {
                let _ = tx.unbounded_send(Msg::Partial(shown));
            };
            let result = translate(&frame, area, &cache, &deps, partial).map(|mut shown| {
                if let Ok((real, target)) = &deps {
                    shown.offer = real.held().map(|h| {
                        let source = h.0.source;
                        Box::new(Offer::new(h, source, target))
                    });
                    shown.outdated = real.outdated();
                }
                shown
            });
            // "auto" read another language than expected: ask for its pair;
            // a page read as noise: the readers for other scripts
            if result.is_err()
                && let Ok((real, _)) = &deps
            {
                if real.missing().and_then(ask).is_some() {
                    return;
                }
                if real.wants_readers() {
                    let (bytes, names) = (
                        crate::features::translate::action::readers_bytes(),
                        crate::features::text_reading::models::missing_other_scripts(),
                    );
                    let _ = tx.unbounded_send(Msg::Readers(bytes, names));
                    return;
                }
            }
            let _ = tx.unbounded_send(Msg::Done(result));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }
}

/// The offline pair asked for before the screen is read: a set source's.
/// "auto" asks only for what the page turns out to need (a guessed English
/// pair asked a French page into German for en-de first, then for fr-de).
fn asked_first(source: &str, target: &str) -> Option<&'static Pair> {
    (source != crate::features::translate::lang::AUTO)
        .then(|| crate::features::translate::mt::next(source, target))
        .flatten()
}

/// Runs the pipeline on `area` of `frame` and pastes the result back into a
/// copy of the frame; `partial` gets just the translated area so far.
/// `deps` = `actions::translate::deps()`. Blocking (OCR helper, HTTP).
fn translate(
    frame: &image::Handle,
    area: [u32; 4],
    cache: &Cache,
    deps: &Result<(impl Deps, String), String>,
    mut partial: impl FnMut(Part) + Send,
) -> Result<Shown, Problem> {
    let image::Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = frame
    else {
        return Err(problem(t!(tx_not_rgba).into(), false));
    };
    let (deps, target) = deps.as_ref().map_err(|e| {
        let first = e.lines().next().unwrap_or_default().to_string();
        problem(first, true)
    })?;
    let [x0, y0, x1, y1] = area;
    let rect = Rectangle::new(
        Point::new(x0 as f32, y0 as f32),
        Size::new((x1 - x0) as f32, (y1 - y0) as f32),
    );
    let crop = picture::crop(pixels, *width, *height, 1.0, rect)
        .ok_or_else(|| problem(t!(tx_nothing_there).into(), false))?;
    // the area's result pasted into a copy of the whole frame
    let mut swap = std::time::Duration::ZERO;
    let mut shown = |out: &pipeline::Translated| {
        let t = std::time::Instant::now();
        let mut full = pixels.to_vec();
        paste(&mut full, *width, area, &out.rgba);
        let blocks = (out.blocks.iter().cloned())
            .map(|mut b| {
                b.bounds[0] += x0 as f32;
                b.bounds[1] += y0 as f32;
                b
            })
            .collect();
        swap += t.elapsed();
        Shown {
            image: image::Handle::from_rgba(*width, *height, full),
            blocks,
            offer: None,
            outdated: Vec::new(),
        }
    };
    let out = pipeline::translate_progressively(
        &crop.rgba,
        crop.width,
        crop.height,
        target,
        cache,
        deps,
        |out| {
            partial(Part {
                image: image::Handle::from_rgba(crop.width, crop.height, out.rgba.clone()),
                area,
            })
        },
    )
    .map_err(|e| {
        // Offline can't do it (no model, not downloaded): AI is the other way.
        let setup = matches!(
            e,
            Error::Ai(AiError::NotConfigured(_) | AiError::Auth(..) | AiError::Offline(_))
        );
        // an AI provider's error: the error card's short headline (owner, 2026-11-10)
        let message = match &e {
            Error::Ai(AiError::Offline(_) | AiError::AlreadyIn(_)) => e.to_string(),
            Error::Ai(ai) => {
                ai.headline(&crate::features::ask_ai::screens::ai_chat::provider_name(
                    &crate::shared::config::current().ai,
                ))
            }
            _ => e.to_string(),
        };
        problem(message, setup)
    })?;
    let result = shown(&out);
    let t = out.timings;
    crate::log!(
        "[opencircle] translate {}x{} at {x0},{y0}: ocr {} ms, translate {} ms, render {} ms, \
         swap {} ms, first shown {} ms, total {} ms; {} blocks, {} drawn, {} left \
         (busy background {}, no room {}), {} kept (no words, or the model gave them back \
         unchanged)",
        crop.width,
        crop.height,
        t.ocr.as_millis(),
        t.translate.as_millis(),
        t.render.as_millis(),
        swap.as_millis(),
        t.first.as_millis(),
        t.total.as_millis(),
        out.blocks.len(),
        out.drawn.drawn,
        out.drawn.busy + out.drawn.no_room,
        out.drawn.busy,
        out.drawn.no_room,
        out.blocks
            .iter()
            .filter(|b| b.translation.is_empty())
            .count(),
    );
    Ok(result)
}

fn problem(message: String, setup: bool) -> Problem {
    Problem { message, setup }
}

/// Copies `rgba` (the area's size) into `frame` (`width` wide) at `area`.
fn paste(frame: &mut [u8], width: u32, area: [u32; 4], rgba: &[u8]) {
    let [x0, y0, x1, _] = area.map(|v| v as usize);
    let row = (x1 - x0) * 4;
    for (i, src) in rgba.chunks_exact(row).enumerate() {
        let at = ((y0 + i) * width as usize + x0) * 4;
        frame[at..at + row].copy_from_slice(src);
    }
}

/// The translated block under `p` (frame pixels); the smallest one if boxes
/// overlap. Blocks left as they were (no translation) are not clickable.
fn hit(blocks: &[Translation], p: Point) -> Option<usize> {
    let area = |b: &Translation| b.bounds[2] * b.bounds[3];
    (blocks.iter().enumerate())
        .filter(|(_, b)| !b.translation.is_empty())
        .filter(|(_, b)| {
            let [x, y, w, h] = b.bounds;
            (x..x + w).contains(&p.x) && (y..y + h).contains(&p.y)
        })
        .min_by(|(_, a), (_, b)| area(a).total_cmp(&area(b)))
        .map(|(i, _)| i)
}

fn explain_question(block: &Translation, target: &str) -> String {
    format!(
        "In this game scene, \"{}\" was translated as \"{}\". Answer in the language \
         with code \"{target}\": the meaning of the words, then ONE line on what they \
         mean here, in the context of the scene.",
        block.original, block.translation
    )
}

pub(crate) mod glossary;
pub(crate) mod view;

#[cfg(test)]
mod tests;

impl Session {
    /// Opens the word card's Add to glossary form; `word` (a clicked name)
    /// is the term, else the first name of the original.
    fn start_add(&mut self, word: Option<String>) {
        let title = crate::shared::platform::capture::window_rect::captured_title();
        let picks = self.glossary.picks();
        let active = self.glossary.active.clone();
        if let Some(card) = self.card.as_mut().filter(|c| c.add.is_none()) {
            let first = (picks.first().map(|p| p.0.clone())).unwrap_or_default();
            let into = if active.is_empty() { first } else { active };
            let mut form =
                glossary::AddForm::new(&card.block.original, &into, title.as_deref(), picks);
            if let Some(w) = word {
                form.term = w;
            }
            card.add = Some(form);
        }
    }
}
