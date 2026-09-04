//! SQLite connection setup and migrations.
//!
//! Household-scale load and sub-millisecond queries mean a single guarded
//! connection is plenty; a pool would be a dependency with nothing to earn it.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Migrations are embedded so the binary is self-contained; the on-disk
/// `migrations/` dir is the source of truth at build time only.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/001_init.sql")),
    (2, include_str!("../migrations/002_global_settings.sql")),
    (3, include_str!("../migrations/003_board_views.sql")),
];

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Opens (creating if needed) the database at `path` and brings it up to
    /// the latest schema version.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening database at {}", path.display()))?;

        // WAL lets readers proceed during a write; busy_timeout absorbs the
        // brief contention two people clicking at once can produce.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate().context("running migrations")?;
        Ok(db)
    }

    /// Runs any migrations the database has not seen yet, each in its own
    /// transaction so a failure leaves the schema at the last good version.
    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");

        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                 version    INTEGER PRIMARY KEY,
                 applied_at TEXT NOT NULL
             )",
            [],
        )?;

        let current: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;

        for (version, sql) in MIGRATIONS {
            if *version <= current {
                continue;
            }
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
                [version],
            )?;
            tx.commit()?;
            tracing::info!(version, "applied migration");
        }

        drop(conn); // release the lock before returning, not at scope end
        Ok(())
    }

    /// Runs `f` with the single shared connection.
    ///
    /// Generic over the closure's error so both shapes pass through unchanged:
    /// a bare `queries::*` function (`rusqlite::Result`) can be handed over
    /// directly, while a closure doing several queries can return
    /// `anyhow::Result` and attach `.context()`. Callers lift either with `?`.
    pub fn with<T, E>(
        &self,
        f: impl FnOnce(&Connection) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let out = f(&conn);
        drop(conn); // release before the caller does anything with the result
        out
    }

    /// Folds the write-ahead log back into the database file.
    ///
    /// Run at shutdown. Committed data is already durable in the `-wal` file
    /// and SQLite replays it on the next open, so this is not about losing
    /// writes — it is about what a backup sees. The backup story here is a ZFS
    /// snapshot or a `cp` of `planner.sqlite3`, and a copy of that file alone,
    /// taken while a log is outstanding, is missing the most recent writes.
    pub fn checkpoint(&self) -> Result<()> {
        self.with(|conn| {
            // Returns a row (busy, log pages, checkpointed pages); nothing to
            // do with it, but the statement is a query and must be read as one.
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        })
        .context("checkpointing the write-ahead log")
    }

    /// Runs `f` inside a transaction, committing on `Ok` and rolling back on `Err`.
    pub fn transaction<T, E>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E>
    where
        E: From<rusqlite::Error>,
    {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        drop(conn);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory of our own, so the test touches nothing else.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "trusted-planner-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// SQLite only parses a statement when it is prepared, so a typo in a
    /// pragma that runs once, at shutdown, would otherwise stay hidden until a
    /// container stopped. It also checks the thing the pragma is *for*: the
    /// log is folded back in, leaving the database file self-contained for
    /// whatever copies it next.
    #[test]
    fn checkpointing_empties_the_write_ahead_log() {
        let dir = scratch("checkpoint");
        let path = dir.join("planner.sqlite3");
        let db = Db::open(&path).unwrap();

        db.with(|conn| {
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES ('probe', 'x')",
                [],
            )
        })
        .unwrap();

        let wal = dir.join("planner.sqlite3-wal");
        assert!(
            std::fs::metadata(&wal).unwrap().len() > 0,
            "the write should be sitting in the log"
        );

        db.checkpoint().unwrap();
        assert_eq!(
            std::fs::metadata(&wal).unwrap().len(),
            0,
            "TRUNCATE leaves the log file empty rather than merely rewound"
        );

        // And the data is still there afterwards, which is the whole point.
        let kept: String = db
            .with(|conn| {
                conn.query_row(
                    "SELECT value FROM app_settings WHERE key = 'probe'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(kept, "x");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The upgrade path, which is the one that runs on the NAS: a database
    /// created by an earlier release must gain the newer schema on first open.
    /// Building it by replaying the older migrations, rather than trusting a
    /// fresh one, is the only way this test can fail when it should.
    #[test]
    fn an_older_database_is_brought_up_to_date_on_open() {
        let dir = scratch("upgrade");
        let path = dir.join("planner.sqlite3");

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)",
            )
            .unwrap();
            // Everything except the last migration: this is the shape the
            // previous release left behind.
            for (version, sql) in &MIGRATIONS[..MIGRATIONS.len() - 1] {
                conn.execute_batch(sql).unwrap();
                conn.execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
                    [version],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO users (name, colour, created_at) VALUES ('Ann', '#e11d48', datetime('now'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO boards (name, created_at) VALUES ('Home', datetime('now'))",
                [],
            )
            .unwrap();
        }

        let db = Db::open(&path).unwrap();

        let applied: i64 = db
            .with(|conn| conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(applied, i64::try_from(MIGRATIONS.len()).unwrap());

        // The newest table is usable — and its foreign keys are live, so this
        // only inserts because the user and board above came through the
        // upgrade with it.
        db.with(|conn| {
            conn.execute(
                "INSERT INTO board_views (user_id, board_id, view) VALUES (1, 1, '4w')",
                [],
            )
        })
        .unwrap();
        let name: String = db
            .with(|conn| conn.query_row("SELECT name FROM users WHERE id = 1", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(name, "Ann");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migrations_run_once_and_are_idempotent_across_opens() {
        let dir = scratch("migrate");
        let path = dir.join("planner.sqlite3");

        let applied = |db: &Db| -> i64 {
            db.with(|conn| {
                conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            })
            .unwrap()
        };

        let db = Db::open(&path).unwrap();
        let first = applied(&db);
        assert_eq!(first, i64::try_from(MIGRATIONS.len()).unwrap());
        drop(db);

        // Re-opening an existing database must not try to apply them again.
        let db = Db::open(&path).unwrap();
        assert_eq!(applied(&db), first);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
