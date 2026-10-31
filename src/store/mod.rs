//! The app's one database (`paths::database`, SQLite compiled into the exe):
//! growing records, one table set per feature in a file of its own (`chats`).
//! The config stays in config.toml (frozen format). Blocking (files): call off
//! the UI thread. One writer at a time (`write`), WAL journal so reads don't
//! wait on it.
//!
//! A new feature: a file here with its functions, and ONE new entry at the end
//! of `MIGRATIONS` (never edit a shipped one).

pub mod chats;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

/// Schema steps, in order: entry `i` takes the database from version `i` to
/// `i + 1` (`PRAGMA user_version`).
const MIGRATIONS: &[&str] = &[
    // 1: saved AI chats (`chats`), text only.
    "CREATE TABLE chats (
         id TEXT PRIMARY KEY, title TEXT NOT NULL,
         created INTEGER NOT NULL, updated INTEGER NOT NULL,
         summary TEXT, summary_at INTEGER);
     CREATE TABLE messages (
         chat_id TEXT NOT NULL, seq INTEGER NOT NULL, role TEXT NOT NULL,
         text TEXT NOT NULL, had_image INTEGER NOT NULL DEFAULT 0,
         PRIMARY KEY (chat_id, seq));",
    // 2: History's star (a favourite) and the size of the chat's last
    // screenshot as text ("420x260"; the picture itself is never kept).
    "ALTER TABLE chats ADD COLUMN star INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE chats ADD COLUMN crop TEXT;",
];

pub type Result<T> = std::result::Result<T, String>;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    format!("database: {e}")
}

/// The database file, or `None` (tests: only the one a test sets with
/// `test_path`, so the data dir stays empty unless a test writes there itself).
pub fn path() -> Option<PathBuf> {
    #[cfg(test)]
    return TEST_PATH.with(|d| d.borrow().clone());
    #[cfg(not(test))]
    crate::paths::database()
}

#[cfg(test)]
thread_local! {
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests: this thread's database.
#[cfg(test)]
pub fn test_path(path: Option<PathBuf>) {
    TEST_PATH.with(|d| *d.borrow_mut() = path);
}

/// One writer at a time in this process (hold it for the whole write).
pub(crate) fn write() -> MutexGuard<'static, ()> {
    static WRITING: Mutex<()> = Mutex::new(());
    WRITING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Opens (creating) the database at `path`, migrated to the current schema.
pub(crate) fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let mut db = Connection::open(path).map_err(err)?;
    db.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(err)?;
    db.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
    // deleted rows are overwritten, never left readable in free pages
    db.pragma_update(None, "secure_delete", "ON").map_err(err)?;
    let version = |db: &Connection| -> Result<usize> {
        let v: usize = (db.pragma_query_value(None, "user_version", |r| r.get(0))).map_err(err)?;
        match v > MIGRATIONS.len() {
            true => Err(err("written by a newer OpenCircle")),
            false => Ok(v),
        }
    };
    if version(&db)? < MIGRATIONS.len() {
        // Another connection (a save thread, History) may be migrating it right
        // now: take the write lock first, then read where it stands.
        let tx = (db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))
            .map_err(err)?;
        for (i, step) in MIGRATIONS.iter().enumerate().skip(version(&tx)?) {
            tx.execute_batch(step).map_err(err)?;
            tx.pragma_update(None, "user_version", i + 1).map_err(err)?;
        }
        tx.commit().map_err(err)?;
    }
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_database_is_migrated_once_to_the_latest_schema() {
        let dir = std::env::temp_dir().join(format!("oc-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("opencircle.db");
        for _ in 0..2 {
            let db = open(&file).unwrap();
            let v: usize = db
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(v, MIGRATIONS.len());
        }
        let db = open(&file).unwrap();
        db.pragma_update(None, "user_version", MIGRATIONS.len() + 1)
            .unwrap();
        drop(db);
        assert!(open(&file).is_err(), "a newer schema is not touched");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A new database opened by several connections at once (the chat's save
    /// thread and History's list): one migrates, the others wait for it
    /// ("table chats already exists" before).
    #[test]
    fn connections_opening_a_new_database_together_migrate_it_once() {
        for round in 0..10 {
            let dir =
                std::env::temp_dir().join(format!("oc-store-race-{}-{round}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let file = dir.join("opencircle.db");
            let start = std::sync::Barrier::new(4);
            let opened: Vec<_> = std::thread::scope(|s| {
                let open = || {
                    start.wait();
                    open(&file).map(drop)
                };
                let threads: Vec<_> = (0..4).map(|_| s.spawn(open)).collect();
                threads.into_iter().map(|t| t.join().unwrap()).collect()
            });
            assert!(opened.iter().all(Result::is_ok), "{opened:?}");
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
