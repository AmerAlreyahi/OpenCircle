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
    /// Rejections of a game that has no glossary yet (carried into the one
    /// its first accepted name creates).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rejected: Vec<String>,
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
    // (a game with no glossary yet may be the first thing in the folder)
    let _ = std::fs::create_dir_all(dir);
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
        if known(g, &key) || book.rejected.contains(&key) {
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
    let book = read(dir, slug);
    (book.seen.into_iter())
        .filter(|(k, s)| {
            s.screens.len() >= NEEDED && !s.common && !known(g, k) && !book.rejected.contains(k)
        })
        .map(|(_, s)| s.text)
        .collect()
}

/// Rejects `name`: kept in the glossary file (else, while the game has none,
/// in its counts), never suggested again.
pub fn reject(dir: &Path, slug: &str, name: &str) -> Result<(), store::Error> {
    let key = Term::key_of(name);
    let Some(mut g) = store::load(dir, slug) else {
        let mut book = read(dir, slug);
        if !book.rejected.contains(&key) {
            book.rejected.push(key);
            book.rejected.truncate(MAX_REJECTED);
            write(dir, slug, &book);
        }
        return Ok(());
    };
    if !g.rejected.contains(&key) {
        if g.rejected.len() >= MAX_REJECTED {
            g.rejected.remove(0);
        }
        g.rejected.push(key);
    }
    store::save(dir, slug, &g)
}

/// What the counts of the game on screen are kept under: its glossary's slug,
/// else (no glossary yet) its window's title as a file name where the OS
/// tells it (Windows, X11: the glossary the first accepted name creates gets
/// the same slug), else one "unnamed game" (Wayland).
pub fn game_key(active: &str, title: Option<&str>) -> String {
    match (active, title.map(str::trim)) {
        ("", Some(t)) if !t.is_empty() => store::slug(t),
        ("", _) => "unnamed-game".into(),
        (slug, _) => slug.into(),
    }
}

/// A glossary was just created for the game counted under `key`: it takes
/// the rejections along, and the counts follow its slug.
pub fn adopt(dir: &Path, key: &str, slug: &str) {
    let book = read(dir, key);
    if let Some(mut g) = store::load(dir, slug) {
        for r in book.rejected {
            if !g.rejected.contains(&r) && g.rejected.len() < MAX_REJECTED {
                g.rejected.push(r);
            }
        }
        let _ = store::save(dir, slug, &g);
    }
    if key != slug && !path(dir, slug).exists() {
        let _ = std::fs::rename(path(dir, key), path(dir, slug));
    }
}

#[cfg(test)]
#[path = "suggest_tests.rs"]
mod tests;

/// A translated game screen's `lines`, counted off the pipeline's thread
/// for the glossary in use or, with none yet, for the game's window.
pub fn observe(lines: Vec<String>) {
    if !screen::is_game() {
        return;
    }
    std::thread::spawn(move || {
        let dir = store::dir();
        let all = store::load_all(&dir);
        let chosen = crate::shared::config::current().translate.glossary;
        let slug = screen::active(&all, &chosen).unwrap_or("");
        let title = crate::shared::platform::capture::window_rect::captured().map(|f| f.title);
        // (no glossary yet: the game is counted under its window, see `game_key`)
        let key = game_key(slug, title.as_deref());
        let g = all.iter().find(|(s, _)| s == slug).map(|(_, g)| g.clone());
        observe_in(&dir, &key, &g.unwrap_or_default(), &lines);
    });
}
