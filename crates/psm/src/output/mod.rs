//! How results leave the program: tables, units, JSON, CSV, exports.

pub mod export;
pub mod table;
pub mod units;

use std::io::Write;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

pub use table::{Cell, Table};
pub use units::{human, human_delta, human_duration, parse_duration, parse_size, set_units};

/// Prints a line; a closed pipe (`psm procs | head`) ends the program quietly.
pub fn out(s: impl AsRef<str>) {
    if writeln!(std::io::stdout(), "{}", s.as_ref()).is_err() {
        std::process::exit(0);
    }
}

/// What the `options` block of a JSON document records.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JsonOptions {
    /// Every option and value as typed.
    Full,
    /// Without descriptions and the `--db`, `--config`, `--proc-root` paths.
    Safe,
    /// No `options` block.
    None,
}

impl JsonOptions {
    pub const NAMES: [&str; 3] = ["full", "safe", "none"];

    pub fn parse(name: &str) -> Result<JsonOptions> {
        Ok(match name {
            "full" => JsonOptions::Full,
            "safe" => JsonOptions::Safe,
            "none" => JsonOptions::None,
            _ => bail!(
                "display.json_options must be one of {} (got {name:?})",
                Self::NAMES.join(", ")
            ),
        })
    }
}

/// Values a shared server has no business seeing: free prose and local paths.
const UNSAFE_OPTIONS: [&str; 4] = ["description", "db", "config", "proc_root"];

/// The header of every JSON document: set once by the parser, the level
/// once the configuration is known, the clock again for every `--watch` round.
struct Envelope {
    command: String,
    options: Map<String, Value>,
    level: JsonOptions,
    started: SystemTime,
    begun: Instant,
}

static ENVELOPE: Mutex<Option<Envelope>> = Mutex::new(None);

pub fn json_begin(command: String, options: Map<String, Value>) {
    *ENVELOPE.lock().unwrap() = Some(Envelope {
        command,
        options,
        level: JsonOptions::Full,
        started: SystemTime::now(),
        begun: Instant::now(),
    });
}

pub fn json_level(level: JsonOptions) {
    if let Some(e) = ENVELOPE.lock().unwrap().as_mut() {
        e.level = level;
    }
}

/// A new `started` and `elapsed_ms` for the next `--watch` round.
pub fn json_round() {
    if let Some(e) = ENVELOPE.lock().unwrap().as_mut() {
        e.started = SystemTime::now();
        e.begun = Instant::now();
    }
}

/// Every JSON document: who wrote it, the command and its options, when,
/// how long, then the command's own output under `data`.
pub fn print_json(data: &Value) {
    out(serde_json::to_string_pretty(&json_document(data)).unwrap_or_default());
}

/// The document `print_json` prints, for a command that also writes it to
/// a file. Without a parsed command line (unit tests) it is the bare value.
pub fn json_document(data: &Value) -> Value {
    let guard = ENVELOPE.lock().unwrap();
    let Some(e) = guard.as_ref() else {
        return data.clone();
    };
    let mut doc = Map::new();
    doc.insert("psm".into(), build_info());
    doc.insert("command".into(), Value::String(e.command.clone()));
    match e.level {
        JsonOptions::Full => {
            doc.insert("options".into(), Value::Object(e.options.clone()));
        }
        JsonOptions::Safe => {
            let kept: Map<String, Value> = e
                .options
                .iter()
                .filter(|(k, _)| !UNSAFE_OPTIONS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            doc.insert("options".into(), Value::Object(kept));
        }
        JsonOptions::None => {}
    }
    doc.insert("started".into(), Value::String(utc(e.started)));
    doc.insert(
        "elapsed_ms".into(),
        json!(e.begun.elapsed().as_millis() as u64),
    );
    doc.insert("data".into(), data.clone());
    Value::Object(doc)
}

/// The writing psm: version, build date, commit, schema. Also the `data`
/// of `psm version --json`.
pub fn build_info() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "built": env!("PSM_BUILD_DATE"),
        "commit": env!("PSM_GIT_HASH"),
        "schema": crate::store::SCHEMA_VERSION,
    })
}

/// The current time as the database writes it: `2026-10-07T21:55:03Z`.
pub fn now_utc() -> String {
    utc(SystemTime::now())
}

/// `2026-10-07T21:55:03Z`, the format the database uses. No time crate:
/// days since the epoch to a civil date (Howard Hinnant's algorithm).
fn utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_is_iso_8601() {
        let at = |s: u64| utc(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z", "leap day");
        assert_eq!(at(1_791_157_323), "2026-10-04T23:42:03Z"); // date -u -d @1791157323
        assert_eq!(at(4_102_444_799), "2099-12-31T23:59:59Z");
    }
}
