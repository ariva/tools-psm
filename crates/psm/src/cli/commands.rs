//! The commands, with their help texts and examples.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Subcommand, ValueEnum};

use super::args::{CaptureArgs, DiffArgs, FilterArgs, ViewArgs, WatchArg};
use super::values::{group_keys, info_tables, metrics};

const REFERENCES: &str = "\
Snapshot references:
  baseline   first snapshot of the session
  base       the same, unless a snapshot is labelled base
  latest     newest snapshot of the session
  prev       the snapshot before the one it is compared with
  now        the live state; collected for this command, never stored
  <n>        a snapshot number within the session: 0 is the baseline, see `psm snapshots`
  <label>    newest snapshot with that label";

#[derive(Clone, Copy, PartialEq, ValueEnum)]
pub enum ReportKind {
    /// Memory impact ranking (same as `snapshots diff --memory`)
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
    /// Every program across all snapshots of the session plus now: growing, shrinking, noisy or flat
    Trend,
    /// Every /proc/meminfo field that changed
    Meminfo,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Json,
    Csv,
}

// Procs carries the view options, filters and the word search; clap builds it once.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Subcommand)]
pub enum Cmd {
    /// Processes: now (list, the default) or of a stored snapshot (show)
    #[command(
        short_flag = 'p',
        args_conflicts_with_subcommands = true,
        after_help = "\
Examples:
  psm procs                        every process now, largest memory first (= psm procs list)
  psm procs --sort cpu --top 10    the ten busiest
  psm procs --group name           one row per program
  psm procs --group cgroup         one row per application (systemd)
  psm procs --user alice --name chrome
  psm procs show baseline          the processes of a stored snapshot"
    )]
    Procs {
        #[command(flatten)]
        view: ViewArgs,
        /// CPU sampling window; 0 = lifetime average [default: 500ms]
        #[arg(long, value_name = "DURATION")]
        interval: Option<String>,
        #[command(flatten)]
        watch: WatchArg,
        #[command(subcommand)]
        cmd: Option<ProcsCmd>,
    },
    /// One process on one screen: what it is, where it comes from, its memory, and its history in the session
    #[command(after_help = format!("\
Examples:
  psm pid 4041081             the process now: name, exe, command line, parent chain, app, cgroup, memory, CPU, I/O
  psm pid 4041081 baseline    the same process as it was in a stored snapshot
  psm pid 4041081 --json      every field, plus its row from each snapshot of the session

{REFERENCES}"))]
    Pid {
        /// Process id
        pid: i64,
        /// baseline, latest, prev, now, a number, or a label
        #[arg(default_value = "now")]
        snapshot: String,
        #[command(flatten)]
        watch: WatchArg,
    },
    /// Live view: system memory information plus the top consumers per metric; nothing is stored
    #[command(after_help = "\
Examples:
  psm info                  now: top 5 by CPU, memory and threads
  psm info 10 --by cpu,mem  top 10, two tables
  psm info --top 10         the same; --top wins over a number
  psm info --group name     rank programs instead of single processes")]
    #[command(short_flag = 'i')]
    Info {
        /// Rows per table
        #[arg(default_value_t = 5)]
        n: usize,
        /// Rows per table, the same as the number; wins when both are given
        #[arg(long, value_name = "N")]
        top: Option<usize>,
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
        #[command(flatten)]
        watch: WatchArg,
    },
    /// New session + baseline snapshot (the active session becomes inactive)
    ///
    /// The first time, it also creates the
    /// config file at its default location.
    #[command(short_flag = 'n')]
    New {
        /// Session name [default: session-YYYYMMDD-HHMMSS]
        name: Option<String>,
        /// Free text shown next to the name
        description: Option<String>,
        #[command(flatten)]
        capture: CaptureArgs,
    },
    /// Another snapshot in the active session
    #[command(
        short_flag = 's',
        after_help = "\
Examples:
  psm snap
  psm snap after-update
  psm snap \"after update\" \"chrome 154, extensions off\"   label and description"
    )]
    Snap {
        /// Label for the snapshot (not baseline, latest, prev or now)
        label: Option<String>,
        /// Free text shown next to the label
        description: Option<String>,
        #[command(flatten)]
        capture: CaptureArgs,
    },
    /// Changes between two snapshots [default: baseline -> now, the live state]
    #[command(after_help = format!("\
Examples:
  psm diff                     baseline -> now
  psm diff prev                last snapshot -> now
  psm diff 1                   snapshot #1 -> now
  psm diff 0 2                 two stored snapshots (#0 is the baseline)
  psm diff --group name        the whole diff per program
  psm diff --memory --top 10   memory ranking only

{REFERENCES}"))]
    #[command(short_flag = 'd')]
    Diff(DiffArgs),
    /// Snapshots of the session, the live state last (same as `psm snapshots`)
    #[command(short_flag = 'l')]
    List,
    /// Active session summary (same as running without a command)
    Status,
    /// One report over two snapshots [default: baseline -> now], or over the whole session (timeline, trend)
    #[command(after_help = format!("\
Examples:
  psm report growth                 programs that grew since the baseline
  psm report meminfo prev           system memory fields changed since the last snapshot
  psm report cpu 0 2                CPU time between two snapshots
  psm report timeline --name chrome one program across all snapshots
  psm report trend                  who keeps growing across all snapshots and now

{REFERENCES}"))]
    Report {
        /// Which report
        kind: ReportKind,
        #[command(flatten)]
        diff: DiffArgs,
    },
    /// Follow a program while it runs: min, max and average of its memory, CPU and threads
    ///
    /// Samples the program every --every until it ends, then prints one
    /// table. The program is started here (`-- cmd args`, its exit code
    /// is passed through), an existing pid and its descendants (--pid),
    /// or whatever the words and filters match, re-matched on every
    /// sample and rolled up by --group so helpers count (`track chrome`).
    /// --warmup drops the cold start from the statistics; --times runs a
    /// command again and again and compares the runs; --save writes the
    /// JSON document after every sample so another tool can follow it.
    #[command(
        short_flag = 't',
        after_help = "\
Examples:
  psm track -- cargo build --release       start it, sample until it exits
  psm track --warmup 5s -- ./server        the first 5s do not count
  psm track --pid 1234                     an existing process and its children
  psm track chrome --for 10m               by words, like `psm procs chrome`, for ten minutes
  psm track --times 5 --skip-runs 1 -- ./bench   five runs, the cold one shown but not counted
  psm track --save build.json -- make      table on screen, every sample in the file

Rows (each summed over every process of the target at a sample):
  rss       memory in RAM right now, shared libraries counted once per process
  anon      heap and stack: what the program allocated itself; the row to watch for a leak
  swap      memory of the target moved out of RAM
  cpu %     CPU over the period between two samples, per core: 100 = one core busy
  threads   threads of all its processes
  procs     how many processes it had (a browser: the main process plus every renderer and helper)
Columns: MIN, MAX, AVG, LAST over the samples after --warmup; AT MAX is seconds
from the start when MAX was seen (empty when the row never changed). HOST is the
machine, not the target: lowest MemAvailable seen, whole-machine CPU, load."
    )]
    Track {
        /// Words to look for in the pid, name, executable path or command line;
        /// every word must match, case does not matter (--match-case: it does)
        #[arg(value_name = "WORD")]
        words: Vec<String>,
        /// Sample period [default: 1s, config track.every]
        #[arg(long, value_name = "DURATION")]
        every: Option<String>,
        /// Drop the first DURATION of every run from min/max/avg [default: 0, config track.warmup]
        #[arg(long, value_name = "DURATION")]
        warmup: Option<String>,
        /// Stop after DURATION (a started command gets SIGINT)
        #[arg(long = "for", value_name = "DURATION")]
        for_: Option<String>,
        /// Run the command N times; the runs are compared at the end
        #[arg(long, value_name = "N", default_value_t = 1)]
        times: u32,
        /// Leave the first N runs out of the comparison (still shown)
        #[arg(long, value_name = "N", default_value_t = 0)]
        skip_runs: u32,
        /// Keep running after a run exits with an error
        #[arg(long)]
        keep_going: bool,
        /// Wait between runs [default: 0, config track.pause]
        #[arg(long, value_name = "DURATION")]
        pause: Option<String>,
        /// Write the JSON document to FILE after every sample and at the end
        #[arg(long, value_name = "FILE")]
        save: Option<PathBuf>,
        /// Roll matched processes up to this key so their helpers count (words mode) [default: app]
        #[arg(long, value_name = "KEY", value_parser = group_keys())]
        group: Option<String>,
        /// No progress line on stderr (a started command has none anyway: it owns the terminal)
        #[arg(long)]
        quiet: bool,
        #[command(flatten)]
        filter: FilterArgs,
        /// The command to start, after `--`
        #[arg(last = true, value_name = "COMMAND")]
        command: Vec<OsString>,
    },
    /// Sessions: list (the default), activate, deactivate, compare, export, import, delete, purge, reset
    #[command(after_help = "\
Examples:
  psm sessions                          id, name, created, snapshot count and state of each
  psm sessions activate chrome-153      make it the active one
  psm sessions compare chrome-153 chrome-154
  psm sessions export > chrome-154.json
  psm sessions purge --older-than 180d")]
    Sessions {
        #[command(subcommand)]
        cmd: Option<SessionsCmd>,
    },
    /// Snapshots of the session: list (the default), delete one, or reset (start the session over)
    #[command(after_help = "\
Examples:
  psm snapshots                    number, label, time and process count of each snapshot
  psm snapshots delete 2           one snapshot; without a number, the newest
  psm snapshots reset              delete every snapshot, take a new baseline (asks first)")]
    Snapshots {
        #[command(subcommand)]
        cmd: Option<SnapshotsCmd>,
    },
    /// Dump one snapshot [default: latest], or with --all the whole active session, as JSON or CSV
    ///
    /// The JSON is the `psm sessions export` format, so `psm sessions
    /// import` loads it as a session of its own and `psm import` adds its
    /// snapshots to an existing session.
    #[command(after_help = format!("\
Examples:
  psm export > latest.json           the newest snapshot of the active session
  psm export after-update > s.json   by label or number
  psm sessions activate old; psm export 2     from another session
  psm export --all > session.json    every snapshot of the active session
  psm export --format csv > procs.csv

{REFERENCES}"))]
    Export {
        /// baseline, latest, prev, a number, or a label [default: latest]
        snapshot: Option<String>,
        /// Every snapshot of the active session (same as `psm sessions export`)
        #[arg(long, conflicts_with = "snapshot")]
        all: bool,
        /// Output format
        #[arg(long, value_enum, default_value = "json")]
        format: ExportFormat,
        /// Leave command lines out
        #[arg(long)]
        no_cmdline: bool,
    },
    /// Add the snapshots of an export file to the active session (JSON)
    ///
    /// Reads what `psm export` or `psm sessions export` wrote and appends
    /// every snapshot in it to the active session, keeping labels and
    /// timestamps. The file's own baseline arrives as a plain snapshot: the
    /// session keeps the baseline it has.
    #[command(after_help = "\
Examples:
  psm export > s.json                  on one machine
  psm import s.json                    into the active session on another
  psm sessions activate old; psm import s.json  into another session
  psm export | psm --db other.db import -")]
    Import {
        /// File written by `psm export`; `-` reads standard input
        file: PathBuf,
    },
    /// Show which configuration file is used; --init creates it (same as a bare `--config`)
    Config {
        /// Write the file with the default settings (never overwrites)
        #[arg(long)]
        init: bool,
    },
    /// Copy the database to PATH
    #[command(short_flag = 'b')]
    Backup {
        /// Destination file; must not exist
        path: PathBuf,
    },
    /// First-time setup: config file, database, shell completions; `force` starts from scratch
    ///
    /// Creates the config file and the database if they are missing, and
    /// writes the completion script for your shell (from $SHELL, or
    /// --shell). Safe to run again: existing config and database are left
    /// alone, the completion script is refreshed. `psm init force` deletes
    /// both first and recreates them with the defaults (asks first).
    #[command(after_help = "\
Examples:
  psm init                 create what is missing, refresh the completions
  psm init --shell zsh
  psm init force           delete the database and the config file, then set up fresh (asks first)
  psm init force --yes")]
    Init {
        /// Shell to install completions for [default: from $SHELL]
        #[arg(long)]
        shell: Option<clap_complete::Shell>,
        #[command(subcommand)]
        cmd: Option<InitCmd>,
    },
    /// After installing a new psm: upgrade the database in place, check the config, refresh completions
    ///
    /// The database is brought to the current schema with a copy kept next
    /// to it (`psm.db.v2.bak`); nothing is deleted. The config file is
    /// parsed and reported, the completion script rewritten. Safe to run
    /// at any time; `just install` runs it.
    Update {
        /// Shell to install completions for [default: from $SHELL]
        #[arg(long)]
        shell: Option<clap_complete::Shell>,
    },
    /// Common questions and the command that answers each
    ///
    /// With words, only rows whose question or command contains every
    /// word are shown, ignoring case: `psm faq what`, `psm faq memory grew`.
    #[command(short_flag = 'q')]
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
    /// This message, or the help of one command (brief with -h, full with --help)
    #[command(
        short_flag = 'h',
        long_flag = "help",
        after_help = "\
Examples:
  psm -h                 the commands and global options, one line each
  psm --help             the same with every option explained
  psm help snapshots diff   the help of one command (also `psm diff --help`)
  psm help session export"
    )]
    Help {
        /// Command, and subcommand, to explain
        command: Vec<String>,
    },
    /// Name, version, build date and commit
    #[command(short_flag = 'v', long_flag = "version")]
    Version,
}

/// `psm (process state monitor) 2.1.5 (built 2026-09-30, commit 563af30)`:
/// what `psm version` prints and the first line of `psm -h`.
pub const VERSION_LINE: &str = concat!(
    "psm (process state monitor) ",
    env!("CARGO_PKG_VERSION"),
    " (built ",
    env!("PSM_BUILD_DATE"),
    ", commit ",
    env!("PSM_GIT_HASH"),
    ")"
);

pub fn version() -> String {
    VERSION_LINE.to_string()
}

#[derive(Clone, Subcommand)]
pub enum InitCmd {
    /// Delete the database and the config file, then set everything up fresh (asks first)
    ///
    /// Every session and snapshot is gone and the config file is rewritten
    /// with the defaults; the completion script is refreshed as usual.
    /// `psm backup <path>` keeps a copy of the database first.
    Force {
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Clone, Subcommand)]
pub enum ProcsCmd {
    /// Processes now, with CPU and memory (the default)
    #[command(
        short_flag = 'l',
        after_help = "\
Examples:
  psm procs list --sort cpu --top 10     the ten busiest
  psm procs -l --group name              one row per program"
    )]
    List {
        #[command(flatten)]
        view: ViewArgs,
        /// CPU sampling window; 0 = lifetime average [default: 500ms]
        #[arg(long, value_name = "DURATION")]
        interval: Option<String>,
        #[command(flatten)]
        watch: WatchArg,
    },
    /// Processes of one stored snapshot; the options of list apply
    #[command(after_help = REFERENCES)]
    Show {
        /// baseline, latest, prev, now, a number, or a label
        #[arg(default_value = "latest")]
        snapshot: String,
        #[command(flatten)]
        view: ViewArgs,
        #[command(flatten)]
        watch: WatchArg,
    },
}

// Compare and Export carry filters and options, List nothing; clap builds it once.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Subcommand)]
pub enum SessionsCmd {
    /// All sessions, with their state: active or inactive (the default)
    #[command(short_flag = 'l')]
    List,
    /// Make another session the active one
    ///
    /// There is one active session: the one `snap` adds to and `diff`
    /// compares against the live state. The others are inactive but kept;
    /// `psm sessions activate <name|id>` makes any of them active again, at any time.
    #[command(after_help = "\
Examples:
  psm sessions               see the names and ids
  psm sessions activate chrome-153    by name
  psm sessions activate 2             by id")]
    Activate {
        /// Session name or id
        session: String,
    },
    /// Make the active session inactive, leaving none active
    ///
    /// Nothing is deleted. `psm sessions activate <name|id>` makes a session active again.
    Deactivate,
    /// Give a session a new name, and optionally a new description
    #[command(after_help = "\
Examples:
  psm sessions rename chrome-153 chrome-old
  psm sessions rename 2 chrome-old \"before the update\"   name and description
  psm sessions rename 2 chrome-old \"\"                    clear the description")]
    Rename {
        /// Session name or id
        session: String,
        /// The new name (the old one is fine); must be free
        name: String,
        /// The new description; omitted keeps the old one, "" clears it
        description: Option<String>,
    },
    /// Delete inactive sessions older than the given age
    ///
    /// The active session is never touched. `psm snapshots reset` starts
    /// the active session over instead.
    Purge {
        /// Age, e.g. 180d
        #[arg(long, value_name = "AGE")]
        older_than: String,
    },
    /// Delete ALL sessions and snapshots: the whole database (asks first)
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
    /// Two sessions, by program (latest snapshot of each)
    #[command(after_help = "\
Examples:
  psm sessions compare chrome-153 chrome-154
  psm sessions compare old new --name chrome   one application only")]
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
    /// Dump a session as JSON or CSV
    ///
    /// JSON is complete and can be loaded again with `psm sessions import`.
    /// CSV is one row per process, for spreadsheets; it cannot be imported.
    #[command(after_help = "\
Examples:
  psm sessions export > chrome-154.json       the active session
  psm sessions export chrome-153 > old.json   another session, by name or id
  psm sessions export --all > everything.json  every session, for psm sessions import
  psm sessions export --format csv > procs.csv")]
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
    /// Load sessions written by `psm sessions export` (JSON)
    ///
    /// Reads a single-session export or an --all export. Sessions keep
    /// their snapshots and timestamps and arrive inactive, so the active
    /// session is not disturbed. Make one active with `psm sessions activate <name>`,
    /// or put it next to another with `psm sessions compare`.
    #[command(after_help = "\
Examples:
  psm sessions export > chrome-153.json                   on one machine
  psm sessions import chrome-153.json                     on another
  psm sessions import chrome-153.json --name chrome-old   under a different name
  psm sessions import everything.json                     all sessions of an --all export
  psm sessions export | psm --db other.db session import -")]
    Import {
        /// File written by `psm sessions export`; `-` reads standard input
        file: PathBuf,
        /// Name for the imported session [default: the exported name]; single-session exports only
        #[arg(long)]
        name: Option<String>,
    },
    /// Delete one session and its snapshots
    #[command(after_help = "\
Examples:
  psm sessions                   see the names and ids
  psm sessions delete chrome-153
  psm sessions delete 2")]
    Delete {
        /// Session name or id
        session: String,
    },
}

// Diff carries every filter and view option, Reset one bool; clap builds it once.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Subcommand)]
pub enum SnapshotsCmd {
    /// Snapshots of the session: number, label, time, process count (the default)
    #[command(short_flag = 'l')]
    List,
    /// Give a snapshot a new label, and optionally a new description
    #[command(after_help = format!("\
Examples:
  psm snapshots rename 2 after-update
  psm snapshots rename 2 after-update \"extensions off\"   label and description
  psm snapshots rename 2 after-update \"\"                  clear the description

{REFERENCES}"))]
    Rename {
        /// latest, prev, a number, or a label (not now)
        snapshot: String,
        /// The new label (not baseline, latest, prev or now)
        label: String,
        /// The new description; omitted keeps the old one, "" clears it
        description: Option<String>,
    },
    /// Delete one snapshot [default: latest]; the others keep their numbers
    ///
    /// The baseline (#0) only goes when it is the last snapshot of the
    /// session: then it is deleted and the live state becomes the new #0,
    /// so the session never ends up empty. With other snapshots present,
    /// `psm snapshots reset` removes them all at once.
    #[command(after_help = format!("\
Examples:
  psm snapshots           see the numbers and labels
  psm snapshots delete              the newest snapshot
  psm snapshots delete 2
  psm snapshots delete after-update

{REFERENCES}"))]
    Delete {
        /// latest, prev, a number, or a label (not now) [default: latest]
        snapshot: Option<String>,
    },
    /// Delete every snapshot of the active session and take a new baseline (asks first)
    ///
    /// The session keeps its name and id, the live state becomes its new #0.
    /// `psm export --all > file.json` keeps a copy first. `psm sessions
    /// reset` is the other reset: it deletes the whole database.
    #[command(short_flag = 'r')]
    Reset {
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
        #[command(flatten)]
        capture: CaptureArgs,
    },
}
