# psm-server

Planned. `crates/psm-server` is a stub whose `main` prints "not
implemented" and exits 2 (`publish = false` until it does something).
It is
independent of the other tools: it talks to the installed `psm` through
its `--json` output and depends on none of its code (see
[../ARCHITECTURE.md](../ARCHITECTURE.md)).

A receiver: machines post `psm snap --json` or `psm diff --json` output
at it (`psm ... --json | curl -d @- $URL`, see
[../psm/USAGE.md](../psm/USAGE.md#sending-output-to-a-server)), and it
keeps what arrives per machine, with the sender's name and IP, and
answers the same questions across machines. Its own storage; it never
reads `/proc` and shares no code with `psm`.
