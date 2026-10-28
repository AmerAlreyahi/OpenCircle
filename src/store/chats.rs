//! Saved AI chats (tables `chats`, `messages`), on this PC only and only while
//! Settings > AI "Keep chat history" is on (`chat.keep_history`, on by
//! default): TEXT ONLY. A capture is never written: a question that had one
//! keeps only a flag (`had_image`, shown as "Screenshot not kept"). The newest
//! `KEEP` chats are kept. Nothing is uploaded. Blocking: call off the UI thread.

use std::path::Path;

use rusqlite::params;

use super::{Result, err, open, write};

/// Chats kept; older ones are deleted on the next save.
pub const KEEP: usize = 100;

/// One saved chat.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Saved {
    /// Its key (`new_id`).
    pub id: String,
    /// Last change, unix seconds.
    pub updated: u64,
    pub turns: Vec<Turn>,
    /// What the model was told about the turns before its last messages.
    pub summary: Option<Summary>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Turn {
    pub question: String,
    pub answer: String,
    /// A screenshot went with the question (it is not kept).
    pub screen: bool,
}

/// A short summary of a chat's older messages, written by the user's provider.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    pub text: String,
    /// How many messages the chat had when it was written.
    pub at: usize,
}

/// The History list's row: a chat without its messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub updated: u64,
}

impl Saved {
    /// The History list's title: the first question, on one line.
    pub fn title(&self) -> String {
        let first = self
            .turns
            .first()
            .map_or("New chat", |t| t.question.as_str());
        let line = first.lines().next().unwrap_or_default().trim();
        match line.chars().count() > 48 {
            true => format!("{}…", line.chars().take(47).collect::<String>()),
            false => line.to_string(),
        }
    }
}

/// Unix seconds now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A new chat's id: the time in ms and a counter (unique in this run).
pub fn new_id() -> String {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{ms}-{n}")
}

/// Writes `chat` (its rows replaced), then keeps the newest `KEEP` chats.
pub fn save(path: &Path, chat: &Saved) -> Result<()> {
    let _w = write();
    let mut db = open(path)?;
    let tx = db.transaction().map_err(err)?;
    let (summary, at) = match &chat.summary {
        Some(s) => (Some(s.text.as_str()), Some(s.at as i64)),
        None => (None, None),
    };
    tx.execute(
        "INSERT INTO chats (id, title, created, updated, summary, summary_at)
         VALUES (?1, ?2, ?3, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET title = ?2, updated = ?3, summary = ?4, summary_at = ?5",
        params![chat.id, chat.title(), chat.updated as i64, summary, at],
    )
    .map_err(err)?;
    tx.execute("DELETE FROM messages WHERE chat_id = ?1", [&chat.id])
        .map_err(err)?;
    {
        let mut add = tx
            .prepare(
                "INSERT INTO messages (chat_id, seq, role, text, had_image)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(err)?;
        for (i, t) in chat.turns.iter().enumerate() {
            let seq = 2 * i as i64;
            add.execute(params![chat.id, seq, "user", t.question, t.screen])
                .map_err(err)?;
            add.execute(params![chat.id, seq + 1, "assistant", t.answer, false])
                .map_err(err)?;
        }
    }
    tx.execute(
        "DELETE FROM chats WHERE id NOT IN
         (SELECT id FROM chats ORDER BY updated DESC, id DESC LIMIT ?1)",
        [KEEP as i64],
    )
    .map_err(err)?;
    tx.execute(
        "DELETE FROM messages WHERE chat_id NOT IN (SELECT id FROM chats)",
        [],
    )
    .map_err(err)?;
    tx.commit().map_err(err)
}

/// Every saved chat, newest first (none when there is no database).
pub fn list(path: &Path) -> Vec<Entry> {
    if !path.exists() {
        return Vec::new();
    }
    let read = || -> Result<Vec<Entry>> {
        let db = open(path)?;
        let mut q = db
            .prepare("SELECT id, title, updated FROM chats ORDER BY updated DESC, id DESC")
            .map_err(err)?;
        let rows = q
            .query_map([], |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    updated: r.get::<_, i64>(2)? as u64,
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(err)
    };
    read().unwrap_or_else(|e| {
        crate::log!("[opencircle] {e}");
        Vec::new()
    })
}

/// The chat `id` with its messages.
pub fn load(path: &Path, id: &str) -> Result<Saved> {
    let db = open(path)?;
    let (updated, summary, at): (i64, Option<String>, Option<i64>) = db
        .query_row(
            "SELECT updated, summary, summary_at FROM chats WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(err)?;
    let mut q = db
        .prepare("SELECT role, text, had_image FROM messages WHERE chat_id = ?1 ORDER BY seq")
        .map_err(err)?;
    let rows = q
        .query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })
        .map_err(err)?;
    let mut turns: Vec<Turn> = Vec::new();
    for row in rows {
        let (role, text, had_image) = row.map_err(err)?;
        match (role.as_str(), turns.last_mut()) {
            ("assistant", Some(turn)) => turn.answer = text,
            _ => turns.push(Turn {
                question: text,
                answer: String::new(),
                screen: had_image,
            }),
        }
    }
    Ok(Saved {
        id: id.into(),
        updated: updated as u64,
        turns,
        summary: summary.map(|text| Summary {
            text,
            at: at.unwrap_or_default() as usize,
        }),
    })
}

pub fn delete(path: &Path, id: &str) -> Result<()> {
    let _w = write();
    let db = open(path)?;
    db.execute("DELETE FROM messages WHERE chat_id = ?1", [id])
        .map_err(err)?;
    db.execute("DELETE FROM chats WHERE id = ?1", [id])
        .map_err(err)?;
    Ok(())
}

/// "Clear all history", and Keep chat history turned off: every row goes, and
/// the freed pages with them (VACUUM), so no old text stays in the file.
pub fn clear(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let _w = write();
    let db = open(path)?;
    db.execute_batch(
        "DELETE FROM messages; DELETE FROM chats; VACUUM;
         PRAGMA wal_checkpoint(TRUNCATE);",
    )
    .map_err(err)
}

/// Keep chat history turned off: every saved chat goes, on a thread of its own.
pub fn forget_all() {
    let Some(path) = super::path() else {
        return;
    };
    std::thread::spawn(move || {
        if let Err(e) = clear(&path) {
            crate::log!("[opencircle] {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use rusqlite::Connection;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oc-chats-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("opencircle.db")
    }

    fn chat(id: &str, updated: u64) -> Saved {
        Saved {
            id: id.into(),
            updated,
            turns: vec![
                Turn {
                    question: format!("Q {id}"),
                    answer: "A".into(),
                    screen: true,
                },
                Turn {
                    question: "Again?".into(),
                    answer: "Yes".into(),
                    screen: false,
                },
            ],
            summary: Some(Summary {
                text: "short".into(),
                at: 22,
            }),
        }
    }

    #[test]
    fn saved_newest_first_round_trip_and_delete() {
        let db = tmp("list");
        assert!(
            list(&db).is_empty() && !db.exists(),
            "nothing written by reading"
        );
        save(&db, &chat("1-0", 10)).unwrap();
        save(&db, &chat("2-0", 30)).unwrap();
        save(&db, &chat("3-0", 20)).unwrap();
        let ids: Vec<_> = list(&db).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["2-0", "3-0", "1-0"]);
        assert_eq!(load(&db, "2-0").unwrap(), chat("2-0", 30));
        assert_eq!(list(&db)[0].title, "Q 2-0");
        // saved again: replaced, not doubled
        let mut c = chat("2-0", 40);
        c.turns.pop();
        save(&db, &c).unwrap();
        assert_eq!(load(&db, "2-0").unwrap().turns.len(), 1);
        delete(&db, "3-0").unwrap();
        assert_eq!(list(&db).len(), 2);
        clear(&db).unwrap();
        assert!(list(&db).is_empty());
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn only_the_newest_hundred_are_kept() {
        let db = tmp("cap");
        for i in 0..(KEEP as u64 + 3) {
            save(&db, &chat(&format!("{i}-0"), i)).unwrap();
        }
        let all = list(&db);
        assert_eq!(all.len(), KEEP);
        assert_eq!(all.last().unwrap().id, "3-0", "the three oldest went");
        assert!(load(&db, "0-0").is_err());
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// Only text: the file holds the words, never a picture's bytes.
    #[test]
    fn the_database_holds_no_image_bytes() {
        let db = tmp("text");
        save(&db, &chat("1-0", 1)).unwrap();
        let conn = Connection::open(&db).unwrap();
        let mut q = conn
            .prepare("SELECT name FROM pragma_table_info('messages')")
            .unwrap();
        let cols: Vec<String> = q
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(cols, ["chat_id", "seq", "role", "text", "had_image"]);
        drop(q);
        drop(conn);
        let mut bytes = std::fs::read(&db).unwrap();
        if let Ok(wal) = std::fs::read(db.with_extension("db-wal")) {
            bytes.extend(wal);
        }
        assert!(
            !bytes.windows(4).any(|w| w == b"\x89PNG"),
            "no PNG in the file"
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// Keep chat history turned off: everything kept goes.
    #[test]
    fn forgetting_clears_every_saved_chat() {
        let db = tmp("forget");
        save(&db, &chat("1-0", 1)).unwrap();
        crate::store::test_path(Some(db.clone()));
        forget_all();
        let gone = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            list(&db).is_empty()
        });
        crate::store::test_path(None);
        assert!(gone);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn titles_are_the_first_question_on_one_line() {
        let mut c = chat("1-0", 1);
        c.turns[0].question = format!("{}\nmore", "x".repeat(60));
        assert_eq!(c.title().chars().count(), 48);
        assert_eq!(Saved::default().title(), "New chat");
    }
}
