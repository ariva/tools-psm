use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use super::table::csv_field;
use crate::model::Snapshot;
use crate::store::Session;

fn without_cmdline(process: &mut Value, keep: bool) {
    if !keep && let Some(obj) = process.as_object_mut() {
        obj.remove("cmdline");
    }
}

pub fn to_json(session: &Session, snapshots: &[Snapshot], cmdline: bool) -> Result<Value> {
    let mut snaps = serde_json::to_value(snapshots)?;
    for snap in snaps.as_array_mut().into_iter().flatten() {
        for p in snap["processes"].as_array_mut().into_iter().flatten() {
            without_cmdline(p, cmdline);
        }
    }
    Ok(json!({ "session": session, "snapshots": snaps }))
}

/// `psm session export --all`: every session, each as `to_json` writes it.
pub fn all_to_json(sessions: &[(Session, Vec<Snapshot>)], cmdline: bool) -> Result<Value> {
    let dumps = sessions
        .iter()
        .map(|(s, snaps)| to_json(s, snaps, cmdline))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({ "sessions": dumps }))
}

/// One row per process per snapshot. Columns are the `Proc` fields, so a new
/// field shows up here without touching this function.
pub fn to_csv(snapshots: &[Snapshot], cmdline: bool) -> Result<String> {
    let mut lines: Vec<String> = Vec::new();
    for s in snapshots {
        for p in &s.processes {
            let mut row = serde_json::to_value(p)?;
            without_cmdline(&mut row, cmdline);
            let Value::Object(fields) = row else { continue };
            if lines.is_empty() {
                let names: Vec<&str> = fields.keys().map(String::as_str).collect();
                lines.push(format!("snapshot_id,label,created_at,{}", names.join(",")));
            }
            let values = fields.values().map(|v| match v {
                Value::Null => String::new(),
                Value::String(text) => csv_field(text),
                other => other.to_string(),
            });
            let head = [
                s.id.to_string(),
                csv_field(s.label.as_deref().unwrap_or("")),
                s.created_at.clone(),
            ];
            lines.push(head.into_iter().chain(values).collect::<Vec<_>>().join(","));
        }
    }
    Ok(lines.join("\n"))
}

#[derive(Deserialize)]
struct Dump {
    session: DumpSession,
    snapshots: Vec<Snapshot>,
}

#[derive(Deserialize)]
struct DumpSession {
    name: String,
}

#[derive(Deserialize)]
struct DumpAll {
    sessions: Vec<Dump>,
}

/// Reads what `to_json` or `all_to_json` wrote: one or more sessions, each
/// as its name and snapshots.
pub fn from_json(text: &str) -> Result<Vec<(String, Vec<Snapshot>)>> {
    let dumps = match serde_json::from_str::<Dump>(text) {
        Ok(one) => vec![one],
        Err(_) => {
            serde_json::from_str::<DumpAll>(text)
                .context("not a psm export: expected the JSON written by `psm session export`")?
                .sessions
        }
    };
    if dumps.is_empty() || dumps.iter().any(|d| d.snapshots.is_empty()) {
        bail!("the export contains a session without snapshots");
    }
    Ok(dumps
        .into_iter()
        .map(|d| (d.session.name, d.snapshots))
        .collect())
}
