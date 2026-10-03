CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    archived_at TEXT,
    notes TEXT
);

CREATE TABLE snapshots (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL,
    -- number within the session: 0 is the baseline; never reused after a delete
    seq INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    label TEXT,

    hostname TEXT,
    boot_id TEXT,
    kernel_version TEXT,

    collector_uid INTEGER NOT NULL,
    deep INTEGER NOT NULL DEFAULT 0,

    clk_tck INTEGER NOT NULL,
    uptime_seconds REAL NOT NULL,

    load_1 REAL,
    load_5 REAL,
    load_15 REAL,

    memory_total INTEGER,
    memory_available INTEGER,
    swap_total INTEGER,
    swap_used INTEGER,

    UNIQUE(session_id, seq),
    FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE TABLE snapshot_meminfo (
    snapshot_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value INTEGER NOT NULL,

    PRIMARY KEY(snapshot_id, key),
    FOREIGN KEY(snapshot_id) REFERENCES snapshots(id)
) WITHOUT ROWID;

CREATE TABLE snapshot_cgroups (
    snapshot_id INTEGER NOT NULL,
    cgroup TEXT NOT NULL,
    memory_current INTEGER,
    swap_current INTEGER,

    PRIMARY KEY(snapshot_id, cgroup),
    FOREIGN KEY(snapshot_id) REFERENCES snapshots(id)
) WITHOUT ROWID;

CREATE TABLE processes (
    id INTEGER PRIMARY KEY,
    snapshot_id INTEGER NOT NULL,

    pid INTEGER NOT NULL,
    ppid INTEGER,

    uid INTEGER,
    username TEXT,

    comm TEXT,
    exe TEXT,
    cmdline TEXT,

    state TEXT,
    kthread INTEGER NOT NULL DEFAULT 0,

    start_time INTEGER NOT NULL,

    rss_bytes INTEGER,
    rss_anon_bytes INTEGER,
    rss_file_bytes INTEGER,
    rss_shmem_bytes INTEGER,
    swap_bytes INTEGER,
    vsz_bytes INTEGER,
    pss_bytes INTEGER,
    uss_bytes INTEGER,

    cpu_user INTEGER,
    cpu_system INTEGER,

    thread_count INTEGER,
    nice INTEGER,

    read_bytes INTEGER,
    write_bytes INTEGER,

    cgroup TEXT,

    UNIQUE(snapshot_id, pid),
    FOREIGN KEY(snapshot_id) REFERENCES snapshots(id)
);

CREATE INDEX idx_snapshots_session
    ON snapshots(session_id);

-- lookups by snapshot are covered by UNIQUE(snapshot_id, pid)

CREATE INDEX idx_processes_pid
    ON processes(pid);

CREATE INDEX idx_processes_exe
    ON processes(exe);

CREATE INDEX idx_processes_comm
    ON processes(comm);

CREATE INDEX idx_processes_start
    ON processes(start_time);
