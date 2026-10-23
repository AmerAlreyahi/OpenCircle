//! Translate in place (DESIGN.md "Full-screen translate"): the frozen frame, or
//! the selection in it, is translated on a thread (`translate::pipeline`) and
//! swapped in where it was; nothing is drawn until every block is ready.
//! Hold Alt = the original. Click a translated block = its word card, whose
//! Explain (an explicit click) asks the AI; `A` continues as an Ask AI chat.
use std::sync::Arc;

use iced::widget::{column, container, image, opaque, pin, row, space, text};
use iced::{Element, Fill, Point, Rectangle, Size, Task, task};

use super::{background, bar, panel, widgets};
use crate::actions::ask::{self, Chat, Status};
use crate::actions::{self, Crop};
use crate::ai::AiError;
use crate::ocr;
use crate::translate::pipeline::{self, Cache, Deps, Error, Translation};

/// The word card's width, and its height for placing it (logical px).
const CARD: Size = Size::new(360.0, 220.0);
/// Notes and prompts (consent, errors) at the top.
const NOTE_WIDTH: f32 = 420.0;
/// Context around a block in the image the word card sends (physical px).
const CONTEXT: u32 = 8;

#[derive(Debug, Clone)]
pub enum Msg {
    /// The user agreed to the OCR model download.
    Download,
    /// Declined the download, or closed an error: the overlay closes.
    Close,
    /// Model download, 0..=1.
    Progress(f32),
    /// The models are in place; translating now.
    Translating,
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
    /// No OCR models yet: ask first (the Text panel's prompt).
    Consent,
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
    phase: Phase,
    peek: bool,
    card: Option<Card>,
    cache: Arc<Cache>,
    /// The download / translation; dropping it stops the thread's messages.
    work: Option<task::Handle>,
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
    /// Translates `area` of `frame` (asks for the OCR models first if missing).
    pub fn start(
        frame: image::Handle,
        area: [u32; 4],
        cache: Arc<Cache>,
        models: bool,
    ) -> (Session, Task<Msg>) {
        let mut session = Session {
            frame,
            area,
            phase: Phase::Consent,
            peek: false,
            card: None,
            cache,
            work: None,
        };
        let task = if models {
            session.phase = Phase::Translating;
            session.run(false)
        } else {
            Task::none()
        };
        (session, task)
    }

    /// The whole frame is translated (no selection to show).
    pub fn full(&self) -> bool {
        match &self.frame {
            image::Handle::Rgba { width, height, .. } => self.area == [0, 0, *width, *height],
            _ => false,
        }
    }

    /// What to show instead of the original frame: the translation, unless
    /// Alt is held or it isn't ready.
    pub fn image(&self) -> Option<&image::Handle> {
        match &self.phase {
            Phase::Shown(shown) if !self.peek => Some(&shown.image),
            _ => None,
        }
    }

    /// The App handles `Close`, `OpenSettings` and `Ask` first.
    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Download if matches!(self.phase, Phase::Consent) => {
                self.phase = Phase::Downloading(0.0);
                return self.run(true);
            }
            Msg::Progress(done) if matches!(self.phase, Phase::Downloading(_)) => {
                self.phase = Phase::Downloading(done);
            }
            Msg::Translating => self.phase = Phase::Translating,
            Msg::Done(result) if matches!(self.phase, Phase::Translating) => {
                self.work = None;
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
                rtl: pipeline::is_rtl(&crate::config::current().translate.target),
            });
        }
    }

    pub fn has_card(&self) -> bool {
        self.card.is_some()
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
        let target = crate::config::current().translate.target;
        let question = explain_question(&card.block, &target);
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
    fn run(&mut self, download: bool) -> Task<Msg> {
        let (frame, area, cache) = (self.frame.clone(), self.area, self.cache.clone());
        let (task, handle) = background(move |tx| {
            if download {
                let fetched = ocr::ensure_models(|done, total| {
                    let _ = tx.unbounded_send(Msg::Progress(done as f32 / total.max(1) as f32));
                });
                if let Err(e) = fetched {
                    let problem = Problem {
                        message: e.to_string(),
                        setup: false,
                    };
                    let _ = tx.unbounded_send(Msg::Translating);
                    let _ = tx.unbounded_send(Msg::Done(Err(problem)));
                    return;
                }
                let _ = tx.unbounded_send(Msg::Translating);
            }
            let deps = actions::translate::deps();
            let _ = tx.unbounded_send(Msg::Done(translate(&frame, area, &cache, deps)));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }

    /// The layer over the frame: status, prompts, the word card.
    /// `scale` = frame pixels per logical px.
    pub fn view(&self, scale: f32, screen: Size) -> Element<'_, Msg> {
        let note = |content: Element<'static, Msg>| -> Element<'_, Msg> {
            let surface = widgets::surface(content).padding(20).width(NOTE_WIDTH);
            container(opaque(surface)).center_x(Fill).padding(24).into()
        };
        match &self.phase {
            Phase::Consent => note(panel::consent(Msg::Download, Msg::Close)),
            Phase::Downloading(done) => note(
                column![
                    widgets::body("Downloading the text-recognition model…"),
                    widgets::progress(*done),
                ]
                .spacing(10)
                .into(),
            ),
            Phase::Translating => container(widgets::pill("Translating…"))
                .center_x(Fill)
                .padding(24)
                .into(),
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
                note(
                    column![widgets::error(problem.message.clone()), buttons]
                        .spacing(16)
                        .into(),
                )
            }
            Phase::Shown(_) => match &self.card {
                Some(card) => card_view(card, scale, screen),
                None => space().into(),
            },
        }
    }
}

/// Runs the pipeline on `area` of `frame` and pastes the result back into a
/// copy of the frame. `deps` = `actions::translate::deps()`. Blocking (OCR
/// helper, HTTP).
fn translate(
    frame: &image::Handle,
    area: [u32; 4],
    cache: &Cache,
    deps: Result<(impl Deps, String), String>,
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
    let (deps, target) = deps.map_err(|e| {
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
    let out = pipeline::translate_frame(&crop.rgba, crop.width, crop.height, &target, cache, &deps)
        .map_err(|e| {
            let setup = matches!(e, Error::Ai(AiError::NotConfigured(_) | AiError::Auth(_)));
            problem(e.to_string(), setup)
        })?;
    let t = out.timings;
    eprintln!(
        "[opencircle] translate {}x{}: ocr {} ms, translate {} ms, render {} ms, total {} ms; {} blocks",
        crop.width,
        crop.height,
        t.ocr.as_millis(),
        t.translate.as_millis(),
        t.render.as_millis(),
        t.total.as_millis(),
        out.blocks.len(),
    );
    let mut full = pixels.to_vec();
    paste(&mut full, *width, area, &out.rgba);
    let blocks = (out.blocks.into_iter())
        .map(|mut b| {
            b.bounds[0] += x0 as f32;
            b.bounds[1] += y0 as f32;
            b
        })
        .collect();
    Ok(Shown {
        image: image::Handle::from_rgba(*width, *height, full),
        blocks,
    })
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

fn card_view(card: &Card, scale: f32, screen: Size) -> Element<'_, Msg> {
    let [x, y, w, h] = card.block.bounds.map(|v| v / scale);
    let at = bar::place(
        Rectangle::new(Point::new(x, y), Size::new(w, h)),
        CARD,
        screen,
    );
    // Mixed lines ("«name» ...") would each pick their own side.
    let align = if card.rtl {
        text::Alignment::Right
    } else {
        text::Alignment::Default
    };
    let mut body = column![
        widgets::caption(&card.block.original).width(Fill),
        widgets::heading(&card.block.translation)
            .width(Fill)
            .align_x(align),
    ]
    .spacing(8);
    if let Some(turn) = card.chat.turns.last() {
        body = body.push(
            if turn.answer.is_empty() && card.chat.status == Status::Streaming {
                widgets::caption("Thinking…").width(Fill)
            } else {
                widgets::body(&turn.answer).width(Fill).align_x(align)
            },
        );
    }
    if let Status::Failed(e) = &card.chat.status {
        body = body.push(widgets::error(e));
    }
    let ask = &actions::ask::ACTION;
    let buttons = row![
        widgets::text_button(ask.icon, "Explain", "E".into(), true, Msg::Explain),
        widgets::text_button(ask.icon, ask.label, "A".into(), false, Msg::Ask),
    ]
    .spacing(8);
    let surface = widgets::surface(body.push(buttons).spacing(12))
        .padding(16)
        .width(CARD.width);
    // opaque: a click on the card is not a click on the frame below.
    pin(opaque(surface)).position(at).into()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            phase: if models {
                Phase::Translating
            } else {
                Phase::Consent
            },
            peek: false,
            card: None,
            cache: Arc::new(Cache::new(8)),
            work: None,
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
        assert!(matches!(&s.phase, Phase::Consent));
        // nothing runs before the user agrees
        let _ = s.update(shown(vec![]));
        assert!(matches!(&s.phase, Phase::Consent));
        let _ = s.update(Msg::Progress(0.5));
        assert!(matches!(&s.phase, Phase::Consent));
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
        let original = frame(200, 100);
        let cache = Cache::new(8);
        let area = [100, 50, 200, 100];
        let ok = |a: Result<&'static str, AiError>| Ok((Fake(a), "ar".to_string()));

        // no key: the steps' first line, Settings offered
        let no_key: Result<(Fake, String), String> =
            Err("AI is not set up yet: no API key is saved.\n\n1. ...".into());
        let e = translate(&original, area, &cache, no_key).unwrap_err();
        let first = "AI is not set up yet: no API key is saved.";
        assert_eq!(e, problem(first.into(), true));
        // the provider fails: an error, nothing painted; a rejected key: Settings
        let e = translate(&original, area, &cache, ok(Err(AiError::Server(503)))).unwrap_err();
        assert!(!e.setup && e.message.contains("503"), "{e:?}");
        let e = translate(&original, area, &cache, ok(Err(AiError::Auth(401)))).unwrap_err();
        assert!(e.setup);

        let shown = translate(&original, area, &cache, ok(Ok("خروج"))).unwrap();
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
