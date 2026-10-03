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
use std::time::Instant;

use anyhow::{Context, Result};
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
            filter: self.filter(&v.filter)?,
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
            "procs", "info", "new", "snap", "list", "diff", "report", "status",
        ],
    ),
    (
        "Sessions and snapshots:",
        &["sessions", "snapshots", "export", "import"],
    ),
    ("Setup:", &["init", "config", "backup", "completions"]),
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

pub fn run(cli: Cli) -> Result<()> {
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
    match cli.cmd.unwrap_or(Cmd::Status) {
        Cmd::Procs {
            view,
            interval,
            cmd: None,
        }
        | Cmd::Procs {
            cmd: Some(ProcsCmd::List { view, interval }),
            ..
        } => views::list(&ctx, &view, &interval),
        Cmd::Procs {
            cmd: Some(ProcsCmd::Show { snapshot, view }),
            ..
        } => views::show(&ctx, &snapshot, &view),
        Cmd::Info {
            n,
            by,
            group,
            interval,
            deep,
            filter,
        } => views::info(&ctx, n, &by, &group, &interval, deep, &filter),
        Cmd::New { name, capture } => capture::new_session(&ctx, name, &capture),
        Cmd::Snap { label, capture } => capture::snap(&ctx, label, &capture),
        Cmd::Status => sessions::status(&ctx),
        Cmd::List => sessions::snapshots(&ctx),
        Cmd::Report { kind, diff: args } => compare::report(&ctx, kind, &args),
        Cmd::Sessions { cmd: None }
        | Cmd::Sessions {
            cmd: Some(SessionsCmd::List),
        } => sessions::sessions(&ctx),
        Cmd::Sessions {
            cmd: Some(SessionsCmd::Purge { older_than }),
        } => maintenance::purge(&ctx, &older_than),
        Cmd::Sessions { cmd: Some(cmd) } => match cmd {
            SessionsCmd::Activate { session } => sessions::switch(&ctx, &session),
            SessionsCmd::Deactivate => sessions::deactivate(&ctx),
            SessionsCmd::Compare {
                a,
                b,
                group,
                metric,
                top,
                filter,
            } => compare::sessions(&ctx, &a, &b, group, metric, top, filter),
            SessionsCmd::Export {
                session,
                all,
                format,
                no_cmdline,
            } => sessions::export(&ctx, session, all, format, no_cmdline),
            SessionsCmd::Import { file, name } => sessions::import(&ctx, &file, name),
            SessionsCmd::Delete { session } => sessions::delete(&ctx, &session),
            SessionsCmd::Reset { yes } => maintenance::reset(&ctx, yes),
            SessionsCmd::List | SessionsCmd::Purge { .. } => unreachable!("matched above"),
        },
        Cmd::Snapshots { cmd: None }
        | Cmd::Snapshots {
            cmd: Some(SnapshotsCmd::List),
        } => sessions::snapshots(&ctx),
        Cmd::Snapshots {
            cmd: Some(SnapshotsCmd::Delete { snapshot }),
        } => sessions::delete_snapshot(&ctx, snapshot.as_deref()),
        Cmd::Snapshots {
            cmd: Some(SnapshotsCmd::Reset { yes, capture }),
        } => maintenance::purge_session(&ctx, yes, &capture),
        Cmd::Diff(args) => compare::diff(&ctx, &args),
        Cmd::Export {
            all: true,
            format,
            no_cmdline,
            ..
        } => sessions::export(&ctx, None, false, format, no_cmdline),
        Cmd::Export {
            snapshot,
            format,
            no_cmdline,
            ..
        } => sessions::export_snapshot(&ctx, snapshot, format, no_cmdline),
        Cmd::Import { file } => sessions::import_snapshots(&ctx, &file),
        Cmd::Backup { path } => maintenance::backup(&ctx, &path),
        Cmd::Config { .. }
        | Cmd::Completions { .. }
        | Cmd::Init { .. }
        | Cmd::Faq { .. }
        | Cmd::Help { .. }
        | Cmd::Version => {
            unreachable!("handled before the configuration is loaded")
        }
    }
}
