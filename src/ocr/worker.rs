//! OCR in a short-lived child process (`opencircle ocr-worker`), so the
//! resident app never loads ONNX Runtime or keeps its memory.
//!
//! Protocol: stdin = width (u32 LE), height (u32 LE), width*height*4 RGBA bytes.
//! stdout = JSON `{"Ok":[TextBlock...]}` or `{"Err":"message"}`, then exit.
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use super::{Engine, OcrError, TextBlock, check_image, models_dir};

/// The hidden subcommand that runs the worker.
pub const WORKER_ARG: &str = "ocr-worker";

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
    let mut child = Command::new(exe)
        .arg(WORKER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return Err(OcrError::Engine("OCR helper has no pipes".into()));
    };
    let deadline = Instant::now() + TIMEOUT;
    let (status, reply) = std::thread::scope(|s| {
        // A dead child makes these fail or end; the exit status says why.
        s.spawn(move || {
            let _ = stdin
                .write_all(&width.to_le_bytes())
                .and_then(|_| stdin.write_all(&height.to_le_bytes()))
                .and_then(|_| stdin.write_all(rgba));
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
                        "OCR helper timed out after {TIMEOUT:?}"
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
        return Err(OcrError::Engine(format!("OCR helper stopped ({status})")));
    }
    let reply = match reply {
        Ok(Ok(reply)) => reply,
        _ => return Err(OcrError::Engine("OCR helper's reply was lost".into())),
    };
    serde_json::from_slice::<Result<Vec<TextBlock>, String>>(&reply)
        .map_err(|e| OcrError::Engine(format!("OCR helper's reply is unreadable: {e}")))?
        .map_err(OcrError::Engine)
}

/// Entry point of `opencircle ocr-worker`: one image in, its lines out.
pub fn worker_main() -> ExitCode {
    let result =
        read_image().and_then(|(rgba, w, h)| Engine::load(&models_dir())?.recognize(&rgba, w, h));
    let reply = result.map_err(|e| e.to_string());
    match serde_json::to_writer(std::io::stdout().lock(), &reply) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
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
