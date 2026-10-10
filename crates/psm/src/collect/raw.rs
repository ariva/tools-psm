//! Direct file readers for what the `procfs` crate does not cover. All of them
//! take the proc root so they work against fixture trees too.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))
}

/// Every `/proc/meminfo` key. `kB` values are converted to bytes; the unit-less
/// `HugePages_*` counters are kept as-is.
pub fn meminfo(root: &Path) -> Result<BTreeMap<String, i64>> {
    let mut map = BTreeMap::new();
    for line in read(&root.join("meminfo"))?.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(value) = parts.next().and_then(|v| v.parse::<i64>().ok()) else {
            continue;
        };
        let mult = if parts.next() == Some("kB") { 1024 } else { 1 };
        map.insert(key.to_string(), value * mult);
    }
    Ok(map)
}

pub fn uptime(root: &Path) -> Result<f64> {
    let path = root.join("uptime");
    let text = read(&path)?;
    text.split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .with_context(|| format!("malformed {}", path.display()))
}

pub fn loadavg(root: &Path) -> (f64, f64, f64) {
    let text = read_trim(&root.join("loadavg")).unwrap_or_default();
    let mut v = text.split_whitespace().map(|x| x.parse().unwrap_or(0.0));
    (
        v.next().unwrap_or(0.0),
        v.next().unwrap_or(0.0),
        v.next().unwrap_or(0.0),
    )
}

/// `(busy, total)` ticks of the whole machine from the first line of
/// `/proc/stat`; `None` without the file (fixture trees). Busy is
/// everything but idle and iowait.
pub fn cpu_total(root: &Path) -> Option<(i64, i64)> {
    parse_cpu_line(&read_trim(&root.join("stat"))?)
}

fn parse_cpu_line(text: &str) -> Option<(i64, i64)> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<i64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 5 {
        return None;
    }
    let total: i64 = v.iter().sum();
    Some((total - v[3] - v[4], total))
}

/// cgroup v2 mount. Fixture trees keep theirs next to the process directories.
pub fn cgroup_root(proc_root: &Path) -> PathBuf {
    if proc_root == Path::new("/proc") {
        PathBuf::from("/sys/fs/cgroup")
    } else {
        proc_root.join("cgroup-root")
    }
}

/// `(memory.current, memory.swap.current)`; `None` on cgroup v1 or for the root cgroup.
pub fn cgroup_mem(cgroup_root: &Path, cgroup: &str) -> (Option<i64>, Option<i64>) {
    let dir = cgroup_root.join(cgroup.trim_start_matches('/'));
    let get = |file: &str| read_trim(&dir.join(file)).and_then(|v| v.parse().ok());
    (get("memory.current"), get("memory.swap.current"))
}

// ponytail: /etc/passwd only, so LDAP/NSS users show as a numeric uid.
// Switch to getpwuid_r if that matters.
pub fn usernames() -> HashMap<i64, String> {
    fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            let uid = f.nth(1)?.parse().ok()?;
            Some((uid, name.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_line_is_busy_and_total() {
        // user nice system idle iowait irq softirq steal guest guest_nice
        let text = "cpu  100 5 50 800 20 3 2 0 0 0\ncpu0 1 2 3 4 5 6 7 8 9 0\n";
        assert_eq!(parse_cpu_line(text), Some((160, 980)));
        assert_eq!(parse_cpu_line("intr 1 2 3"), None);
        assert_eq!(parse_cpu_line("cpu 1 2"), None);
    }
}
