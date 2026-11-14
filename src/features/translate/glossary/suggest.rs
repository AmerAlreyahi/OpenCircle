//! Name suggestions (planning #180, owner 2026-11-14): a capitalised word
//! that shows on three different translated screens of one game becomes a
//! suggestion on the pill's glossary button: Keep as is, Translate as…, or
//! Reject (never suggested again). Local and quiet: game screens only, no
//! pop-up, nothing leaves the PC.
//!
//! The counts live beside the glossary (`<slug>.names.json`, at most
//! [`MAX_CANDIDATES`] words, each with at most [`NEEDED`] screen ids: no
//! more is needed); rejections live IN the glossary file (`rejected`).

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{Glossary, Term, menu, screen, store};
use crate::features::translate::lang;

/// Different screens a word must show on.
pub const NEEDED: usize = 3;
/// Words counted per game, and rejections kept per glossary.
pub const MAX_CANDIDATES: usize = 500;
pub const MAX_REJECTED: usize = 500;

#[derive(Serialize, Deserialize, Default)]
struct Book {
    #[serde(default)]
    seen: BTreeMap<String, Seen>,
}

#[derive(Serialize, Deserialize, Default)]
struct Seen {
    /// As written the first time.
    text: String,
    /// Ids of up to `NEEDED` different screens.
    screens: Vec<u64>,
    /// Also seen in lower case: a common word, never a name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    common: bool,
}

/// The counts file of glossary `slug`.
fn path(dir: &Path, slug: &str) -> PathBuf {
    dir.join(format!("{slug}.names.json"))
}

/// A deleted glossary takes its counts along.
pub fn forget(dir: &Path, slug: &str) {
    let _ = std::fs::remove_file(path(dir, slug));
}

fn read(dir: &Path, slug: &str) -> Book {
    (std::fs::read_to_string(path(dir, slug)).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write(dir: &Path, slug: &str, book: &Book) {
    if let Ok(text) = serde_json::to_string(book) {
        let tmp = dir.join(format!("{slug}.names.tmp"));
        if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path(dir, slug)).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// Which of `words` may be names: capitalised, two letters or more, and not
/// the start of a sentence (the first word, or one after . ! ? or :).
pub fn names(words: &[&str]) -> Vec<bool> {
    (0..words.len())
        .map(|i| {
            let starts =
                i == 0 || words[i - 1].ends_with(['.', '!', '?', ':', '…', '。', '！', '？']);
            !starts && capitalised(words[i])
        })
        .collect()
}

fn capitalised(word: &str) -> bool {
    word.starts_with(char::is_uppercase) && word.chars().filter(|c| c.is_alphabetic()).count() > 1
}

/// The words of `w` as a name would be written: letters (an apostrophe or
/// hyphen inside), no CAPS, no digits.
fn name_of(w: &str) -> Option<&str> {
    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
    let ok = w
        .chars()
        .all(|c| c.is_alphabetic() || c == '\'' || c == '’' || c == '-')
        && capitalised(w)
        && w.chars().any(char::is_lowercase);
    ok.then_some(w)
}

/// A line of running text (four words or more), not a label or a button.
fn sentence(line: &str) -> bool {
    line.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphabetic))
        .count()
        >= 4
}

/// The names the lines of one screen hold (each once): a capitalised word
/// inside a sentence, or a speaker label that heads a sentence ("Kaelith:
/// we go", or a name plate of one word on the line before one). Menu
/// buttons, CAPS and Title Case lines (half the words capitalised), words of
/// the menu table and common words are not names; in German (every noun is
/// capitalised) only labels count.
pub fn candidates(lines: &[String]) -> Vec<String> {
    let german = lang::guess(lines.iter().map(String::as_str)) == Some("de");
    let mut found: Vec<String> = Vec::new();
    for (k, line) in lines.iter().enumerate() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let worded: Vec<&str> = (words.iter().copied())
            .filter(|w| w.chars().any(char::is_alphabetic))
            .collect();
        let caps = worded.iter().filter(|w| w.starts_with(char::is_uppercase));
        if words.len() > 1 && caps.count() * 2 >= worded.len() {
            continue;
        }
        let in_sentence = names(&words);
        let plate = match words.as_slice() {
            [one] => !one.ends_with(':') && lines.get(k + 1).is_some_and(|n| sentence(n)),
            [first, rest @ ..] => first.ends_with(':') && sentence(&rest.join(" ")),
            [] => false,
        };
        for (i, w) in words.iter().enumerate() {
            if !((i == 0 && plate) || (in_sentence[i] && !german)) {
                continue;
            }
            let Some(n) = name_of(w) else { continue };
            if lang::common_word(&n.to_lowercase()) || menu::lookup(n, "en").is_some() {
                continue;
            }
            if !found.iter().any(|f| Term::key_of(f) == Term::key_of(n)) {
                found.push(n.to_string());
            }
        }
    }
    found
}

/// The lower-case words of the lines (a name never shows in lower case).
fn lower(lines: &[String]) -> Vec<String> {
    (lines.iter())
        .flat_map(|l| l.split(|c: char| !c.is_alphabetic()))
        .filter(|w| w.chars().next().is_some_and(char::is_lowercase))
        .map(Term::key_of)
        .collect()
}

/// Counts one translated screen (its `lines`) for glossary `slug` `g`.
pub fn observe_in(dir: &Path, slug: &str, g: &Glossary, lines: &[String]) {
    static BOOKS: Mutex<()> = Mutex::new(());
    let _lock = BOOKS.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = DefaultHasher::new();
    lines.hash(&mut h);
    let id = h.finish();
    let mut book = read(dir, slug);
    for low in lower(lines) {
        if let Some(s) = book.seen.get_mut(&low) {
            s.common = true;
        }
    }
    for name in candidates(lines) {
        let key = Term::key_of(&name);
        if known(g, &key) {
            continue;
        }
        if book.seen.len() >= MAX_CANDIDATES && !book.seen.contains_key(&key) {
            continue;
        }
        let s = book.seen.entry(key).or_insert_with(|| Seen {
            text: name,
            ..Seen::default()
        });
        if s.screens.len() < NEEDED && !s.screens.contains(&id) {
            s.screens.push(id);
        }
    }
    write(dir, slug, &book);
}

/// In the glossary, deleted from it or rejected: never a suggestion.
fn known(g: &Glossary, key: &str) -> bool {
    g.terms.iter().any(|t| t.key() == key) || g.removed.iter().chain(&g.rejected).any(|k| k == key)
}

/// The names to suggest for `g` (glossary `slug`), by name.
pub fn pending(dir: &Path, slug: &str, g: &Glossary) -> Vec<String> {
    (read(dir, slug).seen.into_iter())
        .filter(|(k, s)| s.screens.len() >= NEEDED && !s.common && !known(g, k))
        .map(|(_, s)| s.text)
        .collect()
}

/// Rejects `name`: kept in the glossary file, never suggested again.
pub fn reject(dir: &Path, slug: &str, name: &str) -> Result<(), store::Error> {
    let Some(mut g) = store::load(dir, slug) else {
        return Ok(());
    };
    let key = Term::key_of(name);
    if !g.rejected.contains(&key) {
        if g.rejected.len() >= MAX_REJECTED {
            g.rejected.remove(0);
        }
        g.rejected.push(key);
    }
    store::save(dir, slug, &g)
}

#[cfg(test)]
#[path = "suggest_tests.rs"]
mod tests;

/// A translated game screen's `lines`, counted off the pipeline's thread
/// for the glossary in use (none: nothing to suggest into).
pub fn observe(lines: Vec<String>) {
    if !screen::is_game() {
        return;
    }
    std::thread::spawn(move || {
        let dir = store::dir();
        let all = store::load_all(&dir);
        let chosen = crate::shared::config::current().translate.glossary;
        if let Some(slug) = screen::active(&all, &chosen)
            && let Some((_, g)) = all.iter().find(|(s, _)| s == slug)
        {
            observe_in(&dir, slug, g, &lines);
        }
    });
}
