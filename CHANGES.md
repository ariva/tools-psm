# CHANGES

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
