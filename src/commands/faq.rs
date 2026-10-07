//! `psm faq`: questions in numbered sections, and the command that answers
//! each. docs/FAQ.md is the same list; keep the two in step.

use serde_json::{Value, json};

use crate::output::{Cell, Table, out, print_json};

/// `X` stands for any part of a process name.
const FAQ: &[(&str, &[(&str, &str)])] = &[
    (
        "Generic",
        &[
            (
                "What is using the machine right now?",
                "psm info   (live; nothing is stored)",
            ),
            (
                "Which application uses the most memory, helpers included?",
                "psm procs --group app",
            ),
            (
                "What changed since I started?",
                "psm diff   (baseline -> now; psm new takes the baseline)",
            ),
            ("What changed since my last snapshot?", "psm diff prev"),
            ("Who moved memory the most?", "psm diff --memory"),
            (
                "Memory went down but no process grew. Where did it go?",
                "psm report meminfo",
            ),
            (
                "Did the new version use more memory?",
                "psm sessions compare old new --name X",
            ),
            (
                "How do I keep this state for later?",
                "psm snap after-update",
            ),
            ("How do I start a new experiment?", "psm new chrome-update"),
            (
                "How do I go back to an earlier experiment?",
                "psm sessions, then psm sessions activate <name>",
            ),
            (
                "Which snapshots do I have?",
                "psm list   (the last row, > now, is the live state)",
            ),
        ],
    ),
    (
        "Finding things now",
        &[
            (
                "Which processes are swapped out?",
                "psm procs --sort swap --top 10",
            ),
            (
                "Which processes have done the most disk I/O?",
                "psm procs --sort io --top 10",
            ),
            (
                "How many processes does each program run?",
                "psm procs --group name --sort count",
            ),
            (
                "How much memory does each user take?",
                "psm procs --group user",
            ),
            (
                "How much memory does each service or container take?",
                "psm procs --group cgroup",
            ),
            (
                "Which processes run this exact binary?",
                "psm procs --exe /opt/google/chrome/chrome",
            ),
            (
                "How do I keep a live view refreshing?",
                "psm info --watch   (every 10s; --watch 30 for 30s; also procs, diff)",
            ),
        ],
    ),
    (
        "One process",
        &[
            (
                "What is this process, and where does it come from?",
                "psm pid 4041081   (one screen: exe, command line, parents, app, cgroup, memory)",
            ),
            (
                "Which binary, parent and scripts, for a whole family of processes?",
                "psm procs --name X --group exe   (then parent, cmdline)",
            ),
            (
                "Who started this process?",
                "psm procs --name X --group parent",
            ),
            (
                "How do I find a process by any word I know about it?",
                "psm procs tsserver   (pid, name, path or command line)",
            ),
            (
                "How do I look at one or two known pids?",
                "psm procs --pid 4041081,4041135",
            ),
            (
                "Which of its processes is busy right now?",
                "psm procs --name X --sort cpu --top 3",
            ),
            (
                "How did one application change?",
                "psm diff --name X --memory",
            ),
            (
                "Which part of an application grew?",
                "psm diff --name X --group parent --memory",
            ),
            (
                "Is it still growing?",
                "psm report trend   (--name X for one program)",
            ),
            (
                "What did it look like earlier in the session?",
                "psm pid 4041081 baseline   (any snapshot reference)",
            ),
            (
                "It has exited; what was it?",
                "psm pid 4041081 latest   (or the number of a snapshot that has it)",
            ),
        ],
    ),
    (
        "Comparing",
        &[
            ("Which processes appeared?", "psm diff --new"),
            ("Which processes disappeared?", "psm diff --gone"),
            ("Which processes restarted?", "psm diff --restarted"),
            (
                "Which programs grew, and by what percentage?",
                "psm report growth",
            ),
            (
                "Is it a real leak or just cache?",
                "psm diff --memory --metric anon",
            ),
            (
                "Which program burned the most CPU since my last snapshot?",
                "psm report cpu prev",
            ),
            ("How do I compare two specific snapshots?", "psm diff 0 2"),
            (
                "What was in a snapshot I took earlier?",
                "psm list, then psm procs show <n>",
            ),
        ],
    ),
    (
        "Options and data",
        &[
            (
                "How do I get exact numbers for a script?",
                "add --json to any command",
            ),
            (
                "How do I see PSS instead of RSS?",
                "psm procs --deep, psm snap --deep",
            ),
            (
                "Why is a value n/a?",
                "other users' processes: run with sudo for full data",
            ),
            (
                "How do I keep passwords out of the database?",
                "psm snap --no-cmdline",
            ),
            ("How do I include kernel threads?", "add --kernel"),
            (
                "How do I leave noise out?",
                "--exclude-regex '^chrome_crashpad'",
            ),
            (
                "How do I send a diff to a server?",
                "psm diff --json | curl -d @- $URL",
            ),
        ],
    ),
    (
        "Housekeeping",
        &[
            (
                "How do I move a session to another machine?",
                "psm sessions export > s.json, then psm sessions import s.json",
            ),
            (
                "How do I move everything to another machine?",
                "psm sessions export --all > all.json, then psm sessions import all.json",
            ),
            (
                "How do I change the defaults?",
                "psm config, then edit the file",
            ),
            (
                "How do I delete old sessions?",
                "psm sessions purge --older-than 180d",
            ),
            ("How do I back up everything?", "psm backup ~/psm-backup.db"),
            (
                "How do I start over completely?",
                "psm sessions reset (the database), psm init force (database and config)",
            ),
            (
                "How do I delete one snapshot?",
                "psm snapshots delete 2 (a number or label; no argument: the latest)",
            ),
            (
                "How do I start the current session over?",
                "psm snapshots reset (all its snapshots go, a new baseline is taken)",
            ),
            (
                "How do I copy one snapshot into another session?",
                "psm export 2 > s.json, then psm sessions activate other and psm import s.json",
            ),
            (
                "Which config file and database are in use?",
                "psm --config, psm --db",
            ),
            ("How do I get tab completion?", "psm init"),
            (
                "I installed a new psm and it refuses my database?",
                "psm update   (upgrades it in place, keeps a copy)",
            ),
            ("Which version is this?", "psm version"),
            (
                "How do I rename a snapshot, or change its description?",
                "psm snapshots rename 2 new-label \"new description\"",
            ),
            (
                "How do I rename a session, or change its description?",
                "psm sessions rename old-session \"new session 1\" \"new description\"",
            ),
        ],
    ),
];

/// Every word must appear in the number, question or command, ignoring
/// case. Numbers stay as in the full list, so `1.4` always means the same row.
pub fn faq(words: &[String], json: bool) {
    let words: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let mut parts = Vec::new();
    let mut rows = Vec::new();
    let mut uses_placeholder = false;
    for (section_no, (section, questions)) in FAQ.iter().enumerate() {
        let mut t = Table::new(&[
            ("#", "number"),
            ("QUESTION", "question"),
            ("COMMAND", "command"),
        ]);
        for (i, (q, c)) in questions.iter().enumerate() {
            let number = format!("{}.{}", section_no + 1, i + 1);
            let text = format!("{number} {q} {c}").to_lowercase();
            if words.iter().all(|w| text.contains(w.as_str())) {
                uses_placeholder |= c.split_whitespace().any(|w| w == "X");
                t.rows
                    .push(vec![Cell::text(&number), Cell::text(*q), Cell::text(*c)]);
                rows.push(
                    json!({ "number": number, "section": section, "question": q, "command": c }),
                );
            }
        }
        if !t.rows.is_empty() {
            parts.push(format!("{}. {section}\n{}", section_no + 1, t.render()));
        }
    }
    if json {
        print_json(&Value::Array(rows));
    } else if parts.is_empty() {
        out(format!(
            "No question matches {:?}. `psm faq` lists them all.",
            words.join(" ")
        ));
    } else {
        // The placeholder note only when a shown command actually uses it.
        if uses_placeholder {
            parts.push("X is any part of a process name, e.g. chrome. `psm <command> --help` explains the options.".into());
        }
        out(parts.join("\n\n"));
    }
}
