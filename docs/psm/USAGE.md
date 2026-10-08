# Using psm

`psm` (process snapshot manager) records the Linux process table at
moments you choose and shows what changed between them.

## Quick answers

| Question | Command | Example |
|---|---|---|
| What is using the machine right now? | `psm info` (live; nothing is stored) | [info](#psm-info-n) |
| Which application uses the most memory, helpers included? | `psm procs --group app` | [grouping](#grouping) |
| What changed since I started? | `psm diff` (baseline -> now; `psm new` takes the baseline) | [a typical session](#a-typical-session) |
| What changed since my last snapshot? | `psm diff prev` | [comparing](#comparing) |
| Is it still growing? | `psm report trend` | [trend](#is-it-still-growing) |
| Who moved memory the most? | `psm diff --memory` | [memory impact](#memory-impact) |
| Memory went down but no process grew. Where did it go? | `psm report meminfo` | [reports](#reports) |
| What is this process, and where does it come from? | `psm procs --name X --group exe` (then `parent`, `cmdline`) | [example](#what-is-this-process-and-where-does-it-come-from) |
| Which part of it grew? | `psm diff --name X --group parent --memory` | [example](#which-part-of-it-grew) |
| How did one application change? | `psm diff --name X --memory` | [example](#how-did-one-application-change) |
| Is it still growing? | `psm report timeline --name X` | [example](#is-it-still-growing) |
| Which of its processes is busy right now? | `psm procs --name X --sort cpu --top 3` | [example](#which-of-its-processes-is-busy-right-now) |
| Did the new version use more memory? | `psm sessions compare old new --name X` | [example](#did-the-new-version-use-more-memory) |

`X` is any part of a process name, for example `chrome`.

This is the short version. [FAQ.md](FAQ.md) has the full list and `psm faq` prints
the same list at the command line.

## Contents

- [Concepts](#concepts)
- [A typical session](#a-typical-session)
- [Live views: `list`, `pid`, `info`, `--watch`](#live-views)
- [Capturing: `new`, `snap`](#capturing)
- [Comparing: `diff`](#comparing)
- [Reports](#reports)
- [Use cases: following one program with `--name`](#use-cases-following-one-program-with---name)
- [Sessions and housekeeping](#sessions-and-housekeeping)
- [Options shared by many commands](#shared-options)
- [Reading the numbers](#reading-the-numbers)
- [Output formats](#output-formats)
- [Configuration](#configuration)
- [Privileges](#privileges)
- [Limitations](#limitations)

## Concepts

**Session.** A named experiment, for example `chrome-update`. It holds
snapshots. One session is *active*: `psm snap` adds to it and
`psm diff` compares it with the live state. Every other session is
*inactive*: kept, and one `psm sessions activate <name>` away from being active
again. Starting a new session makes the previous one inactive.

**Snapshot.** Every process at one moment, plus system memory figures.
The first snapshot of a session is labelled `baseline`.

**Reference.** How you name a snapshot on the command line:

| Reference | Meaning |
|---|---|
| `baseline` | first snapshot of the session |
| `base` | the same, unless a snapshot is labelled `base` |
| `latest` | newest snapshot of the session |
| `prev` | the snapshot before the one it is compared with |
| `now` | the live state; collected for the command, never stored |
| `2` | a snapshot number within the session: `0` is the baseline, then `1`, `2`, ... (see `psm snapshots`) |
| `after-update` | a label; the newest snapshot carrying it |

`baseline`, `latest`, `prev` and `now` cannot be used as labels. A
number is tried as a snapshot number before it is tried as a label.

## A typical session

Once, after installing:

```bash
psm init          # config file, database, shell completions; safe to repeat
```

```text
Config:       /home/alice/.config/psm/config.toml (created)
Database:     /home/alice/.local/share/psm/psm.db (created)
Completions:  bash -> /home/alice/.local/share/bash-completion/completions/psm (written)
              Open a new shell to use them.
```

Then:

```bash
psm new code-upgrade         # start a session, take the baseline

# ... upgrade, restart, use the application ...

psm diff                      # baseline -> now; stores nothing
psm snap after-upgrade        # keep this state
psm diff prev                 # what changed since that snapshot
```

`psm diff` against the fixture data used by the tests:

```text
DIFF   #0 baseline -> now
Processes: 5 -> 5    new 2, gone 2, restarted 1

NEW PROCESSES
PID  PPID     RSS  PROCESS
600   100  72 MiB  node
400     1   7 MiB  beta

GONE PROCESSES
PID     RSS  PROCESS
200  42 MiB  old-helper
400   5 MiB  alpha

RESTARTED PROCESSES
OLD PID  NEW PID   BEFORE    AFTER     DELTA  PROCESS
    300      310  812 MiB  344 MiB  -468 MiB  rust-analyzer

MEMORY IMPACT   #0 baseline -> now   (RSS + swap)
PROCESS        PID  STATUS       BEFORE     AFTER     DELTA
rust-analyzer  310  restarted   812 MiB   344 MiB  -468 MiB
code           100  running    1000 MiB  1.37 GiB  +400 MiB
node           600  new               -    72 MiB   +72 MiB
old-helper     200  gone         42 MiB         -   -42 MiB
beta           400  new               -     7 MiB    +7 MiB
alpha          400  gone          5 MiB         -    -5 MiB
Net process change: -36 MiB

PROCESS COUNTS
PROGRAM     BEFORE  AFTER  DELTA
beta             0      1     +1
node             0      1     +1
alpha            1      0     -1
old-helper       1      0     -1

TOP 5 MEMORY IMPACT   #0 baseline -> now

By process   (RSS + swap)
PROCESS        PID  STATUS       BEFORE     AFTER     DELTA
rust-analyzer  310  restarted   812 MiB   344 MiB  -468 MiB
code           100  running    1000 MiB  1.37 GiB  +400 MiB
node           600  new               -    72 MiB   +72 MiB
old-helper     200  gone         42 MiB         -   -42 MiB
beta           400  new               -     7 MiB    +7 MiB

By program   (RSS + swap)
PROGRAM        #BEFORE  #AFTER    BEFORE     AFTER     DELTA
rust-analyzer        1       1   812 MiB   344 MiB  -468 MiB
code                 1       1  1000 MiB  1.37 GiB  +400 MiB
node                 0       1         -    72 MiB   +72 MiB
old-helper           1       0    42 MiB         -   -42 MiB
beta                 0       1         -     7 MiB    +7 MiB

Net process change: -36 MiB    System memory used: 4.56 GiB -> 5.51 GiB (+977 MiB)
```

The diff ends with a digest, because a long diff scrolls past: the five
largest memory movers by process and by program, the net change, and
how system memory moved. The by-program table is the one to read when
an application restarted: its old processes show as gone and its new
ones as new, and they cancel out only when summed per program.

PID 400 appears as both gone and new: the PID was reused by a different
program, and `psm` tells instances apart by PID plus start time.

## Live views

These read the machine as it is now. They need no session and never
touch the database.

### `psm procs`

```bash
psm procs                          # every process, largest memory first
psm procs --sort cpu --top 10
psm procs --group name             # one row per program
psm procs --group cgroup           # one row per application (systemd)
psm procs --user "$USER" --name chrome
psm procs chrome                   # any word: pid, name, path or command line
psm procs tsserver node            # every word must match
```

```text
PID  PPID  USER    %CPU  %MEM       RSS  THR  COMMAND
100     1  alice    6.1   6.1  1000 MiB   18  code
300     1  alice    0.0   5.0   812 MiB   24  rust-analyzer
200     1  alice    0.0   0.3    42 MiB    1  old-helper
```

Grouped by cgroup, with the cgroup's own memory total next to summed RSS:

```text
CGROUP          COUNT  %CPU  %MEM  CGROUP MEM       RSS  THR
app-code.scope      1   6.1   6.1    1.17 GiB  1000 MiB   18
session.scope       3   0.0   5.2     900 MiB   859 MiB   26
odd.service         1   0.0   0.1         n/a    20 MiB    1
```

| Option | Effect |
|---|---|
| `--group <key>` | one row per group instead of per process; see [Grouping](#grouping) |
| `--sort <column>` | `mem` (default), `cpu`, `threads`, `swap`, `io`, `pid`, `name`, `count` |
| `--top <n>` | only the first n rows |
| `[words]` | keep processes where every word appears in the pid, name, executable path or command line, ignoring case (`--match-case` to respect it); `psm procs show <ref> <words>` takes the reference first. `list` and `show` go right after `procs` (after an option they would read as words, so `psm` refuses them there); to search for those words use `--name` |

Every per-process row shows `PID` and `PPID` (the parent's pid), so
"who started it" is one lookup away: `--group parent` names the
parents, with their pid in its own column.
| `--interval <duration>` | CPU sampling window, default `500ms`; `0` = lifetime average |
| `--deep` | add PSS and USS columns |
| `--csv` | CSV instead of a table |

The `SWAP` and `IO` columns appear when you sort by them.

### `psm pid <pid> [ref]`

One process on one screen, from the live state (`now`, the default) or
from a stored snapshot (`psm pid 600 baseline`): what it is, where it
comes from, what it holds, and then its row in every snapshot of the
active session where the same instance (pid and start time, same boot)
appears.

```bash
psm pid 600
```

```text
PID 600  node   state S   user alice (1000)   nice 0   threads 11
Exe:      /usr/bin/node
Cmdline:  node server.js
Chain:    1 systemd > 100 code > 600 node   app: code
Cgroup:   /user.slice/app-code.scope
Age:      1h 2m   (as of now)
Memory:   RSS 72 MiB (anon 60 MiB, file 12 MiB, shmem 0 B)   swap 0 B   VSZ 288 MiB
CPU:      3s total (3s user, 0s system)   0.1 % of one core over its life   (psm procs shows the last 500ms)
I/O:      read 2 MiB   written 5 MiB

In session chrome-154:
ID  LABEL     TIME                    RSS  SWAP  THR  %CPU
 1  after     2026-09-30 09:40:02  70 MiB   0 B   11   0.1
 2  one-hour  2026-09-30 10:41:17  72 MiB   0 B   11   0.1
```

`Chain` is every ancestor up to init; `app` is where `--group app`
would file it. `Age` is as of the snapshot shown. `CPU` is the time the
process has consumed so far (what `ps -o time` shows), split into its
own code and the kernel working for it, and then that total as an
average rate over its life; the live `%CPU` of `psm procs` is the last
500 ms instead. `--json` has `total_seconds`, `user_seconds`,
`system_seconds` and `lifetime_percent`. A `--deep` snapshot adds a
`PSS`/`USS` line. `--json` gives the same fields and the history
as an array; `--watch` repeats the live card. A pid that is not in the
chosen snapshot is an error; `psm pid <pid> latest` looks in the last
stored one, for a process that has already exited.

### `psm info [N]`

Live view, like `procs`: system memory information, then the top N
(default 5) per metric. Nothing is stored; `psm snap` keeps a state. One collection
pass feeds every table, so they describe the same moment.

```bash
psm info
psm info 10 --by cpu,mem
psm info --top 10                 # the same as the number; wins when both are given
psm info --group name
```

```text
Memory:     4.56 GiB used / 16.00 GiB    available 11.44 GiB
Swap:       0 B used / 4.00 GiB
Load:       1.24  0.98  0.87
Processes:  5    Threads: 45    (1 kernel thread hidden)

TOP 3 BY CPU
PID  PPID  USER    %CPU  %MEM       RSS  THR  COMMAND
100     1  alice    6.1   6.1  1000 MiB   18  code
...
```

`--by` takes `cpu`, `mem`, `threads` (the default three), `swap`, `io`.

### Watching

`--watch [duration]` repeats a live view until Ctrl-C, clearing the
screen each round; the default is every 10 seconds, a bare number is
seconds. It is an option of the live views only: `procs`, `info`,
`pid`, `procs show`, `diff` and `report`. With a stored snapshot (`pid
<pid> latest`, `diff 0 1`) it is refused, there is nothing new to see;
other commands do not have the option at all, so completion never
offers it. Like `watch(1)` and `top`, it runs on the
terminal's alternate screen: old rounds never reach the scrollback, and
Ctrl-C brings the shell's screen back as it was. A footer, `Every 10s,
round 3, next refresh in 7s, Ctrl-C stops`, counts down in place under
the output. With `--json` the screen is not touched and nothing counts
down: one document per round.

```bash
psm info --watch              # every 10s
psm procs --sort cpu --top 10 --watch 2
psm diff --memory --watch 30  # baseline -> now, again every 30s
```
`used` is `MemTotal - MemAvailable`, not a sum of process memory.

## Capturing

| Command | Effect |
|---|---|
| `psm new [name]` | Start a new session, which becomes the active one (the previous one becomes inactive), and take its baseline. Without a name: `session-YYYYMMDD-HHMMSS`. Names are unique. The first time, it also creates the [config file](#configuration). |
| `psm snap [label]` | Add a snapshot to the active session, then print the five largest memory changes since the previous one. |

Both accept:

- `--deep`: also collect PSS and USS. Slower; see [Privileges](#privileges).
- `--no-cmdline`: do not store command lines. They often contain tokens
  and passwords, and the database keeps them.

## Comparing

```bash
psm diff [a] [b]
```

A second argument is a free-text description, kept with the snapshot and
shown in `psm list` and in headers: `psm snap "after update" "chrome 154,
extensions off"` gives `#1 after update (chrome 154, extensions off)`.
`psm snapshots rename <ref> <label> [description]` changes both later.
Sessions take a description the same way: `psm new chrome-154 "after the
update"`, changed with `psm sessions rename`. Both show in the lists
(last column) and in `psm status`.

| Command | Compares |
|---|---|
| `psm diff` | baseline -> now |
| `psm diff prev` or `psm diff latest` | last snapshot -> now |
| `psm diff baseline latest` | first snapshot -> last snapshot |
| `psm diff prev latest` | the last two snapshots |
| `psm diff 0 2` | two snapshots by number; `0` is the baseline |

With no target, the target is `now`.

Without section flags you get everything: summary, new, gone,
restarted, memory impact, the programs whose process count changed, and
a closing top-5 digest. `--new`, `--gone`, `--restarted` and `--memory`
pick sections; a single section has no digest.

`--brief` is the whole diff on one line, for a CI log, a commit message
or a chat; the section flags are ignored with it:

```bash
psm diff --brief
```

```text
#0 baseline -> now: 263 -> 265 processes, new 3, gone 1, restarted 0, net +512 MiB (RSS + swap); top: code +400 MiB, chrome -120 MiB, node +72 MiB
```

The movers are the largest absolute changes above `--min-delta`, three
of them unless `--top` says otherwise; `--group`, `--metric` and the
filters apply. With `--json` it is one small object (`from`, `to`,
`processes`, `metric`, `net_change`, `top`). Across a reboot the line
says `different boots: programs compared`.

### Memory impact

`psm diff --memory` is one ranking over **all** processes, largest
absolute change first:

| Status | Counts as |
|---|---|
| running | after - before |
| restarted | new instance - old instance |
| new | + its whole memory |
| gone | - its whole memory |

| Option | Effect |
|---|---|
| `--metric total` | RSS + swap (default) |
| `--metric anon` | anonymous RSS + swap: the leak-hunting view |
| `--metric pss` | PSS + swap; both snapshots must be `--deep` |
| `--min-delta <size>` | hide smaller changes; default `1M` |
| `--group <key>` | one row per group instead of per process |
| `--top <n>` | first n rows of each table |

Grouped by application:

```text
MEMORY IMPACT   #0 baseline -> #1 after-upgrade   (cgroup memory.current + swap)
CGROUP          #BEFORE  #AFTER    BEFORE     AFTER     DELTA
app-code.scope        1       1  1.17 GiB  1.66 GiB  +500 MiB
session.scope         3       3   900 MiB   500 MiB  -400 MiB
Net group change: +100 MiB
```

The header always names the metric that was used.

When the two snapshots come from different boots, every process is new
by definition. `psm` says so and compares programs instead of instances.

## Reports

```bash
psm report <kind> [a] [b]
```

References and defaults are the same as for `diff`.

| Kind | Shows |
|---|---|
| `memory` | the memory impact ranking (same as `diff --memory`) |
| `growth` | programs that grew, with the relative change |
| `processes` | process count per program, changed or not |
| `new`, `gone` | those sections only |
| `cpu` | CPU seconds used between the two snapshots, per program |
| `meminfo` | every `/proc/meminfo` field that changed |
| `timeline` | one row per snapshot of the session |
| `trend` | every program across all snapshots of the session plus `now`: growing, shrinking, noisy or flat |

`timeline` and `trend` cover the whole session and take no references.

`meminfo` answers the case process data cannot: system memory went
down, but no process grew (tmpfs, page cache, kernel slab).

```text
MEMINFO   #0 baseline -> #1 after-upgrade
FIELD               BEFORE      AFTER     DELTA
MemAvailable     11.44 GiB  10.49 GiB  -977 MiB
Shmem               98 MiB    586 MiB  +488 MiB
SwapFree          4.00 GiB   3.99 GiB   -10 MiB
HugePages_Total          0          2        +2
```

`timeline` with a filter adds what the filter matched in each snapshot:

```bash
psm report timeline --name code
```

```text
ID  LABEL          TIME                 PROCS      USED    SWAP  MATCHED RSS+SWAP
 0  baseline       2026-09-30 09:12:31      1  4.56 GiB     0 B          1000 MiB
 1  after-upgrade  2026-09-30 09:12:31      1  5.51 GiB  10 MiB          1.37 GiB
```

`report cpu` counts processes still present in the second snapshot; a
process that exited took its final counters with it.

## Use cases: following one program with `--name`

`--name <text>` keeps the processes whose name **contains** the text,
whatever the case: `--name chrome` also keeps `chrome_crashpad`, and
`--name main` keeps `MainThread` (`--match-case` to insist on `Main`).
It works on
`list`, `info`, `show`, `diff`, `report` and `compare`, and combines
with `--group`, `--sort`, `--top` and the other filters.

The numbers below are made up for illustration; the layout is what
`psm` prints.

### What is this process, and where does it come from?

A diff shows a program called `MainThread` that grew by 1.5 GiB, and the
name says nothing. For one pid, `psm pid 4038` answers on one screen:
binary, command line, parent chain, application, cgroup. For the whole
family, group the same processes three ways.

Which binary is it?

```bash
psm procs --name MainThread --group exe
```

```text
PROGRAM        COUNT  %CPU  %MEM       RSS  THR
/usr/bin/node     26   1.2   3.2  4.05 GiB  270
```

It is Node.js, which names its main thread `MainThread`. Who started
the 26 of them?

```bash
psm procs --name MainThread --group parent
```

```text
PPID  PARENT      COUNT  %CPU  %MEM       RSS  THR
4033  zed-editor     19   0.8   1.5  1.86 GiB  173
4038  MainThread      4   0.3   1.2  1.49 GiB   36
1256  sh              1   0.1   0.4   563 MiB   47
2642  MainThread      2   0.0   0.2   197 MiB   14
```

Mostly the editor, but four have another `MainThread` as parent: Node
processes started by Node processes. `--group parent` shows only the
direct parent. To roll every process up to the application at the top
of its chain:

```bash
psm procs --name MainThread --group app
```

```text
APP         COUNT  %CPU  %MEM       RSS  THR
zed-editor     25   1.1   2.8  3.49 GiB  223
just            1   0.1   0.4   563 MiB   47
```

25 of the 26 belong to the editor; one was started from a terminal by
`just`. Which scripts are they running?

```bash
psm procs --name MainThread --group cmdline --top 4
```

```text
PROGRAM                                     COUNT  %CPU  %MEM       RSS  THR
node /opt/lsp/typescript/tsserver.js            4   0.3   1.2  1.49 GiB   36
node node_modules/.bin/vite build --watch       1   0.1   0.4   563 MiB   47
node /opt/lsp/typescript/server.js --stdio      2   0.2   0.3   412 MiB   28
node /opt/lsp/json/server.js --stdio            3   0.0   0.2   268 MiB   21
```

`--group name` is a poor key for interpreters: every Node program is
`MainThread`, every Python script is `python3`. `--group exe` names the
binary and `--group cmdline` separates the actual programs.

### Which part of it grew?

The same filter on a diff. Who is responsible for the growth since the
baseline?

```bash
psm diff --name MainThread --group parent --memory
```

```text
MEMORY IMPACT   #0 baseline -> now   (RSS + swap)
PARENT           #BEFORE  #AFTER    BEFORE     AFTER     DELTA
4033 zed-editor       12      19  1.10 GiB  1.86 GiB  +778 MiB
4038 MainThread        2       4   749 MiB  1.49 GiB  +777 MiB
1256 sh                1       1   560 MiB   563 MiB    +3 MiB
Net group change: +1.52 GiB
```

The editor started seven more Node processes, and one Node process
doubled its children. The `vite` watcher started from a shell is flat.

### How did one application change?

```bash
psm diff --name chrome --memory
```

```text
MEMORY IMPACT   #0 baseline -> now   (RSS + swap)
PROCESS      PID  STATUS     BEFORE     AFTER     DELTA
chrome   4042574  running   361 MiB   634 MiB  +273 MiB
chrome    240568  new             -   188 MiB  +188 MiB
chrome    228440  gone       82 MiB         -   -82 MiB
chrome   4041135  running  1.55 GiB  1.58 GiB   +31 MiB
Net process change: +410 MiB
```

One tab process grew by 273 MiB, one was opened, one was closed. Just
the process counts:

```bash
psm report processes --name chrome
```

```text
PROCESS COUNTS
PROGRAM          BEFORE  AFTER  DELTA
chrome               40     47     +7
chrome_crashpad       2      2      0
```

### Is it still growing?

Take snapshots at rest over a while, then ask for every program at once:

```bash
psm report trend
```

```text
TREND   session chrome-154: 5 snapshots + now over 2.3 h   (RSS + swap)
PROGRAM          FIRST      LAST     DELTA  UP    SLOPE/H  VERDICT
chrome        3.42 GiB  3.91 GiB  +500 MiB  5/5  +210 MiB  growing
code          1.91 GiB  2.01 GiB  +100 MiB  3/5   +38 MiB  noisy
rust-analyzer  812 MiB   640 MiB  -172 MiB  1/5   -70 MiB  shrinking
node            72 MiB    73 MiB    +1 MiB  2/5      0 B   flat
```

One row per program (`--group` for another key), over every snapshot
of the session with `now` as the last point. `UP` is how many steps
rose out of all steps; `SLOPE/H` is a least-squares line through all
points, in bytes per hour (`n/a` when the points are less than a
minute apart). The verdict uses `--min-delta` (default `1M`): `flat`
when the end-to-end change is below it, `growing` or `shrinking` when
no step of that size went the other way, `noisy` otherwise. A `growing`
row with `UP 5/5` across snapshots taken at rest is what a leak looks
like; `--metric anon` sharpens it to memory the program allocated
itself. Largest change first, `flat` rows last; `--name`, `--top` and
the other filters apply, and `--watch` repeats it with a fresh `now`.

The raw per-snapshot values are `report timeline --name chrome`: one
row per snapshot with the total of the matching processes in the last
column.

### Which of its processes is busy right now?

```bash
psm procs --name chrome --sort cpu --top 3
```

```text
    PID     PPID  USER   %CPU  %MEM       RSS  THR  COMMAND
4041081     3749  alice  17.6   1.3  1.65 GiB   40  chrome
4041135  4041108  alice   3.9   1.3  1.58 GiB   12  chrome
4042574  4041108  alice   2.0   0.5   634 MiB   24  chrome
```

### Did the new version use more memory?

Two sessions, one per version, narrowed to the application:

```bash
psm sessions compare chrome-153 chrome-154 --name chrome
```

```text
COMPARE   chrome-153 (#5 after-1-hour) -> chrome-154 (#9 after-1-hour)

METRIC          OLD       NEW     DELTA
Processes        27        30        +3
Total RSS  3.42 GiB  3.81 GiB  +399 MiB
Swap            0 B       0 B       0 B
Threads         412       438       +26

BY PROGRAM   (RSS + swap)
PROGRAM  #BEFORE  #AFTER    BEFORE     AFTER     DELTA
chrome        26      29  3.41 GiB  3.80 GiB  +399 MiB
```

### Narrowing further

| You want | Add |
|---|---|
| only your own processes | `--user "$USER"` |
| one exact binary, not a name fragment | `--exe /opt/google/chrome/chrome` |
| one or two known pids | `--pid 4041081,4041135` |
| the processes running one script | `--cmdline tsserver.js` |
| everything except one helper | `--exclude-regex '^chrome_crashpad'` |
| the result for a script | `--json` |

```bash
psm procs --name chrome --exclude-regex '^chrome_crashpad' --user "$USER"
psm diff --name node --group cmdline --memory --json
```

`--exclude-regex` is tested against the process name **and** the
command line. A loose pattern removes more than intended: every Chrome
process carries `--crashpad-handler-pid=...` in its command line, so
`--exclude-regex crashpad` drops nearly all of Chrome. Anchor the
pattern (`^chrome_crashpad`) to match the name only.

## Sessions and housekeeping

| Command | Effect |
|---|---|
| `psm` / `psm status` | summary of the active session |
| `psm sessions` | all sessions, with their state: active or inactive (also `psm sessions list`) |
| `psm sessions activate <name\|id>` / `psm sessions deactivate` | make another session the active one; the current one becomes inactive / leave none active |
| `psm list` / `psm snapshots` | snapshots of the session (also `psm snapshots list`); the last row, `>` `now`, is the live state |
| `psm procs show [ref]` | the processes of one stored snapshot (`procs` shows the live ones); same options; default `latest` |
| `psm sessions compare <a> <b>` | two sessions, by program, using the latest snapshot of each |
| `psm sessions export --format json\|csv` | dump a session; `--all` dumps every session (JSON); `--no-cmdline` leaves command lines out |
| `psm sessions import <file>` | load a JSON dump (one session or `--all`) as new, inactive sessions; `--name` renames a single one |
| `psm export [ref] --format json\|csv` | dump one snapshot, default `latest`; `--all` dumps the whole active session (same as `psm sessions export`) |
| `psm import <file>` | add every snapshot of an export file to the active session; labels and timestamps are kept |
| `psm sessions deactivate` | make the active session inactive, leaving none active; nothing is deleted |
| `psm sessions purge --older-than 180d` | delete inactive sessions created before that; never the active one |
| `psm snapshots reset` | start the active session over: all its snapshots go, the live state is the new baseline; asks first |
| `psm sessions delete <name\|id>` | delete one session and its snapshots |
| `psm snapshots delete [ref]` | delete one snapshot, default `latest`; the others keep their numbers. The last one is replaced by a fresh baseline |
| `psm snapshots rename <ref> <label> [description]` | new label, and a new description when given (`""` clears it) |
| `psm sessions rename <name\|id> <new> [description]` | new session name (the old one is fine), and a new description when given (`""` clears it); names stay unique |
| `psm backup <path>` | consistent copy of the database; refuses to overwrite |
| `psm sessions reset` | delete **everything**: all sessions and snapshots; asks first |
| `psm faq [words]` | this guide's quick-answers table, at the command line; words filter it |
| `psm init` | first-time setup: creates the config file and database if missing, installs shell completions; safe to repeat. `psm init force` deletes both and starts from scratch (asks first) |
| `psm update` | after installing a new psm: database schema upgraded in place (copy kept), config checked, completions refreshed |
| `psm config` / `psm --config` | show which config file is used; `--init` creates it with the defaults |
| `psm completions <shell>` | print a shell completion script; see [Tab completion](#tab-completion) |

The daily loop is top level: `new`, `snap`, `diff`, `report`, `status`,
`procs`, `info`, plus `export`/`import` for moving snapshots around.
Everything else about sessions is under `psm sessions`: `list` (the
default), `activate`, `deactivate`, `compare`, `export`, `import`,
`delete`, `purge` and `reset` (the whole database). `psm snapshots`
lists the session's snapshots (`list`, the default) and holds `delete`
and `reset` (the session starts over). `--help` on any group lists its
commands.

### Switching sessions

```bash
psm sessions
```

```text
ID  NAME        CREATED              SNAPS  STATE     DESCRIPTION
 1  chrome-153  2026-09-28 14:17:02      5  inactive
 2  chrome-154  2026-09-30 08:42:10      4  active    after the update
```

```bash
psm sessions activate chrome-153   # by name, or `psm sessions activate 1` by id
```

```text
Session "chrome-153" is now active.
Session "chrome-154" is now inactive.
```

From then on `psm snap` adds to `chrome-153` and `psm diff` compares
its baseline with the live state. Switching deletes nothing and can be
repeated freely.

Every command works on the active session, so to look at another one,
switch to it (and back). `psm sessions compare` and `psm sessions export`
take session names directly:

```bash
psm sessions compare chrome-153 chrome-154
psm sessions export chrome-153 > old.json
```

### Export and import

Both live under `psm sessions`:

```bash
psm sessions export > chrome-154.json          # the active session, as JSON
psm sessions export chrome-153 > old.json      # another session, by name or id
psm sessions import chrome-153.json                    # on another machine or database
psm sessions import chrome-153.json --name chrome-old  # when the name is already taken
psm sessions export | psm --db other.db session import -       # `-` reads standard input
psm sessions export --all > all.json           # every session in one file
psm sessions import all.json                   # ... and back, all at once
```

An imported session keeps its snapshots, labels and original
timestamps. It arrives **inactive**, so it does not replace your active
session: make it active with `psm sessions activate <name>`, or put it next to
another session with `psm sessions compare`. Snapshot numbers start at `0`
again.

One snapshot at a time goes through `psm export` and `psm import`:

```bash
psm export > latest.json             # the newest snapshot of the active session
psm export after-update > s.json     # by label or number
psm import s.json                    # appended to the active session
psm sessions activate old; psm import s.json  # ... or to another one
```

The file is the session export format with one snapshot, so `psm
sessions import` also accepts it (as a session of its own) and `psm
import` also accepts a whole session export (every snapshot in it is
appended). An imported baseline arrives as a plain snapshot: the
session keeps the baseline it has.

Only the JSON export can be imported. CSV is one row per process, for
spreadsheets, and leaves out the system figures.

An export made with `--no-cmdline` imports without command lines.

`--all` writes every session, active and inactive, into one JSON file.
`psm sessions import` recognises that file and loads all of them in one
transaction: if any name already exists, nothing is imported. `--name`
only applies to a single-session file. `--all` is JSON only; the CSV
format has no session column.

### Deleting data

Five commands delete, from narrow to total:

| Command | Deletes | Asks first |
|---|---|---|
| `psm snapshots delete [ref]` | one snapshot, default the latest; the last one is replaced by a fresh baseline | no |
| `psm snapshots reset` | every snapshot of the active session, which then starts over with a new baseline | **yes** |
| `psm sessions delete <name\|id>` | one session | no |
| `psm sessions purge --older-than <age>` | inactive sessions older than that | no |
| `psm sessions reset` | the whole database: every session and snapshot | **yes** |

`psm sessions reset` shows what is about to go and waits for an answer:

```text
$ psm sessions reset
This permanently deletes 3 session(s) and 11 snapshot(s) in /home/alice/.local/share/psm/psm.db.
`psm backup <path>` makes a copy first.
Delete everything? [y/N]
```

Only `y` or `yes` deletes. Anything else, including no answer at all in
a script, deletes nothing. `psm sessions reset --yes` skips the question.

The configuration file is kept. Afterwards there is no session; start
one with `psm new`.

`psm sessions reset` also works on a database this version cannot read (written
by a different schema version), which is the way to start over in that
case.

## Shared options

### Help

Every command explains its own options:

```bash
psm faq                # common questions and the command that answers each
psm faq memory         # only the rows mentioning memory
psm --help             # the commands, and each global option explained with an example
psm -h                 # the same on fewer lines
psm diff --help   # options of one command, each value explained, examples
psm diff -h       # the same on fewer lines
psm help snapshots diff     # the same as psm diff --help; psm help session export for a subcommand
psm version                 # name, version, build date and commit; also psm -v, psm --version
```

`help`, `-h` and `--help` are one command: `psm -h snapshots diff` is
`psm help snapshots diff` on fewer lines.

Global options (`--config`, `--db`, `--kernel`, `--json`, `--proc-root`)
work with every command and can go before or after it: `psm --json diff`
and `psm diff --json` are the same. They are listed by `psm -h`; a
command's own help lists only its options.

Commands are words; some also have a single-dash letter, shown next to
them in `psm -h` (`snap, -s`): `psm -s` is `psm snap`, options follow as
usual (`psm -p --top 10`, `psm -d prev --memory`). Double dashes are
options only; the sole exceptions are `--help` and `--version`, which
every tool honours. Letters: `-p` procs, `-i` info, `-n` new, `-s` snap,
`-l` list, `-d` diff, `-b` backup, `-q` faq, `-h` help, `-v` version;
`psm <command> -h` shows a group's own letters: `-l` for every `list`
and `-r` for `snapshots reset`.

### Tab completion

`psm init` installs completion for the shell named by `$SHELL` (bash,
zsh or fish; `--shell` picks one) and refreshes it on every run, so
running `psm init` after an upgrade keeps the script current.

Underneath, `psm completions <shell>` prints a completion script
generated from the same definitions as `--help`, so commands, options
and fixed values (`--group`, `--sort`, `--metric`, report kinds) all
complete. To install it by hand, or for another shell:

```bash
psm completions bash > ~/.local/share/bash-completion/completions/psm
psm completions zsh  > ~/.zfunc/_psm            # with ~/.zfunc in fpath
psm completions fish > ~/.config/fish/completions/psm.fish
```

```text
psm di<Tab>              diff
psm sessions <Tab>       list  purge  compare  export  import  delete
psm procs --group <Tab>   name  exe  cmdline  app  user  cgroup  parent  pid
```

Session names and snapshot labels are not completed: they live in the
database, and the script is static. It lists the commands of the
version that wrote it, so regenerate it (`psm init`) after upgrading.

A value that is not accepted is rejected with the list of valid ones:

```text
error: invalid value 'comm' for '--group <KEY>'
  [possible values: name, exe, cmdline, user, cgroup, parent, pid]
```

### Grouping

`--group <key>` is accepted by `list`, `info`, `show`, `diff`, `report`
and `compare`. It gives one row per group, with the process count and
the memory, CPU and threads summed.

| Key | Groups by | Useful when |
|---|---|---|
| `name` | program name | the usual choice: all `chrome` processes become one row |
| `exe` | full path of the executable | two programs share a name |
| `cmdline` | full command line | telling apart scripts that all run as `python3` or `node` |
| `app` | the application a process belongs to: its top-level ancestor | you want an editor or browser together with everything it started |
| `cgroup` | systemd application or service | services, containers, and desktops that start each app in its own cgroup |
| `user` | owner | seeing who uses the memory |
| `parent` | parent process | seeing what a launcher spawned |
| `pid` | nothing | switching off a `group` default from the config file |

The program name is the kernel's short name for the process (at most 15
characters), the same one `top` shows.

`app` walks up the parent chain and stops at the highest ancestor that
is an actual program. Shells, terminals, login and session managers,
the desktop shell and sandbox wrappers are skipped, because they start
applications without being one:

```text
systemd -> bash -> zed-editor -> node -> node -> rust-analyzer     app: zed-editor
systemd -> lightdm -> cinnamon -> chrome -> chrome                 app: chrome
systemd -> gnome-terminal -> bash -> just -> npm -> node           app: just
```

So a language server three levels below the editor counts towards the
editor, and a command typed in a terminal is its own application. A
process with only launchers above it (a shell, the desktop itself) is
its own application.

The list of launchers is built in. If your desktop shell or terminal is
not on it, everything it started shows up as one large app named after
it.

In `diff`, grouping replaces the per-process New, Gone and Restarted
sections, which do not apply to groups.

### Filters and global options

Filters, accepted by `list`, `info`, `show`, `diff`, `report`, `compare`:

| Option | Keeps |
|---|---|
| `--user <name\|uid>` | processes of that user |
| `--name <text>` | processes whose name contains the text |
| `--exe <path>` | processes running exactly that executable |
| `--pid <n,...>` | those process ids |
| `--cmdline <text>` | processes whose command line contains the text |
| `--match-case` | `--name`, `--cmdline` and the search words match case exactly (by default they ignore it) |
| `--exclude-regex <regex>` | drops processes whose name or command line matches; `(?i)` at the start ignores case |

`--name` and `--cmdline` each look at one field, for scripts. The words
after `psm procs` are the loose form: one word may hit the name of one
process and the command line of another. `--exe` is a path and is
always exact.

Global options, accepted everywhere:

| Option | Effect |
|---|---|
| `--config [path]` | configuration file; also `PSM_CONFIG`. Alone, with no command: same as `psm config` |
| `--db [path]` | database file; also `PSM_DB`. Alone, with no command: shows the database in use and its schema version |
| `--kernel` | include kernel threads |
| `--json` | machine-readable output |
| `--proc-root <dir>` | read process data from a directory instead of `/proc` (tests) |

`--watch [duration]` belongs to the live views (`procs`, `pid`, `info`,
`diff`, `report`); see [Watching](#watching).

Sizes (`--min-delta`) accept `500K`, `10M`, `1G`; the base is 1024.
Durations (`--interval`, `--watch`, `--older-than`) accept `500ms`, `10s`, `5m`,
`2h`, `180d`.

## Reading the numbers

**Units** are binary and labelled KiB/MiB/GiB, matching `/proc`,
`free -h` and `top`.

**`%CPU`**: 100 is one fully used core, so a multi-threaded process or
a group can exceed 100. `list` and `info` take two readings
`--interval` apart. A process that started inside that window has an
empty `%CPU`. With `--interval 0`, and in `show`, the figure is the
lifetime average (CPU time divided by process age), like `ps`.

**`%MEM`** is RSS divided by total memory.

**RSS** summed over a group double-counts shared pages. Three ways
around that:

- `--deep` adds PSS, which divides shared pages between their users and
  sums correctly;
- `--group cgroup` shows the kernel's own total for the group
  (`memory.current`). It includes page cache charged to the group, so
  compare it with itself across snapshots, not with RSS;
- `--metric anon` looks only at memory the process allocated itself.

**Swap is part of every memory comparison.** Under memory pressure RSS
drops because pages were swapped out; counting RSS alone would report
that as freed memory.

**`n/a`** means not collected or not readable. It is never shown as
zero, and a value missing on either side of a comparison is left out of
the ranking rather than treated as a change.

**`-`** means not applicable: a new process has no "before".

**Restarted** means a gone and a new process with the same executable
and command line.

**Kernel threads** are hidden unless `--kernel` is given. They are
stored in snapshots either way.

## Output formats

On a terminal, every command that prints for a person opens with the
version line (`psm (process snapshot manager) 2.1.5 (built ..., commit
...)`). With `--json`, for `export`, `sessions export`, `completions`,
or when standard output is a pipe or a file, it is left out, so data
stays data. Export files carry the same facts in a `psm` block
(`version`, `built`, `commit`, `schema`); import ignores it.

- Text tables by default.
- `--json` on any command. Byte values are raw numbers.
- `--csv` on `list` and `show`.
- `psm sessions export` for a whole session.

```bash
psm diff --memory --top 1 --json
```

```json
{
  "from": { "id": 0, "label": "baseline" },
  "memory": [
    {
      "after": 360710144,
      "before": 851443712,
      "delta": -490733568,
      "pid": 310,
      "process": "rust-analyzer",
      "status": "restarted"
    }
  ],
  "metric": "RSS + swap",
  "net_change": -37748736,
  "to": { "id": null, "label": "now" }
}
```

Exit codes: `0` success, `2` usage error or failure. `1` is reserved
for thresholds, which do not exist yet. A diff that finds changes still
exits `0`.

### Sending output to a server

There is no built-in upload; the JSON goes to any HTTP endpoint with
`curl`, and the version line stays out because stdout is a pipe:

```bash
psm diff --json | curl -sS -X POST -H 'Content-Type: application/json' -d @- "$URL"
psm report trend --json | curl -sS -X POST -H "Authorization: Bearer $TOKEN" -d @- "$URL"
psm sessions export --format csv | curl -sS -X POST -H 'Content-Type: text/csv' --data-binary @- "$URL"
```

The export carries `hostname`, the session and the `psm` block, so
documents from several machines can be told apart; a `diff` or `report`
does not, so add them yourself (`-H "X-Host: $(hostname)"`). `psm snap`
prints its digest as text only; after a snapshot, send
`psm diff prev --json`. A `--watch --json` run prints one document per
round, so a `while read` loop over it posts each round as it comes.

## Configuration

Optional. Exactly one file is read, the first of:

1. `--config <path>`
2. `PSM_CONFIG`
3. `~/.config/psm/config.toml` (`$XDG_CONFIG_HOME/psm/config.toml`)

A path given explicitly must exist; the default file may be absent.
Unknown keys and wrong types are errors that name the file, key and
line.

```bash
psm init              # create it, with the database and shell completions
psm config            # which file is used, and whether it exists
psm config --init     # create only the config file
```

The first `psm new` also creates the file at the default location if
it is not there yet, and says so. The generated file holds every key at
its built-in default, so it changes nothing until you edit it. After
that it is yours: `psm` never rewrites or overwrites it. With
`--config` or `PSM_CONFIG`, `new` creates nothing.

```toml
database = "~/.local/share/psm/psm.db"

[collection]
io = true             # read /proc/<pid>/io
cgroups = true        # read cgroup membership and memory.current
cmdline = true        # store command lines          (--no-cmdline)
deep = false          # collect PSS/USS              (--deep)

[display]
units = "auto"        # auto, B, KiB, MiB, GiB
top = 30              #                              (--top)
group = "name"        #                              (--group; "pid" = none)
kernel = false        #                              (--kernel)
interval = "500ms"    #                              (--interval)

[diff]
metric = "total"      # total, anon, pss             (--metric)
min_memory_delta = "1M"   #                          (--min-delta)
```

Precedence, highest first: command-line flag, environment variable,
configuration file, built-in default.

Boolean flags take an explicit value with `=`, so a flag can switch off
what the file switched on: `psm snap --deep=false`,
`psm procs --kernel=false`.

The database is `~/.local/share/psm/psm.db`
(`$XDG_DATA_HOME/psm/psm.db`), created with mode `0600`.

## Privileges

Without root, for processes of **other users**:

| Readable | Not readable |
|---|---|
| name, state, RSS and its breakdown, swap, threads, CPU time, command line, cgroup | executable path, I/O counters, PSS/USS |

Those fields are stored as missing and shown as `n/a`. `list`, `new`
and `snap` print how many processes were affected:

```text
82 of 287 processes partially readable (run as root for full data)
```

`psm diff` warns when the two sides were collected with different
privileges, because `exe` and PSS values then appear or disappear
without having changed.

Grouping by `exe` falls back to the process name where the executable
path is not readable.

## Limitations

- Linux only.
- A process that starts **and** exits between two snapshots is never seen.
- A snapshot is not atomic: the process table is read over a few
  milliseconds.
- User names come from `/etc/passwd`; LDAP/NSS users show as a numeric uid.
- Programs started from a terminal share that terminal's cgroup, so
  `--group cgroup` cannot separate them.
- After installing a newer psm, run `psm update`: it upgrades the
  database in place (a copy is kept next to it), checks the config file
  and rewrites the completions. Until then an older database is refused
  with a message; a database from a newer psm is always refused.
- Not implemented: notes, tags, thresholds (`--fail-if-*`),
  HTML reports.
