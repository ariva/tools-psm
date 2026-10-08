//! The report kinds and the single-session export formats.

mod common;

use serde_json::{Value, json};

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

#[test]
fn brief_is_the_diff_on_one_line() {
    let e = Env::new("brief");
    e.before_and_after(&[]);
    let line = e.ok("after", &["diff", "--brief"]);
    assert_eq!(line.lines().count(), 1, "{line}");
    assert!(
        line.starts_with("#0 baseline -> now: 5 -> 5 processes, new 2, gone 2, restarted 1, net -36 MiB (RSS + swap); top: rust-analyzer -468 MiB, code +400 MiB, node +72 MiB"),
        "{line}"
    );
    assert!(
        e.ok("after", &["diff", "--brief", "--top", "1"])
            .ends_with("top: rust-analyzer -468 MiB\n")
    );
    assert!(
        e.ok("after", &["diff", "--brief", "--min-delta", "10G"])
            .contains("top: no change above 10.00 GiB")
    );
    let j = e.json("after", &["diff", "--brief", "--json"]);
    assert_eq!(j["processes"]["new"], 2);
    assert_eq!(j["net_change"], -36 * MIB);
    assert_eq!(
        column(rows(&j, "top"), "name"),
        ["rust-analyzer", "code", "node"]
    );
    assert_eq!(rows(&j, "top")[0]["status"], "restarted");
    assert!(
        e.fails("after", &["report", "growth", "--brief"])
            .contains("--brief is for `psm diff`")
    );
}

#[test]
fn json_envelope() {
    let e = Env::new("envelope");
    e.before_and_after(&[]);
    let raw = |args: &[&str]| -> Value { serde_json::from_str(&e.ok("after", args)).unwrap() };
    // Every test run passes --config and --proc-root; they are options too.
    let given = |doc: &Value| -> Value {
        let mut o = doc["options"].clone();
        for harness in ["config", "proc_root"] {
            o.as_object_mut().unwrap().remove(harness);
        }
        o
    };
    let doc = raw(&[
        "diff",
        "prev",
        "--memory",
        "--min-delta",
        "10M",
        "--group",
        "app",
        "--name",
        "code",
        "--json",
    ]);
    // The writing psm: version, build date, commit, schema.
    assert_eq!(doc["psm"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(doc["psm"]["schema"].as_i64().unwrap() >= 3 && doc["psm"]["commit"].is_string());
    assert_eq!(doc["command"], "diff");
    // Options as typed, by id: flags are true, numbers are numbers, --json itself is not one.
    assert_eq!(
        given(&doc),
        json!({ "a": "prev", "memory": true, "min_delta": "10M", "group": "app", "name": "code" })
    );
    let started = doc["started"].as_str().unwrap();
    assert!(
        started.len() == 20 && started.ends_with('Z') && started.starts_with("20"),
        "{started}"
    );
    assert!(doc["elapsed_ms"].is_u64());
    assert_eq!(doc["data"]["to"]["label"], "now");
    // Keys in order: header first, data last.
    let text = e.ok("after", &["diff", "--json"]);
    assert!(text.find("\"psm\"").unwrap() < text.find("\"data\"").unwrap());

    // Subcommands join with dots; words and lists are arrays; a pid is a number.
    let doc = raw(&[
        "procs", "show", "latest", "code", "rust", "--top", "2", "--json",
    ]);
    assert_eq!(doc["command"], "procs.show");
    assert_eq!(
        given(&doc),
        json!({ "snapshot": "latest", "words": ["code", "rust"], "top": 2 })
    );
    let doc = raw(&["pid", "100", "--json"]);
    assert_eq!(given(&doc), json!({ "pid": 100 }));
    assert_eq!(raw(&["--json"])["command"], "status");
    assert_eq!(
        raw(&["-l", "--json"])["command"],
        "list",
        "letter forms resolve to the command"
    );

    // safe: no descriptions, no paths; none: no block. The flag beats the config.
    let doc = raw(&["snap", "three", "pw is hunter2", "--deep", "--json=safe"]);
    assert_eq!(
        doc["options"],
        json!({ "label": "three", "deep": true }),
        "paths are out too"
    );
    let doc = raw(&["snap", "four", "pw is hunter2", "--json"]);
    assert_eq!(
        doc["options"]["description"], "pw is hunter2",
        "full is the default"
    );
    // snap and new answer in JSON too: what was stored, and the digest.
    assert_eq!(doc["data"]["snapshot"]["label"], "four");
    assert_eq!(doc["data"]["snapshot"]["description"], "pw is hunter2");
    assert_eq!(doc["data"]["processes"], 5);
    assert_eq!(doc["data"]["kernel_threads"], 1);
    // Same fixture as #3: nothing moved, so no digest.
    assert!(doc["data"]["top_changes"].is_null());
    let doc = raw(&["new", "fresh", "--json"]);
    assert_eq!(doc["data"]["baseline"]["id"], 0);
    assert_eq!(doc["data"]["previous"], "t");
    assert!(
        doc["options"]["proc_root"]
            .as_str()
            .unwrap()
            .contains("fixtures")
    );
    let doc = raw(&["diff", "--json=none"]);
    assert!(doc.get("options").is_none() && doc["data"]["net_change"].is_number());
    let cfg = std::env::temp_dir().join(format!("psm-test-{}-envelope.toml", std::process::id()));
    std::fs::write(&cfg, "[display]\njson_options = \"none\"\n").unwrap();
    let out = e
        .command("after")
        .args(["--config", cfg.to_str().unwrap(), "diff", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        doc.get("options").is_none(),
        "config decides for a bare --json"
    );
    std::fs::write(&cfg, "[display]\njson_options = \"loud\"\n").unwrap();
    let out = e
        .command("after")
        .args(["--config", cfg.to_str().unwrap(), "diff", "--json"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("display.json_options must be one of full, safe, none")
    );
    std::fs::remove_file(&cfg).unwrap();
    assert!(
        e.fails("after", &["diff", "--json=loud"])
            .contains("invalid value")
    );

    // version --json is the header's psm block as data.
    let doc = raw(&["version", "--json"]);
    assert_eq!(doc["data"], doc["psm"]);
}
