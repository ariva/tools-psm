//! SQLite storage. This file opens the database; `sessions` and `snapshots`
//! hold the operations, as further `impl Db` blocks.

use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, Row};
use serde::Serialize;

pub mod sessions;
pub mod snapshots;

/// The schema, as the steps that built it. A fresh database runs them all; an
/// older one runs the ones it is missing (`psm update`). `PRAGMA user_version`
/// holds the last applied number.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/0001_initial.sql")),
    (2, include_str!("migrations/0002_seq.sql")),
    (3, include_str!("migrations/0003_description.sql")),
];
pub const SCHEMA_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].0;
pub(crate) const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: i64,
    pub name: String,
    /// Local time, for display.
    pub created: String,
    /// `None` for the active session. Stored in the `archived_at` column.
    pub inactive_since: Option<String>,
    pub snapshots: i64,
    /// Free text given with `psm new`; the `notes` column.
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapMeta {
    /// Row id, for loading. Not shown.
    pub id: i64,
    /// Number within the session, what the user sees: 0 is the baseline.
    /// Stored, so deleting a snapshot never renumbers the others.
    pub seq: i64,
    pub label: Option<String>,
    pub description: Option<String>,
    pub created: String,
    /// Seconds since the epoch, for time axes (`report trend`).
    pub created_epoch: i64,
    pub processes: i64,
    pub deep: bool,
}

const SESSION_SQL: &str = "SELECT s.id, s.name, datetime(s.created_at,'localtime'), s.archived_at,
        (SELECT count(*) FROM snapshots WHERE session_id = s.id), s.notes
     FROM sessions s";

fn session_row(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        name: r.get(1)?,
        created: r.get(2)?,
        inactive_since: r.get(3)?,
        snapshots: r.get(4)?,
        description: r.get(5)?,
    })
}

/// The schema version a database file carries (`PRAGMA user_version`), read
/// without the version check `Db::open` applies.
pub fn file_schema_version(path: &Path) -> Result<i64> {
    let conn = Connection::open(path)
        .with_context(|| format!("cannot open database {}", path.display()))?;
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Applies every migration after `from`, each with its version bump, in one
/// transaction: a failure leaves the file as it was.
fn migrate(conn: &Connection, from: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > from) {
        tx.execute_batch(sql)
            .with_context(|| format!("schema migration to version {version} failed"))?;
        tx.pragma_update(None, "user_version", version)?;
    }
    tx.commit()?;
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if !path.exists() {
            if let Some(dir) = path
                .parent()
                .filter(|d| !d.as_os_str().is_empty() && !d.exists())
            {
                fs::create_dir_all(dir)
                    .with_context(|| format!("cannot create {}", dir.display()))?;
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
            }
            // Command lines can hold secrets: the file is private from the first byte.
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("cannot create database {}", path.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("cannot open database {}", path.display()))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let tables: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
        if version == 0 && tables == 0 {
            migrate(&conn, 0)?;
        } else if version < SCHEMA_VERSION {
            bail!(
                "{} is from an older psm (schema {version}, this one uses {SCHEMA_VERSION}).\n\
                 Run `psm update` to upgrade it in place (a copy is kept), or `psm sessions reset` to start over.",
                path.display()
            );
        } else if version > SCHEMA_VERSION {
            bail!(
                "{} is from a newer psm (schema {version}, this one uses {SCHEMA_VERSION}).\n\
                 Upgrade psm, move the file aside, or pass --db.",
                path.display()
            );
        }
        Ok(Db { conn })
    }

    /// `psm update`: brings a database to the current schema. Returns
    /// `(from, to)`; `from == to` means nothing was done. A copy named
    /// `<file>.v<from>.bak` is written first and left in place.
    pub fn upgrade(path: &Path) -> Result<(i64, i64)> {
        let conn = Connection::open(path)
            .with_context(|| format!("cannot open database {}", path.display()))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            bail!(
                "{} is from a newer psm (schema {version}, this one uses {SCHEMA_VERSION}); upgrade psm instead",
                path.display()
            );
        }
        if version == SCHEMA_VERSION {
            return Ok((version, version));
        }
        let mut copy = path.as_os_str().to_owned();
        copy.push(format!(".v{version}.bak"));
        let copy = PathBuf::from(copy);
        if copy.exists() {
            bail!("{} already exists; move it aside first", copy.display());
        }
        conn.execute(
            "VACUUM INTO ?1",
            [copy.to_str().context("backup path is not valid UTF-8")?],
        )?;
        fs::set_permissions(&copy, fs::Permissions::from_mode(0o600))?;
        migrate(&conn, version)?;
        Ok((version, SCHEMA_VERSION))
    }

    /// The current local time, formatted like the stored timestamps.
    pub fn now_local(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT datetime('now','localtime')", [], |r| r.get(0))?)
    }

    /// Consistent copy of the whole database.
    pub fn backup(&self, path: &Path) -> Result<()> {
        if path.exists() {
            bail!(
                "{} already exists; refusing to overwrite it",
                path.display()
            );
        }
        let target = path.to_str().context("backup path is not valid UTF-8")?;
        self.conn.execute("VACUUM INTO ?1", [target])?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version 1 file gets `seq` from creation order and `description`.
    #[test]
    fn migrations_bring_a_v1_database_to_current() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0].1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        let fixed = "'h', 'b', 'k', 0, 0, 100, 0.0";
        conn.execute_batch(&format!(
            "INSERT INTO sessions (id, name, created_at) VALUES (1, 'a', 't'), (2, 'b', 't');
             INSERT INTO snapshots (id, session_id, created_at, hostname, boot_id, kernel_version,
                 collector_uid, deep, clk_tck, uptime_seconds)
             VALUES (10, 1, 't', {fixed}), (11, 2, 't', {fixed}), (12, 1, 't', {fixed});"
        ))
        .unwrap();
        migrate(&conn, 1).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let seqs: Vec<(i64, i64)> = conn
            .prepare("SELECT id, seq FROM snapshots ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(seqs, [(10, 0), (11, 0), (12, 1)], "numbered per session");
        conn.execute("UPDATE snapshots SET description = 'x' WHERE id = 12", [])
            .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO snapshots (session_id, seq, created_at, hostname, boot_id, kernel_version,
                     collector_uid, deep, clk_tck, uptime_seconds) VALUES (1, 1, 't', 'h', 'b', 'k', 0, 0, 100, 0.0)",
                []
            )
            .is_err(),
            "seq is unique per session"
        );
    }
}
