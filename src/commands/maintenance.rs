//! Deleting and copying data: `purge`, `backup`, `reset`.

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::Ctx;
use crate::output::{out, parse_duration};
use crate::store::Db;

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

/// `psm reset`: deletes the whole database after an explicit yes.
pub fn reset(ctx: &Ctx, yes: bool) -> Result<()> {
    if ctx.session.is_some() {
        bail!(
            "`psm reset` deletes the whole database, not one session; \
             use `psm session delete <name|id>` for a single session"
        );
    }
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
    if !yes {
        use std::io::Write;
        eprint!(
            "This permanently deletes {contents} in {}.\n\
             `psm backup <path>` makes a copy first.\n\
             Delete everything? [y/N] ",
            path.display()
        );
        std::io::stderr().flush().ok();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).ok();
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            out("Nothing was deleted.");
            return Ok(());
        }
    }
    std::fs::remove_file(&path).with_context(|| format!("cannot delete {}", path.display()))?;
    // SQLite side files, if a crash left any behind.
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut side = path.clone().into_os_string();
        side.push(suffix);
        let _ = std::fs::remove_file(side);
    }
    out(format!(
        "Database reset: deleted {contents}. Run `psm new` to start a new session."
    ));
    Ok(())
}
