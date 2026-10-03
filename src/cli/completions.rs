//! Shell completion scripts: clap_complete's output plus the flag forms of
//! the commands (`--snapshot`, `-l`), which clap_complete leaves out of its
//! static scripts. bash gets both the words and what follows them
//! (`psm --snapshot <TAB>` lists the snapshot commands); zsh and fish get
//! the words.

use std::fmt::Write as _;

use clap_complete::Shell;

/// A command's name, its flag forms (`--list`, `-l`) and its one-line help.
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
    match shell {
        Shell::Bash => bash(script, &flagged),
        Shell::Zsh => zsh(script, &flagged),
        Shell::Fish => fish(script, &flagged),
        _ => script,
    }
}

// ponytail: string surgery on clap_complete's output; the anchors are
// asserted by the completions test, so a clap_complete upgrade that moves
// them fails loudly instead of silently dropping the flags.
fn bash(script: String, flagged: &[Flagged]) -> String {
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
    script
        .replacen(
            "        psm)\n            opts=\"",
            &format!("        psm)\n            opts=\"{} ", words.join(" ")),
            1,
        )
        .replacen(
            "                cmd=\"psm\"\n                ;;\n",
            &format!("                cmd=\"psm\"\n                ;;\n{cases}"),
            1,
        )
}

fn zsh(script: String, flagged: &[Flagged]) -> String {
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

fn fish(mut script: String, flagged: &[Flagged]) -> String {
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
