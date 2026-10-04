//! Live views: list and info against the fixture tree.

mod common;

use serde_json::Value;

use common::*;

#[test]
fn list_reads_the_process_table() {
    let e = Env::new("list");
    let list = e.json("before", &["procs", "--interval", "0", "--json"]);
    let list = list.as_array().unwrap();
    assert_eq!(
        column(list, "command"),
        [
            "code",
            "rust-analyzer",
            "old-helper",
            "my (we)ird) name",
            "alpha"
        ],
        "sorted by memory; comm with ')' parsed; kernel thread hidden"
    );
    assert_eq!(list[0]["rss"], 1000 * MIB);
    // 6000 ticks over 990 s of lifetime at 100 Hz.
    assert_eq!(list[0]["cpu_percent"], 6.1);

    let all = e.json(
        "before",
        &[
            "procs",
            "--interval",
            "0",
            "--kernel",
            "--sort",
            "pid",
            "--json",
        ],
    );
    assert_eq!(all[0]["command"], "kthreadd");
    assert_eq!(all[0]["rss"], Value::Null, "no user memory is n/a, not 0");

    let groups = e.json(
        "before",
        &["procs", "--interval", "0", "--group", "cgroup", "--json"],
    );
    assert_eq!(groups[0]["name"], "app-code.scope");
    assert_eq!(groups[0]["cgroup_memory"], 1200 * MIB);
    assert_eq!(groups[1]["count"], 3);
    assert_eq!(
        groups[2]["cgroup_memory"],
        Value::Null,
        "cgroup without memory.current"
    );

    // node (600) was started by code (100): one application.
    let apps = e.json(
        "after",
        &["procs", "--interval", "0", "--group", "app", "--json"],
    );
    assert_eq!(apps[0]["name"], "code");
    assert_eq!(apps[0]["count"], 2);
    assert_eq!(apps[0]["rss"], (1400 + 72) * MIB);

    assert_eq!(list[0]["ppid"], 1, "every row carries its parent pid");
    let parents = e.json(
        "after",
        &["procs", "--interval", "0", "--group", "parent", "--json"],
    );
    let code = parents
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["parent"] == "code")
        .unwrap();
    assert_eq!(
        (code["ppid"].as_i64(), code["count"].as_i64()),
        (Some(100), Some(1))
    );

    let info = e.json("before", &["info", "2", "--interval", "0", "--json"]);
    // `--top` is the same as the number and wins over it.
    assert_eq!(
        e.json(
            "before",
            &["info", "--top", "2", "--interval", "0", "--json"]
        ),
        info
    );
    assert_eq!(
        e.json(
            "before",
            &["info", "5", "--top", "2", "--interval", "0", "--json"]
        ),
        info
    );
    assert_eq!(
        info["system"]["memory_used"],
        (16777216i64 - 12000000) * 1024
    );
    assert_eq!(
        column(rows(&info, "threads"), "command"),
        ["rust-analyzer", "code"]
    );
    assert!(
        e.fails("before", &["info", "--by", "luck"])
            .contains("invalid value 'luck'")
    );
    let err = e.fails("before", &["procs", "--group", "colour"]);
    assert!(
        err.contains("invalid value 'colour'") && err.contains("name"),
        "{err}"
    );
    // The kernel's term is no longer a key: grouping by program is `--group name`.
    assert!(
        e.fails("before", &["procs", "--group", "comm"])
            .contains("invalid value")
    );
    let by_name = e.json(
        "before",
        &["procs", "--interval", "0", "--group", "name", "--json"],
    );
    assert_eq!(by_name[0]["name"], "code");
}
