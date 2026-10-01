//! Snapshots and diffs: the before/after workflow.

mod common;

use serde_json::Value;

use common::*;

#[test]
fn before_after_workflow() {
    let e = Env::new("workflow");
    e.ok("before", &["new", "t"]);
    let snap = e.ok("after", &["snap", "after"]);
    assert!(
        snap.contains("Top memory changes since #1 baseline"),
        "{snap}"
    );

    let d = e.json("after", &["diff", "--json"]);
    let impact: Vec<(&str, &str, i64)> = rows(&d, "memory")
        .iter()
        .map(|r| {
            (
                r["process"].as_str().unwrap(),
                r["status"].as_str().unwrap(),
                r["delta"].as_i64().unwrap() / MIB,
            )
        })
        .collect();
    assert_eq!(
        impact,
        [
            ("rust-analyzer", "restarted", -468),
            ("code", "running", 400),
            ("node", "new", 72),
            ("old-helper", "gone", -42),
            ("beta", "new", 7),
            // pid 400 was reused: alpha is gone, not "running as beta".
            ("alpha", "gone", -5),
            // "my (we)ird) name" went from 20 MiB RSS to 10 MiB RSS + 10 MiB swap:
            // swapped out, not freed, so it is not in the ranking at all.
        ]
    );
    assert_eq!(d["net_change"], -36 * MIB);
    assert_eq!(d["metric"], "RSS + swap");
    assert_eq!(column(rows(&d, "new"), "pid"), [600, 400]);
    assert_eq!(column(rows(&d, "gone"), "pid"), [200, 400]);
    assert_eq!(rows(&d, "restarted")[0]["old_pid"], 300);
    assert_eq!(rows(&d, "restarted")[0]["new_pid"], 310);

    // A full diff closes with a digest: the five largest movers, by process and by program.
    let top = &d["top"];
    assert_eq!(rows(top, "processes").len(), 5);
    assert_eq!(rows(top, "processes")[0]["process"], "rust-analyzer");
    assert_eq!(rows(top, "groups")[1]["name"], "code");
    assert_eq!(top["system_used"]["delta"], 1_000_000 * 1024);
    let text = e.ok("after", &["diff"]);
    let last = text.lines().last().unwrap();
    assert!(
        last.starts_with("Net process change: -36 MiB    System memory used: "),
        "{last}"
    );
    assert!(text.contains("TOP 5 MEMORY IMPACT   #1 baseline -> now"));
    let only_memory = e.json("after", &["diff", "--memory", "--json"]);
    assert_eq!(
        only_memory["top"],
        Value::Null,
        "a single section has no digest"
    );

    // Stored against stored; `prev` is the snapshot before the one it is compared with.
    let text = e.ok("after", &["diff", "prev", "latest", "--memory"]);
    assert!(
        text.starts_with("MEMORY IMPACT   #1 baseline -> #2 after   (RSS + swap)"),
        "{text}"
    );
    assert_eq!(
        e.json("after", &["diff", "1", "after", "--json"])["net_change"],
        -36 * MIB
    );
    assert!(
        e.fails("after", &["diff", "prev", "baseline"])
            .contains("no snapshot before #1")
    );

    // Without arguments the target is `now`: the live state, which is never stored.
    let live = e.json("before", &["diff", "prev", "--json"]);
    assert_eq!(live["from"]["label"], "after");
    assert_eq!(
        live["to"],
        serde_json::json!({ "id": null, "label": "now" })
    );
    assert_eq!(live["net_change"], 36 * MIB, "back to the 'before' state");
    let text = e.ok("before", &["diff", "latest", "--memory"]);
    assert!(
        text.starts_with("MEMORY IMPACT   #2 after -> now   (RSS + swap)"),
        "{text}"
    );
    assert_eq!(
        e.json("before", &["diff", "baseline", "now", "--json"])["net_change"],
        0
    );
    assert_eq!(
        e.json("before", &["show", "now", "--json"])[0]["command"],
        "code"
    );
    assert_eq!(
        e.json("after", &["snapshots", "--json"])
            .as_array()
            .unwrap()
            .len(),
        2,
        "nothing was stored"
    );
    assert!(e.fails("after", &["snap", "now"]).contains("reserved"));
    assert!(
        e.fails("after", &["diff", "now", "now"])
            .contains("both sides")
    );

    // Grouped by application: the cgroup's own total replaces summed RSS.
    let g = e.json(
        "after",
        &["diff", "--memory", "--group", "cgroup", "--json"],
    );
    assert_eq!(g["metric"], "cgroup memory.current + swap");
    assert_eq!(rows(&g, "memory")[0]["name"], "app-code.scope");
    assert_eq!(rows(&g, "memory")[0]["delta"], 500 * MIB);
}

#[test]
fn deep_snapshots_add_pss() {
    let e = Env::new("deep");
    e.before_and_after(&["--deep"]);
    let d = e.json("after", &["diff", "--memory", "--metric", "pss", "--json"]);
    assert_eq!(d["metric"], "PSS + swap");
    assert_eq!(rows(&d, "memory")[0]["delta"], (340 - 800) * MIB);

    let shown = e.json("after", &["show", "baseline", "--json"]);
    assert_eq!(shown[0]["pss"], 900 * MIB);
    assert_eq!(shown[3]["pss"], Value::Null, "smaps_rollup not readable");

    let shallow = Env::new("shallow");
    shallow.before_and_after(&[]);
    assert!(
        shallow
            .fails("after", &["diff", "--metric", "pss"])
            .contains("--deep")
    );
    let anon = shallow.json("after", &["diff", "--memory", "--metric", "anon", "--json"]);
    let ra = rows(&anon, "memory")
        .iter()
        .find(|r| r["process"] == "rust-analyzer")
        .unwrap();
    assert_eq!(ra["delta"], (300 - 700) * MIB);
}
