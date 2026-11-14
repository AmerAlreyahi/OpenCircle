//! The per-game glossary (planning #13, owner 2026-11-14): names and terms a
//! game uses, kept as they are or always translated one way.
//!
//! Authority per line: user glossary > the built-in menu table ([`lookup`]) >
//! the model. Offline MT can't be told a term, so a term in a line is
//! swapped for a number the model copies and swapped back after (`apply` /
//! `restore`, the way `pipeline::shield` keeps counters); the AI is guided
//! by the terms of its batch (`ai_terms`), softly. The cache key carries
//! `stamp`, so editing one term only re-translates the lines holding it.
//!
//! Where the pipeline calls it: see REPORT.md ("Wiring").
// The pipeline's calls (`apply`, `restore`, `stamp`, `ai_terms`, `store::active`) land with
// pipeline.rs after W9's game-mode work: drop this allow with them.
#![allow(dead_code)]

mod menu;
pub(crate) mod store;

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub use menu::lookup;

use super::lang;

/// One term: as it shows on screen, and per target language what it
/// becomes. No entry for a language = kept as it is (owner: an empty
/// translation means keep).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Term {
    pub text: String,
    /// Target language code ("ar", "fr") -> the fixed translation.
    pub translations: BTreeMap<String, String>,
    /// Match case (default off: game text is often all caps).
    pub case: bool,
}

impl Term {
    /// What `text` becomes in `target`: the fixed translation, else `None`
    /// (kept as it is).
    pub fn fixed(&self, target: &str) -> Option<&str> {
        let code = lang::find(target)?.code;
        self.translations
            .get(code)
            .map(String::as_str)
            .filter(|t| !t.trim().is_empty())
    }
}

/// One game's glossary: a file in the data dir (`store`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Glossary {
    pub name: String,
    /// Window titles that pick it by themselves (substring, case-insensitive).
    pub windows: Vec<String>,
    pub terms: Vec<Term>,
}

/// A term found in a line: its byte range and its entry.
struct Hit<'a> {
    range: std::ops::Range<usize>,
    term: &'a Term,
}

/// What `apply` did to a line: what is sent, and what each placeholder
/// stands for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub text: String,
    /// (placeholder number, the term as the line had it, what it becomes).
    kept: Vec<(String, String, String)>,
}

impl Applied {
    /// No term in the line: `text` is the line as it was.
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

impl Glossary {
    /// Whole-word, longest-first, non-overlapping hits of the terms in
    /// `text`, in reading order.
    fn hits(&self, text: &str) -> Vec<Hit<'_>> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut all: Vec<Hit> = Vec::new();
        for term in self.terms.iter().filter(|t| !t.text.trim().is_empty()) {
            let want: Vec<char> = term.text.trim().chars().collect();
            let fold = |c: char| if term.case { c } else { lang::plain(c) };
            let want: Vec<char> = want.into_iter().map(fold).collect();
            let word = |c: char| c.is_alphanumeric() || c == '_';
            for i in 0..chars.len() {
                let end = i + want.len();
                if end > chars.len()
                    || !chars[i..end]
                        .iter()
                        .zip(&want)
                        .all(|(&(_, c), &w)| fold(c) == w)
                {
                    continue;
                }
                let edge = |c: char, near: Option<&(usize, char)>| {
                    !(word(c) && near.is_some_and(|&(_, n)| word(n)))
                };
                let (first, last) = (want[0], want[want.len() - 1]);
                if edge(first, i.checked_sub(1).map(|j| &chars[j])) && edge(last, chars.get(end)) {
                    let from = chars[i].0;
                    let to = chars.get(end).map_or(text.len(), |&(b, _)| b);
                    all.push(Hit {
                        range: from..to,
                        term,
                    });
                }
            }
        }
        // the longest term first, then the earliest; overlaps are dropped
        all.sort_by_key(|h| (std::cmp::Reverse(h.term.text.len()), h.range.start));
        let mut kept: Vec<Hit> = Vec::new();
        for h in all {
            if kept
                .iter()
                .all(|k| h.range.end <= k.range.start || h.range.start >= k.range.end)
            {
                kept.push(h);
            }
        }
        kept.sort_by_key(|h| h.range.start);
        kept
    }

    /// A line that IS a term (a name label, a button) is answered from the
    /// glossary, no model: its fixed translation, or itself (kept).
    pub fn whole(&self, text: &str, target: &str) -> Option<String> {
        let t = text.trim();
        let hits = self.hits(t);
        let [h] = &hits[..] else { return None };
        (h.range == (0..t.len())).then(|| h.term.fixed(target).unwrap_or(t).to_string())
    }

    /// `text` with each term swapped for a number the model copies (one not
    /// in the line), and what to put back.
    pub fn apply(&self, text: &str, target: &str) -> Applied {
        let hits = self.hits(text);
        if hits.is_empty() {
            return Applied {
                text: text.to_string(),
                kept: Vec::new(),
            };
        }
        let used: Vec<String> = numbers(text);
        let mut next = 700;
        let mut fresh = || loop {
            next += 1;
            let n = next.to_string();
            if !used.contains(&n) {
                return n;
            }
        };
        let (mut out, mut at) = (String::new(), 0);
        let mut kept = Vec::new();
        for h in &hits {
            let n = fresh();
            let as_is = &text[h.range.clone()];
            out.push_str(&text[at..h.range.start]);
            out.push_str(&n);
            at = h.range.end;
            let becomes = h.term.fixed(target).unwrap_or(as_is).to_string();
            kept.push((n, as_is.to_string(), becomes));
        }
        out.push_str(&text[at..]);
        Applied { text: out, kept }
    }

    /// The terms of `text` as a stamp for the cache key: empty when none
    /// matches; changes when a matching entry (or its fixed translation
    /// for `target`) does.
    pub fn stamp(&self, text: &str, target: &str) -> String {
        let hits = self.hits(text);
        if hits.is_empty() {
            return String::new();
        }
        let mut h = DefaultHasher::new();
        for hit in hits {
            (&hit.term.text, hit.term.fixed(target), hit.term.case).hash(&mut h);
        }
        format!("{:x}", h.finish())
    }

    /// The prompt lines for the terms occurring in `texts` (only this
    /// batch's: short and private), "" when none. Soft: the AI may inflect.
    pub fn ai_terms(&self, texts: &[String], target: &str) -> String {
        let mut seen: Vec<&Term> = Vec::new();
        for t in texts {
            for h in self.hits(t) {
                if !seen.contains(&h.term) {
                    seen.push(h.term);
                }
            }
        }
        if seen.is_empty() {
            return String::new();
        }
        let list: Vec<String> = (seen.iter())
            .map(|t| match t.fixed(target) {
                Some(x) => format!("{} -> {x}", t.text),
                None => format!("{} -> keep as is", t.text),
            })
            .collect();
        format!("Glossary, use exactly: {}", list.join("; "))
    }

    /// Fixed translations the AI left out of `out` (for the log; never
    /// forced): the terms of `text` whose fixed translation isn't in `out`.
    pub fn missed<'a>(&'a self, text: &str, out: &str, target: &str) -> Vec<&'a str> {
        let out = out.to_lowercase();
        (self.hits(text).into_iter())
            .filter_map(|h| h.term.fixed(target).map(|f| (h.term, f)))
            .filter(|(_, f)| !out.contains(&f.to_lowercase()))
            .map(|(t, _)| t.text.as_str())
            .collect()
    }
}

/// `out` (what the model wrote for `applied.text`) with each number put
/// back as its term. `None` when a number was lost or doubled: the caller
/// translates the line again without numbers and uses [`Applied::fix`].
pub fn restore(out: &str, applied: &Applied) -> Option<String> {
    let mut out = out.to_string();
    for (n, _, becomes) in &applied.kept {
        let runs = digit_runs(&out);
        let mine: Vec<_> = runs.iter().filter(|(_, v)| v == n).collect();
        let [(r, _)] = &mine[..] else { return None };
        out.replace_range(r.clone(), becomes);
    }
    Some(out)
}

impl Applied {
    /// The fallback when `restore` failed: `plain` is the line translated
    /// without numbers; a term it still holds as it was written (names are
    /// often copied) becomes what it should; else `plain` stays.
    pub fn fix(&self, plain: &str) -> String {
        let mut out = plain.to_string();
        for (_, as_is, becomes) in &self.kept {
            if let Some(i) = out.to_lowercase().find(&as_is.to_lowercase())
                && out.is_char_boundary(i)
                && out.is_char_boundary(i + as_is.len())
            {
                out.replace_range(i..i + as_is.len(), becomes);
            }
        }
        out
    }
}

/// ASCII value of a digit incl. Arabic-Indic ones (a model into Arabic may
/// write them).
fn ascii_digit(c: char) -> Option<char> {
    match c {
        '0'..='9' => Some(c),
        '\u{0660}'..='\u{0669}' => char::from_digit(c as u32 - 0x0660, 10),
        '\u{06F0}'..='\u{06F9}' => char::from_digit(c as u32 - 0x06F0, 10),
        _ => None,
    }
}

/// The runs of digits of `s`: byte range and value as ASCII.
fn digit_runs(s: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut runs = Vec::new();
    let mut cur: Option<(usize, String)> = None;
    for (i, c) in s.char_indices().chain([(s.len(), ' ')]) {
        match (ascii_digit(c), cur.take()) {
            (Some(d), Some((a, mut v))) => {
                v.push(d);
                cur = Some((a, v));
            }
            (Some(d), None) => cur = Some((i, d.to_string())),
            (None, Some((a, v))) => runs.push((a..i, v)),
            (None, None) => {}
        }
    }
    runs
}

fn numbers(s: &str) -> Vec<String> {
    digit_runs(s).into_iter().map(|(_, v)| v).collect()
}

#[cfg(test)]
mod tests;
