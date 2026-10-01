//! Built-in help: faq and shell completions.

mod common;

use common::*;

#[test]
fn faq_lists_questions_and_commands() {
    let e = Env::new("faq");
    let text = e.ok("before", &["faq"]);
    assert!(text.starts_with("1. Generic\n#"), "{text}");
    assert!(text.contains("\n\n2. Finding things now\n") && text.contains("\n5. Housekeeping\n"));
    assert!(text.contains("psm diff prev") && text.contains("Who started this process?"));
    let rows = e.json("before", &["faq", "--json"]);
    assert!(rows.as_array().unwrap().len() >= 40);
    assert_eq!(rows[0]["number"], "1.1");
    assert_eq!(rows[0]["section"], "Generic");
    assert_eq!(rows[0]["command"], "psm info");

    // Words filter the rows, case-insensitively; all must match; numbers stay stable.
    let what = e.json("before", &["faq", "WHAT", "--json"]);
    let questions: Vec<&str> = what
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["question"].as_str().unwrap())
        .collect();
    assert!(
        questions.len() >= 4 && questions.iter().all(|q| q.to_lowercase().contains("what")),
        "{questions:?}"
    );
    let two = e.json("before", &["faq", "memory", "version", "--json"]);
    assert_eq!(two.as_array().unwrap().len(), 1);
    assert_eq!(two[0]["command"], "psm compare old new --name X");
    let swapped = e.json("before", &["faq", "swapped", "--json"]);
    assert_eq!(swapped[0]["number"], "2.1");
    let by_number = e.ok("before", &["faq", "5.4"]);
    assert!(
        by_number.contains("psm purge") && !by_number.contains("psm reset"),
        "{by_number}"
    );
    assert!(
        e.ok("before", &["faq", "unicorn"])
            .contains("No question matches")
    );
    // The "X is any part of a process name" note appears only with a row that uses X.
    assert!(
        e.ok("before", &["faq", "swapped"])
            .trim_end()
            .ends_with("psm list --sort swap --top 10")
    );
    assert!(
        e.ok("before", &["faq", "still growing"])
            .contains("X is any part of a process name")
    );
}

#[test]
fn completions_cover_commands_options_and_values() {
    let e = Env::new("completions");
    let bash = e.ok("before", &["completions", "bash"]);
    for expected in [
        "diff",
        "session",
        "deactivate",
        "--group",
        "cgroup",
        "--metric",
    ] {
        assert!(bash.contains(expected), "bash script lacks {expected:?}");
    }
    assert!(
        e.ok("before", &["completions", "zsh"])
            .contains("#compdef psm")
    );
    assert!(
        e.ok("before", &["completions", "fish"])
            .contains("complete -c psm")
    );
    assert!(
        e.fails("before", &["completions", "dos"])
            .contains("invalid value")
    );
}
