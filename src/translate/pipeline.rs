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
use super::{Block, Drawn, Line, MARKUP, group_blocks, render, split_fields};
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
    /// All the text read, before the first `translate` call: "auto" guesses
    /// its language once from all of it (a batch of short headings alone
    /// has no common words to tell Spanish from English).
    fn read(&self, _texts: &[&str]) {}
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
    /// "auto": the language `read` guessed from all the text (`None` = guess
    /// from each call's lines), and every language the page's blocks are
    /// sure of (`lang::sure`): a Spanish subtitle in an English player.
    guessed: Mutex<(Option<&'static str>, Vec<&'static str>)>,
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
            guessed: Mutex::default(),
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

    /// The set source, or ("auto") the page's language (`read`), else the
    /// one the script and words of `texts` suggest.
    fn source(&self, texts: &[String]) -> &str {
        match self.source.as_str() {
            lang::AUTO => match self.guessed.lock().unwrap_or_else(|e| e.into_inner()).0 {
                Some(page) => page,
                None => lang::guess(texts.iter().map(String::as_str)).unwrap_or("en"),
            },
            s => s,
        }
    }

    /// The offline models for `texts` into `target` (`mt::route`), with the
    /// texts (indices) each route takes. "auto" judges each text by its own
    /// words where they are clear (`lang::sure`), else by the page's
    /// language: an English player around a Spanish subtitle no longer
    /// outvotes it. Texts already in `target` take no route (left as they
    /// are). Err = no way offline, or nothing on the page is in another
    /// language (the message offers AI; nothing is sent anywhere).
    #[allow(clippy::type_complexity)]
    pub(super) fn routes(
        &self,
        texts: &[String],
        target: &str,
    ) -> Result<Vec<(Vec<&'static mt::Pair>, Vec<usize>)>, AiError> {
        let fallback = self.source(texts);
        let same = |s: &str| {
            lang::find(s)
                .zip(lang::find(target))
                .is_some_and(|(s, t)| s == t)
        };
        // On a page clearly in one language (3/4 of the lines that tell), a
        // line's own language in the page's script (told by its words, which
        // can be wrong) only counts where its models are here, or the page
        // itself needs nothing: else it goes the page's way. A French line
        // taken for Spanish asked for es-de (112 MB) and the page waited on it.
        // Another script (an Arabic line) is sure: its pair is asked for.
        let told = self
            .guessed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .clone();
        let clear = told.iter().filter(|s| **s == fallback).count() * 4 >= told.len() * 3;
        let page_runs = clear && !same(fallback) && mt::route(fallback, target).is_some();
        let script = |s: &str| lang::find(s).map(|l| l.script);
        let mut own: Vec<(&str, bool)> = Vec::new();
        let mut usable = |s: &'static str| match own.iter().find(|(o, _)| *o == s) {
            Some((_, ok)) => *ok,
            None => {
                let ok = !page_runs
                    || same(s)
                    || script(s) != script(fallback)
                    || mt::route(s, target)
                        .is_some_and(|r| r.iter().all(|p| p.missing_bytes() == 0));
                own.push((s, ok));
                ok
            }
        };
        let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
        for (i, t) in texts.iter().enumerate() {
            let s = match self.source.as_str() {
                lang::AUTO => lang::sure(t).filter(|s| usable(s)).unwrap_or(fallback),
                _ => fallback,
            };
            if same(s) {
                continue;
            }
            match groups.iter_mut().find(|(g, _)| *g == s) {
                Some((_, idx)) => idx.push(i),
                None => groups.push((s, vec![i])),
            }
        }
        if groups.is_empty() && told.iter().all(|s| same(s)) {
            return Err(no_route(fallback, target));
        }
        (groups.into_iter())
            .map(|(s, idx)| {
                let route = mt::route(s, target).ok_or_else(|| no_route(s, target))?;
                Ok((route, idx))
            })
            .collect()
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
        let Via::Ai { ai, key } = &self.via else {
            return self.on_device(texts, target);
        };
        match ai::translate_lines(ai, key.as_ref(), texts, &self.source, target, markup) {
            // A reply that isn't the array (a chatty model): on-device instead
            // of an error card, when a model exists for these lines.
            Err(AiError::BadResponse(why)) if self.routes(texts, target).is_ok() => {
                crate::log!("[opencircle] AI translation unusable ({why}); on-device instead");
                self.on_device(texts, target)
            }
            done => done,
        }
    }

    /// `texts` into `target` with the on-device model(s): through English
    /// when there is no direct one.
    fn on_device(&self, texts: &[String], target: &str) -> Result<Vec<String>, AiError> {
        self.on_device_with(texts, target, |t, pair| {
            mt::translate(t, pair).map_err(|e| AiError::Offline(e.to_string()))
        })
    }

    /// `on_device` with `run` = one model over some texts (the helper in the
    /// app; in process in the real-page tests).
    pub(super) fn on_device_with(
        &self,
        texts: &[String],
        target: &str,
        run: impl Fn(&[String], &'static mt::Pair) -> Result<Vec<String>, AiError>,
    ) -> Result<Vec<String>, AiError> {
        let routes = self.routes(texts, target)?;
        let mut pairs = routes.iter().flat_map(|(r, _)| r);
        if let Some(pair) = pairs.find(|p| p.missing_bytes() > 0) {
            *self.missing.lock().unwrap_or_else(|e| e.into_inner()) = Some(pair);
            return Err(AiError::Offline(format!(
                "{} isn't downloaded yet. Translate once with Ctrl+Shift+T to \
                 download it, or set up AI in Settings > AI",
                pair.label()
            )));
        }
        ocr::keep_warm();
        // a text in no route is in the target already: echoed, not redrawn
        let mut out = texts.to_vec();
        for (route, idx) in routes {
            let part: Vec<String> = idx.iter().map(|&i| texts[i].clone()).collect();
            let part = by_sentence(&part, |src| {
                let (bodies, marks): (Vec<String>, Vec<String>) =
                    src.iter().map(|s| footnotes(s)).unzip();
                let out = hops(&bodies, &route, target, &run)?;
                Ok(out.into_iter().zip(marks).map(|(t, m)| t + &m).collect())
            })?;
            for (i, t) in idx.into_iter().zip(part) {
                out[i] = t;
            }
        }
        Ok(out)
    }
}

/// `src` (sentences) through the models of `route` in turn. Through
/// English, a phrase the first model gave back unchanged ("40 langues",
/// "Pothos, Epipremnum", noise) never goes on to the next, which makes
/// something up from words it doesn't know ("40 langues" -> "إيان غوس 40"); a
/// single plain word may be English already ("Text", "Chat": German and
/// French write them so) and goes on. An output that is the source again or
/// made up (`made_up`) keeps the original.
fn hops(
    src: &[String],
    route: &[&'static mt::Pair],
    target: &str,
    run: impl Fn(&[String], &'static mt::Pair) -> Result<Vec<String>, AiError>,
) -> Result<Vec<String>, AiError> {
    let mut t = src.to_vec();
    // Not in the source's script at all (Latin noise on an Arabic page): an
    // Arabic model makes words up from it ("LILI" -> "LOLI"); it stays as is.
    let script = lang::find(route[0].source).map_or(Script::Latin, |l| l.script);
    let foreign = |s: &str| {
        script != Script::Latin
            && s.chars().any(char::is_alphabetic)
            && !s.chars().any(|c| script.has(c))
    };
    let mut on: Vec<usize> = (0..t.len()).filter(|&i| !foreign(&src[i])).collect();
    for pair in route {
        let sent: Vec<String> = on.iter().map(|&i| t[i].clone()).collect();
        if sent.is_empty() {
            break;
        }
        for (&i, r) in on.iter().zip(run(&sent, pair)?) {
            t[i] = r;
        }
        let one_word = |s: &str| s.split_whitespace().count() == 1 && s.is_ascii();
        on.retain(|&i| !echo(&src[i], &t[i]) || one_word(&src[i]));
    }
    for (s, t) in src.iter().zip(&mut t) {
        if echo(s, t) || made_up(s, t, target) {
            *t = s.clone();
        }
    }
    Ok(t)
}

/// Windows OCR (the user's Windows language, Latin) shown a page in another
/// script (Arabic) reads it as noise: "d-l.-lss '4.3.1>-0-11 a-LI.>I-LII".
/// True when under 60 % of what it read (of 10 tokens or more) are words:
/// letters only (an apostrophe or hyphen inside), lower case, Title or CAPS.
/// Measured on 7 real pages (Edge, Win10 VM): French, German, Spanish,
/// English pages 0.84-0.97; the Arabic Wikipedia page 0.32.
pub(super) fn misread(lines: &[TextBlock]) -> bool {
    let (tokens, words) = word_count(lines);
    tokens >= 10 && words * 10 < tokens * 6
}

/// Tokens (of 2+ letters or digits) in `lines`, and how many are words.
fn word_count(lines: &[TextBlock]) -> (usize, usize) {
    let word = |t: &str| {
        let t = t.trim_matches(|c: char| "([{\"'«».,:;!?)]}".contains(c));
        let parts: Vec<&str> = t.split(['\'', '’', '-']).collect();
        let letters: String = parts.concat();
        parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(char::is_alphabetic))
            && (letters.chars().all(char::is_lowercase)
                || letters.chars().all(char::is_uppercase)
                || letters.chars().skip(1).all(char::is_lowercase))
    };
    let tokens: Vec<&str> = (lines.iter())
        .flat_map(|l| l.text.split_whitespace())
        .filter(|w| w.chars().filter(|c| c.is_alphanumeric()).count() >= 2)
        .collect();
    (tokens.len(), tokens.iter().filter(|t| word(t)).count())
}

/// Of a `misread` page, the lines read as words (most of their tokens), and
/// their boxes: the second reader leaves them alone.
pub(super) fn words_read(lines: Vec<TextBlock>) -> (Vec<TextBlock>, Vec<[f32; 4]>) {
    let keep: Vec<TextBlock> = (lines.into_iter())
        .filter(|l| {
            let (t, w) = word_count(std::slice::from_ref(l));
            t > 0 && w * 10 >= t * 8
        })
        .collect();
    let known = keep.iter().map(|l| l.bounds).collect();
    (keep, known)
}

/// Two reads of one page as one, in reading order: by rows (8 px bands of
/// the line's middle), left to right in a row.
// ponytail: fixed bands; a line straddling two may sort a row early.
pub(super) fn merged(mut a: Vec<TextBlock>, b: Vec<TextBlock>) -> Vec<TextBlock> {
    a.extend(b);
    let key = |l: &TextBlock| {
        (
            ((l.bounds[1] + l.bounds[3] / 2.0) / 8.0) as i32,
            l.bounds[0],
        )
    };
    a.sort_by(|p, q| {
        let ((pr, px), (qr, qx)) = (key(p), key(q));
        pr.cmp(&qr).then(px.total_cmp(&qx))
    });
    a
}

/// `out` is `src` again (case, spaces and punctuation aside).
fn echo(src: &str, out: &str) -> bool {
    let key = |s: &str| -> String {
        (s.chars().filter(|c| c.is_alphanumeric()))
            .flat_map(char::to_lowercase)
            .collect()
    };
    key(src) == key(out)
}

/// An offline output that isn't a translation of `src` (Marian makes things
/// up from what it can't read): a loop ("- - - - -", "Ül-jjjjjjjjj"), far
/// longer than the source, a short label grown into a sentence, a number
/// lost or made up, letters of another script than the target's.
fn made_up(src: &str, out: &str, target: &str) -> bool {
    let words = |s: &str| s.split_whitespace().count();
    let repeats = |s: &str| {
        let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        let tokens: Vec<&str> = s.split_whitespace().collect();
        let run = |n: usize, same: &dyn Fn(usize) -> bool| {
            (1..n).fold((1, 1), |(best, cur), i| {
                let cur = if same(i) { cur + 1 } else { 1 };
                (best.max(cur), cur)
            })
        };
        let (c, _) = run(chars.len(), &|i| chars[i] == chars[i - 1]);
        let (t, _) = run(tokens.len(), &|i| tokens[i] == tokens[i - 1]);
        (c, t)
    };
    let ((sc, st), (oc, ot)) = (repeats(src), repeats(out));
    let looped = (oc >= 6 && oc > sc + 2) || (ot >= 4 && ot > st + 1);
    let (sn, on) = (src.chars().count(), out.chars().count());
    let long = on > 3 * sn + 15 || (words(src) <= 3 && words(out) > 2 * words(src) + 2);
    // numbers: Arabic-Indic digits are the same numbers
    let digits = |s: &str| -> String {
        s.chars()
            .map(|c| match c {
                '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
                '\u{06F0}'..='\u{06F9}' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
                c if c.is_ascii_digit() => c,
                _ => ' ',
            })
            .collect()
    };
    let (sd, od) = (digits(src), digits(out));
    // a number of its own ("40", "1:47", "(1894)"), not digits in a word
    // OCR mangled ("tech07" for "techo")
    let numbers = (src.split_whitespace())
        .filter(|w| !w.chars().any(char::is_alphabetic))
        .flat_map(|w| {
            digits(w)
                .split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        });
    let number_lost = numbers.into_iter().any(|n| !od.contains(&n));
    let number_new = od.split_whitespace().any(|n| !sd.contains(n));
    let script = lang::find(target).map_or(Script::Latin, |l| l.script);
    let letters: Vec<char> = out.chars().filter(|c| c.is_alphabetic()).collect();
    let foreign = match script {
        // into a Latin language: no letters of a script the source didn't have
        Script::Latin => letters
            .iter()
            .any(|c| !Script::Latin.has(*c) && !src.contains(*c)),
        s => letters.iter().filter(|c| s.has(**c)).count() * 2 < letters.len(),
    };
    looped || long || number_lost || number_new || foreign
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
                Ok(lines) if self.source == lang::AUTO && misread(&lines) => {
                    // another script: what it read as words stays (the
                    // address bar, Latin UI), the rest is read again in the
                    // script that suits it (main mode's second reader)
                    crate::log!("[opencircle] Windows OCR read no words: the second reader reads");
                    let (keep, known) = words_read(lines);
                    match ocr::recognize_except(rgba, width, height, &known) {
                        Ok(other) if !other.is_empty() => return Ok(merged(keep, other)),
                        Ok(_) => crate::log!("[opencircle] no other script read; PP-OCR reads"),
                        Err(e) => {
                            crate::log!("[opencircle] second reader failed ({e}); PP-OCR reads")
                        }
                    }
                }
                Ok(lines) => return Ok(lines),
                Err(e) => crate::log!("[opencircle] Windows OCR failed ({e}); PP-OCR reads"),
            }
        }
        ocr::recognize_as(rgba, width, height, &self.source)
    }

    fn translate(&self, texts: &[String], target: &str) -> Result<Vec<String>, AiError> {
        self.lines(texts, target, MARKUP)
    }

    fn read(&self, texts: &[&str]) {
        if self.source == lang::AUTO {
            let page = lang::guess(texts.iter().copied());
            let sure = texts.iter().filter_map(|t| lang::sure(t));
            *self.guessed.lock().unwrap_or_else(|e| e.into_inner()) =
                (page, page.into_iter().chain(sure).collect());
        }
    }

    /// Offline: the top lines first, quickly (a cloud model gets one call: a
    /// round trip each).
    fn batch(&self) -> usize {
        match self.via {
            Via::Offline => FIRST,
            Via::Ai { .. } => usize::MAX,
        }
    }

    /// Offline: loads the likely first model (English for "auto") in the
    /// helper, which stays warm, while the screen is read.
    fn warm(&self, target: &str) {
        let Some(pair) = mt::route(self.source(&[]), target).map(|r| r[0]) else {
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

/// `s` without its footnote marks ("cacti,[2]", "French original[c]", OCR's
/// "conditions.[5" and "indoors[ ]"), and the marks: the model never reads
/// them (it garbled them, or a lost "[5" made the whole line look made up
/// and stay English); they are put back after the translation. A mark is
/// `[`, up to 3 digits, lower-case letters or spaces, then `]` or the word's
/// end; "[edit]" and IPA ("[la sug da") are text.
fn footnotes(s: &str) -> (String, String) {
    let (mut body, mut marks) = (String::new(), String::new());
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        let inner = rest[i + 1..]
            .find(|c: char| !(c.is_ascii_digit() || c.is_ascii_lowercase() || c == ' '))
            .unwrap_or(rest.len() - i - 1);
        let tail = &rest[i + 1 + inner..];
        let closed = tail.starts_with(']');
        let ends = tail.is_empty() || tail.starts_with(char::is_whitespace);
        let filled = !rest[i + 1..i + 1 + inner].trim().is_empty();
        if inner <= 3 && (closed || (ends && filled)) {
            let end = i + 1 + inner + usize::from(closed);
            body.push_str(&rest[..i]);
            marks.push_str(&rest[i..end]);
            rest = &rest[end..];
        } else {
            body.push_str(&rest[..=i]);
            rest = &rest[i + 1..];
        }
    }
    if marks.is_empty() {
        return (s.to_string(), marks);
    }
    body.push_str(rest);
    (body.split_whitespace().collect::<Vec<_>>().join(" "), marks)
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
/// No offline route from `source` into `target`: "already in" when they are
/// the same language (a quiet note), else why not (offers AI).
fn no_route(source: &str, target: &str) -> AiError {
    match lang::find(source).zip(lang::find(target)) {
        Some((s, t)) if s.code == t.code => AiError::AlreadyIn(t.name),
        _ => AiError::Offline(mt::unavailable(source, target)),
    }
}

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
/// will be, so nothing moves when the rest arrives. A partial frame is drawn
/// on its own thread while the next call runs (the calls wait on the helper
/// or the network), so only the whole page's render adds to the total; a
/// drawer behind skips to the newest part. An error after a partial frame
/// still fails the whole (the caller goes back to the original).
pub fn translate_progressively(
    rgba: &[u8],
    width: u32,
    height: u32,
    target: &str,
    cache: &Cache,
    deps: &impl Deps,
    mut shown: impl FnMut(&Translated) + Send,
) -> Result<Translated, Error> {
    let start = Instant::now();
    check_frame(rgba, width, height)?;
    deps.warm(target);
    let found = deps.recognize(rgba, width, height).map_err(Error::Ocr)?;
    let lines: Vec<Line> = (found.into_iter())
        .filter(|b| !cut_off(b, (width, height)))
        .map(|b| to_line(b, width, height))
        .collect();
    let lines = split_fields(rgba, width as usize, height as usize, &lines);
    let ocr = start.elapsed();
    let blocks = group_blocks(&lines);
    deps.read(&blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>());
    let mut tr = vec![String::new(); blocks.len()];
    let mut timings = Timings {
        ocr,
        ..Timings::default()
    };
    let parts = parts(&blocks, deps.batch());
    let (to_draw, todo) = std::sync::mpsc::channel::<Vec<String>>();
    let drawn = std::thread::scope(|s| {
        let blocks = &blocks;
        let drawer = s.spawn(move || {
            let mut t = Timings::default();
            while let Ok(mut tr) = todo.recv() {
                // behind: only the newest part is worth drawing
                while let Ok(newer) = todo.try_recv() {
                    tr = newer;
                }
                let partial = draw(rgba, (width, height), target, blocks, &tr, &mut t);
                if t.first.is_zero() {
                    t.first = start.elapsed();
                }
                shown(&partial);
            }
            t
        });
        let mut last_shown = None::<Instant>;
        let translated = (|| {
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
                let _ = to_draw.send(tr.clone());
            }
            Ok(())
        })();
        drop(to_draw);
        let drawn = drawer.join().expect("the partial frames' drawer");
        translated.map(|()| drawn)
    })?;
    timings.render = drawn.render;
    timings.first = drawn.first;
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
/// ONE call for the rest (each distinct text once, in reading order). A
/// block of short items ("[ modifier | modifier le code ]", [`items`]) is
/// translated item by item and put back together with its brackets and
/// bars, so each link label reads the same wherever it shows.
pub(super) fn translate_blocks(
    blocks: &[Block],
    target: &str,
    cache: &Cache,
    deps: &impl Deps,
) -> Result<Vec<String>, AiError> {
    let latin = matches!(
        lang::find(target).map(|l| l.script),
        Some(Script::Latin) | None
    );
    // per block: its pieces, each (text, Some(what is sent) | None = kept)
    let pieces: Vec<Vec<(String, Option<String>)>> = (blocks.iter())
        .map(|b| match needs_translation(&b.text, target) {
            false => Vec::new(),
            true => (items(&normalise(&b.text)).into_iter())
                .map(|(p, text)| {
                    let key = (text && needs_translation(&p, target)).then(|| sent(&p));
                    (p, key)
                })
                .collect(),
        })
        .collect();
    let mut found: HashMap<&str, String> = HashMap::new();
    let mut todo: Vec<String> = Vec::new();
    for k in pieces.iter().flatten().filter_map(|(_, k)| k.as_ref()) {
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
            let t = if normalise(&unmarked(&t)).to_lowercase() == k.to_lowercase() {
                String::new()
            } else {
                t
            };
            cache.put(k, target, t.clone());
            found.insert(k, t);
        }
    }
    Ok(pieces
        .iter()
        .map(|p| {
            let mut any = false;
            let whole: String = (p.iter())
                .map(|(text, k)| match k.as_ref().map(|k| &found[k.as_str()]) {
                    Some(t) if !t.is_empty() => {
                        any = true;
                        // sent capitalised (`sent`): back to its own case
                        match latin && text.starts_with(char::is_lowercase) {
                            true => lower_first(t),
                            false => t.clone(),
                        }
                    }
                    _ => text.clone(),
                })
                .collect();
            if any { whole } else { String::new() }
        })
        .collect())
}

/// What is sent for a piece: a short label in lower case ("modifier", a
/// link) as a label starts ("Modifier"): alone, MT reads a lower-case word
/// as a fragment ("modifier" -> "amend", "Modifier" -> "Edit").
fn sent(piece: &str) -> String {
    match piece.split(' ').count() <= 3 && piece.starts_with(char::is_lowercase) {
        true => upper_first(piece),
        false => piece.to_string(),
    }
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_lowercase().chain(c).collect())
        .unwrap_or_default()
}

/// A line of short items: `[`, `]` and `|` (with their spaces) cut it into
/// pieces, (piece, true) for text and (piece, false) for the cuts, in order;
/// joined they are the line. OCR reads a bar between two link labels as a
/// capital "I" ("[ modifier I modifier le code ]"): inside brackets a lone
/// "I" or "l" is that bar. A line without a bar, or with a longer piece
/// (more than 5 words: a paragraph with citations), is one piece.
fn items(text: &str) -> Vec<(String, bool)> {
    if !text.contains(['[', ']', '|']) {
        return vec![(text.to_string(), true)];
    }
    // the bars OCR read as letters, inside brackets only (English "I" stays)
    let mut depth = 0;
    let words: Vec<&str> = (text.split(' '))
        .map(|w| {
            let bar = depth > 0 && matches!(w, "I" | "l");
            depth += w.matches('[').count() as i32 - w.matches(']').count() as i32;
            if bar { "|" } else { w }
        })
        .collect();
    let text = words.join(" ");
    // only a row of short items: a paragraph's citations ("planets.[12]")
    // keep its sentences whole
    let short = (text.split(['[', ']', '|'])).all(|p| p.split_whitespace().count() <= 5);
    if !text.contains('|') || !short {
        return vec![(text, true)];
    }
    let mut out: Vec<(String, bool)> = Vec::new();
    let cut = |out: &mut Vec<(String, bool)>, c: &str| match out.last_mut() {
        Some((p, false)) => p.push_str(c),
        _ if !c.is_empty() => out.push((c.to_string(), false)),
        _ => {}
    };
    let mut rest = text.as_str();
    loop {
        let k = rest.find(['[', ']', '|']).unwrap_or(rest.len());
        let seg = &rest[..k];
        let body = seg.trim();
        if body.is_empty() {
            cut(&mut out, seg);
        } else {
            let lead = seg.len() - seg.trim_start().len();
            cut(&mut out, &seg[..lead]);
            out.push((body.to_string(), true));
            cut(&mut out, &seg[lead + body.len()..]);
        }
        if k == rest.len() {
            return out;
        }
        cut(&mut out, &rest[k..k + 1]);
        rest = &rest[k + 1..];
    }
}

/// False for numbers/punctuation only, for lone Latin letters (an icon read
/// as "X" or "C": no word, and MT makes "tenth" of it), for addresses and
/// code (`code_like`: a browser's address bar, a file name, an e-mail: left
/// exactly as they are), and for text already in the target's script (Latin
/// targets can't be told from English: the model decides).
fn needs_translation(text: &str, target: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    let word =
        (text.split(|c: char| !c.is_alphabetic())).any(|w| w.chars().count() > 1 || !w.is_ascii());
    match lang::find(target).map(|l| l.script) {
        _ if !word || code(text) => false,
        Some(Script::Latin) | None => true,
        Some(script) => letters.iter().filter(|c| script.has(**c)).count() * 2 < letters.len(),
    }
}

/// An address or code line (`code_like`): its letters are >= 80 % in such
/// tokens, or it starts with an address (a domain, "www.", "://"), as a
/// browser's address bar does however OCR reads the rest of it: PP-OCR's
/// "fr.wikipedia.org /wiki/Système solaire" has a gap, and a line in front
/// ("Q", a lock icon read as a letter) is skipped.
pub(crate) fn code(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let in_code: usize = (text.split_whitespace())
        .filter(|w| code_like(w))
        .map(|w| w.chars().filter(|c| c.is_alphabetic()).count())
        .sum();
    let first = text.split_whitespace().find(|w| w.chars().count() > 1);
    let address = first.is_some_and(|w| {
        let host = w.split('/').next().unwrap_or_default();
        code_like(w) && (w.contains("://") || w.starts_with("www.") || host.contains('.'))
    });
    letters > 0 && (address || in_code * 5 >= letters * 4)
}

/// A web address, a path, an e-mail address, a file name or an identifier:
/// "fr.wikipedia.org/wiki/Plante_d%27intérieur", "C:\Users", "a@b.org",
/// "photo.jpg", "snake_case". Not a word with a full stop ("etc.", "U.S.",
/// "e.g.") or a percentage ("50%").
pub(crate) fn code_like(word: &str) -> bool {
    let w = word.trim_matches(|c: char| "()[]<>\"'«»“”,;".contains(c));
    let w = w.trim_end_matches(['.', ':', '!', '?']);
    let n = w.chars().count();
    if n < 4 {
        return false;
    }
    let inner = &w[w.chars().next().map_or(0, char::len_utf8)..];
    let inner = inner.strip_suffix(|_: char| true).unwrap_or(inner);
    let domain = || {
        let parts: Vec<&str> = w.split('.').collect();
        let (first, last) = (parts[0], parts[parts.len() - 1]);
        parts.len() > 1
            && parts.iter().all(|p| !p.is_empty())
            && first.chars().count() >= 2
            && (2..=5).contains(&last.len())
            && last
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    };
    w.contains("://")
        || w.starts_with("www.")
        || (w.contains('@') && w.contains('.'))
        || inner.contains(['/', '\\', '_', '%', '=', '#'])
        || domain()
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
        /// How long each translate call takes.
        pub delay: Duration,
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
                delay: Duration::ZERO,
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
            std::thread::sleep(self.delay);
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
            shown.push((p.rgba.clone(), p.blocks.clone()));
        })
        .unwrap();
        let calls = fake.ai_calls.borrow().clone();
        // 2 sentences, then twice as many; a part in reading order
        let want: [&[&str]; 2] = [&["Load", "Save"], &["Quit", "Help", "Exit", "Undo"]];
        assert_eq!(calls, want);
        // the first part is on screen, drawn while the second call ran
        let (first, blocks) = &shown[0];
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

    /// A partial frame is drawn and shown while the next call runs, not
    /// between the calls (gate 0.1.2: 3.04 s for a page, 0.2 s of it partial
    /// renders waiting for each other).
    #[test]
    fn a_partial_frame_is_drawn_while_the_next_call_runs() {
        let lines = || {
            vec![
                tb("Load", [40.0, 40.0, 120.0, 30.0]),
                tb("Quit", [40.0, 300.0, 120.0, 30.0]),
            ]
        };
        let boxes: Vec<[f32; 4]> = lines().iter().map(|b| b.bounds).collect();
        let img = frame(400, 480, &boxes);
        let mut fake = Fake::new(move |_, _, _| lines());
        (fake.batch, fake.delay) = (1, Duration::from_millis(600));
        let mut shown = 0;
        let out = translate_progressively(&img, 400, 480, "ar", &Cache::new(8), &fake, |_| {
            std::thread::sleep(Duration::from_millis(600)); // a slow draw
            shown += 1;
        })
        .unwrap();
        assert_eq!((shown, fake.ai_calls.borrow().len()), (1, 2));
        // two calls of 600 ms, the 600 ms partial beside the second: ~1.2 s;
        // drawn in turn it would be 1.8 s (wide margin: CI runners are slow)
        let total = out.timings.total;
        assert!(total < Duration::from_millis(1600), "{total:?}");
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

    /// A plain model reply (no JSON array) goes on-device, not to an error card
    /// (gate 0.1.2): translated there, or that model's download asked for.
    #[test]
    fn an_unreadable_ai_reply_falls_back_to_on_device() {
        let url = crate::actions::ask::tests::stub(&["Sure! Voici la traduction : Bonjour."]);
        let real = Real::new(
            "en",
            Via::Ai {
                ai: crate::actions::ask::tests::local_setup(url).cfg,
                key: None,
            },
        );
        let done = real.lines(&["Hello".into()], "fr", "");
        assert!(!matches!(done, Err(AiError::BadResponse(_))), "{done:?}");
        assert!(done.is_ok() || real.missing().is_some(), "{done:?}");
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

    /// Footnote marks (as Windows OCR read them on the matrix pages) leave
    /// the model's input and come back after the translation; brackets that
    /// are text stay.
    #[test]
    fn footnote_marks_are_kept_out_of_the_model() {
        let cases = [
            (
                "cacti,[2] which are well-suited",
                "cacti, which are well-suited",
                "[2]",
            ),
            (
                "normal indoor conditions.[5",
                "normal indoor conditions.",
                "[5",
            ),
            (
                "cultivated indoors[ ] for aesthetic",
                "cultivated indoors for aesthetic",
                "[ ]",
            ),
            ("(composed in 1786) [24]", "(composed in 1786)", "[24]"),
            ("French original[c]", "French original", "[c]"),
            ("Lyrics [edit]", "Lyrics [edit]", ""),
            (
                "Culture [ modifier I modifier le code]",
                "Culture [ modifier I modifier le code]",
                "",
            ),
            (
                "[la sug da glwag e.t_a gi.ve II]",
                "[la sug da glwag e.t_a gi.ve II]",
                "",
            ),
            ("[24]", "", "[24]"),
        ];
        for (s, body, marks) in cases {
            assert_eq!(footnotes(s), (body.to_string(), marks.to_string()), "{s}");
        }
    }

    /// An Arabic model never reads a line with no Arabic letter (Latin OCR
    /// noise on an Arabic page: ar-de made "LILI" into "LOLI", "Jho" into
    /// "Joho!"); numbers still go.
    #[test]
    fn a_line_not_in_the_source_script_stays() {
        let route = mt::route("ar", "de").unwrap();
        let sent = RefCell::new(Vec::new());
        let run = |t: &[String], _: &'static mt::Pair| {
            sent.borrow_mut().extend(t.iter().cloned());
            Ok(t.iter().map(|s| format!("{s} (de)")).collect())
        };
        let src: Vec<String> = ["نبات منزلي", "LILI", "Jho", "12"].map(String::from).into();
        let out = hops(&src, &route, "de", run).unwrap();
        assert_eq!(out, ["نبات منزلي (de)", "LILI", "Jho", "12 (de)"]);
        assert_eq!(*sent.borrow(), ["نبات منزلي", "12"]);
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
            let route = r.routes(&texts(t), target)?.remove(0).0;
            Ok::<_, AiError>((route[0].source, route[route.len() - 1].target, route.len()))
        };
        assert_eq!(pair(&auto, &["Main menu"], "ar").unwrap(), ("en", "ar", 1));
        assert_eq!(
            pair(&auto, &["القائمة الرئيسية"], "en").unwrap(),
            ("ar", "en", 1)
        );
        assert_eq!(pair(&auto, &["メニュー"], "en").unwrap(), ("ja", "en", 1));
        // a batch of short headings alone reads as English; the whole page doesn't
        let page = Real::new(lang::AUTO, Via::Offline);
        page.read(&["Un faro es una torre de señalización luminosa situada en el litoral"]);
        assert_eq!(
            pair(&page, &["Artículo", "Discusión"], "en").unwrap().0,
            "es"
        );
        let es = ["Un faro es una torre de señalización luminosa situada en el litoral"];
        assert_eq!(pair(&auto, &es, "en").unwrap(), ("es", "en", 1));
        let same = auto
            .lines(&texts(&["The menu is open"]), "en", "")
            .unwrap_err();
        assert_eq!(
            same,
            AiError::AlreadyIn("English"),
            "a quiet note, not an error card"
        );
        assert_eq!(
            pair(&Real::new("fr", Via::Offline), &["x"], "en").unwrap(),
            ("fr", "en", 1)
        );
        // no direct model: through English, two models
        let it = Real::new("it", Via::Offline);
        assert_eq!(pair(&it, &["x"], "ar").unwrap(), ("it", "ar", 2));
        assert_eq!(
            pair(&it, &["x"], "es").unwrap(),
            ("it", "es", 1),
            "direct first"
        );
        for (real, t, target, from) in [
            (
                &Real::new("pt", Via::Offline),
                &["Olá"][..],
                "en",
                "Portuguese",
            ),
            (
                &Real::new("fr", Via::Offline),
                &["Bonjour"][..],
                "ko",
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

    /// "auto" on a paused video (the owner's Sintel take, read as one-shot
    /// translate reads it): an English player banner and page around a
    /// Spanish subtitle. Before: one guess for the whole screen (English, and
    /// the subtitle had none of the common Spanish words) and "the text is
    /// already in English". Now each line that says what it is goes its own
    /// way; the rest follows the page.
    #[test]
    fn a_foreign_subtitle_in_an_english_page_is_translated() {
        let texts = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let screen = [
            "To exit full screen, move mouse to top of screen or press Esc",
            "Esta hoja tiene un pasado oscuro.",
            "Sintel is an independently produced short film, initiated by the Blender",
            "Ha derramado mucha sangre inocente.",
        ];
        let real = Real::new(lang::AUTO, Via::Offline);
        real.read(&screen);
        let routes = real.routes(&texts(&screen), "en").unwrap();
        let got: Vec<_> = (routes.iter())
            .map(|(r, idx)| (r[0].source, r[r.len() - 1].target, idx.clone()))
            .collect();
        assert_eq!(got, [("es", "en", vec![1, 3])]);
        // a part with only the page's English: left as it is, no error
        assert!(real.routes(&texts(&screen[..1]), "en").unwrap().is_empty());
        // into Arabic, everything goes, each from its own language
        let routes = real.routes(&texts(&screen), "ar").unwrap();
        let mut got: Vec<_> = (routes.iter())
            .map(|(r, i)| (r[0].source, i.clone()))
            .collect();
        got.sort();
        assert_eq!(got, [("en", vec![0, 2]), ("es", vec![1, 3])]);
        // a page all in English still says so
        let en = [screen[0], screen[2]];
        let page = Real::new(lang::AUTO, Via::Offline);
        page.read(&en);
        let same = page.routes(&texts(&en), "en").unwrap_err();
        assert_eq!(
            same,
            AiError::AlreadyIn("English"),
            "a quiet note, not an error card"
        );
    }

    /// Ubuntu gate: a French page into German asked for Spanish -> German
    /// (112 MB) for one line its words took for Spanish, and drew nothing
    /// until it was downloaded. On a page clearly in French, a line in the
    /// same script whose own models aren't here goes the page's way.
    #[test]
    fn a_stray_line_never_holds_the_page_for_a_download() {
        let fr = [
            "Le Système solaire est le système planétaire du Soleil.",
            "Il est composé de cette étoile et des objets célestes qui tournent autour.",
            "Les planètes telluriques sont les plus proches du Soleil.",
            "Esta hoja tiene un pasado oscuro.",
        ];
        let texts: Vec<String> = fr.iter().map(|s| s.to_string()).collect();
        assert_eq!(lang::sure(fr[3]), Some("es"));
        let real = Real::new(lang::AUTO, Via::Offline);
        real.read(&fr);
        let es_here = (mt::route("es", "de").unwrap().iter()).all(|p| p.missing_bytes() == 0);
        let routes = real.routes(&texts, "de").unwrap();
        let got: Vec<_> = routes
            .iter()
            .map(|(r, i)| (r[0].source, i.clone()))
            .collect();
        if es_here {
            assert_eq!(got, [("fr", vec![0, 1, 2]), ("es", vec![3])]);
        } else {
            assert_eq!(got, [("fr", vec![0, 1, 2, 3])]);
        }
        // a page that isn't clearly one language keeps each line's own way
        let half = [fr[0], fr[3]];
        let real = Real::new(lang::AUTO, Via::Offline);
        real.read(&half);
        let texts: Vec<String> = half.iter().map(|s| s.to_string()).collect();
        assert_eq!(real.routes(&texts, "de").unwrap().len(), 2);
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

    fn blocks_of(texts: &[&str]) -> Vec<Block> {
        (texts.iter().enumerate())
            .map(|(i, t)| {
                let l = Line {
                    bounds: [10.0, 40.0 * i as f32, 300.0, 20.0],
                    text: t.to_string(),
                };
                Block {
                    text: l.text.clone(),
                    bounds: l.bounds,
                    lines: vec![l],
                }
            })
            .collect()
    }

    /// The owner's take (French Wikipedia -> en/de/ar/es): the address bar
    /// came out as "en.wikipedia.org/wiki/Plante_d%27inside". Addresses,
    /// paths, e-mail addresses, file names and identifiers are left exactly
    /// as they are; a sentence that mentions one is still translated.
    #[test]
    fn addresses_and_code_are_never_translated() {
        for code in [
            "https://fr.wikipedia.org/wiki/Plante_d%27intérieur",
            "fr.wikipedia.org/wiki/Plante_d%27intérieur",
            "ar.wikipedia.org",
            "www.example.com",
            "jean.dupont@exemple.fr",
            "C:\\Users\\test\\Documents",
            "rapport_final.pdf",
            "photo.jpg",
            "translate_lines",
            "?q=plante&lang=fr",
        ] {
            assert!(code_like(code), "{code}");
            for target in ["en", "ar", "de"] {
                assert!(!needs_translation(code, target), "{code} -> {target}");
            }
        }
        for word in [
            "etc.",
            "U.S.",
            "e.g.",
            "50%",
            "d'intérieur",
            "Wikipédia",
            "1.5",
        ] {
            assert!(!code_like(word), "{word}");
        }
        let mention = "Voir la page fr.wikipedia.org pour la liste complète des plantes";
        assert!(needs_translation(mention, "en"));
        // the fake translates everything it gets: the address never reaches it
        let fake = Fake::new(|_, _, _| Vec::new());
        let blocks = blocks_of(&["https://fr.wikipedia.org/wiki/Plante_d%27intérieur", "Lire"]);
        let tr = translate_blocks(&blocks, "en", &Cache::new(8), &fake).unwrap();
        assert_eq!(tr[0], "", "left as it was, not redrawn");
        assert_eq!(*fake.ai_calls.borrow(), [vec!["Lire".to_string()]]);
    }

    /// "[modifier | modifier le code]" came out as "[amend | modifier le
    /// code]" (OCR reads the bar as "I": one sentence to MT). Each item is
    /// translated alone and put back between its brackets and bars, a
    /// lower-case label as a label ("Modifier"), so the same label reads the
    /// same everywhere: the tab "Modifier" and the link "modifier".
    #[test]
    fn link_items_are_translated_one_by_one() {
        let s = |p: &str, t: bool| (p.to_string(), t);
        assert_eq!(
            items("Culture [ modifier I modifier le code]"),
            [
                s("Culture", true),
                s(" [ ", false),
                s("modifier", true),
                s(" | ", false),
                s("modifier le code", true),
                s("]", false),
            ]
        );
        assert_eq!(items("I think so").len(), 1, "no brackets: English I stays");
        let mut fake = Fake::new(|_, _, _| Vec::new());
        fake.answer = |s| match s {
            "Modifier" => "Edit".into(),
            "Modifier le code" => "Edit code".into(),
            "Culture" => "Culture".into(),
            "Lire" => "Read".into(),
            other => format!("?{other}?"),
        };
        let blocks = blocks_of(&[
            "Culture [ modifier I modifier le code]",
            "Lumière [ modifier | modifier le code ]",
            "Modifier",
            "Lire",
        ]);
        let tr = translate_blocks(&blocks, "en", &Cache::new(64), &fake).unwrap();
        assert_eq!(tr[0], "Culture [ edit | edit code]");
        assert_eq!(tr[1], "?Lumière? [ edit | edit code ]");
        assert_eq!((tr[2].as_str(), tr[3].as_str()), ("Edit", "Read"));
        // each label once, as a label
        let sent: Vec<String> = fake.ai_calls.borrow().concat();
        assert_eq!(
            sent,
            ["Culture", "Modifier", "Modifier le code", "Lumière", "Lire"]
        );
        // into Arabic the case doesn't matter; the bars and brackets stay
        fake.answer = |s| format!("ع{}", s.chars().count());
        let tr = translate_blocks(&blocks[..1], "ar", &Cache::new(64), &fake).unwrap();
        assert_eq!(tr[0], "ع7 [ ع8 | ع16]");
    }

    /// Through English (here German -> Arabic; French was too): a label the first model gives
    /// back unchanged ("40 langues") never goes on to English -> Arabic,
    /// which made a name of it ("إيان غوس 40"); one plain word may be English
    /// already ("Text"); an output that isn't a translation keeps the original.
    #[test]
    fn a_pivot_never_makes_up_a_translation() {
        let route = mt::route("de", "ar").unwrap();
        assert_eq!(route.len(), 2, "German -> Arabic goes through English");
        let calls = RefCell::new(Vec::new());
        let run = |t: &[String], p: &'static mt::Pair| {
            calls.borrow_mut().push((p.target, t.to_vec()));
            Ok(t.iter()
                .map(|s| match (p.target, s.as_str()) {
                    ("en", "Plante d'intérieur") => "Interior plant".into(),
                    ("en", "Sommaire") => "Contents".into(),
                    ("en", other) => other.into(),
                    ("ar", "Interior plant") => "نبتة داخلية".into(),
                    ("ar", "Contents") => "المحتويات المحتويات المحتويات المحتويات".into(),
                    ("ar", "Text") => "نص".into(),
                    ("ar", _) => "إيان غوس 40".into(),
                    _ => unreachable!(),
                })
                .collect())
        };
        let src: Vec<String> = [
            "Plante d'intérieur",
            "40 langues",
            "Lumiere",
            "Sommaire",
            "Text",
        ]
        .map(String::from)
        .into();
        let out = hops(&src, &route, "ar", run).unwrap();
        assert_eq!(
            out,
            ["نبتة داخلية", "40 langues", "Lumiere", "Sommaire", "نص"]
        );
        let second = &calls.borrow()[1];
        assert_eq!(
            second.1,
            ["Interior plant", "Lumiere", "Contents", "Text"],
            "a phrase given back unchanged stops after English, one plain word goes on"
        );
    }

    #[test]
    fn made_up_outputs_are_told_apart() {
        // offline outputs on the matrix pages before this check
        assert!(made_up("- .Jj.i.o Ü14j X", &"- ".repeat(30), "en"));
        let looped = format!("Ül-{}", "j".repeat(40));
        assert!(made_up("Ül-jß.o 'po ü4.JJj-A.Jl", &looped, "en"));
        assert!(made_up("40 langues", "إيان غوس 40 عام 1990", "ar"));
        assert!(made_up("40 langues", "أربعون لغة", "ar"), "a number lost");
        assert!(
            made_up("Standard", "Standard", "ar"),
            "not in Arabic letters"
        );
        assert!(made_up("Sommaire", "目次", "en"), "another script");
        // good ones
        assert!(!made_up("40 langues", "40 لغة", "ar"));
        assert!(!made_up("1:47 AM", "1: 47 صباحاً", "ar"));
        assert!(!made_up(
            "(novembre 2020)",
            "(تشرين الثاني/نوفمبر ٢٠٢٠)",
            "ar"
        ));
        assert!(!made_up("Plante d'intérieur", "Planta de interior", "es"));
        let caption = "Aquarelle de Carl Larsson (1894)";
        assert!(!made_up(caption, "لون الماء لكارل لارسون (1894)", "ar"));
        assert!(echo("40 langues •v", "40 Langues v"));
        // digits in a word OCR mangled are no number (es-ar on the matrix)
        let src = "cultivada en lugares bajo tech07 como casas u oficinas.";
        assert!(!made_up(
            src,
            "مزروعة في أماكن منخفضة التقنية مثل المنازل أو المكاتب.",
            "ar"
        ));
    }

    /// Windows OCR (English) on the Arabic Wikipedia page read noise: nothing
    /// was translated into German, garbage into English. Such a read goes
    /// to PP-OCR.
    #[test]
    fn a_page_windows_ocr_cannot_read_is_read_again() {
        let read = |t: &[&str]| -> Vec<TextBlock> {
            t.iter().map(|t| tb(t, [0.0, 0.0, 10.0, 10.0])).collect()
        };
        // lines of the real Windows OCR read (Win10 VM, Edge)
        let arabic = read(&[
            "d-l.-lss '4.3.1>-0-11 a-LI.>I-LII",
            "JjLi-AJl clio vol-LOJI",
            "axlj-i-o-ll üL.iLLLll",
            "Q! j ÜIju-6 öJlzll Q-ISLOJI ülju-i-ll p JS-üujs",
            "Qllj! 44-1>-0-11 ül-il-ü-ll ü.K.Aij -Lö I-OS",
            "Oil Iii",
        ]);
        assert!(misread(&arabic));
        let french = read(&[
            "Les plantes d'intérieur ou plantes d'appartement sont des plantes cultivées",
            "Faire un don Créer un compte Se connecter",
            "https://fr.wikipedia.org/wiki/Plante_d%27intérieur",
            "Culture [ modifier I modifier le code]",
        ]);
        assert!(!misread(&french));
        assert!(!misread(&read(&["OK", "1:47 AM"])), "too little to judge");
    }
}
