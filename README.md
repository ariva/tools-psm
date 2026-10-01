# tool-psm

`psm` (process snapshot manager) takes named snapshots of the Linux
process table and shows what changed between them: which processes are
new, gone or restarted, and which ones moved memory the most. It is
built for before/after testing (software updates, long-running
workloads), not for continuous monitoring.

```bash
psm new chrome-update      # new session + baseline snapshot
# ... update, run the workload ...
psm diff                    # baseline -> now (the live state; nothing is stored)
psm snap after-update       # keep this state; prints the top memory changes
psm diff prev --memory      # who moved memory since the last snapshot
```

## Install

```bash
just install                # cargo install --path .  ->  ~/.cargo/bin/psm
```

Then, once:

```bash
psm init                    # config file, database, shell completions
```

It creates `~/.config/psm/config.toml` and the database if they are
missing, and installs tab completion for your shell (from `$SHELL`;
`--shell zsh` to pick one). Open a new shell afterwards. Running it
again is safe.

Linux only. The Cargo package is `tool-psm`; the binary is `psm`.

## Commands

| Command | What it does |
|---|---|
| `psm list` | Processes now, with CPU and memory; `--group`, `--sort`, `--top` |
| `psm info [N]` | System memory plus the top N by CPU, memory and threads |
| `psm new [name]` (or `psm session new`) | New session and its baseline; the previous one becomes inactive |
| `psm snap [label]` | Another snapshot in the active session |
| `psm` / `psm status` | Active session summary |
| `psm diff [a] [b]` | Changes between two snapshots; default baseline -> now |
| `psm report <kind>` | `memory`, `growth`, `processes`, `new`, `gone`, `cpu`, `meminfo`, `timeline` |
| `psm show [ref]` | One stored snapshot, same view as `list` |
| `psm snapshots` / `psm sessions` | What is stored |
| `psm compare <a> <b>` | Two sessions, by program |
| `psm session new` / `export` / `import <file>` | Start a session; dump one as JSON or CSV; load a JSON dump back |
| `psm session deactivate` / `delete <name\|id>` | Make the active session inactive; delete one session |
| `psm switch <name\|id>` | Make another session the active one |
| `psm purge`, `backup` | Housekeeping |
| `psm reset` | Delete **all** sessions and snapshots; asks first |
| `psm faq [words]` | Common questions and the command that answers each; words filter the list |
| `psm init` | First-time setup: config file, database, shell completions |
| `psm config` | Which config file is used; `--init` creates it |
| `psm completions <shell>` | Shell completion script, for other shells or machines |

`psm faq` prints the questions-to-commands table; `psm <command> --help` lists every option.

## Documentation

- [docs/FAQ.md](docs/FAQ.md): questions and the command that answers
  each, in numbered sections; the same list as `psm faq`.
- [docs/USAGE.md](docs/USAGE.md): every command, snapshot references,
  worked examples, how to read the numbers, configuration,
  privileges, limitations.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): modules, data flow,
  storage, the diff algorithm, design decisions.
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): recipes, tests, fixtures,
  troubleshooting.

## License

MIT, see [LICENSE](LICENSE).
