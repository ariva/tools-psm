//! Picking values out of `--json` output.

use serde_json::Value;

pub fn rows<'a>(v: &'a Value, key: &str) -> &'a Vec<Value> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("no array {key:?} in {v}"))
}

pub fn column<'a>(rows: &'a [Value], key: &str) -> Vec<&'a Value> {
    rows.iter().map(|r| &r[key]).collect()
}
