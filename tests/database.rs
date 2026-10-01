//! The database file: permissions, versioning, purge, backup, reset.

mod common;

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Output, Stdio};

use common::*;

#[test]
fn database_is_private_versioned_and_kept() {
    let e = Env::new("db");
    e.ok("before", &["new", "old"]);
    e.ok("after", &["new", "current"]);
    assert_eq!(
        fs::metadata(&e.db).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let copy = e.db.with_extension("backup");
    let _ = fs::remove_file(&copy);
    e.ok("after", &["backup", copy.to_str().unwrap()]);
    assert_eq!(
        fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        e.fails("after", &["backup", copy.to_str().unwrap()])
            .contains("already exists")
    );
    fs::remove_file(&copy).unwrap();

    // purge removes old inactive sessions and never the active one.
    let conn = rusqlite::Connection::open(&e.db).unwrap();
    conn.execute(
        "UPDATE sessions SET created_at = '2020-01-01T00:00:00Z'",
        [],
    )
    .unwrap();
    assert!(
        e.ok("after", &["purge", "--older-than", "180d"])
            .contains("1 inactive session(s)")
    );
    let left = e.json("after", &["sessions", "--json"]);
    assert_eq!(column(left.as_array().unwrap(), "name"), ["current"]);
    let orphans: i64 = conn
        .query_row(
            "SELECT count(*) FROM processes WHERE snapshot_id NOT IN (SELECT id FROM snapshots)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0);

    // No migrations: a database from another schema version is refused, not converted.
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(
        e.fails("after", &["sessions"])
            .contains("schema 99, expected 1")
    );

    // `reset` is the way out of a database this version cannot read.
    assert!(
        e.ok("after", &["reset", "--yes"])
            .contains("a database that cannot be read")
    );
    assert!(!e.db.exists());
    assert_eq!(
        e.json("after", &["sessions", "--json"]),
        serde_json::json!([])
    );
}

#[test]
fn reset_deletes_everything_only_after_a_yes() {
    let e = Env::new("reset");
    assert!(e.ok("before", &["reset"]).contains("Nothing to reset"));
    e.ok("before", &["new", "one"]);
    e.ok("before", &["new", "two"]);

    let answer = |input: &str| -> Output {
        let mut child = e
            .command("before")
            .args(["--config", "/dev/null", "reset"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let sessions = || {
        e.json("before", &["sessions", "--json"])
            .as_array()
            .unwrap()
            .len()
    };

    // No answer at all (not interactive), "n", or anything else: nothing happens.
    for input in ["", "n\n", "maybe\n"] {
        let o = answer(input);
        assert!(String::from_utf8_lossy(&o.stdout).contains("Nothing was deleted"));
        assert!(String::from_utf8_lossy(&o.stderr).contains("2 session(s) and 2 snapshot(s)"));
        assert_eq!(sessions(), 2);
    }

    let o = answer("y\n");
    assert!(String::from_utf8_lossy(&o.stdout).contains("Database reset"));
    assert!(!e.db.exists(), "the database file is gone");
    assert!(e.fails("before", &[]).contains("no active session"));
}
