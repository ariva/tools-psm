//! One file per command group. `Ctx` is the merged settings every handler
//! gets; `run` is the dispatch.

mod capture;
mod compare;
mod faq;
mod maintenance;
mod sessions;
mod setup;
mod views;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use regex::Regex;

use crate::analysis::view::{self, View};
use crate::cli::{
    CaptureArgs, Cli, Cmd, FilterArgs, ProcsCmd, SessionsCmd, SnapshotsCmd, ViewArgs,
};
use crate::collect::procfs as collector;
use crate::config::{self, Config};
use crate::model::{Filter, Snapshot};
use crate::output::{Table, out, parse_duration, print_json};
use crate::store::Db;

/// Settings after merging flags, environment and the configuration file.
pub struct Ctx {
    pub cfg: Config,
    pub json: bool,
    pub kernel: bool,
    pub proc_root: PathBuf,
    pub db_path: Option<PathBuf>,
    /// No `--config` / `PSM_CONFIG`: the default location is in use.
    pub default_config: bool,
}

impl Ctx {
    pub fn db_path(&self) -> Result<PathBuf> {
        Ok(match (&self.db_path, &self.cfg.database) {
            (Some(p), _) => p.clone(),
            (None, Some(p)) => config::expand_tilde(p),
            (None, None) => config::default_db_path()?,
        })
    }

    pub fn db(&self) -> Result<Db> {
        Db::open(&self.db_path()?)
    }

    pub fn filter(&self, f: &FilterArgs) -> Result<Filter> {
        Ok(Filter {
            user: f.user.clone(),
            name: f.name.clone(),
            exe: f.exe.clone(),
            pids: f.pid.clone(),
            cmdline: f.cmdline.clone(),
            search: Vec::new(),
            match_case: f.match_case,
            exclude: f
                .exclude_regex
                .as_deref()
                .map(Regex::new)
                .transpose()
                .context("invalid --exclude-regex")?,
            kernel: self.kernel,
        })
    }

    /// `pid` means "do not group", so a configured default can be switched off.
    pub fn group(&self, flag: &Option<String>) -> Option<String> {
        flag.clone()
            .or_else(|| self.cfg.display.group.clone())
            .filter(|g| g != "pid")
    }

    pub fn view(&self, v: &ViewArgs, deep: bool) -> Result<View> {
        Ok(View {
            group: self.group(&v.group),
            sort: v.sort.clone().unwrap_or_else(|| "mem".into()),
            top: v.top.or(self.cfg.display.top),
            deep,
            filter: Filter {
                search: v.words.clone(),
                ..self.filter(&v.filter)?
            },
        })
    }

    pub fn collect(&self, deep: bool, cmdline: bool) -> Result<(Snapshot, usize)> {
        collector::collect(&collector::Options {
            root: self.proc_root.clone(),
            deep,
            cmdline,
            io: self.cfg.collection.io,
            cgroups: self.cfg.collection.cgroups,
        })
    }

    pub fn capture(&self, c: &CaptureArgs) -> Result<Snapshot> {
        let deep = c.deep.unwrap_or(self.cfg.collection.deep);
        let (snapshot, restricted) =
            self.collect(deep, self.cfg.collection.cmdline && !c.no_cmdline)?;
        note_restricted(restricted, &snapshot);
        Ok(snapshot)
    }

    /// Current processes plus `%CPU` per pid. A percentage needs two readings
    /// `interval` apart; with 0 it is the lifetime average instead.
    pub fn live(
        &self,
        deep: bool,
        interval: &Option<String>,
    ) -> Result<(Snapshot, HashMap<i64, f64>, usize)> {
        let interval = parse_duration(interval.as_deref().unwrap_or(&self.cfg.display.interval))?;
        if interval.is_zero() {
            let (snapshot, restricted) = self.collect(deep, true)?;
            let cpu = view::lifetime_cpu(&snapshot);
            return Ok((snapshot, cpu, restricted));
        }
        let started = Instant::now();
        let first = collector::cpu_ticks(&self.proc_root)?;
        std::thread::sleep(interval);
        let elapsed = started.elapsed().as_secs_f64();
        let (snapshot, restricted) = self.collect(deep, true)?;
        // Processes that started inside the window have no first reading: no %CPU.
        let cpu = snapshot
            .processes
            .iter()
            .filter_map(|p| {
                let before = first.get(&(p.pid, p.start_time))?;
                let pct = view::cpu_percent(
                    p.cpu_user + p.cpu_system - before,
                    elapsed,
                    snapshot.clk_tck,
                )?;
                Some((p.pid, pct))
            })
            .collect();
        Ok((snapshot, cpu, restricted))
    }

    pub fn emit(&self, table: &Table, csv: bool) {
        if self.json {
            print_json(&table.json());
        } else if csv {
            out(table.csv());
        } else {
            out(table.render());
        }
    }
}

/// `288 processes (+302 kernel)`
pub fn census(s: &Snapshot) -> String {
    let kthreads = s.processes.iter().filter(|p| p.kthread).count();
    format!(
        "{} processes (+{kthreads} kernel)",
        s.processes.len() - kthreads
    )
}

pub fn note_restricted(restricted: usize, s: &Snapshot) {
    if restricted > 0 {
        let total = s.processes.iter().filter(|p| !p.kthread).count();
        eprintln!(
            "{restricted} of {total} processes partially readable (run as root for full data)"
        );
    }
}

/// `psm help [command...]`, `-h`, `--help`: clap's help, brief for `-h`.
fn help(path: &[String]) -> Result<()> {
    let mut root = crate::cli::command();
    root.build();
    let cmd = path.iter().try_fold(&mut root, |c, name| {
        c.find_subcommand_mut(name)
            .with_context(|| format!("unknown command `{name}`; `psm help` lists them"))
    })?;
    // The subcommand carries the words, so the flag itself is only in argv.
    let brief = std::env::args().skip(1).any(|a| a == "-h");
    let text = if brief {
        cmd.render_help()
    } else {
        cmd.render_long_help()
    }
    .to_string();
    // The top-level list is regrouped; a command's own help is clap's as is.
    let text = if path.is_empty() {
        let shorts: Vec<(String, char)> = root
            .get_subcommands()
            .filter_map(|sc| sc.get_short_flag().map(|s| (sc.get_name().to_string(), s)))
            .collect();
        grouped_commands(&text, &shorts)
    } else {
        text
    };
    // clap's rendering ends with a newline and `out` adds one: keep one.
    out(text.trim_end_matches('\n'));
    Ok(())
}

/// The top-level command list, in groups, as `name, -x`: the long flag forms
/// (`snap, -s, --snap`) double the width and read like options.
const COMMAND_GROUPS: &[(&str, &[&str])] = &[
    (
        "Commands:",
        &[
            "procs", "pid", "info", "new", "snap", "list", "diff", "report", "status",
        ],
    ),
    (
        "Sessions and snapshots:",
        &["sessions", "snapshots", "export", "import"],
    ),
    (
        "Setup:",
        &["init", "update", "config", "backup", "completions"],
    ),
    ("Help:", &["faq", "help", "version"]),
];

fn grouped_commands(help: &str, shorts: &[(String, char)]) -> String {
    let Some(start) = help.find("Commands:\n") else {
        return help.to_string();
    };
    let body = &help[start + "Commands:\n".len()..];
    let end = body.find("\n\n").unwrap_or(body.len());
    let rows = &body[..end];
    // `  name, -x, --name   about` -> (name, about)
    let mut about: Vec<(String, String)> = Vec::new();
    for line in rows.lines() {
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        let name_end = rest.find([',', ' ']).unwrap_or(rest.len());
        let name = &rest[..name_end];
        let desc = rest[name_end..]
            .split("  ")
            .skip(1)
            .map(str::trim)
            .find(|s| !s.is_empty())
            .unwrap_or_default();
        about.push((name.to_string(), desc.to_string()));
    }
    // `snap, -s` when the command has a letter, else just the name.
    let label = |name: &str| match shorts.iter().find(|(n, _)| n == name) {
        Some((_, s)) => format!("{name}, -{s}"),
        None => name.to_string(),
    };
    let width = about.iter().map(|(n, _)| label(n).len()).max().unwrap_or(0);
    let mut block = String::new();
    let mut placed = Vec::new();
    for (heading, names) in COMMAND_GROUPS {
        let mut rows = String::new();
        for name in names.iter() {
            if let Some((n, d)) = about.iter().find(|(n, _)| n == name) {
                rows.push_str(&format!("  {:<width$}  {d}\n", label(n)));
                placed.push(n.clone());
            }
        }
        if !rows.is_empty() {
            block.push_str(&format!("{heading}\n{rows}\n"));
        }
    }
    // Anything new that is not in a group yet still shows up.
    let rest: String = about
        .iter()
        .filter(|(n, _)| !placed.contains(n))
        .map(|(n, d)| format!("  {:<width$}  {d}\n", label(n)))
        .collect();
    if !rest.is_empty() {
        block.push_str(&format!("Other:\n{rest}\n"));
    }
    format!(
        "{}{}{}",
        &help[..start],
        block.trim_end_matches('\n'),
        &body[end..]
    )
}

/// Commands whose standard output is data (a file, a script) or already
/// starts with the version line: no banner for them.
fn prints_data(cmd: &Option<Cmd>) -> bool {
    matches!(
        cmd,
        Some(Cmd::Version)
            | Some(Cmd::Help { .. })
            | Some(Cmd::Completions { .. })
            | Some(Cmd::Export { .. })
            | Some(Cmd::Sessions {
                cmd: Some(SessionsCmd::Export { .. })
            })
    )
}

pub fn run(cli: Cli) -> Result<()> {
    // Every command a person reads opens with the version line; data for
    // pipes and scripts (--json, exports, a redirected stdout) stays clean.
    if !cli.json && !prints_data(&cli.cmd) && std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        out(format!("{}\n", crate::cli::commands::VERSION_LINE));
    }
    // Handled before loading the configuration: these must work without one.
    if cli.bare_config() {
        return setup::config(cli.config_path().as_deref(), false);
    }
    if let Some(Cmd::Config { init }) = &cli.cmd {
        return setup::config(cli.config_path().as_deref(), *init);
    }
    if let Some(Cmd::Init { .. }) = &cli.cmd {
        return setup::init(&cli);
    }
    if let Some(Cmd::Update { .. }) = &cli.cmd {
        return setup::update(&cli);
    }
    if let Some(Cmd::Faq { words }) = &cli.cmd {
        faq::faq(words, cli.json);
        return Ok(());
    }
    if let Some(Cmd::Help { command }) = &cli.cmd {
        return help(command);
    }
    if let Some(Cmd::Version) = &cli.cmd {
        out(crate::cli::commands::version());
        return Ok(());
    }
    if let Some(Cmd::Completions { shell }) = cli.cmd {
        crate::output::out(crate::cli::completions::script(shell));
        return Ok(());
    }
    let config_path = cli.config_path();
    let db_flag = cli.db_flag();
    let bare_db = cli.bare_db();
    let default_config = config_path.is_none();
    let cfg = config::load(config_path.as_deref())?;
    crate::output::set_units(&cfg.display.units)?;
    let ctx = Ctx {
        json: cli.json,
        kernel: cli.kernel.unwrap_or(cfg.display.kernel),
        proc_root: cli.proc_root,
        db_path: db_flag,
        default_config,
        cfg,
    };

    if bare_db {
        return setup::database(&ctx);
    }
    let cmd = cli.cmd.unwrap_or(Cmd::Status);
    match cli.watch {
        Some(every) => watch(&ctx, cmd, &every),
        None => dispatch(&ctx, cmd),
    }
}

/// `--watch`: clear the screen and run a live view again every `every`
/// until Ctrl-C. With `--json` nothing is cleared: one document per round.
fn watch(ctx: &Ctx, cmd: Cmd, every: &str) -> Result<()> {
    if !watchable(&cmd) {
        bail!(
            "--watch repeats live views only: procs, info, procs show now, diff/report against now"
        );
    }
    let every = parse_duration(every)?;
    if every.is_zero() {
        bail!("--watch needs a duration above zero");
    }
    if ctx.json {
        loop {
            dispatch(ctx, cmd.clone())?;
            std::thread::sleep(every);
        }
    }
    // The alternate screen, as `watch(1)` and `top` use it: frames never
    // reach the scrollback, and the shell's screen comes back on exit.
    unsafe {
        let handler = leave_alt_screen as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGHUP, handler);
    }
    print!("{ALT_ON}");
    let result = (1..).try_for_each(|round| {
        // Clear the screen, cursor home; the version line stays out of the loop.
        print!("\x1b[2J\x1b[H");
        dispatch(ctx, cmd.clone())?;
        countdown(every, round);
        Ok(())
    });
    // An error must be readable: leave the alternate screen before it prints.
    print!("{ALT_OFF}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    result
}

const ALT_ON: &str = "\x1b[?1049h";
const ALT_OFF: &str = "\x1b[?1049l";

/// Ctrl-C inside `--watch`: restore the screen, then exit as the signal
/// would have. Only `write` and `_exit` here; nothing else is safe in a
/// signal handler.
extern "C" fn leave_alt_screen(_: libc::c_int) {
    unsafe {
        libc::write(1, ALT_OFF.as_ptr().cast(), ALT_OFF.len());
        libc::_exit(130);
    }
}

/// `Every 10s, round 3, next refresh in 7s, Ctrl-C stops`
fn footer(every: Duration, round: u32, left: Duration) -> String {
    format!(
        "Every {}s, round {round}, next refresh in {}s, Ctrl-C stops",
        every.as_secs_f64(),
        left.as_secs_f64().ceil()
    )
}

/// The footer under the output, rewritten in place once a second. The
/// cursor never leaves that line, so scrolling cannot misplace it.
fn countdown(every: Duration, round: u32) {
    let started = Instant::now();
    println!();
    loop {
        let left = every.saturating_sub(started.elapsed());
        if left.is_zero() {
            break;
        }
        // Carriage return, overwrite, clear the rest of the line.
        print!("\r{}\x1b[K", footer(every, round, left));
        let _ = std::io::Write::flush(&mut std::io::stdout());
        std::thread::sleep(left.min(Duration::from_secs(1)));
    }
}

/// Commands that read the live state: the only ones worth repeating.
fn watchable(cmd: &Cmd) -> bool {
    let live_side = |d: &crate::cli::DiffArgs| {
        d.a.as_deref() == Some("now") || d.b.as_deref().unwrap_or("now") == "now"
    };
    match cmd {
        Cmd::Procs { cmd: None, .. }
        | Cmd::Procs {
            cmd: Some(ProcsCmd::List { .. }),
            ..
        }
        | Cmd::Info { .. } => true,
        Cmd::Procs {
            cmd: Some(ProcsCmd::Show { snapshot, .. }),
            ..
        }
        | Cmd::Pid { snapshot, .. } => snapshot == "now",
        Cmd::Diff(args) | Cmd::Report { diff: args, .. } => live_side(args),
        _ => false,
    }
}

fn dispatch(ctx: &Ctx, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Procs {
            view,
            interval,
            cmd: None,
        }
        | Cmd::Procs {
            cmd: Some(ProcsCmd::List { view, interval }),
            ..
        } => views::list(ctx, &view, &interval),
        Cmd::Procs {
            cmd: Some(ProcsCmd::Show { snapshot, view }),
            ..
        } => views::show(ctx, &snapshot, &view),
        Cmd::Pid { pid, snapshot } => views::pid(ctx, pid, &snapshot),
        Cmd::Info {
            n,
            top,
            by,
            group,
            interval,
            deep,
            filter,
        } => views::info(ctx, top.unwrap_or(n), &by, &group, &interval, deep, &filter),
        Cmd::New {
            name,
            description,
            capture,
        } => capture::new_session(ctx, name, description, &capture),
        Cmd::Snap {
            label,
            description,
            capture,
        } => capture::snap(ctx, label, description, &capture),
        Cmd::Status => sessions::status(ctx),
        Cmd::List => sessions::snapshots(ctx),
        Cmd::Report { kind, diff: args } => compare::report(ctx, kind, &args),
        Cmd::Sessions { cmd: None }
        | Cmd::Sessions {
            cmd: Some(SessionsCmd::List),
        } => sessions::sessions(ctx),
        Cmd::Sessions {
            cmd: Some(SessionsCmd::Purge { older_than }),
        } => maintenance::purge(ctx, &older_than),
        Cmd::Sessions { cmd: Some(cmd) } => match cmd {
            SessionsCmd::Activate { session } => sessions::switch(ctx, &session),
            SessionsCmd::Deactivate => sessions::deactivate(ctx),
            SessionsCmd::Compare {
                a,
                b,
                group,
                metric,
                top,
                filter,
            } => compare::sessions(ctx, &a, &b, group, metric, top, filter),
            SessionsCmd::Export {
                session,
                all,
                format,
                no_cmdline,
            } => sessions::export(ctx, session, all, format, no_cmdline),
            SessionsCmd::Import { file, name } => sessions::import(ctx, &file, name),
            SessionsCmd::Delete { session } => sessions::delete(ctx, &session),
            SessionsCmd::Rename {
                session,
                name,
                description,
            } => sessions::rename(ctx, &session, &name, description.as_deref()),
            SessionsCmd::Reset { yes } => maintenance::reset(ctx, yes),
            SessionsCmd::List | SessionsCmd::Purge { .. } => unreachable!("matched above"),
        },
        Cmd::Snapshots { cmd: None }
        | Cmd::Snapshots {
            cmd: Some(SnapshotsCmd::List),
        } => sessions::snapshots(ctx),
        Cmd::Snapshots {
            cmd: Some(SnapshotsCmd::Delete { snapshot }),
        } => sessions::delete_snapshot(ctx, snapshot.as_deref()),
        Cmd::Snapshots {
            cmd:
                Some(SnapshotsCmd::Rename {
                    snapshot,
                    label,
                    description,
                }),
        } => sessions::rename_snapshot(ctx, &snapshot, &label, description.as_deref()),
        Cmd::Snapshots {
            cmd: Some(SnapshotsCmd::Reset { yes, capture }),
        } => maintenance::purge_session(ctx, yes, &capture),
        Cmd::Diff(args) => compare::diff(ctx, &args),
        Cmd::Export {
            all: true,
            format,
            no_cmdline,
            ..
        } => sessions::export(ctx, None, false, format, no_cmdline),
        Cmd::Export {
            snapshot,
            format,
            no_cmdline,
            ..
        } => sessions::export_snapshot(ctx, snapshot, format, no_cmdline),
        Cmd::Import { file } => sessions::import_snapshots(ctx, &file),
        Cmd::Backup { path } => maintenance::backup(ctx, &path),
        Cmd::Config { .. }
        | Cmd::Completions { .. }
        | Cmd::Init { .. }
        | Cmd::Update { .. }
        | Cmd::Faq { .. }
        | Cmd::Help { .. }
        | Cmd::Version => {
            unreachable!("handled before the configuration is loaded")
        }
    }
}
