//! Translate in place (DESIGN.md "Full-screen translate"): the frozen frame, or
//! the selection in it, is translated on a thread (`translate::pipeline`) and
//! swapped in where it was, top first as the blocks are ready (#27).
//! Hold Alt = the original. Click a translated block = its word card, whose
//! Explain (an explicit click) asks the AI; `A` continues as an Ask AI chat.
use std::sync::Arc;

use iced::widget::image;
use iced::{Point, Rectangle, Size, Task, task};

use crate::actions::ask::{self, Chat, Status};
use crate::actions::{self, Crop};
use crate::ai::AiError;
use crate::mt::Pair;
use crate::ocr::models;
use crate::translate::pipeline::{self, Cache, Deps, Error, Translation};
use crate::ui::background;
use crate::ui::kit::panel_frame as card;
use crate::ui::picker::{self, Picker};

/// The language list under the pill (two columns of names).
const PICKER_LIST: f32 = 360.0;
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
    /// Declined the download, or closed an error: the overlay closes.
    Close,
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
    Explained(Result<(), String>),
    /// The word card's Ask AI (or `A`): continue as a chat.
    Ask,
    /// The AI is not set up: open Settings.
    OpenSettings,
    /// The card's frame (`kit::panel_frame`): the App moves, resizes and
    /// closes it, as for the other result cards.
    Card(card::Event),
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
}

/// The card of one clicked block.
struct Card {
    block: Translation,
    /// The block (with a little context) from the original frame: what Explain sends.
    crop: Arc<Crop>,
    chat: Chat,
    work: Option<task::Handle>,
    /// The target language is right-to-left: the answer is right-aligned.
    rtl: bool,
    /// Its heading: "translation · original".
    title: String,
}

impl Session {
    /// Translates `area` of `frame` from the configured source into the
    /// configured target. Asks first for what is missing: the OCR models
    /// (`models` = present), or, when translation will be offline, its
    /// models with OCR's in one download. The whole frame is all of it
    /// (full screen means the full screen; only live reads the window).
    pub fn start(
        frame: image::Handle,
        area: [u32; 4],
        scale: f32,
        cache: Arc<Cache>,
        models: bool,
    ) -> (Session, Task<Msg>) {
        let cfg = crate::config::current().translate;
        let offline = actions::translate::will_be_offline();
        let size = match &frame {
            image::Handle::Rgba { width, height, .. } => (*width, *height),
            _ => (0, 0),
        };
        let whole = area == [0, 0, size.0, size.1];
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
        };
        let task = session.begin(models);
        (session, task)
    }

    /// Asks for missing models, or translates. A few file sizes: cheap
    /// enough here. A missing API key is only found on the thread
    /// (keyring), which then asks (`Msg::Offline`).
    fn begin(&mut self, models: bool) -> Task<Msg> {
        let (source, target) = (&self.picker.source, &self.picker.target);
        let offline = actions::translate::surely_offline(source, target)
            .map(|p| (p, p.missing_bytes()))
            .filter(|&(_, b)| b > 0);
        self.card = None;
        self.partial = None;
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
            Msg::Offline(pair, bytes) if matches!(self.phase, Phase::Translating) => {
                self.work = None;
                self.phase = Phase::Consent(Some((pair, bytes)));
            }
            Msg::Picker(msg) => {
                if self.picker.update(msg) {
                    let source = &self.picker.source;
                    let models = models::installed(models::recognizer_for(source));
                    return self.begin(models);
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
                self.phase = match result {
                    Ok(shown) => Phase::Shown(shown),
                    Err(problem) => Phase::Failed(problem),
                };
            }
            Msg::Peek(down) => self.peek = down,
            Msg::Explain => return self.explain(),
            Msg::Delta(piece) => {
                if let Some(card) = &mut self.card {
                    card.chat.delta(&piece);
                }
            }
            Msg::Explained(result) => {
                if let Some(card) = &mut self.card {
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
        if let Some(crop) = actions::crop(pixels, *width, *height, 1.0, area) {
            self.card = Some(Card {
                title: format!("{} · {}", block.translation, block.original),
                block,
                crop: Arc::new(crop),
                chat: Chat::default(),
                work: None,
                rtl: pipeline::is_rtl(&self.picker.target),
            });
        }
    }

    pub fn has_card(&self) -> bool {
        self.card.is_some()
    }

    /// The word card's ×: back to the translation.
    pub fn close_card(&mut self) {
        self.card = None;
    }

    /// Hands the card's crop and conversation to an Ask AI panel.
    pub fn take_chat(&mut self) -> Option<(Crop, Chat)> {
        let card = self.card.take()?;
        Some((Arc::unwrap_or_clone(card.crop), card.chat))
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
        let crop = card.crop.clone();
        let (task, handle) = background(move |tx| {
            let result = ask::setup(&crop).and_then(|setup| {
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

    /// The download (if `download`) and the translation, on a thread.
    /// Offline translation's models are fetched only if the user was asked
    /// for them (`self.offline`); otherwise the thread asks (`Msg::Offline`),
    /// also when the text turned out to need another pair ("auto").
    fn run(&mut self, download: bool) -> Task<Msg> {
        let (frame, area, cache) = (self.frame.clone(), self.area, self.cache.clone());
        let (source, target) = (self.picker.source.clone(), self.picker.target.clone());
        let (agreed, scale) = (self.offline, self.scale);
        let (task, handle) = background(move |tx| {
            let deps = actions::translate::deps_for(&source, &target).map(|(mut real, target)| {
                real.scale = scale;
                (real, target)
            });
            let offline = deps.as_ref().is_ok_and(|(real, _)| real.offline());
            if download {
                let progress = |done: u64, total: u64| {
                    let _ = tx.unbounded_send(Msg::Progress(done as f32 / total.max(1) as f32));
                };
                if let Err(e) = actions::translate::download(&source, agreed, progress) {
                    let _ = tx.unbounded_send(Msg::DownloadFailed(e));
                    return;
                }
                let _ = tx.unbounded_send(Msg::Translating);
            }
            let ask = |pair: &'static Pair| {
                let missing = pair.missing_bytes();
                (missing > 0).then(|| tx.unbounded_send(Msg::Offline(pair, missing)))
            };
            let expected = offline
                .then(|| crate::mt::pair(if source == "auto" { "en" } else { &source }, &target))
                .flatten();
            if expected.and_then(ask).is_some() {
                return;
            }
            let partial = |shown| {
                let _ = tx.unbounded_send(Msg::Partial(shown));
            };
            let result = translate(&frame, area, &cache, &deps, partial);
            // "auto" read another language than expected: ask for its pair
            if result.is_err()
                && let Ok((real, _)) = &deps
                && real.missing().and_then(ask).is_some()
            {
                return;
            }
            let _ = tx.unbounded_send(Msg::Done(result));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }
}

/// Runs the pipeline on `area` of `frame` and pastes the result back into a
/// copy of the frame; `partial` gets just the translated area so far.
/// `deps` = `actions::translate::deps()`. Blocking (OCR helper, HTTP).
fn translate(
    frame: &image::Handle,
    area: [u32; 4],
    cache: &Cache,
    deps: &Result<(impl Deps, String), String>,
    mut partial: impl FnMut(Part),
) -> Result<Shown, Problem> {
    let image::Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = frame
    else {
        return Err(problem("the captured frame is not RGBA".into(), false));
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
    let crop = actions::crop(pixels, *width, *height, 1.0, rect)
        .ok_or_else(|| problem("nothing to translate there".into(), false))?;
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
            Error::Ai(AiError::NotConfigured(_) | AiError::Auth(_) | AiError::Offline(_))
        );
        problem(e.to_string(), setup)
    })?;
    let result = shown(&out);
    let t = out.timings;
    crate::log!(
        "[opencircle] translate {}x{} at {x0},{y0}: ocr {} ms, translate {} ms, render {} ms, \
         swap {} ms, first shown {} ms, total {} ms; {} blocks, {} drawn, {} left \
         (busy background {}, no room {})",
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

mod view;

#[cfg(test)]
mod tests;
