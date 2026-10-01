//! Argument groups shared by several commands.

use clap::Args;

use super::values::{group_keys, metrics, sort_columns};

#[derive(Args, Default)]
pub struct FilterArgs {
    /// Only processes of this user (name or uid)
    #[arg(long)]
    pub user: Option<String>,
    /// Only processes whose name contains this text
    #[arg(long)]
    pub name: Option<String>,
    /// Only processes running this executable path
    #[arg(long)]
    pub exe: Option<String>,
    /// Drop processes whose name or command line matches
    #[arg(long, value_name = "REGEX")]
    pub exclude_regex: Option<String>,
}

#[derive(Args)]
pub struct ViewArgs {
    /// One row per distinct group instead of one row per process
    #[arg(long, value_name = "KEY", value_parser = group_keys())]
    pub group: Option<String>,
    /// Column to sort by
    #[arg(long, value_name = "COLUMN", value_parser = sort_columns())]
    pub sort: Option<String>,
    /// Only the first N rows
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
    /// Add PSS/USS columns
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true", value_name = "BOOL")]
    pub deep: Option<bool>,
    /// CSV output
    #[arg(long)]
    pub csv: bool,
    #[command(flatten)]
    pub filter: FilterArgs,
}

#[derive(Args)]
pub struct CaptureArgs {
    /// Also collect PSS/USS (reads smaps_rollup; slower)
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true", value_name = "BOOL")]
    pub deep: Option<bool>,
    /// Do not store command lines (they can contain secrets)
    #[arg(long)]
    pub no_cmdline: bool,
}

#[derive(Args, Default)]
pub struct DiffArgs {
    /// Snapshot: baseline, latest, prev, now, an id, or a label [default: baseline]
    pub a: Option<String>,
    /// Snapshot to compare with [default: now, the live state; nothing is stored]
    pub b: Option<String>,
    /// Memory impact ranking only
    #[arg(long)]
    pub memory: bool,
    /// New processes only
    #[arg(long)]
    pub new: bool,
    /// Gone processes only
    #[arg(long)]
    pub gone: bool,
    /// Restarted processes only
    #[arg(long)]
    pub restarted: bool,
    /// One row per distinct group instead of one row per process
    #[arg(long, value_name = "KEY", value_parser = group_keys())]
    pub group: Option<String>,
    /// What counts as memory in the comparison
    #[arg(long, value_parser = metrics())]
    pub metric: Option<String>,
    /// Only the first N rows of each table
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
    /// Hide memory changes smaller than this, e.g. 10M
    #[arg(long, value_name = "SIZE")]
    pub min_delta: Option<String>,
    #[command(flatten)]
    pub filter: FilterArgs,
}
