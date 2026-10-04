//! Sessions: `status`, `sessions`, `snapshots`, `switch`, the
//! `psm sessions` group (export, import, delete, ...), `activate`/`deactivate`, and the
//! single-snapshot commands `export`, `import`, `snapshots delete`.

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::Ctx;
use crate::analysis::reports;
use crate::cli::{CaptureArgs, ExportFormat, FilterArgs};
use crate::model::Snapshot;
use crate::output::{Cell, Table, export, out, print_json};
use crate::store::Session;

/// Bare `psm` / `psm status`.
pub fn status(ctx: &Ctx) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(None)?;
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
    let session = db.session(None)?;
    let mut t = Table::new(&[
        ("ID", "id"),
        ("LABEL", "label"),
        ("CREATED", "created"),
        ("PROCS", "processes"),
        ("DEEP", "deep"),
        ("DESCRIPTION", "description"),
    ]);
    for s in db.snapshots(session.id, ctx.kernel)? {
        t.rows.push(vec![
            Cell::Int(Some(s.seq)),
            Cell::text(s.label.unwrap_or_default()),
            Cell::text(s.created),
            Cell::Int(Some(s.processes)),
            Cell::text(if s.deep { "yes" } else { "" }),
            Cell::text(s.description.unwrap_or_default()),
        ]);
    }
    // The live state closes the list: `>` in the ID column (null in JSON), label `now`.
    let live = super::capture::live_snapshot(ctx, None)?;
    let procs = live
        .processes
        .iter()
        .filter(|p| ctx.kernel || !p.kthread)
        .count();
    t.rows.push(vec![
        if ctx.json {
            Cell::Int(None)
        } else {
            Cell::text(">")
        },
        Cell::text("now"),
        Cell::text(db.now_local()?),
        Cell::Int(Some(procs as i64)),
        Cell::text(""),
        Cell::text(""),
    ]);
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
        ("DESCRIPTION", "description"),
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
            Cell::text(s.description.unwrap_or_default()),
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
        let session = db.session(session.as_deref())?;
        let snapshots = load(&session)?;
        match format {
            ExportFormat::Json => print_json(&export::to_json(&session, &snapshots, !no_cmdline)?),
            ExportFormat::Csv => out(export::to_csv(&snapshots, !no_cmdline)?),
        }
    }
    Ok(())
}

/// The export file to import; `-` is standard input.
fn read_export(file: &Path) -> Result<Vec<crate::store::sessions::Export>> {
    let text = if file == Path::new("-") {
        std::io::read_to_string(std::io::stdin()).context("cannot read standard input")?
    } else {
        std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?
    };
    export::from_json(&text)
}

pub fn import(ctx: &Ctx, file: &Path, name: Option<String>) -> Result<()> {
    let mut sessions = read_export(file)?;
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
             `psm sessions activate {}` makes it active.",
            session.name, session.snapshots, session.id
        ));
    }
    Ok(())
}

pub fn deactivate(ctx: &Ctx) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(None)?;
    db.deactivate(session.id)?;
    out(format!(
        "Session {:?} is now inactive. Its data is kept; `psm sessions activate {}` makes it active again.",
        session.name, session.id
    ));
    Ok(())
}

/// `psm sessions rename <name|id> <new>`.
pub fn rename(ctx: &Ctx, session: &str, name: &str, description: Option<&str>) -> Result<()> {
    let db = ctx.db()?;
    let s = db.session(Some(session))?;
    db.rename_session(s.id, name, description)?;
    let after = db.session(Some(name))?;
    out(format!(
        "Session {:?} is now {name:?}{}.",
        s.name,
        after
            .description
            .filter(|d| !d.is_empty())
            .map(|d| format!(" ({d})"))
            .unwrap_or_default()
    ));
    Ok(())
}

/// `psm snapshots rename <ref> <label> [description]`.
pub fn rename_snapshot(
    ctx: &Ctx,
    snapshot: &str,
    label: &str,
    description: Option<&str>,
) -> Result<()> {
    if snapshot == "now" {
        bail!("`now` is the live state; there is nothing stored to rename");
    }
    super::capture::check_label(label)?;
    let db = ctx.db()?;
    let session = db.session(None)?;
    let id = db.resolve(&session, snapshot)?;
    let before = db.load(id)?.title();
    db.rename_snapshot(id, label, description)?;
    out(format!(
        "Snapshot {before} is now {}.",
        db.load(id)?.title()
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

/// `psm export [ref]`: one stored snapshot, in the session export format.
pub fn export_snapshot(
    ctx: &Ctx,
    snapshot: Option<String>,
    format: ExportFormat,
    no_cmdline: bool,
) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(None)?;
    let reference = snapshot.as_deref().unwrap_or("latest");
    if reference == "now" {
        bail!("`now` is the live state and is never stored; `psm snap` first, then export it");
    }
    let s = db.load(db.resolve(&session, reference)?)?;
    match format {
        ExportFormat::Json => print_json(&export::to_json(
            &session,
            std::slice::from_ref(&s),
            !no_cmdline,
        )?),
        ExportFormat::Csv => out(export::to_csv(std::slice::from_ref(&s), !no_cmdline)?),
    }
    Ok(())
}

/// `psm import <file>`: every snapshot in the file joins the session.
pub fn import_snapshots(ctx: &Ctx, file: &Path) -> Result<()> {
    let snapshots: Vec<Snapshot> = read_export(file)?
        .into_iter()
        .flat_map(|(_, _, snaps)| snaps)
        .collect();
    let db = ctx.db()?;
    let session = db.session(None)?;
    let numbers = db.add_snapshots(session.id, &snapshots)?;
    let list: Vec<String> = numbers.iter().map(|n| format!("#{n}")).collect();
    out(format!(
        "Imported {} snapshot(s) into session {:?} as {}.",
        numbers.len(),
        session.name,
        list.join(", ")
    ));
    Ok(())
}

/// `psm snapshots delete [ref]`, default `latest`. The last snapshot of a session is
/// replaced by a fresh baseline rather than leaving the session empty.
pub fn delete_snapshot(ctx: &Ctx, snapshot: Option<&str>) -> Result<()> {
    let snapshot = snapshot.unwrap_or("latest");
    if snapshot == "now" {
        bail!("`now` is the live state; there is nothing stored to delete");
    }
    let db = ctx.db()?;
    let session = db.session(None)?;
    let id = db.resolve(&session, snapshot)?;
    if session.snapshots == 1 {
        let fresh = ctx.capture(&CaptureArgs {
            deep: None,
            no_cmdline: false,
        })?;
        db.restart(session.id, &fresh)?;
        out(format!(
            "Deleted snapshot #0 baseline, the only one in session {:?}. New baseline #0: {}.",
            session.name,
            super::census(&fresh)
        ));
        return Ok(());
    }
    let gone = db.delete_snapshot(&session, id)?;
    out(format!(
        "Deleted snapshot #{}{} from session {:?}. The other snapshots keep their numbers.",
        gone.seq,
        gone.label.map(|l| format!(" {l}")).unwrap_or_default(),
        session.name
    ));
    Ok(())
}
