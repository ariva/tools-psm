# psm-tui

Planned. `crates/psm-tui` is a stub whose `main` prints "not
implemented" and exits 2 (`publish = false` until it does something).
It is
independent of the other tools: it talks to the installed `psm` through
its `--json` output and depends on none of its code (see
[../ARCHITECTURE.md](../ARCHITECTURE.md)).

An interactive terminal view: live `procs`, a session's snapshots,
diffs and trends, sorted and filtered by key instead of flags. It runs
`psm <command> --json` as a child process and draws the result; every
figure it shows is one `psm` prints.
