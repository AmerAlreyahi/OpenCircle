//! One-shot full-screen translate: OCR -> lines -> [`group_blocks`] -> skip
//! what needs no translation -> ONE translate call, AI or offline (cache
//! first) -> [`render`] into a copy of the frame. Live mode (`live.rs`)
//! reuses [`translate_blocks`] and the [`Cache`].

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub use super::lang::is_rtl;
use super::lang::{self, Script};
use super::{Block, Drawn, Line, MARKUP, group_blocks, render};
use crate::ai::{self, AiError};
use crate::config::{AiConfig, TextReader};
use crate::mt;
use crate::ocr::{self, OcrError, TextBlock};
use crate::secrets::Secret;

/// OCR and translation, behind a trait so tests inject fakes. [`Real`] is the app's.
pub trait Deps {
    /// Text lines of an RGBA image, in reading order.
    fn recognize(&self, rgba: &[u8], width: u32, height: u32) -> Result<Vec<TextBlock>, OcrError>;
    /// ONE call: exactly `texts.len()` translations, in order, with
    /// [`MARKUP`] spans (the offline engine never marks any).
    fn translate(&self, texts: &[String], target: &str) -> Result<Vec<String>, AiError>;
    /// About how many sentences the first `translate` call gets when the
    /// results are shown as they come (one-shot; each next call twice as
    /// many); MAX = all in one call.
    fn batch(&self) -> usize {
        usize::MAX
    }
    /// Gets translation into `target` ready while the text is read.
    fn warm(&self, _target: &str) {}
}

/// OCR by Windows OCR where it reads the source (`system_reads`), else in
/// the helper process (the recognizer for `source`); translation by the
/// user's AI provider, or offline (`crate::mt`, also in the helper).
pub struct Real {
    /// `translate.source`: "auto" or a language code.
    pub source: String,
    pub via: Via,
    /// The monitor's scale (Windows OCR reads small text better doubled).
    pub scale: f32,
    /// Offline: the pair a translation needed whose models are missing (the
    /// UI then asks to download it).
    missing: Mutex<Option<&'static mt::Pair>>,
}

pub enum Via {
    Ai {
        ai: AiConfig,
        key: Option<Secret>,
    },
    /// No key, nothing leaves the PC. The pair follows the source and the
    /// target; its models may still be missing (`Real::missing`).
    Offline,
}

impl Real {
    pub fn new(source: &str, via: Via) -> Real {
        Real {
            source: source.into(),
            via,
            scale: 1.0,
            missing: Mutex::new(None),
        }
    }

    /// Windows OCR reads this source (`system_reads`; asks Windows once).
    fn system(&self) -> bool {
        let reader = crate::config::current().selection.text_reader;
        system_reads(reader, ocr::system::language(), &self.source)
    }

    pub fn offline(&self) -> bool {
        matches!(self.via, Via::Offline)
    }

    /// The pair whose download a failed offline translation needs.
    pub fn missing(&self) -> Option<&'static mt::Pair> {
        *self.missing.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The offline pair for `texts` into `target`: the set source, or the
    /// one their script suggests ("auto"). Err = none is available offline
    /// (the message offers AI; nothing is sent anywhere).
    fn pair_for(&self, texts: &[String], target: &str) -> Result<&'static mt::Pair, AiError> {
        let source = match self.source.as_str() {
            lang::AUTO => lang::guess(texts.iter().map(String::as_str)).unwrap_or("en"),
            s => s,
        };
        mt::pair(source, target).ok_or_else(|| AiError::Offline(mt::unavailable(source, target)))
    }

    /// `texts` into `target`, one call; `markup` = extra AI instructions.
    /// Offline, `[[...]]` accent spans can't be placed (no word alignment):
    /// a line drawn wholly in a highlight colour keeps it (that is its own
    /// colour), a highlighted name inside a line is drawn in the line's colour.
    pub fn lines(
        &self,
        texts: &[String],
        target: &str,
        markup: &str,
    ) -> Result<Vec<String>, AiError> {
        match &self.via {
            Via::Ai { ai, key } => {
                ai::translate_lines(ai, key.as_ref(), texts, &self.source, target, markup)
            }
            Via::Offline => {
                let pair = self.pair_for(texts, target)?;
                if pair.missing_bytes() > 0 {
                    *self.missing.lock().unwrap_or_else(|e| e.into_inner()) = Some(pair);
                    return Err(AiError::Offline(format!(
                        "{} isn't downloaded yet. Translate once with Ctrl+Shift+T to \
                         download it, or set up AI in Settings > AI",
                        pair.label()
                    )));
                }
                ocr::keep_warm();
                by_sentence(texts, |t| {
                    mt::translate(t, pair).map_err(|e| AiError::Offline(e.to_string()))
                })
            }
        }
    }
}

/// Windows OCR reads the text (#62: a screen in ~0.1-0.3 s, in this
/// process) where `reader` routes to it for its language `tag`
/// (`ocr::route`: not right-to-left) and `source` is that language or
/// "auto"; else PP-OCR. One-shot and live translate alike (#56, #107).
fn system_reads(reader: TextReader, tag: Option<&str>, source: &str) -> bool {
    let base = |t: &str| {
        t.split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_lowercase()
    };
    ocr::route(reader, Some(tag)) == ocr::Source::System
        && (source == lang::AUTO || tag.is_some_and(|t| base(t) == base(source)))
}

impl Deps for Real {
    fn recognize(&self, rgba: &[u8], width: u32, height: u32) -> Result<Vec<TextBlock>, OcrError> {
        if self.system() {
            let k = *ocr::system::passes(self.scale, width, height)
                .last()
                .unwrap_or(&1);
            match ocr::system::recognize(rgba, width, height, k) {
                Ok(lines) => return Ok(lines),
                Err(e) => crate::log!("[opencircle] Windows OCR failed ({e}); PP-OCR reads"),
            }
        }
        ocr::recognize_as(rgba, width, height, &self.source)
    }

    fn translate(&self, texts: &[String], target: &str) -> Result<Vec<String>, AiError> {
        self.lines(texts, target, MARKUP)
    }

    /// Offline: the top lines first, quickly (a cloud model gets one call: a
    /// round trip each).
    fn batch(&self) -> usize {
        match self.via {
            Via::Offline => FIRST,
            Via::Ai { .. } => usize::MAX,
        }
    }

    /// Offline: loads the likely pair (English for "auto") in the helper,
    /// which stays warm, while the screen is read.
    fn warm(&self, target: &str) {
        let Ok(pair) = self.pair_for(&[], target) else {
            return;
        };
        if self.offline() && pair.missing_bytes() == 0 {
            ocr::keep_warm();
            std::thread::spawn(move || mt::translate(&[], pair));
        }
    }
}

/// Offline MT (Marian) drops or merges sentences of a long input and cuts it
/// at 200 tokens: each text is split into sentences, all of them go in one
/// call, and each text's translations are joined back, so every text gets
/// exactly its own translation, whole.
pub(crate) fn by_sentence(
    texts: &[String],
    translate: impl FnOnce(&[String]) -> Result<Vec<String>, AiError>,
) -> Result<Vec<String>, AiError> {
    let parts: Vec<Vec<String>> = texts.iter().map(|t| sentences(t)).collect();
    let flat: Vec<String> = parts.iter().flatten().cloned().collect();
    let out = translate(&flat)?;
    if out.len() != flat.len() {
        return Err(AiError::BadResponse(format!(
            "expected {} translated sentences, got {}",
            flat.len(),
            out.len()
        )));
    }
    let mut out = out.into_iter();
    Ok(parts
        .iter()
        .map(|p| out.by_ref().take(p.len()).collect::<Vec<_>>().join(" "))
        .collect())
}

/// Sentence ends: `.`, `!` or `?` (and a citation mark after it, "planets.[12]",
/// OCR's "Sedna.1131" for "[13]"), then a space and a capital, a digit, a
/// quote or a bracket; not after an initial or a short abbreviation ("T.
/// Goldsmith", "U.S. Patent", "Dr. Who": a fragment comes back as noise).
/// A sentence longer than [`LONG`] words is cut at its middle comma: Marian
/// translates only the start of a long input (a Wikipedia paragraph whose
/// stops all had citations came back as its first sentence; the rest of the
/// erased paragraph stayed blank).
fn sentences(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, _) in t.match_indices(' ') {
        let next = t[i + 1..].chars().next();
        if !next.is_some_and(|n| n.is_uppercase() || n.is_ascii_digit() || "\"'(“".contains(n)) {
            continue;
        }
        let word = t[start..i].rsplit(' ').next().unwrap_or_default();
        let Some(end) = word.rfind(['.', '!', '?']) else {
            continue;
        };
        let (body, tail) = (&word[..end], &word[end + 1..]);
        let cite = tail.is_empty()
            || (tail.len() <= 6
                && (tail.starts_with('[') || tail.bytes().all(|b| b.is_ascii_digit())));
        let short =
            word[end..].starts_with('.') && (body.chars().count() <= 2 || body.contains('.'));
        if cite && !short {
            out.push(t[start..i].trim().to_string());
            start = i + 1;
        }
    }
    out.extend(
        Some(t[start..].trim())
            .filter(|r| !r.is_empty())
            .map(String::from),
    );
    out.into_iter().flat_map(|s| halves(&s)).collect()
}

/// Words in one offline MT input at most (its 200-token cut is ~130 words;
/// it starts dropping clauses well before that).
const LONG: usize = 45;

/// `s` cut at the comma (or semicolon, colon) nearest its middle until each
/// part has at most [`LONG`] words; a part without one stays whole.
fn halves(s: &str) -> Vec<String> {
    let words = s.split(' ').count();
    if words <= LONG {
        return vec![s.to_string()];
    }
    let mid = s.len() / 2;
    let cut = (s.match_indices([',', ';', ':']).map(|(i, _)| i + 1))
        .filter(|&i| s[i..].starts_with(' '))
        .min_by_key(|&i| i.abs_diff(mid));
    match cut {
        Some(i) => [&s[..i], &s[i + 1..]]
            .iter()
            .flat_map(|p| halves(p.trim()))
            .collect(),
        None => vec![s.to_string()],
    }
}

#[derive(Debug)]
pub enum Error {
    Ocr(OcrError),
    Ai(AiError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Ocr(e) => e.fmt(f),
            Error::Ai(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

/// Wall time per stage (goal: total < 1.5 s on a cloud model).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timings {
    pub ocr: Duration,
    /// Zero when every text came from the cache.
    pub translate: Duration,
    pub render: Duration,
    /// One-shot: when the first translated blocks were ready to show
    /// (`translate_progressively`); zero when nothing was shown early.
    pub first: Duration,
    pub total: Duration,
}

/// One text block on screen, for "click a block -> explanation".
#[derive(Debug, Clone, PartialEq)]
pub struct Translation {
    /// x, y, w, h in frame pixels.
    pub bounds: [f32; 4],
    pub original: String,
    /// Without `[[ ]]`; "" = left as it was (numbers, already in the target
    /// language, or the model returned it unchanged).
    pub translation: String,
}

pub struct Translated {
    /// The frame with every translated block erased and redrawn.
    pub rgba: Vec<u8>,
    pub blocks: Vec<Translation>,
    pub timings: Timings,
    /// Blocks redrawn, and left as they were (busy background, no room).
    pub drawn: Drawn,
}

/// Translates every text block of an RGBA frame in place. An OCR or AI
/// error returns before anything is drawn (never a half-translated frame).
#[cfg(test)]
pub fn translate_frame(
    rgba: &[u8],
    width: u32,
    height: u32,
    target: &str,
    cache: &Cache,
    deps: &impl Deps,
) -> Result<Translated, Error> {
    translate_progressively(rgba, width, height, target, cache, deps, |_| {})
}

/// Offline, the first call's sentences: a release-notes page's top 8 came
/// back in ~0.3 s (4 cores). Each next call gets twice as many: every
/// decoding step costs about the same for 1 line or 32 (`mt::BATCH`), so a
/// call per 8 made the page 1.7x slower, doubling 1.1x.
const FIRST: usize = 8;

/// A partial frame is shown at most this often (each is a whole render):
/// the first one at once, then as the rest comes in, then the whole page.
const SHOW_EVERY: Duration = Duration::from_millis(300);

/// `translate_frame`, showing progress (#27): the blocks are translated top
/// of the screen first, [`Deps::batch`] sentences, then twice as many per
/// call, and `shown` gets
/// the frame with what is translated so far (the first part at once, then at
/// most every [`SHOW_EVERY`]), each drawn from the original as the whole page
/// will be, so nothing moves when the rest arrives. An error after a partial
/// frame still fails the whole (the caller goes back to the original).
pub fn translate_progressively(
    rgba: &[u8],
    width: u32,
    height: u32,
    target: &str,
    cache: &Cache,
    deps: &impl Deps,
    mut shown: impl FnMut(&Translated),
) -> Result<Translated, Error> {
    let start = Instant::now();
    check_frame(rgba, width, height)?;
    deps.warm(target);
    let found = deps.recognize(rgba, width, height).map_err(Error::Ocr)?;
    let lines: Vec<Line> = (found.into_iter())
        .filter(|b| !cut_off(b, (width, height)))
        .map(|b| to_line(b, width, height))
        .collect();
    let ocr = start.elapsed();
    let blocks = group_blocks(&lines);
    let mut tr = vec![String::new(); blocks.len()];
    let mut timings = Timings {
        ocr,
        ..Timings::default()
    };
    let mut last_shown = None::<Instant>;
    let parts = parts(&blocks, deps.batch());
    for (k, part) in parts.iter().enumerate() {
        let t = Instant::now();
        let chunk: Vec<Block> = part.iter().map(|&i| blocks[i].clone()).collect();
        let out = translate_blocks(&chunk, target, cache, deps).map_err(Error::Ai)?;
        timings.translate += t.elapsed();
        for (&i, t) in part.iter().zip(out) {
            tr[i] = t;
        }
        let last = k + 1 == parts.len();
        if last || last_shown.is_some_and(|at| at.elapsed() < SHOW_EVERY) {
            continue;
        }
        last_shown = Some(Instant::now());
        let partial = draw(rgba, (width, height), target, &blocks, &tr, &mut timings);
        if timings.first.is_zero() {
            timings.first = start.elapsed();
        }
        shown(&partial);
    }
    let mut out = draw(rgba, (width, height), target, &blocks, &tr, &mut timings);
    timings.total = start.elapsed();
    out.timings = timings;
    Ok(out)
}

/// The original frame with `tr` (one per block, "" = as it was) drawn in.
fn draw(
    rgba: &[u8],
    (width, height): (u32, u32),
    target: &str,
    blocks: &[Block],
    tr: &[String],
    timings: &mut Timings,
) -> Translated {
    let t = Instant::now();
    let mut out = rgba.to_vec();
    let drawn = render(&mut out, width, height, blocks, tr, is_rtl(target));
    timings.render += t.elapsed();
    Translated {
        rgba: out,
        blocks: translations(blocks, tr),
        drawn: Drawn {
            rooms: Vec::new(),
            ink: Vec::new(),
            ..drawn
        },
        timings: *timings,
    }
}

/// The blocks (indices) in the order they are translated: top of the
/// screen first, in parts of about `batch`, `2 batch`, `4 batch`...
/// sentences (a block is never cut).
fn parts(blocks: &[Block], batch: usize) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by(|&a, &b| blocks[a].bounds[1].total_cmp(&blocks[b].bounds[1]));
    let mut out: Vec<Vec<usize>> = Vec::new();
    let (mut n, mut size) = (0, batch);
    for i in order {
        if out.is_empty() || n >= size {
            if !out.is_empty() {
                size = size.saturating_mul(2);
            }
            out.push(Vec::new());
            n = 0;
        }
        n += sentences(&blocks[i].text).len();
        out.last_mut().expect("just pushed").push(i);
    }
    // each part in reading order (what an AI gets as context)
    out.iter_mut().for_each(|p| p.sort_unstable());
    out
}

pub(super) fn check_frame(rgba: &[u8], width: u32, height: u32) -> Result<(), Error> {
    if width == 0 || height == 0 || rgba.len() as u64 != width as u64 * height as u64 * 4 {
        return Err(Error::Ocr(OcrError::BadImage(format!(
            "{width}x{height} RGBA needs {} bytes, got {}",
            width as u64 * height as u64 * 4,
            rgba.len()
        ))));
    }
    Ok(())
}

/// A line an edge of the frame cuts through (a page scrolled halfway past
/// it, a panel beside the window's edge; its OCR box within 3 px of the
/// edge): OCR read part of it and the erase finds no background beyond it,
/// so it is left as it is.
pub(super) fn cut_off(b: &TextBlock, (width, height): (u32, u32)) -> bool {
    let [x, y, w, h] = b.bounds;
    y <= 3.0 || y + h >= height as f32 - 3.0 || x <= 3.0 || x + w >= width as f32 - 3.0
}

/// An OCR box hugs the ink; `render`'s erase wants the line's box (it reads
/// the background just inside the edge), so pad it like a layout line box:
/// 15% of the height above and below, 10% left and right, inside the frame.
pub(super) fn to_line(b: TextBlock, width: u32, height: u32) -> Line {
    let [x, y, w, h] = b.bounds;
    let (x0, y0) = ((x - 0.1 * h).max(0.0), (y - 0.15 * h).max(0.0));
    let x1 = (x + w + 0.1 * h).min(width as f32);
    let y1 = (y + h + 0.15 * h).min(height as f32);
    Line {
        bounds: [x0, y0, x1 - x0, y1 - y0],
        text: b.text,
    }
}

pub(super) fn translations(blocks: &[Block], tr: &[String]) -> Vec<Translation> {
    blocks
        .iter()
        .zip(tr)
        .map(|(b, t)| Translation {
            bounds: b.bounds,
            original: b.text.clone(),
            translation: unmarked(t),
        })
        .collect()
}

/// One translation per block ("" = keep the original): cache first, then
/// ONE call for the rest (each distinct text once, in reading order).
pub(super) fn translate_blocks(
    blocks: &[Block],
    target: &str,
    cache: &Cache,
    deps: &impl Deps,
) -> Result<Vec<String>, AiError> {
    let keys: Vec<Option<String>> = blocks
        .iter()
        .map(|b| needs_translation(&b.text, target).then(|| normalise(&b.text)))
        .collect();
    let mut found: HashMap<&str, String> = HashMap::new();
    let mut todo: Vec<String> = Vec::new();
    for k in keys.iter().flatten() {
        if found.contains_key(k.as_str()) || todo.contains(k) {
            continue;
        }
        match cache.get(k, target) {
            Some(t) => {
                found.insert(k, t);
            }
            None => todo.push(k.clone()),
        }
    }
    if !todo.is_empty() {
        let out = deps.translate(&todo, target)?;
        if out.len() != todo.len() {
            return Err(AiError::BadResponse(format!(
                "expected {} translated lines, got {}",
                todo.len(),
                out.len()
            )));
        }
        for (k, t) in todo.iter().zip(out) {
            // the model echoing the source (a name, noise) = nothing to redraw
            let t = if normalise(&unmarked(&t)) == *k {
                String::new()
            } else {
                t
            };
            cache.put(k, target, t.clone());
            found.insert(k, t);
        }
    }
    Ok(keys
        .iter()
        .map(|k| {
            k.as_ref()
                .map(|k| found[k.as_str()].clone())
                .unwrap_or_default()
        })
        .collect())
}

/// False for numbers/punctuation only, for lone Latin letters (an icon read
/// as "X" or "C": no word, and MT makes "tenth" of it), and for text already
/// in the target's script (Latin targets can't be told from English: the
/// model decides).
fn needs_translation(text: &str, target: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    let word =
        (text.split(|c: char| !c.is_alphabetic())).any(|w| w.chars().count() > 1 || !w.is_ascii());
    match lang::find(target).map(|l| l.script) {
        _ if !word => false,
        Some(Script::Latin) | None => true,
        Some(script) => letters.iter().filter(|c| script.has(**c)).count() * 2 < letters.len(),
    }
}

fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unmarked(t: &str) -> String {
    t.replace("[[", "").replace("]]", "")
}

type Gen = HashMap<(String, String), String>;

/// Translations by (normalised source text, target), shared by one-shot and
/// live mode (one per app, e.g. in an `Arc`). Thread-safe.
// ponytail: two generations instead of a true LRU: when the current one
// fills, it becomes the old one and the previous old one is dropped; a hit in
// the old one moves back. At most 2 x `cap` entries, O(1), std only.
pub struct Cache {
    cap: usize,
    gens: Mutex<[Gen; 2]>,
}

impl Cache {
    pub fn new(cap: usize) -> Self {
        Cache {
            cap: cap.max(1),
            gens: Mutex::default(),
        }
    }

    pub fn get(&self, text: &str, target: &str) -> Option<String> {
        let key = (normalise(text), target.to_string());
        let mut g = self.gens.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = g[0].get(&key) {
            return Some(t.clone());
        }
        let t = g[1].remove(&key)?;
        self.insert(&mut g, key, t.clone());
        Some(t)
    }

    pub fn put(&self, text: &str, target: &str, translation: String) {
        let mut g = self.gens.lock().unwrap_or_else(|e| e.into_inner());
        self.insert(&mut g, (normalise(text), target.to_string()), translation);
    }

    fn insert(&self, g: &mut [Gen; 2], key: (String, String), t: String) {
        if g[0].len() >= self.cap && !g[0].contains_key(&key) {
            g[1] = std::mem::take(&mut g[0]);
        }
        g[0].insert(key, t);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::cell::RefCell;

    type OcrFn = Box<dyn Fn(&[u8], u32, u32) -> Vec<TextBlock>>;

    /// Records every call; OCR answers with `lines`, AI with `answer` per text.
    pub struct Fake {
        pub lines: OcrFn,
        pub answer: fn(&str) -> String,
        pub fail_ai: bool,
        /// `Deps::batch` (sentences per call when shown as they come).
        pub batch: usize,
        pub ocr_calls: RefCell<Vec<(u32, u32)>>,
        pub ai_calls: RefCell<Vec<Vec<String>>>,
    }

    impl Fake {
        pub fn new(lines: impl Fn(&[u8], u32, u32) -> Vec<TextBlock> + 'static) -> Self {
            Fake {
                lines: Box::new(lines),
                answer: |s| format!("ترجمة {s}"),
                fail_ai: false,
                batch: usize::MAX,
                ocr_calls: RefCell::default(),
                ai_calls: RefCell::default(),
            }
        }
    }

    impl Deps for Fake {
        fn recognize(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<TextBlock>, OcrError> {
            self.ocr_calls.borrow_mut().push((w, h));
            Ok((self.lines)(rgba, w, h))
        }

        fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
            self.ai_calls.borrow_mut().push(texts.to_vec());
            if self.fail_ai {
                return Err(AiError::Server(503));
            }
            Ok(texts.iter().map(|t| (self.answer)(t)).collect())
        }

        fn batch(&self) -> usize {
            self.batch
        }
    }

    pub fn tb(text: &str, bounds: [f32; 4]) -> TextBlock {
        TextBlock {
            text: text.into(),
            bounds,
            confidence: 0.9,
        }
    }

    /// Dark frame with a light bar ("glyphs") under each OCR box.
    pub fn frame(w: u32, h: u32, boxes: &[[f32; 4]]) -> Vec<u8> {
        let mut img = [20u8, 24, 32, 255].repeat((w * h) as usize);
        for b in boxes {
            let [x, y, bw, bh] = b.map(|v| v as u32);
            for py in y + bh / 4..y + bh * 3 / 4 {
                for px in (x..x + bw).step_by(3) {
                    let i = ((py * w + px) * 4) as usize;
                    img[i..i + 3].copy_from_slice(&[240, 236, 226]);
                }
            }
        }
        img
    }

    fn screen() -> Vec<TextBlock> {
        vec![
            tb("Bring the Sunstone", [40.0, 40.0, 300.0, 30.0]),
            tb("100 / 250", [40.0, 120.0, 120.0, 30.0]),
            tb("مرحبا بك", [40.0, 200.0, 140.0, 30.0]),
            tb("Quit", [40.0, 280.0, 80.0, 30.0]),
            tb("Bring the  Sunstone", [400.0, 360.0, 300.0, 30.0]),
        ]
    }

    #[test]
    fn one_call_in_order_skipping_numbers_and_target_language() {
        let boxes: Vec<[f32; 4]> = screen().iter().map(|b| b.bounds).collect();
        let img = frame(800, 450, &boxes);
        let fake = Fake::new(|_, _, _| screen());
        let cache = Cache::new(64);
        let out = translate_frame(&img, 800, 450, "ar", &cache, &fake).unwrap();
        // the repeated line (other spacing) is sent once
        assert_eq!(
            *fake.ai_calls.borrow(),
            vec![vec!["Bring the Sunstone".to_string(), "Quit".into()]]
        );
        let tr: Vec<&str> = out.blocks.iter().map(|b| b.translation.as_str()).collect();
        assert_eq!(
            tr,
            [
                "ترجمة Bring the Sunstone",
                "",
                "",
                "ترجمة Quit",
                "ترجمة Bring the Sunstone"
            ]
        );
        assert_eq!(out.blocks[1].original, "100 / 250");
        // OCR boxes padded to line boxes
        assert_eq!(out.blocks[4].bounds, [397.0, 355.5, 306.0, 39.0]);
        assert!(out.rgba != img, "translated blocks are redrawn");
        let t = out.timings;
        assert!(t.total >= t.ocr + t.translate + t.render && t.total > Duration::ZERO);

        // second run: everything from the cache, no AI call at all
        let again = translate_frame(&img, 800, 450, "ar", &cache, &fake).unwrap();
        assert_eq!(fake.ai_calls.borrow().len(), 1);
        assert!(again.rgba == out.rgba);
    }

    /// #27: top of the screen first, a few sentences, then more per call, and the first
    /// part is shown before the rest is translated; the whole page is then
    /// exactly the all-at-once result, and the part shown early stays put.
    #[test]
    fn blocks_arrive_top_first_and_are_shown_before_the_last_is_translated() {
        // OCR lists the bottom label first (a sidebar read before the page)
        let labels = ["Quit", "Load", "Save", "Help", "Exit", "Undo"];
        let ys = [400.0, 40.0, 100.0, 160.0, 220.0, 280.0];
        let lines = move || -> Vec<TextBlock> {
            (labels.iter().zip(ys))
                .map(|(t, y)| tb(t, [40.0, y, 120.0, 30.0]))
                .collect()
        };
        let boxes: Vec<[f32; 4]> = lines().iter().map(|b| b.bounds).collect();
        let img = frame(400, 480, &boxes);
        let mut fake = Fake::new(move |_, _, _| lines());
        fake.batch = 2;
        let mut shown = Vec::new();
        let out = translate_progressively(&img, 400, 480, "ar", &Cache::new(64), &fake, |p| {
            shown.push((
                fake.ai_calls.borrow().len(),
                p.rgba.clone(),
                p.blocks.clone(),
            ));
        })
        .unwrap();
        let calls = fake.ai_calls.borrow().clone();
        // 2 sentences, then twice as many; a part in reading order
        let want: [&[&str]; 2] = [&["Load", "Save"], &["Quit", "Help", "Exit", "Undo"]];
        assert_eq!(calls, want);
        // the first part is on screen after one call of two
        let (after, first, blocks) = &shown[0];
        assert_eq!(*after, 1, "shown before the rest was translated");
        let done: Vec<&str> = (blocks.iter())
            .filter(|b| !b.translation.is_empty())
            .map(|b| b.original.as_str())
            .collect();
        assert_eq!(done, ["Load", "Save"]);
        assert!(out.timings.first > Duration::ZERO && out.timings.first <= out.timings.total);
        // the whole page = one call for everything
        let whole = translate_frame(
            &img,
            400,
            480,
            "ar",
            &Cache::new(64),
            &Fake::new(move |_, _, _| lines()),
        )
        .unwrap();
        assert!(out.rgba == whole.rgba);
        assert_eq!(out.blocks, whole.blocks);
        // rows 0..140 (Load, Save) look the same early and at the end; the
        // rest is still the original in the early frame
        let row = 400 * 4;
        assert!(first[..140 * row] == out.rgba[..140 * row]);
        assert!(first[150 * row..] == img[150 * row..]);
        assert!(out.rgba[150 * row..] != img[150 * row..]);
    }

    /// The one call of a cloud model (and of any `Deps` without a batch):
    /// nothing is shown early.
    #[test]
    fn one_call_shows_nothing_early() {
        let fake = Fake::new(|_, _, _| screen());
        let mut shown = 0;
        let img = frame(800, 450, &[]);
        translate_progressively(&img, 800, 450, "ar", &Cache::new(8), &fake, |_| shown += 1)
            .unwrap();
        assert_eq!((shown, fake.ai_calls.borrow().len()), (0, 1));
        let ai = Real::new(
            "auto",
            Via::Ai {
                ai: AiConfig::default(),
                key: None,
            },
        );
        assert_eq!(ai.batch(), usize::MAX);
        assert_eq!(Real::new("auto", Via::Offline).batch(), FIRST);
    }

    /// #107: one-shot translate reads with Windows OCR where it reads the
    /// source (as main mode and live do), PP-OCR otherwise.
    #[test]
    fn windows_ocr_reads_where_it_has_the_language() {
        use TextReader::{Auto, PpOcr, Windows};
        for (reader, tag, source, system) in [
            (Auto, Some("en-US"), "auto", true),
            (Auto, Some("en-US"), "en", true),
            (Auto, Some("en-GB"), "en-US", true),
            (Windows, Some("en-US"), "auto", true),
            (Auto, Some("en-US"), "fr", false), // its recognizer can't read French
            (Auto, Some("ar-SA"), "auto", false), // right-to-left: PP-OCR reads it better
            (Windows, Some("ar-SA"), "ar", true), // ... unless chosen
            (Auto, None, "auto", false),        // no recognizer (and Linux)
            (PpOcr, Some("en-US"), "auto", false),
        ] {
            assert_eq!(
                system_reads(reader, tag, source),
                system,
                "{reader:?} {tag:?} {source}"
            );
        }
    }

    #[test]
    fn markup_reaches_render_and_is_stripped_from_the_block_list() {
        let lines = vec![tb("Bring the Sunstone", [40.0, 40.0, 300.0, 30.0])];
        let img = frame(400, 120, &[lines[0].bounds]);
        let mut fake = Fake::new(move |_, _, _| lines.clone());
        fake.answer = |_| "أحضر [[حجر الشمس]]".into();
        let out = translate_frame(&img, 400, 120, "ar", &Cache::new(8), &fake).unwrap();
        assert_eq!(out.blocks[0].translation, "أحضر حجر الشمس");
        let line = to_line(
            tb("Bring the Sunstone", [40.0, 40.0, 300.0, 30.0]),
            400,
            120,
        );
        let blocks = group_blocks(&[line]);
        let mut direct = img.clone();
        render(
            &mut direct,
            400,
            120,
            &blocks,
            &["أحضر [[حجر الشمس]]".into()],
            true,
        );
        assert!(out.rgba == direct, "the [[ ]] spans went to render as is");
        // and the real client asks for them
        ai::translate_with(&["x".into()], "auto", "ar", MARKUP, |m| {
            assert!(m[0].text.ends_with(MARKUP));
            Ok(r#"["y"]"#.into())
        })
        .unwrap();
    }

    #[test]
    fn ai_error_means_no_render_and_nothing_cached() {
        let mut fake = Fake::new(|_, _, _| screen());
        fake.fail_ai = true;
        let cache = Cache::new(8);
        let img = frame(800, 450, &[]);
        let err = translate_frame(&img, 800, 450, "ar", &cache, &fake)
            .err()
            .unwrap();
        assert!(matches!(err, Error::Ai(AiError::Server(503))), "{err}");
        assert_eq!(cache.get("Quit", "ar"), None);
        // a bad frame fails before OCR
        assert!(matches!(
            translate_frame(&img[4..], 800, 450, "ar", &cache, &fake),
            Err(Error::Ocr(OcrError::BadImage(_)))
        ));
        assert_eq!(fake.ocr_calls.borrow().len(), 1);
    }

    #[test]
    fn echoed_text_is_left_alone() {
        let mut fake = Fake::new(|_, _, _| vec![tb("Eldrin", [10.0, 10.0, 80.0, 30.0])]);
        fake.answer = |s| format!("[[{s}]]");
        let img = frame(200, 60, &[[10.0, 10.0, 80.0, 30.0]]);
        let out = translate_frame(&img, 200, 60, "en", &Cache::new(8), &fake).unwrap();
        assert_eq!(out.blocks[0].translation, "");
        assert!(out.rgba == img);
    }

    /// Stops with a citation after them end a sentence; abbreviations and
    /// numbers don't; a very long sentence is cut at its middle comma.
    #[test]
    fn sentences_end_after_citations_and_long_ones_are_halved() {
        let t = "Haumea as dwarf planets.[12] Four others: Orcus and Sedna.1131 Less \
                 massive are clouds.[el The U.S. Patent of Dr. Who is 4.6 Million.";
        assert_eq!(
            sentences(t),
            [
                "Haumea as dwarf planets.[12]",
                "Four others: Orcus and Sedna.1131",
                "Less massive are clouds.[el",
                "The U.S. Patent of Dr. Who is 4.6 Million."
            ]
        );
        let long = format!("{}, {}.", ["word"; 30].join(" "), ["more"; 30].join(" "));
        let parts = sentences(&long);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts[0].ends_with("word,") && parts[1].starts_with("more"));
        let words = ["word"; 60].join(" ");
        assert_eq!(sentences(&words), [words], "no comma: whole");
    }

    #[test]
    fn script_and_direction() {
        assert!(!needs_translation("12:30 — 4/5 !", "ar"));
        assert!(!needs_translation("القائمة الرئيسية", "ar-SA"));
        assert!(needs_translation("Main menu", "ar"));
        assert!(needs_translation("Главное меню", "ar"));
        assert!(!needs_translation("Главное меню", "ru"));
        assert!(needs_translation("Main menu", "fr"));
        assert!(is_rtl("ar") && is_rtl("he-IL") && !is_rtl("en") && !is_rtl("fr"));
    }

    /// Offline: the pair follows the source (or the script, for auto); one
    /// that doesn't exist fails before anything runs, and says AI can.
    #[test]
    fn offline_pairs_and_the_not_available_path() {
        let texts = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let auto = Real::new(lang::AUTO, Via::Offline);
        assert!(auto.offline() && auto.missing().is_none());
        let pair = |r: &Real, t: &[&str], target| {
            r.pair_for(&texts(t), target).map(|p| (p.source, p.target))
        };
        assert_eq!(pair(&auto, &["Main menu"], "ar").unwrap(), ("en", "ar"));
        assert_eq!(
            pair(&auto, &["القائمة الرئيسية"], "en").unwrap(),
            ("ar", "en")
        );
        assert_eq!(pair(&auto, &["メニュー"], "en").unwrap(), ("ja", "en"));
        assert_eq!(
            pair(&Real::new("fr", Via::Offline), &["x"], "en").unwrap(),
            ("fr", "en")
        );
        for (real, t, target, from) in [
            (&auto, &["Главное меню"][..], "en", "Russian"),
            (
                &Real::new("fr", Via::Offline),
                &["Bonjour"][..],
                "de",
                "French",
            ),
        ] {
            let err = real.lines(&texts(t), target, "").unwrap_err().to_string();
            assert!(
                err.contains(&format!("no offline model from {from}")),
                "{err}"
            );
            assert!(err.contains("Choose AI"), "offers AI: {err}");
            assert!(real.missing().is_none(), "nothing to download for it");
        }
    }

    #[test]
    fn cache_hit_miss_and_eviction() {
        let c = Cache::new(2);
        assert_eq!(c.get("a", "ar"), None);
        c.put("a", "ar", "A".into());
        c.put("b", "ar", "B".into());
        assert_eq!(c.get("  a ", "ar").as_deref(), Some("A"), "normalised");
        assert_eq!(c.get("a", "fr"), None, "keyed by target too");
        c.put("c", "ar", "C".into()); // full: a, b move to the old generation
        assert_eq!(c.get("a", "ar").as_deref(), Some("A")); // hit moves a back
        c.put("d", "ar", "D".into()); // full again: b is dropped
        assert_eq!(c.get("b", "ar"), None);
        for (k, v) in [("a", "A"), ("c", "C"), ("d", "D")] {
            assert_eq!(c.get(k, "ar").as_deref(), Some(v), "{k}");
        }
    }

    /// Real OCR (in process; models from `ocr::models_dir()`), a fake
    /// translator (fixed Arabic, else the text reversed), the synthetic 1080p
    /// game frame; one-shot, then live steps. PNGs in the temp dir. Run:
    /// cargo test --release e2e_real_ocr -- --ignored --nocapture
    #[test]
    #[ignore]
    fn e2e_real_ocr_fake_translator() {
        use super::super::live::Live;
        use super::super::tests::{ar, game_frame, save_png};

        struct RealOcr(Mutex<ocr::Engine>);
        impl Deps for RealOcr {
            fn recognize(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<TextBlock>, OcrError> {
                let mut engine = self.0.lock().unwrap();
                engine.recognize(rgba, w, h, ocr::Read::One(0))
            }
            fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
                Ok(texts
                    .iter()
                    .map(|t| ar(t).map_or_else(|| t.chars().rev().collect(), String::from))
                    .collect())
            }
        }

        let t = Instant::now();
        let deps = RealOcr(Mutex::new(ocr::Engine::load(&ocr::models_dir()).unwrap()));
        crate::log!("OCR engine load {:?}", t.elapsed());
        let (img, _) = game_frame();
        let (w, h) = (1920, 1080);
        let cache = Cache::new(256);
        for run in 1..=2 {
            let out = translate_frame(&img, w, h, "ar", &Cache::new(256), &deps).unwrap();
            crate::log!("one-shot run {run}: {:?}", out.timings);
        }
        let out = translate_frame(&img, w, h, "ar", &cache, &deps).unwrap();
        for b in &out.blocks {
            crate::log!("{:?} {:?} -> {:?}", b.bounds, b.original, b.translation);
        }
        let dir = std::env::temp_dir().join("opencircle-pipeline");
        std::fs::create_dir_all(&dir).unwrap();
        save_png(&dir.join("before.png"), (1920, 1080), &img);
        save_png(&dir.join("after.png"), (1920, 1080), &out.rgba);
        let translated = out.blocks.iter().filter(|b| !b.translation.is_empty());
        assert!(translated.count() >= 10, "{:?}", out.blocks);

        // live: first frame reads everything (translations from the cache)
        let mut live = Live::new();
        let t = live.step(&img, w, h, "ar", &cache, &deps).unwrap().unwrap();
        crate::log!("live first frame: {t:?}, {} patches", live.patches().len());
        let t = Instant::now();
        assert!(
            live.step(&img, w, h, "ar", &cache, &deps)
                .unwrap()
                .is_none()
        );
        crate::log!("live unchanged frame: {:?}", t.elapsed());
        // X11: the capture shows our layer -> still nothing to do
        let mut shown = img.clone();
        let mut layer = img.clone();
        for p in live.patches() {
            let [x, y, pw, _] = p.bounds.map(|v| v as usize);
            for (row, src) in p.rgba.chunks_exact(pw * 4).enumerate() {
                let i = ((y + row) * 1920 + x) * 4;
                shown[i..i + pw * 4].copy_from_slice(src);
                layer[i..i + pw * 4].copy_from_slice(src);
            }
        }
        save_png(&dir.join("live_layer.png"), (1920, 1080), &layer);
        assert!(
            live.step(&shown, w, h, "ar", &cache, &deps)
                .unwrap()
                .is_none()
        );
        // the prompt "Press E to interact" goes away: only its region is read
        let mut next = img.clone();
        for y in 590..640 {
            for x in 800..1120 {
                let i = (y * 1920 + x) * 4;
                let src = (y * 1920 + 790) * 4;
                next.copy_within(src..src + 4, i);
            }
        }
        let n = live.blocks().len();
        let t = live
            .step(&next, w, h, "ar", &cache, &deps)
            .unwrap()
            .unwrap();
        crate::log!(
            "live, one block gone: {t:?}; blocks {n} -> {}",
            live.blocks().len()
        );
        assert!(
            live.blocks()
                .iter()
                .all(|b| !b.original.starts_with("Press"))
        );
        crate::log!("PNGs in {}", dir.display());
    }
}
