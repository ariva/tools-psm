//! How results leave the program: tables, units, JSON, CSV, exports.

pub mod export;
pub mod table;
pub mod units;

use std::io::Write;

use serde_json::Value;

pub use table::{Cell, Table};
pub use units::{human, human_delta, human_duration, parse_duration, parse_size, set_units};

/// Prints a line; a closed pipe (`psm procs | head`) ends the program quietly.
pub fn out(s: impl AsRef<str>) {
    if writeln!(std::io::stdout(), "{}", s.as_ref()).is_err() {
        std::process::exit(0);
    }
}

pub fn print_json(value: &Value) {
    out(serde_json::to_string_pretty(value).unwrap_or_default());
}
