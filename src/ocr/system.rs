//! Windows' own OCR (Windows.Media.Ocr, unpackaged): reads a whole 1080p screen
//! in ~0.1-0.3 s in the app's process, no download, nothing leaves the PC. It
//! reads only the languages installed in Windows: `language` is the one it
//! uses (the first of the user's languages with a recognizer), `None` when
//! there is none (logged once; PP-OCR reads instead, `super::route`). Linux:
//! never there.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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

#[cfg(windows)]
type Engine = windows::Media::Ocr::OcrEngine;
#[cfg(not(windows))]
type Engine = ();

/// The language Windows OCR reads, asked once (`None`: no recognizer).
static LANGUAGE: OnceLock<Option<String>> = OnceLock::new();
/// The engine while it is used, and when it last was (`IDLE`).
static ENGINE: Mutex<Option<(Engine, Instant)>> = Mutex::new(None);
/// The engine is freed after this long unused (the warm helper's rule), so
/// its memory doesn't stay with an idle app. It is made again in a few ms.
const IDLE: Duration = match cfg!(test) {
    true => Duration::from_millis(300), // (tests)
    false => Duration::from_secs(60),
};
/// A thread waits to free it.
static FREEING: AtomicBool = AtomicBool::new(false);

/// The language Windows OCR reads (a BCP-47 tag such as "en-US"), or `None`
/// when none of the user's Windows languages has a recognizer installed
/// (#63: logged once). The first call makes the engine (off the UI thread).
pub fn language() -> Option<&'static str> {
    let tag = LANGUAGE.get_or_init(|| {
        let made = imp::make();
        match &made {
            Some((_, tag)) => crate::log!("[opencircle] Windows OCR reads {tag}"),
            None if cfg!(windows) => crate::log!(
                "[opencircle] Windows OCR has no recognizer for your Windows languages: \
                 the text reader (PP-OCR) reads instead"
            ),
            None => {}
        }
        let tag = made.as_ref().map(|(_, tag)| tag.clone());
        if let Some((engine, _)) = made {
            *lock() = Some((engine, Instant::now()));
            free_when_idle();
        }
        tag
    });
    tag.as_deref()
}

fn lock() -> std::sync::MutexGuard<'static, Option<(Engine, Instant)>> {
    ENGINE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Frees the engine once it was unused for `IDLE` (one thread at a time).
fn free_when_idle() {
    if FREEING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        loop {
            let wait = {
                let mut engine = lock();
                match &*engine {
                    Some((_, used)) if used.elapsed() < IDLE => IDLE - used.elapsed(),
                    _ => {
                        if engine.take().is_some() {
                            crate::log!("[opencircle] Windows OCR freed (unused for 60 s)");
                        }
                        FREEING.store(false, Ordering::SeqCst);
                        return;
                    }
                }
            };
            std::thread::sleep(wait);
        }
    });
}

/// The languages in `wanted` (BCP-47 tags) that no tag in `installed` covers.
/// Compared by base language ("en-GB" is read by an "en-US" recognizer), each
/// reported once, lower case (#63).
pub fn missing(installed: &[String], wanted: &[String]) -> Vec<String> {
    let base = |t: &str| {
        t.split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let have: Vec<String> = installed.iter().map(|t| base(t)).collect();
    let mut out: Vec<String> = Vec::new();
    for w in wanted.iter().map(|t| base(t)) {
        if !w.is_empty() && !have.contains(&w) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// What Windows OCR can read (the recognizers installed in Windows) and the
/// Windows display language (its first profile language). Empty / `None`
/// elsewhere. Quick, but not free: ask when Settings opens, never while capturing.
pub fn windows_languages() -> (Vec<String>, Option<String>) {
    imp::languages()
}

/// `language`, if it is known yet (never blocks): `None` = not asked yet.
pub fn known() -> Option<Option<&'static str>> {
    #[cfg(test)]
    return LANGUAGE_HERE.get().or(Some(None)); // never this PC's Windows OCR
    #[cfg(not(test))]
    LANGUAGE.get().map(Option::as_deref)
}

#[cfg(test)]
thread_local! {
    /// Tests: what `known` says (else: no Windows OCR).
    pub static LANGUAGE_HERE: std::cell::Cell<Option<Option<&'static str>>> =
        const { std::cell::Cell::new(None) };
}

/// The longest side Windows OCR takes (`OcrEngine::MaxImageDimension`).
const MAX_SIDE: u32 = 10_000;

/// The reads of a `width` x `height` frame drawn at `scale`, as how many
/// times bigger Windows OCR sees it: first as it is (~0.08 s for 1080p on the
/// VM: text selectable right away), then, under 150 %, doubled (~0.24 s),
/// which replaces it: at 1x Windows misses short words in small text (it read
/// Wikipedia's 14 px body text as "needed to trained"). Text at 150 % and up
/// is big enough as it is.
pub fn passes(scale: f32, width: u32, height: u32) -> &'static [u32] {
    if scale < 1.5 && width.max(height) * 2 <= MAX_SIDE {
        &[1, 2]
    } else {
        &[1]
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

/// Reads the text lines in an RGBA image (frame px), seen `k` times bigger
/// (`passes`), in reading order. Blocking: call off the UI thread.
pub fn recognize(rgba: &[u8], width: u32, height: u32, k: u32) -> Result<Vec<TextBlock>, OcrError> {
    super::check_image(rgba, width, height)?;
    language().ok_or_else(|| {
        OcrError::Engine("Windows OCR has no recognizer for your languages".into())
    })?;
    let _gently = imp::below_normal(); // the overlay's frames come first
    let start = Instant::now();
    let image = grey(rgba, width, k);
    let (w, h) = (width * k, height * k);
    let mut engine = lock();
    if engine.is_none() {
        *engine = imp::make().map(|(e, _)| (e, Instant::now())); // freed when idle
    }
    let Some((made, used)) = engine.as_mut() else {
        return Err(OcrError::Engine("Windows OCR didn't start".into()));
    };
    let read = imp::read(made, &image, w, h);
    *used = Instant::now();
    drop(engine);
    free_when_idle();
    let mut lines = read?;
    let n = lines.len();
    for (_, b) in lines.iter_mut().flatten() {
        *b = b.map(|v| v / k as f32);
    }
    let out = blocks(lines);
    crate::log!(
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
    use windows::Win32::System::Com::CoIncrementMTAUsage;

    /// WinRT for the whole process, for good: the engine (`ENGINE`) and the
    /// `windows` crate's cached `OcrEngine` statics outlive the thread that
    /// made them (made on one read's thread, used by the next, freed by
    /// `free_when_idle`'s), so the apartment must outlive them all. A thread's
    /// own `CoInitializeEx` ended with the thread: once the last such thread
    /// was gone COM unloaded the OCR DLLs, and the next read crashed in
    /// `MaxImageDimension` (0xc0000005; CI's tests, VM). Every thread not in
    /// an apartment of its own is then in this multithreaded one.
    fn com() {
        static MTA: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        MTA.get_or_init(|| {
            // SAFETY: no pointers; the cookie is kept (never decremented).
            if let Err(e) = unsafe { CoIncrementMTAUsage() } {
                crate::log!("[opencircle] Windows OCR: no apartment: {e}");
            }
        });
    }

    /// This thread at below-normal priority until the guard drops.
    pub(super) fn below_normal() -> impl Drop {
        use windows::Win32::System::Threading::{
            GetCurrentThread, GetThreadPriority, SetThreadPriority, THREAD_PRIORITY,
            THREAD_PRIORITY_BELOW_NORMAL,
        };
        struct Restore(i32);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: the pseudo handle of this thread.
                let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY(self.0)) };
            }
        }
        // SAFETY: as above.
        let before = unsafe { GetThreadPriority(GetCurrentThread()) };
        // SAFETY: as above.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
        Restore(before)
    }

    pub(super) fn make() -> Option<(OcrEngine, String)> {
        com();
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok()?;
        let tag = engine.RecognizerLanguage().and_then(|l| l.LanguageTag());
        let tag = tag.map(|t| t.to_string()).unwrap_or_default();
        Some((engine, tag))
    }

    /// Installed recognizers' tags, and the first profile language's.
    pub(super) fn languages() -> (Vec<String>, Option<String>) {
        use windows::System::UserProfile::GlobalizationPreferences;
        let installed = OcrEngine::AvailableRecognizerLanguages()
            .map(|v| v.into_iter().filter_map(|l| l.LanguageTag().ok()))
            .map(|tags| tags.map(|t| t.to_string()).collect())
            .unwrap_or_default();
        let first = GlobalizationPreferences::Languages()
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|t| t.to_string());
        (installed, first)
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

    pub(super) fn below_normal() -> impl Sized {}

    pub(super) fn languages() -> (Vec<String>, Option<String>) {
        (Vec::new(), None)
    }

    pub(super) fn read(_: &(), _: &[u8], _: u32, _: u32) -> Result<Vec<Vec<Word>>, OcrError> {
        unreachable!("no engine is made on this OS")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_recognizers_are_found_by_base_language() {
        let s = |v: &[&str]| v.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        let have = s(&["en-US", "fr-FR"]);
        assert!(super::missing(&have, &s(&["en-GB", "fr"])).is_empty());
        assert_eq!(
            super::missing(&have, &s(&["ja", "en", "ja-JP", "AR-sa"])),
            ["ja", "ar"]
        );
        assert_eq!(super::missing(&[], &s(&["en-US"])), ["en"]);
        assert!(super::missing(&have, &[]).is_empty());
    }

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
        assert_eq!(passes(1.0, 1920, 1080), [1, 2], "fast first, then doubled");
        assert_eq!(passes(1.25, 3840, 2160), [1, 2]);
        assert_eq!(passes(1.5, 2880, 1620), [1]);
        assert_eq!(passes(1.75, 1920, 1080), [1]);
        assert_eq!(
            passes(1.0, 5120, 1440),
            [1],
            "10240 px is over Windows' limit"
        );
        // 2 x 1: white, then pure red, green and blue weigh in as luma
        let rgba = [255, 255, 255, 255, 255, 0, 0, 255];
        assert_eq!(grey(&rgba, 2, 1), [255, 76]);
        assert_eq!(grey(&rgba, 2, 2), [255, 255, 76, 76, 255, 255, 76, 76]);
        let column = [0, 150, 0, 255, 0, 0, 255, 255]; // 1 x 2
        assert_eq!(grey(&column, 1, 2), [87, 87, 87, 87, 28, 28, 28, 28]);
    }

    /// The engine is freed once unused for `IDLE` (60 s; 300 ms in tests),
    /// not while it is used. Linux: a stand-in engine in `ENGINE`.
    #[test]
    #[cfg(not(windows))]
    fn the_engine_is_freed_once_unused_for_a_while() {
        *lock() = Some(((), Instant::now()));
        free_when_idle();
        for _ in 0..5 {
            std::thread::sleep(IDLE / 2);
            assert!(lock().is_some(), "in use: kept");
            lock().as_mut().unwrap().1 = Instant::now(); // used again
        }
        std::thread::sleep(IDLE * 2);
        assert!(lock().is_none(), "unused: freed");
        assert!(!FREEING.load(Ordering::SeqCst), "and its thread ended");
    }

    /// Off Windows (and in tests) there is no Windows OCR: PP-OCR reads.
    #[test]
    #[cfg(not(windows))]
    fn there_is_no_windows_ocr_elsewhere() {
        assert_eq!(language(), None);
        assert!(matches!(
            recognize(&[0; 16], 2, 2, 1),
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
        let began = Instant::now();
        let got = recognize(&rgba, info.width, info.height, 2).unwrap();
        // CI latency budget (#59): a cold read (engine made) of a small screen
        // is far under this even on a shared runner.
        let took = began.elapsed();
        println!("Windows OCR took {took:?}");
        assert!(took < Duration::from_millis(1500), "OCR took {took:?}");
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

    /// The engine outlives the threads that use it: reads overlap, each on a
    /// thread that then ends, the engine is freed by yet another (as
    /// `free_when_idle` does) and made again. Before: COM ended with the last
    /// reading thread and the next free or read of the kept engine crashed the
    /// process (CI, 0xc0000005).
    #[test]
    #[cfg(windows)]
    fn the_engine_outlives_the_threads_that_read() {
        if std::thread::spawn(language).join().unwrap().is_none() {
            println!("SKIPPED: this Windows has no OCR recognizer for its languages");
            return;
        }
        let png = png::Decoder::new(std::io::Cursor::new(include_bytes!("fixture.png")));
        let mut reader = png.read_info().unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut rgba).unwrap();
        let (rgba, w, h) = (std::sync::Arc::new(rgba), info.width, info.height);
        for _ in 0..4 {
            let reads: Vec<_> = (1..=3)
                .map(|k| {
                    let rgba = rgba.clone();
                    std::thread::spawn(move || recognize(&rgba, w, h, k).map(|b| b.len()))
                })
                .collect();
            for read in reads {
                assert!(read.join().unwrap().unwrap() > 0, "read");
            }
            std::thread::sleep(Duration::from_millis(100)); // the readers are gone
            std::thread::spawn(|| drop(lock().take())).join().unwrap();
        }
    }
}
