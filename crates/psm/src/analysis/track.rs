//! `psm track`: which processes belong to the tracked program, one sample
//! of them, and the statistics over a run and over several runs.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Map, Value, json};

use super::group::Grouper;
use crate::model::{Filter, Proc, Snapshot, add};

/// What is being followed.
pub enum Target {
    /// A pid (spawned or given) and every descendant.
    Tree(Vec<i64>),
    /// The processes a filter keeps, rolled up to their group (`app` by
    /// default) so helpers the words do not name still count.
    Words {
        filter: Filter,
        group: Option<String>,
    },
}

impl Target {
    /// The processes of the snapshot that belong to the target.
    pub fn members<'a>(&self, s: &'a Snapshot) -> Vec<&'a Proc> {
        match self {
            Target::Tree(roots) => tree(s, roots),
            Target::Words { filter, group } => {
                let matched: Vec<&Proc> = s
                    .processes
                    .iter()
                    .filter(|p| alive(p) && filter.keep(p))
                    .collect();
                let Some(kind) = group else {
                    return matched;
                };
                let Ok(g) = Grouper::new(kind, &[s]) else {
                    return matched;
                };
                let keys: HashSet<String> = matched.iter().map(|p| g.key(p)).collect();
                s.processes
                    .iter()
                    .filter(|p| alive(p) && keys.contains(&g.key(p)))
                    .collect()
            }
        }
    }
}

/// A zombie has exited: no memory, no threads, nothing left to follow.
fn alive(p: &Proc) -> bool {
    !p.kthread && p.state != "Z"
}

/// The roots and every process whose parent chain reaches one of them.
pub fn tree<'a>(s: &'a Snapshot, roots: &[i64]) -> Vec<&'a Proc> {
    let by_pid: HashMap<i64, &Proc> = s.processes.iter().map(|p| (p.pid, p)).collect();
    s.processes
        .iter()
        .filter(|p| alive(p))
        .filter(|p| {
            let mut current = *p;
            // The bound only guards against a corrupt parent loop.
            for _ in 0..64 {
                if roots.contains(&current.pid) {
                    return true;
                }
                match by_pid.get(&current.ppid) {
                    Some(parent) if parent.pid != current.pid => current = parent,
                    _ => return false,
                }
            }
            false
        })
        .collect()
}

/// The machine at one sample.
#[derive(Clone, Debug)]
pub struct System {
    pub mem_available: i64,
    pub mem_used: i64,
    pub swap_used: i64,
    /// Whole-machine CPU over the period, 100 = every core busy; `None` on
    /// the first sample or without `/proc/stat`.
    pub cpu: Option<f64>,
    pub load_1: f64,
}

/// The target at one sample.
#[derive(Clone, Debug)]
pub struct Sample {
    pub t_ms: u64,
    pub rss: Option<i64>,
    pub anon: Option<i64>,
    pub swap: Option<i64>,
    /// Over the period since the previous sample; `None` on the first.
    pub cpu: Option<f64>,
    pub threads: Option<i64>,
    pub procs: usize,
    pub system: System,
}

/// CPU ticks per process instance, the "before" reading of the next sample.
pub type Ticks = HashMap<(i64, i64), i64>;

/// `(busy, total)` ticks of the whole machine, from the first line of `/proc/stat`.
pub type SystemTicks = (i64, i64);

/// What a sample needs from the one before it.
pub struct Previous {
    pub ticks: Ticks,
    pub system: Option<SystemTicks>,
    pub t_ms: u64,
}

impl Sample {
    pub fn new(
        t_ms: u64,
        s: &Snapshot,
        members: &[&Proc],
        system_ticks: Option<SystemTicks>,
        previous: Option<&Previous>,
    ) -> (Sample, Previous) {
        let sum = |f: fn(&Proc) -> Option<i64>| members.iter().fold(None, |acc, p| add(acc, f(p)));
        let ticks: Ticks = members
            .iter()
            .map(|p| ((p.pid, p.start_time), p.cpu_user + p.cpu_system))
            .collect();
        let cpu = previous.and_then(|prev| {
            let seconds = (t_ms.saturating_sub(prev.t_ms)) as f64 / 1000.0;
            // A member that appeared since the last sample counts from zero:
            // its ticks all fall inside the period anyway.
            let used: i64 = ticks
                .iter()
                .map(|(id, now)| now - prev.ticks.get(id).copied().unwrap_or(0))
                .sum();
            super::view::cpu_percent(used, seconds, s.clk_tck)
        });
        let system_cpu = match (system_ticks, previous.and_then(|p| p.system)) {
            (Some((busy, total)), Some((busy0, total0))) if total > total0 => {
                Some((busy - busy0) as f64 / (total - total0) as f64 * 100.0)
            }
            _ => None,
        };
        let sample = Sample {
            t_ms,
            rss: sum(|p| p.rss_bytes),
            anon: sum(|p| p.rss_anon_bytes),
            swap: sum(|p| p.swap_bytes),
            cpu,
            threads: sum(|p| p.thread_count),
            procs: members.len(),
            system: System {
                mem_available: s.memory_available,
                mem_used: s.used(),
                swap_used: s.swap_used,
                cpu: system_cpu,
                load_1: s.load_1,
            },
        };
        let previous = Previous {
            ticks,
            system: system_ticks,
            t_ms,
        };
        (sample, previous)
    }

    pub fn json(&self) -> Value {
        json!({
            "t_ms": self.t_ms,
            "target": {
                "rss": self.rss, "anon": self.anon, "swap": self.swap,
                "cpu": self.cpu.map(round1), "threads": self.threads, "procs": self.procs,
            },
            "system": {
                "mem_available": self.system.mem_available,
                "mem_used": self.system.mem_used,
                "swap_used": self.system.swap_used,
                "cpu": self.system.cpu.map(round1),
                "load_1": self.system.load_1,
            },
        })
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// How a figure prints: bytes, a percentage, or a count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bytes,
    Pct,
    Count,
}

/// Min, max, average and last of one figure over the samples kept, and
/// when the maximum was seen.
#[derive(Clone, Debug, PartialEq)]
pub struct Stat {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub last: f64,
    pub at_max_ms: u64,
}

impl Stat {
    /// `None` when no sample had the figure.
    pub fn of(points: impl Iterator<Item = (u64, Option<f64>)>) -> Option<Stat> {
        let mut stat: Option<Stat> = None;
        let (mut sum, mut n) = (0.0, 0u64);
        for (t, v) in points {
            let Some(v) = v else { continue };
            sum += v;
            n += 1;
            stat = Some(match stat {
                None => Stat {
                    min: v,
                    max: v,
                    avg: v,
                    last: v,
                    at_max_ms: t,
                },
                Some(mut s) => {
                    s.min = s.min.min(v);
                    if v > s.max {
                        s.max = v;
                        s.at_max_ms = t;
                    }
                    s.last = v;
                    s
                }
            });
        }
        stat.map(|mut s| {
            s.avg = sum / n as f64;
            s
        })
    }

    pub fn json(&self, kind: Kind) -> Value {
        let v = |x: f64| match kind {
            Kind::Bytes => json!(x.round() as i64),
            Kind::Count => json!(x.round() as i64),
            Kind::Pct => json!(round1(x)),
        };
        let mut m = Map::new();
        m.insert("min".into(), v(self.min));
        m.insert("max".into(), v(self.max));
        m.insert(
            "avg".into(),
            match kind {
                Kind::Bytes => json!(self.avg.round() as i64),
                _ => json!(round1(self.avg)),
            },
        );
        m.insert("last".into(), v(self.last));
        m.insert("at_max_ms".into(), json!(self.at_max_ms));
        Value::Object(m)
    }
}

/// The rows of the per-run table, in order.
pub const ROWS: [(&str, Kind); 6] = [
    ("rss", Kind::Bytes),
    ("anon", Kind::Bytes),
    ("swap", Kind::Bytes),
    ("cpu", Kind::Pct),
    ("threads", Kind::Count),
    ("procs", Kind::Count),
];

/// The system rows, after the target's.
pub const SYSTEM_ROWS: [(&str, Kind); 3] = [
    ("mem_available", Kind::Bytes),
    ("mem_used", Kind::Bytes),
    ("cpu", Kind::Pct),
];

/// The statistics of one run: every row of `ROWS` and `SYSTEM_ROWS` over the
/// samples after the warmup.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub target: BTreeMap<&'static str, Option<Stat>>,
    pub system: BTreeMap<&'static str, Option<Stat>>,
    pub samples: usize,
    pub dropped: usize,
}

impl Stats {
    pub fn of(samples: &[Sample], warmup_ms: u64) -> Stats {
        let kept: Vec<&Sample> = samples.iter().filter(|s| s.t_ms >= warmup_ms).collect();
        let pick =
            |f: &dyn Fn(&Sample) -> Option<f64>| Stat::of(kept.iter().map(|s| (s.t_ms, f(s))));
        let int = |v: Option<i64>| v.map(|x| x as f64);
        let target = BTreeMap::from([
            ("rss", pick(&|s| int(s.rss))),
            ("anon", pick(&|s| int(s.anon))),
            ("swap", pick(&|s| int(s.swap))),
            ("cpu", pick(&|s| s.cpu)),
            ("threads", pick(&|s| int(s.threads))),
            ("procs", pick(&|s| Some(s.procs as f64))),
        ]);
        let system = BTreeMap::from([
            (
                "mem_available",
                pick(&|s| Some(s.system.mem_available as f64)),
            ),
            ("mem_used", pick(&|s| Some(s.system.mem_used as f64))),
            ("cpu", pick(&|s| s.system.cpu)),
        ]);
        Stats {
            target,
            system,
            samples: samples.len(),
            dropped: samples.len() - kept.len(),
        }
    }

    pub fn json(&self) -> Value {
        let block = |rows: &[(&str, Kind)], stats: &BTreeMap<&str, Option<Stat>>| {
            let mut m = Map::new();
            for (name, kind) in rows {
                m.insert(
                    name.to_string(),
                    stats
                        .get(name)
                        .and_then(|s| s.as_ref())
                        .map_or(Value::Null, |s| s.json(*kind)),
                );
            }
            Value::Object(m)
        };
        let mut m = block(&ROWS, &self.target);
        m["system"] = block(&SYSTEM_ROWS, &self.system);
        m
    }
}

/// One finished run, as the across-runs statistics see it.
#[derive(Clone, Debug)]
pub struct RunSummary {
    pub run: u32,
    pub wall_ms: u64,
    pub skipped: bool,
    pub stats: Stats,
}

/// Min, max, average and spread of one per-run figure over the runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Spread {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub spread: f64,
}

impl Spread {
    pub fn of(values: impl Iterator<Item = f64>) -> Option<Spread> {
        let v: Vec<f64> = values.collect();
        if v.is_empty() {
            return None;
        }
        let min = v.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Some(Spread {
            min,
            max,
            avg: v.iter().sum::<f64>() / v.len() as f64,
            spread: max - min,
        })
    }

    pub fn json(&self, kind: Kind) -> Value {
        let v = |x: f64| match kind {
            Kind::Pct => json!(round1(x)),
            _ => json!(x.round() as i64),
        };
        json!({ "min": v(self.min), "max": v(self.max), "avg": v(self.avg), "spread": v(self.spread) })
    }
}

/// The rows of the FINAL block: `(name, kind, which run figure)`.
pub const FINAL_ROWS: [(&str, Kind); 6] = [
    ("rss_max", Kind::Bytes),
    ("anon_max", Kind::Bytes),
    ("swap_max", Kind::Bytes),
    ("cpu_avg", Kind::Pct),
    ("threads_max", Kind::Count),
    ("procs_max", Kind::Count),
];

/// Across the runs that were not skipped.
#[derive(Clone, Debug, Default)]
pub struct Final {
    pub runs: Vec<u32>,
    pub wall_ms: Option<Spread>,
    pub rows: BTreeMap<&'static str, Option<Spread>>,
    pub system: BTreeMap<&'static str, Option<Spread>>,
}

impl Final {
    pub fn of(runs: &[RunSummary]) -> Final {
        let kept: Vec<&RunSummary> = runs.iter().filter(|r| !r.skipped).collect();
        let figure = |row: &str| -> (&'static str, fn(&Stat) -> f64) {
            match row.rsplit_once('_') {
                Some((name, "max")) => (leak(name), |s| s.max),
                Some((name, "avg")) => (leak(name), |s| s.avg),
                Some((name, "min")) => (leak(name), |s| s.min),
                _ => (leak(row), |s| s.max),
            }
        };
        let over = |stats: &dyn Fn(&RunSummary) -> &BTreeMap<&str, Option<Stat>>, row: &str| {
            let (name, get) = figure(row);
            Spread::of(
                kept.iter()
                    .filter_map(|r| stats(r).get(name).and_then(|s| s.as_ref()).map(get)),
            )
        };
        Final {
            runs: kept.iter().map(|r| r.run).collect(),
            wall_ms: Spread::of(kept.iter().map(|r| r.wall_ms as f64)),
            rows: FINAL_ROWS
                .iter()
                .map(|(row, _)| (*row, over(&|r| &r.stats.target, row)))
                .collect(),
            system: [("mem_available_min", Kind::Bytes), ("cpu_max", Kind::Pct)]
                .iter()
                .map(|(row, _)| (*row, over(&|r| &r.stats.system, row)))
                .collect(),
        }
    }

    pub fn json(&self, exit: i32) -> Value {
        let mut m = Map::new();
        m.insert("runs".into(), json!(self.runs));
        m.insert("exit".into(), json!(exit));
        m.insert(
            "wall_ms".into(),
            self.wall_ms
                .as_ref()
                .map_or(Value::Null, |s| s.json(Kind::Count)),
        );
        for (row, kind) in FINAL_ROWS {
            m.insert(
                row.into(),
                self.rows
                    .get(row)
                    .and_then(|s| s.as_ref())
                    .map_or(Value::Null, |s| s.json(kind)),
            );
        }
        let mut system = Map::new();
        for (row, kind) in [("mem_available_min", Kind::Bytes), ("cpu_max", Kind::Pct)] {
            system.insert(
                row.into(),
                self.system
                    .get(row)
                    .and_then(|s| s.as_ref())
                    .map_or(Value::Null, |s| s.json(kind)),
            );
        }
        m.insert("system".into(), Value::Object(system));
        Value::Object(m)
    }
}

/// The row names are the fixed ones above; this only gives them a static lifetime.
fn leak(name: &str) -> &'static str {
    for (row, _) in ROWS.iter().chain(SYSTEM_ROWS.iter()) {
        if *row == name {
            return row;
        }
    }
    "?"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i64, ppid: i64, comm: &str, rss: i64, ticks: i64) -> Proc {
        Proc {
            pid,
            ppid,
            comm: comm.into(),
            start_time: pid,
            rss_bytes: Some(rss),
            rss_anon_bytes: Some(rss / 2),
            swap_bytes: Some(0),
            thread_count: Some(1),
            cpu_user: ticks,
            cpu_system: 0,
            ..Default::default()
        }
    }

    fn snapshot(procs: Vec<Proc>) -> Snapshot {
        Snapshot {
            clk_tck: 100,
            memory_total: 1000,
            memory_available: 600,
            processes: procs,
            ..Default::default()
        }
    }

    #[test]
    fn tree_is_the_root_and_its_descendants() {
        let s = snapshot(vec![
            proc(1, 0, "init", 1, 0),
            proc(10, 1, "cargo", 100, 0),
            proc(11, 10, "rustc", 200, 0),
            proc(12, 11, "cc", 50, 0),
            proc(20, 1, "stranger", 999, 0),
        ]);
        let pids: Vec<i64> = tree(&s, &[10]).iter().map(|p| p.pid).collect();
        assert_eq!(pids, [10, 11, 12]);
        assert!(tree(&s, &[99]).is_empty());
        let mut s = s;
        s.processes[2].state = "Z".into();
        let pids: Vec<i64> = tree(&s, &[10]).iter().map(|p| p.pid).collect();
        assert_eq!(pids, [10, 12], "a zombie is gone, its child is not");
    }

    #[test]
    fn words_roll_up_to_the_app() {
        let s = snapshot(vec![
            proc(1, 0, "systemd", 1, 0),
            proc(50, 1, "chrome", 100, 0),
            proc(51, 50, "chrome_renderer", 200, 0),
            proc(52, 50, "nacl_helper", 30, 0),
            proc(60, 1, "code", 400, 0),
        ]);
        let t = Target::Words {
            filter: Filter {
                search: vec!["chrome".into()],
                ..Default::default()
            },
            group: Some("app".into()),
        };
        let pids: Vec<i64> = t.members(&s).iter().map(|p| p.pid).collect();
        assert_eq!(
            pids,
            [50, 51, 52],
            "the helper the word does not name is in"
        );
        let plain = Target::Words {
            filter: Filter {
                search: vec!["chrome".into()],
                ..Default::default()
            },
            group: None,
        };
        assert_eq!(plain.members(&s).len(), 2);
    }

    #[test]
    fn cpu_is_over_the_period_and_new_members_count_from_zero() {
        let a = snapshot(vec![proc(10, 1, "a", 100, 100)]);
        let (s0, prev) = Sample::new(0, &a, &tree(&a, &[10]), Some((10, 100)), None);
        assert_eq!(s0.cpu, None, "no period yet");
        assert_eq!(s0.system.cpu, None);
        // One second later: 150 ticks on the root, a child born with 50.
        let b = snapshot(vec![proc(10, 1, "a", 120, 250), proc(11, 10, "b", 30, 50)]);
        let (s1, _) = Sample::new(1000, &b, &tree(&b, &[10]), Some((60, 200)), Some(&prev));
        assert_eq!(s1.cpu, Some(200.0), "(150 + 50) ticks in 1 s at 100 Hz");
        assert_eq!(s1.system.cpu, Some(50.0));
        assert_eq!((s1.rss, s1.procs, s1.threads), (Some(150), 2, Some(2)));
        assert_eq!(s1.system.mem_used, 400);
    }

    #[test]
    fn stats_drop_the_warmup_and_remember_when_the_peak_was() {
        let sample = |t: u64, rss: i64, cpu: Option<f64>| Sample {
            t_ms: t,
            rss: Some(rss),
            anon: None,
            swap: Some(0),
            cpu,
            threads: Some(4),
            procs: 1,
            system: System {
                mem_available: 10,
                mem_used: 5,
                swap_used: 0,
                cpu: None,
                load_1: 0.0,
            },
        };
        let samples = [
            sample(0, 5000, None),
            sample(1000, 100, Some(10.0)),
            sample(2000, 300, Some(30.0)),
            sample(3000, 200, Some(20.0)),
        ];
        let stats = Stats::of(&samples, 500);
        assert_eq!((stats.samples, stats.dropped), (4, 1));
        let rss = stats.target["rss"].as_ref().unwrap();
        assert_eq!(
            (rss.min, rss.max, rss.avg, rss.last, rss.at_max_ms),
            (100.0, 300.0, 200.0, 200.0, 2000)
        );
        assert_eq!(stats.target["cpu"].as_ref().unwrap().max, 30.0);
        assert!(stats.target["anon"].is_none(), "never collected: no row");
        assert!(stats.system["cpu"].is_none());
        let none = Stats::of(&samples, 10_000);
        assert_eq!(none.dropped, 4);
        assert!(none.target["rss"].is_none());
    }

    #[test]
    fn final_leaves_skipped_runs_out() {
        let run = |n: u32, wall: u64, rss_max: f64, skipped: bool| {
            let mut stats = Stats::default();
            stats.target.insert(
                "rss",
                Some(Stat {
                    min: 1.0,
                    max: rss_max,
                    avg: 2.0,
                    last: 1.0,
                    at_max_ms: 0,
                }),
            );
            RunSummary {
                run: n,
                wall_ms: wall,
                skipped,
                stats,
            }
        };
        let f = Final::of(&[
            run(1, 900, 999.0, true),
            run(2, 100, 10.0, false),
            run(3, 300, 30.0, false),
        ]);
        assert_eq!(f.runs, [2, 3]);
        assert_eq!(
            f.wall_ms,
            Some(Spread {
                min: 100.0,
                max: 300.0,
                avg: 200.0,
                spread: 200.0
            })
        );
        assert_eq!(f.rows["rss_max"].as_ref().unwrap().max, 30.0);
        assert!(f.rows["cpu_avg"].is_none());
        assert_eq!(f.json(0)["runs"], json!([2, 3]));
    }
}
