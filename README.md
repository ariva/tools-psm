# tools-psm

`psm` (process snapshot manager) takes named snapshots of the Linux
process table and shows what changed between them: which processes are
new, gone or restarted, and which ones moved memory the most. It is
built for before/after testing (software updates, long-running
workloads), not for continuous monitoring.

```bash
psm new chrome-update      # new session + baseline snapshot
# ... update, run the workload ...
psm diff                    # baseline -> now
psm snap after-update       # keep this state
```

## Tools

| Tool | Status | What it is | Docs |
|---|---|---|---|
| [psm](crates/psm/README.md) | ready | the command-line tool; package `tool-psm`, binary `psm` | [usage](docs/psm/USAGE.md), [FAQ](docs/psm/FAQ.md), [architecture](docs/psm/ARCHITECTURE.md), [development](docs/psm/DEVELOPMENT.md) |


The `psm` tool talks to other external tools through its `--json` output and shares no code with it.

## Commands

| Command | What it does |
|---|---|
| `psm procs [words]` | Processes now, with CPU and memory; words match pid, name, path or command line; `--group`, `--sort`, `--top` |
| `psm pid <pid> [ref]` | One process on one screen: exe, command line, parent chain, app, cgroup, memory breakdown, CPU, I/O, and its row in every snapshot of the session |
| `psm info [N]` | Live view: system memory information plus the top N by CPU, memory and threads; nothing is stored; also `--top N` |
| `--watch [duration]` | Repeat a live view (`procs`, `info`, `diff` against `now`) every 10s, or the duration given, until Ctrl-C |
| `psm new [name] [description]` | New session and its baseline; the previous one becomes inactive |
| `psm snap [label] [description]` | Another snapshot in the active session |
| `psm` / `psm status` | Active session summary |
| `psm diff [a] [b]` | Changes between two snapshots; default baseline -> now; `--brief` for one line |
| `psm report <kind>` | `memory`, `growth`, `processes`, `new`, `gone`, `cpu`, `meminfo`, `timeline`, `trend` |
| `psm procs show [ref]` | Processes of one stored snapshot (`list` is the live ones); same options |
| `psm list` (= `psm snapshots`) / `psm sessions` | What is stored |
| `psm sessions compare <a> <b>` | Two sessions, by program |
| `psm sessions activate <name\|id>` / `psm sessions deactivate` | Make a session active; leave none active |
| `psm sessions export` / `import <file>` / `delete <name\|id>` / `rename <name\|id> <new> [description]` | Dump one as JSON or CSV; load a JSON dump back; delete one; rename or describe one |
| `psm sessions purge --older-than <age>` | Delete inactive sessions older than that |
| `psm export [ref]` / `import <file>` | Dump one snapshot (`--all`: the whole session), add from a dump |
| `psm snapshots delete [ref]` / `rename` / `reset` | Delete one snapshot (default the latest); relabel one; start the session over with a new baseline, asks first |
| `psm backup <path>` | Copy the database |
| `psm sessions reset` | Delete **all** sessions and snapshots; asks first |
| `psm faq [words]` | Common questions and the command that answers each; words filter the list |
| `psm init` | First-time setup: config file, database, shell completions; `init force` starts from scratch |
| `psm update` | After a new psm: upgrade the database in place, check the config, refresh completions |
| `psm config` / `psm --config` / `psm --db` | Which config file and database are used; `config --init` creates the file |
| `psm completions <shell>` | Shell completion script, for other shells or machines |

`psm faq` prints the questions-to-commands table; `psm <command> --help` lists every option.
Some commands have a letter: `psm -s` is `psm snap` (`psm -h` shows them). Double dashes are options.

Then [docs/psm/USAGE.md](docs/psm/USAGE.md) for every option and
worked examples, [docs/psm/FAQ.md](docs/psm/FAQ.md) for questions and
the command that answers each.

## Install

```bash
cargo install --git https://github.com/ariva/tools-psm tool-psm --locked   # from GitHub, no checkout
just install                # from a checkout: cargo install --path crates/psm --force --locked
psm init                    # once: config file, database, shell completions
```

Linux only. The Cargo package is `tool-psm`; the binary is `psm`.

## Repository

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) describes the workspace and
where code goes; [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) has the
recipes (`just check` before a commit) and how to add a crate.

## License

MIT, see [LICENSE](LICENSE).
