//! Schema migrations.
//!
//! Migrations are embedded and applied in order inside a transaction. The
//! applied version lives in `meta.schema_version`, which is also what a future
//! backup/restore feature will key off.

use rusqlite::{Connection, Result as SqlResult};

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../../migrations/001_initial.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../../migrations/002_senses.sql"),
    },
];

/// The schema version this build expects.
pub fn latest_version() -> i64 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

/// Bring `conn` up to [`latest_version`]. Safe to call on every start.
pub fn migrate(conn: &mut Connection) -> SqlResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    let current = current_version(conn)?;
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [migration.version.to_string()],
        )?;
        tx.commit()?;
        tracing::info!(version = migration.version, "applied migration");
    }
    Ok(())
}

fn current_version(conn: &Connection) -> SqlResult<i64> {
    let has_meta: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
    if !has_meta {
        return Ok(0);
    }
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(version.and_then(|v| v.parse().ok()).unwrap_or(0))
}
