# psm development


- Rust (stable, edition 2024)
- [`just`](https://github.com/casey/just)
- a C compiler (SQLite is built from source by `rusqlite`)
- `python3`, only to regenerate the test fixtures

## Layout

Workspace recipes and requirements are in [../DEVELOPMENT.md](../DEVELOPMENT.md);
everything below is `crates/psm`.

```text
crates/psm/
    Cargo.toml            package tool-psm, binary psm
    README.md             the crates.io page and the command list
    build.rs              commit hash and build date for `psm version`
    src/                  the program; see ARCHITECTURE.md for the module table
    tests/                integration tests: the real binary against fixtures, one file per area
        common/           shared helpers; mod.rs is the index, env.rs runs the binary, json.rs reads output
        workflow.rs       snapshots and diffs
        live.rs           list and info
        pid.rs            the one-process card
        reports.rs        report kinds, export formats, trend, brief
        sessions.rs       session rules, switching, export and import
        database.rs       permissions, versioning, purge, backup, reset
        config.rs         config file rules, `psm new` / `psm init` setup
        help.rs           faq, completions
    tests/fixtures/
        generate.py       writes the two trees below
        proc/before/      fake /proc: the "before" state
        proc/after/       fake /proc: the "after" state
```

## Tests

Unit tests sit next to the code (`analysis/diff.rs`, `analysis/group.rs`,
`analysis/view.rs`, `output/units.rs`, `output/table.rs`, `config.rs`). Integration tests in `tests/*.rs` run the built binary
with `--proc-root` pointed at a fixture tree.

### Fixtures

What the two trees contain:

| PID | before | after | Case |
|---|---|---|---|
| 2 | `kthreadd` | `kthreadd` | kernel thread: hidden by default, no memory |
| 100 | `code`, 1000 MiB | `code`, 1400 MiB | running process that grows |
| 200 | `old-helper`, 42 MiB | | gone |
| 300 / 310 | `rust-analyzer`, 812 MiB | `rust-analyzer`, 344 MiB | restarted under a new PID |
| 400 | `alpha`, 5 MiB | `beta`, 7 MiB | PID reused by another program |
| 500 | `my (we)ird) name`, 20 MiB RSS | 10 MiB RSS + 10 MiB swap | awkward name; owned by root with no `exe`/`io`/`smaps_rollup`; swapped out, not freed |
| 600 | | `node`, 72 MiB | new |

Each tree also has `meminfo`, `uptime`, `loadavg`, `sys/kernel/*`, and
a `cgroup-root/` directory that stands in for `/sys/fs/cgroup`.

The trees are checked in, but they are generated: a `stat` line has 52
fields. To change a case, edit `tests/fixtures/generate.py`, run
`just fixtures`, and commit the script and the result together.

Things a fixture cannot show: real permission errors (a missing file
stands in for `EACCES`), two CPU readings apart in time (tests use
`--interval 0`), and a reboot (covered by a unit test in `analysis/diff.rs`).

## Adding a command

1. Add the variant and its arguments to `Cmd` in `src/cli/commands.rs`
   (shared argument groups live in `src/cli/args.rs`, value lists with
   descriptions in `src/cli/values.rs`).
2. Write the handler in the matching file under `src/commands/` (or a
   new one, listed in `src/commands/mod.rs`) and add the arm to `run()`
   there. Build an `output::Table` and print it with `ctx.emit`, so
   text, `--json` and CSV come for free.
3. Add a case to the matching file in `tests/` against the fixtures
   (`tests/common/` has the helpers: `Env::ok`, `Env::json`, `Env::fails`).
4. Describe it in `USAGE.md`, `FAQ.md` (with `src/commands/faq.rs` kept
   in step) and the command list in `crates/psm/README.md`.

Adding a collected field: `model::Proc`, `collect/procfs.rs`, a new
`store/migrations/NNNN_name.sql` (never edit an existing one) listed in
`MIGRATIONS` in `store/mod.rs`, and the insert and load statements in
`store/snapshots.rs`. `psm update` brings existing databases along.

## Troubleshooting

**`error[E0554]: #![feature] may not be used on the stable release channel`**
in `anyhow` or `proc-macro2`. An editor running rust-analyzer on the
same `target/` directory left build-script output that a normal build
then reuses. Fix:

```bash
cargo clean -p anyhow -p proc-macro2
```

**rust-analyzer shows "proc macro not expanded"**. Editor-side version
mismatch; the build is not affected.
