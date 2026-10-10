//! Shell completion scripts: clap_complete's output plus the letter forms of
//! the commands (`-s`, `-l`), which clap_complete leaves out of its static
//! scripts. bash gets both the letters and what follows them
//! (`psm -s <TAB>` completes snap's options); zsh and fish get the letters.

use std::fmt::Write as _;

use clap_complete::Shell;

/// A command whose first positional has fixed values, or that has
/// subcommands next to positionals: those words complete only right
/// after the command.
struct Gated {
    name: String,
    /// The first positional's id (`words`).
    positional: String,
    /// `(value, help)` of the first positional.
    values: Vec<(String, String)>,
    subcommands: Vec<String>,
    /// A trailing `-- <command>` positional (`track`).
    last: bool,
}

/// A command's name, its flag forms (`-l`; `--help`/`--version` for those two) and its one-line help.
struct Flagged {
    name: String,
    forms: Vec<String>,
    about: String,
}

pub fn script(shell: Shell) -> String {
    let mut cmd = super::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "psm", &mut buf);
    let script = String::from_utf8(buf).expect("clap_complete writes UTF-8");
    let flagged: Vec<Flagged> = cmd
        .get_subcommands()
        .filter_map(|sc| {
            let mut forms = Vec::new();
            if let Some(l) = sc.get_long_flag() {
                forms.push(format!("--{l}"));
            }
            if let Some(s) = sc.get_short_flag() {
                forms.push(format!("-{s}"));
            }
            (!forms.is_empty()).then(|| Flagged {
                name: sc.get_name().to_string(),
                forms,
                about: sc.get_about().map(ToString::to_string).unwrap_or_default(),
            })
        })
        .collect();
    // Words that belong right after a command only, where the static
    // scripts offer them at every position (bash) or not at all (fish):
    // the fixed values of a first positional (`report <kind>`,
    // `completions <shell>`), and the subcommands of a command that also
    // takes positionals (`procs <words>`: clap refuses `list` after a word).
    let gated: Vec<Gated> = cmd
        .get_subcommands()
        .filter_map(|sc| {
            let first = sc.get_positionals().next()?;
            let values: Vec<(String, String)> = first
                .get_possible_values()
                .iter()
                .map(|v| {
                    (
                        v.get_name().to_string(),
                        v.get_help().map(ToString::to_string).unwrap_or_default(),
                    )
                })
                .collect();
            let subcommands: Vec<String> = sc
                .get_subcommands()
                .map(|s| s.get_name().to_string())
                .collect();
            let last = sc.get_positionals().any(|p| p.is_last_set());
            (!values.is_empty() || !subcommands.is_empty() || last).then(|| Gated {
                name: sc.get_name().to_string(),
                positional: first.get_id().to_string(),
                values,
                subcommands,
                last,
            })
        })
        .collect();
    match shell {
        Shell::Bash => bash(script, &flagged, &gated),
        Shell::Zsh => zsh(script, &flagged, &gated),
        Shell::Fish => fish(script, &flagged, &gated),
        _ => script,
    }
}

// ponytail: string surgery on clap_complete's output; the anchors are
// asserted by the completions test, so a clap_complete upgrade that moves
// them fails loudly instead of silently dropping the flags.
fn bash(script: String, flagged: &[Flagged], gated: &[Gated]) -> String {
    let words: Vec<&str> = flagged
        .iter()
        .flat_map(|f| f.forms.iter().map(String::as_str))
        .collect();
    let mut cases = String::new();
    for f in flagged {
        for form in &f.forms {
            let _ = write!(
                cases,
                "            psm,{form})\n                cmd=\"psm__subcmd__{}\"\n                ;;\n",
                f.name
            );
        }
    }
    let mut script = script
        .replacen(
            "        psm)\n            opts=\"",
            &format!("        psm)\n            opts=\"{} ", words.join(" ")),
            1,
        )
        .replacen(
            "                cmd=\"psm\"\n                ;;\n",
            &format!("                cmd=\"psm\"\n                ;;\n{cases}"),
            1,
        );
    // `opts="... --help memory processes ..."` -> the values only when the
    // word before the cursor is the command itself.
    for g in gated
        .iter()
        .filter(|g| !g.values.is_empty() || !g.subcommands.is_empty())
    {
        let name = &g.name;
        let values = g
            .values
            .iter()
            .map(|(v, _)| v.as_str())
            .chain(g.subcommands.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let head = format!("        psm__subcmd__{name})\n            opts=\"");
        let Some(start) = script.find(&head) else {
            continue;
        };
        let line_end = start + head.len() + script[start + head.len()..].find('\n').unwrap_or(0);
        let line = script[start + head.len()..line_end].trim_end_matches('"');
        let moved: Vec<&str> = values.split(' ').collect();
        let kept: Vec<&str> = line.split(' ').filter(|w| !moved.contains(w)).collect();
        // The command's flag forms (`-p`) reach the same block.
        let forms = flagged
            .iter()
            .filter(|f| f.name == *name)
            .flat_map(|f| f.forms.iter())
            .map(|f| format!(" || ${{prev}} == {f}"))
            .collect::<String>();
        let patched = format!(
            "{}\"\n            [[ ${{prev}} == {name}{forms} ]] && opts=\"${{opts}} {values}\"",
            kept.join(" ")
        );
        script.replace_range(start + head.len()..line_end, &patched);
    }
    script
}

fn zsh(script: String, flagged: &[Flagged], gated: &[Gated]) -> String {
    let mut script = script;
    // A command with positionals and subcommands (`procs <words>` or
    // `procs list`): clap's spec puts the subcommand at positional 2, so
    // zsh offers `list show` after a word and after `list` itself. One
    // spec at position 1 for both, and the non-subcommand case completes
    // the command's own options plus more words.
    for g in gated.iter().filter(|g| !g.subcommands.is_empty()) {
        let head = format!(
            "({})\n_arguments \"${{_arguments_options[@]}}\" : \\\n",
            g.name
        );
        let Some(block) = script.find(&head) else {
            continue;
        };
        let opts_start = block + head.len();
        let sub_spec = format!("\":: :_psm__subcmd__{}_commands\" \\\n", g.name);
        let Some(sub_at) = script[opts_start..].find(&sub_spec).map(|i| opts_start + i) else {
            continue;
        };
        // The positional's own line sits right above the subcommand spec.
        let pos_at = script[opts_start..sub_at]
            .rfind(&format!("'::{} ", g.positional))
            .or_else(|| script[opts_start..sub_at].rfind(&format!("'*::{} ", g.positional)))
            .map(|i| opts_start + i);
        let Some(pos_at) = pos_at else {
            continue;
        };
        let opts = script[opts_start..pos_at].to_string();
        let merged = format!(
            "'1::{} or subcommand:_psm__subcmd__{}_commands' \\\n",
            g.positional, g.name
        );
        script.replace_range(pos_at..sub_at + sub_spec.len(), &merged);
        // The state handler: the subcommand is positional 1 now, and a
        // word there means "no subcommand": options and more words.
        let handler = format!("    ({})\n", g.name);
        let Some(h) = script.find(&handler) else {
            continue;
        };
        let Some(case_at) = script[h..].find("        case $line[2] in").map(|i| h + i) else {
            continue;
        };
        let fixed =
            script[h..case_at + "        case $line[2] in".len()].replace("$line[2]", "$line[1]");
        script.replace_range(h..case_at + "        case $line[2] in".len(), &fixed);
        let Some(esac) = script[case_at..]
            .find("\n        esac\n")
            .map(|i| case_at + i)
        else {
            continue;
        };
        let branch = format!(
            "\n            (*)\n_arguments \"${{_arguments_options[@]}}\" : \\\n{opts}'*::{}:' \\\n&& ret=0\n            ;;",
            g.positional
        );
        script.insert_str(esac, &branch);
    }
    // A command with words and a trailing `-- <command>` (`track`): clap's
    // spec leaves the command out and `*::words` stops option completion
    // after the first word. Single-colon words keep the options, and once
    // `--` is on the line the rest is completed as a command line of its
    // own. `_arguments -S` has already dropped the `--` from `$words`, so
    // the editor buffer is what says whether it is there.
    for g in gated.iter().filter(|g| g.last) {
        let head = format!("({})\n", g.name);
        let Some(block) = script.find(&head) else {
            continue;
        };
        let call = "_arguments \"${_arguments_options[@]}\" : \\\n";
        if !script[block + head.len()..].starts_with(call) {
            continue;
        }
        let Some(end) = script[block..]
            .find("&& ret=0\n")
            .map(|i| block + i + "&& ret=0\n".len())
        else {
            continue;
        };
        let inner = script[block + head.len()..end].replacen(
            &format!("'*::{} -- ", g.positional),
            &format!("'*:{} -- ", g.positional),
            1,
        );
        let wrapped = format!(
            "{head}local -a lb; lb=(${{(z)LBUFFER}})\n\
             local dd=${{lb[(I)--]}}\n\
             if (( dd )) && {{ (( dd < ${{#lb}} )) || [[ $LBUFFER == *' ' ]] }}; then\n\
             \x20   words=(${{lb[dd+1,-1]}}); [[ $LBUFFER == *' ' ]] && words+=('')\n\
             \x20   CURRENT=${{#words}}\n\
             \x20   _normal && ret=0\n\
             else\n{inner}fi\n"
        );
        script.replace_range(block..end, &wrapped);
    }
    let mut lines = String::new();
    for f in flagged {
        let about: String = f
            .about
            .chars()
            .map(|c| match c {
                '[' | ']' | ':' | '\'' => format!("\\{c}"),
                _ => c.to_string(),
            })
            .collect();
        for form in &f.forms {
            let _ = writeln!(lines, "'{form}[{about}]' \\");
        }
    }
    let anchor = "_arguments \"${_arguments_options[@]}\" : \\\n";
    script.replacen(anchor, &format!("{anchor}{lines}"), 1)
}

fn fish(mut script: String, flagged: &[Flagged], gated: &[Gated]) -> String {
    // The previous token is the command: `psm report <TAB>`, not `psm report trend <TAB>`.
    let after = |name: &str| {
        format!("__fish_psm_using_subcommand {name}; and test (commandline -pco)[-1] = {name}")
    };
    for g in gated {
        // clap_complete leaves a positional's values out of its fish script.
        if !g.values.is_empty() {
            let values = g
                .values
                .iter()
                .map(|(v, h)| format!("{v}\\t'{}'", h.replace('\'', "\\'")))
                .collect::<Vec<_>>()
                .join("\n");
            let _ = writeln!(
                script,
                "complete -c psm -n \"{}\" -f -a \"{values}\"",
                after(&g.name)
            );
        }
        // Its subcommands: offered after every word by clap's condition.
        for sub in &g.subcommands {
            let loose = format!(
                "complete -c psm -n \"__fish_psm_using_subcommand {}; and not __fish_seen_subcommand_from",
                g.name
            );
            let lines: Vec<String> = script
                .lines()
                .map(|l| {
                    if l.starts_with(&loose) && l.contains(&format!("\" -a \"{sub}\" ")) {
                        l.replacen(
                            &format!("__fish_psm_using_subcommand {}; ", g.name),
                            &format!("{}; ", after(&g.name)),
                            1,
                        )
                    } else {
                        l.to_string()
                    }
                })
                .collect();
            script = lines.join("\n") + "\n";
        }
    }
    for f in flagged {
        let about = f.about.replace('\'', "\\'");
        let mut line = String::from("complete -c psm -n \"__fish_psm_needs_command\"");
        for form in &f.forms {
            match form.strip_prefix("--") {
                Some(long) => {
                    let _ = write!(line, " -l {long}");
                }
                None => {
                    let _ = write!(line, " -s {}", &form[1..]);
                }
            }
        }
        let _ = writeln!(script, "{line} -d '{about}'");
    }
    script
}
