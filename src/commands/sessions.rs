//! Sessions: `status`, `sessions`, `snapshots`, `switch`, and the
//! `psm session` group (export, import, deactivate, delete).

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::Ctx;
use crate::analysis::reports;
use crate::cli::{ExportFormat, FilterArgs};
use crate::model::Snapshot;
use crate::output::{Cell, Table, export, out, print_json};
use crate::store::Session;

/// Bare `psm` / `psm status`.
pub fn status(ctx: &Ctx) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(ctx.session.as_deref())?;
    let (text, doc) = reports::status(&db, &session, &ctx.filter(&FilterArgs::default())?)?;
    if ctx.json {
        print_json(&doc)
    } else {
        out(text)
    }
    Ok(())
}

pub fn snapshots(ctx: &Ctx) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(ctx.session.as_deref())?;
    let mut t = Table::new(&[
        ("ID", "id"),
        ("LABEL", "label"),
        ("CREATED", "created"),
        ("PROCS", "processes"),
        ("DEEP", "deep"),
    ]);
    for s in db.snapshots(session.id, ctx.kernel)? {
        t.rows.push(vec![
            Cell::Int(Some(s.id)),
            Cell::text(s.label.unwrap_or_default()),
            Cell::text(s.created),
            Cell::Int(Some(s.processes)),
            Cell::text(if s.deep { "yes" } else { "" }),
        ]);
    }
    ctx.emit(&t, false);
    Ok(())
}

pub fn sessions(ctx: &Ctx) -> Result<()> {
    let mut t = Table::new(&[
        ("ID", "id"),
        ("NAME", "name"),
        ("CREATED", "created"),
        ("SNAPS", "snapshots"),
        ("STATE", "state"),
    ]);
    for s in ctx.db()?.sessions()? {
        t.rows.push(vec![
            Cell::Int(Some(s.id)),
            Cell::text(s.name),
            Cell::text(s.created),
            Cell::Int(Some(s.snapshots)),
            Cell::text(if s.inactive_since.is_some() {
                "inactive"
            } else {
                "active"
            }),
        ]);
    }
    ctx.emit(&t, false);
    Ok(())
}

pub fn switch(ctx: &Ctx, session: &str) -> Result<()> {
    let db = ctx.db()?;
    let target = db.session(Some(session))?;
    let previous = db.active_session()?.filter(|s| s.id != target.id);
    if target.inactive_since.is_none() {
        out(format!("Session {:?} is already active.", target.name));
    } else {
        db.switch(target.id)?;
        out(format!("Session {:?} is now active.", target.name));
        if let Some(p) = previous {
            out(format!("Session {:?} is now inactive.", p.name));
        }
    }
    Ok(())
}

pub fn export(
    ctx: &Ctx,
    session: Option<String>,
    all: bool,
    format: ExportFormat,
    no_cmdline: bool,
) -> Result<()> {
    let db = ctx.db()?;
    let load = |s: &Session| -> Result<Vec<Snapshot>> {
        db.snapshots(s.id, true)?
            .iter()
            .map(|m| db.load(m.id))
            .collect()
    };
    if all {
        if matches!(format, ExportFormat::Csv) {
            bail!("--all is JSON only: CSV has no session column");
        }
        let mut sessions = Vec::new();
        for s in db.sessions()? {
            let snapshots = load(&s)?;
            sessions.push((s, snapshots));
        }
        print_json(&export::all_to_json(&sessions, !no_cmdline)?);
    } else {
        // The positional name wins over the global --session.
        let session = db.session(session.as_deref().or(ctx.session.as_deref()))?;
        let snapshots = load(&session)?;
        match format {
            ExportFormat::Json => print_json(&export::to_json(&session, &snapshots, !no_cmdline)?),
            ExportFormat::Csv => out(export::to_csv(&snapshots, !no_cmdline)?),
        }
    }
    Ok(())
}

pub fn import(ctx: &Ctx, file: &Path, name: Option<String>) -> Result<()> {
    let text = if file == Path::new("-") {
        std::io::read_to_string(std::io::stdin()).context("cannot read standard input")?
    } else {
        std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?
    };
    let mut sessions = export::from_json(&text)?;
    if let Some(name) = name {
        if sessions.len() > 1 {
            bail!(
                "--name applies to a single-session export; this file holds {} sessions",
                sessions.len()
            );
        }
        sessions[0].0 = name;
    }
    for session in ctx.db()?.import(&sessions)? {
        out(format!(
            "Imported session {:?} with {} snapshot(s). It is inactive: read it with \
             `psm --session {} snapshots`, or make it active with `psm switch {}`.",
            session.name, session.snapshots, session.id, session.id
        ));
    }
    Ok(())
}

pub fn deactivate(ctx: &Ctx) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(ctx.session.as_deref())?;
    db.deactivate(session.id)?;
    out(format!(
        "Session {:?} is now inactive. Its data is kept; `psm switch {}` makes it active again.",
        session.name, session.id
    ));
    Ok(())
}

pub fn delete(ctx: &Ctx, session: &str) -> Result<()> {
    let db = ctx.db()?;
    let s = db.session(Some(session))?;
    db.delete_session(s.id)?;
    out(format!(
        "Deleted session {:?} and its {} snapshot(s).",
        s.name, s.snapshots
    ));
    Ok(())
}
