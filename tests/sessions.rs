//! Sessions: rules, switching, export and import.

mod common;

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::process::Stdio;

use common::*;

#[test]
fn session_rules() {
    let e = Env::new("sessions");
    assert!(e.fails("before", &[]).contains("run `psm new` first"));
    assert!(e.fails("before", &["snap"]).contains("no active session"));

    e.ok("before", &["new", "a"]);
    assert!(
        e.fails("before", &["diff", "prev", "latest"])
            .contains("no snapshot before #1")
    );
    assert_eq!(
        e.json("before", &["diff", "--json"])["net_change"],
        0,
        "baseline -> now, unchanged"
    );
    assert!(e.fails("before", &["snap", "latest"]).contains("reserved"));
    assert!(
        e.fails("before", &["--session", "a", "snap"])
            .contains("active session")
    );

    // init makes the previous session inactive; nothing is deleted.
    e.ok("after", &["session", "new", "b"]);
    assert!(e.fails("after", &["new", "a"]).contains("already exists"));
    let sessions = e.json("after", &["sessions", "--json"]);
    assert_eq!(
        column(sessions.as_array().unwrap(), "state"),
        ["inactive", "active"]
    );
    assert_eq!(
        e.json("after", &["--session", "a", "snapshots", "--json"])[0]["label"],
        "baseline"
    );

    let status = e.json("after", &["status", "--json"]);
    assert_eq!(status["session"]["name"], "b");
    assert_eq!(status["processes"]["current"], 5);

    // Sessions are compared by program, never by pid.
    let cmp = e.json("after", &["compare", "a", "b", "--json"]);
    assert_eq!(cmp["totals"][0]["delta"], 0);
    assert_eq!(cmp["programs"][0]["name"], "rust-analyzer");
    assert_eq!(cmp["programs"][0]["delta"], -468 * MIB);

    // An inactive session is compared within itself, not against now.
    let inactive = e.json("after", &["--session", "a", "diff", "--json"]);
    assert_eq!(inactive["to"]["label"], "baseline");

    // Switching: `a` becomes the active session again, `b` inactive. Nothing is lost.
    assert!(
        e.ok("after", &["switch", "a"])
            .contains("\"a\" is now active")
    );
    let sessions = e.json("after", &["sessions", "--json"]);
    assert_eq!(
        column(sessions.as_array().unwrap(), "state"),
        ["active", "inactive"]
    );
    assert_eq!(
        e.json("after", &["status", "--json"])["session"]["name"],
        "a"
    );
    e.ok("after", &["snap", "later"]);
    assert_eq!(
        e.json("after", &["snapshots", "--json"])
            .as_array()
            .unwrap()
            .len(),
        2,
        "snap went to a"
    );
    assert_eq!(
        e.json("after", &["--session", "b", "snapshots", "--json"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        e.json("after", &["diff", "--json"])["to"]["label"],
        "now",
        "active again: compared with now"
    );
    assert!(e.ok("after", &["switch", "a"]).contains("already active"));
    assert!(e.fails("after", &["switch", "nope"]).contains("no session"));
    // Deactivating leaves no active session; switch brings one back by id.
    e.ok("after", &["session", "deactivate"]);
    assert!(e.fails("after", &["snap"]).contains("no active session"));
    e.ok("after", &["switch", "2"]);
    assert_eq!(
        e.json("after", &["status", "--json"])["session"]["name"],
        "b"
    );

    e.ok("after", &["session", "delete", "a"]);
    assert_eq!(
        e.json("after", &["sessions", "--json"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        e.fails("after", &["--session", "a", "snapshots"])
            .contains("no session")
    );
}

#[test]
fn export_then_import_round_trips() {
    let e = Env::new("export");
    e.before_and_after(&[]);
    let dump = e.ok("after", &["session", "export"]);
    let file = std::env::temp_dir().join(format!("psm-test-{}-export.json", std::process::id()));
    fs::write(&file, &dump).unwrap();
    let path = file.to_str().unwrap();

    // Same database: the name is taken, so it needs another one.
    assert!(
        e.fails("after", &["session", "import", path])
            .contains("pass --name")
    );
    assert!(
        e.ok("after", &["session", "import", path, "--name", "copy"])
            .contains("2 snapshot(s)")
    );
    let sessions = e.json("after", &["sessions", "--json"]);
    assert_eq!(
        column(sessions.as_array().unwrap(), "state"),
        ["active", "inactive"]
    );
    assert_eq!(
        sessions[1]["created"], sessions[0]["created"],
        "original timestamps are kept"
    );

    // The copy is the same data: comparing it with the original finds nothing,
    // and its own diff is the original diff.
    let cmp = e.json("after", &["compare", "t", "copy", "--json"]);
    assert_eq!(cmp["programs"], serde_json::json!([]));
    let copy = e.json("after", &["--session", "copy", "diff", "--json"]);
    assert_eq!(copy["net_change"], -36 * MIB);
    assert_eq!(copy["to"]["label"], "after");
    // Re-exporting the copy gives the same snapshots; only the ids are new.
    let original: Value = serde_json::from_str(&dump).unwrap();
    let again = e.json("after", &["session", "export", "copy"]);
    for i in 0..2 {
        for part in [
            "processes",
            "meminfo",
            "cgroups",
            "created_at",
            "label",
            "boot_id",
        ] {
            assert_eq!(
                again["snapshots"][i][part], original["snapshots"][i][part],
                "{part}"
            );
        }
    }

    // The global --session selects the session too; bare `psm session` is a usage error.
    let via_flag = e.json("after", &["--session", "copy", "session", "export"]);
    assert_eq!(via_flag["session"]["name"], "copy");
    assert!(
        e.fails("after", &["session"])
            .contains("Usage: psm session")
    );
    assert!(
        e.fails("after", &["export"])
            .contains("unrecognized subcommand")
    );

    // Another database, from standard input, keeping the exported name.
    let other = Env::new("import");
    let mut child = other
        .command("after")
        .args(["--config", "/dev/null", "session", "import", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(dump.as_bytes())
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    assert_eq!(
        other.json("after", &["--session", "t", "snapshots", "--json"])[1]["label"],
        "after"
    );
    assert!(
        other.fails("after", &[]).contains("no active session"),
        "an import is inactive"
    );

    // --all: every session in one file, imported in one go, atomically.
    let everything = e.ok("after", &["session", "export", "--all"]);
    let parsed: Value = serde_json::from_str(&everything).unwrap();
    assert_eq!(
        parsed["sessions"].as_array().unwrap().len(),
        2,
        "t and copy"
    );
    assert!(
        e.fails("after", &["session", "export", "--all", "--format", "csv"])
            .contains("JSON only")
    );
    assert!(
        e.fails("after", &["session", "export", "--all", "t"])
            .contains("cannot be used with")
    );
    let all_file = file.with_extension("all.json");
    fs::write(&all_file, &everything).unwrap();
    let all_path = all_file.to_str().unwrap();
    assert!(
        e.fails("after", &["session", "import", all_path, "--name", "x"])
            .contains("single-session")
    );
    let third = Env::new("import-all");
    let o = third.ok("after", &["session", "import", all_path]);
    assert_eq!(o.matches("Imported session").count(), 2, "{o}");
    let names = third.json("after", &["sessions", "--json"]);
    assert_eq!(column(names.as_array().unwrap(), "name"), ["t", "copy"]);
    // A clash on any name imports nothing.
    assert!(
        third
            .fails("after", &["session", "import", all_path])
            .contains("already exists")
    );
    assert_eq!(
        third
            .json("after", &["sessions", "--json"])
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fs::remove_file(&all_file).unwrap();

    fs::write(&file, "{\"hello\": 1}").unwrap();
    assert!(
        e.fails("after", &["session", "import", path])
            .contains("not a psm export")
    );
    fs::remove_file(&file).unwrap();
}
