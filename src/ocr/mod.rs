//! OCR: PP-OCRv5 mobile (detect lines, then read them) via oar-ocr on ONNX
//! Runtime, CPU. Neither ORT nor the models ship with the app: `ensure_models`
//! fetches them on first use (the only network access here). The app calls
//! `recognize_in_child`, so ORT and its memory live only in an
//! `opencircle ocr-worker` process that runs `Engine` (one per call, or one
//! warm one while a `Hold` lives: `session`).
pub(crate) mod models;
pub mod screen;
pub(crate) mod session;
pub mod system;
pub(crate) mod worker;

pub use models::{download_size, ensure_models, models_dir, ready};
pub use session::{Hold, gently_here, hold, keep_warm};
pub use worker::{
    WORKER_ARG, reads_other_scripts, recognize_as, recognize_band, recognize_except,
    recognize_in_child, translate_in_child, warm, worker_main,
};

use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::TextReader;
use crate::translate::lang::Script;

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

/// Lines read per recognizer run. One run over all 72 lines of a busy 1080p
/// screen grew RAM to 1.2 GB (ORT keeps its peak); 8 kept it ~250 MB at the
/// same speed.
const REC_BATCH: usize = 8;

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
    /// Reading or writing the model files.
    Io(std::io::Error),
    /// What isn't on disk yet (the user hasn't agreed to its download):
    /// checked before a helper starts or loads ONNX Runtime.
    Missing(String),
    /// ONNX Runtime failed to load a model or run it.
    Engine(String),
}

impl fmt::Display for OcrError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::BadImage(m) => write!(f, "bad image: {m}"),
            Self::Download(m) => write!(f, "could not download the text models: {m}"),
            Self::Checksum(name) => write!(f, "downloaded model {name} is corrupt (checksum)"),
            Self::Io(e) => write!(f, "text model files: {e}"),
            Self::Missing(what) => write!(
                f,
                "{what} isn't downloaded yet. Translate once with Ctrl+Shift+T to download it"
            ),
            Self::Engine(m) => write!(f, "text recognition failed: {m}"),
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

/// Text lines' boxes (x, y, w, h; image px) and the size detection ran at.
type Detected = (Vec<[f32; 4]>, (u32, u32));

/// The second reader keeps a line read at least this well: lower than
/// `AUTO_QUALITY` (Arabic with harakat reads at ~0.77), safe since a line
/// in another script scores ~0 (its letters aren't the recognizer's).
const OTHER_QUALITY: f32 = 0.6;

/// Below this `quality`, `Auto` tries the other recognizers.
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
    /// The loaded recognizer (one at a time: another script replaces it).
    rec: Option<(usize, TextRecognitionPredictor)>,
    /// `Auto`'s first try: the last recognizer that read well.
    auto: usize,
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
            rec: None,
            auto: 0,
        })
    }

    /// Recognizer `i`, loaded (the previous one is freed first).
    fn recognizer(&mut self, i: usize) -> Result<&TextRecognitionPredictor, OcrError> {
        if self.rec.as_ref().is_none_or(|(n, _)| *n != i) {
            self.rec = None;
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
            self.rec = Some((i, rec));
        }
        Ok(&self.rec.as_ref().expect("just loaded").1)
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
        let (boxes, (dw, dh)) = self.detect(&img, width, height)?;
        let mut boxes: Vec<[f32; 4]> = boxes
            .into_iter()
            .filter(|b| {
                let middle = b[1] + b[3] / 2.0;
                rows.is_none_or(|(top, bottom)| (top as f32..bottom as f32).contains(&middle))
            })
            .collect();
        // A batch is padded to its widest line: batch alike shapes (as
        // PaddleOCR does). `reading_order` restores the order.
        boxes.sort_by(|a, b| (a[2] / a[3]).total_cmp(&(b[2] / b[3])));
        let first = match read {
            Read::One(i) => i,
            Read::Auto => self.auto,
        };
        let n = boxes.len();
        let q =
            |i: usize, blocks: &[TextBlock]| quality(blocks, n, models::RECOGNIZERS[i].script());
        let detected = start.elapsed();
        let mut best = (first, self.read_lines(first, &img, &boxes)?);
        if read == Read::Auto && q(first, &best.1) < AUTO_QUALITY {
            for i in 0..models::RECOGNIZERS.len() {
                if i == first || !models::installed(models::RECOGNIZERS[i]) {
                    continue;
                }
                let other = self.read_lines(i, &img, &boxes)?;
                if q(i, &other) > q(best.0, &best.1) {
                    best = (i, other);
                }
            }
            self.auto = best.0;
        }
        crate::log!(
            "[opencircle] ocr {width}x{height}: detect at {dw}x{dh} {} ms, {n} lines read in {} ms",
            detected.as_millis(),
            (start.elapsed() - detected).as_millis()
        );
        Ok(reading_order(best.1, models::RECOGNIZERS[best.0].rtl()))
    }

    /// The text lines' boxes in `img` (`width` x `height`; image px), and
    /// the size detection ran at.
    fn detect(&mut self, img: &RgbImage, width: u32, height: u32) -> Result<Detected, OcrError> {
        let (dw, dh) = detect_size(width, height);
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
    /// each in the script that reads it best. Detection finds the lines; one
    /// whose middle is in a `known` box (Windows OCR's lines; image px) is
    /// skipped; the rest are read by every installed non-Latin recognizer
    /// (Windows OCR reads Latin) and kept where one reads it well
    /// (`quality` >= `OTHER_QUALITY` for that line alone).
    // ponytail: each leftover line read by each installed script; a script
    // classifier if many scripts are installed.
    pub fn recognize_except(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        known: &[[f32; 4]],
    ) -> Result<Vec<TextBlock>, OcrError> {
        let start = std::time::Instant::now();
        let img = to_rgb(rgba, width, height)?;
        let (boxes, _) = self.detect(&img, width, height)?;
        let inside = |b: &[f32; 4], k: &[f32; 4]| {
            let (x, y) = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
            (k[0]..k[0] + k[2]).contains(&x) && (k[1]..k[1] + k[3]).contains(&y)
        };
        let left: Vec<[f32; 4]> = (boxes.into_iter())
            .filter(|b| !known.iter().any(|k| inside(b, k)))
            .collect();
        let mut best: Vec<(f32, TextBlock)> = Vec::new();
        for i in 0..models::RECOGNIZERS.len() {
            let rec = models::RECOGNIZERS[i];
            if left.is_empty() || rec.script() == Script::Latin || !models::installed(rec) {
                continue;
            }
            for block in self.read_lines(i, &img, &left)? {
                let q = quality(std::slice::from_ref(&block), 1, rec.script());
                if q < OTHER_QUALITY {
                    continue;
                }
                match best.iter_mut().find(|(_, b)| b.bounds == block.bounds) {
                    Some(old) if old.0 >= q => {}
                    Some(old) => *old = (q, block),
                    None => best.push((q, block)),
                }
            }
        }
        crate::log!(
            "[opencircle] ocr second reader: {} of {} lines left, {} read, {} ms",
            left.len(),
            known.len() + left.len(),
            best.len(),
            start.elapsed().as_millis()
        );
        Ok(reading_order(
            best.into_iter().map(|(_, b)| b).collect(),
            false,
        ))
    }

    /// Recognizer `i` on the `boxes` of `img`.
    fn read_lines(
        &mut self,
        i: usize,
        img: &RgbImage,
        boxes: &[[f32; 4]],
    ) -> Result<Vec<TextBlock>, OcrError> {
        let rtl = models::RECOGNIZERS[i].rtl();
        let rec = self.recognizer(i)?;
        let mut blocks = Vec::with_capacity(boxes.len());
        for chunk in boxes.chunks(REC_BATCH) {
            let crops = chunk
                .iter()
                .map(|b| {
                    let [x, y, w, h] = b.map(|v| v as u32);
                    image::imageops::crop_imm(img, x, y, w, h).to_image()
                })
                .collect();
            let rec = rec.predict(crops).map_err(engine_err)?;
            for (bounds, (text, confidence)) in chunk.iter().zip(rec.texts.iter().zip(rec.scores)) {
                if !text.trim().is_empty() {
                    let text = text.trim();
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
    fn auto_quality_counts_unread_lines() {
        let b = |t: &str, c: f32| TextBlock {
            text: t.into(),
            bounds: [0.0; 4],
            confidence: c,
        };
        let latin = Script::Latin;
        assert_eq!(quality(&[], 0, latin), 1.0, "no text: nothing to improve");
        assert_eq!(quality(&[], 3, latin), 0.0, "nothing read");
        assert!(quality(&[b("Hacpoκ", 0.88)], 2, latin) < AUTO_QUALITY);
        assert!(quality(&[b("Quit", 0.99), b("New game", 0.97)], 2, latin) > AUTO_QUALITY);
        assert!(
            quality(&[b("xq", 0.5), b("zz", 0.6)], 2, latin) < AUTO_QUALITY,
            "unsure"
        );
        // the Arabic model reading Latin text: sure, but not Arabic
        assert!(quality(&[b("Nouvelle partie", 0.98)], 1, Script::Arabic) < AUTO_QUALITY);
        assert!(quality(&[b("القائمة 2", 0.97)], 1, Script::Arabic) > AUTO_QUALITY);
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
