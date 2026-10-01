//! Snapshots: storing, listing, resolving references, loading.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use super::{Db, NOW, Session, SnapMeta};
use crate::model::{CgroupMem, Proc, Snapshot};

impl Db {
    /// A snapshot is written atomically: a partial one never becomes visible.
    pub fn snap(&self, session_id: i64, label: Option<&str>, snapshot: &Snapshot) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let id = insert_snapshot(&tx, session_id, label, None, snapshot)?;
        tx.commit()?;
        Ok(id)
    }

    /// `kernel` decides whether kernel threads count as processes.
    pub fn snapshots(&self, session_id: i64, kernel: bool) -> Result<Vec<SnapMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, label, datetime(created_at,'localtime'), deep,
                    (SELECT count(*) FROM processes WHERE snapshot_id = snapshots.id AND kthread <= ?2)
             FROM snapshots WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![session_id, kernel], |r| {
                Ok(SnapMeta {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    created: r.get(2)?,
                    deep: r.get(3)?,
                    processes: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// `baseline`, `latest`, `prev`, a snapshot id, or a label (newest match).
    pub fn resolve(&self, session: &Session, reference: &str) -> Result<i64> {
        let pick = |sql: &str| -> Result<Option<i64>> {
            Ok(self
                .conn
                .query_row(sql, [session.id], |r| r.get(0))
                .optional()?)
        };
        let found = match reference {
            "baseline" => {
                pick("SELECT id FROM snapshots WHERE session_id = ?1 ORDER BY id LIMIT 1")?
            }
            "latest" => {
                pick("SELECT id FROM snapshots WHERE session_id = ?1 ORDER BY id DESC LIMIT 1")?
            }
            "prev" => {
                let prev = pick(
                    "SELECT id FROM snapshots WHERE session_id = ?1 ORDER BY id DESC LIMIT 1 OFFSET 1",
                )?;
                if prev.is_none() {
                    bail!(
                        "session {:?} has only one snapshot; run `psm snap` first",
                        session.name
                    );
                }
                prev
            }
            other => {
                let by_id = match other.parse::<i64>() {
                    Ok(id) => self
                        .conn
                        .query_row("SELECT id FROM snapshots WHERE id = ?1", [id], |r| r.get(0))
                        .optional()?,
                    Err(_) => None,
                };
                match by_id {
                    Some(id) => Some(id),
                    None => self
                        .conn
                        .query_row(
                            "SELECT id FROM snapshots WHERE session_id = ?1 AND label = ?2
                             ORDER BY id DESC LIMIT 1",
                            params![session.id, other],
                            |r| r.get(0),
                        )
                        .optional()?,
                }
            }
        };
        found.with_context(|| {
            format!(
                "no snapshot {reference:?} in session {:?}; see `psm snapshots`",
                session.name
            )
        })
    }

    /// The snapshot of the session taken just before `snapshot_id`.
    pub fn before(&self, session: &Session, snapshot_id: i64) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT id FROM snapshots WHERE session_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT 1",
                params![session.id, snapshot_id],
                |r| r.get(0),
            )
            .optional()?
            .with_context(|| {
                format!("there is no snapshot before #{snapshot_id} in session {:?}", session.name)
            })
    }

    pub fn load(&self, snapshot_id: i64) -> Result<Snapshot> {
        let mut snap = self.conn.query_row(
            "SELECT id, label, created_at, hostname, boot_id, kernel_version, collector_uid, deep,
                    clk_tck, uptime_seconds, load_1, load_5, load_15,
                    memory_total, memory_available, swap_total, swap_used
             FROM snapshots WHERE id = ?1",
            [snapshot_id],
            |r| {
                Ok(Snapshot {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    created_at: r.get(2)?,
                    hostname: r.get(3)?,
                    boot_id: r.get(4)?,
                    kernel_version: r.get(5)?,
                    collector_uid: r.get(6)?,
                    deep: r.get(7)?,
                    clk_tck: r.get(8)?,
                    uptime_seconds: r.get(9)?,
                    load_1: r.get(10)?,
                    load_5: r.get(11)?,
                    load_15: r.get(12)?,
                    memory_total: r.get(13)?,
                    memory_available: r.get(14)?,
                    swap_total: r.get(15)?,
                    swap_used: r.get(16)?,
                    ..Default::default()
                })
            },
        )?;

        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM snapshot_meminfo WHERE snapshot_id = ?1")?;
        snap.meminfo = stmt
            .query_map([snapshot_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT cgroup, memory_current, swap_current FROM snapshot_cgroups
             WHERE snapshot_id = ?1 ORDER BY cgroup",
        )?;
        snap.cgroups = stmt
            .query_map([snapshot_id], |r| {
                Ok(CgroupMem {
                    cgroup: r.get(0)?,
                    memory_current: r.get(1)?,
                    swap_current: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT pid, ppid, uid, username, comm, exe, cmdline, state, kthread, start_time,
                    rss_bytes, rss_anon_bytes, rss_file_bytes, rss_shmem_bytes, swap_bytes, vsz_bytes,
                    pss_bytes, uss_bytes, cpu_user, cpu_system, thread_count, nice,
                    read_bytes, write_bytes, cgroup
             FROM processes WHERE snapshot_id = ?1 ORDER BY pid",
        )?;
        snap.processes = stmt
            .query_map([snapshot_id], |r| {
                Ok(Proc {
                    pid: r.get(0)?,
                    ppid: r.get(1)?,
                    uid: r.get(2)?,
                    username: r.get(3)?,
                    comm: r.get(4)?,
                    exe: r.get(5)?,
                    cmdline: r.get(6)?,
                    state: r.get(7)?,
                    kthread: r.get(8)?,
                    start_time: r.get(9)?,
                    rss_bytes: r.get(10)?,
                    rss_anon_bytes: r.get(11)?,
                    rss_file_bytes: r.get(12)?,
                    rss_shmem_bytes: r.get(13)?,
                    swap_bytes: r.get(14)?,
                    vsz_bytes: r.get(15)?,
                    pss_bytes: r.get(16)?,
                    uss_bytes: r.get(17)?,
                    cpu_user: r.get(18)?,
                    cpu_system: r.get(19)?,
                    thread_count: r.get(20)?,
                    nice: r.get(21)?,
                    read_bytes: r.get(22)?,
                    write_bytes: r.get(23)?,
                    cgroup: r.get(24)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(snap)
    }
}

/// `created_at` is given for imported snapshots; a fresh one is stamped now.
pub(super) fn insert_snapshot(
    conn: &Connection,
    session_id: i64,
    label: Option<&str>,
    created_at: Option<&str>,
    s: &Snapshot,
) -> Result<i64> {
    conn.execute(
        &format!(
            "INSERT INTO snapshots (session_id, created_at, label, hostname, boot_id, kernel_version,
                collector_uid, deep, clk_tck, uptime_seconds, load_1, load_5, load_15,
                memory_total, memory_available, swap_total, swap_used)
             VALUES (?1, COALESCE(?17, {NOW}), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"
        ),
        params![
            session_id,
            label,
            s.hostname,
            s.boot_id,
            s.kernel_version,
            s.collector_uid,
            s.deep,
            s.clk_tck,
            s.uptime_seconds,
            s.load_1,
            s.load_5,
            s.load_15,
            s.memory_total,
            s.memory_available,
            s.swap_total,
            s.swap_used,
            created_at
        ],
    )?;
    let id = conn.last_insert_rowid();

    let mut stmt =
        conn.prepare("INSERT INTO snapshot_meminfo (snapshot_id, key, value) VALUES (?1, ?2, ?3)")?;
    for (key, value) in &s.meminfo {
        stmt.execute(params![id, key, value])?;
    }
    let mut stmt = conn.prepare(
        "INSERT INTO snapshot_cgroups (snapshot_id, cgroup, memory_current, swap_current)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for c in &s.cgroups {
        stmt.execute(params![id, c.cgroup, c.memory_current, c.swap_current])?;
    }
    let mut stmt = conn.prepare(
        "INSERT INTO processes (snapshot_id, pid, ppid, uid, username, comm, exe, cmdline, state,
            kthread, start_time, rss_bytes, rss_anon_bytes, rss_file_bytes, rss_shmem_bytes,
            swap_bytes, vsz_bytes, pss_bytes, uss_bytes, cpu_user, cpu_system, thread_count, nice,
            read_bytes, write_bytes, cgroup)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
            ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
    )?;
    for p in &s.processes {
        stmt.execute(params![
            id,
            p.pid,
            p.ppid,
            p.uid,
            p.username,
            p.comm,
            p.exe,
            p.cmdline,
            p.state,
            p.kthread,
            p.start_time,
            p.rss_bytes,
            p.rss_anon_bytes,
            p.rss_file_bytes,
            p.rss_shmem_bytes,
            p.swap_bytes,
            p.vsz_bytes,
            p.pss_bytes,
            p.uss_bytes,
            p.cpu_user,
            p.cpu_system,
            p.thread_count,
            p.nice,
            p.read_bytes,
            p.write_bytes,
            p.cgroup
        ])?;
    }
    Ok(id)
}
