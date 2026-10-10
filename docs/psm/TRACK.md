# Tracking a program: `psm track`

`psm track` follows a program while it runs and prints, when it ends,
the minimum, maximum, average and last value of its memory, CPU,
threads and process count. It is the command for "how much did my
build use", "is the server leaking", "did the new version get
heavier", with min/max/avg instead of one number from `time -v`.

Every output below is what psm prints, trimmed only where a block
repeats; [USAGE.md](USAGE.md#tracking-a-program) is the reference (every flag,
the document layout, the configuration keys).

## What the rows mean

Every row is a sum over the target's processes at one sample: the
browser plus its renderers, `cargo` plus the compilers running at that
moment.

| Row | What it is | Watch it for |
|---|---|---|
| `rss` | memory in RAM right now (`VmRSS`), shared libraries counted once per process | what the program occupies on the machine |
| `anon` | heap and stack (`RssAnon`): what the program allocated itself | leaks, version-to-version comparison; the row to watch |
| `swap` | memory of the target moved out of RAM | pressure: rss dropping while swap rises is not memory freed |
| `cpu %` | CPU used between two samples, per core: 100 = one core busy, 1700 = seventeen | how hard it works; the first sample has none |
| `threads` | threads of all its processes, summed (`Threads:` in `/proc/<pid>/status`) | thread pools and runtimes growing inside the same processes |
| `procs` | how many processes the target had (separate pids, each with its own memory) | tabs, workers, compilers being started; memory grows per process |

`procs` and `threads` are different things: a process is a separate
pid with its own memory; threads live inside a process and share its
memory, so they count nothing extra in `rss`. Chrome with 80 processes
and 1600 threads is 80 pids with about 20 threads each. Read the two
together:

- `procs` up: the program started something (a tab, a compiler, a
  worker process). Memory grows per process.
- `threads` up, `procs` flat: the same processes got busier inside (a
  pool, an async runtime). Memory barely moves; CPU can.
- both flat, `rss`/`anon` up: growth inside the existing processes;
  `LAST` = `MAX` on `anon` run after run is a leak.

Columns: `MIN`, `MAX`, `AVG`, `LAST` over the samples after
`--warmup`; `AT MAX` is seconds from the start when `MAX` was seen,
empty when the row never changed. The `HOST` line is the machine, not
the target: cores, RAM, the lowest `MemAvailable` seen and its value
at the start, whole-machine CPU average and peak, the last load
average. [Memory metrics](USAGE.md#memory-metrics) in USAGE explains
rss, anon, file, shmem, pss and swap and which one answers which
question; `psm help track` lists the rows too.

## Three ways to pick the target

```bash
psm track -- cargo build --release      # start it; ends when it exits, its exit code is yours
psm track --pid 1234                    # an existing process and its descendants; ends when gone
psm track chrome --for 10m              # by words, like `psm procs chrome`; ends with --for or Ctrl-C
```

A started command keeps the terminal: on a terminal psm prints one
`tracking ...` line and a rule, then the command's own output, then a
rule, the psm version line again and the table; psm prints no progress line of its own there (pid
and words mode do, on stderr, since nothing else writes). In a pipe
the rules are left out. Processes are summed over the whole tree (`cargo` plus
every `rustc` it starts), re-read on every sample, so children that
come and go are counted while they live.

## 1. How much memory did my build use?

```bash
psm track --every 500ms -- cargo build -q -p tool-psm --release
```

```text
TRACK  cargo build -q -p tool-psm --release   exit 0   wall 5.2s   samples 11
HOST   box, 20 cpus, 125.51 GiB, free min 106.14 GiB (of 106.60 GiB at start), system cpu avg 39% max 89%, load 2.1
METRIC       MIN      MAX      AVG     LAST  AT MAX
rss      134 MiB  627 MiB  421 MiB  447 MiB  3.5s
anon      42 MiB  514 MiB  286 MiB  333 MiB  3.5s
swap         0 B      0 B      0 B      0 B
cpu %       98.0   1718.0    586.9    100.0  3.5s
threads       29       47       35       32  3.5s
procs          3        3        3        3
rows: sums over the target's processes per sample; cpu % per core (100 = one core); procs = how many processes; AT MAX = seconds from the start. `psm help track` explains each.
```

Reading it:

- Between the two rules is whatever the command printed; psm adds
  nothing while it runs.
- `exit 0`, `wall 5.2s`: the command's own exit code (psm exits with
  it too) and how long it ran. `samples 11`: one every 500 ms.
- `rss` peaked at 627 MiB 3.5 s in; `anon` (its own allocations) at
  514 MiB at the same moment. The difference is the file-backed part:
  the compiler binaries and libraries.
- `cpu %` is per core: 1718 means seventeen cores busy during that
  period. The first sample has no CPU figure (no period yet).
- `AT MAX` is when the maximum was seen, from the start; a row whose
  min equals its max leaves it empty.
- `HOST`: the machine around the run. `free min` is the lowest
  `MemAvailable` seen, `system cpu` the whole machine, not the target.

## 2. Is it stable across runs?

`--times` repeats a started command. `--skip-runs 1` shows the cold
run but leaves it out of the comparison; `--pause` rests between runs.

```bash
psm track --times 3 --skip-runs 1 --every 100ms --pause 300ms -- cargo test -q -p tool-psm --test sessions
```

```text
TRACK  cargo test -q -p tool-psm --test sessions   runs 3 (1 skipped)   every 0.1s, warmup 0.0s, pause 0.3s   total 2.6s
HOST   box, 20 cpus, 125.51 GiB, free min 106.39 GiB (of 106.42 GiB at start), system cpu avg 11% max 14%, load 2.1
RUN  EXIT  WALL  RSS MAX  ANON MAX  CPU AVG  THREADS MAX  PROCS MAX
  1  0     0.7s   71 MiB    25 MiB      2.2            8          4  skipped
  2  0     0.7s   46 MiB    21 MiB      2.2            6          2
  3  0     0.7s   71 MiB    25 MiB      2.2            8          4

FINAL (runs 2–3)     MIN     MAX     AVG  SPREAD
wall                0.7s    0.7s    0.7s    0.0s
rss max           46 MiB  71 MiB  58 MiB  26 MiB
anon max          21 MiB  25 MiB  23 MiB   4 MiB
swap max             0 B     0 B     0 B     0 B
cpu avg              2.2     2.2     2.2     0.1
threads max            6       8       7       2
procs max              2       4       3       2
rows: sums over the target's processes per sample; cpu % per core (100 = one core); procs = how many processes; AT MAX = seconds from the start. `psm help track` explains each.
```

One row per run with each run's peak (and its average CPU), then
`FINAL` over the runs that count: the smallest and largest peak, their
average, and `SPREAD` = max − min. A spread near zero means the figure
is reproducible; here the peak RSS swings by 26 MiB because the test
binary sometimes has a child process alive at sample time (`procs max`
2 vs 4), which is itself the finding. A run that exits non-zero ends
the series and becomes psm's exit code; `--keep-going` runs the rest.

## 3. Does the server leak under load?

Start it under `track`, or point `--pid` at a running one. `--warmup`
keeps the start-up out of the minimum so the growth after it stands
out. This example is a script that allocates 8 MiB every 100 ms for
four seconds and then idles:

```bash
psm track --warmup 1s --every 500ms -- python3 -c "$GROW"
```

```text
TRACK  python3 -c import time blocks = [] for i in range(40): blocks.append(bytearray(8 * 1024 * 1024)) time.sleep(0.1) time.sleep(1)   exit 0   wall 5.2s   samples 11, warmup 2 dropped
HOST   box, 20 cpus, 125.51 GiB, free min 106.24 GiB (of 106.66 GiB at start), system cpu avg 7% max 10%, load 2.4
METRIC      MIN      MAX      AVG     LAST  AT MAX
rss      99 MiB  331 MiB  239 MiB  331 MiB  4.0s
anon     92 MiB  324 MiB  233 MiB  324 MiB  4.0s
swap        0 B      0 B      0 B      0 B
cpu %       0.0      6.0      2.7      0.0  3.0s
threads       1        1        1        1
procs         1        1        1        1
rows: sums over the target's processes per sample; cpu % per core (100 = one core); procs = how many processes; AT MAX = seconds from the start. `psm help track` explains each.
```

What a leak looks like: `anon` climbs from the warmed-up minimum to
the maximum and `LAST` equals `MAX`, nothing was given back. `warmup 2
dropped`: the two samples inside the first second are out of the
statistics (they are still in the saved series). With `--save` the
series shows the slope directly:

```bash
jq -c '.data.runs[0].series | map(.target.rss)' leak.json
```

```text
[19705856,61669376,103632896,145596416,179167232,221130752,263094272,305057792,347021312,347021312,347021312]
```

For a real server: `psm track --pid $(pidof myserver) --warmup 30s
--for 10m` while the load test runs. A process that holds steady at
some level is fine; one whose `anon` row has `LAST` = `MAX` on every
run is not.

## 4. How heavy is a program by name?

Words mode takes the `psm procs` matcher: words that must all appear
in the pid, name, path or command line, or the precise `--name`,
`--exe`, `--cmdline`, `--user`. The set is re-matched on every sample
and rolled up by `--group` (default `app`), so a browser's renderers
count even though the word never names them.

```bash
psm track --name python3 --group name --for 3s --every 500ms
```

```text
TRACK  --name python3 (name)   stopped by --for   wall 3.0s   samples 6
HOST   box, 20 cpus, 125.51 GiB, free min 106.37 GiB (of 106.53 GiB at start), system cpu avg 3% max 4%, load 1.1
METRIC       MIN      MAX      AVG     LAST  AT MAX
rss       43 MiB  235 MiB  139 MiB  235 MiB  2.5s
anon      30 MiB  222 MiB  126 MiB  222 MiB  2.5s
swap     180 KiB  180 KiB  180 KiB  180 KiB
cpu %        2.0      8.0      5.2      6.0  1.5s
threads        2        2        2        2
procs          2        2        2        2
rows: sums over the target's processes per sample; cpu % per core (100 = one core); procs = how many processes; AT MAX = seconds from the start. `psm help track` explains each.
```

Two Python processes on the machine, one of them the allocator from
example 3, followed for three seconds. Without `--for` it runs until
Ctrl-C or until nothing matches any more. Mind the roll-up: a word
that appears in a command line of some other application (`psm track
python3` also matches a terminal whose command line mentions python)
pulls that whole application in, like `psm procs python3 --group app`
would; `--name` and `--group name` or `--group pid` keep it tight.

## 5. Feeding another tool

`--save FILE` writes the complete JSON document after every sample
(to `FILE.tmp`, then renamed over `FILE`, so a reader never sees a
half document) and once more at the end. `--json` prints the same
document once at the end; with a started command prefer the file, the
command's own stdout would land in the same stream.

```bash
psm track --every 500ms --save build.json -- cargo build -q -p tool-psm --release
```

The file, abridged (two of eleven samples shown, `...` for repeated
blocks):

```json
{
  "psm": { "version": "3.0.0", "built": "2026-10-09", "commit": "4469c47", "schema": 3 },
  "command": "track",
  "options": { "save": "build.json", "every": "500ms", "command": ["cargo", "build", "-q", "-p", "tool-psm", "--release"] },
  "started": "2026-10-09T19:44:56Z",
  "elapsed_ms": 5326,
  "data": {
    "status": "done",
    "target": { "mode": "spawn", "command": ["cargo", "build", "-q", "-p", "tool-psm", "--release"] },
    "host": { "hostname": "box", "cpus": 20, "memory_total": 134762676224, "swap_total": 2147479552, "clk_tck": 100, "kernel": "6.8.0-139-generic" },
    "settings": { "every_ms": 500, "warmup_ms": 0, "pause_ms": 0, "for_ms": null, "times": 1, "skip_runs": 0 },
    "runs": [
      {
        "run": 1, "status": "done", "ended": "exit", "exit": 0, "skipped": false,
        "started": "2026-10-09T19:44:56Z", "wall_ms": 5212, "samples": 11, "dropped": 0,
        "stats": {
          "rss":     { "min": 140288000, "max": 656965632, "avg": 440951529, "last": 469209088, "at_max_ms": 3501 },
          "anon":    { "min": 44191744,  "max": 539250688, "avg": 300333987, "last": 348762112, "at_max_ms": 3501 },
          "swap":    { "min": 0, "max": 0, "avg": 0, "last": 0, "at_max_ms": 0 },
          "cpu":     { "min": 98.0, "max": 1718.0, "avg": 586.9, "last": 100.0, "at_max_ms": 3501 },
          "threads": { "min": 29, "max": 47, "avg": 34.6, "last": 32, "at_max_ms": 3501 },
          "procs":   { "min": 3, "max": 3, "avg": 3.0, "last": 3, "at_max_ms": 0 },
          "system": {
            "mem_available": { "min": 113971646464, "max": 114463748096, "avg": 114174544431, "last": 113996152832, "at_max_ms": 0 },
            "mem_used":      { "min": 20298928128, "max": 20791029760, "avg": 20588131793, "last": 20766523392, "at_max_ms": 4002 },
            "cpu":           { "min": 13.3, "max": 89.2, "avg": 39.0, "last": 13.3, "at_max_ms": 3501 }
          }
        },
        "series": [
          { "t_ms": 0,
            "target": { "rss": 140288000, "anon": 44191744, "swap": 0, "cpu": null, "threads": 29, "procs": 3 },
            "system": { "mem_available": 114463748096, "mem_used": 20298928128, "swap_used": 962011136, "cpu": null, "load_1": 2.11 } },
          { "t_ms": 500,
            "target": { "rss": 275214336, "anon": 107307008, "swap": 0, "cpu": 98.0, "threads": 29, "procs": 3 },
            "system": { "mem_available": 114416537600, "mem_used": 20346138624, "swap_used": 962011136, "cpu": 14.4, "load_1": 2.11 } },
          "..."
        ]
      }
    ],
    "final": {
      "runs": [1], "exit": 0,
      "wall_ms":     { "min": 5212, "max": 5212, "avg": 5212, "spread": 0 },
      "rss_max":     { "min": 656965632, "max": 656965632, "avg": 656965632, "spread": 0 },
      "anon_max":    { "min": 539250688, "max": 539250688, "avg": 539250688, "spread": 0 },
      "swap_max":    { "min": 0, "max": 0, "avg": 0, "spread": 0 },
      "cpu_avg":     { "min": 586.9, "max": 586.9, "avg": 586.9, "spread": 0.0 },
      "threads_max": { "min": 47, "max": 47, "avg": 47, "spread": 0 },
      "procs_max":   { "min": 3, "max": 3, "avg": 3, "spread": 0 },
      "system": {
        "mem_available_min": { "min": 113971646464, "max": 113971646464, "avg": 113971646464, "spread": 0 },
        "cpu_max":           { "min": 89.2, "max": 89.2, "avg": 89.2, "spread": 0.0 }
      }
    }
  }
}
```

While it runs the same document has `"status": "running"`, a `"run"`
field naming the run in progress, that run with `"status": "running"`
and `"exit": null`, and `"final": null`. `final` is there with one run
too, so a consumer reads one path whatever `--times` was. Sizes are
bytes, CPU is per cent per core, times are milliseconds from the start
of the run.

Following it from another terminal:

```bash
watch -n1 'jq -r ".data.runs[-1].series[-1] | [.t_ms, .target.rss, .system.mem_available] | @tsv" build.json'
jq '.data.runs[].stats.rss.max' build.json          # peak of every run
jq '.data.final.anon_max' bench.json                 # how reproducible the peak is
jq -r '.data.status' build.json                      # running | done | interrupted | failed
```

## 6. Reading the table

The rows and columns are explained [at the top](#what-the-rows-mean);
what the other parts of the output are:

| Part | Meaning |
|---|---|
| header end | `exit N` (a started command), `gone` (pid or words target disappeared), `stopped by --for`, `interrupted` (Ctrl-C) |
| `samples N, warmup M dropped` | samples taken, and how many fell inside `--warmup` (still in the saved series) |
| `RUN` table | one row per run with `WALL`, each metric's peak and the average CPU; `skipped` marks runs left out of `FINAL` |
| `FINAL` | the smallest, largest and average peak over the counted runs, and `SPREAD` = max − min |
| legend line | the one-line reminder under every table; `psm help track` has the long form |

What sampling cannot see: a process that starts and exits between two
samples, and a peak between two samples. `--every 100ms` tightens
both at the cost of a full `/proc` pass ten times a second. The
per-process `VmHWM` high-water mark would give the true per-process
peak but does not sum across a tree at one instant, so psm does not
use it.
