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

use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use regex::Regex;

use crate::analysis::view::{self, View};
use crate::cli::{CaptureArgs, Cli, Cmd, FilterArgs, SessionCmd, ViewArgs};
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
    pub session: Option<String>,
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

    pub fn writes_active_session(&self, command: &str) -> Result<()> {
        if self.session.is_some() {
            bail!("`psm {command}` always works on the active session; drop --session");
        }
        Ok(())
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

pub fn run(cli: Cli) -> Result<()> {
    // Handled before loading the configuration: these must work without one.
    if let Some(Cmd::Config { init }) = &cli.cmd {
        return setup::config(cli.config.as_deref(), *init);
    }
    if let Some(Cmd::Init { .. }) = &cli.cmd {
        return setup::init(&cli);
    }
    if let Some(Cmd::Faq { words }) = &cli.cmd {
        faq::faq(words, cli.json);
        return Ok(());
    }
    if let Some(Cmd::Completions { shell }) = cli.cmd {
        clap_complete::generate(shell, &mut Cli::command(), "psm", &mut std::io::stdout());
        return Ok(());
    }
    let default_config = cli.config.is_none();
    let cfg = config::load(cli.config.as_deref())?;
    crate::output::set_units(&cfg.display.units)?;
    let ctx = Ctx {
        json: cli.json,
        kernel: cli.kernel.unwrap_or(cfg.display.kernel),
        proc_root: cli.proc_root,
        db_path: cli.db,
        session: cli.session,
        default_config,
        cfg,
    };

    match cli.cmd.unwrap_or(Cmd::Status) {
        Cmd::List { view, interval } => views::list(&ctx, &view, &interval),
        Cmd::Info {
            n,
            by,
            group,
            interval,
            deep,
            filter,
        } => views::info(&ctx, n, &by, &group, &interval, deep, &filter),
        Cmd::Show { snapshot, view } => views::show(&ctx, &snapshot, &view),
        Cmd::New { name, capture } => capture::new_session(&ctx, name, &capture),
        Cmd::Snap { label, capture } => capture::snap(&ctx, label, &capture),
        Cmd::Status => sessions::status(&ctx),
        Cmd::Diff(args) => compare::diff(&ctx, &args),
        Cmd::Report { kind, diff: args } => compare::report(&ctx, kind, &args),
        Cmd::Compare {
            a,
            b,
            group,
            metric,
            top,
            filter,
        } => compare::sessions(&ctx, &a, &b, group, metric, top, filter),
        Cmd::Snapshots => sessions::snapshots(&ctx),
        Cmd::Sessions => sessions::sessions(&ctx),
        Cmd::Switch { session } => sessions::switch(&ctx, &session),
        Cmd::Session { cmd } => match cmd {
            SessionCmd::New { name, capture } => capture::new_session(&ctx, name, &capture),
            SessionCmd::Export {
                session,
                all,
                format,
                no_cmdline,
            } => sessions::export(&ctx, session, all, format, no_cmdline),
            SessionCmd::Import { file, name } => sessions::import(&ctx, &file, name),
            SessionCmd::Deactivate => sessions::deactivate(&ctx),
            SessionCmd::Delete { session } => sessions::delete(&ctx, &session),
        },
        Cmd::Purge { older_than } => maintenance::purge(&ctx, &older_than),
        Cmd::Backup { path } => maintenance::backup(&ctx, &path),
        Cmd::Reset { yes } => maintenance::reset(&ctx, yes),
        Cmd::Config { .. } | Cmd::Completions { .. } | Cmd::Init { .. } | Cmd::Faq { .. } => {
            unreachable!("handled before the configuration is loaded")
        }
    }
}
