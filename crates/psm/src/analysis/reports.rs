//! Reports that are not plain diffs: the system header, `status`,
//! `meminfo`, `cpu`, `timeline`, `trend`.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::diff::{Diff, Status};
use super::group::Grouper;
use crate::model::{Filter, Metric, Proc, Snapshot, add};
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
    let mut t = Table::plain(&[
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

/// The CPU table of a diff, plus the total used and every row's
/// `(name, seconds used)` largest first for the summary and `--brief`.
pub struct CpuTime {
    pub table: Table,
    pub seconds: f64,
    pub wall_seconds: f64,
    pub rows: Vec<(String, f64)>,
    /// Sides without a `%CPU` sample (taken by an older psm or with
    /// `interval = "0"`): BEFORE or AFTER is `-` there and DELTA unknown.
    pub unsampled: Vec<String>,
}

impl CpuTime {
    /// Share of one core over the interval.
    pub fn percent(&self) -> Option<f64> {
        (self.wall_seconds > 0.0).then(|| self.seconds / self.wall_seconds * 100.0)
    }
}

/// CPU between two snapshots, per process (`None`) or per group. BEFORE and
/// AFTER are the `%CPU` sampled at each snapshot (`-` when that side was not
/// sampled or the process was not there), DELTA their difference in points
/// (a new process counts from 0, a gone one to 0, as the memory impact does),
/// USED the CPU seconds consumed in between: a running process its counter
/// delta, a new or restarted one the new instance's whole life (it began
/// after the first snapshot), a gone one unknown (its final counters went
/// with it). Rows idle on both sides that used nothing are left out; the
/// largest change in points comes first, then the most used.
pub fn cpu_table(d: &Diff, g: Option<&Grouper>) -> Result<CpuTime> {
    if !d.same_boot {
        bail!("CPU cannot be compared across a reboot");
    }
    #[derive(Default)]
    struct Row {
        name: String,
        pid: Option<i64>,
        status: &'static str,
        before: Option<f64>,
        after: Option<f64>,
        used: Option<i64>,
    }
    // A missing side is 0 when the process was not there, unknown when that
    // snapshot was not sampled at all.
    let sampled = |s: &Snapshot| s.processes.iter().any(|p| p.cpu_percent.is_some());
    let (a_sampled, b_sampled) = (sampled(d.a), sampled(d.b));
    let delta = |r: &Row| -> Option<f64> {
        let before = r.before.or(a_sampled.then_some(0.0))?;
        let after = r.after.or(b_sampled.then_some(0.0))?;
        Some(after - before)
    };
    let addf = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (x, None) => x,
        (None, y) => y,
    };
    let wall = d.b.uptime_seconds - d.a.uptime_seconds;
    let clk_tck = d.b.clk_tck.max(1);
    let secs = |ticks: i64| ticks as f64 / clk_tck as f64;
    let mut rows: Vec<Row> = match g {
        None => d
            .changes
            .iter()
            .filter_map(|c| {
                let p = c.after.or(c.before)?;
                Some(Row {
                    name: p.comm.clone(),
                    pid: Some(p.pid),
                    status: c.status.label(),
                    before: c.before.and_then(|p| p.cpu_percent),
                    after: c.after.and_then(|p| p.cpu_percent),
                    used: c.cpu_ticks().map(|(b, a)| a - b.unwrap_or(0)),
                })
            })
            .collect(),
        Some(g) => {
            let mut groups: BTreeMap<String, Row> = BTreeMap::new();
            for c in &d.changes {
                let Some(p) = c.after.or(c.before) else {
                    continue;
                };
                let e = groups.entry(g.key(p)).or_default();
                e.before = addf(e.before, c.before.and_then(|p| p.cpu_percent));
                e.after = addf(e.after, c.after.and_then(|p| p.cpu_percent));
                e.used = add(e.used, c.cpu_ticks().map(|(b, a)| a - b.unwrap_or(0)));
            }
            groups
                .into_iter()
                .map(|(key, mut r)| {
                    r.name = g.display(&key);
                    r
                })
                .collect()
        }
    };
    rows.retain(|r| {
        r.before.unwrap_or(0.0).max(r.after.unwrap_or(0.0)) >= 0.05 || r.used.unwrap_or(0) > 0
    });
    rows.sort_by(|x, y| {
        delta(y)
            .map_or(0.0, f64::abs)
            .total_cmp(&delta(x).map_or(0.0, f64::abs))
            .then_with(|| y.used.cmp(&x.used))
            .then_with(|| x.name.cmp(&y.name))
            .then(x.pid.cmp(&y.pid))
    });
    let seconds = secs(rows.iter().filter_map(|r| r.used).sum());
    let mut t = match g {
        None => Table::new(&[
            ("PROCESS", "process"),
            ("PID", "pid"),
            ("STATUS", "status"),
            ("BEFORE", "before"),
            ("AFTER", "after"),
            ("DELTA", "delta"),
            ("USED", "cpu_seconds"),
        ]),
        Some(g) => Table::new(&[
            (g.title(), "name"),
            ("BEFORE", "before"),
            ("AFTER", "after"),
            ("DELTA", "delta"),
            ("USED", "cpu_seconds"),
        ]),
    };
    let pct = |v: Option<f64>| v.map_or(Cell::Dash, |x| Cell::Pct(Some(x)));
    for r in &rows {
        let mut cells = vec![Cell::text(&r.name)];
        if g.is_none() {
            cells.push(Cell::Int(r.pid));
            cells.push(Cell::text(r.status));
        }
        cells.extend([
            pct(r.before),
            pct(r.after),
            Cell::PctDelta(delta(r)),
            r.used.map_or(Cell::Dash, |u| Cell::Pct(Some(secs(u)))),
        ]);
        t.rows.push(cells);
    }
    let mut used: Vec<(String, f64)> = rows
        .iter()
        .filter_map(|r| Some((r.name.clone(), secs(r.used?))))
        .filter(|(_, s)| *s > 0.0)
        .collect();
    used.sort_by(|x, y| y.1.total_cmp(&x.1).then_with(|| x.0.cmp(&y.0)));
    Ok(CpuTime {
        table: t,
        seconds,
        wall_seconds: wall,
        rows: used,
        unsampled: [(a_sampled, d.a), (b_sampled, d.b)]
            .into_iter()
            .filter(|(sampled, _)| !sampled)
            .map(|(_, s)| s.title())
            .collect(),
    })
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

/// One series summarised: how a group's memory moved over the session.
#[derive(Debug, PartialEq)]
pub struct TrendStats {
    pub up: usize,
    pub steps: usize,
    /// Bytes per hour, least squares; `None` when every point has the same time.
    pub slope: Option<i64>,
    pub verdict: &'static str,
}

/// Below this span the slope is not shown: seconds apart, any change reads as
/// terabytes per hour.
const MIN_SLOPE_SPAN_HOURS: f64 = 1.0 / 60.0;

/// `values` per point (0 where the group was absent), `hours` since the first point.
/// Below `min_delta` end to end is `flat`; `growing` or `shrinking` when no step
/// of `min_delta` or more goes against that direction; otherwise `noisy`.
pub fn trend_stats(values: &[i64], hours: &[f64], min_delta: i64) -> TrendStats {
    let steps = values.len().saturating_sub(1);
    let up = values.windows(2).filter(|w| w[1] > w[0]).count();
    let delta = values.last().copied().unwrap_or(0) - values.first().copied().unwrap_or(0);
    let against =
        |w: &[i64]| (w[1] - w[0]).signum() != delta.signum() && (w[1] - w[0]).abs() >= min_delta;
    let verdict = if delta.abs() < min_delta {
        "flat"
    } else if values.windows(2).any(against) {
        "noisy"
    } else if delta > 0 {
        "growing"
    } else {
        "shrinking"
    };
    let n = values.len() as f64;
    let x_mean = hours.iter().sum::<f64>() / n;
    let y_mean = values.iter().map(|&v| v as f64).sum::<f64>() / n;
    let sxx: f64 = hours.iter().map(|x| (x - x_mean).powi(2)).sum();
    let sxy: f64 = hours
        .iter()
        .zip(values)
        .map(|(x, &y)| (x - x_mean) * (y as f64 - y_mean))
        .sum();
    let span = hours.last().copied().unwrap_or(0.0) - hours.first().copied().unwrap_or(0.0);
    let slope = (sxx > 0.0 && span >= MIN_SLOPE_SPAN_HOURS).then(|| (sxy / sxx).round() as i64);
    TrendStats {
        up,
        steps,
        slope,
        verdict,
    }
}

/// One row per group over every point `(epoch seconds, snapshot)` of the
/// session, `now` last. A group absent from a point counts 0 there; one seen
/// at fewer than two points is left out. Largest end-to-end change first,
/// `flat` rows last.
pub fn trend_table(
    points: &[(f64, &Snapshot)],
    g: &Grouper,
    metric: Metric,
    filter: &Filter,
    min_delta: i64,
) -> Table {
    let n = points.len();
    let mut series: BTreeMap<String, Vec<Option<i64>>> = BTreeMap::new();
    for (i, (_, s)) in points.iter().enumerate() {
        for p in s.processes.iter().filter(|p| filter.keep(p)) {
            let slot = &mut series.entry(g.key(p)).or_insert_with(|| vec![None; n])[i];
            *slot = add(*slot, metric.value(p));
        }
    }
    let first_time = points.first().map_or(0.0, |p| p.0);
    let hours: Vec<f64> = points.iter().map(|p| (p.0 - first_time) / 3600.0).collect();
    let mut rows: Vec<(String, Vec<i64>, TrendStats)> = series
        .into_iter()
        .filter(|(_, v)| v.iter().flatten().count() >= 2)
        .map(|(key, v)| {
            let values: Vec<i64> = v.iter().map(|x| x.unwrap_or(0)).collect();
            let stats = trend_stats(&values, &hours, min_delta);
            (key, values, stats)
        })
        .collect();
    rows.sort_by(|a, b| {
        let flat = |r: &(String, Vec<i64>, TrendStats)| r.2.verdict == "flat";
        let delta = |r: &(String, Vec<i64>, TrendStats)| (r.1[n - 1] - r.1[0]).abs();
        flat(a)
            .cmp(&flat(b))
            .then_with(|| delta(b).cmp(&delta(a)))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut t = Table::new(&[
        (g.title(), "name"),
        ("FIRST", "first"),
        ("LAST", "last"),
        ("DELTA", "delta"),
        ("UP", "up"),
        ("SLOPE/H", "slope_per_hour"),
        ("VERDICT", "verdict"),
    ]);
    for (key, values, stats) in rows {
        let (first, last) = (values[0], values[n - 1]);
        t.rows.push(vec![
            Cell::Text(g.display(&key)),
            Cell::Bytes(Some(first)),
            Cell::Bytes(Some(last)),
            Cell::Delta(Some(last - first)),
            Cell::text(format!("{}/{}", stats.up, stats.steps)),
            Cell::Delta(stats.slope),
            Cell::text(stats.verdict),
        ]);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: i64 = 1 << 20;

    #[test]
    fn trend_verdicts_and_slope() {
        let hours = [0.0, 1.0, 2.0, 3.0];
        // Growth with one flat step still counts as growing; least squares gives 90 MiB/h.
        let s = trend_stats(&[100 * MIB, 200 * MIB, 200 * MIB, 400 * MIB], &hours, MIB);
        assert_eq!(s.verdict, "growing");
        assert_eq!((s.up, s.steps), (2, 3));
        assert_eq!(s.slope, Some(90 * MIB));
        // A drop of min_delta or more on the way is noise, whatever the ends say.
        let s = trend_stats(&[100 * MIB, 300 * MIB, 150 * MIB, 400 * MIB], &hours, MIB);
        assert_eq!(s.verdict, "noisy");
        // Below the threshold end to end is flat, even with a small dip.
        let s = trend_stats(
            &[100 * MIB, 100 * MIB - 1, 100 * MIB, 100 * MIB],
            &hours,
            MIB,
        );
        assert_eq!(s.verdict, "flat");
        let s = trend_stats(&[400 * MIB, 300 * MIB, 200 * MIB, 100 * MIB], &hours, MIB);
        assert_eq!((s.verdict, s.slope), ("shrinking", Some(-100 * MIB)));
        // Points seconds apart: no slope, but a verdict.
        let s = trend_stats(&[0, MIB, 2 * MIB], &[0.0, 0.001, 0.002], MIB);
        assert_eq!((s.slope, s.verdict), (None, "growing"));
    }
}
