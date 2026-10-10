//! `psm track`: start a command, or pick processes by pid or words, sample
//! them every period until they end, and print min/max/avg of what they
//! used. `--times` repeats a command and compares the runs; `--save`
//! keeps the whole JSON document on disk, rewritten after every sample.

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::Ctx;
use crate::analysis::track::{
    FINAL_ROWS, Final, Kind, Previous, ROWS, RunSummary, Sample, Stat, Stats, Target,
};
use crate::cli::FilterArgs;
use crate::collect::raw;
use crate::model::Filter;
use crate::output::{
    Cell, Table, human, human_duration, json_document, now_utc, out, parse_duration,
};

pub struct Args {
    pub words: Vec<String>,
    pub every: Option<String>,
    pub warmup: Option<String>,
    pub for_: Option<String>,
    pub times: u32,
    pub skip_runs: u32,
    pub keep_going: bool,
    pub pause: Option<String>,
    pub save: Option<PathBuf>,
    pub group: Option<String>,
    pub quiet: bool,
    pub filter: FilterArgs,
    pub command: Vec<OsString>,
}

/// Set by Ctrl-C (SIGINT) or SIGTERM; the sampling loop checks it.
static STOP: AtomicBool = AtomicBool::new(false);
static SIGNAL: AtomicI32 = AtomicI32::new(0);
/// The "partially readable" note, once per command, not per run. For a
/// started command it waits until the table, so it never lands in the
/// middle of the command's own output: `(restricted, total)` of the
/// first sample that saw it.
static NOTED: AtomicBool = AtomicBool::new(false);
static RESTRICTED: AtomicUsize = AtomicUsize::new(0);
static RESTRICTED_OF: AtomicUsize = AtomicUsize::new(0);

extern "C" fn on_signal(sig: libc::c_int) {
    SIGNAL.store(sig, Ordering::SeqCst);
    STOP.store(true, Ordering::SeqCst);
}

fn stopped() -> bool {
    STOP.load(Ordering::SeqCst)
}

enum Mode {
    Spawn(Vec<OsString>),
    Pid(Vec<i64>),
    Words,
}

struct Settings {
    every: Duration,
    warmup: Duration,
    pause: Duration,
    for_: Option<Duration>,
    times: u32,
    skip_runs: u32,
    keep_going: bool,
    save: Option<PathBuf>,
    progress: bool,
}

/// One run, finished or in progress.
struct Run {
    run: u32,
    status: &'static str,
    /// Why it ended: `exit`, `gone`, `for`, `ctrl-c`; `None` while running.
    ended: Option<&'static str>,
    exit: Option<i32>,
    started: String,
    wall_ms: u64,
    skipped: bool,
    samples: Vec<Sample>,
    stats: Stats,
}

impl Run {
    fn json(&self) -> Value {
        json!({
            "run": self.run,
            "status": self.status,
            "ended": self.ended,
            "exit": self.exit,
            "skipped": self.skipped,
            "started": self.started,
            "wall_ms": self.wall_ms,
            "samples": self.stats.samples,
            "dropped": self.stats.dropped,
            "stats": self.stats.json(),
            "series": self.samples.iter().map(Sample::json).collect::<Vec<_>>(),
        })
    }

    fn summary(&self) -> RunSummary {
        RunSummary {
            run: self.run,
            wall_ms: self.wall_ms,
            skipped: self.skipped,
            stats: self.stats.clone(),
        }
    }
}

/// Everything the JSON document holds besides the runs.
struct Head {
    target: Value,
    target_text: String,
    host: Value,
    hostname: String,
    cpus: usize,
    memory_total: i64,
    settings: Value,
}

pub fn track(ctx: &Ctx, a: Args) -> Result<()> {
    let duration = |flag: &Option<String>, cfg: &str, name: &str| -> Result<Duration> {
        parse_duration(flag.as_deref().unwrap_or(cfg)).with_context(|| format!("--{name}"))
    };
    let settings = Settings {
        every: duration(&a.every, &ctx.cfg.track.every, "every")?,
        warmup: duration(&a.warmup, &ctx.cfg.track.warmup, "warmup")?,
        pause: duration(&a.pause, &ctx.cfg.track.pause, "pause")?,
        for_: a.for_.as_deref().map(parse_duration).transpose()?,
        times: a.times.max(1),
        skip_runs: a.skip_runs,
        keep_going: a.keep_going,
        save: a.save.clone(),
        // A started command owns the terminal: its output is the progress.
        progress: !a.quiet && !ctx.json && a.command.is_empty() && std::io::stderr().is_terminal(),
    };
    if settings.every.is_zero() {
        bail!("--every needs a duration above zero");
    }
    let filter = Filter {
        search: a.words.clone(),
        ..ctx.filter(&a.filter)?
    };
    let mode = if !a.command.is_empty() {
        Mode::Spawn(a.command.clone())
    } else if !a.filter.pid.is_empty() {
        Mode::Pid(a.filter.pid.clone())
    } else if filter.is_narrowing() {
        Mode::Words
    } else {
        bail!(
            "nothing to track: give a command after `--`, a --pid, or words (`psm track chrome`)"
        );
    };
    if settings.times > 1 && !matches!(mode, Mode::Spawn(_)) {
        bail!("--times repeats a command: give one after `--`");
    }
    let group = a
        .group
        .clone()
        .or_else(|| Some("app".into()))
        .filter(|g| g != "pid");
    let (target, target_json, target_text) = match &mode {
        Mode::Spawn(cmd) => {
            let words: Vec<String> = cmd.iter().map(|w| w.to_string_lossy().into()).collect();
            // One line in the header, whatever a script argument contains.
            let line = words
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (
                Target::Tree(Vec::new()),
                json!({ "mode": "spawn", "command": words }),
                line,
            )
        }
        Mode::Pid(pids) => (
            Target::Tree(pids.clone()),
            json!({ "mode": "pid", "pids": pids }),
            format!(
                "pid {}",
                pids.iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
        Mode::Words => {
            // The words, or the precise filters that stand in for them.
            let mut text = a.words.clone();
            for (flag, value) in [
                ("--name", &a.filter.name),
                ("--exe", &a.filter.exe),
                ("--cmdline", &a.filter.cmdline),
                ("--user", &a.filter.user),
            ] {
                if let Some(v) = value {
                    text.push(format!("{flag} {v}"));
                }
            }
            let text = text.join(" ");
            let text = match &group {
                Some(g) => format!("{text} ({g})"),
                None => text,
            };
            (
                Target::Words {
                    filter,
                    group: group.clone(),
                },
                json!({ "mode": "words", "words": a.words, "group": group }),
                text,
            )
        }
    };
    unsafe {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }

    // The first pass gives the host block; every run samples anew.
    let (first, _) = ctx.collect(false, true)?;
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get());
    let head = Head {
        target: target_json,
        target_text,
        host: json!({
            "hostname": first.hostname,
            "cpus": cpus,
            "memory_total": first.memory_total,
            "swap_total": first.swap_total,
            "clk_tck": first.clk_tck,
            "kernel": first.kernel_version,
        }),
        hostname: first.hostname.clone(),
        cpus,
        memory_total: first.memory_total,
        settings: json!({
            "every_ms": settings.every.as_millis() as u64,
            "warmup_ms": settings.warmup.as_millis() as u64,
            "pause_ms": settings.pause.as_millis() as u64,
            "for_ms": settings.for_.map(|d| d.as_millis() as u64),
            "times": settings.times,
            "skip_runs": settings.skip_runs,
        }),
    };
    drop(first);

    // On a terminal a started command's output sits between two rules, so
    // where psm ends and the program begins is plain.
    let framed = !ctx.json && matches!(mode, Mode::Spawn(_)) && std::io::stdout().is_terminal();
    if framed {
        let mut line = format!(
            "tracking {}   every {}",
            head.target_text,
            secs(settings.every)
        );
        if let Some(d) = settings.for_ {
            line.push_str(&format!("   for {}", secs(d)));
        }
        if settings.times > 1 {
            line.push_str(&format!("   {} times", settings.times));
        }
        line.push_str("   (its output follows; the table comes when it ends)");
        out(line);
        out(RULE);
    }
    let begun = Instant::now();
    let mut runs: Vec<Run> = Vec::new();
    let mut exit_code = 0;
    for n in 1..=settings.times {
        if n > 1 && !settings.pause.is_zero() {
            progress(&settings, &format!("pause {}", secs(settings.pause)));
            wait(settings.pause, stopped);
        }
        if stopped() {
            break;
        }
        let run = run_once(ctx, &mode, &target, &settings, &head, &runs, n)?;
        // A `--for` stop is the plan, not a failure, whatever the signal did to the child.
        if let Some(code) = run.exit.filter(|c| *c != 0 && run.ended != Some("for")) {
            exit_code = code;
        }
        let failed = run.status == "failed";
        runs.push(run);
        if stopped() || (failed && !settings.keep_going) {
            break;
        }
    }
    let status = if stopped() {
        "interrupted"
    } else if runs.iter().any(|r| r.status == "failed") {
        "failed"
    } else {
        "done"
    };
    if status == "interrupted" && exit_code == 0 {
        exit_code = 130;
    }
    let summaries: Vec<RunSummary> = runs.iter().map(Run::summary).collect();
    let final_ = Final::of(&summaries);
    let doc = document(status, &head, &runs, None, Some((&final_, exit_code)));
    if let Some(path) = &settings.save {
        save(path, &doc)?;
    }
    progress(&settings, "");
    if framed {
        out("");
        out(RULE);
        out(format!("{}\n", crate::cli::commands::VERSION_LINE));
    }
    let restricted = RESTRICTED.load(Ordering::SeqCst);
    if restricted > 0 {
        eprintln!(
            "{restricted} of {} processes partially readable (run as root for full data)",
            RESTRICTED_OF.load(Ordering::SeqCst)
        );
    }
    if ctx.json {
        // `doc` already carries the envelope: it is what `--save` writes too.
        out(serde_json::to_string_pretty(&doc).unwrap_or_default());
    } else {
        let mut text = if runs.len() == 1 && settings.times == 1 {
            single(&head, &runs[0])
        } else {
            bench(&head, &settings, &runs, &final_, begun.elapsed())
        };
        if let Some(path) = &settings.save {
            text.push_str(&format!("\nsaved {}", path.display()));
        }
        out(text);
    }
    if exit_code != 0 {
        let _ = std::io::stdout().flush();
        std::process::exit(exit_code);
    }
    Ok(())
}

/// One run: spawn when there is a command, then sample until the target
/// ends, `--for` elapses or a signal arrives.
fn run_once(
    ctx: &Ctx,
    mode: &Mode,
    target: &Target,
    settings: &Settings,
    head: &Head,
    done: &[Run],
    n: u32,
) -> Result<Run> {
    let mut child: Option<Child> = None;
    let target = match mode {
        Mode::Spawn(cmd) => {
            let c = std::process::Command::new(&cmd[0])
                .args(&cmd[1..])
                .spawn()
                .with_context(|| format!("cannot start {}", cmd[0].to_string_lossy()))?;
            let pid = c.id() as i64;
            child = Some(c);
            &Target::Tree(vec![pid])
        }
        _ => target,
    };
    let started_at = Instant::now();
    let mut run = Run {
        run: n,
        status: "running",
        ended: None,
        exit: None,
        started: now_utc(),
        wall_ms: 0,
        skipped: n <= settings.skip_runs,
        samples: Vec::new(),
        stats: Stats::default(),
    };
    let mut previous: Option<Previous> = None;
    let mut seen = false;
    let warmup_ms = settings.warmup.as_millis() as u64;
    loop {
        let tick = Instant::now();
        let t_ms = started_at.elapsed().as_millis() as u64;
        let (snapshot, restricted) = ctx.collect(false, true)?;
        if restricted > 0 && !NOTED.swap(true, Ordering::SeqCst) {
            if matches!(mode, Mode::Spawn(_)) {
                RESTRICTED.store(restricted, Ordering::SeqCst);
                RESTRICTED_OF.store(
                    snapshot.processes.iter().filter(|p| !p.kthread).count(),
                    Ordering::SeqCst,
                );
            } else {
                super::note_restricted(restricted, &snapshot);
            }
        }
        let members = target.members(&snapshot);
        let (sample, prev) = Sample::new(
            t_ms,
            &snapshot,
            &members,
            raw::cpu_total(&ctx.proc_root),
            previous.as_ref(),
        );
        previous = Some(prev);
        let present = !members.is_empty();
        // The sample that finds the target gone is not a sample of it.
        if present || !seen {
            run.samples.push(sample);
        }
        run.wall_ms = started_at.elapsed().as_millis() as u64;
        run.stats = Stats::of(&run.samples, warmup_ms);
        if let Some(path) = &settings.save {
            save(path, &document("running", head, done, Some(&run), None))?;
        }
        progress(
            settings,
            &format!(
                "run {}/{}  {}  {} samples  {}",
                n,
                settings.times,
                secs(started_at.elapsed()),
                run.samples.len(),
                run.samples.last().map_or(String::new(), latest)
            ),
        );
        // Why the run ends, checked after the sample so the last state is in.
        match mode {
            Mode::Words if !present && !seen => {
                eprintln!("psm: no process matches; is it running?");
                std::process::exit(1);
            }
            Mode::Words | Mode::Pid(_) if !present => {
                run.ended = Some("gone");
            }
            _ => {}
        }
        seen |= present;
        if run.ended.is_some() {
            break;
        }
        // Sleep to the next tick, watching the child, the clock and the signal.
        let until = settings.every.saturating_sub(tick.elapsed());
        let deadline = settings.for_.map(|d| started_at + d);
        let exited = wait(until, || {
            stopped()
                || deadline.is_some_and(|d| Instant::now() >= d)
                || child
                    .as_mut()
                    .is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))))
        });
        let _ = exited;
        if stopped() {
            run.ended = Some("ctrl-c");
            break;
        }
        if let Some(c) = child.as_mut()
            && let Ok(Some(status)) = c.try_wait()
        {
            run.ended = Some("exit");
            run.exit = Some(exit_code(status));
            break;
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            run.ended = Some("for");
            break;
        }
    }
    if let Some(mut c) = child {
        // Ctrl-C from the terminal already reached the child; a `--for`
        // stop or a SIGTERM to psm has to be passed on.
        let sig = match (run.ended, SIGNAL.load(Ordering::SeqCst)) {
            (Some("exit"), _) => None,
            (Some("for"), _) => Some(libc::SIGINT),
            (_, s) if s == libc::SIGTERM => Some(libc::SIGTERM),
            _ => None,
        };
        if let Some(sig) = sig {
            unsafe {
                libc::kill(c.id() as libc::pid_t, sig);
            }
        }
        // Give it a moment to exit on its own, then insist.
        let grace = Instant::now();
        let status = loop {
            if let Ok(Some(s)) = c.try_wait() {
                break s;
            }
            if grace.elapsed() > Duration::from_secs(5) {
                let _ = c.kill();
                break c.wait()?;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        run.exit = Some(exit_code(status));
    }
    run.wall_ms = started_at.elapsed().as_millis() as u64;
    run.stats = Stats::of(&run.samples, warmup_ms);
    run.status = match (run.ended, run.exit) {
        (Some("ctrl-c"), _) => "interrupted",
        (Some("for"), _) => "done",
        (_, Some(code)) if code != 0 => "failed",
        _ => "done",
    };
    if run.stats.dropped == run.stats.samples {
        bail!(
            "no samples after the warmup: the run took {} and --warmup is {}",
            secs(Duration::from_millis(run.wall_ms)),
            secs(settings.warmup)
        );
    }
    Ok(run)
}

/// The child's exit code; a signal death is 128 + the signal, as shells report it.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1)
}

/// Sleeps up to `d`, waking every 50 ms to ask `stop`; true when it did.
fn wait(d: Duration, mut stop: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + d;
    loop {
        if stop() {
            return true;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(left.min(Duration::from_millis(50)));
    }
}

/// `rss 1.2 GiB  anon 800 MiB  swap 0 B  cpu 12.5%  threads 40  procs 3`:
/// the newest sample, for the progress line.
fn latest(s: &Sample) -> String {
    let bytes = |v: Option<i64>| v.map_or("n/a".to_string(), human);
    let count = |v: Option<i64>| v.map_or("n/a".to_string(), |x| x.to_string());
    format!(
        "rss {}  anon {}  swap {}  cpu {}  threads {}  procs {}",
        bytes(s.rss),
        bytes(s.anon),
        bytes(s.swap),
        s.cpu.map_or("-".to_string(), |c| format!("{c:.1}%")),
        count(s.threads),
        s.procs
    )
}

/// Frames a started command's output on a terminal.
const RULE: &str = "----------------------------------------------------------------------";

/// Under every table: what the rows are, in one line.
const LEGEND: &str = "rows: sums over the target's processes per sample; cpu % per core (100 = one core); \
procs = how many processes; AT MAX = seconds from the start. `psm help track` explains each.";

/// The progress line on stderr, rewritten in place; empty clears it.
fn progress(settings: &Settings, text: &str) {
    if !settings.progress {
        return;
    }
    if text.is_empty() {
        eprint!("\r\x1b[K");
    } else {
        eprint!("\rtrack: {text}\x1b[K");
    }
}

/// The whole document: what `--json` prints and `--save` writes.
fn document(
    status: &str,
    head: &Head,
    done: &[Run],
    current: Option<&Run>,
    final_: Option<(&Final, i32)>,
) -> Value {
    let mut runs: Vec<Value> = done.iter().map(Run::json).collect();
    if let Some(r) = current {
        runs.push(r.json());
    }
    let mut data = serde_json::Map::new();
    data.insert("status".into(), json!(status));
    data.insert("target".into(), head.target.clone());
    data.insert("host".into(), head.host.clone());
    data.insert("settings".into(), head.settings.clone());
    if let Some(r) = current {
        data.insert("run".into(), json!(r.run));
    }
    data.insert("runs".into(), Value::Array(runs));
    data.insert(
        "final".into(),
        final_.map_or(Value::Null, |(f, exit)| f.json(exit)),
    );
    json_document(&Value::Object(data))
}

/// Written whole, next to the target, then renamed over it: a reader
/// never sees half a document.
// ponytail: the whole document is rewritten after every sample, O(n²)
// bytes over a run; an hour at 1 s is a 1 MiB write per second. Days at
// 100 ms want a JSON Lines sibling, add then.
fn save(path: &Path, doc: &Value) -> Result<()> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let text = serde_json::to_string_pretty(doc).unwrap_or_default();
    std::fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))
}

/// `42.3s` under a minute, else `12m 5s`.
fn secs(d: Duration) -> String {
    if d < Duration::from_secs(60) {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        human_duration(d.as_secs_f64())
    }
}

fn ms(ms: u64) -> String {
    secs(Duration::from_millis(ms))
}

fn cell(v: f64, kind: Kind) -> Cell {
    match kind {
        Kind::Bytes => Cell::Bytes(Some(v.round() as i64)),
        Kind::Pct => Cell::Pct(Some(v)),
        Kind::Count => Cell::Int(Some(v.round() as i64)),
    }
}

/// `exit 0`, `gone`, `stopped: --for 10m`, `interrupted`.
fn ending(run: &Run) -> String {
    match (run.ended, run.exit) {
        (Some("ctrl-c"), _) => "interrupted".into(),
        (Some("for"), Some(code)) => format!("stopped by --for, exit {code}"),
        (Some("for"), None) => "stopped by --for".into(),
        (Some("gone"), _) => "gone".into(),
        (_, Some(code)) => format!("exit {code}"),
        _ => run.status.to_string(),
    }
}

/// `HOST   box, 16 cpus, 62.5 GiB, free min 35.1 GiB (of 41.0 GiB at start), system cpu avg 31% max 88%, load 4.2`
fn host_line(head: &Head, runs: &[&Run]) -> String {
    let samples = || runs.iter().flat_map(|r| r.samples.iter());
    let free_min = samples().map(|s| s.system.mem_available).min();
    let free_start = samples().next().map(|s| s.system.mem_available);
    let cpu: Vec<f64> = samples().filter_map(|s| s.system.cpu).collect();
    let load = samples().last().map(|s| s.system.load_1);
    let mut parts = vec![
        head.hostname.clone(),
        format!("{} cpus", head.cpus),
        human(head.memory_total),
    ];
    if let (Some(min), Some(start)) = (free_min, free_start) {
        parts.push(format!(
            "free min {} (of {} at start)",
            human(min),
            human(start)
        ));
    }
    if !cpu.is_empty() {
        let avg = cpu.iter().sum::<f64>() / cpu.len() as f64;
        let max = cpu.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        parts.push(format!("system cpu avg {avg:.0}% max {max:.0}%"));
    }
    if let Some(load) = load {
        parts.push(format!("load {load:.1}"));
    }
    format!("HOST   {}", parts.join(", "))
}

/// The one-run output: header, host, the metric table.
fn single(head: &Head, run: &Run) -> String {
    let mut text = format!(
        "TRACK  {}   {}   wall {}   samples {}",
        head.target_text,
        ending(run),
        ms(run.wall_ms),
        run.stats.samples
    );
    if run.stats.dropped > 0 {
        text.push_str(&format!(", warmup {} dropped", run.stats.dropped));
    }
    text.push('\n');
    text.push_str(&host_line(head, &[run]));
    text.push('\n');
    text.push_str(&stats_table(&run.stats).render());
    text.push('\n');
    text.push_str(LEGEND);
    text
}

fn stats_table(stats: &Stats) -> Table {
    let mut t = Table::new(&[
        ("METRIC", "metric"),
        ("MIN", "min"),
        ("MAX", "max"),
        ("AVG", "avg"),
        ("LAST", "last"),
        ("AT MAX", "at_max"),
    ]);
    let push = |t: &mut Table, name: &str, kind: Kind, s: &Option<Stat>| {
        let label = match name {
            "cpu" => "cpu %".to_string(),
            n => n.to_string(),
        };
        let Some(s) = s else {
            t.rows.push(vec![
                Cell::text(label),
                Cell::Int(None),
                Cell::Int(None),
                Cell::Int(None),
                Cell::Int(None),
                Cell::text(""),
            ]);
            return;
        };
        t.rows.push(vec![
            Cell::text(label),
            cell(s.min, kind),
            cell(s.max, kind),
            cell(s.avg, kind),
            cell(s.last, kind),
            Cell::text(if s.min == s.max {
                String::new()
            } else {
                ms(s.at_max_ms)
            }),
        ]);
    };
    for (name, kind) in ROWS {
        push(&mut t, name, kind, &stats.target[name]);
    }
    t
}

/// The `--times` output: header, host, one row per run, the FINAL block.
fn bench(
    head: &Head,
    settings: &Settings,
    runs: &[Run],
    final_: &Final,
    total: Duration,
) -> String {
    let skipped = runs.iter().filter(|r| r.skipped).count();
    let mut text = format!("TRACK  {}   runs {}", head.target_text, runs.len());
    if runs.len() != settings.times as usize {
        text.push_str(&format!(" of {}", settings.times));
    }
    if skipped > 0 {
        text.push_str(&format!(" ({skipped} skipped)"));
    }
    text.push_str(&format!(
        "   every {}, warmup {}, pause {}   total {}",
        secs(settings.every),
        secs(settings.warmup),
        secs(settings.pause),
        secs(total)
    ));
    if let Some(r) = runs.iter().find(|r| r.status != "done") {
        text.push_str(&format!("   {}: run {}", ending(r), r.run));
    }
    text.push('\n');
    text.push_str(&host_line(head, &runs.iter().collect::<Vec<_>>()));
    text.push('\n');
    let mut t = Table::new(&[
        ("RUN", "run"),
        ("EXIT", "exit"),
        ("WALL", "wall"),
        ("RSS MAX", "rss_max"),
        ("ANON MAX", "anon_max"),
        ("CPU AVG", "cpu_avg"),
        ("THREADS MAX", "threads_max"),
        ("PROCS MAX", "procs_max"),
        ("", "note"),
    ]);
    let get = |stats: &Stats, name: &str, f: fn(&Stat) -> f64| stats.target[name].as_ref().map(f);
    for r in runs {
        t.rows.push(vec![
            Cell::Int(Some(r.run as i64)),
            Cell::text(r.exit.map_or("-".to_string(), |c| c.to_string())),
            Cell::text(ms(r.wall_ms)),
            Cell::Bytes(get(&r.stats, "rss", |s| s.max).map(|v| v as i64)),
            Cell::Bytes(get(&r.stats, "anon", |s| s.max).map(|v| v as i64)),
            Cell::Pct(get(&r.stats, "cpu", |s| s.avg)),
            Cell::Int(get(&r.stats, "threads", |s| s.max).map(|v| v as i64)),
            Cell::Int(get(&r.stats, "procs", |s| s.max).map(|v| v as i64)),
            Cell::text(if r.skipped {
                "skipped"
            } else if r.status != "done" {
                r.status
            } else {
                ""
            }),
        ]);
    }
    text.push_str(&t.render());
    text.push_str("\n\n");
    let runs_label = match (final_.runs.first(), final_.runs.last()) {
        (Some(a), Some(b)) if a != b => format!("runs {a}–{b}"),
        (Some(a), _) => format!("run {a}"),
        _ => "no runs".into(),
    };
    // Column titles are static; one small leak per `psm track`, not per row.
    let title: &'static str = Box::leak(format!("FINAL ({runs_label})").into_boxed_str());
    let mut f = Table::new(&[
        (title, "row"),
        ("MIN", "min"),
        ("MAX", "max"),
        ("AVG", "avg"),
        ("SPREAD", "spread"),
    ]);
    if let Some(w) = &final_.wall_ms {
        f.rows.push(vec![
            Cell::text("wall"),
            Cell::text(ms(w.min as u64)),
            Cell::text(ms(w.max as u64)),
            Cell::text(ms(w.avg as u64)),
            Cell::text(ms(w.spread as u64)),
        ]);
    }
    for (row, kind) in FINAL_ROWS {
        let Some(s) = &final_.rows[row] else { continue };
        f.rows.push(vec![
            Cell::text(row.replace('_', " ")),
            cell(s.min, kind),
            cell(s.max, kind),
            cell(s.avg, kind),
            cell(s.spread, kind),
        ]);
    }
    text.push_str(&f.render());
    text.push('\n');
    text.push_str(LEGEND);
    text
}
