//! Reads the process table through the `procfs` crate. Its types stay in this
//! file; the rest of the program only sees `model` structs.

use std::collections::{BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use procfs::ProcError;
use procfs::process::{Process, all_processes_with_root};

use super::raw as rawproc;
use crate::model::{CgroupMem, Proc, Snapshot};

const PF_KTHREAD: u32 = 0x0020_0000;

pub struct Options {
    pub root: PathBuf,
    pub deep: bool,
    pub cmdline: bool,
    pub io: bool,
    pub cgroups: bool,
}

/// Cumulative CPU ticks per process instance `(pid, start_time)`: the first of
/// the two readings a CPU percentage needs.
pub fn cpu_ticks(root: &Path) -> Result<HashMap<(i64, i64), i64>> {
    Ok(processes(root)?
        .filter_map(|p| p.stat().ok())
        .map(|s| {
            (
                (s.pid as i64, s.starttime as i64),
                (s.utime + s.stime) as i64,
            )
        })
        .collect())
}

fn processes(root: &Path) -> Result<impl Iterator<Item = Process>> {
    let iter = all_processes_with_root(root)
        .with_context(|| format!("cannot read process table at {}", root.display()))?;
    // A process that exits while we scan is normal churn, not an error.
    Ok(iter.filter_map(|p| p.ok()))
}

/// `None` for an unreadable field, remembering whether privileges were the reason.
fn allowed<T>(r: Result<T, ProcError>, denied: &mut bool) -> Option<T> {
    if matches!(r, Err(ProcError::PermissionDenied(_))) {
        *denied = true;
    }
    r.ok()
}

/// Returns the snapshot and the number of processes with fields hidden by
/// missing privileges.
pub fn collect(o: &Options) -> Result<(Snapshot, usize)> {
    let users = rawproc::usernames();
    let mut procs = Vec::new();
    let mut restricted = 0;

    // psm itself would show up as new and gone in every comparison.
    let own_pid = if o.root == Path::new("/proc") {
        std::process::id() as i32
    } else {
        -1
    };

    for p in processes(&o.root)? {
        if p.pid == own_pid {
            continue;
        }
        let Ok(stat) = p.stat() else { continue };
        let status = p.status().ok();
        let kthread = stat.flags & PF_KTHREAD != 0;
        let mut denied = false;
        let kb = |v: Option<u64>| v.map(|x| (x * 1024) as i64);

        let exe = allowed(p.exe(), &mut denied).map(|e| {
            // After an upgrade the old binary shows as "/path (deleted)"; keep one key per program.
            let s = e.to_string_lossy();
            s.strip_suffix(" (deleted)").unwrap_or(&s).to_string()
        });
        let io = if o.io {
            allowed(p.io(), &mut denied)
        } else {
            None
        };
        let rollup = if o.deep && !kthread {
            allowed(p.smaps_rollup(), &mut denied)
        } else {
            None
        };
        let rollup = rollup
            .and_then(|r| r.memory_map_rollup.0.into_iter().next())
            .map(|m| m.extension.map);
        let cgroup = if o.cgroups { p.cgroups().ok() } else { None }.and_then(|c| {
            // cgroup v2 is hierarchy 0; on v1 take the first entry.
            let v2 = c.0.iter().position(|g| g.hierarchy == 0).unwrap_or(0);
            c.0.into_iter().nth(v2).map(|g| g.pathname)
        });
        let cmdline = if o.cmdline { p.cmdline().ok() } else { None }
            .map(|args| args.join(" "))
            .filter(|s| !s.is_empty());
        let uid = status.as_ref().map(|s| s.euid as i64);

        if denied && !kthread {
            restricted += 1;
        }
        procs.push(Proc {
            pid: stat.pid as i64,
            ppid: stat.ppid as i64,
            uid,
            username: uid.and_then(|u| users.get(&u).cloned()),
            comm: stat.comm.clone(),
            exe,
            cmdline,
            state: stat.state.to_string(),
            kthread,
            start_time: stat.starttime as i64,
            rss_bytes: kb(status.as_ref().and_then(|s| s.vmrss)),
            rss_anon_bytes: kb(status.as_ref().and_then(|s| s.rssanon)),
            rss_file_bytes: kb(status.as_ref().and_then(|s| s.rssfile)),
            rss_shmem_bytes: kb(status.as_ref().and_then(|s| s.rssshmem)),
            swap_bytes: kb(status.as_ref().and_then(|s| s.vmswap)),
            vsz_bytes: (!kthread).then_some(stat.vsize as i64),
            pss_bytes: rollup
                .as_ref()
                .and_then(|m| m.get("Pss"))
                .map(|v| *v as i64),
            uss_bytes: rollup.as_ref().map(|m| {
                ["Private_Clean", "Private_Dirty", "Private_Hugetlb"]
                    .iter()
                    .filter_map(|k| m.get(*k))
                    .sum::<u64>() as i64
            }),
            cpu_user: stat.utime as i64,
            cpu_system: stat.stime as i64,
            thread_count: Some(stat.num_threads),
            nice: Some(stat.nice),
            read_bytes: io.as_ref().map(|i| i.read_bytes as i64),
            write_bytes: io.as_ref().map(|i| i.write_bytes as i64),
            cgroup,
        });
    }
    procs.sort_by_key(|p| p.pid);

    let meminfo = rawproc::meminfo(&o.root)?;
    let mem = |k: &str| meminfo.get(k).copied().unwrap_or(0);
    let (load_1, load_5, load_15) = rawproc::loadavg(&o.root);
    let cgroup_root = rawproc::cgroup_root(&o.root);
    let cgroups = procs
        .iter()
        .filter_map(|p| p.cgroup.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|cgroup| {
            let (memory_current, swap_current) = rawproc::cgroup_mem(&cgroup_root, &cgroup);
            CgroupMem {
                cgroup,
                memory_current,
                swap_current,
            }
        })
        .collect();
    let sys =
        |file: &str| rawproc::read_trim(&o.root.join("sys/kernel").join(file)).unwrap_or_default();

    let snapshot = Snapshot {
        hostname: sys("hostname"),
        boot_id: sys("random/boot_id"),
        kernel_version: sys("osrelease"),
        collector_uid: std::fs::metadata("/proc/self")
            .map(|m| m.uid() as i64)
            .unwrap_or(-1),
        deep: o.deep,
        clk_tck: procfs::ticks_per_second() as i64,
        uptime_seconds: rawproc::uptime(&o.root)?,
        load_1,
        load_5,
        load_15,
        memory_total: mem("MemTotal"),
        memory_available: mem("MemAvailable"),
        swap_total: mem("SwapTotal"),
        swap_used: mem("SwapTotal") - mem("SwapFree"),
        meminfo,
        cgroups,
        processes: procs,
        ..Default::default()
    };
    Ok((snapshot, restricted))
}
