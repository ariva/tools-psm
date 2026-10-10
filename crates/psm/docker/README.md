# Docker images for psm

Images that check psm in environments the host does not have. Each
subfolder is one image with its own Dockerfile and a `just` recipe that
builds and runs it; none of them is needed to build or install psm.

| Folder | What it checks | Recipe |
|---|---|---|
| `completions/` | the bash, zsh and fish completion scripts in the real shells: `psm report trend <TAB>` and friends, one shared case list | `just test-completions` |

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
