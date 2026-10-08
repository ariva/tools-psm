//! One throwaway database per test, and running the real binary against the
//! fake /proc trees in tests/fixtures/proc (see tests/fixtures/generate.py).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

pub const MIB: i64 = 1 << 20;

/// One throwaway database per test; the developer's own config never applies.
pub struct Env {
    pub db: PathBuf,
}

impl Env {
    pub fn new(name: &str) -> Env {
        let db = std::env::temp_dir().join(format!("psm-test-{}-{name}.db", std::process::id()));
        let _ = fs::remove_file(&db);
        Env { db }
    }

    pub fn command(&self, fixture: &str) -> Command {
        let root = format!(
            "{}/tests/fixtures/proc/{fixture}",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut c = Command::new(env!("CARGO_BIN_EXE_psm"));
        c.env("PSM_DB", &self.db)
            .env_remove("PSM_CONFIG")
            .args(["--proc-root", &root]);
        c
    }

    pub fn run(&self, fixture: &str, args: &[&str]) -> Output {
        self.command(fixture)
            .args(["--config", "/dev/null"])
            .args(args)
            .output()
            .unwrap()
    }

    pub fn ok(&self, fixture: &str, args: &[&str]) -> String {
        let o = self.run(fixture, args);
        assert!(
            o.status.success(),
            "psm {args:?} failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }

    /// The `data` of a JSON document; the envelope around it is checked here
    /// once for every call, and in detail by `json_envelope` in reports.rs.
    pub fn json(&self, fixture: &str, args: &[&str]) -> Value {
        let mut v: Value = serde_json::from_str(&self.ok(fixture, args)).unwrap();
        assert!(
            v["psm"]["version"].is_string()
                && v["command"].is_string()
                && v["elapsed_ms"].is_number(),
            "psm {args:?}: not an envelope: {v}"
        );
        v["data"].take()
    }

    /// Exit code 2 and the error text.
    pub fn fails(&self, fixture: &str, args: &[&str]) -> String {
        let o = self.run(fixture, args);
        assert_eq!(
            o.status.code(),
            Some(2),
            "psm {args:?} should fail with exit code 2"
        );
        String::from_utf8(o.stderr).unwrap()
    }

    pub fn before_and_after(&self, extra: &[&str]) {
        self.ok("before", &[&["new", "t"][..], extra].concat());
        self.ok("after", &[&["snap", "after"][..], extra].concat());
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.db);
    }
}
