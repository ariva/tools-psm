//! The report kinds and the single-session export formats.

mod common;

use serde_json::Value;

use common::*;

#[test]
fn reports_and_export() {
    let e = Env::new("reports");
    e.before_and_after(&[]);

    let mem = e.json("after", &["report", "meminfo", "--json"]);
    let field = |name: &str| {
        rows(&mem, "meminfo")
            .iter()
            .find(|r| r["field"] == name)
            .cloned()
            .unwrap()
    };
    assert_eq!(field("Shmem")["delta"], 500000 * 1024);
    assert_eq!(
        field("HugePages_Total")["delta"],
        2,
        "page counts are not bytes"
    );
    assert!(
        rows(&mem, "meminfo").iter().all(|r| r["field"] != "Slab"),
        "unchanged fields are left out"
    );

    let cpu = e.json("after", &["report", "cpu", "--json"]);
    assert_eq!(rows(&cpu, "cpu")[0]["name"], "code");
    assert_eq!(rows(&cpu, "cpu")[0]["cpu_seconds"], 6.0);

    let growth = e.json("after", &["report", "growth", "--json"]);
    assert_eq!(
        column(rows(&growth, "growth"), "name"),
        ["code", "node", "beta"]
    );
    assert_eq!(rows(&growth, "growth")[0]["percent"], 40.0);

    let counts = e.json("after", &["report", "processes", "--json"]);
    assert_eq!(
        rows(&counts, "counts").len(),
        7,
        "every program, changed or not"
    );

    assert_eq!(
        e.json("after", &["report", "timeline", "--json"])
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let csv = e.ok("after", &["sessions", "export", "--format", "csv"]);
    let header = csv.lines().next().unwrap();
    assert!(header.starts_with("snapshot_id,label,created_at,") && header.contains(",cmdline,"));
    assert_eq!(csv.lines().count(), 1 + 6 + 6);
    assert!(csv.contains("code --type=renderer"));
    let private = e.ok(
        "after",
        &["sessions", "export", "--format", "csv", "--no-cmdline"],
    );
    assert!(!private.contains("cmdline") && !private.contains("--type=renderer"));
    let dump = e.json("after", &["sessions", "export"]);
    assert_eq!(dump["snapshots"][1]["meminfo"]["HugePages_Total"], 2);
    assert_eq!(
        dump["snapshots"][1]["cgroups"][3]["memory_current"],
        500 * MIB
    );

    // --no-cmdline at capture time stores NULL.
    let quiet = Env::new("quiet");
    quiet.ok("before", &["new", "q", "--no-cmdline"]);
    assert_eq!(
        quiet.json("before", &["sessions", "export"])["snapshots"][0]["processes"][1]["cmdline"],
        Value::Null
    );
}
