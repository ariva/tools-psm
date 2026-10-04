//! Reports that are not plain diffs: the system header, `status`,
//! `meminfo`, `cpu`, `timeline`.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::diff::{Diff, Status};
use super::group::Grouper;
use super::view::cpu_percent;
use crate::model::{Filter, Proc, Snapshot, add};
use crate::output::{Cell, Table, human, human_delta};
use crate::store::{Db, Session};

/// System header of `psm info`, as text lines and as JSON.
pub fn system_header(s: &Snapshot, filter: &Filter) -> (String, Value) {
    let shown: Vec<&Proc> = s.processes.iter().filter(|p| filter.keep(p)).collect();
    let kthreads = s.processes.iter().filter(|p| p.kthread).count();
    let threads: i64 = shown.iter().filter_map(|p| p.thread_count).sum();
    let text = format!(
        "Memory:     {} used / {}    available {}\n\
         Swap:       {} used / {}\n\
         Load:       {:.2}  {:.2}  {:.2}\n\
         Processes:  {}    Threads: {}{}",
        human(s.used()),
        human(s.memory_total),
        human(s.memory_available),
        human(s.swap_used),
        human(s.swap_total),
        s.load_1,
        s.load_5,
        s.load_15,
        shown.len(),
        threads,
        if filter.kernel {
            String::new()
        } else {
            format!(
                "    ({kthreads} kernel thread{} hidden)",
                if kthreads == 1 { "" } else { "s" }
            )
        },
    );
    let json = json!({
        "memory_total": s.memory_total,
        "memory_used": s.used(),
        "memory_available": s.memory_available,
        "swap_total": s.swap_total,
        "swap_used": s.swap_used,
        "load": [s.load_1, s.load_5, s.load_15],
        "processes": shown.len(),
        "threads": threads,
        "kernel_threads": kthreads,
    });
    (text, json)
}

/// Bare `psm` / `psm status`: the active session at a glance.
pub fn status(db: &Db, session: &Session, filter: &Filter) -> Result<(String, Value)> {
    let snaps = db.snapshots(session.id, filter.kernel)?;
    let (Some(first), Some(last)) = (snaps.first(), snaps.last()) else {
        bail!("session {:?} has no snapshots", session.name);
    };
    let (a, b) = (db.load(first.id)?, db.load(last.id)?);
    let d = Diff::new(&a, &b, filter);
    let delta = b.used() - a.used();
    // ` (description) (inactive)`: either part only when it applies.
    let mut suffix = session
        .description
        .as_deref()
        .filter(|d| !d.is_empty())
        .map(|d| format!(" ({d})"))
        .unwrap_or_default();
    if session.inactive_since.is_some() {
        suffix.push_str(" (inactive)");
    }
    let text = format!(
        "Session: {}{}\nStarted: {}\nSnapshots: {}\n\n\
         Baseline: {}\nLatest:   {}\n\n\
         Processes:\n  baseline: {:>5}\n  current:  {:>5}\n  new:      {:>5}\n  gone:     {:>5}\n  restarted:{:>5}\n\n\
         Memory:\n  baseline used: {}\n  current used:  {}\n  delta:         {}",
        session.name,
        suffix,
        session.created,
        snaps.len(),
        a.title(),
        b.title(),
        d.before.len(),
        d.after.len(),
        d.count(Status::New),
        d.count(Status::Gone),
        d.count(Status::Restarted),
        human(a.used()),
        human(b.used()),
        human_delta(delta),
    );
    let json = json!({
        "session": session,
        "baseline": a.id,
        "latest": b.id,
        "processes": {
            "baseline": d.before.len(),
            "current": d.after.len(),
            "new": d.count(Status::New),
            "gone": d.count(Status::Gone),
            "restarted": d.count(Status::Restarted),
        },
        "memory": { "baseline_used": a.used(), "current_used": b.used(), "delta": delta },
    });
    Ok((text, json))
}

/// Every `/proc/meminfo` field that changed: where memory went when no process grew.
pub fn meminfo_table(a: &Snapshot, b: &Snapshot) -> Table {
    let mut rows: Vec<(&String, i64, i64)> = a
        .meminfo
        .iter()
        .filter_map(|(k, before)| Some((k, *before, *b.meminfo.get(k)?)))
        .filter(|(_, before, after)| before != after)
        .collect();
    rows.sort_by_key(|(k, before, after)| (std::cmp::Reverse((after - before).abs()), *k));
    let mut t = Table::new(&[
        ("FIELD", "field"),
        ("BEFORE", "before"),
        ("AFTER", "after"),
        ("DELTA", "delta"),
    ]);
    for (key, before, after) in rows {
        // HugePages_* are page counts, not bytes.
        t.rows.push(if key.starts_with("HugePages_") {
            vec![
                Cell::text(key),
                Cell::Int(Some(before)),
                Cell::Int(Some(after)),
                Cell::IntDelta(after - before),
            ]
        } else {
            vec![
                Cell::text(key),
                Cell::Bytes(Some(before)),
                Cell::Bytes(Some(after)),
                Cell::Delta(Some(after - before)),
            ]
        });
    }
    t
}

/// CPU time used between two snapshots, per group.
pub fn cpu_table(d: &Diff, g: &Grouper) -> Result<Table> {
    if !d.same_boot {
        bail!("CPU time cannot be compared across a reboot");
    }
    let wall = d.b.uptime_seconds - d.a.uptime_seconds;
    let mut ticks: BTreeMap<String, i64> = BTreeMap::new();
    for c in &d.changes {
        // A gone process took its final counters with it; only what is still visible counts.
        let Some(after) = c.after else { continue };
        let before = c.before.filter(|_| c.status == Status::Running);
        let used =
            after.cpu_user + after.cpu_system - before.map_or(0, |p| p.cpu_user + p.cpu_system);
        *ticks.entry(g.key(after)).or_default() += used;
    }
    let mut rows: Vec<(String, i64)> = ticks.into_iter().filter(|(_, t)| *t > 0).collect();
    rows.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| x.0.cmp(&y.0)));
    let mut t = Table::new(&[
        (g.title(), "name"),
        ("CPU SEC", "cpu_seconds"),
        ("%CPU", "cpu_percent"),
    ]);
    for (key, used) in rows {
        t.rows.push(vec![
            Cell::Text(g.display(&key)),
            Cell::Pct(Some(used as f64 / d.b.clk_tck as f64)),
            Cell::Pct(cpu_percent(used, wall, d.b.clk_tck)),
        ]);
    }
    Ok(t)
}

/// One row per snapshot of the session; with a filter, also what it matched.
pub fn timeline_table(db: &Db, session: &Session, filter: &Filter) -> Result<Table> {
    let mut cols = vec![
        ("ID", "id"),
        ("LABEL", "label"),
        ("TIME", "time"),
        ("PROCS", "processes"),
        ("USED", "memory_used"),
        ("SWAP", "swap_used"),
    ];
    if filter.is_narrowing() {
        cols.push(("MATCHED RSS+SWAP", "matched_memory"));
    }
    cols.push(("DESCRIPTION", "description"));
    let mut t = Table::new(&cols);
    for meta in db.snapshots(session.id, filter.kernel)? {
        let s = db.load(meta.id)?;
        let shown: Vec<&Proc> = s.processes.iter().filter(|p| filter.keep(p)).collect();
        let mut row = vec![
            Cell::Int(Some(s.id)),
            Cell::text(meta.label.unwrap_or_default()),
            Cell::text(meta.created),
            Cell::Int(Some(shown.len() as i64)),
            Cell::Bytes(Some(s.used())),
            Cell::Bytes(Some(s.swap_used)),
        ];
        if filter.is_narrowing() {
            let matched = shown.iter().fold(None, |acc, p| {
                add(acc, crate::model::Metric::Total.value(p))
            });
            row.push(Cell::Bytes(matched));
        }
        row.push(Cell::text(meta.description.unwrap_or_default()));
        t.rows.push(row);
    }
    Ok(t)
}
