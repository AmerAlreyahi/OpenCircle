//! OCR: PP-OCRv5 mobile (detect lines, then read them) via oar-ocr on ONNX
//! Runtime, CPU. Neither ORT nor the models ship with the app: `ensure_models`
//! fetches them on first use (the only network access here). The app calls
//! `recognize_in_child`, so ORT and its memory live only in an
//! `opencircle ocr-worker` process that runs `Engine` (one per call, or one
//! warm one while a `Hold` lives: `session`).
#[cfg(all(test, target_os = "linux"))]
mod bench;
pub(crate) mod models;
pub mod screen;
pub(crate) mod session;
pub mod system;
pub(crate) mod worker;

pub use models::{download_size, ensure_models, models_dir, ready};
pub use session::{Hold, gently_here, hold, keep_warm, when_idle};
pub use worker::{
    WORKER_ARG, read_screen, reads_other_scripts, recognize_as, recognize_band, recognize_except,
    recognize_in_child, translate_in_child, warm, worker_main,
};

use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::TextReader;
use crate::translate::lang::{self, Script};

use image::RgbImage;
use image::imageops::FilterType::Triangle;
use oar_ocr::core::config::{OrtExecutionProvider, OrtSessionConfig};
use oar_ocr::domain::tasks::TextDetectionConfig;
use oar_ocr::predictors::{TextDetectionPredictor, TextRecognitionPredictor};
use oar_ocr::processors::LimitType;

/// Longest side the detector sees. 960 px read 35/36 lines at ~100 ms in spike
/// S4 (native 1080p 36/36 at 3x the time), but on a 1716 px wide page it
/// shrank 18 px body text to 10 px: whole lines were missed (left in English
/// between translated ones) and words misread ("10s t early 80s"). 1280 px
/// read them all, for ~50 ms more detection on 2 cores.
const DETECT_SIDE: u32 = 1280;

/// ...but it never shrinks an image more than this. Screen text is sized in
/// pixels, not in screen widths (12-22 px at 100 % scale on any monitor), and
/// below ~10 px the detector ends lines early or drops them: 3440x1440 at
/// 960 px turned 22 px text into 6 px and read "Speak to the bla".
const MIN_DETECT_SCALE: f32 = 0.5;

/// The detector's input size: the image shrunk to `DETECT_SIDE` (by no more
/// than `MIN_DETECT_SCALE`), rounded to the model's multiple of 32.
fn detect_size(width: u32, height: u32) -> (u32, u32) {
    let s = (DETECT_SIDE as f32 / width.max(height) as f32).clamp(MIN_DETECT_SCALE, 1.0);
    let side = |v: u32| (((v as f32 * s) as u32 + 16) / 32 * 32).max(32);
    (side(width), side(height))
}

// (#169: a 1080p strip owns 256 rows and sees 64 past them, `Grid`: 214 /
// 306 MB peak at 1080p / 3440 with 448: 239 / 436; a line up to twice the
// rows seen is whole in one strip.)
// ponytail: a line taller than that (a banner) can be cut by a strip's
// edge; shrunk detection for such lines if that shows up.
/// A strip's owned rows (top, bottom).
type Strip = (u32, u32);
/// Rows a Latin band sees past its own (`Engine::band`): main mode's bands
/// overlap by 24 rows (`ocr::screen`), so a line up to 48 rows tall is whole.
const BAND_SEE: f32 = 24.0;

/// The strips (owned rows: top, bottom) of an image `height` tall, `owned`
/// rows each, that own rows of `rows` (all for `None`).
fn strips(height: u32, owned: u32, rows: Option<(f32, f32)>) -> Vec<(u32, u32)> {
    (0..height.div_ceil(owned))
        .map(|s| (s * owned, ((s + 1) * owned).min(height)))
        .filter(|&(a, b)| rows.is_none_or(|(top, bottom)| (a as f32) < bottom && top < b as f32))
        .collect()
}

/// A line's height (image px) as the shrunk detector finds 15 px text at
/// 100 % (#169, corpus medians: 26-31 px at 1920 and 3440 wide; 46.6 px at
/// 150 %): the non-Latin readers' lines are found at `LINE_PX` / the
/// screen's median, at most full size. Larger text is found smaller.
const LINE_PX: f32 = 30.0;

/// A strip's detector input at most this many pixels (1920 x 384: one
/// 1080p strip). A 3440 strip at full size was 3440 x 384: helper peak 334
/// -> 400 MB on the 3440 article.
const STRIP_PIXELS: f32 = 1920.0 * 384.0;

/// How a screen's non-Latin lines are found (#169): the detector's scale
/// (from the lines' height), rows per strip and rows seen past them (from
/// the width: `STRIP_PIXELS`), and whether the shrunk lines do (the scale is
/// within 15 % of the shrunk detector's).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    scale: f32,
    owned: u32,
    see: u32,
    shrunk: bool,
}

impl Grid {
    /// For an image `width` wide whose lines found shrunk (at `shrunk` scale)
    /// are `line` px tall (median).
    /// `bands`: Linux main mode's bands (the shrunk scale is taken within
    /// 30 % of it: at 150 % 0.5 instead of 0.64, +60 % time against main's
    /// bands and CER 0.105 either way; the whole-screen paths keep 15 %: at
    /// 0.5 the 150 % page read CER 0.038 -> 0.075 there).
    fn new(width: u32, line: f32, shrunk: f32, bands: bool) -> Grid {
        let scale = (LINE_PX / line.max(1.0)).clamp(MIN_DETECT_SCALE, 1.0);
        let near = if bands { 1.3 } else { 1.15 };
        let shrunk_ok = scale <= shrunk * near;
        let scale = if shrunk_ok { shrunk } else { scale };
        let px = width as f32 * scale * scale;
        let rows = ((STRIP_PIXELS / px.max(1.0)) as u32).clamp(128, 384);
        let see = rows / 6;
        Grid {
            scale,
            owned: rows - 2 * see,
            see,
            shrunk: shrunk_ok,
        }
    }
}

/// The median height of `boxes` (0 without).
fn median_height(boxes: &[[f32; 4]]) -> f32 {
    let mut h: Vec<f32> = boxes.iter().map(|b| b[3]).collect();
    h.sort_by(f32::total_cmp);
    h.get(h.len() / 2).copied().unwrap_or(0.0)
}

/// Lines read per recognizer run, at most. One run over all 72 lines of a busy
/// 1080p screen grew RAM to 1.2 GB (ORT keeps its peak); 8 kept it ~250 MB at
/// the same speed.
const REC_BATCH: usize = 8;

/// A run's padded size, in columns at the recognizer's 48 px height (lines x
/// the widest line): ORT's peak grows with it (~29 KB per column per line), so
/// eight full-width lines (8 x 4500) cost ~1 GB while eight of 1500 cost 326
/// MB. Ordinary pages stay at 8 lines per run.
const REC_BATCH_COLUMNS: f32 = 12_000.0;

/// How many of `boxes` (widest last, as `recognize_rows` sorts them) go in
/// the first run: up to `REC_BATCH`, fewer while the widest would pad past
/// `REC_BATCH_COLUMNS`; always at least one.
fn rec_batch_len(boxes: &[[f32; 4]]) -> usize {
    let mut n = 0;
    let mut widest = 0f32;
    for b in boxes.iter().take(REC_BATCH) {
        widest = widest.max(b[2] / b[3].max(1.0) * 48.0);
        if n > 0 && (n + 1) as f32 * widest > REC_BATCH_COLUMNS {
            break;
        }
        n += 1;
    }
    n
}

/// `boxes` as recognizer runs (indices): sorted by shape (narrow first) and
/// split by `rec_batch_len`. A run is padded to its widest line, so alike
/// shapes go together (as PaddleOCR does); callers put the lines in reading
/// order. The second reader batched in detection order: a 1080p Arabic
/// article took 2418 ms on 2 cores, sorted 1264 ms (#169).
fn runs(boxes: &[[f32; 4]]) -> Vec<Vec<usize>> {
    let ratio = |b: &[f32; 4]| b[2] / b[3].max(1.0);
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| ratio(&boxes[a]).total_cmp(&ratio(&boxes[b])));
    let sorted: Vec<[f32; 4]> = order.iter().map(|&i| boxes[i]).collect();
    let mut out = Vec::new();
    let mut from = 0;
    while from < sorted.len() {
        let n = rec_batch_len(&sorted[from..]);
        out.push(order[from..from + n].to_vec());
        from += n;
    }
    out
}

/// The longest line, in heights, recognizer `i` reads in one piece (#169).
/// The Arabic reader misreads long lines inside the model (CER 0.12 under 10
/// heights, 0.42-0.58 at 10-45): cut at 15, CER-all on the Arabic pages
/// went 0.413 -> 0.301 and the 3440 article 2496 -> 1467 ms. The others read
/// a line well up to their input cap (3200 columns at 48 px high: 66.7
/// heights; past it the line is squeezed and lost) and cut pieces worse
/// (the English reader read "n s.al and waves"), so only past that.
// ponytail: measured for Arabic and English; Cyrillic and CJK keep the cap.
fn longest(i: usize) -> f32 {
    match models::RECOGNIZERS[i].rtl() {
        true => 15.0,
        false => 66.0, // (cut at 60, a French page's 61-65 heights lines read worse)
    }
}

/// Line `b` of `img` in pieces of at most `most` heights: each cut in the
/// widest empty gap (a word space is wider than the gap between letters)
/// within 3 heights of its even place, else in the emptiest column there.
/// A line no longer than that is one piece.
fn cut(img: &RgbImage, b: &[f32; 4], most: f32) -> Vec<[f32; 4]> {
    let ratio = b[2] / b[3].max(1.0);
    let [x0, y0, w, h] = b.map(|v| v as u32);
    if ratio <= most || w == 0 || h == 0 {
        return vec![*b];
    }
    let k = (ratio / (most * 0.8)).ceil() as u32;
    let luma = |x: u32, y: u32| {
        let p = img.get_pixel(x.min(img.width() - 1), y.min(img.height() - 1));
        (p[0] as i32 * 3 + p[1] as i32 * 6 + p[2] as i32) / 10
    };
    // the background: the box's most common brightness on its top and bottom rows
    let mut edge: Vec<i32> = (x0..x0 + w)
        .flat_map(|x| [luma(x, y0), luma(x, y0 + h - 1)])
        .collect();
    edge.sort_unstable();
    let bg = edge[edge.len() / 2];
    let ink = |x: u32| {
        (y0..y0 + h)
            .filter(|&y| (luma(x, y) - bg).abs() > 40)
            .count()
    };
    let mut cuts = vec![x0];
    for i in 1..k {
        let ideal = x0 + w * i / k;
        let lo = ideal.saturating_sub(h * 3).max(cuts[cuts.len() - 1] + h);
        let hi = (ideal + h * 3).min(x0 + w - h);
        if lo >= hi {
            continue;
        }
        let empty: Vec<bool> = (lo..hi).map(|x| ink(x) == 0).collect();
        let mut best: Option<(usize, u32)> = None; // (gap width, its middle)
        let mut j = 0;
        while j < empty.len() {
            if !empty[j] {
                j += 1;
                continue;
            }
            let start = j;
            while j < empty.len() && empty[j] {
                j += 1;
            }
            let mid = lo + ((start + j - 1) / 2) as u32;
            let width = j - start;
            if best.is_none_or(|(w, m)| {
                width > w || (width == w && mid.abs_diff(ideal) < m.abs_diff(ideal))
            }) {
                best = Some((width, mid));
            }
        }
        let at = best.map(|(_, m)| m).unwrap_or_else(|| {
            (lo..hi)
                .min_by_key(|&x| (ink(x), x.abs_diff(ideal)))
                .unwrap_or(ideal)
        });
        cuts.push(at);
    }
    cuts.push(x0 + w);
    (cuts.windows(2))
        .filter(|c| c[1] > c[0] + 2)
        .map(|c| [c[0] as f32, b[1], (c[1] - c[0]) as f32, b[3]])
        .collect()
}

/// A line's pieces as one block over its whole box `b`, in reading order
/// (right to left for `rtl`), joined by a space; `None` if none was read.
fn joined(mut parts: Vec<TextBlock>, b: [f32; 4], rtl: bool) -> Option<TextBlock> {
    if parts.len() <= 1 {
        return parts.pop().map(|p| TextBlock { bounds: b, ..p });
    }
    parts.sort_by(|p, q| p.bounds[0].total_cmp(&q.bounds[0]));
    if rtl {
        parts.reverse();
    }
    let chars = |p: &TextBlock| p.text.chars().count() as f32;
    let n: f32 = parts.iter().map(chars).sum();
    let confidence = parts.iter().map(|p| p.confidence * chars(p)).sum::<f32>() / n.max(1.0);
    let text = (parts.iter().map(|p| p.text.as_str())).collect::<Vec<_>>();
    Some(TextBlock {
        text: text.join(" "),
        bounds: b,
        confidence,
    })
}

/// One line of text found in the image.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextBlock {
    pub text: String,
    /// x, y, w, h in image pixels (axis-aligned box around the line).
    pub bounds: [f32; 4],
    /// Recognizer confidence, 0..1.
    pub confidence: f32,
}

#[derive(Debug)]
pub enum OcrError {
    /// The pixel buffer is empty or doesn't match width x height x 4.
    BadImage(String),
    /// A model download failed (network, HTTP status).
    Download(String),
    /// A downloaded model doesn't match its pinned size and SHA-256.
    Checksum(&'static str),
    /// Every retry failed: the server answered with an error (`server`:
    /// 5xx, 429) or couldn't be reached. The detail is in the log.
    Unreachable { server: bool },
    /// Reading or writing the model files.
    Io(std::io::Error),
    /// What isn't on disk yet (the user hasn't agreed to its download):
    /// checked before a helper starts or loads ONNX Runtime.
    Missing(String),
    /// ONNX Runtime failed to load a model or run it.
    Engine(String),
    /// A download was stopped on request (`models::ensure_until`); its
    /// `.part` file stays, to continue.
    Stopped,
}

impl fmt::Display for OcrError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::BadImage(m) => write!(f, "bad image: {m}"),
            Self::Download(m) => write!(f, "could not download the text models: {m}"),
            Self::Checksum(name) => write!(f, "downloaded model {name} is corrupt (checksum)"),
            Self::Unreachable { server: true } => f.write_str(
                "The download server had a problem. Try again in a minute.",
            ),
            Self::Unreachable { server: false } => f.write_str(
                "Could not reach the download server. Check your internet connection, then try again in a minute.",
            ),
            Self::Io(e) => write!(f, "text model files: {e}"),
            Self::Missing(what) => write!(
                f,
                "{what} isn't downloaded yet. Translate once with Ctrl+Shift+T to download it"
            ),
            Self::Engine(m) => write!(f, "text recognition failed: {m}"),
            Self::Stopped => f.write_str("the download was stopped"),
        }
    }
}

impl std::error::Error for OcrError {}

impl From<std::io::Error> for OcrError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

fn engine_err(e: impl fmt::Display) -> OcrError {
    OcrError::Engine(e.to_string())
}

/// Loads ONNX Runtime from `dir` (once per process). A missing file is
/// `Missing`, never a load attempt: on Windows that failed as
/// `LoadLibraryExW` (read as a missing VC++ runtime) and a later ORT call
/// panicked ("OrtGetApiBase must be present"), killing the helper.
pub(crate) fn init_ort(dir: &Path) -> Result<(), OcrError> {
    let path = dir.join(models::ORT.name);
    if !path.is_file() {
        return Err(OcrError::Missing("the text-recognition model".into()));
    }
    ort::init_from(path).map_err(load_err)?.commit();
    Ok(())
}

/// The ORT DLL is present and checksum-verified (`init_ort` checks first),
/// so on Windows a failed `LoadLibraryExW` means one of its own imports
/// (MSVCP140, VCRUNTIME140_1) is missing. We never bundle or download the
/// redistributable: we say where to get it.
pub(crate) fn load_err(e: impl fmt::Display) -> OcrError {
    let msg = e.to_string();
    if msg.contains("LoadLibraryExW") {
        OcrError::Engine(format!(
            "Install the Microsoft Visual C++ Redistributable (x64): \
             https://aka.ms/vs/17/release/vc_redist.x64.exe ({msg})"
        ))
    } else {
        OcrError::Engine(msg)
    }
}

/// Which recognizer reads the lines: one (an index into
/// `models::RECOGNIZERS`), or `Auto`: the last good one (English at first),
/// and when its lines look wrong, every other one on disk on the same lines;
/// the best stays loaded and is tried first next time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Read {
    One(usize),
    Auto,
}

impl Read {
    /// For a source language: its recognizer, or `Auto` for "auto".
    pub fn for_source(source: &str) -> Read {
        if source == crate::translate::lang::AUTO {
            return Read::Auto;
        }
        let rec = models::recognizer_for(source);
        Read::One(index(rec))
    }

    /// One byte on the wire.
    fn byte(self) -> u8 {
        match self {
            Read::One(i) => i as u8,
            Read::Auto => u8::MAX,
        }
    }

    fn from_byte(b: u8) -> Option<Read> {
        match b {
            u8::MAX => Some(Read::Auto),
            i if (i as usize) < models::RECOGNIZERS.len() => Some(Read::One(i as usize)),
            _ => None,
        }
    }
}

/// Who reads main mode's screen text (#62): Windows' own OCR (the whole
/// screen at once, `system`) or the text reader helper (PP-OCR, band by band).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    System,
    Helper,
}

/// The `setting`'s reader, given the Windows OCR language (`system::known`:
/// `None` = not asked yet, `Some(None)` = none). Auto: Windows OCR, but not
/// for a right-to-left language (PP-OCR reads Arabic better) nor without a
/// recognizer (#63; Linux never has one); not asked yet = try it (a failed
/// read falls back to the helper). Live translate follows it too (#56, for
/// its source language); selections and one-shot translation keep PP-OCR.
pub fn route(setting: TextReader, system: Option<Option<&str>>) -> Source {
    match (setting, system) {
        (TextReader::PpOcr, _) | (_, Some(None)) => Source::Helper,
        (TextReader::Auto, Some(Some(tag))) if crate::translate::lang::is_rtl(tag) => {
            Source::Helper
        }
        _ => Source::System,
    }
}

fn index(rec: &models::Recognizer) -> usize {
    (models::RECOGNIZERS.iter())
        .position(|r| r.id == rec.id)
        .unwrap_or(0)
}

/// The share of `text`'s letters in `script` (0 without letters).
fn share(text: &str, script: Script) -> f32 {
    let letters = text.chars().filter(|c| c.is_alphabetic());
    let (mine, all) = letters.fold((0, 0), |(m, a), c| (m + script.has(c) as usize, a + 1));
    mine as f32 / all.max(1) as f32
}

/// `b`'s middle is in `k` (boxes: x, y, w, h).
fn holds(k: &[f32; 4], b: &[f32; 4]) -> bool {
    let (x, y) = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
    (k[0]..k[0] + k[2]).contains(&x) && (k[1]..k[1] + k[3]).contains(&y)
}

/// Lines `left` of `all` for the non-Latin readers: the strips owning them
/// to find again at full size, or, when they are few (a fifth or less: a
/// page in the user's language with a stray line or icon), themselves as
/// found (an English page's second reader went 963 -> 1900 ms on the Win10
/// VM re-finding three of them at full size).
fn few(
    strips: Vec<(u32, u32)>,
    left: Vec<[f32; 4]>,
    all: usize,
) -> (Vec<(u32, u32)>, Vec<[f32; 4]>) {
    match left.len() * 5 <= all {
        true => (Vec::new(), left),
        false => (owning(strips, &left), Vec::new()),
    }
}

/// Of `strips`, those that own the middle of one of `lines`.
fn owning(strips: Vec<(u32, u32)>, lines: &[[f32; 4]]) -> Vec<(u32, u32)> {
    let owns = |&(a, b): &(u32, u32)| {
        (lines.iter()).any(|l| (a as f32..b as f32).contains(&(l[1] + l[3] / 2.0)))
    };
    strips.into_iter().filter(owns).collect()
}

/// Text lines' boxes (x, y, w, h; image px) and the size detection ran at.
type Detected = (Vec<[f32; 4]>, (u32, u32));

/// How well a recognizer for `script` read `boxes` lines: the share of
/// lines it read at all, times their mean confidence (weighted by length),
/// times the share of its letters in its own script. A model shown a script
/// it doesn't know reads few lines, or reads them unsure; one whose list
/// also has Latin letters (all of them) reads Latin text well, but then it
/// is Latin. Measured (`translate::tests::languages_end_to_end`): the
/// English model on Arabic lines read none (0.0), on Russian 1 of 2 at 0.88
/// (0.44); the Arabic model on French at 0.98, but in Latin letters (0.0);
/// each model on its own script 0.97-1.0.
// ponytail: a threshold on the model's own confidence; a script classifier
// if it misfires.
fn quality(blocks: &[TextBlock], boxes: usize, script: Script) -> f32 {
    if boxes == 0 {
        return 1.0; // no text: nothing to read better
    }
    let letters = blocks
        .iter()
        .flat_map(|b| b.text.chars())
        .filter(|c| c.is_alphabetic());
    let (mine, all) = letters.fold((0, 0), |(m, a), c| (m + script.has(c) as usize, a + 1));
    let own = if all == 0 {
        1.0
    } else {
        mine as f32 / all as f32
    };
    let chars = |b: &TextBlock| b.text.chars().count().max(1) as f32;
    let n: f32 = blocks.iter().map(chars).sum();
    let mean = blocks.iter().map(|b| b.confidence * chars(b)).sum::<f32>() / n.max(1.0);
    mean * own * blocks.len() as f32 / boxes as f32
}

/// Below this `quality`, Linux main mode's `Auto` (`Engine::recognize_band`)
/// tries the other recognizers.
const AUTO_QUALITY: f32 = 0.8;

/// The Arabic model reads glyphs left to right; PaddleOCR reverses its
/// output (`pred_reverse`): runs of Latin letters, digits, `:*./%+-` and
/// spaces keep their order, the sequence of runs and other characters is
/// reversed. So "ةيسيئرلا ةمئاقلا" -> "القائمة الرئيسية". Unlike PaddleOCR,
/// a run's outer spaces stay between it and its neighbours.
fn reverse_rtl(text: &str) -> String {
    let keep = |c: char| c.is_ascii_alphanumeric() || " :*./%+-".contains(c);
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let len = match keep(c) {
            true => rest.find(|c| !keep(c)).unwrap_or(rest.len()),
            false => c.len_utf8(),
        };
        let (part, tail) = rest.split_at(len);
        let core = part.trim();
        if core.is_empty() {
            parts.push(part);
        } else {
            let lead = &part[..part.len() - part.trim_start().len()];
            let trail = &part[part.trim_end().len()..];
            parts.extend([lead, core, trail].into_iter().filter(|s| !s.is_empty()));
        }
        rest = tail;
    }
    parts.into_iter().rev().collect()
}

/// ORT plus loaded models. Runs in the `ocr-worker` process only.
pub struct Engine {
    dir: PathBuf,
    det: TextDetectionPredictor,
    ort: OrtSessionConfig,
    /// The loaded recognizers, the last used last: two at most (a page read
    /// line by line takes a Latin and a non-Latin one; a third replaces the
    /// one used longest ago).
    recs: Vec<(usize, TextRecognitionPredictor)>,
    /// `Auto`'s first try: the recognizer that read most lines last time.
    auto: usize,
    /// The last Latin and non-Latin recognizers that read most lines.
    latin: usize,
    other: Option<usize>,
    /// `recognize_band`'s first try: the last recognizer that read well.
    band_auto: usize,
    /// The screen `read_screen` reads a part at a time.
    frame: Option<Frame>,
}

/// `Engine::read_screen`'s error for a screen it doesn't hold (the helper
/// started again): the app sends the picture once more.
pub const NO_FRAME: &str = "the screen isn't held";

/// A screen read a part at a time: its picture and the lines found on it
/// (shrunk, and at full size by strip: owned rows -> lines).
pub(crate) struct Frame {
    key: u64,
    img: RgbImage,
    shrunk: Option<Vec<[f32; 4]>>,
    strips: Vec<(Strip, Vec<[f32; 4]>)>,
    /// Lines already read whatever the rows (`few`).
    read: Vec<[f32; 4]>,
    /// How its non-Latin lines are found (`Engine::grid`).
    grid: Option<Grid>,
    /// Every line to read was read (`ScreenRead::done`).
    done: bool,
    /// Lines of a Latin and a non-Latin script were kept: both recognizers
    /// stay loaded between its parts (else the one that read nothing is
    /// freed after each part: a second held a 3440 helper ~40 MB higher).
    two: bool,
    /// `ScreenRead::noise` of the last part read.
    noise: u32,
    /// The lines Latin bands found (`Engine::band`) when the whole screen
    /// was never found shrunk.
    seen: Vec<[f32; 4]>,
}

impl Frame {
    fn new(key: u64, img: RgbImage) -> Frame {
        Frame {
            key,
            img,
            shrunk: None,
            strips: Vec::new(),
            read: Vec::new(),
            grid: None,
            done: false,
            two: false,
            noise: 0,
            seen: Vec::new(),
        }
    }
}

/// A part of a screen read (`Engine::read_screen`): its lines, and every
/// line found on the screen that isn't known (image px).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenRead {
    pub lines: Vec<TextBlock>,
    pub found: Vec<[f32; 4]>,
    /// The whole screen is read (a few lines left, read at once: `few`):
    /// no band needs asking for.
    #[serde(default)]
    pub done: bool,
    /// Lines the Latin reader read as noise with no other reader on disk
    /// (another script whose reader isn't installed).
    #[serde(default)]
    pub noise: u32,
}

impl Engine {
    /// Loads ORT and the detector from `models` (the folder `ensure_models`
    /// returns); a recognizer loads on first use.
    pub fn load(models: &Path) -> Result<Engine, OcrError> {
        init_ort(models)?;
        if !models.join(models::DET.name).is_file() {
            return Err(OcrError::Missing("the text-recognition model".into()));
        }
        // Memory patterns cache a plan per input shape; every capture has a new
        // shape, so they only add RAM (~100 MB measured).
        // No spin-waiting threads after a run: in a warm helper they kept
        // the CPU busy while the app rendered (a live step 0.6 s -> 0.2 s).
        let ort = OrtSessionConfig::new()
            .with_memory_pattern(false)
            .add_config_entry("session.intra_op.allow_spinning", "0")
            .add_config_entry("session.inter_op.allow_spinning", "0")
            // The CPU EP registered as ours = no ORT memory arena: a warm
            // helper kept every capture's peak (~210 MB more after one 1080p
            // read) for about the same speed.
            .with_execution_providers(vec![OrtExecutionProvider::CPU]);
        let det = TextDetectionPredictor::builder()
            .with_config(TextDetectionConfig {
                score_threshold: 0.3,
                box_threshold: 0.6,
                unclip_ratio: 1.5,
                max_candidates: 1000,
                // `recognize` sizes the input itself (`detect_size`), so
                // these never resize it.
                limit_side_len: Some(u32::MAX),
                limit_type: Some(LimitType::Max),
                max_side_len: Some(u32::MAX),
            })
            .with_ort_config(ort.clone())
            .build(models.join(models::DET.name))
            .map_err(engine_err)?;
        Ok(Engine {
            dir: models.into(),
            det,
            ort,
            recs: Vec::new(),
            auto: 0,
            latin: 0,
            other: None,
            band_auto: 0,
            frame: None,
        })
    }

    /// Recognizer `i`, loaded (`recs`).
    fn recognizer(&mut self, i: usize) -> Result<&TextRecognitionPredictor, OcrError> {
        if let Some(k) = self.recs.iter().position(|(n, _)| *n == i) {
            let rec = self.recs.remove(k);
            self.recs.push(rec);
        } else {
            if self.recs.len() >= 2 {
                self.recs.remove(0); // freed before the next one loads
            }
            let files = models::RECOGNIZERS[i];
            if !models::installed(files) {
                let what = format!("the {} text-recognition model", files.script);
                return Err(OcrError::Missing(what));
            }
            let rec = TextRecognitionPredictor::builder()
                .dict_path(self.dir.join(files.dict.name))
                .with_ort_config(self.ort.clone())
                .build(self.dir.join(files.model.name))
                .map_err(engine_err)?;
            self.recs.push((i, rec));
        }
        Ok(&self.recs.last().expect("just loaded").1)
    }

    /// Loads the recognizer `read` starts with (the engine is loaded): the
    /// first read after it is quick.
    pub fn warm(&mut self, read: Read) -> Result<(), OcrError> {
        let first = match read {
            Read::One(i) => i,
            Read::Auto => self.auto,
        };
        self.recognizer(first).map(drop)
    }

    /// Finds and reads the text lines in an RGBA image, in reading order.
    pub fn recognize(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        read: Read,
    ) -> Result<Vec<TextBlock>, OcrError> {
        self.recognize_rows(rgba, width, height, read, None)
    }

    /// `recognize`, reading only the lines whose middle is in `rows` (top,
    /// bottom; all of them for `None`): recognition is the costly part.
    pub fn recognize_rows(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        read: Read,
        rows: Option<(u32, u32)>,
    ) -> Result<Vec<TextBlock>, OcrError> {
        let start = std::time::Instant::now();
        let img = to_rgb(rgba, width, height)?;
        let rows = rows.map(|(top, bottom)| (top as f32, bottom as f32));
        let Read::One(i) = read else {
            return self.per_line(&mut Frame::new(0, img), rows, &[], true);
        };
        let (boxes, (dw, dh)) = self.lines_for(i, &img, rows)?;
        let detected = start.elapsed();
        let lines = self.read_lines(i, &img, &boxes)?;
        crate::log!(
            "[opencircle] ocr {width}x{height}: detect at {dw}x{dh} {} ms, {} lines read in {} ms",
            detected.as_millis(),
            boxes.len(),
            (start.elapsed() - detected).as_millis()
        );
        Ok(reading_order(lines, models::RECOGNIZERS[i].rtl()))
    }

    /// Linux main mode's band of the screen text, read as before #169 (owner,
    /// 2026-11-12: Linux main mode keeps this path until its follow-up; the
    /// app's plug-in point is `App::read_band`): the band's crop detected
    /// shrunk, only the lines whose middle is in `rows` (top, bottom) read,
    /// batched by shape; `Auto` reads them with the last recognizer that read
    /// well, then with every other below `AUTO_QUALITY`. One recognizer
    /// loaded at a time.
    pub fn recognize_band(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        read: Read,
        rows: (u32, u32),
    ) -> Result<Vec<TextBlock>, OcrError> {
        let start = std::time::Instant::now();
        let img = to_rgb(rgba, width, height)?;
        let (boxes, (dw, dh)) = self.detect(&img, width, height)?;
        let (top, bottom) = (rows.0 as f32, rows.1 as f32);
        let mut boxes: Vec<[f32; 4]> = (boxes.into_iter())
            .filter(|b| (top..bottom).contains(&(b[1] + b[3] / 2.0)))
            .collect();
        boxes.sort_by(|a, b| (a[2] / a[3]).total_cmp(&(b[2] / b[3])));
        let first = match read {
            Read::One(i) => i,
            Read::Auto => self.band_auto,
        };
        let n = boxes.len();
        let q =
            |i: usize, blocks: &[TextBlock]| quality(blocks, n, models::RECOGNIZERS[i].script());
        let detected = start.elapsed();
        let mut best = (first, self.read_band_lines(first, &img, &boxes)?);
        if read == Read::Auto && q(first, &best.1) < AUTO_QUALITY {
            for i in 0..models::RECOGNIZERS.len() {
                if i == first || !models::installed(models::RECOGNIZERS[i]) {
                    continue;
                }
                let other = self.read_band_lines(i, &img, &boxes)?;
                if q(i, &other) > q(best.0, &best.1) {
                    best = (i, other);
                }
            }
            self.band_auto = best.0;
        }
        crate::log!(
            "[opencircle] ocr {width}x{height}: detect at {dw}x{dh} {} ms, {n} lines read in {} ms",
            detected.as_millis(),
            (start.elapsed() - detected).as_millis()
        );
        Ok(reading_order(best.1, models::RECOGNIZERS[best.0].rtl()))
    }

    /// `recognize_band`'s reading: recognizer `i` alone (any other freed
    /// first) on `boxes` (sorted by shape), whole, in runs of `rec_batch_len`.
    fn read_band_lines(
        &mut self,
        i: usize,
        img: &RgbImage,
        boxes: &[[f32; 4]],
    ) -> Result<Vec<TextBlock>, OcrError> {
        self.recs.retain(|(n, _)| *n == i);
        let rtl = models::RECOGNIZERS[i].rtl();
        let rec = self.recognizer(i)?;
        let mut blocks = Vec::with_capacity(boxes.len());
        let mut rest = boxes;
        while !rest.is_empty() {
            let (chunk, tail) = rest.split_at(rec_batch_len(rest));
            rest = tail;
            let crops = (chunk.iter())
                .map(|b| {
                    let [x, y, w, h] = b.map(|v| v as u32);
                    image::imageops::crop_imm(img, x, y, w, h).to_image()
                })
                .collect();
            let rec = rec.predict(crops).map_err(engine_err)?;
            for (bounds, (text, confidence)) in chunk.iter().zip(rec.texts.iter().zip(rec.scores)) {
                let text = text.trim();
                if !text.is_empty() {
                    blocks.push(TextBlock {
                        text: if rtl { reverse_rtl(text) } else { text.into() },
                        bounds: *bounds,
                        confidence,
                    });
                }
            }
        }
        Ok(blocks)
    }

    /// The lines of `img` whose middle is in `rows` (all for `None`) and not
    /// in a `known` box, each read by the reader that suits it (#169; `Auto`
    /// read the whole page with one, then with every other below 0.8
    /// `quality`). The reader that read most lines last time reads first. A
    /// Latin reading that isn't real words (`lang::real_lines`), and every
    /// line a Latin reader didn't read, go to the non-Latin readers (their
    /// lines found again at full size), each reading only what the one
    /// before left; one that reads a line in its own script (3 letters in
    /// 10: a mixed Arabic line stays Arabic) keeps it. What none keeps is a
    /// Latin line: the Latin reader's if `latin`, else dropped (Windows OCR
    /// read it: main mode's and translate's second reader).
    fn per_line(
        &mut self,
        f: &mut Frame,
        rows: Option<(f32, f32)>,
        known: &[[f32; 4]],
        latin: bool,
    ) -> Result<Vec<TextBlock>, OcrError> {
        let start = std::time::Instant::now();
        let (w, h) = f.img.dimensions();
        let installed = |i: usize| models::installed(models::RECOGNIZERS[i]);
        let script = |i: usize| models::RECOGNIZERS[i].script();
        let mut others: Vec<usize> = (0..models::RECOGNIZERS.len())
            .filter(|&i| script(i) != Script::Latin && installed(i))
            .collect();
        others.sort_by_key(|&i| Some(i) != self.other); // the last winner first
        let latin_reader = [self.latin, 0]
            .into_iter()
            .find(|&i| installed(i) && script(i) == Script::Latin)
            .filter(|_| latin);
        let unknown = |b: &[f32; 4]| !known.iter().any(|k| holds(k, b));
        let mut kept: Vec<(usize, TextBlock)> = Vec::new();
        // Latin readings not kept: a line no other reader keeps keeps it
        let mut fallback: Vec<TextBlock> = Vec::new();
        let latin_first =
            latin_reader.filter(|_| script(self.auto) == Script::Latin || others.is_empty());
        let inside = |b: &[f32; 4]| {
            let middle = b[1] + b[3] / 2.0;
            rows.is_none_or(|(top, bottom)| (top..bottom).contains(&middle))
        };
        // (the strips to find again at full size, the lines to read as found)
        let (full, direct): (Vec<(u32, u32)>, Vec<[f32; 4]>) = match (latin_first, known.is_empty())
        {
            (Some(l), _) => {
                let boxes = match rows {
                    Some(rows) => {
                        let boxes = self.band(&f.img, rows)?;
                        f.seen.extend_from_slice(&boxes);
                        boxes
                    }
                    None => self.shrunk(f)?,
                };
                let boxes: Vec<[f32; 4]> = (boxes.into_iter())
                    .filter(|b| inside(b) && unknown(b))
                    .collect();
                let read = self.read_each(l, &f.img, &boxes)?;
                let texts: Vec<&str> = (read.iter())
                    .map(|r| r.as_ref().map_or("", |b| b.text.as_str()))
                    .collect();
                let real = lang::real_lines(&texts);
                let mut left: Vec<([f32; 4], Option<TextBlock>)> = Vec::new();
                for ((b, r), real) in boxes.iter().zip(read).zip(real) {
                    match r {
                        Some(r) if real => kept.push((l, r)),
                        r => left.push((*b, r)),
                    }
                }
                // the other Latin readers next (as `Auto` did: on a 3440
                // English page shrunk 2x the Latin reader read lines the
                // English one garbled, CER-all 0.413 against 0.540)
                for o in (0..models::RECOGNIZERS.len())
                    .filter(|&o| o != l && script(o) == Script::Latin && installed(o))
                {
                    if left.is_empty() {
                        break;
                    }
                    let these: Vec<[f32; 4]> = left.iter().map(|(b, _)| *b).collect();
                    let read = self.read_each(o, &f.img, &these)?;
                    let texts: Vec<&str> = (read.iter())
                        .map(|r| r.as_ref().map_or("", |b| b.text.as_str()))
                        .collect();
                    let real = lang::real_lines(&texts);
                    let mut rest = Vec::new();
                    for ((b, before), (r, real)) in left.into_iter().zip(read.into_iter().zip(real))
                    {
                        match r {
                            Some(r) if real => kept.push((o, r)),
                            r => rest.push((b, before.or(r))),
                        }
                    }
                    left = rest;
                }
                let lines: Vec<[f32; 4]> = left.iter().map(|(b, _)| *b).collect();
                fallback.extend(left.into_iter().filter_map(|(_, r)| r));
                // (the grid only when strips are needed: it may find a band)
                let strips = match lines.len() * 5 <= boxes.len() {
                    true => Vec::new(),
                    false => strips(h, self.grid(f, rows)?.owned, rows),
                };
                few(strips, lines, boxes.len())
            }
            // found shrunk first (cheap): only the strips with lines left
            // are found again at full size
            (None, false) => {
                let boxes = self.shrunk(f)?;
                let all = boxes.len();
                let left: Vec<[f32; 4]> = boxes.into_iter().filter(|b| unknown(b)).collect();
                // (judged on the whole screen: a few left are read all at
                // once, in one run, whatever the rows; the English page's
                // second reader read them band by band in 1641 ms, at once 963)
                let strips = match left.len() * 5 <= all {
                    true => Vec::new(),
                    false => strips(h, self.grid(f, rows)?.owned, rows),
                };
                match few(strips, left, all) {
                    (full, direct) if full.is_empty() => {
                        let direct: Vec<[f32; 4]> = (direct.into_iter())
                            .filter(|b| !f.read.contains(b))
                            .collect();
                        f.read.extend_from_slice(&direct);
                        f.done = true;
                        (full, direct)
                    }
                    (full, _) => (full, Vec::new()),
                }
            }
            (None, true) => (strips(h, self.grid(f, rows)?.owned, rows), Vec::new()),
        };
        let first = kept.len();
        f.noise = match others.is_empty() {
            true => fallback.len() as u32,
            false => 0,
        };
        let mut boxes: Vec<[f32; 4]> = match others.is_empty() || full.is_empty() {
            true => Vec::new(),
            false => self.full(f, &full)?,
        };
        let direct_any = !direct.is_empty();
        boxes.extend(direct);
        boxes.retain(|b| {
            (direct_any || inside(b))
                && unknown(b)
                && !kept.iter().any(|(_, k)| holds(&k.bounds, b))
        });
        let mut last: Vec<Option<TextBlock>> = vec![None; boxes.len()];
        for &o in &others {
            if boxes.is_empty() {
                break;
            }
            let read = self.read_each(o, &f.img, &boxes)?;
            let (mut rest, mut rest_last) = (Vec::new(), Vec::new());
            for ((b, r), before) in boxes.into_iter().zip(read).zip(last) {
                match r {
                    Some(r) if share(&r.text, script(o)) >= 0.3 => kept.push((o, r)),
                    r => {
                        rest.push(b);
                        rest_last.push(r.or(before));
                    }
                }
            }
            (boxes, last) = (rest, rest_last);
        }
        let other = kept.len() - first;
        // Latin lines: the Latin reader's reading from before (its own
        // box), else read now; no letters: the other script's reading
        if let Some(l) = latin_reader {
            let mut need = Vec::new();
            for (b, before) in boxes.into_iter().zip(last) {
                let over = |f: &TextBlock| holds(&f.bounds, &b) || holds(&b, &f.bounds);
                match fallback.iter().position(over) {
                    Some(k) => {
                        let f = fallback.swap_remove(k);
                        kept.push((l, TextBlock { bounds: b, ..f }));
                    }
                    None => need.push((b, before)),
                }
            }
            let boxes: Vec<[f32; 4]> = need.iter().map(|(b, _)| *b).collect();
            let read = self.read_each(l, &f.img, &boxes)?;
            let letters = |t: &TextBlock| t.text.chars().any(char::is_alphabetic);
            for ((_, before), r) in need.into_iter().zip(read) {
                match (r.filter(letters), before) {
                    (Some(r), _) => kept.push((l, r)),
                    (None, Some(b)) => kept.push((others[0], b)),
                    (None, None) => {}
                }
            }
            // a line no reader found again keeps its Latin reading
            for f in fallback {
                if !kept.iter().any(|(_, k)| holds(&k.bounds, &f.bounds)) {
                    kept.push((l, f));
                }
            }
        }
        let mut count = vec![0usize; models::RECOGNIZERS.len()];
        kept.iter().for_each(|(i, _)| count[*i] += 1);
        let most = (0..count.len()).filter(|&i| count[i] > 0);
        let kept_by = |latin: bool| {
            (0..count.len()).any(|i| count[i] > 0 && (script(i) == Script::Latin) == latin)
        };
        f.two |= kept_by(true) && kept_by(false);
        if let Some(best) = most.max_by_key(|&i| count[i]) {
            // (one script on this screen so far: the other reader's memory
            // goes back, #169: two loaded held 3440's peak 7-45 % over main's)
            if !f.two {
                self.recs.retain(|(i, _)| count[*i] > 0);
            }
            self.auto = best;
            match script(best) {
                Script::Latin => self.latin = best,
                _ => self.other = Some(best),
            }
        }
        crate::log!(
            "[opencircle] ocr per line {w}x{h}: {} lines kept ({first} Latin first, {other} another script, {} Latin after), {} strips at full size, {} known, {} ms",
            kept.len(),
            kept.len() - first - other,
            full.len(),
            known.len(),
            start.elapsed().as_millis()
        );
        let rtl = models::RECOGNIZERS[self.auto].rtl();
        Ok(reading_order(
            kept.into_iter().map(|(_, b)| b).collect(),
            rtl,
        ))
    }

    /// The text lines' boxes in `img` (`width` x `height`; image px), and
    /// the size detection ran at.
    fn detect(&mut self, img: &RgbImage, width: u32, height: u32) -> Result<Detected, OcrError> {
        self.detect_at(img, width, height, detect_size(width, height))
    }

    /// The lines of `img` whose middle is in `rows` (all for `None`), found
    /// as recognizer `i` reads them best, and the detector's input size:
    /// shrunk (`detect`) for a Latin reader; at full size in strips
    /// (`detect_strips`) for the others (#169: 14-16 px Arabic shrunk to 10
    /// px read loose boxes and lost lines; at full size missed lines went
    /// 1.3 % -> 0 %, CER-all 0.301 -> 0.172; Latin pages read as well
    /// shrunk, 3x faster).
    fn lines_for(
        &mut self,
        i: usize,
        img: &RgbImage,
        rows: Option<(f32, f32)>,
    ) -> Result<Detected, OcrError> {
        let (w, h) = img.dimensions();
        let (boxes, size) = match models::RECOGNIZERS[i].script() {
            Script::Latin => self.detect(img, w, h)?,
            _ => {
                let mut f = Frame::new(0, img.clone());
                let grid = self.grid(&mut f, rows)?;
                (self.full(&mut f, &strips(h, grid.owned, rows))?, (w, h))
            }
        };
        let inside = |b: &[f32; 4]| {
            let middle = b[1] + b[3] / 2.0;
            rows.is_none_or(|(top, bottom)| (top..bottom).contains(&middle))
        };
        Ok((boxes.into_iter().filter(inside).collect(), size))
    }

    /// The lines of `img` at full size in `strips` (owned rows: top, bottom),
    /// each seen `STRIP_SEE` past its rows (one full-size pass held 798 MB at
    /// 3440x1440, strips of 256 rows 306 MB); a line is in the strip that
    /// owns its middle.
    fn detect_strips(
        &mut self,
        img: &RgbImage,
        strips: &[(u32, u32)],
        grid: Grid,
    ) -> Result<Vec<[f32; 4]>, OcrError> {
        let (w, h) = img.dimensions();
        let mut out = Vec::new();
        for &(y0, y1) in strips {
            let top = y0.saturating_sub(grid.see);
            let bottom = (y1 + grid.see).min(h);
            let part = image::imageops::crop_imm(img, 0, top, w, bottom - top).to_image();
            let (pw, ph) = part.dimensions();
            let side = |v: u32| ((v as f32 * grid.scale).ceil() as u32).div_ceil(32) * 32;
            let full = (side(pw), side(ph));
            let (boxes, _) = self.detect_at(&part, pw, ph, full)?;
            let owned = y0 as f32..y1 as f32;
            out.extend(
                (boxes.into_iter())
                    .map(|b| [b[0], b[1] + top as f32, b[2], b[3]])
                    .filter(|b| owned.contains(&(b[1] + b[3] / 2.0))),
            );
        }
        Ok(out)
    }

    /// `detect` with the detector's input at `(dw, dh)`.
    fn detect_at(
        &mut self,
        img: &RgbImage,
        width: u32,
        height: u32,
        (dw, dh): (u32, u32),
    ) -> Result<Detected, OcrError> {
        let small = if (dw, dh) == (width, height) {
            img.clone()
        } else {
            image::imageops::resize(img, dw, dh, Triangle)
        };
        let det = self.det.predict(vec![small]).map_err(engine_err)?;
        let (w, h) = (width as f32, height as f32);
        let (sx, sy) = (w / dw as f32, h / dh as f32);
        let boxes: Vec<[f32; 4]> = det
            .detections
            .first()
            .into_iter()
            .flatten()
            .map(|d| {
                let (x0, y0, x1, y1) = d.bbox.aabb();
                let (x0, y0, x1, y1) = (x0 * sx, y0 * sy, x1 * sx, y1 * sy);
                let (x0, y0) = (x0.clamp(0.0, w), y0.clamp(0.0, h));
                [x0, y0, x1.min(w) - x0, y1.min(h) - y0]
            })
            .filter(|b| b[2] >= 2.0 && b[3] >= 2.0)
            .collect();
        Ok((boxes, (dw, dh)))
    }

    /// Main mode's second reader: the lines Windows OCR didn't read (#62),
    /// in their own scripts (`per_line`): one whose middle is in a `known`
    /// box (Windows OCR's lines; image px) is skipped; a line no non-Latin
    /// reader reads in its script is left to Windows OCR.
    pub fn recognize_except(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        known: &[[f32; 4]],
    ) -> Result<Vec<TextBlock>, OcrError> {
        let img = to_rgb(rgba, width, height)?;
        self.per_line(&mut Frame::new(0, img), None, known, false)
    }

    /// Main mode's screen text, a part at a time (#169): the lines whose
    /// middle is in `rows` (image px) and not in a `known` box, read line
    /// by line (`per_line`; `latin`: the Latin reader may read, else Windows
    /// OCR did), and every line found on the screen not in a known box
    /// (`found`: the lines still to read show the working cursor). The
    /// screen `key` comes with its picture once (`img`); the lines found on
    /// it are kept for its later parts, so it is detected once per opening
    /// (#169 R7: Linux main mode detected every band again). Without a
    /// picture for a key not held: `NO_FRAME`.
    pub fn read_screen(
        &mut self,
        key: u64,
        img: Option<RgbImage>,
        rows: (f32, f32),
        known: &[[f32; 4]],
        latin: bool,
    ) -> Result<ScreenRead, OcrError> {
        let mut f = match (img, self.frame.take()) {
            (Some(img), _) => Frame::new(key, img),
            (None, Some(f)) if f.key == key => f,
            _ => return Err(OcrError::Engine(NO_FRAME.into())),
        };
        let read = self.per_line(&mut f, Some(rows), known, latin);
        // (only once found anyway: a Latin page read in bands never needs
        // the whole screen found, ~150 ms on 2 cores at 1080p; its bands'
        // lines found so far instead)
        let unknown = |b: &&[f32; 4]| !known.iter().any(|k| holds(k, b));
        let found = (f.shrunk.as_ref().unwrap_or(&f.seen).iter())
            .filter(unknown)
            .copied()
            .collect();
        self.frame = Some(f);
        Ok(ScreenRead {
            lines: read?,
            found,
            done: self.frame.as_ref().is_some_and(|f| f.done),
            noise: self.frame.as_ref().map_or(0, |f| f.noise),
        })
    }

    /// Forgets the screen `read_screen` keeps (its picture and lines).
    pub fn forget_screen(&mut self) {
        self.frame = None;
    }

    /// The lines of `img` in `rows` as a Latin reader reads them best: the
    /// band (with `BAND_SEE` rows around) found shrunk on its own. Found on
    /// the whole screen shrunk instead, an English page in bands read CER
    /// 0.210 against 0.001 (`ocr::bench`, h-en-1080): looser boxes.
    fn band(
        &mut self,
        img: &RgbImage,
        (top, bottom): (f32, f32),
    ) -> Result<Vec<[f32; 4]>, OcrError> {
        let (w, h) = img.dimensions();
        let y0 = (top - BAND_SEE).max(0.0) as u32;
        let y1 = ((bottom + BAND_SEE) as u32).min(h);
        if y1 <= y0 {
            return Ok(Vec::new());
        }
        let part = image::imageops::crop_imm(img, 0, y0, w, y1 - y0).to_image();
        let (boxes, _) = self.detect(&part, w, y1 - y0)?;
        Ok((boxes.into_iter())
            .map(|b| [b[0], b[1] + y0 as f32, b[2], b[3]])
            .filter(|b| (top..bottom).contains(&(b[1] + b[3] / 2.0)))
            .collect())
    }

    /// How `f`'s non-Latin lines are found (`Grid`), once per frame: from
    /// its lines found shrunk.
    fn grid(&mut self, f: &mut Frame, rows: Option<(f32, f32)>) -> Result<Grid, OcrError> {
        if let Some(grid) = f.grid {
            return Ok(grid);
        }
        let (w, h) = f.img.dimensions();
        // the lines' height from what was found already, else from the band
        // asked for (Linux bands: the whole screen found shrunk held a 3440
        // helper at 387 MB, main's bands 266), else the whole screen
        if f.shrunk.is_none()
            && f.seen.len() < 3
            && let Some(rows) = rows
        {
            let boxes = self.band(&f.img, rows)?;
            f.seen.extend_from_slice(&boxes);
        }
        let line = match (&f.shrunk, f.seen.len() >= 3) {
            (Some(boxes), _) => median_height(boxes),
            (None, true) => median_height(&f.seen),
            (None, false) => median_height(&self.shrunk(f)?),
        };
        let shrunk = detect_size(w, h).0 as f32 / w as f32;
        let bands = f.shrunk.is_none();
        let grid = Grid::new(w, if line > 0.0 { line } else { LINE_PX }, shrunk, bands);
        f.grid = Some(grid);
        Ok(grid)
    }

    /// The lines of `f` found shrunk (`detect`), once per frame.
    fn shrunk(&mut self, f: &mut Frame) -> Result<Vec<[f32; 4]>, OcrError> {
        if let Some(boxes) = &f.shrunk {
            return Ok(boxes.clone());
        }
        let (w, h) = f.img.dimensions();
        let (boxes, _) = self.detect(&f.img, w, h)?;
        f.shrunk = Some(boxes.clone());
        Ok(boxes)
    }

    /// The lines of `f` at full size in `strips` (`detect_strips`), each
    /// strip found once per frame.
    fn full(&mut self, f: &mut Frame, strips: &[(u32, u32)]) -> Result<Vec<[f32; 4]>, OcrError> {
        let grid = self.grid(f, None)?;
        let mut out = Vec::new();
        if grid.shrunk && f.shrunk.is_some() {
            // large text: the shrunk lines are found well (no strip again);
            // not found yet (Linux bands): strips at that scale instead (the
            // whole screen found shrunk held a 150 % helper at 309 MB, main's
            // bands 202)
            let owns = |b: &&[f32; 4]| {
                let middle = b[1] + b[3] / 2.0;
                strips
                    .iter()
                    .any(|&(a, z)| (a as f32..z as f32).contains(&middle))
            };
            out.extend(self.shrunk(f)?.iter().filter(owns));
            return Ok(out);
        }
        for s in strips {
            let at = f.strips.iter().position(|(t, _)| t == s);
            let at = match at {
                Some(at) => at,
                None => {
                    let boxes = self.detect_strips(&f.img, std::slice::from_ref(s), grid)?;
                    f.strips.push((*s, boxes));
                    f.strips.len() - 1
                }
            };
            out.extend_from_slice(&f.strips[at].1);
        }
        Ok(out)
    }

    /// Recognizer `i` on the `boxes` of `img`: the lines it read.
    fn read_lines(
        &mut self,
        i: usize,
        img: &RgbImage,
        boxes: &[[f32; 4]],
    ) -> Result<Vec<TextBlock>, OcrError> {
        Ok(self
            .read_each(i, img, boxes)?
            .into_iter()
            .flatten()
            .collect())
    }

    /// Recognizer `i` on each of the `boxes` of `img` (`None`: nothing read).
    fn read_each(
        &mut self,
        i: usize,
        img: &RgbImage,
        boxes: &[[f32; 4]],
    ) -> Result<Vec<Option<TextBlock>>, OcrError> {
        let rtl = models::RECOGNIZERS[i].rtl();
        // long lines are read in pieces (`cut`), each piece's line noted
        let (mut pieces, mut owner) = (Vec::new(), Vec::new());
        for (k, b) in boxes.iter().enumerate() {
            let parts = cut(img, b, longest(i));
            owner.extend(std::iter::repeat_n(k, parts.len()));
            pieces.extend(parts);
        }
        let rec = self.recognizer(i)?;
        let mut read: Vec<Vec<TextBlock>> = vec![Vec::new(); boxes.len()];
        for run in runs(&pieces) {
            let crops = (run.iter())
                .map(|&j| {
                    let [x, y, w, h] = pieces[j].map(|v| v as u32);
                    image::imageops::crop_imm(img, x, y, w, h).to_image()
                })
                .collect();
            let rec = rec.predict(crops).map_err(engine_err)?;
            for (&j, (text, confidence)) in run.iter().zip(rec.texts.iter().zip(rec.scores)) {
                if !text.trim().is_empty() {
                    let text = text.trim();
                    read[owner[j]].push(TextBlock {
                        text: if rtl { reverse_rtl(text) } else { text.into() },
                        bounds: pieces[j],
                        confidence,
                    });
                }
            }
        }
        let joined = (read.into_iter().zip(boxes))
            .map(|(parts, b)| joined(parts, *b, rtl))
            .collect();
        Ok(joined)
    }
}

fn check_image(rgba: &[u8], width: u32, height: u32) -> Result<(), OcrError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(rgba.len()) {
        return Err(OcrError::BadImage(format!(
            "{width}x{height} RGBA needs {} bytes, got {}",
            expected.map_or("too many".into(), |n| n.to_string()),
            rgba.len()
        )));
    }
    Ok(())
}

fn to_rgb(rgba: &[u8], width: u32, height: u32) -> Result<RgbImage, OcrError> {
    check_image(rgba, width, height)?;
    let rgb = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| &p[..3])
        .copied()
        .collect();
    RgbImage::from_raw(width, height, rgb)
        .ok_or_else(|| OcrError::BadImage(format!("{width}x{height} is too large")))
}

/// Top to bottom, then left to right within a line (right to left for an
/// RTL script). A box joins the current line when its vertical centre lies
/// inside the line's first box.
// ponytail: one column. Side-by-side columns interleave line by line; add
// when a multi-column layout needs it.
fn reading_order(mut blocks: Vec<TextBlock>, rtl: bool) -> Vec<TextBlock> {
    blocks.sort_by(|a, b| a.bounds[1].total_cmp(&b.bounds[1]));
    let mut lines: Vec<Vec<TextBlock>> = Vec::new();
    for b in blocks {
        let centre = b.bounds[1] + b.bounds[3] / 2.0;
        match lines.last_mut() {
            Some(line) if centre < line[0].bounds[1] + line[0].bounds[3] => line.push(b),
            _ => lines.push(vec![b]),
        }
    }
    lines
        .into_iter()
        .flat_map(|mut line| {
            line.sort_by(|a, b| a.bounds[0].total_cmp(&b.bounds[0]));
            if rtl {
                line.reverse();
            }
            line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_dll_load_points_to_the_vc_redistributable() {
        // What ort's LoadError::Dlopen prints on Windows (libloading's LoadLibraryExW variant).
        let e = load_err("failed to load from `C:\\ocr\\onnxruntime.dll`: LoadLibraryExW failed");
        let text = e.to_string();
        assert!(text.contains("Install the Microsoft Visual C++ Redistributable (x64): https://aka.ms/vs/17/release/vc_redist.x64.exe"), "{text}");
        assert!(
            text.contains("LoadLibraryExW failed"),
            "keeps the detail: {text}"
        );
        // Any other failure keeps its own message.
        let other = load_err("ort 2.0 is not compatible with the ONNX Runtime binary").to_string();
        assert!(!other.contains("Redistributable"), "{other}");
    }

    /// No ONNX Runtime on disk: a typed "missing" error that says how to get
    /// it, never a load attempt (on Windows: a VC++ hint, then a panic).
    #[test]
    fn missing_runtime_is_a_typed_error_not_a_load() {
        let dir = std::env::temp_dir().join(format!("oc-no-ort-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = Engine::load(&dir)
            .err()
            .expect("no engine without its files");
        assert!(matches!(err, OcrError::Missing(_)), "{err}");
        let text = err.to_string();
        assert!(
            text.contains("isn't downloaded yet") && !text.contains("Redistributable"),
            "{text}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn block(text: &str, x: f32, y: f32) -> TextBlock {
        TextBlock {
            text: text.into(),
            bounds: [x, y, 50.0, 20.0],
            confidence: 1.0,
        }
    }

    #[test]
    fn reading_order_is_lines_then_left_to_right() {
        let blocks = vec![
            block("c", 0.0, 40.0),
            block("b", 100.0, 2.0), // same line as "a", a little higher
            block("a", 0.0, 5.0),
            block("d", 100.0, 43.0),
        ];
        let texts = |got: Vec<TextBlock>| got.into_iter().map(|b| b.text).collect::<Vec<_>>();
        assert_eq!(
            texts(reading_order(blocks.clone(), false)),
            ["a", "b", "c", "d"]
        );
        assert_eq!(
            texts(reading_order(blocks, true)),
            ["b", "a", "d", "c"],
            "RTL"
        );
    }

    #[test]
    fn arabic_output_is_reversed_like_paddleocr() {
        assert_eq!(reverse_rtl("ةيسيئرلا ةمئاقلا"), "القائمة الرئيسية");
        // Latin and number runs keep their order
        assert_eq!(reverse_rtl("ىوتسملا 12 FPS"), "12 FPS المستوى");
        assert_eq!(reverse_rtl(""), "");
    }

    #[test]
    fn readers_round_trip_on_the_wire() {
        // the wire byte round-trips
        for r in [
            Read::Auto,
            Read::One(0),
            Read::One(models::RECOGNIZERS.len() - 1),
        ] {
            assert_eq!(Read::from_byte(r.byte()), Some(r));
        }
        assert_eq!(Read::from_byte(200), None);
        assert_eq!(Read::for_source("auto"), Read::Auto);
        assert_eq!(Read::for_source("en"), Read::One(0));
        let ar = Read::for_source("ar-EG");
        assert!(matches!(ar, Read::One(i) if models::RECOGNIZERS[i].id == "arabic"));
    }

    /// #62/#63: Windows OCR reads main mode's screen unless the setting says
    /// PP-OCR, Windows has no recognizer (or no Windows: Linux), or (Auto)
    /// its language is right-to-left; not known yet = try it.
    #[test]
    fn main_mode_text_is_routed_to_the_right_reader() {
        use Source::{Helper, System};
        use TextReader::{Auto, PpOcr, Windows};
        for (setting, system, want) in [
            (Auto, Some(Some("en-US")), System),
            (Auto, Some(Some("de")), System),
            (Auto, Some(Some("ja")), System),
            (Auto, None, System), // first opening: tried, a failure falls back
            (Auto, Some(None), Helper), // no recognizer installed (or Linux)
            (Auto, Some(Some("ar-SA")), Helper),
            (Auto, Some(Some("he")), Helper),
            (Windows, Some(Some("ar-SA")), System),
            (Windows, Some(None), Helper),
            (PpOcr, Some(Some("en-US")), Helper),
            (PpOcr, None, Helper),
        ] {
            assert_eq!(route(setting, system), want, "{setting:?} {system:?}");
        }
    }

    #[test]
    fn recognition_runs_are_limited_by_padded_width() {
        let run = |ratios: &[f32]| {
            let boxes: Vec<_> = ratios.iter().map(|r| [0.0, 0.0, r * 40.0, 40.0]).collect();
            let mut rest = &boxes[..];
            let mut sizes = vec![];
            while !rest.is_empty() {
                let n = rec_batch_len(rest);
                sizes.push(n);
                rest = &rest[n..];
            }
            sizes
        };
        // ordinary lines: 8 per run as before
        assert_eq!(run(&[5.0; 20]), [8, 8, 4]);
        // eight lines of ~1500 columns (ratio 31) still fit
        assert_eq!(run(&[31.0; 8]), [8]);
        // full-width lines (ratio ~94 = 4500 columns): 2 per run, not 8
        assert_eq!(run(&[94.0; 5]), [2, 2, 1]);
        // one huge line still reads
        assert_eq!(run(&[400.0]), [1]);
        // narrow first, wide last: the wide ones close the run early
        assert_eq!(
            run(&[5.0, 5.0, 5.0, 5.0, 94.0, 94.0, 94.0, 94.0]),
            [4, 2, 2]
        );
    }

    /// #169: lines are read in runs of alike shapes whatever order they
    /// were found in (the second reader's were in detection order).
    #[test]
    fn lines_are_batched_by_shape() {
        let ratios = [5.0, 94.0, 5.0, 94.0, 6.0, 94.0, 5.0, 94.0];
        let boxes: Vec<_> = ratios.iter().map(|r| [0.0, 0.0, r * 40.0, 40.0]).collect();
        let got: Vec<Vec<f32>> = (runs(&boxes).iter())
            .map(|run| run.iter().map(|&j| ratios[j]).collect())
            .collect();
        assert_eq!(
            got,
            [vec![5.0, 5.0, 5.0, 6.0], vec![94.0, 94.0], vec![94.0, 94.0]]
        );
    }

    /// #169: a line longer than the reader's limit is read in pieces, each
    /// cut in a word space; the pieces read back as one line in reading order.
    #[test]
    fn long_lines_are_cut_in_word_spaces_and_joined() {
        // a 20 px tall line, 1000 px long (50 heights): "words" of 90 px ink
        // with 10 px spaces, white page
        let mut img = RgbImage::from_pixel(1000, 20, image::Rgb([255, 255, 255]));
        for x in 0..1000 {
            if x % 100 < 90 {
                for y in 4..16 {
                    img.put_pixel(x, y, image::Rgb([0, 0, 0]));
                }
            }
        }
        let line = [0.0, 0.0, 1000.0, 20.0];
        let parts = cut(&img, &line, 15.0);
        assert_eq!(parts.len(), 5, "{parts:?}"); // 50 / (15 * 0.8) -> 5 pieces
        for p in &parts {
            assert!(p[2] / p[3] <= 15.0, "{p:?}");
        }
        // every cut is in a space, the pieces cover the line
        for w in parts.windows(2) {
            let at = w[1][0] as u32;
            assert!(at % 100 >= 90, "cut at {at}, inside a word");
            assert_eq!(w[0][0] + w[0][2], w[1][0]);
        }
        assert_eq!((parts[0][0], parts[4][0] + parts[4][2]), (0.0, 1000.0));
        // short enough: one piece; the English limit cuts only past its cap
        assert_eq!(cut(&img, &[0.0, 0.0, 280.0, 20.0], 15.0).len(), 1);
        assert_eq!(cut(&img, &line, 60.0).len(), 1);

        let piece = |text: &str, x: f32| TextBlock {
            text: text.into(),
            bounds: [x, 0.0, 100.0, 20.0],
            confidence: 0.5 + x / 1000.0,
        };
        let parts = vec![piece("b", 100.0), piece("a", 0.0)];
        let ltr = joined(parts.clone(), line, false).unwrap();
        assert_eq!((ltr.text.as_str(), ltr.bounds), ("a b", line));
        assert_eq!(
            joined(parts, line, true).unwrap().text,
            "b a",
            "right to left"
        );
        assert_eq!(joined(vec![], line, false), None);
    }

    /// #169: a line read in its own script stays with that reader, a mixed
    /// Arabic line too; one in Latin letters goes on to the Latin reader.
    #[test]
    fn lines_are_classified_by_the_letters_read() {
        let arabic = |t: &str| share(t, Script::Arabic) >= 0.3;
        assert!(arabic("القائمة الرئيسية"));
        assert!(arabic(
            "يعمل نظام التشغيل Windows 11 على أجهزة الحاسوب الحديثة"
        ));
        assert!(
            !arabic("Windows 11 Office Teams"),
            "Latin read by the Arabic reader"
        );
        assert!(!arabic("٢٠٢٤ 12,450"), "digits only: no letters");
        assert!(!arabic(""));
        assert_eq!(share("Новая game", Script::Cyrillic), 5.0 / 9.0);
        // the strips to read again at full size: those owning a line left
        let lines = [[0.0, 300.0, 50.0, 20.0], [0.0, 900.0, 50.0, 20.0]];
        assert_eq!(
            owning(strips(1080, 256, None), &lines),
            [(256, 512), (768, 1024)]
        );
        assert!(owning(strips(1080, 256, None), &[]).is_empty());
        // a few lines left of a page read: read as found, no strip again
        assert_eq!(
            few(strips(1080, 256, None), lines.to_vec(), 10),
            (vec![], lines.to_vec())
        );
        assert_eq!(few(strips(1080, 256, None), lines.to_vec(), 9).0.len(), 2);
        assert!(holds(&[0.0, 0.0, 100.0, 20.0], &[40.0, 5.0, 10.0, 10.0]));
        assert!(!holds(&[0.0, 0.0, 100.0, 20.0], &[95.0, 5.0, 20.0, 10.0]));
    }

    /// #169: full-size detection runs in strips of `STRIP` rows; only those
    /// that own rows wanted.
    #[test]
    fn full_size_detection_runs_in_strips() {
        assert_eq!(strips(1080, 256, None).len(), 5);
        assert_eq!(strips(1080, 256, None)[4], (1024, 1080));
        assert_eq!(
            strips(200, 256, None),
            [(0, 200)],
            "a small image: one strip"
        );
        // a band of rows 300..420: the strip that owns them only
        assert_eq!(strips(1080, 256, Some((300.0, 420.0))), [(256, 512)]);
        assert_eq!(
            strips(1080, 256, Some((500.0, 520.0))),
            [(256, 512), (512, 768)]
        );
        // 15 px text at 1080p: full size, 256 rows (+64 seen) as before
        let g = Grid::new(1920, 29.3, 1280.0 / 1920.0, false);
        assert_eq!((g.scale, g.owned, g.see, g.shrunk), (1.0, 256, 64, false));
        // 3440 wide: no more pixels per strip than 1920 x 384
        let g = Grid::new(3440, 29.3, 0.5, false);
        assert!(
            (g.owned + 2 * g.see) as f32 * 3440.0 <= 1920.0 * 384.0,
            "{g:?}"
        );
        // 150 %: large text found shrunk
        assert!(Grid::new(2880, 46.6, 0.5, false).scale < 0.7);
        assert_eq!(
            Grid::new(2880, 46.6, 0.5, true).scale,
            0.5,
            "bands: the shrunk scale"
        );
        assert!(Grid::new(1920, 60.0, 0.667, false).shrunk);
    }

    #[test]
    fn detector_shrinks_big_screens_at_most_2x() {
        assert_eq!(detect_size(1920, 1080), (1280, 736));
        // 18 px body text stays >= 12 px (at 960: 10 px, lines went missing)
        assert_eq!(detect_size(1716, 1145), (1280, 864));
        assert_eq!(detect_size(3440, 1440), (1728, 736)); // not 1280x544
        assert_eq!(detect_size(3840, 2160), (1920, 1088));
        assert_eq!(detect_size(1280, 720), (1280, 736)); // not shrunk (rounded to 32)
        assert_eq!(detect_size(640, 160), (640, 160)); // a selection: never grown
        assert_eq!(detect_size(10, 5), (32, 32));
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(matches!(to_rgb(&[], 0, 0), Err(OcrError::BadImage(_))));
        assert!(matches!(to_rgb(&[0; 15], 2, 2), Err(OcrError::BadImage(_))));
        assert!(matches!(
            to_rgb(&[0; 16], u32::MAX, u32::MAX),
            Err(OcrError::BadImage(_))
        ));
        assert_eq!(to_rgb(&[1, 2, 3, 255], 1, 1).unwrap().into_raw(), [1, 2, 3]);
        // Rejected before any process is spawned.
        assert!(matches!(
            recognize_in_child(&[0; 15], 2, 2),
            Err(OcrError::BadImage(_))
        ));
    }

    /// The translate tests' 1920x1080 game frame (quest text 22 px) as the
    /// owner's 3440x1440 monitor at 100 % scale shows it: same pixels, top
    /// right, black around. Every line must come back whole and as one block,
    /// on the 1080p frame, the wide frame and a selection crop of the quest
    /// panel. In process, models from `models_dir()`:
    /// `cargo test --release ocr::tests::reads_whole -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_whole_lines_on_a_wide_screen() {
        let mut engine = Engine::load(&ensure_models(|_, _| {}).unwrap()).unwrap();
        let (frame, lines) = crate::translate::tests::game_frame();
        let frame = image::RgbaImage::from_raw(1920, 1080, frame).unwrap();
        let mut wide = image::RgbaImage::from_pixel(3440, 1440, image::Rgba([0, 0, 0, 255]));
        image::imageops::overlay(&mut wide, &frame, 3440 - 1920, 0);
        let quest = image::imageops::crop_imm(&wide, 2980, 120, 460, 210).to_image();
        let all: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        // ponytail: spaces are ignored; the recognizer drops some between
        // words ("northerngate") at any detector size, a separate issue.
        let squash = |s: &str| s.split_whitespace().collect::<String>();
        let mut failed = Vec::new();
        for (name, img, want) in [
            ("1920x1080", &frame, &all[..]),
            ("3440x1440", &wide, &all[..]),
            ("quest crop", &quest, &all[3..7]),
        ] {
            let (w, h) = img.dimensions();
            let (mut got, mut took) = (Vec::new(), std::time::Duration::MAX);
            for _ in 0..4 {
                let t = std::time::Instant::now();
                got = engine.recognize(img.as_raw(), w, h, Read::One(0)).unwrap();
                took = took.min(t.elapsed()); // best of 4 (the first warms up)
            }
            let read: Vec<String> = got.iter().map(|b| squash(&b.text)).collect();
            let whole = want.iter().filter(|l| read.contains(&squash(l))).count();
            println!(
                "{name}: {whole}/{} lines whole, {} blocks, {took:?}",
                want.len(),
                got.len()
            );
            if whole != want.len() || got.len() != want.len() {
                let texts: Vec<&str> = got.iter().map(|b| b.text.as_str()).collect();
                println!("  read {texts:?}");
                failed.push(name);
            }
        }
        assert!(failed.is_empty(), "lines cut or split: {failed:?}");
    }

    #[cfg(target_os = "linux")]
    fn rss() -> String {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS")).map(String::from))
            .unwrap_or_else(|| "VmRSS: n/a".into())
    }

    /// Peak RSS of the largest finished child process, in MB (Linux).
    #[cfg(target_os = "linux")]
    fn child_peak_mb() -> i64 {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: getrusage only writes into the struct we pass.
        unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut usage) };
        usage.ru_maxrss / 1024
    }

    /// Runs the real `opencircle ocr-worker`, so build it first. Downloads ORT
    /// and the models on first run (~22 MB):
    /// `cargo build --release && cargo test --release ocr::tests::recognizes -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn recognizes_generated_text() {
        ensure_models(|_, _| {}).unwrap();
        // target/<profile>/deps/<test exe> -> target/<profile>/opencircle
        let exe = std::env::current_exe().unwrap();
        let exe = exe.parent().unwrap().parent().unwrap().join("opencircle");
        let recognize = |rgba: &[u8], w: usize, h: usize| {
            let t = std::time::Instant::now();
            let lines =
                worker::recognize_with(&exe, rgba, w as u32, h as u32, Read::One(0)).unwrap();
            (lines, t.elapsed())
        };
        println!("app before: {}", rss());

        let png = png::Decoder::new(std::io::Cursor::new(include_bytes!("fixture.png")));
        let mut reader = png::Decoder::read_info(png).unwrap();
        let mut tile = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut tile).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let (tw, th) = (info.width as usize, info.height as usize);

        let (lines, took) = recognize(&tile, tw, th);
        let texts: Vec<&str> = lines.iter().map(|b| b.text.as_str()).collect();
        println!("fixture in {took:?}: {lines:?}");
        assert_eq!(texts, ["Hello OpenCircle", "Read 42 words here"]);

        // A 1920x1080 screen: the 480x120 tile once (2 lines), then 4 across,
        // 9 down (72 lines).
        let (w, h) = (1920, 1080);
        let mut screen = vec![255; w * h * 4];
        for y in 0..th {
            screen[y * w * 4..][..tw * 4].copy_from_slice(&tile[y * tw * 4..][..tw * 4]);
        }
        let (lines, took) = recognize(&screen, w, h);
        println!("1080p, 2 lines: {} lines in {took:?}", lines.len());
        assert_eq!(lines.len(), 2);
        for y in 0..h {
            let src = (y % th) * tw * 4;
            for x in (0..w).step_by(tw) {
                screen[(y * w + x) * 4..][..tw * 4].copy_from_slice(&tile[src..][..tw * 4]);
            }
        }
        for run in 1..=3 {
            let (lines, took) = recognize(&screen, w, h);
            println!("1080p run {run}: {} lines in {took:?}", lines.len());
            assert_eq!(lines.len(), 72, "{lines:?}");
        }
        println!("app after: {}; child peak {} MB", rss(), child_peak_mb());
    }
}
