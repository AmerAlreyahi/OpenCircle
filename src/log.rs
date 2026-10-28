//! The local log (#57): everything `log!` prints also goes to
//! `<data_local>/OpenCircle/logs/opencircle.log`, rotated by size (3 files). A
//! release exe on Windows has no console, so this is where its messages live.
//! A panic writes `crash-<time>.txt` beside it (5 kept). Nothing is ever sent
//! anywhere; every line passes `redact` before it is printed or written.

use std::collections::VecDeque;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Like `eprintln!`, and the line goes to the log file too.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(format_args!($($arg)*)) };
}

const FILE: &str = "opencircle.log";
/// Rotate at this size (override for tests and the VM: `OPENCIRCLE_LOG_MAX_BYTES`).
const MAX_BYTES: u64 = 1024 * 1024;
const FILES: usize = 3;
/// Lines kept in memory for crash files and diagnostics.
const TAIL: usize = 200;
const CRASH_FILES: usize = 5;

struct Log {
    dir: Option<PathBuf>,
    max: u64,
    file: Option<File>,
    size: u64,
    tail: VecDeque<String>,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

pub fn dir() -> PathBuf {
    crate::paths::logs()
}

impl Log {
    fn new(dir: Option<PathBuf>, max: u64) -> Log {
        Log {
            dir,
            max,
            file: None,
            size: 0,
            tail: VecDeque::new(),
        }
    }

    fn add(&mut self, line: &str) {
        if self.tail.len() == TAIL {
            self.tail.pop_front();
        }
        self.tail.push_back(line.to_string());
        let Some(dir) = self.dir.clone() else { return };
        let bytes = line.len() as u64 + 1;
        if self.file.is_some() && self.size + bytes > self.max {
            self.file = None;
            rotate(&dir);
        }
        if self.file.is_none() {
            let _ = fs::create_dir_all(&dir);
            let open = OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(FILE));
            let Ok(file) = open else { return };
            self.size = file.metadata().map_or(0, |m| m.len());
            self.file = Some(file);
        }
        if let Some(file) = &mut self.file {
            // One write per line (append mode): no buffer to lose in a crash.
            if file.write_all(format!("{line}\n").as_bytes()).is_ok() {
                self.size += bytes;
            }
        }
    }
}

/// `opencircle.log` -> `.1` -> `.2`; the oldest goes.
fn rotate(dir: &Path) {
    let name = |i: usize| match i {
        0 => dir.join(FILE),
        i => dir.join(format!("{FILE}.{i}")),
    };
    let _ = fs::remove_file(name(FILES - 1));
    for i in (0..FILES - 1).rev() {
        let _ = fs::rename(name(i), name(i + 1));
    }
}

fn with_log<T>(f: impl FnOnce(&mut Log) -> T) -> T {
    let mut slot = LOG.lock().unwrap_or_else(|e| e.into_inner());
    f(slot.get_or_insert_with(|| {
        let max = std::env::var("OPENCIRCLE_LOG_MAX_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(MAX_BYTES);
        // Tests never write the machine's log.
        Log::new((!cfg!(test)).then(dir), max)
    }))
}

/// What `log!` calls: stderr (if there is one) and the log file.
pub fn write(args: fmt::Arguments) {
    let line = redact(&args.to_string());
    eprintln!("{line}");
    let line = format!("{} {line}", clock(now()));
    with_log(|log| log.add(&line));
}

/// The last `n` log lines of this run (already redacted).
pub fn tail(n: usize) -> Vec<String> {
    with_log(|log| log.tail.iter().rev().take(n).rev().cloned().collect())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn clock(secs: u64) -> String {
    let s = secs % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// `20261027-153012` (UTC), for file names.
fn stamp(secs: u64) -> String {
    // days -> civil date (Howard Hinnant's algorithm)
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}{month:02}{day:02}-{}", clock(secs).replace(':', ""))
}

/// Panics write a crash file into `dir`, then the previous hook runs (the
/// message on stderr, the tray icon cleanup, the default behaviour).
pub fn install_panic_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // The log lock may be held by the panicking thread itself: never wait for it.
        let lines = match LOG.try_lock() {
            Ok(slot) => slot
                .as_ref()
                .map_or(vec![], |l| l.tail.iter().cloned().collect()),
            Err(_) => vec![],
        };
        let thread = std::thread::current();
        let report = format!(
            "OpenCircle {}\n{}\nthread: {}\n\n{info}\n\nbacktrace:\n{}\n\nlast log lines:\n{}\n",
            env!("CARGO_PKG_VERSION"),
            os(),
            thread.name().unwrap_or("?"),
            std::backtrace::Backtrace::force_capture(),
            lines.join("\n"),
        );
        let _ = write_crash(&dir, &redact(&report));
        previous(info);
    }));
}

/// `crash-<time>.txt` in `dir`; keeps the newest `CRASH_FILES`.
fn write_crash(dir: &Path, report: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let base = format!("crash-{}", stamp(now()));
    let mut n = 0;
    let (path, mut file) = loop {
        let path = dir.join(match n {
            0 => format!("{base}.txt"),
            n => format!("{base}-{n}.txt"),
        });
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    file.write_all(report.as_bytes())?;
    let mut crashes: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    crashes.sort(); // the stamp sorts by time
    for old in crashes.iter().rev().skip(CRASH_FILES) {
        let _ = fs::remove_file(old);
    }
    Ok(path)
}

/// "Windows 10.0.19045" / the distro name: one line, for crash files and diagnostics.
pub fn os() -> String {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("cmd")
            .args(["/c", "ver"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        format!("{} ({})", out.unwrap_or_default(), std::env::consts::ARCH)
    }
    #[cfg(not(windows))]
    {
        let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
        let name = (release.lines())
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map_or("Linux", |v| v.trim_matches('"'));
        format!("{name} ({})", std::env::consts::ARCH)
    }
}

// ---- redaction ----

/// Hides what must never reach a log, a crash file or the clipboard: API keys
/// and tokens (`sk-...`, `Bearer ...`, `key=...`, JWTs, long random strings)
/// and the user's home folder (`~`). One function for everything we write.
pub fn redact(text: &str) -> String {
    let text = redact_pairs(text);
    let text = redact_tokens(&text);
    redact_home(&text)
}

const GONE: &str = "[redacted]";
/// `name=value`, `name: value`, `"name": "value"`; also `Bearer value`.
const SECRET_NAMES: [&str; 11] = [
    "key",
    "api_key",
    "apikey",
    "api-key",
    "token",
    "secret",
    "password",
    "passwd",
    "authorization",
    "bearer",
    "credential",
];

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn redact_pairs(text: &str) -> String {
    let lower = text.to_ascii_lowercase(); // same byte offsets as `text`
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &lower[i..];
        let at_start = !text[..i].chars().next_back().is_some_and(is_name_char)
            || text[..i].ends_with(['_', '-']); // `x-api-key`, `openai_key`
        let hit = at_start
            .then(|| SECRET_NAMES.iter().find(|n| rest.starts_with(**n)))
            .flatten();
        if let Some(name) = hit {
            let after = &text[i + name.len()..];
            let sep = after.trim_start_matches(['"', '\'']);
            let sep_skipped = after.len() - sep.len();
            let (is_bearer, ok) = if *name == "bearer" {
                (true, sep.starts_with([' ', '\t']))
            } else {
                (false, sep.trim_start().starts_with(['=', ':']))
            };
            if ok {
                let mut j = i + name.len() + sep_skipped;
                if !is_bearer {
                    j += sep.len() - sep.trim_start().len() + 1; // spaces, then = or :
                }
                let body = &text[j..];
                let value = body.trim_start_matches([' ', '\t', '"', '\'']);
                let skipped = body.len() - value.len();
                let len = value
                    .find(|c: char| c.is_whitespace() || "&\"',;})".contains(c))
                    .unwrap_or(value.len());
                if len > 0 {
                    out.push_str(&text[i..j + skipped]);
                    out.push_str(GONE);
                    i = j + skipped + len;
                    continue;
                }
            }
        }
        let c = text[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Runs of `[A-Za-z0-9_-]` that look like a key: a known prefix, or 32+ mixed
/// letters and digits (a hash or an id is hidden too: not worth the risk).
fn redact_tokens(text: &str) -> String {
    const PREFIXES: [&str; 9] = [
        "sk-",
        "sk_",
        "AIza",
        "ghp_",
        "gho_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "eyJ",
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let start = rest.find(is_name_char).unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let jwt = rest.starts_with("eyJ");
        let end = rest
            .find(|c: char| !(is_name_char(c) || (jwt && c == '.')))
            .unwrap_or(rest.len());
        let word = &rest[..end];
        let key_like = (PREFIXES.iter().any(|p| word.starts_with(p)) && word.len() >= 12)
            || (word.len() >= 32
                && word.bytes().any(|b| b.is_ascii_digit())
                && word.bytes().any(|b| b.is_ascii_alphabetic()));
        out.push_str(if key_like { GONE } else { word });
        rest = &rest[end..];
    }
    out
}

/// The home folder -> `~`. Besides the real one: any `C:\Users\<name>`, `/Users/<name>`,
/// `/home/<name>`, so a path from another account or a copied report is hidden too.
fn redact_home(text: &str) -> String {
    let mut text = text.to_string();
    if let Some(home) = dirs::home_dir().and_then(|h| h.to_str().map(str::to_string))
        && home.len() > 3
    {
        text = text
            .replace(&home, "~")
            .replace(&home.replace('\\', "/"), "~");
    }
    for marker in ["\\Users\\", "/Users/", "/home/"] {
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(at) = rest.find(marker) {
            let mut head = &rest[..at];
            if marker.starts_with('\\') {
                // the drive letter in front: `C:`
                let b = head.as_bytes();
                if b.len() >= 2 && b[b.len() - 1] == b':' && b[b.len() - 2].is_ascii_alphabetic() {
                    head = &head[..head.len() - 2];
                }
            }
            out.push_str(head);
            out.push('~');
            let user = &rest[at + marker.len()..];
            let end = user
                .find(|c: char| matches!(c, '\\' | '/' | '"' | '\'') || c.is_whitespace())
                .unwrap_or(user.len());
            rest = &user[end..];
        }
        out.push_str(rest);
        text = out;
    }
    text
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oc-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    /// A fake key: `prefix` and 36 plain characters (never a real one).
    pub(crate) fn fake(prefix: &str) -> String {
        format!("{prefix}{}", "FakeTest0x".repeat(4).get(..36).unwrap())
    }

    #[test]
    fn redacts_realistic_secrets() {
        // built at run time: no key-shaped text in the source (secret scanners)
        let secrets = [
            fake("sk-proj-"),
            fake("sk-ant-api03-"),
            fake("AIza"),
            fake("ghp_"),
            format!("{}.{}.{}", fake("eyJ"), fake("eyJ"), fake("")),
            "3f9a8c1e5b7d4a6f8e2c0b9d7a5f3e1c".to_string(),
        ];
        for s in &secrets {
            let s = s.as_str();
            for line in [
                format!("request failed with {s} for the user"),
                format!("Authorization: Bearer {s}"),
                format!("GET /v1?key={s}&alt=json"),
                format!("{{\"api_key\": \"{s}\"}}"),
                format!("x-api-key: {s}"),
            ] {
                let out = redact(&line);
                assert!(!out.contains(s), "{line} -> {out}");
                assert!(out.contains(GONE), "{line} -> {out}");
            }
        }
        // the sentence around it survives; plain words are untouched
        assert_eq!(
            redact("update check failed: timed out (key=abc123 retry=3)"),
            "update check failed: timed out (key=[redacted] retry=3)"
        );
        assert_eq!(
            redact("the monkey token-less state"),
            "the monkey token-less state"
        );
        assert_eq!(
            redact("[opencircle] Windows OCR reads en-US at 1.25x"),
            "[opencircle] Windows OCR reads en-US at 1.25x"
        );
    }

    #[test]
    fn redacts_the_home_folder() {
        assert_eq!(
            redact(r"cannot read C:\Users\amer.z\AppData\Local\OpenCircle\config.toml"),
            r"cannot read ~\AppData\Local\OpenCircle\config.toml"
        );
        assert_eq!(
            redact("saved /home/amer/Pictures/a.png and /Users/bob/x"),
            "saved ~/Pictures/a.png and ~/x"
        );
        if let Some(home) = dirs::home_dir().and_then(|h| h.to_str().map(str::to_string))
            && home.len() > 3
        {
            let out = redact(&format!("{home}/data/x.txt"));
            assert!(!out.contains(&home), "{out}");
        }
    }

    #[test]
    fn the_log_rotates_by_size_and_keeps_three_files() {
        let dir = temp("rotate");
        let mut log = Log::new(Some(dir.clone()), 200);
        for i in 0..40 {
            log.add(&format!("line number {i:03} of the rotation test"));
        }
        let names = |i: usize| match i {
            0 => dir.join(FILE),
            i => dir.join(format!("{FILE}.{i}")),
        };
        for i in 0..3 {
            let len = fs::metadata(names(i)).unwrap().len();
            assert!(len > 0 && len <= 200, "file {i}: {len} bytes");
        }
        assert!(!names(3).exists(), "at most 3 files");
        let newest = fs::read_to_string(names(0)).unwrap();
        assert!(newest.contains("039"), "{newest}");
        assert!(!fs::read_to_string(names(2)).unwrap().contains("000"));
        assert_eq!(log.tail.len(), 40);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_panic_writes_a_redacted_crash_file_and_keeps_five() {
        let dir = temp("crash");
        install_panic_hook(dir.clone());
        crate::log!("before the crash: key=sk-live-0123456789abcdefXYZ");
        for _ in 0..7 {
            let t = std::thread::Builder::new()
                .name("crasher".into())
                .spawn(|| {
                    panic!("boom with token={}", fake("ghp_"));
                });
            assert!(t.unwrap().join().is_err());
        }
        let _ = std::panic::take_hook(); // back to the default
        let mut files: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        assert_eq!(files.len(), 5, "{files:?}");
        let report = fs::read_to_string(files.last().unwrap()).unwrap();
        assert!(report.contains(env!("CARGO_PKG_VERSION")), "{report}");
        assert!(report.contains("boom with"), "{report}");
        assert!(report.contains("backtrace:"), "{report}");
        assert!(report.contains("before the crash"), "{report}");
        assert!(
            !report.contains(&fake("ghp_")) && !report.contains("sk-live-0123"),
            "{report}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(1_793_114_730), "20261027-152530");
    }
}
