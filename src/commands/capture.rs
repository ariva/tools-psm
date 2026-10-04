//! Taking snapshots: `new` (a session with its baseline), `snap`, and the
//! never-stored live snapshot behind `now`.

use anyhow::{Result, bail};

use super::{Ctx, census};
use crate::cli::CaptureArgs;
use crate::config;
use crate::model::Snapshot;
use crate::output::out;

/// `psm new`: a session with its baseline.
pub fn new_session(
    ctx: &Ctx,
    name: Option<String>,
    description: Option<String>,
    capture: &CaptureArgs,
) -> Result<()> {
    let db = ctx.db()?;
    let previous = db.active_session()?;
    let snapshot = ctx.capture(capture)?;
    let description = description.filter(|d| !d.is_empty());
    let (session, id) = db.init(name.as_deref(), description.as_deref(), &snapshot)?;
    if let Some(p) = previous {
        out(format!("Session {:?} is now inactive.", p.name));
    }
    out(format!(
        "Session {:?} started. Baseline snapshot #{id}: {}.",
        session.name,
        census(&snapshot)
    ));
    // First use: leave a config file behind so the settings are discoverable.
    if ctx.default_config
        && let Some(path) = config::default_path().filter(|p| !p.exists())
    {
        match config::create(&path) {
            Ok(()) => out(format!(
                "Created config file {} with the default settings.",
                path.display()
            )),
            Err(e) => eprintln!("warning: {e:#}"),
        }
    }
    Ok(())
}

pub fn snap(
    ctx: &Ctx,
    label: Option<String>,
    description: Option<String>,
    capture: &CaptureArgs,
) -> Result<()> {
    if let Some(l) = label.as_deref() {
        check_label(l)?;
    }
    let db = ctx.db()?;
    let session = db.session(None)?;
    let previous = db.resolve(&session, "latest").ok();
    let mut snapshot = ctx.capture(capture)?;
    snapshot.description = description.filter(|d| !d.is_empty());
    snapshot.id = db.snap(session.id, label.as_deref(), &snapshot)?;
    snapshot.label = label;
    out(format!(
        "Snapshot {} in session {:?}: {}.",
        snapshot.title(),
        session.name,
        census(&snapshot)
    ));
    if let Some(prev) = previous {
        super::compare::snap_summary(ctx, &db.load(prev)?, &snapshot)?;
    }
    Ok(())
}

/// Labels with a fixed meaning cannot be given to a snapshot.
pub fn check_label(label: &str) -> Result<()> {
    if crate::store::snapshots::RESERVED_LABELS.contains(&label) {
        bail!("{label:?} is a reserved snapshot reference and cannot be used as a label");
    }
    Ok(())
}

/// The live state as a snapshot that is never stored. It is collected the same
/// way as the snapshot it is compared with, so the values line up.
pub fn live_snapshot(ctx: &Ctx, like: Option<&Snapshot>) -> Result<Snapshot> {
    let deep = like.map_or(ctx.cfg.collection.deep, |s| s.deep);
    let cmdline = like.map_or(ctx.cfg.collection.cmdline, |s| {
        s.processes.iter().any(|p| p.cmdline.is_some())
    });
    let (mut snapshot, _) = ctx.collect(deep, cmdline)?;
    snapshot.label = Some("now".into());
    Ok(snapshot)
}
