//! OCR and offline translation in a helper process (`opencircle ocr-worker`),
//! so the resident app never loads ONNX Runtime or keeps its memory. The
//! helper loads each model on its first request and keeps it for the next;
//! `session` decides how long a helper lives (one call, or while held).
//!
//! Requests (framed by `session`; replies are JSON `{"Ok":...}` or
//! `{"Err":"message"}`):
//! - OCR: `o`, the recognizer (`Read`: one byte), width (u32 LE), height
//!   (u32 LE), width*height*4 RGBA bytes;
//!   reply `[TextBlock...]`.
//! - Translate: `t`, JSON `[source, target, [text...]]`; reply `[text...]` (same length).
#[cfg(test)]
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use std::time::Duration;
use std::time::Instant;

#[cfg(test)]
use super::session::Session;
use super::session::{self, MAX_REQUEST};
use super::{Engine, OcrError, Read, TextBlock, check_image, models, models_dir};
use crate::mt::{self, Pair, Translator};

/// The hidden subcommand that runs the worker.
pub const WORKER_ARG: &str = "ocr-worker";
const OCR: u8 = b'o';
/// OCR of a band of the screen (main mode's text, #123): `b`, the recognizer
/// (one byte, as `OCR`'s), gently (`Gentle`, one byte: 0 or 1), the rows to
/// read (top, bottom: u32 LE each; a line is read if its middle is in them),
/// then as `OCR`: width, height, RGBA.
const BAND: u8 = b'b';
/// Load the engine and a recognizer (one byte, as `OCR`'s), gently; reply `null`.
const WARM: u8 = b'w';
const TRANSLATE: u8 = b't';
/// 1 MB of text is far beyond any screen.
const MAX_TEXT: usize = 1 << 20;
/// Idle this long, the helper gives its freed memory back to the OS.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const IDLE_TRIM: Duration = Duration::from_secs(2);

/// Reads the text lines of an RGBA image in the helper, with the recognizer
/// for the configured source language (`translate.source`). The models must
/// already be present (`ensure_models`); the helper never downloads.
pub fn recognize_in_child(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<TextBlock>, OcrError> {
    let source = crate::config::current().translate.source;
    recognize_as(rgba, width, height, &source)
}

/// `recognize_in_child` for `source` ("auto" or a language code).
pub fn recognize_as(
    rgba: &[u8],
    width: u32,
    height: u32,
    source: &str,
) -> Result<Vec<TextBlock>, OcrError> {
    // Bad input is a bad image whether or not the models are on disk.
    check_image(rgba, width, height)?;
    let read = Read::for_source(source);
    // a bad image is the caller's error, models or not
    if width == 0 || height == 0 || rgba.len() as u64 != width as u64 * height as u64 * 4 {
        return Err(OcrError::BadImage(format!(
            "{width}x{height} RGBA needs {} bytes, got {}",
            width as u64 * height as u64 * 4,
            rgba.len()
        )));
    }
    ready(read)?;
    recognize_via(session::call, rgba, width, height, read)
}

/// `recognize_in_child` for a band of main mode's screen text (#123): only
/// the lines whose middle is in `rows` (top, bottom; image px) are read (the
/// rest is cut by its edges or read already). `gently`: on at most two of
/// the helper's cores at below-normal priority (`Gentle`), so the overlay
/// and the rest of the PC keep the others.
pub fn recognize_band(
    rgba: &[u8],
    width: u32,
    height: u32,
    rows: (u32, u32),
    gently: bool,
) -> Result<Vec<TextBlock>, OcrError> {
    check_image(rgba, width, height)?;
    let read = Read::for_source(&crate::config::current().translate.source);
    ready(read)?;
    let header = [
        [BAND, read.byte(), u8::from(gently)].as_slice(),
        &rows.0.to_le_bytes(),
        &rows.1.to_le_bytes(),
        &width.to_le_bytes(),
        &height.to_le_bytes(),
    ]
    .concat();
    parse(session::call(&[&header, rgba])?)
}

/// Starts the helper (held: it stays warm) and loads the engine and the
/// recognizer for the configured source, gently, so the first read is quick.
/// Nothing is started without the models.
pub fn warm() -> Result<(), OcrError> {
    let read = Read::for_source(&crate::config::current().translate.source);
    ready(read)?;
    parse(session::call(&[&[WARM, read.byte()]])?)
}

/// Its models are on disk: no helper is started for a request that can only
/// fail (a helper without ONNX Runtime used to crash, again and again).
fn ready(read: Read) -> Result<(), OcrError> {
    let installed = match read {
        Read::One(i) => models::installed(models::RECOGNIZERS[i]),
        Read::Auto => models::RECOGNIZERS.iter().any(|r| models::installed(r)),
    };
    match installed {
        true => Ok(()),
        false => Err(OcrError::Missing("the text-recognition model".into())),
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(super) fn recognize_with(
    exe: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
    read: Read,
) -> Result<Vec<TextBlock>, OcrError> {
    recognize_via(|p| Session::worker(exe).call(p), rgba, width, height, read)
}

/// OCR through `call` (one helper request).
pub(crate) fn recognize_via(
    call: impl FnOnce(&[&[u8]]) -> Result<Vec<u8>, OcrError>,
    rgba: &[u8],
    width: u32,
    height: u32,
    read: Read,
) -> Result<Vec<TextBlock>, OcrError> {
    check_image(rgba, width, height)?;
    let header = [
        [OCR, read.byte()].as_slice(),
        &width.to_le_bytes(),
        &height.to_le_bytes(),
    ]
    .concat();
    parse(call(&[&header, rgba])?)
}

/// Translates `texts` with `pair` offline in the helper (`crate::mt`; its
/// models must already be present).
pub fn translate_in_child(texts: &[String], pair: &'static Pair) -> Result<Vec<String>, OcrError> {
    if pair.missing_bytes() > 0 {
        return Err(OcrError::Missing(pair.label()));
    }
    translate_via(session::call, texts, pair)
}

#[cfg(test)]
pub(crate) fn translate_with(
    exe: &Path,
    texts: &[String],
    pair: &Pair,
) -> Result<Vec<String>, OcrError> {
    translate_via(|p| Session::worker(exe).call(p), texts, pair)
}

/// Translation through `call` (one helper request).
pub(crate) fn translate_via(
    call: impl FnOnce(&[&[u8]]) -> Result<Vec<u8>, OcrError>,
    texts: &[String],
    pair: &Pair,
) -> Result<Vec<String>, OcrError> {
    let input = serde_json::to_vec(&(pair.source, pair.target, texts))
        .map_err(|e| OcrError::Engine(e.to_string()))?;
    let out: Vec<String> = parse(call(&[&[TRANSLATE], &input])?)?;
    if out.len() != texts.len() {
        return Err(OcrError::Engine(format!(
            "translation helper returned {} lines for {}",
            out.len(),
            texts.len()
        )));
    }
    Ok(out)
}

fn parse<T: serde::de::DeserializeOwned>(reply: Vec<u8>) -> Result<T, OcrError> {
    serde_json::from_slice::<Result<T, String>>(&reply)
        .map_err(|e| OcrError::Engine(format!("helper's reply is unreadable: {e}")))?
        .map_err(OcrError::Engine)
}

/// Entry point of `opencircle ocr-worker`: answers requests until stdin ends.
/// A broken frame ends it (the app then starts another).
pub fn worker_main() -> ExitCode {
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout().lock());
    let idle = heap_keeper();
    let mut engine = None;
    let mut translator = None;
    loop {
        let (id, request) = match session::read_frame(&mut input, MAX_REQUEST) {
            Ok(Some(frame)) => frame,
            Ok(None) => return ExitCode::SUCCESS,
            Err(_) => return ExitCode::FAILURE,
        };
        *idle.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let reply = match request.split_first() {
            Some((&OCR, body)) => json(recognize_here(body, &mut engine)),
            Some((&BAND, body)) => json(band_here(body, &mut engine)),
            Some((&WARM, body)) => {
                let _gentle = Gentle::begin();
                json(warm_here(body, &mut engine))
            }
            Some((&TRANSLATE, body)) => json(translate_here(body, &mut translator)),
            _ => json::<()>(Err(OcrError::BadImage("unknown request".into()))),
        };
        drop(request);
        if session::write_frame(&mut output, id, &[&reply]).is_err() {
            return ExitCode::FAILURE;
        }
        *idle.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
}

/// glibc: freed memory stays in the heap while requests come (ORT runs
/// without its arena, and a translation's ~200 MB would otherwise be mapped
/// and zeroed again on every run, 4x slower), and goes back to the OS once
/// the helper has been idle for `IDLE_TRIM` (the next live step then costs
/// ~45 ms more). The serve loop sets the mark: `Some` = idle since then.
// ponytail: a thread that wakes 4x a second; a condvar if that ever shows.
fn heap_keeper() -> Arc<Mutex<Option<Instant>>> {
    let idle = Arc::new(Mutex::new(None::<Instant>));
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: allocator settings only; no memory is touched.
        unsafe {
            libc::mallopt(libc::M_MMAP_THRESHOLD, 256 << 20);
            libc::mallopt(libc::M_TRIM_THRESHOLD, 1 << 30);
        }
        let mark = idle.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let mut since = mark.lock().unwrap_or_else(|e| e.into_inner());
                if since.is_some_and(|t| t.elapsed() >= IDLE_TRIM) {
                    *since = None;
                    drop(since);
                    // SAFETY: malloc_trim only releases free heap pages.
                    unsafe { libc::malloc_trim(0) };
                }
            }
        });
    }
    idle
}

fn json<T: serde::Serialize>(reply: Result<T, OcrError>) -> Vec<u8> {
    serde_json::to_vec(&reply.map_err(|e| e.to_string())).unwrap_or_default()
}

fn recognize_here(body: &[u8], engine: &mut Option<Engine>) -> Result<Vec<TextBlock>, OcrError> {
    let Some((&byte, body)) = body.split_first() else {
        return Err(OcrError::BadImage("short request".into()));
    };
    let read =
        Read::from_byte(byte).ok_or_else(|| OcrError::BadImage("unknown recognizer".into()))?;
    let Some((size, rgba)) = body.split_first_chunk::<8>() else {
        return Err(OcrError::BadImage("short request".into()));
    };
    let w = u32::from_le_bytes([size[0], size[1], size[2], size[3]]);
    let h = u32::from_le_bytes([size[4], size[5], size[6], size[7]]);
    check_image(rgba, w, h)?;
    loaded(engine)?.recognize(rgba, w, h, read)
}

fn band_here(body: &[u8], engine: &mut Option<Engine>) -> Result<Vec<TextBlock>, OcrError> {
    let short = || OcrError::BadImage("short request".into());
    let (head, body) = body.split_first_chunk::<10>().ok_or_else(short)?;
    let read =
        Read::from_byte(head[0]).ok_or_else(|| OcrError::BadImage("unknown recognizer".into()))?;
    let u32_at = |i: usize| u32::from_le_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
    let rows = (u32_at(2), u32_at(6));
    let (size, rgba) = body.split_first_chunk::<8>().ok_or_else(short)?;
    let w = u32::from_le_bytes([size[0], size[1], size[2], size[3]]);
    let h = u32::from_le_bytes([size[4], size[5], size[6], size[7]]);
    check_image(rgba, w, h)?;
    let _gentle = (head[1] != 0).then(Gentle::begin);
    loaded(engine)?.recognize_rows(rgba, w, h, read, Some(rows))
}

/// The engine, loaded on first use.
fn loaded(engine: &mut Option<Engine>) -> Result<&mut Engine, OcrError> {
    Ok(match engine {
        Some(e) => e,
        None => {
            let t = Instant::now();
            let e = engine.insert(Engine::load(&models_dir())?);
            crate::log!(
                "[opencircle] ocr engine loaded in {} ms",
                t.elapsed().as_millis()
            );
            e
        }
    })
}

fn warm_here(body: &[u8], engine: &mut Option<Engine>) -> Result<(), OcrError> {
    let read = (body.first().copied().and_then(Read::from_byte))
        .ok_or_else(|| OcrError::BadImage("unknown recognizer".into()))?;
    loaded(engine)?.warm(read)
}

/// While it lives, this process runs on at most two of its cores (the
/// highest ones it may use), at below-normal priority on Windows; dropped, as
/// before. Threads started meanwhile (ORT's, on a first load) are covered.
struct Gentle(Option<Box<dyn FnOnce()>>);

impl Drop for Gentle {
    fn drop(&mut self) {
        if let Some(restore) = self.0.take() {
            restore();
        }
    }
}

/// The two highest cores of an affinity `mask`.
#[cfg(windows)]
fn two_of(mask: u64) -> u64 {
    let top = |m: u64| m.checked_ilog2().map_or(0, |b| 1 << b);
    let first = top(mask);
    first | top(mask & !first)
}

#[cfg(all(test, windows))]
#[test]
fn gentle_reads_take_the_two_highest_cores() {
    assert_eq!(two_of(0b1111), 0b1100);
    assert_eq!(two_of(0b0011), 0b0011, "two cores: both");
    assert_eq!(two_of(0b0001), 0b0001);
    assert_eq!(two_of(0b1010_0100), 0b1010_0000);
}

impl Gentle {
    #[cfg(windows)]
    fn begin() -> Gentle {
        use windows::Win32::System::Threading::{
            BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, GetPriorityClass,
            GetProcessAffinityMask, PROCESS_CREATION_FLAGS, SetPriorityClass,
            SetProcessAffinityMask,
        };
        // SAFETY: plain calls on this process's own pseudo handle.
        unsafe {
            let me = GetCurrentProcess();
            let (mut mask, mut system) = (0usize, 0usize);
            if GetProcessAffinityMask(me, &mut mask, &mut system).is_err() {
                return Gentle(None);
            }
            let class = GetPriorityClass(me);
            let _ = SetProcessAffinityMask(me, two_of(mask as u64) as usize);
            let _ = SetPriorityClass(me, BELOW_NORMAL_PRIORITY_CLASS);
            Gentle(Some(Box::new(move || {
                let me = GetCurrentProcess();
                let _ = SetProcessAffinityMask(me, mask);
                if class != 0 {
                    let _ = SetPriorityClass(me, PROCESS_CREATION_FLAGS(class));
                }
            })))
        }
    }

    /// Linux: the cores only. An unprivileged process can lower its threads'
    /// priority but never raise it back (RLIMIT_NICE), and this helper also
    /// translates, at full speed.
    #[cfg(target_os = "linux")]
    fn begin() -> Gentle {
        let size = std::mem::size_of::<libc::cpu_set_t>();
        // SAFETY: a zeroed cpu_set_t is a valid empty set.
        let mut mine: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        // SAFETY: reads this thread's set into a buffer of the given size.
        if unsafe { libc::sched_getaffinity(0, size, &mut mine) } != 0 {
            return Gentle(None);
        }
        let cpus: Vec<usize> = (0..libc::CPU_SETSIZE as usize)
            // SAFETY: c < CPU_SETSIZE.
            .filter(|&c| unsafe { libc::CPU_ISSET(c, &mine) })
            .collect();
        // SAFETY: as above.
        let mut two: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        for &c in cpus.iter().rev().take(2) {
            // SAFETY: c < CPU_SETSIZE.
            unsafe { libc::CPU_SET(c, &mut two) };
        }
        let every_thread = move |set: libc::cpu_set_t| {
            let tasks = std::fs::read_dir("/proc/self/task").into_iter().flatten();
            for tid in tasks
                .flatten()
                .filter_map(|t| t.file_name().to_str()?.parse().ok())
            {
                // SAFETY: a valid set for one of this process's threads.
                unsafe { libc::sched_setaffinity(tid, size, &set) };
            }
        };
        every_thread(two);
        Gentle(Some(Box::new(move || every_thread(mine))))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    fn begin() -> Gentle {
        Gentle(None)
    }
}

fn translate_here(
    body: &[u8],
    loaded: &mut Option<(&'static Pair, Translator)>,
) -> Result<Vec<String>, OcrError> {
    if body.len() > MAX_TEXT {
        return Err(OcrError::BadImage("too much text".into()));
    }
    let (source, target, texts): (String, String, Vec<String>) = serde_json::from_slice(body)
        .map_err(|e| OcrError::BadImage(format!("bad request: {e}")))?;
    let pair = mt::pair(&source, &target)
        .ok_or_else(|| OcrError::Engine(mt::unavailable(&source, &target)))?;
    let t = Instant::now();
    if !loaded.as_ref().is_some_and(|(p, _)| std::ptr::eq(*p, pair)) {
        *loaded = None; // free the other pair first
        *loaded = Some((pair, Translator::load(pair, &models_dir())?));
    }
    let load = t.elapsed();
    let (_, translator) = loaded.as_mut().expect("just loaded");
    let out = translator.translate(&texts);
    crate::log!(
        "[opencircle] mt {} texts: load {} ms, translate {} ms",
        texts.len(),
        load.as_millis(),
        (t.elapsed() - load).as_millis()
    );
    out
}
