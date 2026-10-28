//! Translate in place (DESIGN.md "Full-screen translate"): the frozen frame, or
//! the selection in it, is translated on a thread (`translate::pipeline`) and
//! swapped in where it was, top first as the blocks are ready (#27).
//! Hold Alt = the original. Click a translated block = its word card, whose
//! Explain (an explicit click) asks the AI; `A` continues as an Ask AI chat.
use std::sync::Arc;

use iced::widget::{column, container, image, opaque, pin, row, stack, text};
use iced::{Element, Fill, Point, Rectangle, Size, Task, task};

use super::picker::{self, Picker};
use super::{background, panel, widgets};
use crate::actions::ask::{self, Chat, Status};
use crate::actions::{self, Crop};
use crate::ai::AiError;
use crate::mt::Pair;
use crate::ocr::models;
use crate::translate::pipeline::{self, Cache, Deps, Error, Translation};
use crate::ui::kit::panel_frame as card;

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
    /// The card's frame (`widgets::card`): the App moves, resizes and
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
    /// No OCR models for the source yet: ask first (the Text panel's
    /// prompt). `Some`: an offline pair's models (and OCR's, if missing)
    /// instead, with the bytes to download.
    Consent(Option<(&'static Pair, u64)>),
    Downloading(f32),
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
            Msg::Download if matches!(self.phase, Phase::Consent(_)) => {
                self.offline = match self.phase {
                    Phase::Consent(Some((pair, _))) => Some(pair),
                    _ => None,
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
                    let problem = Problem {
                        message: e,
                        setup: false,
                    };
                    let _ = tx.unbounded_send(Msg::Translating);
                    let _ = tx.unbounded_send(Msg::Done(Err(problem)));
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

    /// The layer over the frame: status and the language picker, and the
    /// floating card (`widgets::card`, at `at`, like every result card) for
    /// prompts, errors and the word card. `hover`: on the card's resize edge.
    pub fn view(&self, screen: Size, at: Rectangle, hover: bool) -> Element<'_, Msg> {
        // a prompt is as tall as its text and buttons, not a full-height slab
        let card = |body: Element<'static, Msg>,
                    footer: Option<Element<'static, Msg>>,
                    fit: Option<(usize, f32)>| {
            let at = fit.map_or(at, |(chars, below)| card::short(at, screen, chars, below));
            let header = card::Header {
                glyph: actions::translate::ACTION.icon,
                title: actions::translate::ACTION.label,
                chip: None,
            };
            panel::card(header, body, footer, at, screen, hover, Msg::Card)
        };
        // the pill stays beside the card
        let with_pill = |card: Element<'static, Msg>| -> Element<'_, Msg> {
            let free = card::free(screen, Some(at));
            let pill = container(self.languages(None)).width(free.width);
            stack![pin(pill).position(free.position()), card].into()
        };
        match &self.phase {
            Phase::Consent(None) => card(
                panel::consent(Msg::Download, Msg::Close),
                None,
                Some((90, 60.0)),
            ),
            Phase::Consent(Some((pair, bytes))) => {
                card(offline_consent(pair, *bytes), None, Some((175, 60.0)))
            }
            Phase::Downloading(done) => card(
                column![
                    widgets::body(if self.offline.is_some() {
                        "Downloading offline translation…"
                    } else {
                        "Downloading the text-recognition model…"
                    }),
                    widgets::progress(*done),
                ]
                .spacing(10)
                .into(),
                None,
                Some((45, 16.0)),
            ),
            Phase::Translating => self.languages(Some("Translating…")),
            Phase::Failed(problem) => {
                let mut buttons = row![].spacing(8);
                if problem.setup {
                    buttons = buttons.push(widgets::text_button(
                        actions::ask::ACTION.icon,
                        "Set up AI",
                        String::new(),
                        true,
                        Msg::OpenSettings,
                    ));
                }
                buttons = buttons.push(widgets::text_button(
                    widgets::CLOSE,
                    "Close",
                    "Esc".into(),
                    !problem.setup,
                    Msg::Close,
                ));
                // the picker stays: another pair may work offline
                let body = widgets::error(problem.message.clone()).into();
                with_pill(card(
                    body,
                    Some(buttons.into()),
                    Some((problem.message.chars().count(), 77.0)),
                ))
            }
            Phase::Shown(_) => match &self.card {
                Some(word) => {
                    let (body, footer) = card_view(word);
                    with_pill(card(body, Some(footer), None))
                }
                None => self.languages(None),
            },
        }
    }

    /// The pill at the top: source → target (the picker), and `status`.
    fn languages(&self, status: Option<&'static str>) -> Element<'_, Msg> {
        let mut top = row![self.picker.chips(picker::CHIP_HEIGHT).map(Msg::Picker)]
            .spacing(12)
            .align_y(iced::Center);
        if let Some(status) = status {
            top = top.push(widgets::body(status));
        }
        let mut body = column![top].spacing(picker::SPACING);
        if let Some(list) = self.picker.list(PICKER_LIST) {
            body = body.push(list.map(Msg::Picker));
        }
        let surface = widgets::docked(container(body).padding(iced::Padding::ZERO.right(8)));
        container(opaque(surface)).center_x(Fill).padding(24).into()
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

/// The offline download question (`panel::consent`'s layout, plus Set up AI
/// as the other way to translate).
fn offline_consent(pair: &Pair, bytes: u64) -> Element<'static, Msg> {
    let mb = bytes.div_ceil(10_000_000) * 10; // "about 120 MB"
    column![
        widgets::body(format!(
            "Download {} (about {mb} MB) from Hugging Face and GitHub? \
             It needs no key and nothing you translate leaves your PC.",
            pair.label()
        )),
        row![
            widgets::text_button(
                '\u{e0b6}', // download
                "Download",
                String::new(),
                true,
                Msg::Download
            ),
            widgets::text_button(
                actions::ask::ACTION.icon,
                "Set up AI",
                String::new(),
                false,
                Msg::OpenSettings
            ),
            widgets::text_button(widgets::CLOSE, "Cancel", String::new(), false, Msg::Close),
        ]
        .spacing(8),
    ]
    .spacing(16)
    .into()
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

/// The word card's body (original, translation, explanation) and footer.
fn card_view(card: &Card) -> (Element<'static, Msg>, Element<'static, Msg>) {
    // Mixed lines ("«name» ...") would each pick their own side.
    let align = if card.rtl {
        text::Alignment::Right
    } else {
        text::Alignment::Default
    };
    let mut body = column![
        widgets::caption(card.block.original.clone()).width(Fill),
        widgets::heading(card.block.translation.clone())
            .width(Fill)
            .align_x(align),
    ]
    .spacing(8);
    if let Some(turn) = card.chat.turns.last() {
        body = body.push(
            if turn.answer.is_empty() && card.chat.status == Status::Streaming {
                widgets::caption("Thinking…").width(Fill)
            } else {
                widgets::body(turn.answer.clone())
                    .width(Fill)
                    .align_x(align)
            },
        );
    }
    if let Status::Failed(e) = &card.chat.status {
        body = body.push(widgets::error(e.clone()));
    }
    let ask = &actions::ask::ACTION;
    let buttons = row![
        widgets::text_button(ask.icon, "Explain", "E".into(), true, Msg::Explain),
        widgets::text_button(ask.icon, ask.label, "A".into(), false, Msg::Ask),
    ]
    .spacing(8);
    (body.spacing(12).into(), buttons.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr;
    use iced::widget::container as c;

    fn block(text: &str, tr: &str, bounds: [f32; 4]) -> Translation {
        Translation {
            bounds,
            original: text.into(),
            translation: tr.into(),
        }
    }

    fn frame(w: u32, h: u32) -> image::Handle {
        image::Handle::from_rgba(w, h, vec![7u8; (w * h * 4) as usize])
    }

    fn shown(blocks: Vec<Translation>) -> Msg {
        Msg::Done(Ok(Shown {
            image: frame(4, 4),
            blocks,
        }))
    }

    /// A session as `start` leaves it, without a thread.
    fn session(models: bool) -> Session {
        Session {
            frame: frame(200, 100),
            area: [0, 0, 200, 100],
            whole: true,
            phase: if models {
                Phase::Translating
            } else {
                Phase::Consent(None)
            },
            peek: false,
            card: None,
            cache: Arc::new(Cache::new(8)),
            work: None,
            offline: None,
            picker: Picker::new("auto", "ar", false),
            scale: 1.0,
            partial: None,
        }
    }

    /// A redraw of part of the screen (a hovered button, the progress bar)
    /// leaves the rest as it was. Before: the card's shadow was drawn whole,
    /// outside the redrawn part too (tiny-skia doesn't clip shadows), so it
    /// piled up into a black block over the download prompt (VMs, 1125x575
    /// and 1280x800).
    #[test]
    fn a_partial_redraw_leaves_the_rest_of_the_download_prompt() {
        let mut s = session(false);
        s.phase = Phase::Consent(Some((crate::mt::pair("en", "ar").unwrap(), 160_000_000)));
        for (w, h) in [(1280.0, 800.0), (1125.0, 575.0)] {
            let size = Size::new(w, h);
            let at = card::place(size, None, None);
            // the Download button's row
            let damage = Rectangle::new(Point::new(at.x + 20.0, 150.0), Size::new(140.0, 60.0));
            let (first, again) = widgets::shot::render_twice(
                s.view(size, at, false),
                w as u32,
                h as u32,
                Some(damage),
            );
            let outside = |i: usize| {
                let p = Point::new((i % w as usize) as f32, (i / w as usize) as f32);
                !damage.expand(1.0).contains(p)
            };
            let changed = (first.chunks(4).zip(again.chunks(4)).enumerate())
                .filter(|&(i, (a, b))| a != b && outside(i))
                .count();
            assert_eq!(changed, 0, "{w}x{h}: {changed} pixels changed outside");
        }
    }

    /// The download question, its progress and an error are short cards:
    /// as tall as their text and buttons, on any screen down to 1125x575 (a
    /// narrow one too), every text and button inside the window, nothing
    /// painted outside the card but its shadow. Before: a full-height slab
    /// of empty dark panel with a few lines at its top (the "black block").
    #[test]
    fn the_prompts_are_short_cards_on_small_screens() {
        let mut s = session(false);
        let phases = [
            ("consent", Phase::Consent(None)),
            (
                "offline consent",
                Phase::Consent(Some((crate::mt::pair("en", "ar").unwrap(), 160_000_000))),
            ),
            ("downloading", Phase::Downloading(0.4)),
            (
                "failed",
                Phase::Failed(problem(
                    "The AI provider has a problem right now (HTTP 503). Try again in a minute, \
                     or pick another provider in Settings."
                        .into(),
                    true,
                )),
            ),
        ];
        let backdrop = iced::Color::from_rgb8(240, 240, 240);
        for (name, phase) in phases {
            s.phase = phase;
            for (w, h) in [
                (1125.0, 575.0),
                (1280.0, 800.0),
                (1920.0, 1080.0),
                (700.0, 500.0),
                (600.0, 1000.0),
            ] {
                let size = Size::new(w, h);
                let at = card::place(size, None, None);
                let lost = widgets::shot::out_of_reach(s.view(size, at, false), size);
                assert!(lost.is_empty(), "{name} at {w}x{h}: out of reach {lost:?}");
                let view = c(s.view(size, at, false))
                    .style(move |_| c::Style::default().background(backdrop));
                let px = widgets::shot::save(
                    &format!("prompt-{}-{w}x{h}", name.replace(' ', "-")),
                    view.into(),
                    size,
                );
                let at_px =
                    |x: f32, y: f32| &px[((y as usize * w as usize) + x as usize) * 4..][..3];
                let dark = |p: &[u8]| p.iter().all(|&v| v < 100);
                // the card's rows: dark panel at a point right of its middle
                let x = at.x + at.width * 0.9;
                let rows: Vec<f32> = (0..h as usize)
                    .map(|y| y as f32)
                    .filter(|&y| dark(at_px(x, y)))
                    .collect();
                assert!(!rows.is_empty(), "{name} at {w}x{h}: no card");
                let (top, bottom) = (rows[0], rows[rows.len() - 1]);
                assert!(
                    bottom - top < 330.0 && bottom - top < 0.7 * h,
                    "{name} at {w}x{h}: a slab {}px tall",
                    bottom - top
                );
                // ink down to its last quarter at least: no big empty panel
                let panel = at_px(x, top + 40.0).to_vec();
                let last = (top as usize..=bottom as usize - 6)
                    .filter(|&y| {
                        (at.x as usize + 24..(at.x + at.width) as usize - 24)
                            .any(|px_x| at_px(px_x as f32, y as f32) != &panel[..])
                    })
                    .max()
                    .unwrap();
                assert!(
                    bottom - last as f32 <= 80.0,
                    "{name} at {w}x{h}: {}px of empty panel under the content",
                    bottom - last as f32
                );
                // nothing outside the card, 14 px for its shadow, but the backdrop
                let b = backdrop.into_rgba8();
                for (qx, qy) in [(at.x - 20.0, top + 30.0), (x, bottom + 20.0)] {
                    if qx >= 0.0 && qy < h {
                        assert_eq!(at_px(qx, qy), &b[..3], "{name} at {w}x{h}: ({qx}, {qy})");
                    }
                }
            }
        }
    }

    #[test]
    fn translating_then_shown_then_peek() {
        let mut s = session(true);
        assert!(s.full());
        assert!(matches!(&s.phase, Phase::Translating));
        assert!(s.image().is_none(), "the original until it is ready");
        let _ = s.update(shown(vec![block("Quit", "خروج", [10.0, 10.0, 50.0, 20.0])]));
        assert!(matches!(&s.phase, Phase::Shown(_)));
        let translated = s.image().cloned().unwrap();
        let _ = s.update(Msg::Peek(true));
        assert!(s.image().is_none(), "Alt: the original");
        let _ = s.update(Msg::Peek(false));
        assert_eq!(s.image(), Some(&translated));
        // a late result changes nothing
        let _ = s.update(Msg::Done(Err(problem("late".into(), false))));
        assert_eq!(s.image(), Some(&translated));
    }

    /// #27: what is translated so far shows while the rest is translated;
    /// an error afterwards goes back to the original.
    #[test]
    fn parts_show_while_translating() {
        let mut s = session(true);
        let part = Part {
            image: frame(2, 2),
            area: [1, 1, 3, 3],
        };
        let _ = s.update(Msg::Partial(part.clone()));
        assert!(
            s.image().is_none(),
            "the frame stays; the part goes over it"
        );
        assert_eq!(s.part(), Some((&part.image, [1, 1, 3, 3])));
        let _ = s.update(Msg::Peek(true));
        assert!(s.part().is_none(), "Alt: the original");
        let _ = s.update(Msg::Peek(false));
        let _ = s.update(Msg::Done(Err(problem("helper stopped".into(), false))));
        assert!(s.part().is_none(), "failed: the original");
        let _ = s.update(Msg::Partial(part));
        assert!(s.part().is_none(), "a late part changes nothing");
    }

    #[test]
    fn errors_keep_the_original_frame() {
        for p in [
            problem("AI is not set up yet: no API key is saved.".into(), true),
            problem(
                "The AI provider has a problem right now (HTTP 503).".into(),
                false,
            ),
        ] {
            let mut s = session(true);
            let _ = s.update(Msg::Done(Err(p.clone())));
            assert!(matches!(&s.phase, Phase::Failed(f) if *f == p));
            assert!(s.image().is_none(), "nothing painted");
            s.click(Point::new(20.0, 20.0));
            assert!(!s.has_card());
        }
    }

    #[test]
    fn missing_models_ask_first() {
        let mut s = session(false);
        assert!(matches!(&s.phase, Phase::Consent(None)));
        // nothing runs before the user agrees
        let _ = s.update(shown(vec![]));
        assert!(matches!(&s.phase, Phase::Consent(None)));
        let _ = s.update(Msg::Progress(0.5));
        assert!(matches!(&s.phase, Phase::Consent(None)));
        let _ = s.update(Msg::Offline(en_ar(), 1));
        assert!(
            matches!(&s.phase, Phase::Consent(None)),
            "only while translating"
        );
    }

    fn en_ar() -> &'static Pair {
        crate::mt::pair("en", "ar").unwrap()
    }

    #[test]
    fn offline_models_are_asked_for_with_their_size() {
        // the thread found translation will be offline, without its models
        let mut s = session(true);
        let _ = s.update(Msg::Offline(en_ar(), 117_600_822));
        assert!(matches!(&s.phase, Phase::Consent(Some((_, 117_600_822)))));
        assert!(s.work.is_none(), "the thread is done");
        let texts = widgets::shot::texts(s.view(
            Size::new(1920.0, 1080.0),
            card::place(Size::new(1920.0, 1080.0), None, None),
            false,
        ));
        assert!(
            texts
                .iter()
                .any(|t| t
                    .starts_with("Download English → Arabic offline translation (about 120 MB)")),
            "{texts:?}"
        );
        for button in ["Download", "Set up AI", "Cancel"] {
            assert!(texts.iter().any(|t| t == button), "{button}: {texts:?}");
        }
        // Download: that pair is the one fetched
        s.offline = Some(en_ar());
        s.phase = Phase::Downloading(0.3);
        let texts = widgets::shot::texts(s.view(
            Size::new(1920.0, 1080.0),
            card::place(Size::new(1920.0, 1080.0), None, None),
            false,
        ));
        assert!(
            texts
                .iter()
                .any(|t| t == "Downloading offline translation…")
        );
    }

    /// Light theme over a dark frame: the consent and the download progress
    /// are opaque panels (`panel_solid`), their text the theme's; before,
    /// the see-through glass turned dark there and hid the label.
    #[test]
    fn notes_are_opaque_in_the_light_theme() {
        use iced::widget::container as c;
        crate::ui::theme::set_light(true);
        let palette = crate::ui::theme::current();
        let (text, solid) = (palette.text, crate::ui::kit::themes::c().widget);
        let mut s = session(false);
        for phase in [Phase::Consent(None), Phase::Downloading(0.5)] {
            s.phase = phase;
            let dark = c(s.view(
                Size::new(720.0, 300.0),
                card::place(Size::new(720.0, 300.0), None, None),
                false,
            ))
            .style(|_| c::Style::default().background(iced::Color::from_rgb8(10, 12, 30)));
            let px = widgets::shot::render(dark.into(), 720, 300);
            let at = |x: usize, y: usize| &px[(y * 720 + x) * 4..][..3];
            let want = solid.into_rgba8();
            assert_eq!(at(360, 262), &want[..3], "the panel is solid");
            // and the label is drawn in the theme's text colour
            let ink = text.into_rgba8();
            let near = |p: &[u8]| p.iter().zip(&ink).all(|(a, b)| a.abs_diff(*b) < 40);
            assert!(px.chunks(4).any(near), "a visible label");
        }
        crate::ui::theme::set_light(false);
    }

    /// The "Translating…" pill holds the picker; a new language translates
    /// again (here: its recognizer is missing, so it asks first). PNGs via
    /// the headless renderer (`OPENCIRCLE_RENDER_DIR=<dir>`).
    #[test]
    fn the_pill_picks_languages_and_translates_again() {
        use crate::ui::picker::{Msg as P, Side};
        let mut s = session(true);
        let size = Size::new(720.0, 420.0);
        let texts = widgets::shot::texts(s.view(size, card::place(size, None, None), false));
        for t in ["Detect", "Arabic", "Translating…"] {
            assert!(texts.iter().any(|x| x == t), "{t}: {texts:?}");
        }
        widgets::shot::save(
            "translate-pill",
            s.view(size, card::place(size, None, None), false),
            size,
        );
        let _ = s.update(Msg::Picker(P::Open(Side::Target)));
        widgets::shot::save(
            "translate-pill-picker",
            s.view(size, card::place(size, None, None), false),
            size,
        );
        let texts = widgets::shot::texts(s.view(size, card::place(size, None, None), false));
        assert!(texts.iter().any(|t| t == "English"), "{texts:?}");
        // the same target again: nothing restarts
        let _ = s.update(Msg::Picker(P::Pick("ar".into())));
        assert!(matches!(s.phase, Phase::Translating));
        // a shown translation keeps the pill; a new target starts over
        let _ = s.update(shown(vec![block("Quit", "خروج", [10.0, 10.0, 50.0, 20.0])]));
        let texts = widgets::shot::texts(s.view(size, card::place(size, None, None), false));
        assert!(texts.iter().any(|t| t == "Arabic") && !texts.iter().any(|t| t == "Translating…"));
        let _ = s.update(Msg::Picker(P::Open(Side::Source)));
        let _ = s.update(Msg::Picker(P::Query("kyr".into())));
        assert!(!s.picker.update(P::First), "nothing matches: no change");
        let _ = s.update(Msg::Picker(P::Query("".into())));
        // a source whose recognizer is (in this sandbox) missing: ask first
        let _ = s.update(Msg::Picker(P::Pick("zh".into())));
        assert_eq!(s.picker.label(), "ZH → AR");
        let ready = models::installed(models::recognizer_for("zh"));
        assert!(ready || matches!(s.phase, Phase::Consent(None)));
        assert!(s.image().is_none(), "the old translation is gone");
    }
    /// OCR finds one line at (20, 10) of what it is given; AI answers or fails.
    struct Fake(Result<&'static str, AiError>);
    impl Deps for Fake {
        fn recognize(
            &self,
            _: &[u8],
            _: u32,
            _: u32,
        ) -> Result<Vec<ocr::TextBlock>, ocr::OcrError> {
            Ok(vec![ocr::TextBlock {
                text: "Quit".into(),
                bounds: [20.0, 10.0, 40.0, 20.0],
                confidence: 0.9,
            }])
        }
        fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
            let answer = self.0.clone()?;
            Ok(texts.iter().map(|_| answer.to_string()).collect())
        }
    }

    fn pixels(h: &image::Handle) -> &[u8] {
        match h {
            image::Handle::Rgba { pixels, .. } => pixels,
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_result_is_pasted_in_place_or_nothing_is() {
        // "glyphs" where OCR finds "Quit": (20, 10) of the area
        let mut px = vec![7u8; 200 * 100 * 4];
        for y in 64..76 {
            for x in (120..160).step_by(3) {
                px[(y * 200 + x) * 4..][..3].copy_from_slice(&[230; 3]);
            }
        }
        let original = image::Handle::from_rgba(200, 100, px);
        let cache = Cache::new(8);
        let area = [100, 50, 200, 100];
        let ok = |a: Result<&'static str, AiError>| Ok((Fake(a), "ar".to_string()));

        // no key: the steps' first line, Settings offered
        let no_key: Result<(Fake, String), String> =
            Err("AI is not set up yet: no API key is saved.\n\n1. ...".into());
        let e = translate(&original, area, &cache, &no_key, |_| {}).unwrap_err();
        let first = "AI is not set up yet: no API key is saved.";
        assert_eq!(e, problem(first.into(), true));
        // the provider fails: an error, nothing painted; a rejected key: Settings
        let e = translate(
            &original,
            area,
            &cache,
            &ok(Err(AiError::Server(503))),
            |_| {},
        )
        .unwrap_err();
        assert!(!e.setup && e.message.contains("503"), "{e:?}");
        let e = translate(
            &original,
            area,
            &cache,
            &ok(Err(AiError::Auth(401))),
            |_| {},
        )
        .unwrap_err();
        assert!(e.setup);

        let shown = translate(&original, area, &cache, &ok(Ok("خروج")), |_| {}).unwrap();
        // blocks in frame pixels: OCR saw the crop, offset by the area's corner
        let b = &shown.blocks[0];
        assert_eq!(
            (b.original.as_str(), b.translation.as_str()),
            ("Quit", "خروج")
        );
        assert!((b.bounds[0] - 118.0).abs() < 0.01 && (b.bounds[1] - 57.0).abs() < 0.01);
        // only the area changed
        let (before, after) = (pixels(&original), pixels(&shown.image));
        assert_eq!(before.len(), after.len());
        let changed: Vec<usize> = (0..before.len() / 4)
            .filter(|i| before[i * 4..i * 4 + 4] != after[i * 4..i * 4 + 4])
            .collect();
        assert!(!changed.is_empty());
        let inside = |i: &usize| i % 200 >= 100 && i / 200 >= 50;
        assert!(changed.iter().all(inside), "only inside the area");
    }

    /// Two lines far apart, one sentence per call: the first part is shown
    /// before the second is translated.
    struct Two;
    impl Deps for Two {
        fn recognize(
            &self,
            _: &[u8],
            _: u32,
            _: u32,
        ) -> Result<Vec<ocr::TextBlock>, ocr::OcrError> {
            Ok(["Quit", "Play"]
                .into_iter()
                .zip([10.0, 100.0])
                .map(|(text, y)| ocr::TextBlock {
                    text: text.into(),
                    bounds: [20.0, y, 40.0, 20.0],
                    confidence: 0.9,
                })
                .collect())
        }
        fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
            Ok(texts.iter().map(|_| "خروج".to_string()).collect())
        }
        fn batch(&self) -> usize {
            1
        }
    }

    /// Audit 12: a partial is just the translated area (never a copy of the whole
    /// frame), and it is the same picture the full render has there.
    #[test]
    fn a_partial_is_the_area_only_and_matches_the_full_render() {
        let (w, h) = (300u32, 200u32);
        let px: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
        let original = image::Handle::from_rgba(w, h, px);
        let area = [100, 50, 300, 200];
        let (cw, ch) = (area[2] - area[0], area[3] - area[1]);
        let mut parts = Vec::new();
        let deps: Result<(Two, String), String> = Ok((Two, "ar".into()));
        let done = translate(&original, area, &Cache::new(8), &deps, |p| parts.push(p)).unwrap();
        assert_eq!(parts.len(), 1, "the first line is shown before the second");
        let part = &parts[0];
        assert_eq!(part.area, area);
        let (frame_len, part_len) = (pixels(&original).len(), pixels(&part.image).len());
        assert_eq!(part_len, (cw * ch * 4) as usize);
        assert!(part_len < frame_len, "not a frame-sized picture");
        // the part is the full render's crop where only the first line differs
        // (the rows above the second line)
        let full = pixels(&done.image);
        let row = (cw * 4) as usize;
        for y in 0..90usize {
            let at = ((area[1] as usize + y) * w as usize + area[0] as usize) * 4;
            assert_eq!(
                pixels(&part.image)[y * row..(y + 1) * row],
                full[at..at + row],
                "row {y}"
            );
        }
        // and the second line is not drawn yet in the part
        let at = ((area[1] as usize + 100) * w as usize + area[0] as usize) * 4;
        assert_ne!(
            pixels(&part.image)[100 * row..110 * row],
            full[at..at + 10 * row]
        );
    }

    #[test]
    fn clicks_hit_translated_blocks_only() {
        let blocks = vec![
            block("Inventory", "المخزون", [0.0, 0.0, 100.0, 40.0]),
            block("100 / 250", "", [0.0, 50.0, 100.0, 20.0]),
            block("Potion", "جرعة", [10.0, 5.0, 30.0, 10.0]), // inside the first
        ];
        assert_eq!(hit(&blocks, Point::new(80.0, 30.0)), Some(0));
        assert_eq!(hit(&blocks, Point::new(20.0, 10.0)), Some(2), "smallest");
        assert_eq!(hit(&blocks, Point::new(20.0, 60.0)), None, "not translated");
        assert_eq!(hit(&blocks, Point::new(150.0, 10.0)), None);
        assert_eq!(hit(&blocks, Point::new(100.0, 10.0)), None, "end exclusive");

        let mut s = session(true);
        let _ = s.update(shown(blocks));
        s.click(Point::new(80.0, 30.0));
        let (crop, chat) = s.take_chat().unwrap();
        assert_eq!(
            (crop.width, crop.height),
            (108, 48),
            "block + context, clipped"
        );
        assert!(chat.turns.is_empty(), "nothing is sent before Explain");
        s.click(Point::new(80.0, 30.0));
        // the word card is the shared floating card, the pill beside it
        let size = Size::new(1280.0, 720.0);
        let view = s.view(size, card::place(size, None, None), false);
        let texts = widgets::shot::texts(view);
        for t in ["Translate", "Inventory", "المخزون", "Explain"] {
            assert!(texts.iter().any(|x| x == t), "{t}: {texts:?}");
        }
        let view = s.view(size, card::place(size, None, None), false);
        widgets::shot::save("translate-word-card", view, size);
        s.click(Point::new(150.0, 90.0));
        assert!(!s.has_card(), "a click elsewhere closes the card");
    }

    #[test]
    fn explain_streams_into_the_card() {
        let mut s = session(true);
        let _ = s.update(shown(vec![block(
            "Sunstone",
            "حجر الشمس",
            [0.0, 0.0, 50.0, 20.0],
        )]));
        s.click(Point::new(5.0, 5.0));
        let card = s.card.as_mut().unwrap();
        // what Explain starts (without the thread)
        card.chat
            .ask(&explain_question(&card.block, "ar"), &[])
            .unwrap();
        let _ = s.update(Msg::Delta("حجر ".into()));
        let _ = s.update(Msg::Delta("نادر".into()));
        let _ = s.update(Msg::Explained(Ok(())));
        let card = s.card.as_ref().unwrap();
        assert_eq!(card.chat.status, Status::Done);
        assert_eq!(card.chat.turns[0].answer, "حجر نادر");
        assert!(card.chat.turns[0].question.contains("\"Sunstone\""));
        // explained already: a second Explain sends nothing
        assert!(s.explain().units() == 0);
    }

    #[test]
    fn paste_puts_the_area_back_in_place() {
        let mut f = vec![0u8; 4 * 3 * 4];
        paste(&mut f, 4, [1, 1, 3, 3], &[9u8; 2 * 2 * 4]);
        let px = |x: usize, y: usize| f[(y * 4 + x) * 4];
        assert_eq!((px(0, 0), px(1, 1), px(2, 2), px(3, 2)), (0, 9, 9, 0));
    }
}
