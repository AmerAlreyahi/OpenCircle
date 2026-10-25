//! The pinned files OCR needs (ONNX Runtime + models): fetched over HTTPS on
//! first use, checked against a hard-coded size and SHA-256, written via temp
//! file + rename, then reused. Nothing is unpacked by name: archive members are
//! cut out at pinned offsets, so an archive can't place files anywhere.
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// OCR's files for one recognizer (ONNX Runtime first) and where each goes.
pub(crate) fn ocr_files(rec: &'static Recognizer) -> Vec<(PathBuf, &'static ModelFile)> {
    let dir = models_dir();
    [&ORT, &DET, &rec.model, &rec.dict]
        .iter()
        .map(|f| (dir.join(f.name), *f))
        .collect()
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
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenCircle")
        .join("ocr")
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

/// `ensure_models` for any list of pinned files (OCR's plus offline
/// translation's: one download, one progress).
pub(crate) fn ensure(
    files: &[(PathBuf, &ModelFile)],
    mut progress: impl FnMut(u64, u64),
) -> Result<(), OcrError> {
    let total = files.iter().map(|(_, f)| f.size).sum();
    let mut done = 0;
    let mut agent = None;
    for (path, f) in files {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        if !is_verified(path, f)? {
            let agent = agent.get_or_insert_with(|| {
                ureq::Agent::config_builder()
                    .https_only(true)
                    .timeout_connect(Some(Duration::from_secs(30)))
                    .timeout_global(Some(Duration::from_secs(600)))
                    .build()
                    .new_agent()
            });
            fetch(agent, f, path, |n| progress(done + n, total))?;
        }
        done += f.size;
        progress(done, total);
    }
    Ok(())
}

fn fetch(
    agent: &ureq::Agent,
    f: &ModelFile,
    dest: &Path,
    progress: impl FnMut(u64),
) -> Result<(), OcrError> {
    let net = |e: ureq::Error| OcrError::Download(e.to_string());
    match f.source {
        Source::Plain(url) => {
            let body = agent.get(url).call().map_err(net)?.into_body();
            write_verified(body.into_reader(), dest, f, progress)
        }
        #[cfg(not(windows))]
        Source::Tgz { url, offset } => {
            let body = agent.get(url).call().map_err(net)?.into_body();
            let mut tar = flate2::read::GzDecoder::new(body.into_reader());
            io::copy(&mut (&mut tar).take(offset), &mut io::sink())
                .map_err(|e| OcrError::Download(e.to_string()))?;
            // The rest of the tar follows the member: stop at its end.
            write_verified(tar.take(f.size), dest, f, progress)
        }
        #[cfg(windows)]
        Source::ZipRange { url, start, end } => {
            let response = agent
                .get(url)
                .header("Range", format!("bytes={start}-{end}"))
                .call()
                .map_err(net)?;
            if response.status() != 206 {
                return Err(OcrError::Download("server ignored the byte range".into()));
            }
            let body = response.into_body().into_reader();
            write_verified(flate2::read::DeflateDecoder::new(body), dest, f, progress)
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hmac_sha256::Hash::hash(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn is_verified(path: &Path, f: &ModelFile) -> io::Result<bool> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes.len() as u64 == f.size && sha256_hex(&bytes) == f.sha256),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Streams `body` to `<dest>.part` and renames it to `dest` only when the size
/// and checksum match; otherwise deletes the partial file.
fn write_verified(
    body: impl Read,
    dest: &Path,
    f: &ModelFile,
    mut progress: impl FnMut(u64),
) -> Result<(), OcrError> {
    let tmp = dest.with_extension("part");
    let result = (|| {
        let mut out = File::create(&tmp)?;
        let mut body = body.take(f.size + 1);
        let mut hash = hmac_sha256::Hash::new();
        let mut buf = vec![0; 64 * 1024];
        let mut got = 0u64;
        loop {
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(OcrError::Download(e.to_string())),
            };
            hash.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            got += n as u64;
            progress(got.min(f.size));
        }
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != f.size || digest != f.sha256 {
            return Err(OcrError::Checksum(f.name));
        }
        out.sync_all()?;
        drop(out);
        fs::rename(&tmp, dest)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: ModelFile = ModelFile {
        name: "hello.bin",
        size: 5,
        sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        source: Source::Plain(""),
    };

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-ocr-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

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
        let dir = temp_dir("corrupt");
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
        assert!(!is_verified(&dir.join("missing"), &HELLO).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ensure_spans_folders_and_fetches_only_what_is_missing() {
        let dir = temp_dir("ensure");
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
}
