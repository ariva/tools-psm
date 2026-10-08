# CHANGES

## v3.0.0 — 2026-10-08

### Summary
Versioned JSON envelope, workspace layout, trend report, richer process search and a one-process card

### Breaking Changes
- Move every `--json` result under `data` behind a `psm`/`command`/`options` header (9cc2ef1)

### New Features
- Read the Flatpak app id from the cgroup for `--group app` so sandbox layers, crash handler and portal-spawned helpers count as one application (a698496)
- Add diff `--brief` printing the whole diff on one line with counts, net change and top movers (c768bbb)
- Add one-process card with session history (9c79754)
- Search processes by any word across pid, name, exe and command line (280e19f)
- Add precise `--pid` and `--cmdline` filters (280e19f)
- Add `--match-case` filters, with `--name` ignoring case by default (280e19f)
- Add `psm report trend` ranking every program over the session (49fe92c)

### Fixes
- `--watch` showing up in completion for sessions (1a1e5b0)

### Other
- Keep import reading 2.x exports (9cc2ef1)
- Document the new JSON document in USAGE, FAQ and ARCHITECTURE (9cc2ef1)
- Show how to install psm straight from GitHub (9c9c0bf)
- Bring the command overview back into the root README.md (9c9c0bf)
- Move psm into crates/psm with a workspace root Cargo.toml (936317e)
- Add stub crates for server, tui and gui (936317e)
- Split docs into docs/psm and per-tool folders (936317e)
- Turn the root README into a tools overview (936317e)
- Faq document sending output to a server with a curl pipe (daecf47)
- Fix stale doc (f84ad33)
- Clarify in the faq that psm diff (1d9122d)

## v2.2.0 — 2026-10-04

### Summary
CLI restructure with command groups, live view watching, descriptions and renaming, and in-place database upgrades

### Breaking Changes
- Change the `faq` short flag from `-f` to `-q` (27467d7)
- Remove long flags (2fa0a34)
- Restructure the CLI into `procs`, `sessions` and `snapshots` command groups (aba1170)

### New Features
- Add `--watch` flag to repeat live views until Ctrl-C (da84664)
- Add free-text descriptions to sessions and snapshots (0a343ce)
- Add `sessions rename` and `snapshots rename` (0a343ce)
- Add `base` as a snapshot reference alias (0a343ce)
- Add `psm update` for in-place database schema upgrades (0a343ce)
- Add single-letter flag aliases for commands (aba1170)
- Number snapshots per session, kept stable across deletes (aba1170)
- Add single-snapshot `export` and `import` (aba1170)
- Add `help` and `version` commands (aba1170)

### Fixes
- Restore back `--top` for info command (88c2bad)

### Other
- Describe `psm info` as a live view that stores nothing (4df0073)

## v2.1.5 — 2026-09-30

### Summary
Initial port of psm tool from private ariva-tools repo

### Other
- Init port: v2.1.5 from private tools repo (ab37950)
