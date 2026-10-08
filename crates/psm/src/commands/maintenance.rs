//! Deleting and copying data: `snapshots reset`, `sessions purge`, `backup`, `sessions reset`.

use std::path::Path;

use anyhow::{Context, Result};

use super::{Ctx, census};
use crate::cli::CaptureArgs;
use crate::output::{out, parse_duration};
use crate::store::Db;

/// Asks on stderr, reads one line: only `y` or `yes` is a yes. No answer
/// (a script, a closed stdin) is a no.
pub fn confirmed(question: &str) -> bool {
    use std::io::Write;
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).ok();
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

/// `psm snapshots reset`: the active session starts over with a fresh baseline.
pub fn purge_session(ctx: &Ctx, yes: bool, capture: &CaptureArgs) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(None)?;
    if !yes
        && !confirmed(&format!(
            "This permanently deletes the {} snapshot(s) of session {:?} and takes a new baseline.\n\
             `psm export --all > file.json` keeps a copy first.\n\
             Purge the session?",
            session.snapshots, session.name
        ))
    {
        out("Nothing was deleted.");
        return Ok(());
    }
    let snapshot = ctx.capture(capture)?;
    let removed = db.restart(session.id, &snapshot)?;
    out(format!(
        "Session {:?} purged: {removed} snapshot(s) deleted. New baseline #0: {}.",
        session.name,
        census(&snapshot)
    ));
    Ok(())
}

/// `psm sessions purge`: inactive sessions older than `older_than` go.
pub fn purge(ctx: &Ctx, older_than: &str) -> Result<()> {
    let removed = ctx.db()?.purge(parse_duration(older_than)?)?;
    for s in &removed {
        out(format!(
            "Deleted session {:?} ({} snapshots, created {}).",
            s.name, s.snapshots, s.created
        ));
    }
    out(format!(
        "{} inactive session(s) older than {older_than} deleted.",
        removed.len()
    ));
    Ok(())
}

pub fn backup(ctx: &Ctx, path: &Path) -> Result<()> {
    ctx.db()?.backup(path)?;
    out(format!("Database copied to {}.", path.display()));
    Ok(())
}

/// Deletes the database file and the SQLite side files a crash may have left.
pub fn remove_database(path: &Path) -> Result<()> {
    std::fs::remove_file(path).with_context(|| format!("cannot delete {}", path.display()))?;
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut side = path.to_path_buf().into_os_string();
        side.push(suffix);
        let _ = std::fs::remove_file(side);
    }
    Ok(())
}

/// `psm sessions reset`: deletes the whole database after an explicit yes.
pub fn reset(ctx: &Ctx, yes: bool) -> Result<()> {
    let path = ctx.db_path()?;
    if !path.exists() {
        out(format!(
            "Nothing to reset: {} does not exist.",
            path.display()
        ));
        return Ok(());
    }
    // A database this version cannot read can still be reset; that is the way out.
    let contents = match Db::open(&path).and_then(|db| db.sessions()) {
        Ok(sessions) => format!(
            "{} session(s) and {} snapshot(s)",
            sessions.len(),
            sessions.iter().map(|s| s.snapshots).sum::<i64>()
        ),
        Err(e) => format!("a database that cannot be read ({e})"),
    };
    if !yes
        && !confirmed(&format!(
            "This permanently deletes {contents} in {}.\n\
             `psm backup <path>` makes a copy first.\n\
             Delete everything?",
            path.display()
        ))
    {
        out("Nothing was deleted.");
        return Ok(());
    }
    remove_database(&path)?;
    out(format!(
        "Database reset: deleted {contents}. Run `psm new` to start a new session."
    ));
    Ok(())
}
