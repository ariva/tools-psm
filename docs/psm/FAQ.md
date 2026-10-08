# psm FAQ

Questions and the command that answers each. `X` is any part of a
process name, for example `chrome`. The "More" column links to the
part of [USAGE.md](USAGE.md) that explains the command.

The same list is built into the tool: `psm faq` prints it, `psm faq
swap` shows only the rows containing "swap" (case-insensitive, every
word given must match), and `psm faq 5.3` picks one row by number.

## 1. Generic

| # | Question | Command | More |
|---|---|---|---|
| 1.1 | What is using the machine right now? | `psm info` (live; nothing is stored) | [info](USAGE.md#psm-info-n) |
| 1.2 | Which application uses the most memory, helpers included? | `psm procs --group app` | [grouping](USAGE.md#grouping) |
| 1.3 | What changed since I started? | `psm diff` (baseline -> now; `psm new` takes the baseline) | [a typical session](USAGE.md#a-typical-session) |
| 1.4 | What changed since my last snapshot? | `psm diff prev` | [comparing](USAGE.md#comparing) |
| 1.5 | Who moved memory the most? | `psm diff --memory` | [memory impact](USAGE.md#memory-impact) |
| 1.6 | Memory went down but no process grew. Where did it go? | `psm report meminfo` | [reports](USAGE.md#reports) |
| 1.7 | Did the new version use more memory? | `psm sessions compare old new --name X` | [example](USAGE.md#did-the-new-version-use-more-memory) |
| 1.8 | How do I keep this state for later? | `psm snap after-update` | [capturing](USAGE.md#capturing) |
| 1.9 | How do I start a new experiment? | `psm new chrome-update` | [capturing](USAGE.md#capturing) |
| 1.10 | How do I go back to an earlier experiment? | `psm sessions`, then `psm sessions activate <name>` | [switching sessions](USAGE.md#switching-sessions) |
| 1.11 | Which snapshots do I have? | `psm list` (the last row, `> now`, is the live state) | [sessions and housekeeping](USAGE.md#sessions-and-housekeeping) |

## 2. Finding things now

| # | Question | Command | More |
|---|---|---|---|
| 2.1 | Which processes are swapped out? | `psm procs --sort swap --top 10` | [list](USAGE.md#psm-procs) |
| 2.2 | Which processes have done the most disk I/O? | `psm procs --sort io --top 10` | [list](USAGE.md#psm-procs) |
| 2.3 | How many processes does each program run? | `psm procs --group name --sort count` | [grouping](USAGE.md#grouping) |
| 2.4 | How much memory does each user take? | `psm procs --group user` | [grouping](USAGE.md#grouping) |
| 2.5 | How much memory does each service or container take? | `psm procs --group cgroup` | [grouping](USAGE.md#grouping) |
| 2.6 | Which processes run this exact binary? | `psm procs --exe /opt/google/chrome/chrome` | [filters](USAGE.md#filters-and-global-options) |
| 2.7 | How do I keep a live view refreshing? | `psm info --watch` (every 10s; `--watch 30` for 30s; also `procs`, `diff`) | [watching](USAGE.md#watching) |

2. Finding things now
The I/O figure is bytes read and written since each process started,
not a rate.

## 3. One process

| # | Question | Command | More |
|---|---|---|---|
| 3.1 | What is this process, and where does it come from? | `psm pid 4041081` (one screen: exe, command line, parents, app, cgroup, memory) | [pid](USAGE.md#psm-pid-pid-ref) |
| 3.2 | Which binary, parent and scripts, for a whole family of processes? | `psm procs --name X --group exe` (then `parent`, `cmdline`) | [example](USAGE.md#what-is-this-process-and-where-does-it-come-from) |
| 3.3 | Who started this process? | `psm procs --name X --group parent` | [example](USAGE.md#what-is-this-process-and-where-does-it-come-from) |
| 3.4 | How do I find a process by any word I know about it? | `psm procs tsserver` (pid, name, path or command line) | [list](USAGE.md#psm-procs) |
| 3.5 | How do I look at one or two known pids? | `psm procs --pid 4041081,4041135` | [filters](USAGE.md#filters-and-global-options) |
| 3.6 | Which of its processes is busy right now? | `psm procs --name X --sort cpu --top 3` | [example](USAGE.md#which-of-its-processes-is-busy-right-now) |
| 3.7 | How did one application change? | `psm diff --name X --memory` | [example](USAGE.md#how-did-one-application-change) |
| 3.8 | Which part of an application grew? | `psm diff --name X --group parent --memory` | [example](USAGE.md#which-part-of-it-grew) |
| 3.9 | Is it still growing? | `psm report trend` (`--name X` for one program) | [example](USAGE.md#is-it-still-growing) |
| 3.10 | What did it look like earlier in the session? | `psm pid 4041081 baseline` (any snapshot reference) | [pid](USAGE.md#psm-pid-pid-ref) |
| 3.11 | It has exited; what was it? | `psm pid 4041081 latest` (or the number of a snapshot that has it) | [pid](USAGE.md#psm-pid-pid-ref) |

## 4. Comparing

| # | Question | Command | More |
|---|---|---|---|
| 4.1 | Which processes appeared? | `psm diff --new` | [comparing](USAGE.md#comparing) |
| 4.2 | Which processes disappeared? | `psm diff --gone` | [comparing](USAGE.md#comparing) |
| 4.3 | Which processes restarted? | `psm diff --restarted` | [comparing](USAGE.md#comparing) |
| 4.4 | Which programs grew, and by what percentage? | `psm report growth` | [reports](USAGE.md#reports) |
| 4.5 | Is it a real leak or just cache? | `psm diff --memory --metric anon` | [memory impact](USAGE.md#memory-impact) |
| 4.6 | Which program burned the most CPU since my last snapshot? | `psm report cpu prev` | [reports](USAGE.md#reports) |
| 4.7 | How do I compare two specific snapshots? | `psm diff 0 2` | [comparing](USAGE.md#comparing) |
| 4.8 | What was in a snapshot I took earlier? | `psm list`, then `psm procs show <n>` | [sessions and housekeeping](USAGE.md#sessions-and-housekeeping) |
| 4.9 | How do I get the diff as one line for a log or a commit message? | `psm diff --brief` | [comparing](USAGE.md#comparing) |

3. Comparing

## 5. Options and data

| # | Question | Command | More |
|---|---|---|---|
| 5.1 | How do I get exact numbers for a script? | add `--json` to any command; the result is under `data` | [the JSON document](USAGE.md#the-json-document) |
| 5.2 | How do I see PSS instead of RSS? | `psm procs --deep`, `psm snap --deep` | [reading the numbers](USAGE.md#reading-the-numbers) |
| 5.3 | Why is a value `n/a`? | other users' processes: run with `sudo` for full data | [privileges](USAGE.md#privileges) |
| 5.4 | How do I keep passwords out of the database? | `psm snap --no-cmdline` | [capturing](USAGE.md#capturing) |
| 5.5 | How do I include kernel threads? | add `--kernel` | [reading the numbers](USAGE.md#reading-the-numbers) |
| 5.6 | How do I leave noise out? | `--exclude-regex '^chrome_crashpad'` | [narrowing further](USAGE.md#narrowing-further) |
| 5.7 | How do I send a diff to a server? | `psm diff --json \| curl -d @- $URL` | [sending output](USAGE.md#sending-output-to-a-server) |

4. Options and data

## 6. Housekeeping

| # | Question | Command | More |
|---|---|---|---|
| 6.1 | How do I move a session to another machine? | `psm sessions export > s.json`, then `psm sessions import s.json` | [export and import](USAGE.md#export-and-import) |
| 6.2 | How do I move everything to another machine? | `psm sessions export --all > all.json`, then `psm sessions import all.json` | [export and import](USAGE.md#export-and-import) |
| 6.3 | How do I change the defaults? | `psm config`, then edit the file | [configuration](USAGE.md#configuration) |
| 6.4 | How do I delete old sessions? | `psm sessions purge --older-than 180d` | [deleting data](USAGE.md#deleting-data) |
| 6.5 | How do I back up everything? | `psm backup ~/psm-backup.db` | [sessions and housekeeping](USAGE.md#sessions-and-housekeeping) |
| 6.6 | How do I start over completely? | `psm sessions reset` (the database), `psm init force` (database and config) | [deleting data](USAGE.md#deleting-data) |
| 6.7 | How do I delete one snapshot? | `psm snapshots delete 2` (a number or label; no argument: the latest) | [deleting data](USAGE.md#deleting-data) |
| 6.8 | How do I start the current session over? | `psm snapshots reset` (all its snapshots go, a new baseline is taken) | [deleting data](USAGE.md#deleting-data) |
| 6.9 | How do I copy one snapshot into another session? | `psm export 2 > s.json`, then `psm sessions activate other` and `psm import s.json` | [export and import](USAGE.md#export-and-import) |
| 6.10 | Which config file and database are in use? | `psm --config`, `psm --db` | [configuration](USAGE.md#configuration) |
| 6.11 | How do I get tab completion? | `psm init` | [tab completion](USAGE.md#tab-completion) |
| 6.12 | I installed a new psm and it refuses my database? | `psm update` (upgrades it in place, keeps a copy) | [sessions and housekeeping](USAGE.md#sessions-and-housekeeping) |
| 6.13 | Which version is this? | `psm version` | [help](USAGE.md#help) |
| 6.14 | How do I rename a snapshot, or change its description? | `psm snapshots rename 2 new-label "new description"` | [sessions and housekeeping](USAGE.md#sessions-and-housekeeping) |
| 6.15 | How do I rename a session, or change its description? | `psm sessions rename old-session "new session 1" "new description"` | [sessions and housekeeping](USAGE.md#sessions-and-housekeeping) |

5. Housekeeping

## Keeping this file and `psm faq` in step

Note: The rows above are the same as `FAQ` in the code, in the same
order and with the same numbers. When you add a question, add it to
both.
