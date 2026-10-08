//! Configuration file rules and what `psm new` / `psm init` set up.

mod common;

use serde_json::Value;
use std::fs;
use std::process::{Command, Output};

use common::*;

#[test]
fn config_file_rules() {
    let e = Env::new("config");
    let dir = std::env::temp_dir().join(format!("psm-test-{}-config", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = |name: &str, text: &str| {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        path
    };
    let run = |configure: &dyn Fn(&mut Command), args: &[&str]| {
        let mut c = e.command("before");
        configure(&mut c);
        c.args(["procs", "--interval", "0", "--sort", "pid", "--json"])
            .args(args)
            .output()
            .unwrap()
    };
    let first = |o: &Output| -> Value {
        serde_json::from_slice::<Value>(&o.stdout).unwrap()["data"][0]["command"].clone()
    };

    let kernel = file("kernel.toml", "[display]\nkernel = true\n");
    let plain = file("plain.toml", "");
    let typo = file("typo.toml", "[collection]\ndeap = true\n");

    // The file sets a default; a flag overrides it, also to switch it off.
    let o = run(
        &|c| {
            c.arg("--config").arg(&kernel);
        },
        &[],
    );
    assert_eq!(first(&o), "kthreadd");
    let o = run(
        &|c| {
            c.arg("--config").arg(&kernel);
        },
        &["--kernel=false"],
    );
    assert_eq!(first(&o), "code");

    // PSM_CONFIG selects the file; --config wins over it.
    let o = run(
        &|c| {
            c.env("PSM_CONFIG", &kernel);
        },
        &[],
    );
    assert_eq!(first(&o), "kthreadd");
    let o = run(
        &|c| {
            c.env("PSM_CONFIG", &kernel).arg("--config").arg(&plain);
        },
        &[],
    );
    assert_eq!(first(&o), "code");

    // An explicit path must exist and parse; a typo is never ignored.
    let o = run(
        &|c| {
            c.arg("--config").arg(dir.join("missing.toml"));
        },
        &[],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot read config file"));
    let o = run(
        &|c| {
            c.env("PSM_CONFIG", &typo);
        },
        &[],
    );
    assert_eq!(o.status.code(), Some(2));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("typo.toml") && err.contains("deap") && err.contains("line 2"),
        "{err}"
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn first_init_leaves_a_config_file() {
    let e = Env::new("autoconfig");
    let home = std::env::temp_dir().join(format!("psm-test-{}-xdg", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let file = home.join("psm/config.toml");
    // No --config and no PSM_CONFIG: the default location, redirected to a temp directory.
    let psm = |args: &[&str]| {
        e.command("before")
            .env("XDG_CONFIG_HOME", &home)
            .args(args)
            .output()
            .unwrap()
    };
    let stdout = |o: &Output| String::from_utf8_lossy(&o.stdout).into_owned();

    assert!(stdout(&psm(&["config"])).contains("not found; built-in defaults apply"));
    // A bare --config (no path, no command) is the same as `psm config`.
    assert_eq!(stdout(&psm(&["--config"])), stdout(&psm(&["config"])));
    // A bare --db shows the database in use, before and after it exists.
    let o = psm(&["--db"]);
    assert!(
        o.status.success()
            && stdout(&o).contains("Database: ")
            && stdout(&o).contains("not created yet"),
        "{}",
        stdout(&o)
    );
    // With a command, a bare --config is simply the default file.
    let o = psm(&["procs", "--interval", "0", "--config"]);
    assert!(
        o.status.success() && stdout(&o).contains("PID"),
        "{}",
        stdout(&o)
    );
    psm(&["procs", "--interval", "0"]);
    assert!(!file.exists(), "only `new` creates the file");

    let o = psm(&["new", "one"]);
    assert!(
        o.status.success() && stdout(&o).contains("Created config file"),
        "{}",
        stdout(&o)
    );
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("[collection]") && text.contains("min_memory_delta"));
    assert!(stdout(&psm(&["config"])).contains("(in use)"));
    assert!(stdout(&psm(&["--db"])).contains("schema 3; exists, 1 session(s)"));

    // It is the user's file from then on: never rewritten.
    fs::write(&file, "[display]\nkernel = true\n").unwrap();
    let o = psm(&["new", "two"]);
    assert!(o.status.success() && !stdout(&o).contains("Created config file"));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[display]\nkernel = true\n"
    );
    let o = psm(&["config", "--init"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("already exists; not overwritten"));

    // An explicit path is created by `config --init`, but never by `init`.
    let other = home.join("other.toml");
    let o = e
        .command("before")
        .arg("--config")
        .arg(&other)
        .args(["config", "--init"])
        .output()
        .unwrap();
    assert!(o.status.success() && other.exists());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn init_sets_everything_up_and_is_repeatable() {
    let e = Env::new("init");
    let home = std::env::temp_dir().join(format!("psm-test-{}-init", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let psm = |args: &[&str], shell: &str| -> Output {
        e.command("before")
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_DATA_HOME", home.join("data"))
            .env("SHELL", shell)
            .args(args)
            .output()
            .unwrap()
    };
    let text = |o: &Output| String::from_utf8_lossy(&o.stdout).into_owned();

    let o = psm(&["init"], "/bin/bash");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let t = text(&o);
    assert!(
        t.contains("config.toml (created)") && t.contains("(created)\n"),
        "{t}"
    );
    assert!(t.contains("bash -> ") && t.contains("(written)"), "{t}");
    assert!(home.join("config/psm/config.toml").exists());
    assert!(e.db.exists(), "the database is created without a session");
    let script = home.join("data/bash-completion/completions/psm");
    assert!(fs::read_to_string(&script).unwrap().contains("_psm()"));

    // Second run: nothing is recreated, the script is refreshed.
    fs::write(&script, "stale").unwrap();
    let t = text(&psm(&["init"], "/bin/bash"));
    assert!(
        t.contains("config.toml (exists)") && t.contains("(exists, 0 session(s))"),
        "{t}"
    );
    assert!(fs::read_to_string(&script).unwrap().contains("_psm()"));

    // Other shells, and an unknown one.
    let t = text(&psm(&["init"], "/usr/bin/fish"));
    assert!(
        t.contains("fish -> ") && home.join("config/fish/completions/psm.fish").exists(),
        "{t}"
    );
    let t = text(&psm(&["init", "--shell", "zsh"], "/bin/bash"));
    assert!(
        t.contains("fpath") && home.join("data/zsh/site-functions/_psm").exists(),
        "{t}"
    );
    let t = text(&psm(&["init"], "/bin/weird"));
    assert!(t.contains("not recognised"), "{t}");

    // `init force`: without a yes nothing changes; with it, config and database are recreated.
    let config_file = home.join("config/psm/config.toml");
    fs::write(&config_file, "[display]\nkernel = true\n").unwrap();
    let o = psm(&["init", "force"], "/bin/bash");
    assert!(text(&o).contains("Nothing was changed"), "{}", text(&o));
    assert_eq!(
        fs::read_to_string(&config_file).unwrap(),
        "[display]\nkernel = true\n"
    );
    let t = text(&psm(&["init", "force", "--yes"], "/bin/bash"));
    assert!(
        t.contains("config.toml (created)") && t.contains("(created)\n"),
        "{t}"
    );
    assert!(
        fs::read_to_string(&config_file)
            .unwrap()
            .contains("[collection]")
    );

    fs::remove_dir_all(&home).unwrap();
}
