//! OCR: PP-OCRv5 mobile (detect lines, then read them) via oar-ocr on ONNX
//! Runtime, CPU. Neither ORT nor the models ship with the app: `ensure_models`
//! fetches them on first use (the only network access here). The app calls
//! `recognize_in_child`, so ORT and its memory live only in a short-lived
//! `opencircle ocr-worker` process that runs `Engine`.
pub(crate) mod models;
pub(crate) mod worker;

pub use models::{ensure_models, models_dir};
pub use worker::{WORKER_ARG, recognize_in_child, translate_in_child, worker_main};

use std::fmt;
use std::path::Path;

use image::RgbImage;
use image::imageops::FilterType::Triangle;
use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::core::traits::OrtConfigurable;
use oar_ocr::domain::tasks::TextDetectionConfig;
use oar_ocr::predictors::{TextDetectionPredictor, TextRecognitionPredictor};
use oar_ocr::processors::LimitType;

/// Longest side the detector sees. 960 px read 35/36 lines at ~100 ms in spike
/// S4; native 1080p read 36/36 but took 3x as long.
const DETECT_SIDE: u32 = 960;

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

/// The ORT DLL is checksum-verified, so on Windows a failed `LoadLibraryExW`
/// means one of its own imports (MSVCP140, VCRUNTIME140_1) is missing. We never
/// bundle or download the redistributable: we say where to get it.
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

/// ORT plus loaded models. Runs in the `ocr-worker` process only.
pub struct Engine {
    det: TextDetectionPredictor,
    rec: TextRecognitionPredictor,
}

impl Engine {
    /// Loads ORT and the models from `models` (the folder `ensure_models` returns).
    pub fn load(models: &Path) -> Result<Engine, OcrError> {
        let rec_files = &models::RECOGNIZER;
        ort::init_from(models.join(models::ORT.name))
            .map_err(load_err)?
            .commit();
        // Memory patterns cache a plan per input shape; every capture has a new
        // shape, so they only add RAM (~100 MB measured).
        let ort = OrtSessionConfig::new().with_memory_pattern(false);
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
        let rec = TextRecognitionPredictor::builder()
            .dict_path(models.join(rec_files.dict.name))
            .with_ort_config(ort)
            .build(models.join(rec_files.model.name))
            .map_err(engine_err)?;
        Ok(Engine { det, rec })
    }

    /// Finds and reads the text lines in an RGBA image, in reading order.
    pub fn recognize(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<TextBlock>, OcrError> {
        let img = to_rgb(rgba, width, height)?;
        let (dw, dh) = detect_size(width, height);
        let small = if (dw, dh) == (width, height) {
            img.clone()
        } else {
            image::imageops::resize(&img, dw, dh, Triangle)
        };
        let det = self.det.predict(vec![small]).map_err(engine_err)?;
        let (w, h) = (width as f32, height as f32);
        let (sx, sy) = (w / dw as f32, h / dh as f32);
        let mut boxes: Vec<[f32; 4]> = det
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
        // A batch is padded to its widest line: batch alike shapes (as
        // PaddleOCR does). `reading_order` restores the order.
        boxes.sort_by(|a, b| (a[2] / a[3]).total_cmp(&(b[2] / b[3])));
        let mut blocks = Vec::with_capacity(boxes.len());
        for chunk in boxes.chunks(REC_BATCH) {
            let crops = chunk
                .iter()
                .map(|b| {
                    let [x, y, w, h] = b.map(|v| v as u32);
                    image::imageops::crop_imm(&img, x, y, w, h).to_image()
                })
                .collect();
            let rec = self.rec.predict(crops).map_err(engine_err)?;
            for (bounds, (text, confidence)) in chunk.iter().zip(rec.texts.iter().zip(rec.scores)) {
                if !text.trim().is_empty() {
                    blocks.push(TextBlock {
                        text: text.trim().to_string(),
                        bounds: *bounds,
                        confidence,
                    });
                }
            }
        }
        Ok(reading_order(blocks))
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

/// Top to bottom, then left to right within a line. A box joins the current
/// line when its vertical centre lies inside the line's first box.
// ponytail: one column, left-to-right. Side-by-side columns interleave line by
// line, and RTL scripts need right-to-left; add when a multi-column or Arabic
// recognizer lands.
fn reading_order(mut blocks: Vec<TextBlock>) -> Vec<TextBlock> {
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

    fn block(text: &str, x: f32, y: f32) -> TextBlock {
        TextBlock {
            text: text.into(),
            bounds: [x, y, 50.0, 20.0],
            confidence: 1.0,
        }
    }

    #[test]
    fn reading_order_is_lines_then_left_to_right() {
        let got = reading_order(vec![
            block("c", 0.0, 40.0),
            block("b", 100.0, 2.0), // same line as "a", a little higher
            block("a", 0.0, 5.0),
            block("d", 100.0, 43.0),
        ]);
        let texts: Vec<&str> = got.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c", "d"]);
    }

    #[test]
    fn detector_shrinks_big_screens_at_most_2x() {
        assert_eq!(detect_size(1920, 1080), (960, 544)); // 960 px, as before
        assert_eq!(detect_size(3440, 1440), (1728, 736)); // not 960x416
        assert_eq!(detect_size(3840, 2160), (1920, 1088));
        assert_eq!(detect_size(1280, 720), (960, 544));
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
        let engine = Engine::load(&ensure_models(|_, _| {}).unwrap()).unwrap();
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
                got = engine.recognize(img.as_raw(), w, h).unwrap();
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
            let lines = worker::recognize_with(&exe, rgba, w as u32, h as u32).unwrap();
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
