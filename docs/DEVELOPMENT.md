# Development

## Requirements

- Rust (stable, edition 2024)
- [`just`](https://github.com/casey/just)
- a C compiler (SQLite is built from source by `rusqlite`)
- `python3`, only to regenerate the test fixtures
- Docker, only for `just test-completions` (completion scripts checked
  in real bash, zsh and fish) and for the aarch64 builds through `cross`

## Recipes

```bash
just                  # list recipes
just check            # fmt check + clippy (warnings are errors) + tests
just test             # tests only
just fmt              # format
just run list --top 10        # run from source
just smoke            # new -> snap -> diff on this machine, throwaway database
just install          # cargo install --path crates/psm --force --locked -> ~/.cargo/bin/psm, then psm update
just fixtures         # regenerate crates/psm/tests/fixtures/proc
just test-completions # bash, zsh and fish completion in Docker (crates/psm/docker/completions); needs Docker running

just release-static   # static x86_64 binary (musl); needs: rustup target add x86_64-unknown-linux-musl, apt install musl-tools
just release-aarch64  # static aarch64 binary via `cross`; needs: cargo install cross, Docker running
just test-aarch64     # the tests on aarch64 under QEMU, via `cross`
```

`just --list` shows every recipe with its comment. The `build*`
recipes pass extra flags through to cargo (`just build --locked`), and
each `release*` recipe runs `lint` first, then its `build*` counterpart
with `--release`. Cross-built
binaries cannot run on this machine; `test-aarch64` is how they get
exercised, and CI on native ARM runners is the other way (see the
private publishing notes).

`just check` must pass before a commit.

`just smoke` and the tests pass `--config /dev/null` and set `PSM_DB`,
so your own configuration and database are never touched.

## Layout

```text
Cargo.toml            workspace: members, one version, shared dependency versions
Cargo.lock            one lock for every crate (committed)
justfile              recipes; cargo takes -p tool-psm where it matters
crates/psm/           the psm tool              -> docs/psm/
crates/psm/docker/    images for checks the host cannot run (completion scripts in real shells); README.md there
crates/psm-server/    stub: -> docs/psm-server/
crates/psm-tui/       stub  -> docs/psm-tui/
crates/psm-gui/       stub  -> docs/psm-gui/
docs/                 this file and ARCHITECTURE.md for the workspace; one folder per tool
```

Per tool: [psm/DEVELOPMENT.md](psm/DEVELOPMENT.md) (integration tests,
fixtures, adding a command, troubleshooting).

## Adding a tool

The three stubs already exist (`cargo run -p tool-psm-tui` prints "not
implemented" and exits 2); build inside them and drop `publish = false`
when there is something to publish. For a fourth tool:

```bash
cargo new crates/psm-web --name tool-psm-web
```

Then in its `Cargo.toml`: `version.workspace = true` and the other
`[workspace.package]` fields, dependencies as `{ workspace = true }`
(add new ones to `[workspace.dependencies]` in the root). It does not
depend on `tool-psm`: it runs the installed `psm` and reads `--json`.
`members = ["crates/*"]` picks it up; give it a folder under `docs/`, a
row in [ARCHITECTURE.md](ARCHITECTURE.md) and recipes in the `justfile`
with `-p`.

## Releasing

Bump `version` once, in the root `Cargo.toml`. Each crate is published
on its own with `cargo publish -p <package>`.
