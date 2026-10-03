//! First-run setup: `init`, `config`, and where shell completions go. These
//! run before the configuration is loaded.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::Cli;
use crate::config;
use crate::output::out;
use crate::store::Db;

/// Where a shell picks up a user's completion script, and what to tell the
/// user about it. `None`: no known per-user location.
fn completion_target(shell: clap_complete::Shell) -> Option<(PathBuf, &'static str)> {
    use clap_complete::Shell;
    let data = |sub: &str| config::xdg("XDG_DATA_HOME", ".local/share").map(|d| d.join(sub));
    let conf = |sub: &str| config::xdg("XDG_CONFIG_HOME", ".config").map(|d| d.join(sub));
    match shell {
        // A running bash keeps the function it loaded first; only a new shell or
        // a fresh `source` sees an updated script.
        Shell::Bash => Some((
            data("bash-completion/completions/psm")?,
            "New shells pick this up. In a shell that already completed psm, run: source <that file>",
        )),
        Shell::Fish => Some((
            conf("fish/completions/psm.fish")?,
            "Open a new shell to use them.",
        )),
        Shell::Zsh => Some((
            data("zsh/site-functions/_psm")?,
            "Make sure that directory is in $fpath before compinit, then open a new shell.",
        )),
        _ => None,
    }
}

/// `psm init`: everything a first run needs, reported line by line.
pub fn init(cli: &Cli) -> Result<()> {
    let config_path = match cli.config_path().or_else(config::default_path) {
        Some(p) => p,
        None => bail!("cannot locate the config file: HOME is not set; pass --config"),
    };
    if let Some(yes) = cli.init_force() {
        // The database named by the current config goes too, so read it before deleting.
        let cfg = if config_path.exists() {
            config::load(Some(&config_path))?
        } else {
            config::Config::default()
        };
        let db_path = match (cli.db_flag(), &cfg.database) {
            (Some(p), _) => p,
            (None, Some(p)) => config::expand_tilde(p),
            (None, None) => config::default_db_path()?,
        };
        let db_state = if db_path.exists() {
            db_contents(&db_path).unwrap_or_else(|e| format!("cannot be read: {e}"))
        } else {
            "does not exist".to_string()
        };
        if !yes
            && !super::maintenance::confirmed(&format!(
                "This permanently deletes the database {} ({db_state}) and rewrites the config file {} with the defaults.\n\
                 `psm backup <path>` makes a copy of the database first.\n\
                 Start from scratch?",
                db_path.display(),
                config_path.display()
            ))
        {
            out("Nothing was changed.");
            return Ok(());
        }
        if db_path.exists() {
            super::maintenance::remove_database(&db_path)?;
        }
        if config_path.exists() {
            std::fs::remove_file(&config_path)
                .with_context(|| format!("cannot delete {}", config_path.display()))?;
        }
    }
    let config_state = if config_path.exists() {
        "exists"
    } else {
        config::create(&config_path)?;
        "created"
    };
    out(format!(
        "Config:       {} ({config_state})",
        config_path.display()
    ));

    let cfg = config::load(Some(&config_path))?;
    let db_path = match (cli.db_flag(), &cfg.database) {
        (Some(p), _) => p,
        (None, Some(p)) => config::expand_tilde(p),
        (None, None) => config::default_db_path()?,
    };
    let db_state = if db_path.exists() {
        db_contents(&db_path)?
    } else {
        Db::open(&db_path)?;
        "created".to_string()
    };
    out(format!("Database:     {} ({db_state})", db_path.display()));

    let Some(shell) = cli.shell_for_init().or_else(clap_complete::Shell::from_env) else {
        out(
            "Completions:  shell not recognised from $SHELL; run `psm completions <shell>` by hand.",
        );
        return Ok(());
    };
    match completion_target(shell) {
        Some((path, note)) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("cannot create {}", dir.display()))?;
            }
            std::fs::write(&path, crate::cli::completions::script(shell))
                .with_context(|| format!("cannot write {}", path.display()))?;
            out(format!(
                "Completions:  {shell} -> {} (written)\n              {note}",
                path.display()
            ));
        }
        None => out(format!(
            "Completions:  no standard location for {shell}; use `psm completions {shell}` and install the script by hand."
        )),
    }
    Ok(())
}

/// `exists, 3 session(s)`
fn db_contents(path: &std::path::Path) -> Result<String> {
    let sessions = Db::open(path)?.sessions()?.len();
    Ok(format!("exists, {sessions} session(s)"))
}

/// `psm --db` alone: which database file is in use, after the flag, `PSM_DB`,
/// the config file and the default have been merged.
pub fn database(ctx: &super::Ctx) -> Result<()> {
    let path = ctx.db_path()?;
    let state = if path.exists() {
        db_contents(&path)?
    } else {
        "not created yet; the first `psm new` creates it".to_string()
    };
    out(format!("Database: {} ({state})", path.display()));
    Ok(())
}

/// `psm config`: where the configuration comes from, and `--init` to create it.
pub fn config(explicit: Option<&Path>, init: bool) -> Result<()> {
    let path = match explicit
        .map(Path::to_path_buf)
        .or_else(config::default_path)
    {
        Some(path) => path,
        None => bail!("cannot locate the config file: HOME is not set; pass --config"),
    };
    if init {
        config::create(&path)?;
        out(format!(
            "Created config file {} with the default settings.",
            path.display()
        ));
    } else if path.exists() {
        config::load(Some(&path))?;
        out(format!("Config file: {} (in use)", path.display()));
    } else {
        out(format!(
            "Config file: {} (not found; built-in defaults apply)\nCreate it with `psm config --init`.",
            path.display()
        ));
    }
    Ok(())
}
