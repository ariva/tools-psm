# Docker images for psm

Images for things the host should not do itself: checks in shells it
does not have, and a demo recording that must show no real process.
Each subfolder is one image with its own Dockerfile and a `just` recipe
that builds and runs it; none of them is needed to build or install psm.

| Folder | What it checks | Recipe |
|---|---|---|
| `completions/` | the bash, zsh and fish completion scripts in the real shells: `psm report trend <TAB>` and friends, one shared case list | `just test-completions` |
| `demo/` | records `docs/assets/demo.gif`: psm around a synthetic workload, in a private PID namespace, through [VHS](https://github.com/charmbracelet/vhs) | `just demo` |

The build context is the repository root (`docker build -f
crates/psm/docker/<image>/Dockerfile .`), so an image can compile psm
from source in a `rust` stage; `.dockerignore` at the root keeps
`target`, `.git` and `.private` out of it.

## completions/

| File | Role |
|---|---|
| `Dockerfile` | `rust:1-slim-bookworm` builds `psm --release`; `debian:bookworm-slim` with bash, zsh and fish gets the binary and the three scripts installed the way USAGE.md says (`/etc/bash_completion.d/psm`, `~/.zfunc/_psm`, `~/.config/fish/completions/psm.fish`) |
| `run.sh` | the entrypoint: reads `cases.txt`, asks each shell what it would offer, checks the expectations, exits 1 on the first miss with what was offered; the last line says how many cases passed and how long they took, in total and per shell (zsh dominates: a fresh interactive zsh with `compinit` per case; reading stops when the prompt is redrawn or the output goes quiet) |
| `cases.txt` | one case per line: `shells \| command line \| +must -mustnot`; a space before the bar means "complete the next word", none means the last word is being typed; `all` or a comma list of `bash,zsh,fish` |
| `zsh-complete.zsh` | zsh completion only runs inside the line editor, so an interactive zsh is driven through a pseudo-terminal (`zpty`): type the line, press Tab, read the screen until the prompt comes back (a listing) or the output goes quiet (a single match inserted in place) |

How each shell is asked: bash by calling the script's `_psm` function
with `COMP_WORDS`/`COMP_CWORD` set as readline would; fish with its
built-in `complete -C "<line>"`; zsh through the pty harness. zsh and
fish list options only once a `-` is typed, so expectations about
options are bash-only or end the line with `--`.

Iterating without a rebuild (the image already has the binary):

```bash
docker run --rm -v "$PWD/crates/psm/docker/completions:/opt/completions:ro" psm-completions
```

Add a case for every completion fix in `cli/completions.rs`; the
textual anchors in `tests/help.rs` cover the same patches without
Docker.

## demo/

| File | Role |
|---|---|
| `Dockerfile` | `rust:1-slim-bookworm` builds `psm --release` (`.git` is outside the context, so `just demo` passes `--build-arg PSM_GIT_HASH` for the version line); `ghcr.io/charmbracelet/vhs` (ttyd, ffmpeg, headless Chrome) gets the binary, `jq`, a `dev` user and this folder as `/opt/demo`; the entrypoint is `vhs /opt/demo/demo.tape` |
| `demo.tape` | the script VHS plays: terminal size, theme, every command typed and how long its output stays on screen; `Output demo.gif` is relative to `/out`, the volume `just demo` mounts on `docs/assets` (a second `Output demo.mp4` line is all an MP4 needs) |
| `enter` | the tape's first, hidden command: `unshare -fp --mount-proc` into a fresh PID namespace, `proc/meminfo` and `proc/loadavg` bind-mounted over the kernel's, the workload copied to `/home/dev`, then a `dev` shell |
| `proc/` | the `meminfo` (16 GiB, 10 GiB available) and `loadavg` the demo shows instead of the host's |
| `workload` | `start [CACHE_MB]`, `build`, `stop`, `restart`: the fake programs the commands look at, each a symlink to python3 named `api-server`, `cache-daemon`, `job-runner`, `worker` or `build-job` so the process table shows those names |
| `app.py` | what those programs do: allocate memory that is really resident, hold it, start and kill workers, spin threads for a few seconds; nothing is read, written or connected to |

Nothing on screen is real. `enter` puts the shell in a fresh PID
namespace as user `dev`, so the process table holds only that shell and
the workload (VHS, ttyd and Chrome are outside it); the hostname is
`demo-box` (`--hostname`); the database and config are the `dev` user's
in the container. The kernel is shared, so what psm reads about the
machine is faked where it shows: `proc/meminfo` and `proc/loadavg` are
bound over `/proc`'s (the diff footer, the `HOST` row of `track`),
`--cpuset-cpus=0-3` makes the CPU count 4, and the tape keeps `psm info`
off the screen. The whole-machine CPU percentage in that `HOST` row
comes from the real `/proc/stat`. The mounts and the `unshare` are why
`just demo` runs the container with `--cap-add SYS_ADMIN` and seccomp
and apparmor unconfined. A last `docker run` hands the GIF to the
calling user; the volume is written as root otherwise.

Iterating on the tape or the workload without a rebuild (the image
already has the binary):

```bash
docker run --rm --hostname demo-box --cpuset-cpus=0-3 --cap-add SYS_ADMIN \
    --security-opt seccomp=unconfined --security-opt apparmor=unconfined \
    -v "$PWD/docs/assets:/out" -v "$PWD/crates/psm/docker/demo:/opt/demo:ro" psm-demo
```
