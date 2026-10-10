//! Built-in help: faq, shell completions, the flag forms of the commands.

mod common;

use common::*;

#[test]
fn faq_lists_questions_and_commands() {
    let e = Env::new("faq");
    let text = e.ok("before", &["faq"]);
    assert!(text.starts_with("1. Generic\n#"), "{text}");
    assert!(
        text.contains("\n\n2. Finding things now\n")
            && text.contains("\n3. One process\n")
            && text.contains("\n6. Housekeeping\n")
    );
    assert!(text.contains("psm diff prev") && text.contains("Who started this process?"));
    let rows = e.json("before", &["faq", "--json"]);
    assert!(rows.as_array().unwrap().len() >= 40);
    assert_eq!(rows[0]["number"], "1.1");
    assert_eq!(rows[0]["section"], "Generic");
    assert_eq!(rows[0]["command"], "psm info   (live; nothing is stored)");

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
    assert_eq!(two[0]["command"], "psm sessions compare old new --name X");
    let swapped = e.json("before", &["faq", "swapped", "--json"]);
    assert_eq!(swapped[0]["number"], "2.1");
    let by_number = e.ok("before", &["faq", "6.4"]);
    assert!(
        by_number.contains("sessions purge") && !by_number.contains("psm sessions reset"),
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
            .ends_with("psm procs --sort swap --top 10")
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
    // The flag forms of the commands are added by hand; bash also follows them.
    assert!(bash.contains("opts=\"-p -i -n -s -d -l"), "{bash}");
    assert!(
        bash.contains("psm,-s)\n                cmd=\"psm__subcmd__snap\""),
        "{bash}"
    );
    // A positional's fixed values are offered right after the command only,
    // not at every later word.
    let report = bash.find("psm__subcmd__report)").unwrap();
    let block = &bash[report..report + 600];
    assert!(
        block.contains("[[ ${prev} == report ]] && opts=\"${opts} memory processes new gone growth cpu timeline trend meminfo\""),
        "{block}"
    );
    assert!(!block.contains("--watch memory processes"), "{block}");
    assert!(
        bash.contains("[[ ${prev} == completions ]] && opts=\"${opts} bash"),
        "{bash}"
    );
    assert!(
        bash.contains("[[ ${prev} == procs || ${prev} == -p ]] && opts=\"${opts} list show\""),
        "{bash}"
    );
    // Values that are not paths do not complete to the directory listing.
    let after = |flag: &str| {
        let i = bash
            .find(&format!("{flag})\n"))
            .unwrap_or_else(|| panic!("{flag} in script"));
        bash[i..].lines().nth(1).unwrap_or_default().to_string()
    };
    assert!(
        !after("--older-than").contains("compgen -f"),
        "{}",
        after("--older-than")
    );
    assert!(after("--db").contains("compgen -f"), "{}", after("--db"));
    assert!(
        !after("--user").contains("compgen -f"),
        "{}",
        after("--user")
    );
    let zsh = e.ok("before", &["completions", "zsh"]);
    assert!(zsh.contains("#compdef psm") && zsh.contains("'-s[Another snapshot"));
    // procs: words or a subcommand at position 1, options plus words after a word.
    assert!(
        zsh.contains("'1::words or subcommand:_psm__subcmd__procs_commands' \\\n\"*::: :->procs\""),
        "{zsh}"
    );
    assert!(
        !zsh.contains("\":: :_psm__subcmd__procs_commands\""),
        "{zsh}"
    );
    assert!(zsh.contains("psm-procs-command-$line[1]:"), "{zsh}");
    assert!(
        zsh.contains("            (*)\n_arguments \"${_arguments_options[@]}\" : \\\n'--group="),
        "{zsh}"
    );
    assert!(
        zsh.contains("'*::words:' \\\n&& ret=0\n            ;;\n        esac"),
        "{zsh}"
    );
    // track: options between words, and the command after `--` completed as one.
    let track = zsh
        .find("(track)\nlocal -a lb; lb=(${(z)LBUFFER})")
        .expect("track branch");
    let block = &zsh[track..track + zsh[track..].find("\nfi\n").expect("end of branch")];
    assert!(
        block.contains("_normal && ret=0\nelse\n_arguments"),
        "{block}"
    );
    assert!(block.contains("'*:words -- "), "{block}");
    assert!(!block.contains("'*::words"), "{block}");
    let fish = e.ok("before", &["completions", "fish"]);
    assert!(fish.contains("complete -c psm") && fish.contains("-s p -d"));
    // A positional's values, which clap_complete leaves out, right after the command only.
    assert!(
        fish.contains("-n \"__fish_psm_using_subcommand report; and test (commandline -pco)[-1] = report\" -f -a \"memory\\t'"),
        "{fish}"
    );
    assert!(
        fish.contains("and test (commandline -pco)[-1] = procs; and not __fish_seen_subcommand_from list show\" -a \"list\""),
        "{fish}"
    );
    assert!(
        e.fails("before", &["completions", "dos"])
            .contains("invalid value")
    );
}

#[test]
fn commands_have_flag_forms() {
    let e = Env::new("flag-forms");
    let help = e.ok("before", &["--help"]);
    // The top-level list is grouped, each row `name, -x`; long flag forms are in the footer.
    for expected in [
        "Commands:\n  procs, -p ",
        "\n  snap, -s ",
        "\n  diff, -d ",
        "\n  status ",
        "Sessions and snapshots:\n  sessions ",
        "Setup:\n  init ",
        "Help:\n  faq, -q ",
        "\n  version, -v ",
        "\n  config ",
        "work as a flag",
    ] {
        assert!(help.contains(expected), "{expected:?} in\n{help}");
    }
    assert!(
        !help.contains(", --procs") && !help.contains("Other:"),
        "{help}"
    );
    let by_name = e.ok("before", &["procs", "--interval", "0", "--top", "2"]);
    assert_eq!(
        e.ok("before", &["-p", "--interval", "0", "--top", "2"]),
        by_name
    );
    assert!(
        e.fails("before", &["--procs"])
            .contains("unexpected argument"),
        "double dashes are options, not commands"
    );
    assert!(e.ok("before", &["-q", "6.4"]).contains("sessions purge"));
    // procs: list is the default, show the other subcommand; both have flag forms.
    assert_eq!(
        e.ok(
            "before",
            &["procs", "list", "--interval", "0", "--top", "2"]
        ),
        by_name
    );
    assert_eq!(
        e.ok("before", &["procs", "-l", "--interval", "0", "--top", "2"]),
        by_name
    );
    assert!(e.ok("before", &["procs", "-h"]).contains("list, -l"));
    assert!(e.ok("before", &["snapshots", "-h"]).contains("reset, -r"));
    assert!(!e.ok("before", &["sessions", "-h"]).contains("reset, -r"));
    assert!(
        e.fails("before", &["list"]).contains("no active session"),
        "top-level list is snapshots list"
    );
    assert!(help.contains("\n  list, -l "), "{help}");

    // help, -h and --help are one command; -h is the brief form. Subcommands keep their own.
    let brief = e.ok("before", &["-h"]);
    assert!(brief.lines().count() < help.lines().count(), "{brief}");
    assert_eq!(e.ok("before", &["help"]), help);
    let diff = e.ok("before", &["help", "diff"]);
    assert!(
        diff.contains("Usage: psm {diff|-d}") && diff.contains("-h, --help"),
        "{diff}"
    );
    assert_eq!(e.ok("before", &["diff", "--help"]), diff);
    // Global options are accepted after a command but listed only at the top level.
    assert!(
        help.contains("--json") && !diff.contains("--json"),
        "{diff}"
    );
    assert!(e.json("before", &["faq", "6.4", "--json"]).is_array());
    assert_eq!(
        e.ok("before", &["-h", "diff"]),
        e.ok("before", &["diff", "-h"])
    );
    assert!(
        e.ok("before", &["help", "sessions", "export"])
            .contains("--no-cmdline")
    );
    assert!(
        e.fails("before", &["help", "bogus"])
            .contains("unknown command `bogus`")
    );

    // version is a command too, with the same flag forms as help.
    let version = e.ok("before", &["version"]);
    assert!(
        version.starts_with("psm (process state monitor) ")
            && version.contains("(built ")
            && version.contains(", commit "),
        "{version}"
    );
    assert_eq!(e.ok("before", &["-v"]), version);
    assert_eq!(e.ok("before", &["--version"]), version);
    // Row 1 is the version line, row 2 the description, row 3 author and repository;
    // only the top-level help has them.
    let mut rows = help.lines();
    assert_eq!(rows.next().unwrap_or_default(), version.trim_end());
    let second = rows.next().unwrap_or_default();
    assert!(
        second.starts_with("Named snapshots of the process table"),
        "{second}"
    );
    let third = rows.next().unwrap_or_default();
    assert!(
        third.starts_with("Arunas Ivanauskas <") && third.contains("https://github.com/"),
        "{third}"
    );
    assert!(!diff.contains("Arunas"), "{diff}");
    assert!(e.fails("before", &["-V"]).contains("unexpected argument"));
}
