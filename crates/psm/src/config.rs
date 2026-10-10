use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

/// Unknown keys are errors, so a typo like `deap = true` is never silently ignored.
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub database: Option<String>,
    pub collection: Collection,
    pub display: Display,
    pub diff: Diff,
    pub track: Track,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Collection {
    pub io: bool,
    pub cgroups: bool,
    pub cmdline: bool,
    pub deep: bool,
}

impl Default for Collection {
    fn default() -> Self {
        Collection {
            io: true,
            cgroups: true,
            cmdline: true,
            deep: false,
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Display {
    pub units: String,
    pub top: Option<usize>,
    pub group: Option<String>,
    pub kernel: bool,
    pub interval: String,
    /// What `--json` records about the run: full, safe, none.
    pub json_options: String,
}

impl Default for Display {
    fn default() -> Self {
        Display {
            units: "auto".into(),
            top: None,
            group: None,
            kernel: false,
            interval: "500ms".into(),
            json_options: "full".into(),
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Diff {
    pub metric: String,
    pub min_memory_delta: String,
}

impl Default for Diff {
    fn default() -> Self {
        Diff {
            metric: "total".into(),
            min_memory_delta: "1M".into(),
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Track {
    pub every: String,
    pub warmup: String,
    pub pause: String,
}

impl Default for Track {
    fn default() -> Self {
        Track {
            every: "1s".into(),
            warmup: "0s".into(),
            pause: "0s".into(),
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `$VAR`, or `~/<fallback>` when the variable is unset.
pub fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(fallback)))
}

pub fn default_db_path() -> Result<PathBuf> {
    xdg("XDG_DATA_HOME", ".local/share")
        .map(|d| d.join("psm/psm.db"))
        .context("cannot locate the database: HOME is not set; pass --db")
}

pub fn expand_tilde(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// What `psm new` and `psm config --init` write: every key at its built-in
/// default, so the file documents the settings without changing any.
pub const TEMPLATE: &str = r#"# psm configuration. Command-line flags override these values.
# Every value below is the built-in default. Lines starting with '#' are
# optional settings: remove the '#' to use one.

# database = "~/.local/share/psm/psm.db"

[collection]
io = true          # read /proc/<pid>/io
cgroups = true     # read cgroup membership and memory.current
cmdline = true     # store command lines (they can contain secrets)
deep = false       # also collect PSS/USS on every snapshot (slower)

[display]
units = "auto"     # auto, B, KiB, MiB, GiB
kernel = false     # show kernel threads
interval = "500ms" # CPU sampling window of `list` and `info`
json_options = "full"  # what --json records about the run: full, safe (no descriptions, no paths), none
# top = 30         # limit every table to this many rows
# group = "name"   # name, app, exe, cmdline, user, cgroup, parent

[diff]
metric = "total"         # total, anon, pss
min_memory_delta = "1M"  # hide smaller memory changes

[track]
every = "1s"       # sample period of `psm track`
warmup = "0s"      # dropped from min/max/avg at the start of every run
pause = "0s"       # wait between runs (--times)
"#;

pub fn default_path() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", ".config").map(|dir| dir.join("psm/config.toml"))
}

/// Writes the template. An existing file is never overwritten.
pub fn create(path: &Path) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                anyhow!(
                    "config file {} already exists; not overwritten",
                    path.display()
                )
            }
            _ => anyhow!("cannot create config file {}: {e}", path.display()),
        })?;
    file.write_all(TEMPLATE.as_bytes())?;
    Ok(())
}

/// Exactly one file is read. An explicit path (`--config` or `PSM_CONFIG`)
/// must exist; the default location is optional.
pub fn load(explicit: Option<&Path>) -> Result<Config> {
    let (path, required) = match (explicit, default_path()) {
        (Some(p), _) => (p.to_path_buf(), true),
        (None, Some(p)) => (p, false),
        (None, None) => return Ok(Config::default()),
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if !required && e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(e) => {
            return Err(e).with_context(|| format!("cannot read config file {}", path.display()));
        }
    };
    toml::from_str(&text).map_err(|e| anyhow!("config file {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_is_an_error() {
        let err = toml::from_str::<Config>("[collection]\ndeap = true\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("deap") && err.contains("line 2"), "{err}");
    }

    #[test]
    fn template_is_exactly_the_defaults() {
        assert_eq!(
            toml::from_str::<Config>(TEMPLATE).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn partial_file_keeps_defaults() {
        let c: Config = toml::from_str("[collection]\ndeep = true\n").unwrap();
        assert!(c.collection.deep && c.collection.cmdline);
        assert_eq!(c.display.interval, "500ms");
        assert_eq!(c.diff.metric, "total");
        assert_eq!(c.track.every, "1s");
    }
}
