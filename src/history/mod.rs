//! What the app remembers on this PC, each only when the user turned it on:
//! searches handed to the browser, recent songs, recent snip captures, and
//! (in `store::chats`) AI chats. Settings > History counts and clears the
//! kinds it lists through `tidy`, from where each one lives.
use std::path::Path;

pub mod recent;
pub mod search;
pub mod song;

/// A kind of record Settings > History lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Snip's recent captures (`recent`, PNG files).
    Captures,
    /// Saved AI chats (`store::chats`, opencircle.db).
    Chats,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Captures, Kind::Chats];
}

/// How many of each kind are kept now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub captures: usize,
    pub chats: usize,
}

impl Counts {
    pub fn of(&self, kind: Kind) -> usize {
        match kind {
            Kind::Captures => self.captures,
            Kind::Chats => self.chats,
        }
    }
}

/// Blocking (call it on a thread): deletes every record of the kinds in
/// `clear`, then counts what is kept.
pub fn tidy(clear: &[Kind]) -> Counts {
    tidy_in(
        recent::dir().as_deref(),
        crate::store::path().as_deref(),
        clear,
    )
}

fn tidy_in(captures: Option<&Path>, db: Option<&Path>, clear: &[Kind]) -> Counts {
    if clear.contains(&Kind::Captures)
        && let Some(dir) = captures
    {
        recent::clear_dir(dir);
    }
    if clear.contains(&Kind::Chats)
        && let Some(db) = db
    {
        crate::store::chats::forget(db);
    }
    Counts {
        captures: captures.map_or(0, |d| recent::list(d).len()),
        chats: db.map_or(0, |db| crate::store::chats::list(db, "").len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::chats::{self, Saved, Turn};

    #[test]
    fn tidy_counts_each_kind_and_clears_only_what_it_is_asked() {
        let root = std::env::temp_dir().join(format!("oc-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (dir, db) = (root.join("recent"), root.join("opencircle.db"));
        let shot = crate::actions::Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        for _ in 0..3 {
            recent::keep(&dir, &shot, 20).unwrap();
        }
        for i in 0..2 {
            let chat = Saved {
                id: format!("chat-{i}"),
                updated: 1,
                turns: vec![Turn {
                    question: "hello".into(),
                    answer: "hi".into(),
                    screen: false,
                }],
                ..Saved::default()
            };
            chats::save(&db, &chat, chats::generation()).unwrap();
        }
        let (d, b) = (Some(dir.as_path()), Some(db.as_path()));
        let both = Counts {
            captures: 3,
            chats: 2,
        };
        assert_eq!(tidy_in(d, b, &[]), both);
        assert_eq!(both.of(Kind::Chats), 2);
        let chats_only = Counts {
            captures: 0,
            chats: 2,
        };
        assert_eq!(tidy_in(d, b, &[Kind::Captures]), chats_only);
        assert_eq!(tidy_in(d, b, &Kind::ALL), Counts::default());
        // a chat save that began before the clear writes nothing
        let late = Saved {
            id: "late".into(),
            ..Saved::default()
        };
        chats::save(&db, &late, chats::generation() - 1).unwrap();
        assert_eq!(tidy_in(d, b, &[]), Counts::default());
        // nothing kept anywhere yet: nothing to count
        assert_eq!(tidy_in(None, None, &Kind::ALL), Counts::default());
        let _ = std::fs::remove_dir_all(&root);
    }
}
