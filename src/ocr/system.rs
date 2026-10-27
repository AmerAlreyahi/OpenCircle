//! Windows' own OCR (Windows.Media.Ocr, unpackaged): reads a whole 1080p screen
//! in ~0.1-0.3 s in the app's process, no download, nothing leaves the PC. It
//! reads only the languages installed in Windows: `language` is the one it
//! uses (the first of the user's languages with a recognizer), `None` when
//! there is none (logged once; PP-OCR reads instead, `super::route`). Linux:
//! never there.
use std::sync::OnceLock;

use super::{OcrError, TextBlock};

/// A gap between two words wider than this many line heights splits a line:
/// two columns side by side are two lines (as PP-OCR's detector finds them),
/// so a drag keeps to its column (`ui::word::Picked`).
const COLUMN_GAP: f32 = 2.5;

/// A word Windows read: its text and box (x, y, w, h; frame px).
pub type Word = (String, [f32; 4]);

/// Windows' lines (each its words, in order) as our lines, in reading order:
/// one `TextBlock` per line (per column: `COLUMN_GAP`), its box around its
/// words, its words joined by a space (none between two CJK characters,
/// which Windows returns one per word).
pub fn blocks(lines: Vec<Vec<Word>>) -> Vec<TextBlock> {
    let mut out = Vec::new();
    for words in lines {
        let mut run: Vec<Word> = Vec::new();
        for word in words {
            let gap = run.last().map(|(_, b)| word.1[0] - (b[0] + b[2]));
            let tall = run.iter().map(|(_, b)| b[3]).fold(word.1[3], f32::max);
            if gap.is_some_and(|g| g > COLUMN_GAP * tall) {
                out.push(block(std::mem::take(&mut run)));
            }
            run.push(word);
        }
        if !run.is_empty() {
            out.push(block(run));
        }
    }
    out.retain(|b| !b.text.trim().is_empty());
    super::reading_order(out, false)
}

fn block(words: Vec<Word>) -> TextBlock {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut text = String::new();
    for (word, [x, y, w, h]) in words {
        let joined = |a: Option<char>, b: Option<char>| a.is_some_and(cjk) && b.is_some_and(cjk);
        if !text.is_empty() && !joined(text.chars().last(), word.chars().next()) {
            text.push(' ');
        }
        text.push_str(word.trim());
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h));
    }
    TextBlock {
        text,
        bounds: [x0, y0, x1 - x0, y1 - y0],
        // Windows gives no confidence; what it returns it stands by.
        confidence: 1.0,
    }
}

/// Chinese and Japanese characters (and full-width forms): written without
/// spaces. Hangul is not: Korean has spaces between words.
fn cjk(c: char) -> bool {
    matches!(c as u32, 0x3000..=0x30FF | 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// The engine and its language tag, made once (`None`: no recognizer).
#[cfg(windows)]
type Made = Option<(windows::Media::Ocr::OcrEngine, String)>;
#[cfg(not(windows))]
type Made = Option<((), String)>;

static ENGINE: OnceLock<Made> = OnceLock::new();

/// The language Windows OCR reads (a BCP-47 tag such as "en-US"), or `None`
/// when none of the user's Windows languages has a recognizer installed
/// (#63: logged once). The first call makes the engine (off the UI thread).
pub fn language() -> Option<&'static str> {
    let made = ENGINE.get_or_init(|| {
        let made = imp::make();
        match &made {
            Some((_, tag)) => eprintln!("[opencircle] Windows OCR reads {tag}"),
            None if cfg!(windows) => eprintln!(
                "[opencircle] Windows OCR has no recognizer for your Windows languages: \
                 the text reader (PP-OCR) reads instead"
            ),
            None => {}
        }
        made
    });
    made.as_ref().map(|(_, tag)| tag.as_str())
}

/// `language`, if it is known yet (never blocks): `None` = not asked yet.
pub fn known() -> Option<Option<&'static str>> {
    #[cfg(test)]
    return LANGUAGE_HERE.get().or(Some(None)); // never this PC's Windows OCR
    #[cfg(not(test))]
    ENGINE
        .get()
        .map(|m| m.as_ref().map(|(_, tag)| tag.as_str()))
}

#[cfg(test)]
thread_local! {
    /// Tests: what `known` says (else: no Windows OCR).
    pub static LANGUAGE_HERE: std::cell::Cell<Option<Option<&'static str>>> =
        const { std::cell::Cell::new(None) };
}

/// The longest side Windows OCR takes (`OcrEngine::MaxImageDimension`).
const MAX_SIDE: u32 = 10_000;

/// How many times bigger Windows OCR sees a `width` x `height` frame drawn
/// at `scale`. It misses short words in small text: at 100 % it read
/// Wikipedia's 14 px body text as "needed to trained" ("be" lost); the frame
/// doubled read it whole (VM: 1080p in ~0.22 s instead of ~0.08 s). Text at
/// 150 % and up is big enough as it is.
pub fn zoom(scale: f32, width: u32, height: u32) -> u32 {
    if scale < 1.5 && width.max(height) * 2 <= MAX_SIDE {
        2
    } else {
        1
    }
}

/// An RGBA image as Windows OCR reads it: grey (8 bits: a quarter of the
/// memory of colour; it reads grey anyway), each pixel `k` x `k` times.
fn grey(rgba: &[u8], width: u32, k: u32) -> Vec<u8> {
    let (k, width) = (k as usize, width as usize);
    let mut out = Vec::with_capacity(rgba.len() / 4 * k * k);
    for row in rgba.chunks_exact(width * 4) {
        let start = out.len();
        for &[r, g, b, _] in row.as_chunks::<4>().0 {
            let y = (77 * r as u32 + 150 * g as u32 + 29 * b as u32) >> 8;
            out.extend(std::iter::repeat_n(y as u8, k));
        }
        for _ in 1..k {
            out.extend_from_within(start..);
        }
    }
    out
}

/// Reads the text lines in an RGBA image (frame px; text drawn at `scale`,
/// the display's), in reading order. Blocking: call off the UI thread.
pub fn recognize(
    rgba: &[u8],
    width: u32,
    height: u32,
    scale: f32,
) -> Result<Vec<TextBlock>, OcrError> {
    super::check_image(rgba, width, height)?;
    language().ok_or_else(|| {
        OcrError::Engine("Windows OCR has no recognizer for your languages".into())
    })?;
    let engine = ENGINE.get().and_then(Option::as_ref).map(|(e, _)| e);
    let start = std::time::Instant::now();
    let k = zoom(scale, width, height);
    let image = grey(rgba, width, k);
    let (w, h) = (width * k, height * k);
    let mut lines = imp::read(engine.expect("made by `language`"), &image, w, h)?;
    let n = lines.len();
    for (_, b) in lines.iter_mut().flatten() {
        *b = b.map(|v| v / k as f32);
    }
    let out = blocks(lines);
    eprintln!(
        "[opencircle] windows ocr {width}x{height} (x{k}): {n} lines in {} ms",
        start.elapsed().as_millis()
    );
    Ok(out)
}

#[cfg(windows)]
mod imp {
    use super::{OcrError, Word};
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    /// WinRT on this thread (a background one: the multithreaded apartment).
    fn com() {
        // SAFETY: no pointers; a thread already in an apartment just says so.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    pub(super) fn make() -> Option<(OcrEngine, String)> {
        com();
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok()?;
        let tag = engine.RecognizerLanguage().and_then(|l| l.LanguageTag());
        let tag = tag.map(|t| t.to_string()).unwrap_or_default();
        Some((engine, tag))
    }

    fn err(e: windows::core::Error) -> OcrError {
        OcrError::Engine(format!("Windows OCR: {e}"))
    }

    pub(super) fn read(
        engine: &OcrEngine,
        grey: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<Vec<Word>>, OcrError> {
        com();
        let max = OcrEngine::MaxImageDimension().map_err(err)?;
        if width.max(height) > max {
            return Err(OcrError::BadImage(format!("over {max} px")));
        }
        let writer = DataWriter::new().map_err(err)?;
        writer.WriteBytes(grey).map_err(err)?;
        let buffer = writer.DetachBuffer().map_err(err)?;
        let (w, h) = (width as i32, height as i32);
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Gray8, w, h)
            .map_err(err)?;
        let result = engine.RecognizeAsync(&bitmap).map_err(err)?.join();
        let mut lines = Vec::new();
        for line in result.and_then(|r| r.Lines()).map_err(err)? {
            let mut words = Vec::new();
            for word in line.Words().map_err(err)? {
                let r = word.BoundingRect().map_err(err)?;
                let text = word.Text().map_err(err)?.to_string();
                words.push((text, [r.X, r.Y, r.Width, r.Height]));
            }
            lines.push(words);
        }
        Ok(lines)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{OcrError, Word};

    pub(super) fn make() -> Option<((), String)> {
        None
    }

    pub(super) fn read(_: &(), _: &[u8], _: u32, _: u32) -> Result<Vec<Vec<Word>>, OcrError> {
        unreachable!("no engine is made on this OS")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, x: f32, y: f32, width: f32) -> Word {
        (text.into(), [x, y, width, 20.0])
    }

    /// Windows' words become our lines: one box around a line's words, the
    /// words joined by a space, a column apart split, CJK without spaces,
    /// in reading order (a line Windows lists later but higher comes first).
    #[test]
    fn windows_words_map_to_our_lines() {
        let got = blocks(vec![
            vec![w("Second", 10.0, 50.0, 60.0), w("line", 76.0, 51.0, 40.0)],
            vec![
                w("The", 10.0, 10.0, 30.0),
                w("first", 46.0, 12.0, 40.0),
                // 300 px on: the next column (more than 2.5 line heights)
                w("Side", 386.0, 10.0, 40.0),
                w("note", 432.0, 10.0, 40.0),
            ],
            vec![w("日", 10.0, 90.0, 20.0), w("本語", 30.0, 90.0, 40.0)],
            vec![w("  ", 10.0, 130.0, 10.0)],
            vec![],
        ]);
        let texts: Vec<&str> = got.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["The first", "Side note", "Second line", "日本語"]);
        assert_eq!(got[0].bounds, [10.0, 10.0, 76.0, 22.0], "around its words");
        assert_eq!(got[1].bounds, [386.0, 10.0, 86.0, 20.0]);
        assert_eq!(got[2].bounds, [10.0, 50.0, 106.0, 21.0]);
        assert!(got.iter().all(|b| b.confidence == 1.0));
        // Korean keeps its spaces
        let ko = blocks(vec![vec![
            w("안녕", 0.0, 0.0, 30.0),
            w("하세요", 36.0, 0.0, 40.0),
        ]]);
        assert_eq!(ko[0].text, "안녕 하세요");
    }

    /// Small text (under 150 %) is read doubled, unless that is too big for
    /// Windows; each pixel becomes k x k grey ones, rows in order.
    #[test]
    fn small_text_is_read_doubled_in_grey() {
        assert_eq!(zoom(1.0, 1920, 1080), 2);
        assert_eq!(zoom(1.25, 3840, 2160), 2);
        assert_eq!(zoom(1.5, 2880, 1620), 1);
        assert_eq!(zoom(1.75, 1920, 1080), 1);
        assert_eq!(zoom(1.0, 5120, 1440), 1, "10240 px is over Windows' limit");
        // 2 x 1: white, then pure red, green and blue weigh in as luma
        let rgba = [255, 255, 255, 255, 255, 0, 0, 255];
        assert_eq!(grey(&rgba, 2, 1), [255, 76]);
        assert_eq!(grey(&rgba, 2, 2), [255, 255, 76, 76, 255, 255, 76, 76]);
        let column = [0, 150, 0, 255, 0, 0, 255, 255]; // 1 x 2
        assert_eq!(grey(&column, 1, 2), [87, 87, 87, 87, 28, 28, 28, 28]);
    }

    /// Off Windows (and in tests) there is no Windows OCR: PP-OCR reads.
    #[test]
    #[cfg(not(windows))]
    fn there_is_no_windows_ocr_elsewhere() {
        assert_eq!(language(), None);
        assert!(matches!(
            recognize(&[0; 16], 2, 2, 1.0),
            Err(OcrError::Engine(_))
        ));
    }

    /// The real Windows OCR on the test fixture (two lines of text), if this
    /// Windows has a recognizer for its languages (windows-latest: en-US).
    #[test]
    #[cfg(windows)]
    fn windows_ocr_reads_the_fixture() {
        let Some(tag) = language() else {
            println!("SKIPPED: this Windows has no OCR recognizer for its languages");
            return;
        };
        let png = png::Decoder::new(std::io::Cursor::new(include_bytes!("fixture.png")));
        let mut reader = png.read_info().unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut rgba).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let got = recognize(&rgba, info.width, info.height, 1.0).unwrap();
        let texts: Vec<&str> = got.iter().map(|b| b.text.as_str()).collect();
        println!("Windows OCR ({tag}): {got:?}");
        if !tag.starts_with("en") {
            return; // another language's recognizer: the words may differ
        }
        assert_eq!(texts, ["Hello OpenCircle", "Read 42 words here"]);
        for b in &got {
            let [x, y, w, h] = b.bounds;
            assert!(x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0, "{b:?}");
            assert!(
                x + w <= info.width as f32 && y + h <= info.height as f32,
                "{b:?}"
            );
        }
    }
}
