//! SQLite storage. This file opens the database; `sessions` and `snapshots`
//! hold the operations, as further `impl Db` blocks.

use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, Row};
use serde::Serialize;

pub mod sessions;
pub mod snapshots;

const SCHEMA_VERSION: i64 = 2;
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
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapMeta {
    /// Row id, for loading. Not shown.
    pub id: i64,
    /// Number within the session, what the user sees: 0 is the baseline.
    /// Stored, so deleting a snapshot never renumbers the others.
    pub seq: i64,
    pub label: Option<String>,
    pub created: String,
    pub processes: i64,
    pub deep: bool,
}

const SESSION_SQL: &str = "SELECT s.id, s.name, datetime(s.created_at,'localtime'), s.archived_at,
        (SELECT count(*) FROM snapshots WHERE session_id = s.id)
     FROM sessions s";

fn session_row(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        name: r.get(1)?,
        created: r.get(2)?,
        inactive_since: r.get(3)?,
        snapshots: r.get(4)?,
    })
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
            conn.execute_batch(include_str!("schema.sql"))?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        } else if version != SCHEMA_VERSION {
            bail!(
                "{} was created by a different version (schema {version}, expected {SCHEMA_VERSION}).\n\
                 Move it aside, pass --db, or run `psm sessions reset` to delete it and start over.",
                path.display()
            );
        }
        Ok(Db { conn })
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
