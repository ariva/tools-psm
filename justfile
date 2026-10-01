# list available recipes
default:
    @just --list

# debug build for this machine -> target/debug/psm; extra flags go to cargo
build *args:
    cargo build {{args}}

# lint, then optimised build for this machine -> target/release/psm
release *args:
    just lint
    just build --release {{args}}

# format the code
fmt:
    cargo fmt

# format check + clippy, warnings are errors
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings

# unit + integration tests (the latter run the binary against tests/fixtures/proc)
test:
    cargo test

# everything CI runs
check: lint test

# run the tool from source: just run list --group comm
run *args:
    cargo run -q -- {{args}}

# install into ~/.cargo/bin; extra flags pass through: just install --force
install *args:
    cargo install --path . {{args}}

# Static x86_64 builds: one file that runs on any x86_64 Linux, no glibc
# version to match. Needs the musl target and C compiler once:
#   rustup target add x86_64-unknown-linux-musl && sudo apt install musl-tools
static_target := "x86_64-unknown-linux-musl"

# static x86_64 debug build -> target/x86_64-unknown-linux-musl/debug/psm; extra flags go to cargo
build-static *args:
    cargo build --target {{static_target}} {{args}}

# lint, then static x86_64 release build, the file the install script and tarballs ship
release-static *args:
    just lint
    just build-static --release {{args}}
    @echo "-> target/{{static_target}}/release/psm"

# aarch64 builds via `cross`: it compiles in a Docker/Podman container that has
# the aarch64 C compiler for the bundled SQLite. Needs `cargo install cross` and
# Docker running; the first run pulls the image. Also static musl.
aarch64_target := "aarch64-unknown-linux-musl"

# aarch64 debug build -> target/aarch64-unknown-linux-musl/debug/psm (not runnable here); extra flags go to cross
build-aarch64 *args:
    cross build --target {{aarch64_target}} {{args}}

# lint, then aarch64 release build for the release tarball (not runnable here; see test-aarch64)
release-aarch64 *args:
    just lint
    just build-aarch64 --release {{args}}
    @echo "-> target/{{aarch64_target}}/release/psm"

# the test suite on aarch64, under QEMU
test-aarch64:
    cross test --target {{aarch64_target}}

# regenerate the fake /proc trees the tests read
fixtures:
    python3 tests/fixtures/generate.py

# end-to-end new -> snap -> diff against a throwaway database
smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    export PSM_DB="$(mktemp --suffix=.db)"
    trap 'rm -f "$PSM_DB"' EXIT
    cargo run -q -- --config /dev/null new smoke
    cargo run -q -- --config /dev/null snap after
    cargo run -q -- --config /dev/null diff
