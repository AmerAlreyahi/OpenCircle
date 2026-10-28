//! Saved AI chats (tables `chats`, `messages`), on this PC only and only while
//! Settings > AI "Keep chat history" is on (`chat.keep_history`, on by
//! default): TEXT ONLY. A capture is never written: a question that had one
//! keeps only a flag (`had_image`, shown as "Screenshot not kept"). The newest
//! `KEEP` chats are kept. Nothing is uploaded. Blocking: call off the UI thread.

use std::path::Path;

use rusqlite::params;

use std::sync::atomic::{AtomicU64, Ordering};

use super::{Result, err, open, write};

/// Bumped by every "forget everything" (`forget_all`): a save that began
/// before it (its `generation` older) writes nothing, even if it gets the
/// write lock after the clear. (Tests: one per test thread, so parallel tests
/// don't move each other's.)
#[cfg(not(test))]
static GENERATION: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
thread_local! {
    static GENERATION: AtomicU64 = const { AtomicU64::new(0) };
}

fn with_generation<T>(f: impl FnOnce(&AtomicU64) -> T) -> T {
    #[cfg(not(test))]
    return f(&GENERATION);
    #[cfg(test)]
    GENERATION.with(f)
}

/// The generation a save starting now belongs to (`save`).
pub fn generation() -> u64 {
    with_generation(|g| g.load(Ordering::SeqCst))
}

/// After a delete: the WAL is emptied into the file, so no deleted text
/// stays in it.
fn flush(db: &rusqlite::Connection) -> Result<()> {
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(err)
}

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
    /// Its last screenshot's size, physical px (`None` on a save: the one
    /// kept stays).
    pub crop: Option<(u32, u32)>,
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
    /// A favourite: listed first.
    pub star: bool,
    /// Its last screenshot's size, physical px.
    pub crop: Option<(u32, u32)>,
}

/// A crop size as the database keeps it ("420x260"), and back.
fn crop_text(crop: Option<(u32, u32)>) -> Option<String> {
    crop.map(|(w, h)| format!("{w}x{h}"))
}

fn crop_of(text: Option<String>) -> Option<(u32, u32)> {
    let text = text?;
    let (w, h) = text.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
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
/// Nothing when history was forgotten since `generation` (`forget_all`).
pub fn save(path: &Path, chat: &Saved, generation: u64) -> Result<()> {
    let _w = write();
    if generation != self::generation() {
        return Ok(()); // Keep chat history went off (or Clear all) meanwhile
    }
    let mut db = open(path)?;
    let tx = db.transaction().map_err(err)?;
    let (summary, at) = match &chat.summary {
        Some(s) => (Some(s.text.as_str()), Some(s.at as i64)),
        None => (None, None),
    };
    // (the star is History's own: a save never changes it)
    tx.execute(
        "INSERT INTO chats (id, title, created, updated, summary, summary_at, crop)
         VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET title = ?2, updated = ?3, summary = ?4,
             summary_at = ?5, crop = COALESCE(?6, crop)",
        params![
            chat.id,
            chat.title(),
            chat.updated as i64,
            summary,
            at,
            crop_text(chat.crop)
        ],
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
    // (favourites are the last to go)
    tx.execute(
        "DELETE FROM chats WHERE id NOT IN
         (SELECT id FROM chats ORDER BY star DESC, updated DESC, id DESC LIMIT ?1)",
        [KEEP as i64],
    )
    .map_err(err)?;
    tx.execute(
        "DELETE FROM messages WHERE chat_id NOT IN (SELECT id FROM chats)",
        [],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    flush(&db)
}

/// Every saved chat whose title or text holds `find` (any case; blank = all),
/// favourites first, then newest first (none when there is no database).
pub fn list(path: &Path, find: &str) -> Vec<Entry> {
    if !path.exists() {
        return Vec::new();
    }
    let read = || -> Result<Vec<Entry>> {
        let db = open(path)?;
        let mut q = db
            .prepare(
                "SELECT id, title, updated, star, crop FROM chats
                 WHERE ?1 = '' OR instr(lower(title), ?1) > 0 OR id IN
                     (SELECT chat_id FROM messages WHERE instr(lower(text), ?1) > 0)
                 ORDER BY star DESC, updated DESC, id DESC",
            )
            .map_err(err)?;
        let rows = q
            .query_map([find.trim().to_lowercase()], |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    updated: r.get::<_, i64>(2)? as u64,
                    star: r.get(3)?,
                    crop: crop_of(r.get(4)?),
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
    type Row = (i64, Option<String>, Option<i64>, Option<String>);
    let (updated, summary, at, crop): Row = db
        .query_row(
            "SELECT updated, summary, summary_at, crop FROM chats WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
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
        crop: crop_of(crop),
    })
}

/// Stars the chat `id` (a favourite, listed first), or not.
pub fn star(path: &Path, id: &str, on: bool) -> Result<()> {
    let _w = write();
    let db = open(path)?;
    db.execute("UPDATE chats SET star = ?2 WHERE id = ?1", params![id, on])
        .map_err(err)?;
    Ok(())
}

pub fn delete(path: &Path, id: &str) -> Result<()> {
    let _w = write();
    let db = open(path)?;
    db.execute("DELETE FROM messages WHERE chat_id = ?1", [id])
        .map_err(err)?;
    db.execute("DELETE FROM chats WHERE id = ?1", [id])
        .map_err(err)?;
    flush(&db)
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

/// Keep chat history turned off, or "Clear all history": every saved chat
/// goes, on a thread of its own; a save still on its way writes nothing.
pub fn forget_all() {
    with_generation(|g| g.fetch_add(1, Ordering::SeqCst));
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
            crop: Some((420, 260)),
        }
    }

    /// History's search (title and text, any case), its star (favourites
    /// first, kept by later saves) and the crop size (kept when a save has none).
    #[test]
    fn search_stars_and_crop_sizes() {
        let db = tmp("star");
        save(&db, &chat("1-0", 10), generation()).unwrap();
        let mut other = chat("2-0", 20);
        other.turns[1].answer = "The Ferris wheel".into();
        other.crop = None;
        save(&db, &other, generation()).unwrap();
        let ids =
            |find: &str| -> Vec<String> { list(&db, find).into_iter().map(|e| e.id).collect() };
        assert_eq!(ids(""), ["2-0", "1-0"]);
        assert_eq!(ids("  ferris "), ["2-0"], "in an answer, any case");
        assert_eq!(ids("q 1-0"), ["1-0"], "in the title");
        assert!(ids("nowhere").is_empty());
        star(&db, "1-0", true).unwrap();
        save(&db, &chat("1-0", 11), generation()).unwrap(); // a later save keeps it
        let all = list(&db, "");
        assert_eq!((all[0].id.as_str(), all[0].star), ("1-0", true));
        assert_eq!(all[0].crop, Some((420, 260)));
        assert_eq!((all[1].star, all[1].crop), (false, None));
        // a save without a crop keeps the one kept
        let mut again = chat("1-0", 12);
        again.crop = None;
        save(&db, &again, generation()).unwrap();
        assert_eq!(load(&db, "1-0").unwrap().crop, Some((420, 260)));
        star(&db, "1-0", false).unwrap();
        assert_eq!(ids(""), ["2-0", "1-0"]);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// A database from before the star (schema 1) gets it, its chats kept.
    #[test]
    fn a_schema_one_database_is_migrated_with_its_chats() {
        let db = tmp("v1");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        let old = Connection::open(&db).unwrap();
        old.execute_batch(crate::store::MIGRATIONS[0]).unwrap();
        old.execute_batch(
            "INSERT INTO chats (id, title, created, updated) VALUES ('1-0', 'Old', 1, 1);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(old);
        let all = list(&db, "");
        assert_eq!(all.len(), 1);
        assert_eq!(
            (all[0].title.as_str(), all[0].star, all[0].crop),
            ("Old", false, None)
        );
        star(&db, "1-0", true).unwrap();
        assert!(list(&db, "")[0].star);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn saved_newest_first_round_trip_and_delete() {
        let db = tmp("list");
        assert!(
            list(&db, "").is_empty() && !db.exists(),
            "nothing written by reading"
        );
        save(&db, &chat("1-0", 10), generation()).unwrap();
        save(&db, &chat("2-0", 30), generation()).unwrap();
        save(&db, &chat("3-0", 20), generation()).unwrap();
        let ids: Vec<_> = list(&db, "").into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["2-0", "3-0", "1-0"]);
        assert_eq!(load(&db, "2-0").unwrap(), chat("2-0", 30));
        assert_eq!(list(&db, "")[0].title, "Q 2-0");
        // saved again: replaced, not doubled
        let mut c = chat("2-0", 40);
        c.turns.pop();
        save(&db, &c, generation()).unwrap();
        assert_eq!(load(&db, "2-0").unwrap().turns.len(), 1);
        delete(&db, "3-0").unwrap();
        assert_eq!(list(&db, "").len(), 2);
        clear(&db).unwrap();
        assert!(list(&db, "").is_empty());
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn only_the_newest_hundred_are_kept() {
        let db = tmp("cap");
        for i in 0..(KEEP as u64 + 3) {
            save(&db, &chat(&format!("{i}-0"), i), generation()).unwrap();
        }
        let all = list(&db, "");
        assert_eq!(all.len(), KEEP);
        assert_eq!(all.last().unwrap().id, "3-0", "the three oldest went");
        assert!(load(&db, "0-0").is_err());
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// Only text: the file holds the words, never a picture's bytes.
    #[test]
    fn the_database_holds_no_image_bytes() {
        let db = tmp("text");
        save(&db, &chat("1-0", 1), generation()).unwrap();
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
        save(&db, &chat("1-0", 1), generation()).unwrap();
        crate::store::test_path(Some(db.clone()));
        forget_all();
        let gone = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            list(&db, "").is_empty()
        });
        crate::store::test_path(None);
        assert!(gone);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// A deleted chat's words are gone from the file and its WAL (secure
    /// delete, checkpoint), and so are a pruned one's.
    #[test]
    fn deleted_text_does_not_stay_in_the_file() {
        let db = tmp("secure");
        let mut secret = chat("1-0", 1);
        secret.turns[0].question = "MARKER-zq81x my bank pin".into();
        save(&db, &secret, generation()).unwrap();
        save(&db, &chat("2-0", 2), generation()).unwrap();
        let bytes = |db: &Path| {
            let mut all = std::fs::read(db).unwrap();
            all.extend(std::fs::read(db.with_extension("db-wal")).unwrap_or_default());
            all
        };
        let marker = b"MARKER-zq81x";
        assert!(
            bytes(&db).windows(marker.len()).any(|w| w == marker),
            "the test sees it"
        );
        delete(&db, "1-0").unwrap();
        assert!(
            !bytes(&db).windows(marker.len()).any(|w| w == marker),
            "deleted for good"
        );
        // pruned past the cap: gone too
        save(&db, &secret, generation()).unwrap();
        for i in 0..KEEP as u64 {
            save(&db, &chat(&format!("{}-1", i + 10), i + 10), generation()).unwrap();
        }
        assert!(load(&db, "1-0").is_err());
        assert!(
            !bytes(&db).windows(marker.len()).any(|w| w == marker),
            "pruned for good"
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// A save that began before Keep chat history went off, and gets the lock
    /// only after, writes nothing.
    #[test]
    fn a_save_from_before_forgetting_writes_nothing() {
        let db = tmp("race");
        let began = generation();
        forget_all(); // (no database set: only the generation moves)
        save(&db, &chat("1-0", 1), began).unwrap();
        assert!(!db.exists(), "nothing written");
        save(&db, &chat("2-0", 2), generation()).unwrap();
        assert_eq!(list(&db, "").len(), 1, "a new save goes on");
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
