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

## Install

```bash
just install                # cargo install --path crates/psm --force --locked  ->  ~/.cargo/bin/psm
psm init                    # once: config file, database, shell completions
```

Linux only. See [crates/psm/README.md](crates/psm/README.md) for the command list.

## Repository

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) describes the workspace and
where code goes; [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) has the
recipes (`just check` before a commit) and how to add a crate.

## License

MIT, see [LICENSE](LICENSE).
