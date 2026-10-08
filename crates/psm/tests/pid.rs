//! `psm pid`: the one-process card and its session history.

mod common;

use serde_json::Value;

use common::*;

#[test]
fn pid_card_and_history() {
    let e = Env::new("pid");
    // No session yet: the card works on the live state alone.
    let code = e.json("before", &["pid", "100", "--json"]);
    assert_eq!(code["name"], "code");
    assert_eq!(code["exe"], "/usr/share/code/code");
    assert_eq!(code["cmdline"], "code --type=renderer");
    assert_eq!(code["app"], "code");
    assert_eq!(code["memory"]["rss"], 1000 * MIB);
    assert_eq!(code["memory"]["anon"], 800 * MIB);
    assert_eq!(code["cpu"]["user_seconds"], 50.0);
    assert_eq!(code["cpu"]["total_seconds"], 60.0);
    assert!(
        e.ok("before", &["pid", "100"])
            .contains("CPU:      1m total (50s user, 10s system)   6.1 % of one core"),
    );
    assert_eq!(code["cpu"]["lifetime_percent"], 6.1);
    assert_eq!(code["age_seconds"], 990.0);
    assert_eq!(code["snapshot"]["label"], "now");
    assert_eq!(code["session"], Value::Null);
    assert_eq!(code["history"].as_array().unwrap().len(), 0);
    assert!(
        e.fails("before", &["pid", "999"])
            .contains("no process 999 in now")
    );

    e.before_and_after(&[]);
    // node (600) was started by code (100): the chain names both, the app is code.
    let node = e.json("after", &["pid", "600", "--json"]);
    assert_eq!(column(node["chain"].as_array().unwrap(), "pid"), [100, 600]);
    assert_eq!(node["app"], "code");
    assert_eq!(node["session"], "t");
    let history = node["history"].as_array().unwrap();
    assert_eq!(history.len(), 1, "node is only in the second snapshot");
    assert_eq!(history[0]["label"], "after");
    assert_eq!(history[0]["rss"], 72 * MIB);

    // A stored snapshot as the source, and the full history of a long-lived process.
    let old = e.json("before", &["pid", "100", "baseline", "--json"]);
    assert_eq!(old["snapshot"]["id"], 0);
    assert_eq!(old["history"].as_array().unwrap().len(), 2);
    let text = e.ok("after", &["pid", "600"]);
    assert!(
        text.contains("Chain:    100 code > 600 node   app: code"),
        "{text}"
    );
    assert!(text.contains("In session t:"), "{text}");
    assert!(
        e.fails("after", &["pid", "200", "latest"])
            .contains("no process 200 in #1 after"),
        "gone in the chosen snapshot"
    );
}
