# psm architecture

How `psm` (`crates/psm`, package `tool-psm`) is put together and why.
For what it does from the outside, see [USAGE.md](USAGE.md); for the
repository and the other tools, [../ARCHITECTURE.md](../ARCHITECTURE.md).
Paths below are relative to `crates/psm/src`.

## Shape

One binary, no daemon, no background work. Every command is: read
something, compare or arrange it, print a table.

```text
/proc, /sys/fs/cgroup
        |
        v
  collect/  procfs.rs (procfs crate)  +  raw.rs (direct file readers)
        |
        v
  model::Snapshot  ------------------------------+
        |                                        |
        |  new / snap                            |  list / info / `now`
        v                                        |
    store/  (SQLite)  -- load -->  model::Snapshot
                                        |
                        +---------------+---------------+
                        v                               v
              analysis/diff.rs             analysis/view.rs, reports.rs
              (two snapshots)                    (one snapshot,
                        |                         status, reports)
                        +---------------+---------------+
                                        v
                                 output::Table
                                        |
                              text  /  JSON  /  CSV

  commands/  one handler file per command group, above all of this
```

The central type is `model::Snapshot`: a list of `Proc` plus system
figures. A snapshot collected a second ago and one loaded from the
database are the same type, so every view and comparison works on both.
That is what makes `now` (the live state as a diff target) cheap: it is
a snapshot that was never inserted.

## Modules

Folders are layers; every `mod.rs` is only an index. The dependency
direction is `commands` → `analysis` → `model`, with `commands` also
using `store` and `collect`, and everything printing through `output`.
Nothing below `commands` knows about clap.

| Path | Responsibility |
|---|---|
| `main.rs` | Entry point: module list, `main()`, exit code. |
| `model.rs` | `Proc`, `Snapshot`, `Metric`, `Filter` (every filter, precise flags and the loose word search, in one `keep`). Plain data, used by every layer. |
| `config.rs` | Config file loading, the default-file template, default paths, `~` expansion. |
| `cli/` | Command-line definition (clap derive). No logic. |
| `cli/mod.rs` | `Cli`: the global options. |
| `cli/args.rs` | Argument groups shared by several commands: filters, view, capture, diff. |
| `cli/values.rs` | The fixed value lists with a description per value (group keys, sort columns, metrics, info tables). |
| `cli/completions.rs` | clap_complete's script plus the commands' flag forms, which it leaves out. |
| `build.rs` | Stamps the short commit hash (`-dirty` when the tree has changes) and the build date into the binary for `psm version`; "unknown" when git or date is missing. Asks git for the repository directory, since the crate is not at the repository root. |
| `cli/commands.rs` | `Cmd`, `SessionCmd`, `ReportKind`, `ExportFormat`, help texts and examples. |
| `commands/` | One handler file per command group. |
| `commands/mod.rs` | `Ctx` (flags + environment + config merged), `run()`, the dispatch, and the `--watch` loop around it. |
| `commands/views.rs` | `procs` (`list`, the default, and `show`), `pid` (one process, with its session history), `info`. |
| `commands/capture.rs` | `new`, `snap`, and the never-stored live snapshot behind `now`. |
| `commands/compare.rs` | `diff`, `report`, `compare`: reference resolution, diff settings, sections, the closing digest, the `--brief` line. |
| `commands/sessions.rs` | `status`, `sessions list/activate/deactivate/export/import/delete`, `snapshots list/delete`, `export`, `import`. |
| `commands/maintenance.rs` | `purge`, `backup`, `reset`. |
| `commands/setup.rs` | `init`, `config`, where completion scripts go. |
| `commands/faq.rs` | The FAQ table and its filter. |
| `commands/track.rs` | `track`: start a command or pick a pid / words target, the sampling loop (period, warmup, `--for`, `--times`, `--pause`, signals), the progress line, the text tables, the document `--save` rewrites after every sample. |
| `collect/procfs.rs` | Builds a `Snapshot` from a proc root through the `procfs` crate. The only file that sees `procfs` types. |
| `collect/raw.rs` | Direct readers for what the crate does not cover: meminfo as raw key/value, uptime, loadavg, the whole-machine CPU ticks of `/proc/stat`, cgroup memory files, `/etc/passwd`. |
| `store/mod.rs` | `Db`: opening the file, schema creation and version check, backup. |
| `store/sessions.rs` | Active session, switching, deactivating, importing, purging, deleting. |
| `store/snapshots.rs` | Storing, listing, loading, deleting, appending snapshots; resolving `baseline`/`latest`/`prev`/number/label. |
| `store/migrations/` | The schema as numbered SQL steps, embedded in the binary; `store/mod.rs` applies them. |
| `analysis/group.rs` | `Grouper`: the group keys, application roots, the launcher list. |
| `analysis/diff.rs` | Classifies processes between two snapshots and builds the comparison tables. |
| `analysis/view.rs` | The table of one snapshot: filter, group, sort, cut; `%CPU` formulas. |
| `analysis/reports.rs` | System header, `status`, and the meminfo, cpu, timeline and trend reports. |
| `analysis/track.rs` | `Target` (a pid tree, or a filter rolled up by group), one `Sample` of it and the machine, `Stats` over a run with the warmup cut, `Final` over several runs. |
| `output/mod.rs` | `out` (pipe-safe printing); `print_json`, `json_document` and the envelope every JSON document gets (`psm`, `command`, `options`, `started`, `elapsed_ms`, `data`); `build_info`; the UTC timestamp without a time crate. |
| `output/table.rs` | `Table` and `Cell`: one definition rendered as text, JSON or CSV. |
| `output/units.rs` | Binary units, size and duration parsing. |
| `output/export.rs` | Session dump as JSON or CSV, and reading the JSON dump back. |

## Collection

**The `procfs` crate does the parsing.** It handles the awkward parts:
a process name containing spaces and `)` in `stat`, `smaps_rollup`,
`status` fields. It also accepts a custom root directory, which the
tests rely on.

**Direct readers live in one place.** When the crate is not enough, the
reader goes into `collect/raw.rs` and takes the same root. Current cases:
cgroup memory files (not under `/proc`), and meminfo, which is stored
key by key rather than as the crate's typed struct.

**Crate types stay inside `collect/procfs.rs`.** Storage and reports only
see `model` structs, so the crate can be replaced without touching them.

**A vanished process is not an error.** Processes exit while the table
is being read; a `stat` that fails skips the process. Permission errors
are remembered separately so the user can be told how many processes
were only partly readable.

**Missing is `None`, never 0.** Every optional field is an `Option`
from collection through storage (`NULL`) to output (`n/a`). A
comparison with a missing side is dropped from the ranking instead of
being reported as a change from or to zero.

**`psm` leaves itself out.** Its own process would otherwise appear as
new and gone in every diff.

**`(deleted)` is stripped from `exe`.** After an upgrade the running
old binary reads as `/path (deleted)`; without the strip an upgraded
program would split into two `exe` groups.

Per process, the cheap data comes from `stat` and `status`, both
world-readable. `exe`, `io` and `smaps_rollup` need access to the
process and are read only when allowed. `smaps_rollup` is read only
with `--deep`.

## Storage

SQLite, one file, five tables:

```text
sessions            id, name (unique), created_at, archived_at
snapshots           id, session_id, seq, label, description, system figures,
                    collector_uid, deep, clk_tck, uptime_seconds
processes           one row per process per snapshot
snapshot_meminfo    every /proc/meminfo key per snapshot
snapshot_cgroups    memory.current / memory.swap.current per cgroup
```

The snapshot numbers the user sees (`#0 baseline`, `#1`, ... in `psm
snapshots`, diff headers, `psm diff 0 2`, JSON `id`) are per session:
the `seq` column, `MAX(seq) + 1` at insert time, never reused. Deleting
a snapshot (`psm snapshots delete`) therefore leaves the others' numbers
alone; the baseline (`seq = 0`) only goes with its session. Row ids
stay inside `store`; `Snapshot::id` is the number. The live state
(`now`) has no number; `Snapshot::is_live` tells it apart by its label.

- **A snapshot is one transaction.** For `new` that transaction also
  makes the previous session inactive and creates the new one. Collection
  happens before it starts, so a failed collection leaves nothing
  behind.
- **The active session is derived**, not stored: the newest session
  with `archived_at IS NULL`. `new` deactivates before creating, so there
  is at most one.
- **Inactive is the `archived_at` column.** The user-facing word is
  *inactive*; the column kept its first name because there are no
  migrations. `psm sessions activate <name>` sets it on the session that was active and
  clears it on the target, in one transaction, so exactly one session
  is active afterwards.
- **SQLite writes and formats the timestamps** (`strftime('now')` in
  UTC, `datetime(..., 'localtime')` for display). There is no time
  crate in the dependency tree.
- **The schema is its migrations.** `store/migrations/NNNN_*.sql` are
  embedded in order; a fresh database runs them all, `psm update` runs
  the ones a file is missing (after a `VACUUM INTO` copy), so the two
  paths cannot drift. `PRAGMA user_version` is the last applied number.
  `Db::open` refuses an older file with a pointer to `psm update` and a
  newer one outright; it never migrates on its own.
- **The file is created `0600`** before SQLite opens it, because
  command lines can hold secrets.
- **Process age is not stored.** It is derived from the snapshot's
  `uptime_seconds` and `clk_tck` and the process's `start_time`.
- **`backup` is `VACUUM INTO`**: a consistent copy in one statement.
- **`reset` deletes the file**, not the rows. That also works when the
  file cannot be opened (a newer schema). It is the only command that
  asks for confirmation; the answer is read from standard input, so a
  script without `--yes` gets "no".
- **`--watch` is a loop around the dispatch**, not a mode inside each
  command: switch to the alternate screen (`?1049h`, as `watch(1)` does,
  so frames stay out of the scrollback), clear, run the same `Cmd`
  again, then count down a second at a time on a footer line rewritten
  with `\r` (the cursor never leaves it, so scrolling cannot misplace
  it). No terminal crate: three escape sequences, and a `libc` signal
  handler for SIGINT/SIGTERM/SIGHUP that writes `?1049l` and `_exit`s,
  so Ctrl-C never leaves the terminal on the alternate screen. The
  option is not global: `WatchArg` is flattened into the live views
  only (`procs`, `pid`, `info`, `diff`, `report`), so `snap --watch`
  (a loop that would fill the database) is a parse error and shell
  completion never offers `--watch` elsewhere. `watch_of` reads it off
  the command and still refuses a stored snapshot on both sides.
  Ctrl-C ends it the ordinary way; nothing needs cleaning up.
- **The version line comes first.** `commands::run` prints it before
  any command a person reads, only when stdout is a terminal and the
  command is not data (`--json`, `export`, `completions`, `help`,
  `version`).
- **Every JSON document is the envelope.** `cli::parse` records the
  command path and the options that came from the command line (from
  clap's matches: ids, raw values, the subcommand chain; no per-command
  code) before the `Cli` struct is built; `print_json` wraps whatever a
  command gives it, so a new command is enveloped without knowing. The
  level (`full`, `safe`, `none`) is the flag's `=LEVEL`, else the config
  key once the file is loaded. `--watch` resets the clock per round.
  Export files are the same envelope with the dump under `data`;
  `from_json` also reads the pre-3.0 bare shape.
- **Export and import are one format.** `Snapshot` and `Proc` derive
  both `Serialize` and `Deserialize`, so the JSON dump is the model
  itself and a new field travels both ways without extra code. Import
  inserts through the same `insert_snapshot` as `snap`, passing the
  original timestamp instead of "now". An imported session is stored
  inactive, so it can never take over as the active session. An
  `--all` file is the single-session dump repeated in a `sessions`
  array; import detects which of the two it was given and loads every
  session in one transaction.

Snapshot ids are global across sessions. Reference resolution
(`baseline`, `latest`, a label) is scoped to a session; a numeric id is
not.

## Comparing two snapshots

`Diff::new` in `analysis/diff.rs`.

1. **Same boot?** If `boot_id` differs, every process is new by
   definition. Instance matching is skipped and only grouped
   comparisons are produced.
2. **Instance identity is `(pid, start_time)`.** PIDs are reused, so a
   PID alone is not an identity. Present on both sides: *running*.
3. **Restart pairing.** Among the unmatched, a gone and a new instance
   with the same *restart key* become one *restarted* change, paired in
   start-time order. The key is `exe + cmdline`, falling back to `exe`
   when the command line was not stored, and to the process name when
   `exe` was not readable.
4. **The rest** is *gone* or *new*.

The memory impact ranking is a single list over all four kinds, sorted
by absolute change. `Metric` defines the value (`total`, `anon`, `pss`)
and always adds swap.

### Grouping

`Grouper` maps a process to a group key. It is built over *both*
snapshots of a comparison:

- a program whose `exe` is unreadable in either snapshot is keyed by
  its name in both, so the two sides stay comparable;
- the key `name` is the program name, which the kernel and the `Proc`
  struct call `comm`;
- `parent` needs the name of the parent process;
- `app` needs the whole parent chain. For every process it walks up to
  init and keeps the highest ancestor that is not a launcher (shell,
  terminal, session manager, sandbox wrapper). A Flatpak app comes
  first from its cgroup: systemd puts every sandbox layer, the crash
  handler and portal-spawned helpers in `app-flatpak-<id>-<pid>.scope`,
  and the chain cannot join those (the portal's sandboxes hang off
  `flatpak-portal`, zypak's off a second `bwrap`). The result is stored per
  process instance `(pid, start_time)` and only computed when the key
  is `app`. The key is the application's name, not its PID, so an
  application that restarted between two snapshots still lines up.

For `--group cgroup` the diff prefers the kernel's own figure
(`memory.current + memory.swap.current`) over summed RSS, but only when
both snapshots have it and no filter narrows the process set (a
filtered group is not the whole cgroup).

### Trend over a session

`report trend` is one `Grouper` built over every snapshot of the session
plus `now`, so a program keeps one key throughout. Each group becomes a
series with one value per point, 0 where the group is absent (its
process count there is zero), and a group seen at fewer than two points
is left out. `trend_stats` is pure: steps up, a least-squares slope in
bytes per hour (withheld when the span is under a minute, where it
would read as terabytes), and the verdict against `min_delta`: `flat`
below it end to end, `growing`/`shrinking` when no step of that size
goes against the end-to-end direction, `noisy` otherwise. The time axis
is `created_at` as epoch seconds (`strftime('%s')` in the `snapshots`
query), so it survives a reboot where `uptime_seconds` would not.

## Views of one snapshot

`report::view_table` is the single implementation behind `list`, `show`
and each table of `info`: filter, optionally group, sort, cut, render.
`info` calls it once per metric over the same collected snapshot.

`%CPU` needs two readings. For live views `Ctx::live` reads the tick
counters, sleeps for the interval, collects, and computes
`delta_ticks / (delta_seconds * CLK_TCK) * 100` per
`(pid, start_time)`. A stored snapshot has one reading, so `show` uses
the lifetime average.

## Tracking a program

`track` (`commands/track.rs`, `analysis/track.rs`) is the one command
that watches time pass instead of comparing two moments. Every sample
is a full collector pass (`Ctx::collect`), the same `Snapshot` the
other commands use; the target's processes are picked out of it:

- a started command or `--pid`: the roots and every process whose
  `ppid` chain reaches one of them (`track::tree`), re-walked on every
  sample so children that come and go are counted while they live;
- words: `Filter::keep` as in `procs`, then every process whose group
  key (default `app`) is among the matched ones, so a browser's
  renderers count though the word never names them.

Zombies (`state == "Z"`) are never members: they hold nothing, and a
target whose last process is a zombie counts as gone.

`%CPU` reuses `view::cpu_percent` with the previous sample as the
first reading: ticks of the current members minus their ticks last
time (a member born since counts from zero), over the real period. No
extra sleep inside a sample. The whole-machine figure is the busy
share of `/proc/stat`'s first line over the same period
(`raw::cpu_total`).

The loop is `track`'s own, not the `--watch` one: it prints nothing
until the end (a progress line on stderr when that is a terminal),
sleeps in 50 ms slices so a child exit, the `--for` deadline or a
signal is seen promptly, and treats a `--for` stop as the plan (SIGINT
to the child, 5 s grace, SIGKILL; exit 0). Ctrl-C sets a flag; the
terminal has already delivered it to a started child, so only a
SIGTERM to psm is forwarded. Statistics (`Stats`) are recomputed
after every sample over the samples past the warmup; `Final` takes
each counted run's peaks. `--save` writes the complete document after
every sample to `FILE.tmp` and renames it over `FILE`, so a reader
never sees a half file; the printed `--json` is the same document, so
a consumer parses one shape whether it polled or waited.

## Output

Every table is a `format::Table` of typed cells (`Text`, `Int`,
`Bytes`, `Delta`, `Pct`, ...). The same table renders as:

- text: human units, numeric columns right-aligned;
- JSON: raw values, bytes as numbers;
- CSV: raw values.

So there is one place where a column is defined and no separate JSON
structs to keep in step.

All output goes through `output::out`, which exits quietly when the
pipe is closed (`psm procs | head`). JSON goes through `print_json`,
which adds the envelope; keys keep insertion order (serde_json
`preserve_order`), so the header comes first and a row's keys follow
the table's columns.

## Configuration

`Ctx` in `commands/mod.rs` is the result of merging, per setting: flag, then
environment, then config file, then built-in default. Config structs
reject unknown fields, so a typo fails instead of being ignored.

Exactly one config file is read. `--config` and `PSM_CONFIG` are
handled by clap as one argument with an environment fallback. The path
is optional: a bare `--config` with no command is `psm config`. clap
skips the environment fallback when the flag is present without a
value, so `Cli::config_path` reads `PSM_CONFIG` itself in that case.

The file `psm new` creates on first use is `config::TEMPLATE`: every
key at its built-in default. A unit test asserts that the template
parses to exactly `Config::default()`, so the two cannot drift apart.
The file is created with `create_new`, which makes overwriting
impossible rather than merely checked for. `psm config` is dispatched
before the configuration is loaded, so `--init` works when the file
does not exist; `psm completions` and `psm init` are dispatched
there too: `init` creates the config file before anything reads it,
then opens the database (which creates it), then writes the completion
script for the shell in `$SHELL` to that shell's per-user location. Its script comes from
`clap_complete` over the same `Cli` definition as `--help`, so a new
option or value completes without extra work. `cli/completions.rs`
then adds the letter forms of the commands (`-s`, `-l`), which
clap_complete's static scripts leave out: bash gets the words and what
follows them, zsh and fish the words. The anchors it edits are asserted
by the completions test. `cli::command` also gives every valued
argument a `ValueHint`, because clap_complete offers file names for
any value without one: paths complete to files, everything else
(session names, snapshot references, labels, sizes, user names) to
nothing.

## Testing

Tests never read the real `/proc`. `--proc-root` points the collector
at `crates/psm/tests/fixtures/proc/before` and `after`, two small fake trees that
contain the interesting cases: growth, new, gone, restart, PID reuse,
a kernel thread, an unreadable process, a swapped-out process, a name
containing `)`.

The integration tests (`tests/*.rs`, one file per area, helpers in
`tests/common/`) run the real binary against them with a temporary
database. Pure logic (diff classification, unit formatting, the CPU
formula, config parsing) has unit tests next to the code.

See [DEVELOPMENT.md](DEVELOPMENT.md) for how to run and extend them.

## Known shortcuts

Marked in the code with a `ponytail:` comment.

| Where | Shortcut | Upgrade when |
|---|---|---|
| `collect::raw::usernames` | reads `/etc/passwd` only | LDAP/NSS users need names: use `getpwuid_r` |
| `analysis::group::LAUNCHERS` | a fixed list of launcher names decides where `--group app` stops | a desktop shell or terminal is missing: make it a config key |
| `Diff::group_impact` | a cgroup without `memory.current` keeps summed RSS in the same ranking as cgroups that have it | mixed rows mislead: split the table |
| `commands::track::save` | the whole document is rewritten after every sample, O(n²) bytes over a run | days at `--every 100ms`: add a JSON Lines sibling |

## Not built

Interval snapshots stored in a session (`track` samples but stores
nothing), notes and tags, thresholds with exit code `1`, HTML reports.
