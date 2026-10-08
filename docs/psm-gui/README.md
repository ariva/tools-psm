# psm-gui

Planned. `crates/psm-gui` is a stub whose `main` prints "not
implemented" and exits 2 (`publish = false` until it does something).
It is
independent of the other tools: it talks to the installed `psm` through
its `--json` output and depends on none of its code (see
[../ARCHITECTURE.md](../ARCHITECTURE.md)).

A desktop window over the same `psm --json` output, for people who do
not live in a terminal. Shape to be decided once the TUI shows which
views matter.
