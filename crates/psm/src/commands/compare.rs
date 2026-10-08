//! Comparing snapshots: `diff`, `report`, `compare`, and the digest printed
//! after `snap`. Resolves references (including `now`), merges the diff
//! settings, and assembles the sections.

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

use super::Ctx;
use super::capture::live_snapshot;
use crate::analysis::diff::{Diff, Status, growth_table, impact_table};
use crate::analysis::group::Grouper;
use crate::analysis::reports;
use crate::cli::{DiffArgs, FilterArgs, ReportKind};
use crate::model::{Filter, Metric, Proc, Snapshot, add};
use crate::output::{Cell, Table, human, human_delta, out, print_json};
use crate::store::{Db, Session};

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Summary,
    /// The closing digest of a full diff: the few largest memory movers.
    Top,
    New,
    Gone,
    Restarted,
    Memory,
    Counts {
        only_changed: bool,
    },
    Growth,
    Cpu,
    Meminfo,
}

/// Rows in the digest that closes a full diff.
const TOP_ROWS: usize = 5;

pub fn diff(ctx: &Ctx, args: &DiffArgs) -> Result<()> {
    let picked = [
        (args.new, Section::New),
        (args.gone, Section::Gone),
        (args.restarted, Section::Restarted),
        (args.memory, Section::Memory),
    ];
    let mut sections: Vec<Section> = picked
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, s)| *s)
        .collect();
    if sections.is_empty() {
        sections = vec![
            Section::Summary,
            Section::New,
            Section::Gone,
            Section::Restarted,
            Section::Memory,
            Section::Counts { only_changed: true },
            Section::Top,
        ];
    }
    let db = ctx.db()?;
    let (a, b) = pair(ctx, &db, args)?;
    if args.brief {
        return brief(ctx, &a, &b, args);
    }
    compare(ctx, &a, &b, args, &sections)
}

/// Rows of the `--brief` line when `--top` is not given.
const BRIEF_MOVERS: usize = 3;

/// `psm diff --brief`: the whole diff on one line, for logs and messages.
fn brief(ctx: &Ctx, a: &Snapshot, b: &Snapshot, args: &DiffArgs) -> Result<()> {
    let mut s = diff_settings(ctx, a, b, args)?;
    s.top = Some(args.top.unwrap_or(BRIEF_MOVERS));
    let d = Diff::new(a, b, &s.filter);
    if !d.same_boot {
        s.group.get_or_insert_with(|| "name".into());
    }
    let grouper = s
        .group
        .as_deref()
        .map(|kind| Grouper::new(kind, &[a, b]))
        .transpose()?;
    let (mut rows, label) = match &grouper {
        Some(g) => d.group_impact(s.metric, g),
        None => (d.impact(s.metric), s.metric.label()),
    };
    let net: i64 = rows.iter().map(|r| r.delta).sum();
    rows.retain(|r| r.delta.abs() >= s.min_delta);
    rows.truncate(s.top.unwrap_or(BRIEF_MOVERS));
    let display = |name: &str| {
        grouper
            .as_ref()
            .map_or(name.to_string(), |g| g.display(name))
    };
    if ctx.json {
        let side = |s: &Snapshot| json!({ "id": (!s.is_live()).then_some(s.id), "label": s.label });
        print_json(&json!({
            "from": side(a),
            "to": side(b),
            "same_boot": d.same_boot,
            "processes": {
                "before": d.before.len(),
                "after": d.after.len(),
                "new": d.count(Status::New),
                "gone": d.count(Status::Gone),
                "restarted": d.count(Status::Restarted),
            },
            "metric": label,
            "net_change": net,
            "top": rows.iter().map(|r| json!({
                "name": display(&r.name), "pid": r.pid, "status": r.status, "delta": r.delta,
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    let movers = if rows.is_empty() {
        format!("no change above {}", human(s.min_delta))
    } else {
        rows.iter()
            .map(|r| format!("{} {}", display(&r.name), human_delta(r.delta)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    out(format!(
        "{} -> {}: {} -> {} processes, new {}, gone {}, restarted {}, net {} ({}){}; top: {movers}",
        a.title(),
        b.title(),
        d.before.len(),
        d.after.len(),
        d.count(Status::New),
        d.count(Status::Gone),
        d.count(Status::Restarted),
        human_delta(net),
        label,
        if d.same_boot {
            ""
        } else {
            ", different boots: programs compared"
        }
    ));
    Ok(())
}

pub fn report(ctx: &Ctx, kind: ReportKind, args: &DiffArgs) -> Result<()> {
    if args.brief {
        bail!("--brief is for `psm diff`; a report is already one table");
    }
    let db = ctx.db()?;
    let section = match kind {
        ReportKind::Timeline => {
            let session = db.session(None)?;
            let table = reports::timeline_table(&db, &session, &ctx.filter(&args.filter)?)?;
            ctx.emit(&table, false);
            return Ok(());
        }
        ReportKind::Trend => return trend(ctx, &db, args),
        ReportKind::Memory => Section::Memory,
        ReportKind::Processes => Section::Counts {
            only_changed: false,
        },
        ReportKind::New => Section::New,
        ReportKind::Gone => Section::Gone,
        ReportKind::Growth => Section::Growth,
        ReportKind::Cpu => Section::Cpu,
        ReportKind::Meminfo => Section::Meminfo,
    };
    let (a, b) = pair(ctx, &db, args)?;
    compare(ctx, &a, &b, args, &[section])
}

/// `psm report trend`: every snapshot of the session plus `now`, one row per group.
fn trend(ctx: &Ctx, db: &Db, args: &DiffArgs) -> Result<()> {
    if args.a.is_some() || args.b.is_some() {
        bail!("report trend takes no snapshot references: it covers every snapshot of the session");
    }
    let session = db.session(None)?;
    let filter = ctx.filter(&args.filter)?;
    let metas = db.snapshots(session.id, filter.kernel)?;
    let stored: Vec<Snapshot> = metas.iter().map(|m| db.load(m.id)).collect::<Result<_>>()?;
    let live = live_snapshot(ctx, stored.last())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let points: Vec<(f64, &Snapshot)> = metas
        .iter()
        .map(|m| m.created_epoch as f64)
        .zip(&stored)
        .chain([(now, &live)])
        .collect();
    if points.len() < 3 {
        bail!(
            "report trend needs at least 3 points (2 snapshots plus now); session {:?} has {} snapshot{}, take another `psm snap`",
            session.name,
            stored.len(),
            if stored.len() == 1 { "" } else { "s" }
        );
    }
    let metric = Metric::parse(args.metric.as_deref().unwrap_or(&ctx.cfg.diff.metric))?;
    if metric == Metric::Pss && !points.iter().all(|(_, s)| s.deep) {
        bail!("--metric pss needs every snapshot taken with --deep");
    }
    let min_delta = crate::output::parse_size(
        args.min_delta
            .as_deref()
            .unwrap_or(&ctx.cfg.diff.min_memory_delta),
    )?;
    let kind = ctx.group(&args.group).unwrap_or_else(|| "name".into());
    let snaps: Vec<&Snapshot> = points.iter().map(|p| p.1).collect();
    let grouper = Grouper::new(&kind, &snaps)?;
    let mut table = reports::trend_table(&points, &grouper, metric, &filter, min_delta);
    table.truncate(args.top.or(ctx.cfg.display.top));
    let span_hours = (now - points[0].0) / 3600.0;
    if ctx.json {
        print_json(&json!({
            "session": session.name,
            "snapshots": stored.len(),
            "span_hours": (span_hours * 100.0).round() / 100.0,
            "metric": metric.label(),
            "trend": table.json(),
        }));
    } else {
        out(format!(
            "TREND   session {}: {} snapshot{} + now over {:.1} h   ({})\n{}",
            session.name,
            stored.len(),
            if stored.len() == 1 { "" } else { "s" },
            span_hours,
            metric.label(),
            table.render()
        ));
    }
    Ok(())
}

/// `psm sessions compare a b`: two sessions by program, using the latest snapshot of each.
pub fn sessions(
    ctx: &Ctx,
    a: &str,
    b: &str,
    group: Option<String>,
    metric: Option<String>,
    top: Option<usize>,
    filter: FilterArgs,
) -> Result<()> {
    let db = ctx.db()?;
    let (sa, sb) = (db.session(Some(a))?, db.session(Some(b))?);
    let old = db.load(db.resolve(&sa, "latest")?)?;
    let new = db.load(db.resolve(&sb, "latest")?)?;
    compare_sessions(
        ctx,
        (&sa, &old),
        (&sb, &new),
        &DiffArgs {
            group,
            metric,
            top,
            filter,
            ..Default::default()
        },
    )
}

/// No argument: baseline -> now. One: `<ref> -> now`. Two: as given.
/// `now` is the live state; `prev` is the snapshot before the one it is compared with.
fn pair(ctx: &Ctx, db: &Db, args: &DiffArgs) -> Result<(Snapshot, Snapshot)> {
    let session = db.session(None)?;
    let a_ref = args.a.as_deref().unwrap_or("baseline");
    let b_ref = args.b.as_deref().unwrap_or("now");
    if a_ref == "now" && b_ref == "now" {
        bail!("nothing to compare: both sides are `now`");
    }
    let b_id = (b_ref != "now")
        .then(|| db.resolve(&session, b_ref))
        .transpose()?;
    let a_id = match (a_ref, b_id) {
        ("now", _) => None,
        ("prev", None) => Some(db.resolve(&session, "latest")?),
        ("prev", Some(b)) => Some(db.before(&session, b)?),
        (other, _) => Some(db.resolve(&session, other)?),
    };
    let stored_b = b_id.map(|id| db.load(id)).transpose()?;
    let a = match a_id {
        Some(id) => db.load(id)?,
        None => live_snapshot(ctx, stored_b.as_ref())?,
    };
    let b = match stored_b {
        Some(snapshot) => snapshot,
        None => live_snapshot(ctx, Some(&a))?,
    };
    Ok((a, b))
}

struct DiffSettings {
    filter: Filter,
    metric: Metric,
    min_delta: i64,
    top: Option<usize>,
    group: Option<String>,
}

fn diff_settings(ctx: &Ctx, a: &Snapshot, b: &Snapshot, args: &DiffArgs) -> Result<DiffSettings> {
    let metric = Metric::parse(args.metric.as_deref().unwrap_or(&ctx.cfg.diff.metric))?;
    if metric == Metric::Pss && !(a.deep && b.deep) {
        bail!("--metric pss needs both snapshots taken with --deep");
    }
    // Without root, exe and deep-memory values of other users' processes are missing.
    if (a.collector_uid == 0) != (b.collector_uid == 0) {
        eprintln!(
            "warning: {} and {} were taken with different privileges; \
             exe and deep-memory values may appear or disappear without having changed",
            a.title(),
            b.title()
        );
    }
    Ok(DiffSettings {
        filter: ctx.filter(&args.filter)?,
        metric,
        min_delta: crate::output::parse_size(
            args.min_delta
                .as_deref()
                .unwrap_or(&ctx.cfg.diff.min_memory_delta),
        )?,
        top: args.top.or(ctx.cfg.display.top),
        group: ctx.group(&args.group),
    })
}

/// The memory impact ranking as `(table, net change, metric label)`.
fn memory_impact(
    d: &Diff,
    s: &DiffSettings,
    grouper: Option<&Grouper>,
) -> (Table, i64, &'static str) {
    let (mut rows, label) = match grouper {
        Some(g) => d.group_impact(s.metric, g),
        None => (d.impact(s.metric), s.metric.label()),
    };
    let net = rows.iter().map(|r| r.delta).sum();
    rows.retain(|r| r.delta.abs() >= s.min_delta);
    let mut table = impact_table(&rows, grouper.map(Grouper::title));
    table.truncate(s.top);
    (table, net, label)
}

fn compare(
    ctx: &Ctx,
    a: &Snapshot,
    b: &Snapshot,
    args: &DiffArgs,
    sections: &[Section],
) -> Result<()> {
    let mut s = diff_settings(ctx, a, b, args)?;
    let d = Diff::new(a, b, &s.filter);
    let mut text: Vec<String> = Vec::new();
    if !d.same_boot {
        // Every process is new after a reboot, so only programs can be compared.
        text.push(
            "Snapshots are from different boots: comparing programs, not process instances.".into(),
        );
        s.group.get_or_insert_with(|| "name".into());
    }
    let grouper = s
        .group
        .as_deref()
        .map(|kind| Grouper::new(kind, &[a, b]))
        .transpose()?;
    let by_name = Grouper::new("name", &[a, b])?;
    let program = grouper.as_ref().unwrap_or(&by_name);

    let mut doc = Map::new();
    // The live state has no id.
    let side = |s: &Snapshot| json!({ "id": (!s.is_live()).then_some(s.id), "label": s.label });
    doc.insert("from".into(), side(a));
    doc.insert("to".into(), side(b));
    let top = s.top;
    let section = |text: &mut Vec<String>,
                   doc: &mut Map<String, Value>,
                   title: String,
                   key: &str,
                   mut table: Table| {
        table.truncate(top);
        doc.insert(key.into(), table.json());
        text.push(format!("{title}\n{}", table.render()));
    };
    let per_process = d.same_boot && grouper.is_none();

    for sec in sections {
        match sec {
            Section::Summary => text.push(format!(
                "DIFF   {} -> {}\nProcesses: {} -> {}    new {}, gone {}, restarted {}",
                a.title(),
                b.title(),
                d.before.len(),
                d.after.len(),
                d.count(Status::New),
                d.count(Status::Gone),
                d.count(Status::Restarted)
            )),
            Section::Top => {
                // A long diff scrolls; the answer to "who moved memory" goes last.
                let digest = DiffSettings {
                    top: Some(TOP_ROWS),
                    ..diff_settings(ctx, a, b, args)?
                };
                let mut parts = vec![format!(
                    "TOP {TOP_ROWS} MEMORY IMPACT   {} -> {}",
                    a.title(),
                    b.title()
                )];
                let mut top = Map::new();
                if d.same_boot {
                    let (table, _, label) = memory_impact(&d, &digest, None);
                    parts.push(format!("By process   ({label})\n{}", table.render()));
                    top.insert("processes".into(), table.json());
                }
                let (table, net, label) = memory_impact(&d, &digest, Some(program));
                parts.push(format!(
                    "By {}   ({label})\n{}",
                    program.title().to_lowercase(),
                    table.render()
                ));
                top.insert("groups".into(), table.json());
                let used = b.used() - a.used();
                parts.push(format!(
                    "Net process change: {}    System memory used: {} -> {} ({})",
                    human_delta(net),
                    human(a.used()),
                    human(b.used()),
                    human_delta(used)
                ));
                top.insert(
                    "system_used".into(),
                    json!({ "before": a.used(), "after": b.used(), "delta": used }),
                );
                doc.insert("top".into(), Value::Object(top));
                text.push(parts.join("\n\n"));
            }
            Section::New if per_process => section(
                &mut text,
                &mut doc,
                "NEW PROCESSES".into(),
                "new",
                d.new_table(),
            ),
            Section::Gone if per_process => section(
                &mut text,
                &mut doc,
                "GONE PROCESSES".into(),
                "gone",
                d.gone_table(),
            ),
            Section::Restarted if per_process => section(
                &mut text,
                &mut doc,
                "RESTARTED PROCESSES".into(),
                "restarted",
                d.restarted_table(s.metric),
            ),
            Section::New | Section::Gone | Section::Restarted => {}
            Section::Memory => {
                let (table, net, label) = memory_impact(&d, &s, grouper.as_ref());
                doc.insert("metric".into(), json!(label));
                doc.insert("net_change".into(), json!(net));
                doc.insert("memory".into(), table.json());
                text.push(format!(
                    "MEMORY IMPACT   {} -> {}   ({label})\n{}\nNet {} change: {}",
                    a.title(),
                    b.title(),
                    table.render(),
                    if grouper.is_some() {
                        "group"
                    } else {
                        "process"
                    },
                    human_delta(net)
                ));
            }
            Section::Counts { only_changed } => section(
                &mut text,
                &mut doc,
                "PROCESS COUNTS".into(),
                "counts",
                d.counts_table(program, *only_changed),
            ),
            Section::Growth => {
                let (mut rows, label) = d.group_impact(s.metric, program);
                rows.retain(|r| r.delta >= s.min_delta);
                section(
                    &mut text,
                    &mut doc,
                    format!("MEMORY GROWTH   ({label})"),
                    "growth",
                    growth_table(&rows, program.title()),
                );
            }
            Section::Cpu => section(
                &mut text,
                &mut doc,
                format!("CPU TIME   {} -> {}", a.title(), b.title()),
                "cpu",
                reports::cpu_table(&d, program)?,
            ),
            Section::Meminfo => section(
                &mut text,
                &mut doc,
                format!("MEMINFO   {} -> {}", a.title(), b.title()),
                "meminfo",
                reports::meminfo_table(a, b),
            ),
        }
    }
    if ctx.json {
        print_json(&Value::Object(doc))
    } else {
        out(text.join("\n\n"))
    }
    Ok(())
}

/// For `psm snap`: who moved memory the most since the previous snapshot,
/// as (the metric's label, the table). `None` across a reboot or when
/// nothing moved.
pub fn snap_digest(
    ctx: &Ctx,
    previous: &Snapshot,
    current: &Snapshot,
) -> Result<Option<(String, Table)>> {
    let mut s = diff_settings(ctx, previous, current, &DiffArgs::default())?;
    if s.metric == Metric::Pss && !(previous.deep && current.deep) {
        s.metric = Metric::Total;
    }
    s.top = Some(TOP_ROWS);
    s.group = None;
    let d = Diff::new(previous, current, &s.filter);
    if !d.same_boot {
        return Ok(None);
    }
    let (table, _, label) = memory_impact(&d, &s, None);
    Ok((!table.rows.is_empty()).then_some((label.to_string(), table)))
}

/// Sessions are compared by program, never by PID: the latest snapshot of each.
fn compare_sessions(
    ctx: &Ctx,
    (sa, a): (&Session, &Snapshot),
    (sb, b): (&Session, &Snapshot),
    args: &DiffArgs,
) -> Result<()> {
    let mut s = diff_settings(ctx, a, b, args)?;
    let kind = s.group.get_or_insert_with(|| "name".into()).clone();
    let d = Diff::new(a, b, &s.filter);
    let grouper = Grouper::new(&kind, &[a, b])?;

    let sum = |procs: &[&Proc], f: fn(&Proc) -> Option<i64>| {
        procs.iter().fold(None, |acc, p| add(acc, f(p)))
    };
    let mut totals = Table::new(&[
        ("METRIC", "metric"),
        ("OLD", "old"),
        ("NEW", "new"),
        ("DELTA", "delta"),
    ]);
    let (n_old, n_new) = (d.before.len() as i64, d.after.len() as i64);
    totals.rows.push(vec![
        Cell::text("Processes"),
        Cell::Int(Some(n_old)),
        Cell::Int(Some(n_new)),
        Cell::IntDelta(n_new - n_old),
    ]);
    let mut bytes_row = |name: &str, f: fn(&Proc) -> Option<i64>| {
        let (old, new) = (sum(&d.before, f), sum(&d.after, f));
        totals.rows.push(vec![
            Cell::text(name),
            Cell::Bytes(old),
            Cell::Bytes(new),
            Cell::Delta(old.zip(new).map(|(o, n)| n - o)),
        ]);
    };
    bytes_row("Total RSS", |p| p.rss_bytes);
    if a.deep && b.deep {
        bytes_row("Total PSS", |p| p.pss_bytes);
    }
    bytes_row("Swap", |p| p.swap_bytes);
    let (t_old, t_new) = (
        sum(&d.before, |p| p.thread_count),
        sum(&d.after, |p| p.thread_count),
    );
    totals.rows.push(vec![
        Cell::text("Threads"),
        Cell::Int(t_old),
        Cell::Int(t_new),
        Cell::IntDelta(t_new.unwrap_or(0) - t_old.unwrap_or(0)),
    ]);

    let (programs, _, label) = memory_impact(&d, &s, Some(&grouper));
    if ctx.json {
        print_json(&json!({
            "old": { "session": sa.name, "snapshot": a.id },
            "new": { "session": sb.name, "snapshot": b.id },
            "metric": label,
            "totals": totals.json(),
            "programs": programs.json(),
        }));
    } else {
        out(format!(
            "COMPARE   {} ({}) -> {} ({})\n\n{}\n\nBY {}   ({label})\n{}",
            sa.name,
            a.title(),
            sb.name,
            b.title(),
            totals.render(),
            grouper.title(),
            programs.render()
        ));
    }
    Ok(())
}
