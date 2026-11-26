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
use super::{
    Engine, NO_FRAME, OcrError, Read, ScreenRead, TextBlock, check_image, models, models_dir,
    to_rgb,
};
use crate::features::translate::mt::{self, Pair, Translator};

/// The hidden subcommand that runs the worker.
pub const WORKER_ARG: &str = "ocr-worker";
const OCR: u8 = b'o';
/// Main mode's second reader (`Engine::recognize_except`): `x`, gently (one
/// byte), how many known boxes (u32 LE), each as four f32 LE (x, y, w, h;
/// image px), then as `OCR`: width, height, RGBA.
const EXCEPT: u8 = b'x';
/// Main mode's screen text, a part at a time (`Engine::read_screen`): `s`,
/// gently (one byte), the Latin reader may read (one byte), the screen's key
/// (u64 LE), the rows (top, bottom: f32 LE each), how many known boxes (u32
/// LE) and each as `EXCEPT`'s, then whether a picture follows (one byte)
/// and, if so, its top row in the screen (u32 LE) and as `OCR`: width,
/// height, RGBA (the whole screen, or the rows a part needs). Reply
/// `ScreenRead`.
const SCREEN: u8 = b's';
/// Load the engine and a recognizer (one byte, as `OCR`'s), gently; reply `null`.
const WARM: u8 = b'w';
const TRANSLATE: u8 = b't';
/// Cut out (`features::cutout`): `c`, the picture as the model's input
/// (RGB bytes); reply the model's mask (bytes). No session outlives a request.
const CUTOUT: u8 = b'c';
/// Far more lines than any screen holds.
const MAX_KNOWN: usize = 100_000;
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
    let source = crate::shared::config::current().translate.source;
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

/// The cut-out model's mask for `input` (`cutout::model::pre`'s bytes), in the
/// helper. The model must already be on disk (the helper never downloads).
pub fn cut_out_in_child(input: &[u8]) -> Result<Vec<u8>, OcrError> {
    if crate::features::cutout::model::missing_bytes() > 0 {
        return Err(OcrError::Missing("the cut-out model".into()));
    }
    parse(session::call(&[&[CUTOUT], input])?)
}

/// Some recognizer for a script Windows OCR doesn't read for the user is on
/// disk: the second reader has something to read with.
pub fn reads_other_scripts() -> bool {
    (models::RECOGNIZERS.iter()).any(|r| {
        r.script() != crate::features::translate::lang::Script::Latin && models::installed(r)
    })
}

/// The lines of an RGBA image that aren't in the `known` boxes (Windows
/// OCR's lines; image px), read in the script that suits each, in the
/// helper (`Engine::recognize_except`); `gently` (main mode: in the
/// background) on two cores below normal, else (a translate the user waits
/// for) on all of them: an Arabic page on the Win10 VM (4 cores) read in
/// 14.7-17.3 s gently. None without a non-Latin recognizer on disk (the
/// helper isn't even started).
pub fn recognize_except(
    rgba: &[u8],
    width: u32,
    height: u32,
    known: &[[f32; 4]],
    gently: bool,
) -> Result<Vec<TextBlock>, OcrError> {
    check_image(rgba, width, height)?;
    if !reads_other_scripts() {
        return Ok(Vec::new());
    }
    let mut header = vec![EXCEPT, u8::from(gently)];
    header.extend_from_slice(&(known.len() as u32).to_le_bytes());
    for v in known.iter().flatten() {
        header.extend_from_slice(&v.to_le_bytes());
    }
    header.extend_from_slice(&width.to_le_bytes());
    header.extend_from_slice(&height.to_le_bytes());
    parse(session::call(&[&header, rgba])?)
}

/// Main mode's screen text, a part at a time, in the helper (#169,
/// `Engine::read_screen`): the lines whose middle is in `rows` (image px)
/// and not in a `known` box (Windows OCR's lines read well), and every line
/// found on the screen not known. `part`: the rows of the screen (`key`:
/// one per opening) sent with this part: all of them once (the second
/// reader judges the whole screen), or each part's own (`part_rows`: the
/// helper keeps the lines it found, not the picture); `None`: the screen
/// the helper holds, sent whole again if it started again since. `latin`:
/// the Latin reader may read (main mode's own read); else lines in Latin
/// letters are Windows OCR's. `gently`: on at most two of the helper's
/// cores at below-normal priority (`Gentle`), so the overlay and the rest
/// of the PC keep the others.
#[allow(clippy::too_many_arguments)]
pub fn read_screen(
    key: u64,
    rgba: &[u8],
    width: u32,
    height: u32,
    part: Option<(u32, u32)>,
    rows: (f32, f32),
    known: &[[f32; 4]],
    latin: bool,
    gently: bool,
) -> Result<ScreenRead, OcrError> {
    check_image(rgba, width, height)?;
    match latin {
        true => ready(Read::Auto)?,
        false if !reads_other_scripts() => {
            return Ok(ScreenRead {
                lines: Vec::new(),
                found: Vec::new(),
                done: true,
                noise: 0,
            });
        }
        false => {}
    }
    let call = |part: Option<(u32, u32)>| {
        let mut header = vec![SCREEN, u8::from(gently), u8::from(latin)];
        header.extend_from_slice(&key.to_le_bytes());
        header.extend_from_slice(&rows.0.to_le_bytes());
        header.extend_from_slice(&rows.1.to_le_bytes());
        header.extend_from_slice(&(known.len() as u32).to_le_bytes());
        for v in known.iter().flatten() {
            header.extend_from_slice(&v.to_le_bytes());
        }
        header.push(u8::from(part.is_some()));
        let Some((top, bottom)) = part else {
            return parse(session::call(&[&header])?);
        };
        let bottom = bottom.min(height);
        let top = top.min(bottom);
        header.extend_from_slice(&top.to_le_bytes());
        header.extend_from_slice(&width.to_le_bytes());
        header.extend_from_slice(&(bottom - top).to_le_bytes());
        let row = width as usize * 4;
        parse(session::call(&[
            &header,
            &rgba[top as usize * row..bottom as usize * row],
        ])?)
    };
    match call(part) {
        Err(OcrError::Engine(e)) if part.is_none() && e == NO_FRAME => call(Some((0, height))),
        read => read,
    }
}

/// Starts the helper (held: it stays warm) and loads the engine and the
/// recognizer for the configured source, gently, so the first read is quick.
/// Nothing is started without the models.
pub fn warm() -> Result<(), OcrError> {
    let read = Read::for_source(&crate::shared::config::current().translate.source);
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

/// Translates `texts` with `pair` offline in the helper (`crate::features::translate::mt`; its
/// models must already be present).
pub fn translate_in_child(
    texts: &[String],
    pair: &'static Pair,
    retry: bool,
) -> Result<Vec<String>, OcrError> {
    if pair.missing_bytes() > 0 {
        return Err(OcrError::Missing(pair.label()));
    }
    translate_via(session::call, texts, pair, retry)
}

#[cfg(test)]
pub(crate) fn translate_with(
    exe: &Path,
    texts: &[String],
    pair: &Pair,
) -> Result<Vec<String>, OcrError> {
    translate_via(|p| Session::worker(exe).call(p), texts, pair, false)
}

/// Translation through `call` (one helper request).
pub(crate) fn translate_via(
    call: impl FnOnce(&[&[u8]]) -> Result<Vec<u8>, OcrError>,
    texts: &[String],
    pair: &Pair,
    retry: bool,
) -> Result<Vec<String>, OcrError> {
    let input = serde_json::to_vec(&(pair.source, pair.target, texts, retry))
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
    let mut engine: Option<Engine> = None;
    let mut translator = Mt::new();
    loop {
        let (id, request) = match session::read_frame(&mut input, MAX_REQUEST) {
            Ok(Some(frame)) => frame,
            Ok(None) => return ExitCode::SUCCESS,
            Err(_) => return ExitCode::FAILURE,
        };
        *idle.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // only main mode's screen requests come one after another for one screen
        if request.first() != Some(&SCREEN)
            && let Some(e) = engine.as_mut()
        {
            e.forget_screen();
        }
        let reply = match request.split_first() {
            Some((&SCREEN, body)) => json(screen_here(body, &mut engine)),
            Some((&OCR, body)) => json(recognize_here(body, &mut engine)),
            Some((&EXCEPT, body)) => json(except_here(body, &mut engine)),
            Some((&WARM, body)) => {
                let _gentle = Gentle::begin();
                json(warm_here(body, &mut engine))
            }
            Some((&TRANSLATE, body)) => json(translate_here(body, &mut translator)),
            Some((&CUTOUT, body)) => json(crate::features::cutout::model::run(&models_dir(), body)),
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
        crate::shared::platform::sys::mem::keep_free_heap();
        let mark = idle.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let mut since = mark.lock().unwrap_or_else(|e| e.into_inner());
                if since.is_some_and(|t| t.elapsed() >= IDLE_TRIM) {
                    *since = None;
                    drop(since);
                    crate::shared::platform::sys::mem::trim_heap();
                }
            }
        });
    }
    idle
}

fn json<T: serde::Serialize>(reply: Result<T, OcrError>) -> Vec<u8> {
    serde_json::to_vec(&reply.map_err(|e| e.detail())).unwrap_or_default()
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

fn screen_here(body: &[u8], engine: &mut Option<Engine>) -> Result<ScreenRead, OcrError> {
    let short = || OcrError::BadImage("short request".into());
    let (head, body) = body.split_first_chunk::<22>().ok_or_else(short)?;
    let f32_at = |i: usize| f32::from_le_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
    let key = u64::from_le_bytes(head[2..10].try_into().expect("8 bytes"));
    let rows = (f32_at(10), f32_at(14));
    let n = u32::from_le_bytes([head[18], head[19], head[20], head[21]]) as usize;
    if n > MAX_KNOWN || body.len() < n * 16 + 1 {
        return Err(short());
    }
    let (boxes, body) = body.split_at(n * 16);
    let f = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let known: Vec<[f32; 4]> = (boxes.as_chunks::<16>().0.iter())
        .map(|b| [f(&b[0..]), f(&b[4..]), f(&b[8..]), f(&b[12..])])
        .collect();
    let img = match body.split_first() {
        Some((0, _)) => None,
        Some((_, body)) => {
            let (size, rgba) = body.split_first_chunk::<12>().ok_or_else(short)?;
            let top = u32::from_le_bytes([size[0], size[1], size[2], size[3]]);
            let w = u32::from_le_bytes([size[4], size[5], size[6], size[7]]);
            let h = u32::from_le_bytes([size[8], size[9], size[10], size[11]]);
            Some((to_rgb(rgba, w, h)?, top))
        }
        None => return Err(short()),
    };
    let _gentle = (head[0] != 0).then(Gentle::begin);
    loaded(engine)?.read_screen(key, img, rows, &known, head[1] != 0)
}

fn except_here(body: &[u8], engine: &mut Option<Engine>) -> Result<Vec<TextBlock>, OcrError> {
    let short = || OcrError::BadImage("short request".into());
    let (head, body) = body.split_first_chunk::<5>().ok_or_else(short)?;
    let n = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if n > MAX_KNOWN || body.len() < n * 16 {
        return Err(short());
    }
    let (boxes, body) = body.split_at(n * 16);
    let f = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let known: Vec<[f32; 4]> = (boxes.as_chunks::<16>().0.iter())
        .map(|b| [f(&b[0..]), f(&b[4..]), f(&b[8..]), f(&b[12..])])
        .collect();
    let (size, rgba) = body.split_first_chunk::<8>().ok_or_else(short)?;
    let w = u32::from_le_bytes([size[0], size[1], size[2], size[3]]);
    let h = u32::from_le_bytes([size[4], size[5], size[6], size[7]]);
    check_image(rgba, w, h)?;
    let _gentle = (head[0] != 0).then(Gentle::begin);
    loaded(engine)?.recognize_except(rgba, w, h, &known)
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

/// The helper's translation models (`T`: a `Translator`): the last two pairs
/// asked for, each loaded or loading, the older one first (a pivot pair
/// ru -> en -> ar alternates two models; one slot reloaded them every call).
/// A warm-up (an empty request) loads a model on its own thread and is
/// answered at once, also when that pair is already loading, so the screen
/// read asked for next runs meanwhile (#117, W96). A model only warmed up
/// (no translation used it yet) is dropped when another warm-up arrives, so
/// the two kept are models that were really used (W96).
pub(crate) struct Mt<T> {
    slots: Vec<Slot<T>>,
    /// Pairs only warmed up so far.
    unused: Vec<&'static Pair>,
    /// Loads of dropped slots, still running (they can't be called off):
    /// at most one, so a third load never stacks another ~250 MB on them.
    gone: Vec<std::thread::JoinHandle<Result<T, OcrError>>>,
}

enum Slot<T> {
    Loading(&'static Pair, std::thread::JoinHandle<Result<T, OcrError>>),
    Loaded(&'static Pair, T),
}

impl<T> Slot<T> {
    fn pair(&self) -> &'static Pair {
        match self {
            Slot::Loading(p, _) | Slot::Loaded(p, _) => p,
        }
    }
}

/// Models kept at once (~+250 MB while warm for the second one).
const KEPT: usize = 2;

impl<T> Mt<T> {
    pub(crate) fn new() -> Self {
        Mt {
            slots: Vec::new(),
            unused: Vec::new(),
            gone: Vec::new(),
        }
    }

    /// Frees a slot; a load under way runs on, kept in `gone`.
    fn drop_slot(&mut self, slot: Slot<T>) {
        if let Slot::Loading(_, job) = slot {
            self.gone.push(job);
        }
    }
}

impl<T: Send + 'static> Mt<T> {
    /// `pair`'s model, loaded by `load` if it isn't (a load under way is
    /// waited for; past two models the least recently used is freed first).
    /// `warm`: only start loading it, on a thread (`None`).
    pub(crate) fn get(
        &mut self,
        pair: &'static Pair,
        warm: bool,
        load: impl FnOnce() -> Result<T, OcrError> + Send + 'static,
    ) -> Result<Option<&mut T>, OcrError> {
        // (a finished dropped load still holds its model until its handle goes)
        self.gone.retain(|j| !j.is_finished());
        let mine = self.slots.iter().position(|s| std::ptr::eq(s.pair(), pair));
        let mut slot = mine.map(|i| self.slots.remove(i));
        if warm && matches!(slot, Some(Slot::Loading(..))) {
            self.slots.push(slot.take().expect("matched"));
            return Ok(None); // already loading: no waiting for it
        }
        // (a failed warm load is tried again here, as before)
        if let Some(Slot::Loading(p, job)) = slot {
            slot = match job.join() {
                Ok(Ok(t)) => Some(Slot::Loaded(p, t)),
                _ => None,
            };
        }
        if slot.is_none() {
            // one dropped load may still run: a warm-up then waits for its
            // turn (skipped, the real request loads it), a real request waits
            if !self.gone.is_empty() {
                if warm {
                    return Ok(None);
                }
                self.gone.drain(..).for_each(|j| drop(j.join()));
            }
            // a warm-up replaces a warmed-up model nobody used (a wrong guess);
            // a real request keeps it (its load can't be called off, and the
            // page may still need it: reloading it cost time and peak memory)
            if warm {
                let unused = std::mem::take(&mut self.unused);
                let (old, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.slots)
                    .into_iter()
                    .partition(|s| unused.iter().any(|p| std::ptr::eq(*p, s.pair())));
                self.slots = kept;
                old.into_iter().for_each(|s| self.drop_slot(s));
            }
            if self.slots.len() >= KEPT {
                let oldest = self.slots.remove(0);
                self.drop_slot(oldest);
            }
            if warm {
                self.unused.push(pair);
                self.slots
                    .push(Slot::Loading(pair, std::thread::spawn(load)));
                return Ok(None);
            }
            slot = Some(Slot::Loaded(pair, load()?));
        }
        if !warm {
            self.unused.retain(|p| !std::ptr::eq(*p, pair)); // used now
        }
        self.slots.push(slot.expect("loaded above"));
        match self.slots.last_mut() {
            Some(Slot::Loaded(_, t)) => Ok(Some(t)),
            _ => unreachable!("loaded above"),
        }
    }
}

/// How long a model that would not load waits before its one second try.
const LOAD_AGAIN: std::time::Duration = std::time::Duration::from_millis(600);

/// A model load that failed is tried once more after `wait` (`None`: not at
/// all, Live's way): its file may be held for a moment (another OpenCircle
/// renaming the folder or swapping the decoder, antivirus scanning a new
/// file). A missing model is not retried.
fn load_again<T>(
    wait: Option<std::time::Duration>,
    mut load: impl FnMut() -> Result<T, OcrError>,
) -> Result<T, OcrError> {
    match load() {
        Err(e) if wait.is_some() && !matches!(e, OcrError::Missing(_)) => {
            crate::log!(
                "[opencircle] model load failed ({}); trying once more",
                e.detail()
            );
            std::thread::sleep(wait.unwrap_or_default());
            load()
        }
        done => done,
    }
}

fn translate_here(body: &[u8], loaded: &mut Mt<Translator>) -> Result<Vec<String>, OcrError> {
    if body.len() > MAX_TEXT {
        return Err(OcrError::BadImage("too much text".into()));
    }
    let (source, target, texts, retry): (String, String, Vec<String>, bool) =
        serde_json::from_slice(body)
            .map_err(|e| OcrError::BadImage(format!("bad request: {e}")))?;
    let pair = mt::pair(&source, &target)
        .ok_or_else(|| OcrError::Engine(mt::unavailable(&source, &target)))?;
    let t = Instant::now();
    let load = move || {
        let t = Instant::now();
        let loaded = load_again(retry.then_some(LOAD_AGAIN), || {
            Translator::load(pair, &models_dir())
        });
        crate::log!(
            "[opencircle] mt {} loaded in {} ms",
            pair.label(),
            t.elapsed().as_millis()
        );
        loaded
    };
    let Some(translator) = loaded.get(pair, texts.is_empty(), load)? else {
        return Ok(Vec::new()); // loading meanwhile
    };
    let load = t.elapsed();
    let out = translator.translate(&texts);
    crate::log!(
        "[opencircle] mt {} texts: load {} ms, translate {} ms",
        texts.len(),
        load.as_millis(),
        (t.elapsed() - load).as_millis()
    );
    out
}

#[cfg(test)]
mod mt_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::time::Duration;

    use std::sync::Arc;

    fn slow(n: u32) -> impl FnOnce() -> Result<u32, OcrError> + Send + 'static {
        counted(n, Arc::default())
    }

    /// `slow`, counting its load in `loads` (one counter per test: they run in parallel).
    fn counted(
        n: u32,
        loads: Arc<AtomicUsize>,
    ) -> impl FnOnce() -> Result<u32, OcrError> + Send + 'static {
        move || {
            std::thread::sleep(Duration::from_millis(300));
            loads.fetch_add(1, SeqCst);
            Ok(n)
        }
    }

    #[test]
    fn a_warm_up_loads_meanwhile_and_the_next_request_waits_for_it() {
        let (en_ar, fr_en) = (mt::pair("en", "ar").unwrap(), mt::pair("fr", "en").unwrap());
        let mut m = Mt::new();
        let loads = Arc::new(AtomicUsize::new(0));
        let t = Instant::now();
        assert!(
            m.get(en_ar, true, counted(1, loads.clone()))
                .unwrap()
                .is_none(),
            "answered at once"
        );
        assert!(
            t.elapsed() < Duration::from_millis(200),
            "{:?}",
            t.elapsed()
        );
        // the read asked for next would run here
        assert_eq!(
            m.get(en_ar, false, counted(2, loads.clone())).unwrap(),
            Some(&mut 1),
            "the warm one"
        );
        assert_eq!(
            m.get(en_ar, false, counted(9, loads.clone())).unwrap(),
            Some(&mut 1),
            "kept for the next request"
        );
        assert_eq!(loads.load(SeqCst), 1, "loaded once");
        // another pair: loaded beside it
        assert_eq!(m.get(fr_en, false, slow(3)).unwrap(), Some(&mut 3));
        // a failed warm load is tried again by the next request
        let mut m: Mt<u32> = Mt::new();
        let fail = || Err(OcrError::Engine("broken".into()));
        assert!(m.get(en_ar, true, fail).unwrap().is_none());
        assert_eq!(
            m.get(en_ar, false, slow(5)).unwrap(),
            Some(&mut 5),
            "tried again"
        );
    }

    // W96 AC-2: a warm-up for a pair that is already loading answers at once
    // (the helper no longer waits for the whole load before the screen read).
    #[test]
    fn ac2_a_warm_up_of_a_loading_pair_does_not_wait() {
        let en_ar = mt::pair("en", "ar").unwrap();
        let mut m = Mt::new();
        assert!(m.get(en_ar, true, slow(1)).unwrap().is_none());
        let t = Instant::now();
        assert!(m.get(en_ar, true, slow(2)).unwrap().is_none());
        assert!(
            t.elapsed() < Duration::from_millis(100),
            "waited {:?}",
            t.elapsed()
        );
        assert_eq!(m.get(en_ar, false, slow(3)).unwrap(), Some(&mut 1));
    }

    // W96 AC-4: a model that was only warmed up (no translation used it) is
    // dropped when a second warm-up arrives; a used one stays.
    #[test]
    fn ac4_a_warmed_model_nobody_used_is_dropped_when_another_pair_loads() {
        let (en_ar, es_en, fr_en) = (
            mt::pair("en", "ar").unwrap(),
            mt::pair("es", "en").unwrap(),
            mt::pair("fr", "en").unwrap(),
        );
        let fresh = |n| move || -> Result<u32, OcrError> { Ok(n) };
        let kept = |m: &Mt<u32>| m.slots.iter().map(|s| s.pair().label()).collect::<Vec<_>>();
        // never used: the real pair's warm-up replaces it
        let mut m = Mt::new();
        m.get(en_ar, true, fresh(1)).unwrap();
        m.get(es_en, true, fresh(2)).unwrap();
        assert_eq!(kept(&m), [es_en.label()]);
        // used: it stays beside the next one
        let mut m = Mt::new();
        m.get(en_ar, true, fresh(1)).unwrap();
        m.get(en_ar, false, fresh(9)).unwrap();
        m.get(fr_en, true, fresh(3)).unwrap();
        assert_eq!(kept(&m), [en_ar.label(), fr_en.label()]);
        // a real request for another pair keeps an unused warm-up
        let mut m = Mt::new();
        m.get(en_ar, true, fresh(1)).unwrap();
        m.get(es_en, false, fresh(2)).unwrap();
        assert_eq!(kept(&m), [en_ar.label(), es_en.label()]);
    }

    // W96 AC-5: at most one dropped load runs on beside the live ones: a third
    // warm-up is skipped while it does; a real request waits for it.
    #[test]
    fn ac5_a_new_warm_up_does_not_stack_a_load_on_a_dropped_one() {
        let (en_ar, es_en, fr_en) = (
            mt::pair("en", "ar").unwrap(),
            mt::pair("es", "en").unwrap(),
            mt::pair("fr", "en").unwrap(),
        );
        let kept = |m: &Mt<u32>| m.slots.iter().map(|s| s.pair().label()).collect::<Vec<_>>();
        let mut m = Mt::new();
        m.get(en_ar, true, slow(1)).unwrap(); // loading
        m.get(es_en, true, slow(2)).unwrap(); // replaces it: its load runs on
        assert_eq!(kept(&m), [es_en.label()]);
        // a third warm-up while that one still runs is skipped
        assert!(m.get(fr_en, true, slow(3)).unwrap().is_none());
        assert_eq!(kept(&m), [es_en.label()], "nothing stacked");
        // a real request waits for the dropped load, then loads
        let t = Instant::now();
        assert_eq!(m.get(fr_en, false, slow(4)).unwrap(), Some(&mut 4));
        assert!(
            t.elapsed() >= Duration::from_millis(250),
            "{:?}",
            t.elapsed()
        );
        // once it is over a warm-up goes ahead again
        let mut m = Mt::new();
        m.get(en_ar, true, slow(1)).unwrap();
        m.get(es_en, true, slow(2)).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        m.get(fr_en, true, slow(3)).unwrap();
        assert_eq!(kept(&m), [fr_en.label()], "the unused warm-up was replaced");
    }

    // W96 AC-5: a dropped load that has finished frees its model at the next
    // request (its handle holds the result until then).
    #[test]
    fn ac5_a_finished_dropped_load_frees_its_model() {
        let (en_ar, es_en) = (mt::pair("en", "ar").unwrap(), mt::pair("es", "en").unwrap());
        let probe = Arc::new(());
        let (a, b) = (probe.clone(), Arc::new(()));
        let mut m: Mt<Arc<()>> = Mt::new();
        m.get(en_ar, true, move || Ok(a)).unwrap();
        m.get(es_en, true, move || Ok(b)).unwrap(); // drops the first: it runs on
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(Arc::strong_count(&probe), 2, "held by the finished handle");
        m.get(es_en, false, || Ok(Arc::new(()))).unwrap();
        assert_eq!(Arc::strong_count(&probe), 1, "freed");
    }

    // W96 AC-3: the last two models stay loaded (a pivot pair ru -> en -> ar
    // alternates two); a third frees the least recently used.
    #[test]
    fn ac3_the_helper_keeps_the_last_two_models() {
        let (en_ar, ru_en, fr_en) = (
            mt::pair("en", "ar").unwrap(),
            mt::pair("ru", "en").unwrap(),
            mt::pair("fr", "en").unwrap(),
        );
        let fresh = |n| move || -> Result<u32, OcrError> { Ok(n) };
        let mut m = Mt::new();
        m.get(en_ar, false, fresh(1)).unwrap();
        m.get(ru_en, false, fresh(2)).unwrap();
        assert_eq!(
            m.get(en_ar, false, fresh(10)).unwrap(),
            Some(&mut 1),
            "kept"
        );
        assert_eq!(
            m.get(ru_en, false, fresh(20)).unwrap(),
            Some(&mut 2),
            "kept"
        );
        // en_ar is now the least recently used: the third pair frees it
        m.get(fr_en, false, fresh(3)).unwrap();
        assert_eq!(m.get(ru_en, false, fresh(21)).unwrap(), Some(&mut 2));
        assert_eq!(
            m.get(en_ar, false, fresh(11)).unwrap(),
            Some(&mut 11),
            "freed, reloaded"
        );
    }
}

#[cfg(test)]
mod w116_tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    // W116 AC-1: a helper's Engine error crosses the pipe once: no second
    // "text recognition failed:" label, and a translation model's failure
    // never reads as text recognition.
    #[test]
    fn ac1_the_helper_reply_carries_the_error_without_its_label() {
        let load = "Load model from X failed:system error number 13";
        let reply = json::<Vec<String>>(Err(OcrError::Engine(load.into())));
        let err = parse::<Vec<String>>(reply).unwrap_err();
        assert_eq!(err.detail(), load);
        assert_eq!(
            err.to_string().matches("text recognition failed").count(),
            1,
            "{err}"
        );
    }

    // W116 AC-2: a load that fails once is tried again; a missing model is not.
    #[test]
    fn ac2_a_failed_model_load_is_tried_once_more() {
        let calls = Cell::new(0);
        let flaky = || {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => Err(OcrError::Engine("held".into())),
                n => Ok(n),
            }
        };
        assert_eq!(load_again(Some(Duration::ZERO), flaky).unwrap(), 2);
        let calls = Cell::new(0);
        let broken = || -> Result<u32, OcrError> {
            calls.set(calls.get() + 1);
            Err(OcrError::Engine("bad".into()))
        };
        assert!(load_again(Some(Duration::ZERO), broken).is_err());
        assert_eq!(calls.get(), 2, "once more, not forever");
        let calls = Cell::new(0);
        let missing = || -> Result<u32, OcrError> {
            calls.set(calls.get() + 1);
            Err(OcrError::Missing("x".into()))
        };
        assert!(load_again(Some(Duration::ZERO), missing).is_err());
        assert_eq!(calls.get(), 1);
        // Live: no second try, no wait
        let calls = Cell::new(0);
        let broken = || -> Result<u32, OcrError> {
            calls.set(calls.get() + 1);
            Err(OcrError::Engine("bad".into()))
        };
        assert!(load_again(None, broken).is_err());
        assert_eq!(calls.get(), 1);
    }

    // W116 AC-9: the request tells the helper whether a failed load is tried again:
    // the one-shot Translate says yes, Live and the warm-ups say no.
    #[test]
    fn ac9_the_request_carries_whether_a_failed_load_is_retried() {
        let pair = mt::pair("en", "ar").unwrap();
        for retry in [false, true] {
            let seen = std::cell::RefCell::new(Vec::new());
            let call = |p: &[&[u8]]| -> Result<Vec<u8>, OcrError> {
                *seen.borrow_mut() = p[1].to_vec();
                Err(OcrError::Engine("stop".into()))
            };
            let _ = translate_via(call, &["a".to_string()], pair, retry);
            let (_, _, _, asked): (String, String, Vec<String>, bool) =
                serde_json::from_slice(&seen.borrow()).unwrap();
            assert_eq!(asked, retry);
        }
    }
}
