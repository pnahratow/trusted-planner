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
