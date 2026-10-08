//! Tables of one snapshot: `list` and `info` (live), `show` (stored or live),
//! and the one-process card `pid`.

use std::collections::HashMap;

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

use super::{Ctx, note_restricted};
use crate::analysis::group::Grouper;
use crate::analysis::reports::system_header;
use crate::analysis::view::{self, View, lifetime_cpu, view_table};
use crate::cli::{FilterArgs, ViewArgs};
use crate::model::Proc;
use crate::output::{Cell, Table, human, human_duration, out, print_json};

pub fn list(ctx: &Ctx, view: &ViewArgs, interval: &Option<String>) -> Result<()> {
    // After an option clap no longer looks for subcommands, so `psm procs
    // --top 3 list` would search for the word "list". Say so instead.
    if let Some(sub) = view.words.iter().find(|w| *w == "list" || *w == "show") {
        bail!(
            "`{sub}` is a subcommand: put it right after `procs` (`psm procs {sub} ...`); to search for the word use --name {sub}"
        );
    }
    let deep = view.deep.unwrap_or(ctx.cfg.collection.deep);
    let (snapshot, cpu, restricted) = ctx.live(deep, interval)?;
    ctx.emit(
        &view_table(&snapshot, &cpu, &ctx.view(view, deep)?)?,
        view.csv,
    );
    note_restricted(restricted, &snapshot);
    Ok(())
}

pub fn info(
    ctx: &Ctx,
    n: usize,
    by: &[String],
    group: &Option<String>,
    interval: &Option<String>,
    deep: Option<bool>,
    filter: &FilterArgs,
) -> Result<()> {
    let deep = deep.unwrap_or(ctx.cfg.collection.deep);
    let (snapshot, cpu, _) = ctx.live(deep, interval)?;
    let group = ctx.group(group);
    let (header, system) = system_header(&snapshot, &ctx.filter(filter)?);
    let mut text = vec![header];
    let mut doc = Map::from_iter([("system".to_string(), system)]);
    for metric in by {
        // Every table is exactly `psm procs --sort <metric> --top <n>` over the same pass.
        let view = View {
            group: group.clone(),
            sort: metric.clone(),
            top: Some(n),
            deep,
            filter: ctx.filter(filter)?,
        };
        let table = view_table(&snapshot, &cpu, &view)?;
        let title = if metric == "mem" {
            "MEMORY".to_string()
        } else {
            metric.to_uppercase()
        };
        text.push(format!("TOP {n} BY {title}\n{}", table.render()));
        doc.insert(metric.clone(), table.json());
    }
    if ctx.json {
        print_json(&Value::Object(doc))
    } else {
        out(text.join("\n\n"))
    }
    Ok(())
}

pub fn show(ctx: &Ctx, snapshot: &str, view: &ViewArgs) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(None)?;
    let s = match snapshot {
        "now" => super::capture::live_snapshot(ctx, None)?,
        stored => db.load(db.resolve(&session, stored)?)?,
    };
    let deep = view.deep.unwrap_or(s.deep);
    // A stored snapshot holds one reading, so %CPU is the lifetime average.
    let table = view::view_table(&s, &lifetime_cpu(&s), &ctx.view(view, deep)?)?;
    ctx.emit(&table, view.csv);
    Ok(())
}

/// `psm pid <pid> [ref]`: one process on one screen, then its row in every
/// snapshot of the session (same boot, same instance).
pub fn pid(ctx: &Ctx, pid: i64, snapshot: &str) -> Result<()> {
    let db = ctx.db()?;
    let s = match snapshot {
        "now" => super::capture::live_snapshot(ctx, None)?,
        stored => db.load(db.resolve(&db.session(None)?, stored)?)?,
    };
    let Some(p) = s.processes.iter().find(|p| p.pid == pid) else {
        bail!(
            "no process {pid} in {}{}",
            s.title(),
            if s.is_live() {
                "; `psm pid <pid> latest` looks in the last snapshot"
            } else {
                ""
            }
        );
    };
    let by_pid: HashMap<i64, &Proc> = s.processes.iter().map(|p| (p.pid, p)).collect();
    // Init first, the process last. The bound only guards a corrupt parent loop.
    let mut chain = vec![p];
    while chain.len() < 64 {
        let last = chain[chain.len() - 1];
        match by_pid.get(&last.ppid) {
            Some(parent) if parent.pid != last.pid => chain.push(parent),
            _ => break,
        }
    }
    chain.reverse();
    let app = Grouper::new("app", &[&s])?.key(p);
    let age = (s.clk_tck > 0).then(|| s.uptime_seconds - p.start_time as f64 / s.clk_tck as f64);
    let secs = |ticks: i64| ticks as f64 / s.clk_tck.max(1) as f64;
    let cpu = lifetime_cpu(&s).get(&pid).copied();
    let user = p
        .username
        .clone()
        .or(p.uid.map(|u| u.to_string()))
        .unwrap_or_else(|| "?".into());
    let opt = |v: Option<i64>| v.map_or("n/a".to_string(), human);

    let mut lines = vec![
        format!(
            "PID {pid}  {}   state {}   user {user}{}   nice {}   threads {}",
            p.comm,
            p.state,
            p.uid.map(|u| format!(" ({u})")).unwrap_or_default(),
            p.nice.map_or("n/a".into(), |n| n.to_string()),
            p.thread_count.map_or("n/a".into(), |n| n.to_string()),
        ),
        format!("Exe:      {}", p.exe.as_deref().unwrap_or("n/a")),
        format!("Cmdline:  {}", p.cmdline.as_deref().unwrap_or("n/a")),
        format!(
            "Chain:    {}   app: {app}",
            chain
                .iter()
                .map(|c| format!("{} {}", c.pid, c.comm))
                .collect::<Vec<_>>()
                .join(" > ")
        ),
        format!("Cgroup:   {}", p.cgroup.as_deref().unwrap_or("n/a")),
        format!(
            "Age:      {}   (as of {})",
            age.map_or("n/a".into(), human_duration),
            s.title()
        ),
        format!(
            "Memory:   RSS {} (anon {}, file {}, shmem {})   swap {}   VSZ {}",
            opt(p.rss_bytes),
            opt(p.rss_anon_bytes),
            opt(p.rss_file_bytes),
            opt(p.rss_shmem_bytes),
            opt(p.swap_bytes),
            opt(p.vsz_bytes)
        ),
    ];
    if s.deep {
        lines.push(format!(
            "          PSS {}   USS {}",
            opt(p.pss_bytes),
            opt(p.uss_bytes)
        ));
    }
    // Time consumed so far (what `ps -o time` shows), then the average
    // rate over the process's life; `psm procs` measures the last 500 ms.
    lines.push(format!(
        "CPU:      {} total ({} user, {} system)   {} of one core over its life   (psm procs shows the last 500ms)",
        human_duration(secs(p.cpu_user + p.cpu_system)),
        human_duration(secs(p.cpu_user)),
        human_duration(secs(p.cpu_system)),
        cpu.map_or("n/a".into(), |c| format!("{c:.1} %"))
    ));
    lines.push(format!(
        "I/O:      read {}   written {}",
        opt(p.read_bytes),
        opt(p.write_bytes)
    ));

    // The same instance in every stored snapshot of the session: a reused pid
    // from another boot or an older start time is not it.
    let mut history = Table::new(&[
        ("ID", "id"),
        ("LABEL", "label"),
        ("TIME", "time"),
        ("RSS", "rss"),
        ("SWAP", "swap"),
        ("THR", "threads"),
        ("%CPU", "cpu_percent"),
    ]);
    let session = db.session(None).ok();
    if let Some(session) = &session {
        for meta in db.snapshots(session.id, true)? {
            let old = db.load(meta.id)?;
            if old.boot_id != s.boot_id {
                continue;
            }
            let Some(then) = old
                .processes
                .iter()
                .find(|q| q.pid == pid && q.start_time == p.start_time)
            else {
                continue;
            };
            history.rows.push(vec![
                Cell::Int(Some(old.id)),
                Cell::text(meta.label.clone().unwrap_or_default()),
                Cell::text(meta.created.clone()),
                Cell::Bytes(then.rss_bytes),
                Cell::Bytes(then.swap_bytes),
                Cell::Int(then.thread_count),
                Cell::Pct(lifetime_cpu(&old).get(&pid).copied()),
            ]);
        }
    }

    if ctx.json {
        print_json(&json!({
            "pid": pid,
            "name": p.comm,
            "state": p.state,
            "user": user,
            "uid": p.uid,
            "nice": p.nice,
            "threads": p.thread_count,
            "exe": p.exe,
            "cmdline": p.cmdline,
            "chain": chain.iter().map(|c| json!({ "pid": c.pid, "name": c.comm })).collect::<Vec<_>>(),
            "app": app,
            "cgroup": p.cgroup,
            "age_seconds": age,
            "snapshot": { "id": (!s.is_live()).then_some(s.id), "label": s.label },
            "memory": {
                "rss": p.rss_bytes, "anon": p.rss_anon_bytes, "file": p.rss_file_bytes,
                "shmem": p.rss_shmem_bytes, "swap": p.swap_bytes, "vsz": p.vsz_bytes,
                "pss": p.pss_bytes, "uss": p.uss_bytes,
            },
            "cpu": {
                "total_seconds": secs(p.cpu_user + p.cpu_system),
                "user_seconds": secs(p.cpu_user),
                "system_seconds": secs(p.cpu_system),
                "lifetime_percent": cpu.map(|c| (c * 10.0).round() / 10.0),
            },
            "io": { "read": p.read_bytes, "written": p.write_bytes },
            "session": session.as_ref().map(|x| x.name.clone()),
            "history": history.json(),
        }));
    } else {
        if let Some(session) = &session {
            lines.push(format!(
                "\nIn session {}:\n{}",
                session.name,
                history.render()
            ));
        }
        out(lines.join("\n"));
    }
    Ok(())
}
