#!/usr/bin/env python3
"""The fake programs of the demo. Every "program" is this file started
through a symlink to python3 named after it (workload makes them), so
the process table shows api-server, cache-daemon, job-runner, worker and
build-job. Nothing here reads, writes or connects to anything.

  grow STEP_MB MAX_MB THREADS   allocate STEP_MB every second up to MAX_MB, THREADS idle threads
  hold MB                       allocate MB once and sleep
  churn                         start a worker every 2 s that lives 3 s: new and gone rows
  build SECONDS                 allocate, spin a few threads, free, exit 0: what `track` follows
"""

import os
import subprocess
import sys
import threading
import time

PAGE = 4096


def block(mb):
    """Memory that is really resident: every page written once."""
    b = bytearray(mb << 20)
    for i in range(0, len(b), PAGE):
        b[i] = 1
    return b


def idle_threads(n):
    for _ in range(n):
        threading.Thread(target=lambda: time.sleep(10**9), daemon=True).start()


def grow(step_mb, max_mb, threads):
    idle_threads(threads)
    blocks = []
    while True:
        if len(blocks) * step_mb < max_mb:
            blocks.append(block(step_mb))
        time.sleep(1)


def hold(mb):
    keep = block(mb)
    while keep:
        time.sleep(1)


def churn():
    worker = os.path.join(os.environ["DEMO_BIN"], "worker")
    while True:
        child = subprocess.Popen([worker, __file__, "hold", "12"])
        time.sleep(3)
        child.kill()
        child.wait()
        time.sleep(2)


def spin(stop):
    x = 0
    while not stop.is_set():
        x = (x * 31 + 7) % 1_000_003


def build(seconds):
    stop = threading.Event()
    for _ in range(3):
        threading.Thread(target=spin, args=(stop,), daemon=True).start()
    blocks = []
    end = time.time() + seconds
    while time.time() < end:
        blocks.append(block(24))
        time.sleep(0.5)
        if len(blocks) == 5:
            blocks.clear()
    stop.set()


if __name__ == "__main__":
    mode, args = sys.argv[1], [int(a) for a in sys.argv[2:]]
    {"grow": grow, "hold": hold, "churn": churn, "build": build}[mode](*args)
