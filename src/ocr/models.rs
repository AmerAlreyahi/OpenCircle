//! The pinned files OCR needs (ONNX Runtime + models): fetched over HTTPS on
//! first use, checked against a hard-coded size and SHA-256, written via temp
//! file + rename, then reused. Nothing is unpacked by name: archive members are
//! cut out at pinned offsets, so an archive can't place files anywhere.
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::OcrError;
use crate::translate::lang::Script;

#[derive(Debug)]
pub(crate) struct ModelFile {
    /// File name on disk.
    pub(crate) name: &'static str,
    pub(crate) size: u64,
    pub(crate) sha256: &'static str,
    source: Source,
}

#[derive(Debug)]
enum Source {
    /// The file itself.
    Plain(&'static str),
    /// A file inside a .tgz: its data starts `offset` bytes into the tar stream.
    #[cfg(not(windows))]
    Tgz { url: &'static str, offset: u64 },
    /// A deflated zip member: fetch only its compressed bytes `start..=end`.
    #[cfg(windows)]
    ZipRange {
        url: &'static str,
        start: u64,
        end: u64,
    },
}

/// ONNX Runtime 1.28.0, CPU, from Microsoft's GitHub release (ort 2.0.0-rc.13
/// targets 1.28; it accepts >= 1.17).
#[cfg(not(windows))]
pub(crate) const ORT: ModelFile = ModelFile {
    name: "libonnxruntime.so.1.28.0",
    size: 24_268_848,
    sha256: "1461ef7cc3d9e49982591721683cc3e3a55580aeca9a5254e7aac47b75ee4bab",
    source: Source::Tgz {
        // 9,125,960 bytes; archive SHA-256 a3e1b79d...
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.28.0/onnxruntime-linux-x64-1.28.0.tgz",
        offset: 333_824,
    },
};
#[cfg(windows)]
pub(crate) const ORT: ModelFile = ModelFile {
    name: "onnxruntime.dll",
    size: 15_809_848,
    sha256: "18370c375f07357fa5874344a9d9ac17e6b6fe1eb18b1dd209d79483b4470257",
    source: Source::ZipRange {
        // The zip is 78,796,801 bytes (mostly a .pdb); the dll is 5,659,543 of them.
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.28.0/onnxruntime-win-x64-1.28.0.zip",
        start: 209_109,
        end: 5_868_651,
    },
};

/// oar-ocr's official model release (PaddleOCR PP-OCRv5 exported to ONNX).
macro_rules! oar {
    ($name:literal) => {
        Source::Plain(concat!(
            "https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/",
            $name
        ))
    };
}

pub(super) const DET: ModelFile = ModelFile {
    name: "pp-ocrv5_mobile_det.onnx",
    size: 4_826_518,
    sha256: "1eb7b4f7ab657ebd1c66d5f79bca7497f29768a2e3c15e52daecbba1a8e4a039",
    source: oar!("pp-ocrv5_mobile_det.onnx"),
};

/// A text recognizer: one script's model plus its character list.
pub(crate) struct Recognizer {
    /// What `lang::Language::ocr` names it by.
    pub(crate) id: &'static str,
    /// For the consent ("the Arabic text-recognition model").
    pub(crate) script: &'static str,
    pub(crate) model: ModelFile,
    pub(crate) dict: ModelFile,
}

/// A recognizer of oar-ocr's v0.3.0 release (sizes and SHA-256 of the files
/// as published there).
macro_rules! rec {
    ($id:literal, $script:literal, $model:literal, $msize:literal, $msha:literal,
     $dict:literal, $dsize:literal, $dsha:literal) => {
        Recognizer {
            id: $id,
            script: $script,
            model: ModelFile {
                name: $model,
                size: $msize,
                sha256: $msha,
                source: oar!($model),
            },
            dict: ModelFile {
                name: $dict,
                size: $dsize,
                sha256: $dsha,
                source: oar!($dict),
            },
        }
    };
}

impl Recognizer {
    /// Reads a right-to-left script (its output comes out reversed).
    pub(crate) fn rtl(&self) -> bool {
        self.id == "arabic"
    }

    /// The script it is for (its character list has Latin letters too).
    pub(crate) fn script(&self) -> Script {
        match self.id {
            "arabic" => Script::Arabic,
            "cyrillic" => Script::Cyrillic,
            "cjk" => Script::Cjk,
            _ => Script::Latin,
        }
    }
}

/// English: the default (and "auto"'s first try).
pub(crate) const EN: Recognizer = rec!(
    "en",
    "English",
    "en_pp-ocrv5_mobile_rec.onnx",
    7_876_014,
    "8307465d3c9ef2ba4055c3bd0be55aafe11f518630212b7598b70ccb376028ac",
    "ppocrv5_en_dict.txt",
    1_416,
    "e025a66d31f327ba0c232e03f407ae8d105e1e709e7ccb3f408aa778c24e70d6"
);

/// Every recognizer, `EN` first. Another script (korean, th, el, ...: in the
/// same release) is one more entry plus its id on the languages it reads.
pub(crate) const RECOGNIZERS: [&Recognizer; 5] = [
    &EN,
    // French, German, Spanish, Portuguese, Italian, Polish, Turkish, Vietnamese, ...
    &rec!(
        "latin",
        "Latin",
        "latin_pp-ocrv5_mobile_rec.onnx",
        8_069_614,
        "e3a6bfeea1c8a01d6fccfd480a0bd363fd907f8c65931e228bb2736f5c3e142f",
        "ppocrv5_latin_dict.txt",
        2_616,
        "ccbcc45730b3fbbd9050c5bc74db6a99067141ef1035e3d14889a84a6b9b1aff"
    ),
    // Arabic, Persian, Urdu
    &rec!(
        "arabic",
        "Arabic",
        "arabic_pp-ocrv5_mobile_rec.onnx",
        8_026_538,
        "2768206d9a0ce48eba45b59619184e18161dde8f44115f029920ca17a9dc0384",
        "ppocrv5_arabic_dict.txt",
        2_369,
        "7f92f7dbb9b75a4787a83bfb4f6d14a8ab515525130c9d40a9036f61cf6999e9"
    ),
    // Russian, Ukrainian, ...
    &rec!(
        "cyrillic",
        "Cyrillic",
        "cyrillic_pp-ocrv5_mobile_rec.onnx",
        8_076_390,
        "a18d96d7c8d73d90f2ed056549caa1de3a8e6cb744cccba16cd593ea8cd2d569",
        "ppocrv5_cyrillic_dict.txt",
        2_781,
        "db40aa52ceb112055be80c694afdf655d5d2c4f7873704524cc16a447ca913ba"
    ),
    // PP-OCRv5's main model: Chinese (simplified and traditional), Japanese, English
    &rec!(
        "cjk",
        "Chinese and Japanese",
        "pp-ocrv5_mobile_rec.onnx",
        16_562_373,
        "243a0f06d826761323e9045e9b113ab2c191c3aa50565585e628300b8eda0224",
        "ppocrv5_dict.txt",
        74_012,
        "d1979e9f794c464c0d2e0b70a7fe14dd978e9dc644c0e71f14158cdf8342af1b"
    ),
];

/// The recognizer for a source language: the table's, else `EN` ("auto",
/// unknown codes, languages no recognizer reads yet).
pub(crate) fn recognizer_for(source: &str) -> &'static Recognizer {
    let id = crate::translate::lang::find(source).and_then(|l| l.ocr);
    (RECOGNIZERS.into_iter().find(|r| Some(r.id) == id)).unwrap_or(&EN)
}

/// The non-Latin recognizer for the first of `languages` (codes) that isn't
/// on disk (#169: a screen read as noise is offered the reader for the
/// script its user reads).
pub(crate) fn missing_reader(languages: &[String]) -> Option<&'static Recognizer> {
    (languages.iter())
        .map(|l| recognizer_for(l))
        .find(|r| r.script() != crate::translate::lang::Script::Latin && !installed(r))
}

/// Downloads recognizer `rec` (and what OCR needs besides), `progress` as
/// `ensure`'s.
pub(crate) fn ensure_reader(
    rec: &'static Recognizer,
    progress: impl FnMut(u64, u64),
) -> Result<(), OcrError> {
    ensure(&ocr_files(rec), progress)
}

/// OCR's files for one recognizer (ONNX Runtime first) and where each goes.
pub(crate) fn ocr_files(rec: &'static Recognizer) -> Vec<(PathBuf, &'static ModelFile)> {
    let dir = models_dir();
    [&ORT, &DET, &rec.model, &rec.dict]
        .iter()
        .map(|f| (dir.join(f.name), *f))
        .collect()
}

/// The files of every reader for another script than Latin (Arabic,
/// Cyrillic, Chinese and Japanese) that isn't on disk, ONNX Runtime and the
/// detector too if missing: what a page OCR read as noise may need.
pub(crate) fn other_scripts_files() -> Vec<(PathBuf, &'static ModelFile)> {
    let mut files: Vec<(PathBuf, &'static ModelFile)> = Vec::new();
    for rec in RECOGNIZERS.iter().filter(|r| r.script() != Script::Latin) {
        for f in ocr_files(rec) {
            if !files.iter().any(|(p, _)| *p == f.0) {
                files.push(f);
            }
        }
    }
    files.retain(|f| missing_bytes(std::slice::from_ref(f)) > 0);
    files
}

/// The readers for other scripts than Latin that aren't on disk, by name,
/// for the offer's text ("Cyrillic, Chinese and Japanese").
pub(crate) fn missing_other_scripts() -> String {
    other_scripts_but(installed)
}

/// The other scripts' names but those `have`: "a", "a and b", "a, b and c"
/// (a last name with its own "and" ends the list).
fn other_scripts_but(have: impl Fn(&'static Recognizer) -> bool) -> String {
    let names: Vec<&str> = (RECOGNIZERS.into_iter())
        .filter(|r| r.script() != Script::Latin && !have(r))
        .map(|r| r.script)
        .collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() && !last.contains(" and ") => {
            format!("{} and {last}", rest.join(", "))
        }
        _ => names.join(", "),
    }
}

/// All of `rec`'s files are on disk (sizes only; `ensure` verifies).
pub(crate) fn installed(rec: &'static Recognizer) -> bool {
    missing_bytes(&ocr_files(rec)) == 0
}

impl ModelFile {
    /// A file downloaded as is from `url`.
    pub(crate) const fn plain(
        name: &'static str,
        size: u64,
        sha256: &'static str,
        url: &'static str,
    ) -> ModelFile {
        ModelFile {
            name,
            size,
            sha256,
            source: Source::Plain(url),
        }
    }
}

/// Bytes still to download: files missing or of the wrong size (a cheap
/// check; `ensure` verifies the checksum).
pub(crate) fn missing_bytes(files: &[(PathBuf, &ModelFile)]) -> u64 {
    (files.iter())
        .filter(|(path, f)| fs::metadata(path).map_or(true, |m| m.len() != f.size))
        .map(|(_, f)| f.size)
        .sum()
}

/// Per-user folder for ORT and the models (Linux `~/.local/share/OpenCircle/ocr`,
/// Windows `%LOCALAPPDATA%\OpenCircle\ocr`).
pub fn models_dir() -> PathBuf {
    crate::paths::ocr()
}

/// Makes sure every file OCR needs for the configured source language
/// (`translate.source`) is present and verified, downloading the missing or
/// damaged ones. `progress(done, total)` counts bytes written to disk. Returns
/// the folder for `Engine::load`. Verified files are never fetched again.
pub fn ensure_models(progress: impl FnMut(u64, u64)) -> Result<PathBuf, OcrError> {
    ensure(&ocr_files(current_recognizer()), progress)?;
    Ok(models_dir())
}

/// The recognizer for the configured source language.
pub(crate) fn current_recognizer() -> &'static Recognizer {
    recognizer_for(&crate::config::current().translate.source)
}

/// What the app's OCR (Text, tap a word) needs is on disk: the user agreed
/// to its download once (sizes only).
pub fn ready() -> bool {
    installed(current_recognizer())
}

/// Bytes `ensure_models` would still download for the app's OCR (sizes only).
pub fn download_size() -> u64 {
    missing_bytes(&ocr_files(current_recognizer()))
}

/// Bytes of the app's whole OCR download (every file, present or not): what
/// `ensure_models`' progress counts to (the model download card's size).
pub fn full_size() -> u64 {
    ocr_files(current_recognizer())
        .iter()
        .map(|(_, f)| f.size)
        .sum()
}

#[cfg(test)]
thread_local! {
    /// A test serving the files from its own local fake server may fetch.
    pub(crate) static LOCAL_FAKE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Waits after a failed attempt (seconds): a transient server or network
/// error is retried after each, then the download gives up (about 35 s).
const BACKOFF: [u64; 4] = [1, 3, 9, 20];
/// The longest `Retry-After` honoured (a longer one waits this long).
const MAX_RETRY_AFTER: u64 = 30;

/// A download is waiting to try a file again (the card says "Retrying...").
static RETRYING: AtomicBool = AtomicBool::new(false);

// ponytail: one flag for the process; two downloads at once share it (the
// card may say "Retrying..." for the other one's wait).
pub fn retrying() -> bool {
    RETRYING.load(Ordering::Relaxed)
}

/// `ensure_models` for any list of pinned files (OCR's plus offline
/// translation's: one download, one progress).
pub(crate) fn ensure(
    files: &[(PathBuf, &ModelFile)],
    progress: impl FnMut(u64, u64),
) -> Result<(), OcrError> {
    ensure_until(files, progress, &AtomicBool::new(false))
}

/// `ensure` that stops soon after `stop` is set (`OcrError::Stopped`),
/// keeping the `.part` file it was writing: the next call continues it.
pub(crate) fn ensure_until(
    files: &[(PathBuf, &ModelFile)],
    mut progress: impl FnMut(u64, u64),
    stop: &AtomicBool,
) -> Result<(), OcrError> {
    let total = files.iter().map(|(_, f)| f.size).sum();
    let mut done = 0;
    let mut agent = None;
    for (path, f) in files {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        if !is_verified(path, f)? {
            // Tests never download (a real fetch filled CI's data folder and
            // broke other tests); the manual, ignored model tests opt in.
            #[cfg(test)]
            if !LOCAL_FAKE.get() && std::env::var_os("OPENCIRCLE_TEST_DOWNLOADS").is_none() {
                panic!(
                    "a test tried to download {} (set OPENCIRCLE_TEST_DOWNLOADS=1 for the manual model tests)",
                    path.display()
                );
            }
            let agent = agent.get_or_insert_with(|| {
                // tests' and the VM's fake servers are plain HTTP
                let local = cfg!(test) || redirect("https://x").is_some();
                crate::net::agent(crate::net::Policy {
                    https_only: !local,
                    ..crate::net::MODELS
                })
            });
            let got = fetch_retrying(agent, f, path, stop, |n| progress(done + n, total));
            RETRYING.store(false, Ordering::Relaxed);
            if got.is_err() && !matches!(got, Err(OcrError::Stopped)) {
                let _ = fs::remove_file(path.with_extension("part"));
            }
            got?;
        }
        done += f.size;
        progress(done, total);
    }
    Ok(())
}

/// Why an attempt failed.
enum Fail {
    /// For good: a 4xx, a bad checksum, a disk error. No retry.
    Now(OcrError),
    /// Maybe for a moment (5xx, 429, timeout, reset, short body): try again,
    /// after `after` seconds when the server said so.
    Again {
        why: String,
        server: bool,
        after: Option<u64>,
    },
}

impl From<io::Error> for Fail {
    fn from(e: io::Error) -> Self {
        Fail::Now(e.into())
    }
}

/// `fetch` until it works, with `BACKOFF` between attempts; a `Plain` file
/// continues from its `.part` file. Each failure's detail is logged; the last
/// one gives the user's text (`OcrError::Unreachable`).
fn fetch_retrying(
    agent: &ureq::Agent,
    f: &ModelFile,
    dest: &Path,
    stop: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<(), OcrError> {
    let mut waits = BACKOFF.iter();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(OcrError::Stopped);
        }
        let got = fetch(agent, f, dest, stop, &mut |n| {
            RETRYING.store(false, Ordering::Relaxed);
            progress(n)
        });
        let (why, server, after) = match got {
            Ok(()) => return Ok(()),
            Err(Fail::Now(e)) => return Err(e),
            Err(Fail::Again { why, server, after }) => (why, server, after),
        };
        let Some(&wait) = waits.next() else {
            crate::log!(
                "[opencircle] download of {} failed: {why}; giving up",
                f.name
            );
            return Err(OcrError::Unreachable { server });
        };
        let wait = after.map_or(wait, |s| s.min(MAX_RETRY_AFTER));
        crate::log!(
            "[opencircle] download of {} failed: {why}; retrying in {wait} s",
            f.name
        );
        RETRYING.store(true, Ordering::Relaxed);
        progress(part_len(dest).min(f.size)); // the card redraws, saying so
        pause(wait);
    }
}

fn pause(secs: u64) {
    // tests: milliseconds for seconds
    let unit = if cfg!(test) { 1 } else { 1000 };
    std::thread::sleep(std::time::Duration::from_millis(secs * unit));
}

fn part_len(dest: &Path) -> u64 {
    fs::metadata(dest.with_extension("part")).map_or(0, |m| m.len())
}

/// Test hook `OPENCIRCLE_TEST_MODELS_URL`: model downloads go to that server
/// instead, as `<base>/<host>/<path>` over plain HTTP (the VM's flaky-server
/// test). Never read in a user's copy (`sys::test_hook`).
fn redirect(url: &str) -> Option<String> {
    let base = crate::shared::platform::sys::test_hook("OPENCIRCLE_TEST_MODELS_URL")?;
    let base = base.to_str()?.trim_end_matches('/');
    Some(format!("{base}/{}", url.strip_prefix("https://")?))
}

/// One attempt: the request, then the body into `<dest>.part`.
fn fetch(
    agent: &ureq::Agent,
    f: &ModelFile,
    dest: &Path,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<(), Fail> {
    let tmp = dest.with_extension("part");
    let get = |url: &str, range: Option<String>| {
        let url = redirect(url).unwrap_or_else(|| url.into());
        let mut req = agent.get(&url);
        if let Some(range) = range {
            req = req.header("Range", range);
        }
        let response = req.call().map_err(classify)?;
        let status = response.status().as_u16();
        if status == 429 || status >= 500 {
            let after = response.headers().get("retry-after");
            let after = after.and_then(|v| v.to_str().ok()?.trim().parse().ok());
            let why = format!("http status: {status}");
            return Err(Fail::Again {
                why,
                server: true,
                after,
            });
        }
        if !(200..300).contains(&status) {
            let why = format!("http status: {status}");
            return Err(Fail::Now(OcrError::Download(why)));
        }
        Ok(response)
    };
    match f.source {
        Source::Plain(url) => {
            // continue a cut-off attempt where it stopped
            let have = Some(part_len(dest)).filter(|&n| n < f.size).unwrap_or(0);
            let response = get(url, (have > 0).then(|| format!("bytes={have}-")))?;
            let partial = have > 0 && response.status() == 206;
            if partial && !starts_at(&response, have) {
                let _ = fs::remove_file(&tmp);
                return Err(Fail::Again {
                    why: "the server sent another byte range".into(),
                    server: true,
                    after: None,
                });
            }
            let have = if partial { have } else { 0 }; // a 200 starts over
            let body = response.into_body().into_reader();
            write_part(body, &tmp, dest, f, have, stop, progress)
        }
        #[cfg(not(windows))]
        Source::Tgz { url, offset } => {
            let body = get(url, None)?.into_body();
            let mut tar = flate2::read::GzDecoder::new(body.into_reader());
            io::copy(&mut (&mut tar).take(offset), &mut io::sink()).map_err(cut)?;
            // The rest of the tar follows the member: stop at its end.
            write_part(tar.take(f.size), &tmp, dest, f, 0, stop, progress)
        }
        #[cfg(windows)]
        Source::ZipRange { url, start, end } => {
            // A deflate stream can't resume: the whole range again each time.
            let response = get(url, Some(format!("bytes={start}-{end}")))?;
            if response.status() != 206 {
                let why = "server ignored the byte range".into();
                return Err(Fail::Now(OcrError::Download(why)));
            }
            let body = flate2::read::DeflateDecoder::new(response.into_body().into_reader());
            write_part(body, &tmp, dest, f, 0, stop, progress)
        }
    }
}

/// A 206 answer's `Content-Range` starts at byte `at`.
fn starts_at(response: &ureq::http::Response<ureq::Body>, at: u64) -> bool {
    let range = response.headers().get("content-range");
    let range = range.and_then(|v| v.to_str().ok());
    range.is_some_and(|r| r.starts_with(&format!("bytes {at}-")))
}

/// A request that got no answer: a network hiccup is retried.
fn classify(e: ureq::Error) -> Fail {
    use ureq::Error as E;
    match e {
        E::Timeout(_) | E::Io(_) | E::ConnectionFailed | E::HostNotFound | E::BodyStalled => {
            Fail::Again {
                why: e.to_string(),
                server: false,
                after: None,
            }
        }
        e => Fail::Now(OcrError::Download(e.to_string())),
    }
}

/// The body broke off while reading: retried (from the `.part` file).
fn cut(e: io::Error) -> Fail {
    Fail::Again {
        why: format!("connection lost: {e}"),
        server: false,
        after: None,
    }
}

/// Streams `body` to `<dest>.part` and renames it to `dest` only when the size
/// and checksum match; otherwise deletes the partial file (tests).
#[cfg(test)]
pub(crate) fn write_verified(
    body: impl Read,
    dest: &Path,
    f: &ModelFile,
    mut progress: impl FnMut(u64),
) -> Result<(), OcrError> {
    let tmp = dest.with_extension("part");
    let result = write_part(
        body,
        &tmp,
        dest,
        f,
        0,
        &AtomicBool::new(false),
        &mut progress,
    );
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|e| match e {
        Fail::Now(e) => e,
        Fail::Again { .. } => OcrError::Checksum(f.name),
    })
}

/// Streams `body` onto `tmp` after its first `have` bytes (a resumed
/// download; 0 starts it over) and renames it to `dest` once the size and
/// checksum match. A body that ends short or breaks keeps `tmp` to continue
/// (`Fail::Again`); a wrong one deletes it (`Checksum`).
fn write_part(
    body: impl Read,
    tmp: &Path,
    dest: &Path,
    f: &ModelFile,
    have: u64,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<(), Fail> {
    let mut hash = hmac_sha256::Hash::new();
    let mut buf = vec![0; 64 * 1024];
    let mut out = if have > 0 {
        // the bytes already there count towards the checksum
        let mut old = File::open(tmp)?.take(have);
        loop {
            match old.read(&mut buf)? {
                0 => break,
                n => hash.update(&buf[..n]),
            }
        }
        fs::OpenOptions::new().append(true).open(tmp)?
    } else {
        File::create(tmp)?
    };
    let mut body = body.take(f.size + 1 - have);
    let mut got = have;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Fail::Now(OcrError::Stopped)); // `tmp` stays, to continue
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(cut(e)),
        };
        hash.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        got += n as u64;
        progress(got.min(f.size));
    }
    if got < f.size {
        return Err(Fail::Again {
            why: format!("the body ended at {got} of {} bytes", f.size),
            server: false,
            after: None,
        });
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != f.size || digest != f.sha256 {
        drop(out);
        let _ = fs::remove_file(tmp);
        // A resumed `.part` may be another file version's: once more from byte 0.
        if have > 0 {
            return Err(Fail::Again {
                why: "a resumed download failed its checksum".into(),
                server: false,
                after: None,
            });
        }
        return Err(Fail::Now(OcrError::Checksum(f.name)));
    }
    out.sync_all()?;
    drop(out);
    fs::rename(tmp, dest)?;
    Ok(())
}

fn is_verified(path: &Path, f: &ModelFile) -> io::Result<bool> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() != f.size {
        return Ok(false);
    }
    // Streamed: a model is tens of MB, never held in RAM to be hashed.
    let hash = crate::update::verify::sha256(file)?;
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect::<String>() == f.sha256)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const HELLO: ModelFile = ModelFile {
        name: "hello.bin",
        size: 5,
        sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        source: Source::Plain(""),
    };

    /// Every language's recognizer exists; ids and files are unique and pinned.
    #[test]
    fn recognizer_table_and_choice() {
        use crate::translate::lang::{AUTO, LANGUAGES};
        for (i, r) in RECOGNIZERS.iter().enumerate() {
            assert!(
                RECOGNIZERS[..i]
                    .iter()
                    .all(|o| o.id != r.id && o.model.name != r.model.name)
            );
            for f in [&r.model, &r.dict] {
                assert!(f.size > 0 && f.sha256.len() == 64, "{}", f.name);
            }
        }
        for l in LANGUAGES {
            if let Some(id) = l.ocr {
                assert!(RECOGNIZERS.iter().any(|r| r.id == id), "{}: {id}", l.code);
            }
        }
        let id = |source: &str| recognizer_for(source).id;
        assert_eq!(id("en"), "en");
        assert_eq!(
            (id("fr"), id("pt-BR"), id("vi")),
            ("latin", "latin", "latin")
        );
        assert_eq!(
            (id("ar"), id("fa"), id("ur")),
            ("arabic", "arabic", "arabic")
        );
        assert_eq!((id("ru"), id("uk")), ("cyrillic", "cyrillic"));
        assert_eq!((id("ja"), id("zh"), id("zh-Hant")), ("cjk", "cjk", "cjk"));
        // auto, unknown codes and scripts with no recognizer: English
        assert_eq!((id(AUTO), id("xx"), id("ko")), ("en", "en", "en"));
        // OCR's files: ORT, the detector, then the recognizer's two
        let files = ocr_files(recognizer_for("ar"));
        let names: Vec<&str> = files.iter().map(|(_, f)| f.name).collect();
        assert_eq!(names[..2], [ORT.name, DET.name]);
        assert_eq!(
            names[2..],
            ["arabic_pp-ocrv5_mobile_rec.onnx", "ppocrv5_arabic_dict.txt"]
        );
    }

    #[test]
    fn checksum_rejects_corrupt_download_and_keeps_nothing() {
        let dir = crate::paths::test_dir("ocr-corrupt");
        let dest = dir.join(HELLO.name);
        for bad in [&b"hellO"[..], b"hell", b"hello!"] {
            let r = write_verified(bad, &dest, &HELLO, |_| {});
            assert!(matches!(r, Err(OcrError::Checksum("hello.bin"))), "{bad:?}");
            assert!(!dest.exists() && !dest.with_extension("part").exists());
        }
        write_verified(&b"hello"[..], &dest, &HELLO, |_| {}).unwrap();
        assert!(is_verified(&dest, &HELLO).unwrap());
        fs::write(&dest, b"hellO").unwrap();
        assert!(!is_verified(&dest, &HELLO).unwrap());
        fs::write(&dest, b"hello!").unwrap(); // wrong size
        assert!(!is_verified(&dest, &HELLO).unwrap());
        assert!(!is_verified(&dir.join("missing"), &HELLO).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_offer_names_only_the_missing_readers() {
        assert_eq!(
            other_scripts_but(|_| false),
            "Arabic, Cyrillic, Chinese and Japanese"
        );
        let arabic = |r: &Recognizer| r.id == "arabic";
        assert_eq!(other_scripts_but(arabic), "Cyrillic, Chinese and Japanese");
        assert_eq!(other_scripts_but(|r| r.id == "cjk"), "Arabic and Cyrillic");
        assert_eq!(other_scripts_but(|r| r.id != "cyrillic"), "Cyrillic");
    }

    #[test]
    fn ensure_spans_folders_and_fetches_only_what_is_missing() {
        LOCAL_FAKE.set(true); // its files come from a local fake server
        let dir = crate::paths::test_dir("ocr-ensure");
        let files = [
            (dir.join("a/hello.bin"), &HELLO),
            (dir.join("b/hello.bin"), &HELLO),
        ];
        assert_eq!(missing_bytes(&files), 10);
        for (path, _) in &files {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"hello").unwrap();
        }
        assert_eq!(missing_bytes(&files), 0);
        // both verified: nothing is fetched (HELLO's URL is empty), progress ends at the total
        let mut last = (0, 0);
        ensure(&files, |done, total| last = (done, total)).unwrap();
        assert_eq!(last, (10, 10));
        // a damaged file of the right size is fetched again (and here, can't be)
        fs::write(&files[1].0, b"hellO").unwrap();
        assert_eq!(missing_bytes(&files), 0, "sizes only");
        assert!(matches!(
            ensure(&files, |_, _| {}),
            Err(OcrError::Download(_))
        ));
        assert!(!files[1].0.with_extension("part").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// What the fake server does with one request (given its Range start).
    #[derive(Clone, Copy)]
    pub(crate) enum Answer {
        /// The file (206 from the Range's start when asked for one).
        File,
        /// The status, no body.
        Status(u16),
        /// 200 with the full length announced, then half the bytes and a hang-up.
        Cut,
        /// 200 with a wrong body of the right length.
        Wrong,
    }

    pub(crate) type Seen = std::sync::Arc<std::sync::Mutex<Vec<Option<u64>>>>;

    /// A local server answering request i with `answers[i]` (the last one for
    /// any after), `Connection: close`; returns the file's pin and the Range
    /// start of every request it got.
    pub(crate) fn fake_server(
        name: &str,
        data: &'static [u8],
        answers: Vec<Answer>,
    ) -> (&'static ModelFile, Seen) {
        use std::io::BufRead;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/{name}", listener.local_addr().unwrap());
        let seen = Seen::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for (i, conn) in listener.incoming().enumerate() {
                let Ok(mut conn) = conn else { return };
                let mut from = None;
                let mut reader = io::BufReader::new(conn.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(r) = lower.strip_prefix("range: bytes=") {
                        from = r
                            .split('-')
                            .next()
                            .and_then(|n| n.trim().parse::<usize>().ok());
                    }
                }
                log.lock().unwrap().push(from.map(|n| n as u64));
                let len = data.len();
                let head = |status: &str, extra: &str, n: usize| {
                    format!("HTTP/1.1 {status}\r\nContent-Length: {n}\r\n{extra}Connection: close\r\n\r\n")
                        .into_bytes()
                };
                let out: Vec<u8> = match (answers[i.min(answers.len() - 1)], from) {
                    (Answer::File, Some(at)) => {
                        let range = format!("Content-Range: bytes {at}-{}/{len}\r\n", len - 1);
                        [
                            head("206 Partial Content", &range, len - at),
                            data[at..].to_vec(),
                        ]
                        .concat()
                    }
                    (Answer::File, None) => [head("200 OK", "", len), data.to_vec()].concat(),
                    (Answer::Status(s), _) => head(&format!("{s} Oops"), "", 0),
                    (Answer::Cut, _) => {
                        [head("200 OK", "", len), data[..len / 2].to_vec()].concat()
                    }
                    (Answer::Wrong, _) => [head("200 OK", "", len), vec![b'x'; len]].concat(),
                };
                let _ = conn.write_all(&out);
                let _ = conn.flush();
            }
        });
        let sha: String = (hmac_sha256::Hash::hash(data).iter())
            .map(|b| format!("{b:02x}"))
            .collect();
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let f = ModelFile::plain(leak(name.into()), data.len() as u64, leak(sha), leak(url));
        (Box::leak(Box::new(f)), seen)
    }

    /// 300 kB of a pattern (several reads).
    pub(crate) fn payload() -> &'static [u8] {
        Box::leak((0..300_000u32).map(|i| (i * 7 % 251) as u8).collect())
    }

    /// Downloads `f` into a fresh folder: the result and the folder.
    fn download(f: &'static ModelFile, name: &str) -> (Result<(), OcrError>, PathBuf) {
        LOCAL_FAKE.set(true);
        let dir = crate::paths::test_dir(&format!("ocr-{name}"));
        let got = ensure(&[(dir.join(f.name), f)], |_, _| {});
        (got, dir)
    }

    /// A GitHub 504 (a user's fresh install, 2026-11-09) is retried: the
    /// third request works and the file is verified.
    #[test]
    fn a_server_error_is_retried_until_it_works() {
        let data = payload();
        let answers = vec![Answer::Status(504), Answer::Status(504), Answer::File];
        let (f, seen) = fake_server("retry.bin", data, answers);
        let (got, dir) = download(f, "retry");
        got.unwrap();
        assert_eq!(fs::read(dir.join(f.name)).unwrap(), data);
        assert_eq!(seen.lock().unwrap().len(), 3);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A connection dropped mid-file continues from the `.part` file with a
    /// Range request; the whole is verified.
    #[test]
    fn a_cut_download_resumes_with_a_range() {
        let data = payload();
        let (f, seen) = fake_server("resume.bin", data, vec![Answer::Cut, Answer::File]);
        let (got, dir) = download(f, "resume");
        got.unwrap();
        assert_eq!(fs::read(dir.join(f.name)).unwrap(), data);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen, [None, Some(data.len() as u64 / 2)]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A 404 fails at once (no hammering); a wrong body of the right size
    /// fails the checksum at once and leaves no `.part` file.
    #[test]
    fn a_missing_or_wrong_file_fails_at_once() {
        let data = payload();
        let (f, seen) = fake_server("gone.bin", data, vec![Answer::Status(404)]);
        let (got, dir) = download(f, "gone");
        assert!(matches!(got, Err(OcrError::Download(_))), "{got:?}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        fs::remove_dir_all(dir).unwrap();

        let (f, seen) = fake_server("wrong.bin", data, vec![Answer::Wrong]);
        let (got, dir) = download(f, "wrong");
        assert!(
            matches!(got, Err(OcrError::Checksum("wrong.bin"))),
            "{got:?}"
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert!(!dir.join("wrong.part").exists() && !dir.join(f.name).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// A `.part` left by another version of the file (a crash) is resumed, fails
    /// its checksum, is deleted, and the download restarts from byte 0 once.
    #[test]
    fn a_stale_part_restarts_from_zero_once() {
        let data = payload();
        let (f, seen) = fake_server("stale.bin", data, vec![Answer::File]);
        LOCAL_FAKE.set(true);
        let dir = crate::paths::test_dir("ocr-stale");
        let _ = fs::remove_file(dir.join(f.name));
        fs::write(dir.join("stale.part"), vec![b'x'; data.len() / 2]).unwrap();
        ensure(&[(dir.join(f.name), f)], |_, _| {}).unwrap();
        assert_eq!(fs::read(dir.join(f.name)).unwrap(), data);
        assert_eq!(*seen.lock().unwrap(), [Some(data.len() as u64 / 2), None]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A server that keeps failing: a bounded number of tries, then the
    /// user's plain text (never only "http status: 504"), no `.part` left.
    #[test]
    fn a_server_that_keeps_failing_gives_up_plainly() {
        let data = payload();
        let (f, seen) = fake_server("down.bin", data, vec![Answer::Status(504)]);
        let (got, dir) = download(f, "down");
        let Err(e) = got else { panic!("downloaded") };
        assert!(matches!(e, OcrError::Unreachable { server: true }), "{e:?}");
        let said = e.to_string();
        assert!(said.contains("download server had a problem") && said.contains("minute"));
        assert!(!said.contains("504"), "{said}");
        assert_eq!(seen.lock().unwrap().len(), BACKOFF.len() + 1);
        assert!(!dir.join("down.part").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
