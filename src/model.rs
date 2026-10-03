use std::collections::BTreeMap;

use anyhow::{Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// One process as seen in one snapshot. `None` means "not collected or not
/// readable" and is never stored as 0.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Proc {
    pub pid: i64,
    pub ppid: i64,
    pub uid: Option<i64>,
    pub username: Option<String>,
    pub comm: String,
    pub exe: Option<String>,
    pub cmdline: Option<String>,
    pub state: String,
    pub kthread: bool,
    /// Clock ticks since boot; with `boot_id` and `pid` it identifies the instance.
    pub start_time: i64,
    pub rss_bytes: Option<i64>,
    pub rss_anon_bytes: Option<i64>,
    pub rss_file_bytes: Option<i64>,
    pub rss_shmem_bytes: Option<i64>,
    pub swap_bytes: Option<i64>,
    pub vsz_bytes: Option<i64>,
    pub pss_bytes: Option<i64>,
    pub uss_bytes: Option<i64>,
    pub cpu_user: i64,
    pub cpu_system: i64,
    pub thread_count: Option<i64>,
    pub nice: Option<i64>,
    pub read_bytes: Option<i64>,
    pub write_bytes: Option<i64>,
    pub cgroup: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CgroupMem {
    pub cgroup: String,
    pub memory_current: Option<i64>,
    pub swap_current: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    /// Number within its session: 0 is the baseline, then 1, 2, ... Not a
    /// database id; those stay inside `store`. The live snapshot has none.
    pub id: i64,
    pub label: Option<String>,
    pub created_at: String,
    pub hostname: String,
    pub boot_id: String,
    pub kernel_version: String,
    pub collector_uid: i64,
    pub deep: bool,
    pub clk_tck: i64,
    pub uptime_seconds: f64,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
    pub memory_total: i64,
    pub memory_available: i64,
    pub swap_total: i64,
    pub swap_used: i64,
    pub meminfo: BTreeMap<String, i64>,
    pub cgroups: Vec<CgroupMem>,
    pub processes: Vec<Proc>,
}

impl Snapshot {
    /// `#107 after-1-hour`, or `now` for the live state, which is never stored.
    /// The live state, collected for one command and never stored.
    pub fn is_live(&self) -> bool {
        self.label.as_deref() == Some("now")
    }

    pub fn title(&self) -> String {
        match &self.label {
            _ if self.is_live() => "now".into(),
            Some(l) => format!("#{} {l}", self.id),
            None => format!("#{}", self.id),
        }
    }

    /// System figure, not a sum of per-process RSS (which double-counts shared pages).
    pub fn used(&self) -> i64 {
        self.memory_total - self.memory_available
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Metric {
    Total,
    Anon,
    Pss,
}

impl Metric {
    pub fn parse(s: &str) -> Result<Metric> {
        Ok(match s {
            "total" => Metric::Total,
            "anon" => Metric::Anon,
            "pss" => Metric::Pss,
            _ => bail!("unknown metric {s:?} (expected total, anon or pss)"),
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Metric::Total => "RSS + swap",
            Metric::Anon => "anonymous RSS + swap",
            Metric::Pss => "PSS + swap",
        }
    }

    /// Swap is always included: a swapped-out process has not freed anything.
    pub fn value(self, p: &Proc) -> Option<i64> {
        let base = match self {
            Metric::Total => p.rss_bytes,
            Metric::Anon => p.rss_anon_bytes,
            Metric::Pss => p.pss_bytes,
        };
        base.map(|b| b + p.swap_bytes.unwrap_or(0))
    }
}

pub struct Filter {
    pub user: Option<String>,
    pub name: Option<String>,
    pub exe: Option<String>,
    pub exclude: Option<Regex>,
    pub kernel: bool,
}

impl Filter {
    pub fn keep(&self, p: &Proc) -> bool {
        (self.kernel || !p.kthread)
            && self.user.as_ref().is_none_or(|u| {
                p.username.as_ref() == Some(u) || p.uid.map(|x| x.to_string()).as_ref() == Some(u)
            })
            && self
                .name
                .as_ref()
                .is_none_or(|n| p.comm.contains(n.as_str()))
            && self.exe.as_ref().is_none_or(|e| p.exe.as_ref() == Some(e))
            && !self.exclude.as_ref().is_some_and(|r| {
                r.is_match(&p.comm) || p.cmdline.as_ref().is_some_and(|c| r.is_match(c))
            })
    }

    /// True when the user narrowed the process set (kernel visibility aside).
    pub fn is_narrowing(&self) -> bool {
        self.user.is_some() || self.name.is_some() || self.exe.is_some() || self.exclude.is_some()
    }
}

/// Sum where a missing value does not poison the total; all-missing stays missing.
pub fn add(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (x, None) => x,
        (None, y) => y,
    }
}
