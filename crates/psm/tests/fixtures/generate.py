#!/usr/bin/env python3
"""Regenerates tests/fixtures/proc/{before,after}: two small fake /proc trees.

Run `just fixtures` after changing this file, then commit the result.
All sizes are in MiB here and written in kB, the way the kernel does.
"""

import os
import shutil
from pathlib import Path

ROOT = Path(__file__).parent / "proc"
KTHREAD = 0x208040  # includes PF_KTHREAD (0x200000)
USER = 0x400000
MIB = 1024  # kB


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def process(root, pid, comm, *, ppid=1, uid=1000, start, rss=None, anon=0, swap=0, threads=1,
            cpu=(0, 0), exe=None, cmdline=None, cgroup="/user.slice/session.scope",
            io=True, pss=None, flags=USER):
    d = root / str(pid)
    d.mkdir(parents=True)
    rss_kb = (rss or 0) * MIB
    tail = "0 " * 12 + "17 " + "0 " * 14
    write(d / "stat",
          f"{pid} ({comm}) S {ppid} {pid} {pid} 0 -1 {flags} 0 0 0 0 {cpu[0]} {cpu[1]} 0 0 20 0 "
          f"{threads} 0 {start} {rss_kb * 4096} {rss_kb // 4} 18446744073709551615 {tail.strip()}\n")
    memory = "" if rss is None else (
        f"VmRSS:\t{rss_kb} kB\nRssAnon:\t{anon * MIB} kB\nRssFile:\t{(rss - anon) * MIB} kB\n"
        f"RssShmem:\t0 kB\nVmSwap:\t{swap * MIB} kB\n")
    zeros = "0000000000000000"
    write(d / "status",
          f"Name:\t{comm}\nUmask:\t0002\nState:\tS (sleeping)\nTgid:\t{pid}\nNgid:\t0\nPid:\t{pid}\n"
          f"PPid:\t{ppid}\nTracerPid:\t0\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\nGid:\t{uid}\t{uid}\t{uid}\t{uid}\n"
          f"FDSize:\t64\nGroups:\t{uid}\n{memory}Threads:\t{threads}\nSigQ:\t0/128000\n"
          + "".join(f"{k}:\t{zeros}\n" for k in
                    ("SigPnd", "ShdPnd", "SigBlk", "SigIgn", "SigCgt", "CapInh", "CapPrm", "CapEff")))
    write(d / "cmdline", "".join(arg + "\0" for arg in (cmdline or "").split()))
    write(d / "cgroup", f"0::{cgroup}\n")
    if exe:
        os.symlink(exe, d / "exe")
    if io:
        write(d / "io", "rchar: 1000\nwchar: 2000\nsyscr: 10\nsyscw: 20\n"
                        f"read_bytes: {pid * 4096}\nwrite_bytes: {pid * 8192}\ncancelled_write_bytes: 0\n")
    if pss is not None:
        write(d / "smaps_rollup",
              "00400000-7ffc00000000 ---p 00000000 00:00 0                          [rollup]\n"
              f"Rss:\t{rss_kb} kB\nPss:\t{pss * MIB} kB\n"
              f"Private_Clean:\t{MIB} kB\nPrivate_Dirty:\t{(pss - 2) * MIB} kB\n")


def system(root, *, uptime, available, swap_free, shmem, hugepages, code_cgroup, session_cgroup):
    write(root / "uptime", f"{uptime:.2f} {uptime * 8:.2f}\n")
    write(root / "loadavg", "1.24 0.98 0.87 2/500 1234\n")
    write(root / "meminfo",
          f"MemTotal:       16777216 kB\nMemFree:         8000000 kB\nMemAvailable:   {available} kB\n"
          f"SwapTotal:       4194304 kB\nSwapFree:        {swap_free} kB\nShmem:           {shmem} kB\n"
          f"Slab:             200000 kB\nHugePages_Total:  {hugepages}\n")
    write(root / "sys/kernel/hostname", "fixture-host\n")
    write(root / "sys/kernel/osrelease", "6.8.0-fixture\n")
    write(root / "sys/kernel/random/boot_id", "11111111-2222-3333-4444-555555555555\n")
    cg = root / "cgroup-root/user.slice"
    write(cg / "app-code.scope/memory.current", f"{code_cgroup * MIB * 1024}\n")
    write(cg / "app-code.scope/memory.swap.current", "0\n")
    write(cg / "session.scope/memory.current", f"{session_cgroup * MIB * 1024}\n")
    write(cg / "session.scope/memory.swap.current", "0\n")
    # /system.slice/odd.service has no memory.current: stands in for cgroup v1 / unreadable.


def common(root, *, code_rss, code_cpu, odd_rss, odd_swap):
    process(root, 2, "kthreadd", ppid=0, uid=0, start=0, cgroup="/", io=False, flags=KTHREAD)
    # Continuing process that grows.
    process(root, 100, "code", start=1000, rss=code_rss, anon=code_rss - 200, threads=18, cpu=code_cpu,
            exe="/usr/share/code/code", cmdline="code --type=renderer",
            cgroup="/user.slice/app-code.scope", pss=code_rss - 100)
    # Another user's process: no exe / io / smaps_rollup (stands in for EACCES).
    # Its comm has spaces and ')', and between the snapshots it is swapped out, not shrunk.
    process(root, 500, "my (we)ird) name", uid=0, start=4000, rss=odd_rss, anon=odd_rss, swap=odd_swap,
            io=False, cgroup="/system.slice/odd.service")


def main():
    shutil.rmtree(ROOT, ignore_errors=True)

    before = ROOT / "before"
    system(before, uptime=1000, available=12000000, swap_free=4194304, shmem=100000, hugepages=0,
           code_cgroup=1200, session_cgroup=900)
    common(before, code_rss=1000, code_cpu=(5000, 1000), odd_rss=20, odd_swap=0)
    process(before, 200, "old-helper", start=3000, rss=42, anon=30, exe="/usr/bin/old-helper",
            cmdline="old-helper --serve", pss=40)
    process(before, 300, "rust-analyzer", start=2000, rss=812, anon=700, threads=24,
            exe="/usr/bin/rust-analyzer", cmdline="rust-analyzer", pss=800)
    process(before, 400, "alpha", start=5000, rss=5, anon=3, exe="/usr/bin/alpha", cmdline="alpha", pss=4)

    after = ROOT / "after"
    system(after, uptime=4600, available=11000000, swap_free=4184064, shmem=600000, hugepages=2,
           code_cgroup=1700, session_cgroup=500)
    common(after, code_rss=1400, code_cpu=(5500, 1100), odd_rss=10, odd_swap=10)
    # old-helper (200) is gone. rust-analyzer restarted under a new pid.
    process(after, 310, "rust-analyzer", start=50000, rss=344, anon=300, threads=24,
            exe="/usr/bin/rust-analyzer", cmdline="rust-analyzer", pss=340)
    # PID 400 was reused by a different program.
    process(after, 400, "beta", start=90000, rss=7, anon=5, exe="/usr/bin/beta", cmdline="beta", pss=6)
    process(after, 600, "node", ppid=100, start=100000, rss=72, anon=60, threads=11, cpu=(300, 60),
            exe="/usr/bin/node", cmdline="node server.js", pss=70)


if __name__ == "__main__":
    main()
