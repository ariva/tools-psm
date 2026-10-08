//! The table of one snapshot: `list`, `show`, and each table of `info`.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, bail};

use super::group::Grouper;
use crate::model::{Filter, Proc, Snapshot, add};
use crate::output::{Cell, Table};

pub const SORTS: &[&str] = &[
    "mem", "cpu", "threads", "swap", "io", "pid", "name", "count",
];

pub struct View {
    pub group: Option<String>,
    pub sort: String,
    pub top: Option<usize>,
    pub deep: bool,
    pub filter: Filter,
}

#[derive(Default, Clone)]
struct Row {
    name: String,
    pid: Option<i64>,
    ppid: Option<i64>,
    user: String,
    count: i64,
    cpu: Option<f64>,
    rss: Option<i64>,
    swap: Option<i64>,
    pss: Option<i64>,
    uss: Option<i64>,
    io: Option<i64>,
    threads: Option<i64>,
    cgroup_mem: Option<i64>,
}

/// `%CPU` from two readings: `delta_ticks / (delta_seconds * CLK_TCK) * 100`,
/// where 100 is one fully used core.
pub fn cpu_percent(delta_ticks: i64, delta_seconds: f64, clk_tck: i64) -> Option<f64> {
    (delta_seconds > 0.0 && clk_tck > 0)
        .then(|| delta_ticks as f64 / (delta_seconds * clk_tck as f64) * 100.0)
}

/// Lifetime average (the `ps` convention): all a single reading can give.
pub fn lifetime_cpu(s: &Snapshot) -> HashMap<i64, f64> {
    s.processes
        .iter()
        .filter_map(|p| {
            let age = s.uptime_seconds - p.start_time as f64 / s.clk_tck as f64;
            Some((
                p.pid,
                cpu_percent(p.cpu_user + p.cpu_system, age, s.clk_tck)?,
            ))
        })
        .collect()
}

fn row(p: &Proc, cpu: &HashMap<i64, f64>) -> Row {
    Row {
        name: p.comm.clone(),
        pid: Some(p.pid),
        ppid: Some(p.ppid),
        user: p
            .username
            .clone()
            .or(p.uid.map(|u| u.to_string()))
            .unwrap_or_else(|| "?".into()),
        count: 1,
        cpu: cpu.get(&p.pid).copied(),
        rss: p.rss_bytes,
        swap: p.swap_bytes,
        pss: p.pss_bytes,
        uss: p.uss_bytes,
        io: add(p.read_bytes, p.write_bytes),
        threads: p.thread_count,
        cgroup_mem: None,
    }
}

/// The process table of one snapshot: filtered, optionally grouped, sorted, cut.
pub fn view_table(s: &Snapshot, cpu: &HashMap<i64, f64>, v: &View) -> Result<Table> {
    if !SORTS.contains(&v.sort.as_str()) {
        bail!(
            "unknown sort column {:?} (expected {})",
            v.sort,
            SORTS.join(", ")
        );
    }
    let procs = s.processes.iter().filter(|p| v.filter.keep(p));
    let grouper = v
        .group
        .as_deref()
        .map(|kind| Grouper::new(kind, &[s]))
        .transpose()?;

    let mut rows: Vec<Row> = match &grouper {
        None => procs.map(|p| row(p, cpu)).collect(),
        Some(g) => {
            let cgroup_mem: HashMap<&str, i64> = s
                .cgroups
                .iter()
                .filter_map(|c| Some((c.cgroup.as_str(), c.memory_current?)))
                .collect();
            let mut groups: BTreeMap<String, Row> = BTreeMap::new();
            for p in procs {
                let key = g.key(p);
                let r = row(p, cpu);
                let e = groups.entry(key.clone()).or_insert_with(|| Row {
                    name: g.display(&key),
                    cgroup_mem: cgroup_mem
                        .get(key.as_str())
                        .copied()
                        .filter(|_| g.kind == "cgroup"),
                    ..Default::default()
                });
                e.count += 1;
                e.cpu = match (e.cpu, r.cpu) {
                    (Some(x), Some(y)) => Some(x + y),
                    (x, y) => x.or(y),
                };
                e.rss = add(e.rss, r.rss);
                e.swap = add(e.swap, r.swap);
                e.pss = add(e.pss, r.pss);
                e.uss = add(e.uss, r.uss);
                e.io = add(e.io, r.io);
                e.threads = add(e.threads, r.threads);
            }
            groups.into_values().collect()
        }
    };

    // Descending for quantities, ascending for identifiers; ties are stable by name, pid.
    rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.pid.cmp(&b.pid)));
    let mem = |r: &Row| if v.deep { r.pss.or(r.rss) } else { r.rss };
    match v.sort.as_str() {
        "mem" => rows.sort_by_key(|r| std::cmp::Reverse(mem(r))),
        "cpu" => rows.sort_by(|a, b| b.cpu.unwrap_or(-1.0).total_cmp(&a.cpu.unwrap_or(-1.0))),
        "threads" => rows.sort_by_key(|r| std::cmp::Reverse(r.threads)),
        "swap" => rows.sort_by_key(|r| std::cmp::Reverse(r.swap)),
        "io" => rows.sort_by_key(|r| std::cmp::Reverse(r.io)),
        "count" => rows.sort_by_key(|r| std::cmp::Reverse(r.count)),
        "pid" => rows.sort_by_key(|r| r.pid),
        _ => {}
    }
    if let Some(n) = v.top {
        rows.truncate(n);
    }

    let cells = |r: &Row| -> Vec<(&'static str, &'static str, Cell)> {
        let pct_mem = r
            .rss
            .filter(|_| s.memory_total > 0)
            .map(|b| b as f64 / s.memory_total as f64 * 100.0);
        let mut c = Vec::new();
        match &grouper {
            // A parent group is "<ppid> <name>": two columns, so the pid is a number.
            Some(g) if g.kind == "parent" => {
                let (ppid, name) = r.name.split_once(' ').unwrap_or((&r.name, "?"));
                c.push(("PPID", "ppid", Cell::Int(ppid.parse().ok())));
                c.push(("PARENT", "parent", Cell::text(name)));
                c.push(("COUNT", "count", Cell::Int(Some(r.count))));
            }
            Some(g) => {
                c.push((g.title(), "name", Cell::text(&r.name)));
                c.push(("COUNT", "count", Cell::Int(Some(r.count))));
            }
            None => {
                c.push(("PID", "pid", Cell::Int(r.pid)));
                c.push(("PPID", "ppid", Cell::Int(r.ppid)));
                c.push(("USER", "user", Cell::text(&r.user)));
            }
        }
        c.push(("%CPU", "cpu_percent", Cell::Pct(r.cpu)));
        c.push(("%MEM", "mem_percent", Cell::Pct(pct_mem)));
        if grouper.as_ref().is_some_and(|g| g.kind == "cgroup") {
            c.push(("CGROUP MEM", "cgroup_memory", Cell::Bytes(r.cgroup_mem)));
        }
        c.push(("RSS", "rss", Cell::Bytes(r.rss)));
        if v.sort == "swap" {
            c.push(("SWAP", "swap", Cell::Bytes(r.swap)));
        }
        if v.sort == "io" {
            c.push(("IO", "io", Cell::Bytes(r.io)));
        }
        if v.deep {
            c.push(("PSS", "pss", Cell::Bytes(r.pss)));
            c.push(("USS", "uss", Cell::Bytes(r.uss)));
        }
        c.push(("THR", "threads", Cell::Int(r.threads)));
        if grouper.is_none() {
            c.push(("COMMAND", "command", Cell::text(&r.name)));
        }
        c
    };
    let cols: Vec<_> = cells(&Row::default())
        .into_iter()
        .map(|(t, k, _)| (t, k))
        .collect();
    let mut table = Table::new(&cols);
    table.rows = rows
        .iter()
        .map(|r| cells(r).into_iter().map(|c| c.2).collect())
        .collect();
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_is_per_core() {
        // 50 ticks in half a second at 100 Hz is one full core.
        assert_eq!(cpu_percent(50, 0.5, 100), Some(100.0));
        assert_eq!(cpu_percent(150, 0.5, 100), Some(300.0));
        assert_eq!(cpu_percent(0, 0.5, 100), Some(0.0));
        assert_eq!(cpu_percent(10, 0.0, 100), None);
    }
}
