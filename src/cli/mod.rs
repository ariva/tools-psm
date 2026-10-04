//! Command-line definition (clap derive). No logic beyond parsing.

pub mod args;
pub mod commands;
pub mod completions;
pub mod values;

use std::path::PathBuf;

use clap::{Arg, ArgAction, CommandFactory, FromArgMatches, Parser, ValueHint};

pub use args::{CaptureArgs, DiffArgs, FilterArgs, ViewArgs};
pub use commands::{Cmd, ExportFormat, InitCmd, ProcsCmd, ReportKind, SessionsCmd, SnapshotsCmd};

/// clap's default help with three header rows: the version line (the same
/// as `psm version`), the description, then author and repository.
const HELP_TEMPLATE: &str = concat!(
    "{before-help}",
    "psm (process snapshot manager) ",
    env!("CARGO_PKG_VERSION"),
    " (built ",
    env!("PSM_BUILD_DATE"),
    ", commit ",
    env!("PSM_GIT_HASH"),
    ")\n{about}\n",
    "{author}  ·  ",
    env!("CARGO_PKG_REPOSITORY"),
    "\n\n{usage-heading} {usage}\n\n{all-args}{after-help}"
);

/// Named snapshots of the process table and before/after comparison of
/// memory, CPU and process changes.
#[derive(Parser)]
#[command(
    name = "psm",
    author,
    help_template = HELP_TEMPLATE,
    disable_help_flag = true,
    disable_help_subcommand = true,
    after_help = "\
Run without a command to see the active session.
Commands with a letter also work as a flag: `psm -s` is `psm snap`.
`psm <command> -h` lists its own commands. Double dashes are options.
Global options work with every command and can go before or after it;
only this help lists them. `psm --help` explains each of them with an example.
`psm <command> --help` explains the options and values of one command."
)]
pub struct Cli {
    /// Configuration file [default: ~/.config/psm/config.toml]
    ///
    /// Read this file instead of the default one. A path given here must
    /// exist; only the default file may be missing. The flag wins over
    /// PSM_CONFIG. Alone, without a path and without a command, it is the
    /// same as `psm config`: it shows which file is in use.
    ///
    /// Example: psm --config ./ci.toml diff
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "PSM_CONFIG",
        value_name = "PATH",
        num_args = 0..=1
    )]
    pub config: Option<Option<PathBuf>>,

    /// Database file [default: ~/.local/share/psm/psm.db]
    ///
    /// Use this database instead of the default one; it is created if it
    /// does not exist. Handy for a throwaway experiment. The flag wins over
    /// PSM_DB, which wins over `database` in the config file. Alone, without
    /// a path and without a command, it shows which database is in use.
    ///
    /// Example: psm --db /tmp/trial.db new trial
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "PSM_DB",
        value_name = "PATH",
        num_args = 0..=1
    )]
    pub db: Option<Option<PathBuf>>,

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
    /// Example: psm procs --kernel
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

/// The command tree used for parsing, help and completions.
///
/// The top level has a `help` subcommand with `-h`/`--help` as its flag
/// forms, so clap's automatic `-h` flag is disabled there (the two cannot
/// share the letter). clap propagates that choice to every subcommand, so
/// each gets its own `-h`/`--help` here.
///
/// The global options are accepted after every command but listed only
/// by `psm -h`: a command's help is about that command. Each subcommand
/// gets hidden copies of them up front; clap's own propagation then skips
/// the ids that already exist. (Hiding after `build()` is not an option:
/// `mut_arg` on a built command breaks its key map.)
pub fn command() -> clap::Command {
    fn add(cmd: &mut clap::Command, hidden: &[Arg]) {
        for sc in cmd.get_subcommands_mut() {
            let help = Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("Print help (-h brief, --help full)");
            // Skip ids the command defines itself (`sessions export <SESSION>`), as clap does.
            let fresh: Vec<Arg> = hidden
                .iter()
                .filter(|h| sc.get_arguments().all(|a| a.get_id() != h.get_id()))
                .cloned()
                .collect();
            *sc = std::mem::take(sc).args(fresh).arg(help);
            add(sc, hidden);
        }
    }
    /// Shell completion offers files for any valued argument without a
    /// hint. Only a few here are paths; the rest (session names, snapshot
    /// references, labels, sizes, durations, user names: bash has no
    /// finer hint) complete to nothing.
    fn hints(cmd: &mut clap::Command) {
        let ids: Vec<clap::Id> = cmd
            .get_arguments()
            .filter(|a| {
                a.get_action().takes_values()
                    && a.get_value_hint() == ValueHint::Unknown
                    && a.get_possible_values().is_empty()
            })
            .map(|a| a.get_id().clone())
            .collect();
        for id in ids {
            let hint = match id.as_str() {
                "config" | "db" | "path" | "file" | "exe" => ValueHint::FilePath,
                "proc_root" => ValueHint::DirPath,
                _ => ValueHint::Other,
            };
            *cmd = std::mem::take(cmd).mut_arg(id, |a| a.value_hint(hint));
        }
        for sc in cmd.get_subcommands_mut() {
            hints(sc);
        }
    }
    let mut cmd = Cli::command();
    let hidden: Vec<Arg> = cmd
        .get_arguments()
        .filter(|a| a.is_global_set())
        .map(|a| a.clone().hide(true))
        .collect();
    add(&mut cmd, &hidden);
    hints(&mut cmd);
    cmd
}

pub fn parse() -> Cli {
    Cli::from_arg_matches(&command().get_matches()).unwrap_or_else(|e| e.exit())
}

impl Cli {
    /// The configuration file to read: `--config PATH`, else `PSM_CONFIG`;
    /// `None` means the default location. A bare `--config` counts as absent
    /// here (clap then skips the environment fallback, so it is read by hand).
    pub fn config_path(&self) -> Option<PathBuf> {
        self.config
            .clone()
            .flatten()
            .or_else(|| std::env::var_os("PSM_CONFIG").map(PathBuf::from))
    }

    /// `psm --config` with no path and no command: show the configuration file.
    pub fn bare_config(&self) -> bool {
        self.cmd.is_none() && self.config == Some(None)
    }

    /// The database from the command line: `--db PATH`, else `PSM_DB`. The
    /// config file and the default come later, in `Ctx::db_path`.
    pub fn db_flag(&self) -> Option<PathBuf> {
        self.db
            .clone()
            .flatten()
            .or_else(|| std::env::var_os("PSM_DB").map(PathBuf::from))
    }

    /// `psm --db` with no path and no command: show the database in use.
    pub fn bare_db(&self) -> bool {
        self.cmd.is_none() && self.db == Some(None)
    }

    /// The `--shell` given to `psm init`, if any.
    pub fn shell_for_init(&self) -> Option<clap_complete::Shell> {
        match &self.cmd {
            Some(Cmd::Init { shell, .. }) | Some(Cmd::Update { shell }) => *shell,
            _ => None,
        }
    }

    /// `psm init force [--yes]`: `Some(yes)`; plain `psm init`: `None`.
    pub fn init_force(&self) -> Option<bool> {
        match &self.cmd {
            Some(Cmd::Init {
                cmd: Some(InitCmd::Force { yes }),
                ..
            }) => Some(*yes),
            _ => None,
        }
    }
}
