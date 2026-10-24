//! OCR and offline translation in a short-lived child process (`opencircle
//! ocr-worker`), so the resident app never loads ONNX Runtime or keeps its
//! memory.
//!
//! Requests (one per process):
//! - OCR: `ocr-worker`; stdin = width (u32 LE), height (u32 LE),
//!   width*height*4 RGBA bytes; stdout = JSON `{"Ok":[TextBlock...]}` or
//!   `{"Err":"message"}`.
//! - Translate: `ocr-worker translate <target>`; stdin = JSON array of
//!   texts; stdout = JSON `{"Ok":[text...]}` (same length) or `{"Err":"..."}`.
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use super::{Engine, OcrError, TextBlock, check_image, models_dir};

/// The hidden subcommand that runs the worker.
pub const WORKER_ARG: &str = "ocr-worker";
/// `ocr-worker translate <target>`: the offline translation request.
const TRANSLATE_ARG: &str = "translate";

/// Generous: a cold start (ORT + models from disk) plus a text-dense 4K screen.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Reads the text lines of an RGBA image in a child process. The models must
/// already be present (`ensure_models`); the child never downloads.
pub fn recognize_in_child(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<TextBlock>, OcrError> {
    recognize_with(&std::env::current_exe()?, rgba, width, height)
}

pub(super) fn recognize_with(
    exe: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<TextBlock>, OcrError> {
    check_image(rgba, width, height)?;
    let header = [width.to_le_bytes(), height.to_le_bytes()].concat();
    run_child(exe, &[], &[&header, rgba])
}

/// Translates `texts` into `target` offline in a child process (`crate::mt`;
/// its models must already be present).
pub fn translate_in_child(texts: &[String], target: &str) -> Result<Vec<String>, OcrError> {
    translate_with(&std::env::current_exe()?, texts, target)
}

pub(crate) fn translate_with(
    exe: &Path,
    texts: &[String],
    target: &str,
) -> Result<Vec<String>, OcrError> {
    let input = serde_json::to_vec(texts).map_err(|e| OcrError::Engine(e.to_string()))?;
    let out: Vec<String> = run_child(exe, &[TRANSLATE_ARG, target], &[&input])?;
    if out.len() != texts.len() {
        return Err(OcrError::Engine(format!(
            "translation helper returned {} lines for {}",
            out.len(),
            texts.len()
        )));
    }
    Ok(out)
}

/// Runs `exe ocr-worker <args>`, writes `input` to its stdin and reads its
/// JSON reply, within `TIMEOUT`.
fn run_child<T: serde::de::DeserializeOwned>(
    exe: &Path,
    args: &[&str],
    input: &[&[u8]],
) -> Result<T, OcrError> {
    let mut child = Command::new(exe)
        .arg(WORKER_ARG)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return Err(OcrError::Engine("helper has no pipes".into()));
    };
    let deadline = Instant::now() + TIMEOUT;
    let (status, reply) = std::thread::scope(|s| {
        // A dead child makes these fail or end; the exit status says why.
        s.spawn(move || {
            for part in input {
                if stdin.write_all(part).is_err() {
                    break;
                }
            }
        });
        let reader = s.spawn(move || {
            let mut reply = Vec::new();
            stdout.read_to_end(&mut reply).map(|_| reply)
        });
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(OcrError::Engine(format!(
                        "helper timed out after {TIMEOUT:?}"
                    )));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(e.into());
                }
            }
        };
        (status, reader.join())
    });
    let status = status?;
    if !status.success() {
        return Err(OcrError::Engine(format!("helper stopped ({status})")));
    }
    let reply = match reply {
        Ok(Ok(reply)) => reply,
        _ => return Err(OcrError::Engine("helper's reply was lost".into())),
    };
    serde_json::from_slice::<Result<T, String>>(&reply)
        .map_err(|e| OcrError::Engine(format!("helper's reply is unreadable: {e}")))?
        .map_err(OcrError::Engine)
}

/// Entry point of `opencircle ocr-worker`: one request in, its answer out.
pub fn worker_main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let written = match args.as_slice() {
        [t, target] if t == TRANSLATE_ARG => {
            let reply = read_texts()
                .and_then(|texts| crate::mt::translate_here(&texts, target))
                .map_err(|e| e.to_string());
            serde_json::to_writer(std::io::stdout().lock(), &reply)
        }
        _ => {
            let reply = read_image()
                .and_then(|(rgba, w, h)| Engine::load(&models_dir())?.recognize(&rgba, w, h))
                .map_err(|e| e.to_string());
            serde_json::to_writer(std::io::stdout().lock(), &reply)
        }
    };
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn read_texts() -> Result<Vec<String>, OcrError> {
    // 1 MB of text is far beyond any screen; refuse before parsing.
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .take(1 << 20)
        .read_to_end(&mut input)?;
    serde_json::from_slice(&input).map_err(|e| OcrError::BadImage(format!("bad request: {e}")))
}

fn read_image() -> Result<(Vec<u8>, u32, u32), OcrError> {
    let mut stdin = std::io::stdin().lock();
    let mut header = [0; 8];
    stdin.read_exact(&mut header)?;
    let w = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let h = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    // 16k x 16k is beyond any screen; refuse before allocating.
    if w > 16384 || h > 16384 {
        return Err(OcrError::BadImage(format!("{w}x{h} is too large")));
    }
    let mut rgba = vec![0; w as usize * h as usize * 4];
    stdin.read_exact(&mut rgba)?;
    Ok((rgba, w, h))
}
