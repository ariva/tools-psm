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
    // The writing psm is recorded: version, build date, commit, schema.
    assert_eq!(dump["psm"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(dump["psm"]["schema"].as_i64().unwrap() >= 3 && dump["psm"]["commit"].is_string());
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

#[test]
fn trend_ranks_growth_over_the_session() {
    let e = Env::new("trend");
    e.before_and_after(&[]);
    e.ok("after", &["snap", "again"]);
    // Points: before, after, after, now (= after): three steps, the first one moves.
    let t = e.json("after", &["report", "trend", "--json"]);
    assert_eq!(t["snapshots"], 3);
    let row = |name: &str| {
        rows(&t, "trend")
            .iter()
            .find(|r| r["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no row {name}"))
    };
    assert_eq!(row("code")["verdict"], "growing");
    assert_eq!(row("code")["up"], "1/3");
    assert_eq!(row("code")["delta"], 400 * MIB);
    assert_eq!(row("rust-analyzer")["verdict"], "shrinking");
    assert_eq!(
        row("node")["first"],
        0,
        "absent from the baseline counts as 0"
    );
    assert_eq!(
        row("code")["slope_per_hour"],
        Value::Null,
        "seconds apart: no slope"
    );
    assert_eq!(
        column(rows(&t, "trend"), "verdict")
            .last()
            .unwrap()
            .as_str(),
        Some("flat"),
        "flat rows last"
    );
    assert!(
        rows(
            &e.json(
                "after",
                &["report", "trend", "--min-delta", "10G", "--json"]
            ),
            "trend"
        )
        .iter()
        .all(|r| r["verdict"] == "flat")
    );
    assert_eq!(
        rows(
            &e.json(
                "after",
                &["report", "trend", "--group", "app", "--top", "1", "--json"]
            ),
            "trend"
        )
        .len(),
        1
    );
    assert!(
        e.fails("after", &["report", "trend", "0", "1"])
            .contains("no snapshot references")
    );

    let short = Env::new("trend-short");
    short.ok("before", &["new", "s"]);
    assert!(
        short
            .fails("after", &["report", "trend"])
            .contains("at least 3 points")
    );
}
