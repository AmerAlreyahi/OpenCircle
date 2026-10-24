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
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::session::{self, MAX_REQUEST, Session};
use super::{Engine, OcrError, Read, TextBlock, check_image, models, models_dir};
use crate::mt::{self, Pair, Translator};

/// The hidden subcommand that runs the worker.
pub const WORKER_ARG: &str = "ocr-worker";
const OCR: u8 = b'o';
const TRANSLATE: u8 = b't';
/// 1 MB of text is far beyond any screen.
const MAX_TEXT: usize = 1 << 20;
/// Idle this long, the helper gives its freed memory back to the OS.
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
    ready(read)?;
    recognize_via(session::call, rgba, width, height, read)
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

#[cfg(test)]
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
    let engine = match engine {
        Some(e) => e,
        None => engine.insert(Engine::load(&models_dir())?),
    };
    engine.recognize(rgba, w, h, read)
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
    if !loaded.as_ref().is_some_and(|(p, _)| std::ptr::eq(*p, pair)) {
        *loaded = None; // free the other pair first
        *loaded = Some((pair, Translator::load(pair, &models_dir())?));
    }
    let (_, translator) = loaded.as_mut().expect("just loaded");
    translator.translate(&texts)
}
