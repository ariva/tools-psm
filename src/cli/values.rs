//! The fixed value lists, each value with a description: `-h` lists them on
//! one line, `--help` explains them, and an invalid value is rejected with
//! the list.

use clap::builder::{PossibleValue, PossibleValuesParser};

pub fn group_keys() -> PossibleValuesParser {
    PossibleValuesParser::new([
        PossibleValue::new("name").help("Program name: all chrome processes become one row"),
        PossibleValue::new("exe").help("Full path of the executable"),
        PossibleValue::new("cmdline")
            .help("Full command line: tells apart scripts run by the same interpreter"),
        PossibleValue::new("app").help(
            "Application: the top-level ancestor, skipping shells, terminals and session managers",
        ),
        PossibleValue::new("user").help("Owner of the process"),
        PossibleValue::new("cgroup")
            .help("systemd application or service, with the cgroup's own memory total"),
        PossibleValue::new("parent").help("Parent process"),
        PossibleValue::new("pid").help("No grouping (overrides a default from the config file)"),
    ])
}

pub fn sort_columns() -> PossibleValuesParser {
    PossibleValuesParser::new([
        PossibleValue::new("mem").help("Memory, largest first (default)"),
        PossibleValue::new("cpu").help("CPU usage, highest first"),
        PossibleValue::new("threads").help("Thread count, highest first"),
        PossibleValue::new("swap").help("Swapped-out memory; adds a SWAP column"),
        PossibleValue::new("io").help("Bytes read + written; adds an IO column"),
        PossibleValue::new("pid").help("Process id, ascending"),
        PossibleValue::new("name").help("Name, alphabetically"),
        PossibleValue::new("count").help("Processes per group, highest first"),
    ])
}

pub fn metrics() -> PossibleValuesParser {
    PossibleValuesParser::new([
        PossibleValue::new("total").help("RSS + swap (default)"),
        PossibleValue::new("anon").help("Anonymous RSS + swap: what the process allocated itself"),
        PossibleValue::new("pss").help("PSS + swap; both snapshots must be taken with --deep"),
    ])
}

pub fn info_tables() -> PossibleValuesParser {
    PossibleValuesParser::new([
        PossibleValue::new("cpu").help("Top CPU users"),
        PossibleValue::new("mem").help("Top memory users"),
        PossibleValue::new("threads").help("Most threads"),
        PossibleValue::new("swap").help("Most swapped-out memory"),
        PossibleValue::new("io").help("Most bytes read + written"),
    ])
}
