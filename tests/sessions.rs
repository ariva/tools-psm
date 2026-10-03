//! Sessions: rules, activating, export and import; the snapshot group.

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
            .contains("no snapshot before #0")
    );
    assert_eq!(
        e.json("before", &["diff", "--json"])["net_change"],
        0,
        "baseline -> now, unchanged"
    );
    assert!(e.fails("before", &["snap", "latest"]).contains("reserved"));

    // init makes the previous session inactive; nothing is deleted.
    e.ok("after", &["new", "b"]);
    assert!(e.fails("after", &["new", "a"]).contains("already exists"));
    let sessions = e.json("after", &["sessions", "--json"]);
    assert_eq!(
        column(sessions.as_array().unwrap(), "state"),
        ["inactive", "active"]
    );
    assert_eq!(
        e.json("after", &["sessions", "export", "a"])["snapshots"][0]["label"],
        "baseline"
    );

    let status = e.json("after", &["status", "--json"]);
    assert_eq!(status["session"]["name"], "b");
    assert_eq!(status["processes"]["current"], 5);

    // Sessions are compared by program, never by pid.
    let cmp = e.json("after", &["sessions", "compare", "a", "b", "--json"]);
    assert_eq!(cmp["totals"][0]["delta"], 0);
    assert_eq!(cmp["programs"][0]["name"], "rust-analyzer");
    assert_eq!(cmp["programs"][0]["delta"], -468 * MIB);

    // Activating: `a` becomes the active session again, `b` inactive. Nothing is lost.
    assert!(
        e.ok("after", &["sessions", "activate", "a"])
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
    let list = e.json("after", &["snapshots", "--json"]);
    assert_eq!(
        list.as_array().unwrap().len(),
        3,
        "snap went to a; plus the `now` row"
    );
    // The live state closes the list: no id, label `now`, a process count.
    assert_eq!(list[2]["id"], Value::Null);
    assert_eq!(list[2]["label"], "now");
    assert_eq!(list[2]["processes"], 5);
    let text = e.ok("after", &["snapshots"]);
    assert!(
        text.lines().last().unwrap().trim_start().starts_with("> "),
        "{text}"
    );
    assert_eq!(e.ok("after", &["-l"]).lines().count(), text.lines().count());
    assert_eq!(
        e.json("after", &["sessions", "export", "b"])["snapshots"]
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
    assert!(
        e.ok("after", &["sessions", "activate", "a"])
            .contains("already active")
    );
    assert!(
        e.fails("after", &["sessions", "activate", "nope"])
            .contains("no session")
    );
    // Deactivating leaves no active session; activate brings one back by id.
    e.ok("after", &["sessions", "deactivate"]);
    assert!(e.fails("after", &["snap"]).contains("no active session"));
    e.ok("after", &["sessions", "activate", "2"]);
    assert_eq!(
        e.json("after", &["status", "--json"])["session"]["name"],
        "b"
    );
    // Snapshots are numbered per session, from 0 = baseline.
    assert_eq!(e.json("after", &["snapshots", "--json"])[0]["id"], 0);
    assert_eq!(
        e.json("after", &["sessions", "export", "a"])["snapshots"][0]["id"],
        0
    );

    e.ok("after", &["sessions", "delete", "a"]);
    assert_eq!(
        e.json("after", &["sessions", "--json"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        e.fails("after", &["sessions", "export", "a"])
            .contains("no session")
    );
}

#[test]
fn export_then_import_round_trips() {
    let e = Env::new("export");
    e.before_and_after(&[]);
    let dump = e.ok("after", &["sessions", "export"]);
    let file = std::env::temp_dir().join(format!("psm-test-{}-export.json", std::process::id()));
    fs::write(&file, &dump).unwrap();
    let path = file.to_str().unwrap();

    // Same database: the name is taken, so it needs another one.
    assert!(
        e.fails("after", &["sessions", "import", path])
            .contains("pass --name")
    );
    assert!(
        e.ok("after", &["sessions", "import", path, "--name", "copy"])
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
    let cmp = e.json("after", &["sessions", "compare", "t", "copy", "--json"]);
    assert_eq!(cmp["programs"], serde_json::json!([]));
    e.ok("after", &["sessions", "activate", "copy"]);
    let copy = e.json("after", &["diff", "--json"]);
    assert_eq!(copy["net_change"], -36 * MIB);
    assert_eq!(copy["to"]["label"], "now");
    e.ok("after", &["sessions", "activate", "t"]);
    // Re-exporting the copy gives the same snapshots; only the ids are new.
    let original: Value = serde_json::from_str(&dump).unwrap();
    let again = e.json("after", &["sessions", "export", "copy"]);
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

    // activate needs a name; `session` is no command.
    assert!(
        e.fails("after", &["sessions", "activate"])
            .contains("required")
    );
    assert!(
        e.fails("after", &["session", "a"])
            .contains("unrecognized subcommand")
    );

    // Another database, from standard input, keeping the exported name.
    let other = Env::new("import");
    let mut child = other
        .command("after")
        .args(["--config", "/dev/null", "sessions", "import", "-"])
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
        other.json("after", &["sessions", "export", "t"])["snapshots"][1]["label"],
        "after"
    );
    assert!(
        other.fails("after", &[]).contains("no active session"),
        "an import is inactive"
    );

    // --all: every session in one file, imported in one go, atomically.
    let everything = e.ok("after", &["sessions", "export", "--all"]);
    let parsed: Value = serde_json::from_str(&everything).unwrap();
    assert_eq!(
        parsed["sessions"].as_array().unwrap().len(),
        2,
        "t and copy"
    );
    assert!(
        e.fails("after", &["sessions", "export", "--all", "--format", "csv"])
            .contains("JSON only")
    );
    assert!(
        e.fails("after", &["sessions", "export", "--all", "t"])
            .contains("cannot be used with")
    );
    let all_file = file.with_extension("all.json");
    fs::write(&all_file, &everything).unwrap();
    let all_path = all_file.to_str().unwrap();
    assert!(
        e.fails("after", &["sessions", "import", all_path, "--name", "x"])
            .contains("single-session")
    );
    let third = Env::new("import-all");
    let o = third.ok("after", &["sessions", "import", all_path]);
    assert_eq!(o.matches("Imported session").count(), 2, "{o}");
    let names = third.json("after", &["sessions", "--json"]);
    assert_eq!(column(names.as_array().unwrap(), "name"), ["t", "copy"]);
    // A clash on any name imports nothing.
    assert!(
        third
            .fails("after", &["sessions", "import", all_path])
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
        e.fails("after", &["sessions", "import", path])
            .contains("not a psm export")
    );
    fs::remove_file(&file).unwrap();
}

#[test]
fn snapshot_group() {
    let e = Env::new("snapshot-group");
    e.before_and_after(&[]); // #0 baseline, #1 after
    assert!(e.ok("before", &["snap", "two"]).contains("Snapshot #2 two"));
    assert!(
        e.ok("after", &["--snap", "three"])
            .contains("Snapshot #3 three")
    );

    // Deleting leaves the other numbers alone; the baseline and `now` cannot go.
    assert!(
        e.ok("after", &["snapshots", "delete", "1"])
            .contains("Deleted snapshot #1 after")
    );
    let numbers = |extra: &[&str]| -> Vec<i64> {
        e.json("after", &[extra, &["snapshots", "--json"]].concat())
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["id"].as_i64())
            .collect()
    };
    assert_eq!(numbers(&[]), [0, 2, 3]);
    assert!(
        e.fails("after", &["snapshots", "delete", "baseline"])
            .contains("cannot be deleted on its own")
    );
    assert!(
        e.fails("after", &["snapshots", "delete", "now"])
            .contains("live state")
    );
    assert!(
        e.fails("after", &["snapshots", "delete", "9"])
            .contains("no snapshot")
    );
    assert!(
        e.ok("after", &["snap", "four"])
            .contains("Snapshot #4 four")
    );
    assert_eq!(numbers(&[]), [0, 2, 3, 4]);

    // One snapshot out, into another session and back into this one.
    let dump = e.ok("after", &["export", "2"]);
    let v: Value = serde_json::from_str(&dump).unwrap();
    assert_eq!(v["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(v["snapshots"][0]["label"], "two");
    let file = std::env::temp_dir().join(format!("psm-test-{}-snap.json", std::process::id()));
    fs::write(&file, &dump).unwrap();
    let path = file.to_str().unwrap();
    e.ok("after", &["new", "b"]);
    assert!(
        e.ok("after", &["import", path])
            .contains("into session \"b\" as #1")
    );
    e.ok("after", &["sessions", "activate", "t"]);
    assert!(
        e.ok("after", &["import", path])
            .contains("into session \"t\" as #5")
    );
    assert_eq!(numbers(&[]), [0, 2, 3, 4, 5]);
    assert_eq!(
        e.json("after", &["diff", "2", "5", "--json"])["net_change"],
        0
    );
    e.ok("after", &["sessions", "activate", "b"]);
    // The file's baseline joins as a plain snapshot: the session keeps its own.
    fs::write(&file, e.ok("after", &["export", "baseline"])).unwrap();
    e.ok("after", &["import", path]);
    assert_eq!(e.json("after", &["snapshots", "--json"])[2]["label"], "");
    fs::remove_file(&file).unwrap();

    assert!(
        e.fails("after", &["export", "now"])
            .contains("never stored")
    );
    assert!(
        e.ok("after", &["export", "--format", "csv"])
            .starts_with("snapshot_id,label")
    );
    assert!(
        e.fails("after", &["snapshot", "export"])
            .contains("unrecognized subcommand")
    );

    // purge starts the active session over: without a yes nothing happens.
    assert!(
        e.ok("after", &["snapshots", "reset"])
            .contains("Nothing was deleted")
    );
    assert_eq!(numbers(&[]), [0, 1, 2]);
    let purged = e.ok("after", &["snapshots", "reset", "--yes"]);
    assert!(
        purged.contains("purged: 3 snapshot(s) deleted") && purged.contains("New baseline #0"),
        "{purged}"
    );
    assert_eq!(numbers(&[]), [0]);
    assert_eq!(
        e.json("after", &["status", "--json"])["session"]["name"],
        "b"
    );
    // snapshots reset is purge under another name.
    e.ok("after", &["snap", "y"]);
    assert!(
        e.ok("before", &["snapshots", "reset", "-y"])
            .contains("purged: 2 snapshot(s) deleted")
    );
    assert_eq!(numbers(&[]), [0]);
    // delete without a reference takes the latest; the last one is replaced by a new baseline.
    e.ok("after", &["snap", "x"]);
    assert!(
        e.ok("after", &["snapshots", "delete"])
            .contains("Deleted snapshot #1 x")
    );
    assert_eq!(numbers(&[]), [0]);
    let last = e.ok("before", &["snapshots", "delete"]);
    assert!(
        last.contains("the only one in session \"b\"") && last.contains("New baseline #0"),
        "{last}"
    );
    assert_eq!(numbers(&[]), [0]);
    assert_eq!(
        e.json("after", &["diff", "--json"])["net_change"],
        -36 * MIB,
        "the new baseline is the 'before' state"
    );
    assert_eq!(
        e.json("after", &["sessions", "--json"])
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
