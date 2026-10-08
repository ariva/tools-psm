//! Sessions: which one is active, switching, importing, deleting.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use super::snapshots::insert_snapshot;
use super::{Db, NOW, SESSION_SQL, Session, session_row};
use crate::model::Snapshot;

/// One exported session: name, description, snapshots.
pub type Export = (String, Option<String>, Vec<Snapshot>);

/// All snapshots of a session with their rows; the session row stays.
fn delete_snapshots_of(conn: &Connection, session_id: i64) -> Result<()> {
    let snaps = "SELECT id FROM snapshots WHERE session_id = ?1";
    for table in ["processes", "snapshot_meminfo", "snapshot_cgroups"] {
        conn.execute(
            &format!("DELETE FROM {table} WHERE snapshot_id IN ({snaps})"),
            [session_id],
        )?;
    }
    conn.execute("DELETE FROM snapshots WHERE session_id = ?1", [session_id])?;
    Ok(())
}

/// Timestamps from a file: keep only those SQLite can read back.
pub(super) fn valid_timestamp(conn: &Connection, t: &str) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT CASE WHEN datetime(?1) IS NOT NULL THEN ?1 END",
        [t],
        |r| r.get(0),
    )?)
}

impl Db {
    /// The one session that is not inactive (`archived_at IS NULL`).
    pub fn active_session(&self) -> Result<Option<Session>> {
        let sql = format!("{SESSION_SQL} WHERE s.archived_at IS NULL ORDER BY s.id DESC LIMIT 1");
        Ok(self.conn.query_row(&sql, [], session_row).optional()?)
    }

    /// `sel` is a session name or id; without it, the active session.
    pub fn session(&self, sel: Option<&str>) -> Result<Session> {
        let Some(sel) = sel else {
            return self
                .active_session()?
                .context("no active session; run `psm new` first");
        };
        let by_name = format!("{SESSION_SQL} WHERE s.name = ?1");
        if let Some(s) = self
            .conn
            .query_row(&by_name, [sel], session_row)
            .optional()?
        {
            return Ok(s);
        }
        let by_id = format!("{SESSION_SQL} WHERE s.id = ?1");
        if let Ok(id) = sel.parse::<i64>()
            && let Some(s) = self.conn.query_row(&by_id, [id], session_row).optional()?
        {
            return Ok(s);
        }
        bail!("no session with name or id {sel:?}; see `psm sessions`")
    }

    pub fn sessions(&self) -> Result<Vec<Session>> {
        let sql = format!("{SESSION_SQL} ORDER BY s.id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], session_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Makes the active session inactive, creates a new one, and stores its baseline,
    /// all in one transaction. Returns the session and the baseline's number (0).
    pub fn init(
        &self,
        name: Option<&str>,
        description: Option<&str>,
        baseline: &Snapshot,
    ) -> Result<(Session, i64)> {
        let tx = self.conn.unchecked_transaction()?;
        let name = match name {
            Some(n) => n.to_string(),
            None => tx.query_row(
                "SELECT 'session-' || strftime('%Y%m%d-%H%M%S','now','localtime')",
                [],
                |r| r.get(0),
            )?,
        };
        let taken: bool = tx.query_row(
            "SELECT count(*) > 0 FROM sessions WHERE name = ?1",
            [&name],
            |r| r.get(0),
        )?;
        if taken {
            bail!("session {name:?} already exists; pick another name");
        }
        tx.execute(
            &format!("UPDATE sessions SET archived_at = {NOW} WHERE archived_at IS NULL"),
            [],
        )?;
        tx.execute(
            &format!("INSERT INTO sessions (name, created_at, notes) VALUES (?1, {NOW}, ?2)"),
            params![name, description],
        )?;
        let session_id = tx.last_insert_rowid();
        let snapshot_id = insert_snapshot(&tx, session_id, Some("baseline"), None, baseline)?;
        tx.commit()?;
        Ok((self.session(Some(&name))?, self.seq(snapshot_id)?))
    }

    /// `psm sessions rename`: names stay unique.
    pub fn rename_session(
        &self,
        session_id: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<()> {
        let taken: bool = self.conn.query_row(
            "SELECT count(*) > 0 FROM sessions WHERE name = ?1 AND id != ?2",
            params![name, session_id],
            |r| r.get(0),
        )?;
        if taken {
            bail!("session {name:?} already exists; pick another name");
        }
        self.conn.execute(
            "UPDATE sessions SET name = ?2, notes = COALESCE(?3, notes) WHERE id = ?1",
            params![session_id, name, description],
        )?;
        Ok(())
    }

    pub fn deactivate(&self, session_id: i64) -> Result<()> {
        self.conn.execute(
            &format!(
                "UPDATE sessions SET archived_at = {NOW} WHERE id = ?1 AND archived_at IS NULL"
            ),
            [session_id],
        )?;
        Ok(())
    }

    /// Makes `session_id` the active session; whichever was active becomes inactive.
    pub fn switch(&self, session_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!(
                "UPDATE sessions SET archived_at = {NOW} WHERE archived_at IS NULL AND id != ?1"
            ),
            [session_id],
        )?;
        tx.execute(
            "UPDATE sessions SET archived_at = NULL WHERE id = ?1",
            [session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_session(&self, session_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        delete_snapshots_of(&tx, session_id)?;
        tx.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        tx.commit()?;
        Ok(())
    }

    /// `psm snapshots reset`: every snapshot of the session goes and `baseline` becomes
    /// its new #0, in one transaction. Returns how many were deleted.
    pub fn restart(&self, session_id: i64, baseline: &Snapshot) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let removed: i64 = tx.query_row(
            "SELECT count(*) FROM snapshots WHERE session_id = ?1",
            [session_id],
            |r| r.get(0),
        )?;
        delete_snapshots_of(&tx, session_id)?;
        insert_snapshot(&tx, session_id, Some("baseline"), None, baseline)?;
        tx.commit()?;
        Ok(removed)
    }

    /// Stores exported sessions under their names, keeping the original
    /// timestamps. They arrive inactive, so they never displace the active session.
    pub fn import(&self, sessions: &[Export]) -> Result<Vec<Session>> {
        let tx = self.conn.unchecked_transaction()?;
        // All names are checked first, so a clash leaves nothing half-imported.
        for (name, _, _) in sessions {
            let taken: bool = tx.query_row(
                "SELECT count(*) > 0 FROM sessions WHERE name = ?1",
                [name],
                |r| r.get(0),
            )?;
            if taken {
                bail!(
                    "session {name:?} already exists; pass --name to import it under another name"
                );
            }
        }
        for (name, description, snapshots) in sessions {
            let started = snapshots
                .first()
                .map(|s| valid_timestamp(&tx, &s.created_at))
                .transpose()?
                .flatten();
            tx.execute(
                &format!(
                    "INSERT INTO sessions (name, created_at, archived_at, notes) VALUES (?1, COALESCE(?2, {NOW}), {NOW}, ?3)"
                ),
                params![name, started, description],
            )?;
            let session_id = tx.last_insert_rowid();
            for s in snapshots {
                insert_snapshot(
                    &tx,
                    session_id,
                    s.label.as_deref(),
                    valid_timestamp(&tx, &s.created_at)?.as_deref(),
                    s,
                )?;
            }
        }
        tx.commit()?;
        sessions
            .iter()
            .map(|(name, _, _)| self.session(Some(name)))
            .collect()
    }

    /// Deletes inactive sessions created before `now - older_than`. The active
    /// session is never touched.
    pub fn purge(&self, older_than: Duration) -> Result<Vec<Session>> {
        let cutoff = format!("-{} seconds", older_than.as_secs());
        let sql = format!(
            "{SESSION_SQL} WHERE s.archived_at IS NOT NULL
               AND s.created_at < strftime('%Y-%m-%dT%H:%M:%SZ','now', ?1) ORDER BY s.id"
        );
        let old: Vec<Session> = self
            .conn
            .prepare(&sql)?
            .query_map([cutoff], session_row)?
            .collect::<rusqlite::Result<_>>()?;
        for s in &old {
            self.delete_session(s.id)?;
        }
        Ok(old)
    }
}
