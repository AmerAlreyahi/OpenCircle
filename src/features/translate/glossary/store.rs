//! Glossary files: one TOML per game in `<data dir>/glossaries/<slug>.toml`
//! (readable, commentable, shareable as one file). Import is validated at the
//! trust boundary; export is the same file. Nothing is sent anywhere.
//!
//! ```toml
//! name = "Ashen Tides"
//! windows = ["Ashen Tides"]     # optional: titles that pick it by themselves
//! [[term]]
//! text = "Vyrnhold"             # no language keys = kept as is
//! [[term]]
//! text = "Emberglass"
//! ar = "زجاج الجمر"             # one key per target language code
//! case = true                   # optional: match case
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Glossary, Term};
use crate::features::translate::lang;

/// Import limits (owner-approved spec): a bigger file is refused.
pub const MAX_BYTES: usize = 1 << 20;
pub const MAX_TERMS: usize = 5_000;
pub const MAX_TERM_CHARS: usize = 200;
/// "Use for windows named" entries per glossary.
pub const MAX_WINDOWS: usize = 100;

/// The folder of the glossary files.
pub fn dir() -> PathBuf {
    crate::shared::paths::app_data().join("glossaries")
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    TooBig,
    TooMany,
    TermTooLong,
    NoName,
    Bad(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooBig => write!(f, "The file is bigger than 1 MB."),
            Error::TooMany => write!(f, "A glossary holds at most {MAX_TERMS} terms."),
            Error::TermTooLong => write!(f, "A term is longer than {MAX_TERM_CHARS} characters."),
            Error::NoName => write!(f, "The glossary has no name."),
            Error::Bad(e) => write!(f, "Not a glossary file: {e}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    windows: Vec<String>,
    #[serde(default, rename = "term", skip_serializing_if = "Vec::is_empty")]
    terms: Vec<Entry>,
    /// Deleted terms (tombstones); old files have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    removed: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Entry {
    #[serde(default)]
    text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    case: bool,
    /// The shared translations under the player's own (merge); old files have none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    base: BTreeMap<String, String>,
    /// Language codes -> translations; anything else is ignored on import.
    #[serde(flatten)]
    rest: BTreeMap<String, toml::Value>,
}

/// File name stem of a glossary name: lower-case letters and digits, other
/// runs as one "-" ("Ashen Tides" -> "ashen-tides"); "glossary" when empty.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        match c.is_alphanumeric() {
            true => out.extend(c.to_lowercase()),
            false if !out.is_empty() && !out.ends_with('-') => out.push('-'),
            false => {}
        }
    }
    let out = out.trim_end_matches('-');
    match out.is_empty() {
        true => "glossary".into(),
        false => out.into(),
    }
}

/// Reads and validates a glossary file's text (unknown keys and unknown
/// language codes are ignored; limits as above).
pub fn parse(text: &str) -> Result<Glossary, Error> {
    if text.len() > MAX_BYTES {
        return Err(Error::TooBig);
    }
    let f: File = toml::from_str(text).map_err(|e| Error::Bad(e.message().to_string()))?;
    if f.terms.len() > MAX_TERMS {
        return Err(Error::TooMany);
    }
    let name = f.name.trim().to_string();
    if name.is_empty() {
        return Err(Error::NoName);
    }
    let mut terms = Vec::new();
    for e in f.terms {
        let text = e.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let translations: BTreeMap<String, String> = (e.rest.into_iter())
            .filter_map(|(k, v)| Some((lang::find(&k)?.code.to_string(), v.as_str()?.to_string())))
            .filter(|(_, v)| !v.trim().is_empty())
            .collect();
        if text.chars().count() > MAX_TERM_CHARS
            || translations
                .values()
                .any(|v| v.chars().count() > MAX_TERM_CHARS)
        {
            return Err(Error::TermTooLong);
        }
        let base = (e.base.into_iter())
            .filter_map(|(k, v)| Some((lang::find(&k)?.code.to_string(), v)))
            .filter(|(_, v)| !v.trim().is_empty() && v.chars().count() <= MAX_TERM_CHARS)
            .collect();
        terms.push(Term {
            text,
            translations,
            case: e.case,
            base,
        });
    }
    Ok(Glossary {
        name,
        windows: (f.windows.into_iter())
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty())
            .collect(),
        removed: (f.removed.into_iter())
            .map(|r| Term::key_of(&r))
            .filter(|r| !r.is_empty())
            .take(MAX_TERMS)
            .collect(),
        terms,
    })
}

/// The file's text (what Export saves).
pub fn render(g: &Glossary) -> String {
    let f = File {
        name: g.name.clone(),
        windows: g.windows.clone(),
        terms: (g.terms.iter())
            .map(|t| Entry {
                text: t.text.clone(),
                case: t.case,
                base: t.base.clone(),
                rest: (t.translations.iter())
                    .map(|(k, v)| (k.clone(), toml::Value::String(v.clone())))
                    .collect(),
            })
            .collect(),
        // (a term typed again is no longer deleted)
        removed: (g.removed.iter())
            .filter(|r| !g.terms.iter().any(|t| t.key() == **r))
            .cloned()
            .collect(),
    };
    toml::to_string(&f).unwrap_or_default()
}

fn path(dir: &Path, slug: &str) -> PathBuf {
    dir.join(format!("{slug}.toml"))
}

/// Every readable glossary in `dir` as (slug, glossary), by name; a broken
/// file is skipped.
pub fn load_all(dir: &Path) -> Vec<(String, Glossary)> {
    let mut all: Vec<(String, Glossary)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "toml").then_some(())?;
            let g = parse(&std::fs::read_to_string(&p).ok()?).ok()?;
            Some((p.file_stem()?.to_string_lossy().into_owned(), g))
        })
        .collect();
    all.sort_by_key(|(_, g)| g.name.to_lowercase());
    all
}

/// The glossary `slug` names, if its file reads.
pub fn load(dir: &Path, slug: &str) -> Option<Glossary> {
    if slug.is_empty() || slug.contains(['/', '\\']) || slug.starts_with('.') {
        return None;
    }
    parse(&std::fs::read_to_string(path(dir, slug)).ok()?).ok()
}

/// `g` within the limits `parse` enforces on reading (else the file would be
/// refused on the next load and the glossary lost).
pub fn check(g: &Glossary) -> Result<(), Error> {
    if g.terms.len() > MAX_TERMS || g.windows.len() > MAX_WINDOWS || g.removed.len() > MAX_TERMS {
        return Err(Error::TooMany);
    }
    match render(g).len() > MAX_BYTES {
        true => Err(Error::TooBig),
        false => Ok(()),
    }
}

/// Writes `g` as `slug`: refused, the file untouched, when it is over the
/// limits; else a unique temp file, then a rename (a crash never leaves a
/// half file or loses the old one).
pub fn save(dir: &Path, slug: &str, g: &Glossary) -> Result<(), Error> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    check(g)?;
    let io = |e: std::io::Error| Error::Io(e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let n = N.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{slug}.{}.{n}.tmp", std::process::id()));
    std::fs::write(&tmp, render(g)).map_err(io)?;
    std::fs::rename(&tmp, path(dir, slug)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

pub fn delete(dir: &Path, slug: &str) {
    if !slug.contains(['/', '\\']) {
        let _ = std::fs::remove_file(path(dir, slug));
    }
}

/// A slug for `name` that no file in `dir` has yet ("ashen-tides-2").
pub fn free_slug(dir: &Path, name: &str) -> String {
    let base = slug(name);
    let mut s = base.clone();
    let mut n = 1;
    while path(dir, &s).exists() {
        n += 1;
        s = format!("{base}-{n}");
    }
    s
}

/// The glossary whose "windows" list names the focused window's title
/// (substring, case-insensitive). `None` title (Wayland) or no match = keep
/// the user's pick.
pub fn for_window<'a>(all: &'a [(String, Glossary)], title: Option<&str>) -> Option<&'a str> {
    let title = title?.to_lowercase();
    all.iter()
        .find(|(_, g)| g.windows.iter().any(|w| title.contains(&w.to_lowercase())))
        .map(|(s, _)| s.as_str())
}

/// The glossary in use now (`screen::active`: none off a game screen), read at
/// most every 2 s.
pub fn current() -> Option<Glossary> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    type Cached = Option<(Instant, String, Option<Glossary>)>;
    static CACHE: Mutex<Cached> = Mutex::new(None);
    let chosen = crate::shared::config::current().translate.glossary;
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    // (the state the answer depends on, apart from the files)
    let key = format!("{chosen}\u{1}{}", super::screen::key());
    if let Some((at, k, g)) = c.as_ref()
        && *k == key
        && at.elapsed() < Duration::from_secs(2)
    {
        return g.clone();
    }
    let all = load_all(&dir());
    let slug = super::screen::active(&all, &chosen).map(str::to_string);
    let g = slug.and_then(|s| all.into_iter().find(|(x, _)| *x == s).map(|(_, g)| g));
    *c = Some((Instant::now(), key, g.clone()));
    g
}
