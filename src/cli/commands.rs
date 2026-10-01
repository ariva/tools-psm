//! The commands, with their help texts and examples.

use std::path::PathBuf;

use clap::{Subcommand, ValueEnum};

use super::args::{CaptureArgs, DiffArgs, FilterArgs, ViewArgs};
use super::values::{group_keys, info_tables, metrics};

const REFERENCES: &str = "\
Snapshot references:
  baseline   first snapshot of the session
  latest     newest snapshot of the session
  prev       the snapshot before the one it is compared with
  now        the live state; collected for this command, never stored
  <id>       a snapshot id, see `psm snapshots`
  <label>    newest snapshot with that label";

#[derive(Clone, Copy, PartialEq, ValueEnum)]
pub enum ReportKind {
    /// Memory impact ranking (same as `diff --memory`)
    Memory,
    /// Process count per program, changed or not
    Processes,
    /// New processes
    New,
    /// Gone processes
    Gone,
    /// Programs that grew, with the relative change
    Growth,
    /// CPU seconds used between the two snapshots, per program
    Cpu,
    /// One row per snapshot of the session
    Timeline,
    /// Every /proc/meminfo field that changed
    Meminfo,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Processes now, with CPU and memory
    #[command(after_help = "\
Examples:
  psm list                         every process, largest memory first
  psm list --sort cpu --top 10     the ten busiest
  psm list --group name            one row per program
  psm list --group cgroup          one row per application (systemd)
  psm list --user alice --name chrome")]
    List {
        #[command(flatten)]
        view: ViewArgs,
        /// CPU sampling window; 0 = lifetime average [default: 500ms]
        #[arg(long, value_name = "DURATION")]
        interval: Option<String>,
    },
    /// System memory plus the top consumers per metric
    #[command(after_help = "\
Examples:
  psm info                  top 5 by CPU, memory and threads
  psm info 10 --by cpu,mem  top 10, two tables
  psm info --group name     rank programs instead of single processes")]
    Info {
        /// Rows per table
        #[arg(default_value_t = 5)]
        n: usize,
        /// Tables to show, comma-separated
        #[arg(long, value_delimiter = ',', default_value = "cpu,mem,threads", value_parser = info_tables())]
        by: Vec<String>,
        /// Rank distinct groups instead of single processes
        #[arg(long, value_name = "KEY", value_parser = group_keys())]
        group: Option<String>,
        /// CPU sampling window; 0 = lifetime average [default: 500ms]
        #[arg(long, value_name = "DURATION")]
        interval: Option<String>,
        /// Rank memory by PSS instead of RSS
        #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true", value_name = "BOOL")]
        deep: Option<bool>,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// New session + baseline snapshot (the active session becomes inactive)
    ///
    /// The same as `psm session new`. The first time, it also creates the
    /// config file at its default location.
    New {
        /// Session name [default: session-YYYYMMDD-HHMMSS]
        name: Option<String>,
        #[command(flatten)]
        capture: CaptureArgs,
    },
    /// Another snapshot in the active session
    Snap {
        /// Label for the snapshot (not baseline, latest, prev or now)
        label: Option<String>,
        #[command(flatten)]
        capture: CaptureArgs,
    },
    /// Active session summary (same as running without a command)
    Status,
    /// One stored snapshot, same view as list
    #[command(after_help = REFERENCES)]
    Show {
        /// baseline, latest, prev, now, an id, or a label
        #[arg(default_value = "latest")]
        snapshot: String,
        #[command(flatten)]
        view: ViewArgs,
    },
    /// Changes between two snapshots [default: baseline -> now, the live state]
    #[command(after_help = format!("\
Examples:
  psm diff                     baseline -> now
  psm diff prev                last snapshot -> now
  psm diff 1                   snapshot 1 -> now
  psm diff 1 2                 two stored snapshots
  psm diff --group name        the whole diff per program
  psm diff --memory --top 10   memory ranking only

{REFERENCES}"))]
    Diff(DiffArgs),
    /// One report over two snapshots [default: baseline -> now], or the session timeline
    #[command(after_help = format!("\
Examples:
  psm report growth                 programs that grew since the baseline
  psm report meminfo prev           system memory fields changed since the last snapshot
  psm report cpu 1 2                CPU time between two snapshots
  psm report timeline --name chrome one program across all snapshots

{REFERENCES}"))]
    Report {
        /// Which report
        kind: ReportKind,
        #[command(flatten)]
        diff: DiffArgs,
    },
    /// Snapshots of the session
    Snapshots,
    /// All sessions
    Sessions,
    /// Two sessions, by program (latest snapshot of each)
    Compare {
        /// Older session (name or id)
        a: String,
        /// Newer session (name or id)
        b: String,
        /// Group by this key [default: name]
        #[arg(long, value_name = "KEY", value_parser = group_keys())]
        group: Option<String>,
        /// What counts as memory in the comparison
        #[arg(long, value_parser = metrics())]
        metric: Option<String>,
        /// Only the first N programs
        #[arg(long, value_name = "N")]
        top: Option<usize>,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Manage whole sessions: new, export, import, deactivate, delete
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Make another session the active one
    ///
    /// There is one active session: the one `snap` adds to and `diff`
    /// compares against the live state. The others are inactive but kept;
    /// switch back to any of them at any time.
    #[command(after_help = "\
Examples:
  psm sessions          see the names and ids
  psm switch chrome-153
  psm switch 2")]
    Switch {
        /// Session name or id
        session: String,
    },
    /// Delete inactive sessions older than the given age
    Purge {
        /// Age, e.g. 180d
        #[arg(long, value_name = "AGE")]
        older_than: String,
    },
    /// Delete ALL sessions and snapshots (asks first)
    ///
    /// Removes the database file, so every session, active and inactive,
    /// is gone. The configuration file is kept. Asks for confirmation
    /// unless --yes is given; anything but "y" or "yes" deletes nothing.
    /// `psm backup <path>` makes a copy first.
    Reset {
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Show which configuration file is used; --init creates it
    Config {
        /// Write the file with the default settings (never overwrites)
        #[arg(long)]
        init: bool,
    },
    /// Copy the database to PATH
    Backup {
        /// Destination file; must not exist
        path: PathBuf,
    },
    /// First-time setup: config file, database, shell completions
    ///
    /// Creates the config file and the database if they are missing, and
    /// writes the completion script for your shell (from $SHELL, or
    /// --shell). Safe to run again: existing config and database are left
    /// alone, the completion script is refreshed.
    Init {
        /// Shell to install completions for [default: from $SHELL]
        #[arg(long)]
        shell: Option<clap_complete::Shell>,
    },
    /// Common questions and the command that answers each
    ///
    /// With words, only rows whose question or command contains every
    /// word are shown, ignoring case: `psm faq what`, `psm faq memory grew`.
    Faq {
        /// Words to filter by (all must match, case-insensitive)
        words: Vec<String>,
    },
    /// Print a shell completion script for psm's commands, options and values
    #[command(after_help = "\
Setup (once, then open a new shell):
  bash   psm completions bash > ~/.local/share/bash-completion/completions/psm
  zsh    psm completions zsh > ~/.zfunc/_psm        (with ~/.zfunc in fpath)
  fish   psm completions fish > ~/.config/fish/completions/psm.fish

Session names and snapshot labels are not completed; they live in the database.")]
    Completions {
        /// Shell to generate for
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
pub enum SessionCmd {
    /// New session + baseline snapshot (same as `psm new`)
    New {
        /// Session name [default: session-YYYYMMDD-HHMMSS]
        name: Option<String>,
        #[command(flatten)]
        capture: CaptureArgs,
    },
    /// Dump a session as JSON or CSV
    ///
    /// JSON is complete and can be loaded again with `psm session import`.
    /// CSV is one row per process, for spreadsheets; it cannot be imported.
    #[command(after_help = "\
Examples:
  psm session export > chrome-154.json       the active session
  psm session export chrome-153 > old.json   another session, by name or id
  psm session export --all > everything.json  every session, for psm session import
  psm session export --format csv > procs.csv")]
    Export {
        /// Session name or id [default: the active session]
        session: Option<String>,
        /// Every session (JSON only)
        #[arg(long, conflicts_with = "session")]
        all: bool,
        /// Output format
        #[arg(long, value_enum, default_value = "json")]
        format: ExportFormat,
        /// Leave command lines out
        #[arg(long)]
        no_cmdline: bool,
    },
    /// Load sessions written by `psm session export` (JSON)
    ///
    /// Reads a single-session export or an --all export. Sessions keep
    /// their snapshots and timestamps and arrive inactive, so the active
    /// session is not disturbed. Read them with --session, make one active
    /// with `psm switch`, or use `psm compare`.
    #[command(after_help = "\
Examples:
  psm session export > chrome-153.json                   on one machine
  psm session import chrome-153.json                     on another
  psm session import chrome-153.json --name chrome-old   under a different name
  psm session import everything.json                     all sessions of an --all export
  psm session export | psm --db other.db session import -")]
    Import {
        /// File written by `psm session export`; `-` reads standard input
        file: PathBuf,
        /// Name for the imported session [default: the exported name]; single-session exports only
        #[arg(long)]
        name: Option<String>,
    },
    /// Make the active session inactive, leaving none active
    ///
    /// Nothing is deleted. `psm switch` makes a session active again.
    Deactivate,
    /// Delete one session and its snapshots
    #[command(after_help = "\
Examples:
  psm sessions                   see the names and ids
  psm session delete chrome-153
  psm session delete 2")]
    Delete {
        /// Session name or id
        session: String,
    },
}
