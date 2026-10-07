//! Comparison of two snapshots: who is new, gone, restarted, and who moved memory.

use std::collections::{BTreeMap, HashMap, VecDeque};

use super::group::Grouper;
use crate::model::{Filter, Metric, Proc, Snapshot, add};
use crate::output::{Cell, Table};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Running,
    Restarted,
    New,
    Gone,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Restarted => "restarted",
            Status::New => "new",
            Status::Gone => "gone",
        }
    }
}

pub struct Change<'a> {
    pub status: Status,
    pub before: Option<&'a Proc>,
    pub after: Option<&'a Proc>,
}

pub struct Diff<'a> {
    pub a: &'a Snapshot,
    pub b: &'a Snapshot,
    /// Across a reboot every process is new by definition; only grouped
    /// comparisons are meaningful and `changes` stays empty.
    pub same_boot: bool,
    pub before: Vec<&'a Proc>,
    pub after: Vec<&'a Proc>,
    pub changes: Vec<Change<'a>>,
    narrowed: bool,
}

/// One row of the memory impact ranking: a process, or a group of them.
pub struct Impact {
    pub name: String,
    pub pid: Option<i64>,
    pub status: &'static str,
    pub n_before: i64,
    pub n_after: i64,
    pub before: Option<i64>,
    pub after: Option<i64>,
    pub delta: i64,
}

impl Impact {
    fn empty() -> Impact {
        Impact {
            name: String::new(),
            pid: None,
            status: "",
            n_before: 0,
            n_after: 0,
            before: None,
            after: None,
            delta: 0,
        }
    }
}

/// Same program: exe + cmdline, falling back to exe alone when the command
/// line was not stored, and to comm when exe was not readable.
fn restart_key(p: &Proc) -> String {
    match (&p.exe, &p.cmdline) {
        (Some(exe), Some(cmdline)) => format!("{exe}\0{cmdline}"),
        (Some(exe), None) => exe.clone(),
        (None, _) => format!("comm:{}", p.comm),
    }
}

fn sort_impact(rows: &mut [Impact]) {
    rows.sort_by(|x, y| {
        y.delta
            .abs()
            .cmp(&x.delta.abs())
            .then_with(|| x.name.cmp(&y.name))
            .then(x.pid.cmp(&y.pid))
    });
}

impl<'a> Diff<'a> {
    pub fn new(a: &'a Snapshot, b: &'a Snapshot, filter: &Filter) -> Diff<'a> {
        let before: Vec<&Proc> = a.processes.iter().filter(|p| filter.keep(p)).collect();
        let after: Vec<&Proc> = b.processes.iter().filter(|p| filter.keep(p)).collect();
        let same_boot = a.boot_id == b.boot_id;
        let mut changes = Vec::new();

        if same_boot {
            // PIDs are reused, so an instance is pid + start time (within one boot).
            let id = |p: &Proc| (p.pid, p.start_time);
            let old: HashMap<_, &Proc> = before.iter().map(|p| (id(p), *p)).collect();
            let cur: HashMap<_, &Proc> = after.iter().map(|p| (id(p), *p)).collect();

            let mut fresh: BTreeMap<String, VecDeque<&Proc>> = BTreeMap::new();
            let mut new: Vec<&Proc> = after
                .iter()
                .copied()
                .filter(|p| !old.contains_key(&id(p)))
                .collect();
            new.sort_by_key(|p| p.start_time);
            for p in new {
                fresh.entry(restart_key(p)).or_default().push_back(p);
            }

            for p in &after {
                if let Some(o) = old.get(&id(p)) {
                    changes.push(Change {
                        status: Status::Running,
                        before: Some(*o),
                        after: Some(*p),
                    });
                }
            }
            let mut gone: Vec<&Proc> = before
                .iter()
                .copied()
                .filter(|p| !cur.contains_key(&id(p)))
                .collect();
            gone.sort_by_key(|p| p.start_time);
            for g in gone {
                // A gone and a new instance of the same program are one restart.
                match fresh.get_mut(&restart_key(g)).and_then(VecDeque::pop_front) {
                    Some(n) => changes.push(Change {
                        status: Status::Restarted,
                        before: Some(g),
                        after: Some(n),
                    }),
                    None => changes.push(Change {
                        status: Status::Gone,
                        before: Some(g),
                        after: None,
                    }),
                }
            }
            for n in fresh.into_values().flatten() {
                changes.push(Change {
                    status: Status::New,
                    before: None,
                    after: Some(n),
                });
            }
        }
        Diff {
            a,
            b,
            same_boot,
            before,
            after,
            changes,
            narrowed: filter.is_narrowing(),
        }
    }

    pub fn count(&self, status: Status) -> usize {
        self.changes.iter().filter(|c| c.status == status).count()
    }

    fn with(&self, status: Status) -> impl Iterator<Item = &Change<'a>> {
        self.changes.iter().filter(move |c| c.status == status)
    }

    /// One ranking over all processes: a running or restarted one counts its
    /// change, a new one its whole memory, a gone one minus its whole memory.
    pub fn impact(&self, metric: Metric) -> Vec<Impact> {
        let mut rows = Vec::new();
        for c in &self.changes {
            let before = c.before.and_then(|p| metric.value(p));
            let after = c.after.and_then(|p| metric.value(p));
            // A value missing on either side is "n/a", never a change from or to zero.
            let delta = match (c.status, before, after) {
                (Status::Running | Status::Restarted, Some(b), Some(a)) => a - b,
                (Status::New, _, Some(a)) => a,
                (Status::Gone, Some(b), _) => -b,
                _ => continue,
            };
            let Some(p) = c.after.or(c.before) else {
                continue;
            };
            rows.push(Impact {
                name: p.comm.clone(),
                pid: Some(p.pid),
                status: c.status.label(),
                n_before: c.before.is_some() as i64,
                n_after: c.after.is_some() as i64,
                before,
                after,
                delta,
            });
        }
        sort_impact(&mut rows);
        rows
    }

    /// The same ranking with one row per group. Returns the rows and the label
    /// of the metric actually used.
    pub fn group_impact(&self, metric: Metric, g: &Grouper) -> (Vec<Impact>, &'static str) {
        let mut groups: BTreeMap<String, Impact> = BTreeMap::new();
        for p in &self.before {
            let e = groups.entry(g.key(p)).or_insert_with(Impact::empty);
            e.n_before += 1;
            e.before = add(e.before, metric.value(p));
        }
        for p in &self.after {
            let e = groups.entry(g.key(p)).or_insert_with(Impact::empty);
            e.n_after += 1;
            e.after = add(e.after, metric.value(p));
        }

        // cgroup v2 keeps an exact total per group; prefer it over summed RSS
        // when the whole group is in view.
        // ponytail: a cgroup without memory.current keeps its summed value in the
        // same ranking. Split the table if such mixed rows ever mislead.
        let mut label = metric.label();
        let current = |s: &'a Snapshot| -> HashMap<&'a str, i64> {
            s.cgroups
                .iter()
                .filter_map(|c| {
                    Some((
                        c.cgroup.as_str(),
                        c.memory_current? + c.swap_current.unwrap_or(0),
                    ))
                })
                .collect()
        };
        let (ca, cb) = (current(self.a), current(self.b));
        if g.kind == "cgroup"
            && metric == Metric::Total
            && !self.narrowed
            && !ca.is_empty()
            && !cb.is_empty()
        {
            label = "cgroup memory.current + swap";
            for (key, e) in groups.iter_mut() {
                if e.n_before > 0 {
                    e.before = ca.get(key.as_str()).copied().or(e.before);
                }
                if e.n_after > 0 {
                    e.after = cb.get(key.as_str()).copied().or(e.after);
                }
            }
        }

        let mut rows: Vec<Impact> = groups
            .into_iter()
            .filter(|(_, e)| e.before.is_some() || e.after.is_some())
            .map(|(key, mut e)| {
                e.name = g.display(&key);
                e.delta = e.after.unwrap_or(0) - e.before.unwrap_or(0);
                e
            })
            .collect();
        sort_impact(&mut rows);
        (rows, label)
    }

    pub fn new_table(&self) -> Table {
        let mut t = Table::new(&[
            ("PID", "pid"),
            ("PPID", "ppid"),
            ("RSS", "rss"),
            ("PROCESS", "process"),
        ]);
        let mut procs: Vec<&Proc> = self.with(Status::New).filter_map(|c| c.after).collect();
        procs.sort_by_key(|p| (std::cmp::Reverse(p.rss_bytes), p.pid));
        for p in procs {
            t.rows.push(vec![
                Cell::Int(Some(p.pid)),
                Cell::Int(Some(p.ppid)),
                Cell::Bytes(p.rss_bytes),
                Cell::text(&p.comm),
            ]);
        }
        t
    }

    pub fn gone_table(&self) -> Table {
        let mut t = Table::new(&[("PID", "pid"), ("RSS", "rss"), ("PROCESS", "process")]);
        let mut procs: Vec<&Proc> = self.with(Status::Gone).filter_map(|c| c.before).collect();
        procs.sort_by_key(|p| (std::cmp::Reverse(p.rss_bytes), p.pid));
        for p in procs {
            t.rows.push(vec![
                Cell::Int(Some(p.pid)),
                Cell::Bytes(p.rss_bytes),
                Cell::text(&p.comm),
            ]);
        }
        t
    }

    pub fn restarted_table(&self, metric: Metric) -> Table {
        let mut t = Table::new(&[
            ("OLD PID", "old_pid"),
            ("NEW PID", "new_pid"),
            ("BEFORE", "before"),
            ("AFTER", "after"),
            ("DELTA", "delta"),
            ("PROCESS", "process"),
        ]);
        for c in self.with(Status::Restarted) {
            let (Some(old), Some(new)) = (c.before, c.after) else {
                continue;
            };
            let (b, a) = (metric.value(old), metric.value(new));
            t.rows.push(vec![
                Cell::Int(Some(old.pid)),
                Cell::Int(Some(new.pid)),
                Cell::Bytes(b),
                Cell::Bytes(a),
                Cell::Delta(b.zip(a).map(|(b, a)| a - b)),
                Cell::text(&new.comm),
            ]);
        }
        t
    }

    /// Process counts per group; `only_changed` hides groups whose count is unchanged.
    pub fn counts_table(&self, g: &Grouper, only_changed: bool) -> Table {
        let mut counts: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for p in &self.before {
            counts.entry(g.key(p)).or_default().0 += 1;
        }
        for p in &self.after {
            counts.entry(g.key(p)).or_default().1 += 1;
        }
        let mut rows: Vec<(String, i64, i64)> = counts
            .into_iter()
            .filter(|(_, (b, a))| !only_changed || a != b)
            .map(|(k, (b, a))| (g.display(&k), b, a))
            .collect();
        rows.sort_by(|x, y| {
            (y.2 - y.1)
                .abs()
                .cmp(&(x.2 - x.1).abs())
                .then(y.2.cmp(&x.2))
                .then_with(|| x.0.cmp(&y.0))
        });
        let mut t = Table::new(&[
            (g.title(), "name"),
            ("BEFORE", "before"),
            ("AFTER", "after"),
            ("DELTA", "delta"),
        ]);
        for (name, b, a) in rows {
            t.rows.push(vec![
                Cell::Text(name),
                Cell::Int(Some(b)),
                Cell::Int(Some(a)),
                Cell::IntDelta(a - b),
            ]);
        }
        t
    }
}

pub fn impact_table(rows: &[Impact], grouped_title: Option<&'static str>) -> Table {
    let bytes = |v: Option<i64>| v.map_or(Cell::Dash, |b| Cell::Bytes(Some(b)));
    match grouped_title {
        None => {
            let mut t = Table::new(&[
                ("PROCESS", "process"),
                ("PID", "pid"),
                ("STATUS", "status"),
                ("BEFORE", "before"),
                ("AFTER", "after"),
                ("DELTA", "delta"),
            ]);
            for r in rows {
                t.rows.push(vec![
                    Cell::text(&r.name),
                    Cell::Int(r.pid),
                    Cell::text(r.status),
                    bytes(r.before),
                    bytes(r.after),
                    Cell::Delta(Some(r.delta)),
                ]);
            }
            t
        }
        Some(title) => {
            let mut t = Table::new(&[
                (title, "name"),
                ("#BEFORE", "count_before"),
                ("#AFTER", "count_after"),
                ("BEFORE", "before"),
                ("AFTER", "after"),
                ("DELTA", "delta"),
            ]);
            for r in rows {
                t.rows.push(vec![
                    Cell::text(&r.name),
                    Cell::Int(Some(r.n_before)),
                    Cell::Int(Some(r.n_after)),
                    bytes(r.before),
                    bytes(r.after),
                    Cell::Delta(Some(r.delta)),
                ]);
            }
            t
        }
    }
}

/// Groups that grew, with the relative change.
pub fn growth_table(rows: &[Impact], title: &'static str) -> Table {
    let mut t = Table::new(&[
        (title, "name"),
        ("BEFORE", "before"),
        ("AFTER", "after"),
        ("DELTA", "delta"),
        ("%", "percent"),
    ]);
    for r in rows.iter().filter(|r| r.delta > 0) {
        let pct = r
            .before
            .filter(|b| *b > 0)
            .map(|b| r.delta as f64 / b as f64 * 100.0);
        t.rows.push(vec![
            Cell::text(&r.name),
            r.before.map_or(Cell::Dash, |b| Cell::Bytes(Some(b))),
            Cell::Bytes(r.after),
            Cell::Delta(Some(r.delta)),
            Cell::Pct(pct),
        ]);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i64, start: i64, comm: &str, rss_mib: i64) -> Proc {
        Proc {
            pid,
            start_time: start,
            comm: comm.into(),
            exe: Some(format!("/usr/bin/{comm}")),
            cmdline: Some(comm.into()),
            rss_bytes: Some(rss_mib << 20),
            ..Default::default()
        }
    }

    fn snap(boot: &str, processes: Vec<Proc>) -> Snapshot {
        Snapshot {
            boot_id: boot.into(),
            processes,
            ..Default::default()
        }
    }

    const ALL: Filter = Filter {
        user: None,
        name: None,
        exe: None,
        pids: Vec::new(),
        cmdline: None,
        search: Vec::new(),
        match_case: false,
        exclude: None,
        kernel: false,
    };

    #[test]
    fn classifies_and_ranks_every_process() {
        let a = snap(
            "b1",
            vec![
                proc(1, 10, "code", 100),
                proc(2, 20, "old", 42),
                proc(3, 30, "ra", 812),
                proc(4, 40, "alpha", 5),
            ],
        );
        let b = snap(
            "b1",
            vec![
                proc(1, 10, "code", 500),
                proc(9, 90, "ra", 344),
                proc(4, 99, "beta", 7),
                proc(7, 70, "node", 72),
            ],
        );
        let d = Diff::new(&a, &b, &ALL);

        assert_eq!(d.count(Status::Running), 1);
        assert_eq!(
            d.count(Status::Restarted),
            1,
            "same exe + cmdline under a new pid"
        );
        assert_eq!(d.count(Status::New), 2, "node, and beta on a reused pid");
        assert_eq!(
            d.count(Status::Gone),
            2,
            "old, and alpha whose pid was reused"
        );

        let rows = d.impact(Metric::Total);
        let got: Vec<(&str, &str, i64)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.status, r.delta >> 20))
            .collect();
        assert_eq!(
            got,
            [
                ("ra", "restarted", -468),
                ("code", "running", 400),
                ("node", "new", 72),
                ("old", "gone", -42),
                ("beta", "new", 7),
                ("alpha", "gone", -5),
            ]
        );
    }

    #[test]
    fn swap_counts_as_memory() {
        let mut swapped = proc(1, 10, "db", 100);
        swapped.rss_bytes = Some(10 << 20);
        swapped.swap_bytes = Some(90 << 20);
        let a = snap("b1", vec![proc(1, 10, "db", 100)]);
        let b = snap("b1", vec![swapped]);
        let rows = Diff::new(&a, &b, &ALL).impact(Metric::Total);
        assert_eq!(rows[0].delta, 0, "swapped out is not freed");
    }

    #[test]
    fn reboot_compares_groups_only() {
        let a = snap("b1", vec![proc(1, 10, "code", 100)]);
        let b = snap(
            "b2",
            vec![proc(1, 10, "code", 150), proc(2, 20, "code", 50)],
        );
        let d = Diff::new(&a, &b, &ALL);
        assert!(!d.same_boot && d.changes.is_empty());
        let g = Grouper::new("name", &[&a, &b]).unwrap();
        let (rows, _) = d.group_impact(Metric::Total, &g);
        assert_eq!(
            (rows[0].n_before, rows[0].n_after, rows[0].delta >> 20),
            (1, 2, 100)
        );
    }

    #[test]
    fn unreadable_value_is_not_a_change() {
        let mut hidden = proc(1, 10, "svc", 100);
        hidden.rss_bytes = None;
        let a = snap("b1", vec![proc(1, 10, "svc", 100)]);
        let b = snap("b1", vec![hidden]);
        assert!(Diff::new(&a, &b, &ALL).impact(Metric::Total).is_empty());
    }
}
