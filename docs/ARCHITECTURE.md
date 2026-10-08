# Architecture

The repository is a Cargo workspace of independent tools. They share a
version, a lock file and the recipes, and nothing else: no common
library. The other tools drive `psm` as a program and read its
`--json` output, so each can be built, released or dropped on its own.

```text
crates/
  psm/         binary `psm` (package `tool-psm`): the tool, see docs/psm/
```

| Crate | Talks to | Docs |
|---|---|---|
| `tool-psm` | `/proc`, its SQLite database | [psm/ARCHITECTURE.md](psm/ARCHITECTURE.md) |

## The contract is `--json`

Every `psm` command prints a table that also renders as JSON (`--json`)
and CSV (`--csv`); that output is the interface the other tools use.
A JSON document is always the same envelope: `psm` (version, build,
commit, schema), `command` (`procs.show`), `options` (what was typed),
`started`, `elapsed_ms`, and the result under `data`. A consumer
switches on `command`, checks `psm.version` or `psm.schema`, and reads
`data`; it never parses argv. The shape is stable in the sense a
command line is: a field is added, never renamed or removed, within a
major version. A tool that needs a figure
`psm` does not print gets it by a change in `psm`, documented in
[psm/USAGE.md](psm/USAGE.md), not by reaching into its code.

## One version

`[workspace.package]` in the root `Cargo.toml` holds the version,
edition, license and repository for every crate; `[workspace.dependencies]`
holds the dependency versions so two tools cannot pin different ones. A
release bumps one number. Crates are published separately
(`cargo publish -p <package>`); none depends on another.
