//! Merge, never replace (owner, 2026-11-14): importing a file for a game that
//! already has a glossary merges into it. Same term (case, spaces and accents
//! ignored) = one row; where the player's translation differs the player's
//! stays and the file's goes underneath (`Term::base`); a term the player
//! deleted stays deleted (`Glossary::removed`).

use super::{Glossary, Term, lang};

/// What a merge did: the summary's three numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Merged {
    /// Terms the glossary didn't have.
    pub added: usize,
    /// Terms where the player's translation stays over the file's.
    pub kept_yours: usize,
    /// Terms already the same.
    pub duplicates: usize,
}

impl Term {
    /// The term's identity: its text without case, spaces and accents.
    pub fn key(&self) -> String {
        Term::key_of(&self.text)
    }

    pub fn key_of(text: &str) -> String {
        (text.chars().filter(|c| !c.is_whitespace()))
            .map(lang::plain)
            .collect()
    }
}

impl Glossary {
    /// Merges `file` into this glossary (see the module's rules).
    pub fn merge(&mut self, file: Glossary) -> Merged {
        let mut done = Merged::default();
        for w in file.windows {
            if !self.windows.iter().any(|o| o.eq_ignore_ascii_case(&w)) {
                self.windows.push(w);
            }
        }
        for mut t in file.terms {
            let key = t.key();
            if self.removed.contains(&key) {
                continue;
            }
            let Some(mine) = self.terms.iter_mut().find(|m| m.key() == key) else {
                self.terms.push(t);
                done.added += 1;
                continue;
            };
            let mut conflict = false;
            for (code, theirs) in std::mem::take(&mut t.translations) {
                match mine.translations.get(&code) {
                    None => {
                        mine.translations.insert(code, theirs);
                    }
                    Some(own) if *own == theirs => {}
                    Some(_) => {
                        conflict = true;
                        mine.base.insert(code, theirs);
                    }
                }
            }
            match conflict {
                true => done.kept_yours += 1,
                false => done.duplicates += 1,
            }
        }
        done
    }
}
