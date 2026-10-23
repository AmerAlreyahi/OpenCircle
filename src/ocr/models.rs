//! The pinned files OCR needs (ONNX Runtime + models): fetched over HTTPS on
//! first use, checked against a hard-coded size and SHA-256, written via temp
//! file + rename, then reused. Nothing is unpacked by name: archive members are
//! cut out at pinned offsets, so an archive can't place files anywhere.
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::OcrError;

pub(super) struct ModelFile {
    /// File name on disk.
    pub(super) name: &'static str,
    size: u64,
    sha256: &'static str,
    source: Source,
}

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
pub(super) const ORT: ModelFile = ModelFile {
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
pub(super) const ORT: ModelFile = ModelFile {
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
pub(super) struct Recognizer {
    pub(super) model: ModelFile,
    pub(super) dict: ModelFile,
}

/// The recognizer in use. Another script (latin, arabic, ...) is one more
/// const of this shape; names, sizes and SHA-256s are in oar-ocr's registry.
pub(super) const RECOGNIZER: Recognizer = EN;

const EN: Recognizer = Recognizer {
    model: ModelFile {
        name: "en_pp-ocrv5_mobile_rec.onnx",
        size: 7_876_014,
        sha256: "8307465d3c9ef2ba4055c3bd0be55aafe11f518630212b7598b70ccb376028ac",
        source: oar!("en_pp-ocrv5_mobile_rec.onnx"),
    },
    dict: ModelFile {
        name: "ppocrv5_en_dict.txt",
        size: 1_416,
        sha256: "e025a66d31f327ba0c232e03f407ae8d105e1e709e7ccb3f408aa778c24e70d6",
        source: oar!("ppocrv5_en_dict.txt"),
    },
};

const FILES: [&ModelFile; 4] = [&ORT, &DET, &RECOGNIZER.model, &RECOGNIZER.dict];

/// Per-user folder for ORT and the models (Linux `~/.local/share/OpenCircle/ocr`,
/// Windows `%LOCALAPPDATA%\OpenCircle\ocr`).
pub fn models_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenCircle")
        .join("ocr")
}

/// Makes sure every file is present and verified, downloading the missing or
/// damaged ones. `progress(done, total)` counts bytes written to disk. Returns
/// the folder for `Engine::load`. Verified files are never fetched again.
pub fn ensure_models(mut progress: impl FnMut(u64, u64)) -> Result<PathBuf, OcrError> {
    let dir = models_dir();
    fs::create_dir_all(&dir)?;
    let total = FILES.iter().map(|f| f.size).sum();
    let mut done = 0;
    let mut agent = None;
    for f in FILES {
        let path = dir.join(f.name);
        if !is_verified(&path, f)? {
            let agent = agent.get_or_insert_with(|| {
                ureq::Agent::config_builder()
                    .https_only(true)
                    .timeout_connect(Some(Duration::from_secs(30)))
                    .timeout_global(Some(Duration::from_secs(600)))
                    .build()
                    .new_agent()
            });
            fetch(agent, f, &path, |n| progress(done + n, total))?;
        }
        done += f.size;
        progress(done, total);
    }
    Ok(dir)
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
}
