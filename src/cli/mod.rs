//! Command-line definition (clap derive). No logic beyond parsing.

pub mod args;
pub mod commands;
pub mod values;

use std::path::PathBuf;

use clap::Parser;

pub use args::{CaptureArgs, DiffArgs, FilterArgs, ViewArgs};
pub use commands::{Cmd, ExportFormat, ReportKind, SessionCmd};

/// psm (process snapshot manager): named snapshots of the process table
/// and before/after comparison of memory, CPU and process changes.
///
/// Run without a command to see the active session.
#[derive(Parser)]
#[command(
    name = "psm",
    version,
    after_help = "\
Global options work with every command and can go before or after it.
`psm --help` explains each of them with an example.
`psm <command> --help` explains the options and values of one command."
)]
pub struct Cli {
    /// Configuration file [default: ~/.config/psm/config.toml]
    ///
    /// Read this file instead of the default one. A path given here must
    /// exist; only the default file may be missing. The flag wins over
    /// PSM_CONFIG. `psm config` shows which file is in use.
    ///
    /// Example: psm --config ./ci.toml diff
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "PSM_CONFIG",
        value_name = "PATH"
    )]
    pub config: Option<PathBuf>,

    /// Database file [default: ~/.local/share/psm/psm.db]
    ///
    /// Use this database instead of the default one; it is created if it
    /// does not exist. Handy for a throwaway experiment. The flag wins over
    /// PSM_DB, which wins over `database` in the config file.
    ///
    /// Example: psm --db /tmp/trial.db new trial
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "PSM_DB",
        value_name = "PATH"
    )]
    pub db: Option<PathBuf>,

    /// Address a session other than the active one (name or id)
    ///
    /// Read commands (status, snapshots, show, diff, report, export,
    /// session export) then work on that session, for example an inactive one.
    /// `psm sessions` lists names and ids. new and snap always use the
    /// active session and reject this option.
    ///
    /// Example: psm --session chrome-153 diff
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        value_name = "NAME|ID"
    )]
    pub session: Option<String>,

    /// Read process data from DIR instead of /proc
    ///
    /// DIR must be laid out like /proc. This exists for the tests, which
    /// point it at the fake trees in tests/fixtures/proc.
    ///
    /// Example: psm --proc-root tests/fixtures/proc/before list
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        value_name = "DIR",
        default_value = "/proc"
    )]
    pub proc_root: PathBuf,

    /// Include kernel threads (hidden by default)
    ///
    /// Kernel threads use no user memory and are about half of the process
    /// table, so lists, diffs and counts leave them out. They are stored in
    /// snapshots either way. `--kernel=false` overrides `kernel = true` in
    /// the config file.
    ///
    /// Example: psm list --kernel
    #[arg(long, global = true, help_heading = "Global options", num_args = 0..=1, require_equals = true, default_missing_value = "true", value_name = "BOOL")]
    pub kernel: Option<bool>,

    /// Machine-readable output
    ///
    /// Print JSON instead of tables. Sizes are raw bytes, missing values
    /// are null. Works with every command that prints a table.
    ///
    /// Example: psm diff --json | jq .top.groups
    #[arg(long, global = true, help_heading = "Global options")]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

impl Cli {
    /// The `--shell` given to `psm init`, if any.
    pub fn shell_for_init(&self) -> Option<clap_complete::Shell> {
        match &self.cmd {
            Some(Cmd::Init { shell }) => *shell,
            _ => None,
        }
    }
}
