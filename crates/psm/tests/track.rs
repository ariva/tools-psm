//! `psm track`: a started command, an existing pid, words against the
//! fixture tree, `--times`, `--for`, `--save`, the exit codes.

mod common;

use std::process::{Command, Output};

use serde_json::Value;

use common::*;

/// psm against the real /proc: spawn and pid modes need the child to be visible.
fn real(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_psm"))
        .env_remove("PSM_CONFIG")
        .env_remove("PSM_DB")
        .args(["--config", "/dev/null", "--proc-root", "/proc"])
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn data(o: &Output) -> Value {
    let mut v: Value =
        serde_json::from_str(&stdout(o)).unwrap_or_else(|e| panic!("{e}: {}", stdout(o)));
    assert_eq!(v["command"], "track", "{v}");
    assert!(v["psm"]["version"].is_string() && v["elapsed_ms"].is_number());
    v["data"].take()
}

#[test]
fn spawn_samples_until_the_command_exits() {
    let o = real(&[
        "track",
        "--every",
        "100ms",
        "--quiet",
        "--",
        "sh",
        "-c",
        "sleep 0.35",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = stdout(&o);
    assert!(
        text.contains("TRACK  sh -c sleep 0.35   exit 0   wall "),
        "{text}"
    );
    assert!(
        text.contains("\nHOST   ") && text.contains("\nMETRIC "),
        "{text}"
    );
    for row in [
        "\nrss ",
        "\nanon ",
        "\nswap ",
        "\ncpu % ",
        "\nthreads ",
        "\nprocs ",
    ] {
        assert!(text.contains(row), "{row:?} in {text}");
    }
    assert!(!text.contains("saved "), "{text}");

    let o = real(&[
        "--json",
        "track",
        "--every",
        "100ms",
        "--",
        "sh",
        "-c",
        "sleep 0.35",
    ]);
    let d = data(&o);
    assert_eq!(d["status"], "done");
    assert_eq!(d["target"]["mode"], "spawn");
    assert_eq!(
        d["target"]["command"],
        serde_json::json!(["sh", "-c", "sleep 0.35"])
    );
    assert!(d["host"]["cpus"].as_u64().unwrap() >= 1 && d["host"]["memory_total"].is_number());
    assert_eq!(d["settings"]["every_ms"], 100);
    assert_eq!(d["settings"]["times"], 1);
    assert!(d.get("run").is_none(), "no run in progress when done: {d}");
    let runs = rows(&d, "runs");
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(
        (
            run["run"].as_u64(),
            run["status"].as_str(),
            run["ended"].as_str(),
            run["exit"].as_i64()
        ),
        (Some(1), Some("done"), Some("exit"), Some(0))
    );
    let samples = run["samples"].as_u64().unwrap();
    assert!(samples >= 2, "{run}");
    assert_eq!(rows(run, "series").len() as u64, samples);
    let first = &run["series"][0];
    assert_eq!(first["t_ms"], 0);
    assert!(first["target"]["procs"].as_u64().unwrap() >= 1, "{first}");
    assert!(
        first["target"]["cpu"].is_null(),
        "no period before the first sample"
    );
    assert!(first["system"]["mem_available"].is_number() && first["system"]["load_1"].is_number());
    assert!(run["stats"]["procs"]["max"].as_u64().unwrap() >= 1, "{run}");
    assert!(run["stats"]["system"]["mem_available"]["min"].is_number());
    // `final` is there with one run too, so consumers read one path.
    assert_eq!(d["final"]["runs"], serde_json::json!([1]));
    assert_eq!(d["final"]["exit"], 0);
    assert!(d["final"]["wall_ms"]["min"].is_number(), "{}", d["final"]);
}

#[test]
fn exit_code_passes_through_and_for_stops_a_command() {
    let o = real(&[
        "track", "--every", "50ms", "--quiet", "--", "sh", "-c", "exit 3",
    ]);
    assert_eq!(o.status.code(), Some(3));
    assert!(stdout(&o).contains("   exit 3   "), "{}", stdout(&o));

    let o = real(&[
        "--json", "track", "--for", "300ms", "--every", "100ms", "--", "sleep", "20",
    ]);
    assert!(
        o.status.success(),
        "a --for stop is not a failure: {:?}",
        o.status
    );
    let d = data(&o);
    assert_eq!(d["runs"][0]["ended"], "for");
    assert_eq!(d["runs"][0]["status"], "done");
    let wall = d["runs"][0]["wall_ms"].as_u64().unwrap();
    assert!((300..3000).contains(&wall), "wall {wall}");
}

#[test]
fn pid_mode_follows_a_process_and_its_children_until_gone() {
    let mut child = Command::new("sh")
        .args(["-c", "sleep 0.6"])
        .spawn()
        .unwrap();
    let pid = child.id();
    // Reap it when it exits, or it would stay a zombie in /proc.
    std::thread::spawn(move || child.wait());
    let o = real(&[
        "--json",
        "track",
        "--pid",
        &pid.to_string(),
        "--every",
        "100ms",
    ]);
    let d = data(&o);
    assert_eq!(d["target"]["mode"], "pid");
    assert_eq!(d["target"]["pids"], serde_json::json!([pid]));
    let run = &d["runs"][0];
    assert_eq!(run["ended"], "gone");
    assert_eq!(run["exit"], Value::Null);
    // sh plus its sleep, and never the sample that found them gone.
    assert_eq!(run["stats"]["procs"]["max"], 2, "{run}");
    assert_eq!(run["stats"]["procs"]["min"], 2, "{run}");
}

#[test]
fn words_mode_matches_the_fixture_tree() {
    let e = Env::new("track-words");
    // A fixture never changes, so every sample is the same and the target
    // never goes away: --for ends it.
    let d = e.json(
        "after",
        &[
            "track", "code", "--for", "250ms", "--every", "100ms", "--json",
        ],
    );
    assert_eq!(d["target"]["mode"], "words");
    assert_eq!(d["target"]["group"], "app");
    let run = &d["runs"][0];
    assert_eq!(run["ended"], "for");
    let rss = &run["stats"]["rss"];
    assert_eq!(rss["min"], rss["max"], "static tree: {rss}");
    // node (600) was started by code (100): the app roll-up takes it along.
    assert_eq!(rss["max"], (1400 + 72) * MIB, "{rss}");
    assert_eq!(run["stats"]["procs"]["max"], 2);
    assert!(
        run["series"][0]["system"]["cpu"].is_null(),
        "no /proc/stat in the fixture"
    );
    let plain = e.json(
        "after",
        &[
            "track", "code", "--group", "pid", "--for", "150ms", "--every", "100ms", "--json",
        ],
    );
    assert_eq!(plain["runs"][0]["stats"]["procs"]["max"], 1, "no roll-up");
    let text = e.ok(
        "after",
        &[
            "track", "code", "--for", "150ms", "--every", "100ms", "--quiet",
        ],
    );
    assert!(
        text.starts_with("TRACK  code (app)   stopped by --for   wall "),
        "{text}"
    );
    assert!(
        e.fails(
            "after",
            &[
                "track", "--warmup", "10s", "code", "--for", "150ms", "--every", "100ms", "--quiet"
            ]
        )
        .contains("no samples after the warmup")
    );
}

#[test]
fn times_repeats_a_command_and_compares_the_runs() {
    let o = real(&[
        "track",
        "--every",
        "50ms",
        "--times",
        "3",
        "--skip-runs",
        "1",
        "--pause",
        "100ms",
        "--quiet",
        "--",
        "sh",
        "-c",
        "sleep 0.15",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = stdout(&o);
    assert!(
        text.contains("   runs 3 (1 skipped)   every 0.1s, warmup 0.0s, pause 0.1s   total "),
        "{text}"
    );
    assert!(
        text.contains("\nRUN  EXIT  WALL  RSS MAX  ANON MAX  CPU AVG  THREADS MAX  PROCS MAX\n"),
        "{text}"
    );
    assert!(text.contains("  skipped\n"), "{text}");
    assert!(text.contains("\nFINAL (runs 2–3)  "), "{text}");
    assert!(
        text.contains("\nwall ") && text.contains("\nrss max ") && text.contains("\nprocs max "),
        "{text}"
    );

    let o = real(&[
        "--json",
        "track",
        "--every",
        "50ms",
        "--times",
        "3",
        "--skip-runs",
        "1",
        "--",
        "sh",
        "-c",
        "sleep 0.15",
    ]);
    let d = data(&o);
    let runs = rows(&d, "runs");
    assert_eq!(runs.len(), 3);
    assert_eq!(column(runs, "skipped"), [true, false, false]);
    assert_eq!(d["final"]["runs"], serde_json::json!([2, 3]));
    assert!(
        d["final"]["rss_max"]["spread"].is_number(),
        "{}",
        d["final"]
    );
    assert_eq!(d["settings"]["pause_ms"], 0);

    // A failing run ends the series unless --keep-going.
    let o = real(&[
        "--json", "track", "--every", "50ms", "--times", "3", "--", "sh", "-c", "exit 2",
    ]);
    assert_eq!(o.status.code(), Some(2));
    let d = data(&o);
    assert_eq!(rows(&d, "runs").len(), 1);
    assert_eq!(d["status"], "failed");
    assert_eq!(d["runs"][0]["status"], "failed");
    let o = real(&[
        "--json",
        "track",
        "--every",
        "50ms",
        "--times",
        "3",
        "--keep-going",
        "--",
        "sh",
        "-c",
        "exit 2",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(rows(&data(&o), "runs").len(), 3);
}

#[test]
fn save_writes_the_document_while_running_and_when_done() {
    let dir = std::env::temp_dir().join(format!("psm-track-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("out.json");
    let path = file.to_str().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_psm"))
        .env_remove("PSM_CONFIG")
        .env_remove("PSM_DB")
        .args(["--config", "/dev/null", "--proc-root", "/proc", "--json"])
        .args([
            "track", "--every", "50ms", "--save", path, "--", "sleep", "0.8",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // While it runs: a complete document with status running.
    let mut running = None;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Ok(text) = std::fs::read_to_string(&file)
            && let Ok(v) = serde_json::from_str::<Value>(&text)
            && v["data"]["status"] == "running"
        {
            running = Some(v);
            break;
        }
    }
    let running = running.expect("a running document appears within two seconds");
    assert_eq!(running["command"], "track");
    assert_eq!(running["data"]["run"], 1);
    assert_eq!(running["data"]["runs"][0]["status"], "running");
    assert_eq!(running["data"]["final"], Value::Null);
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success());
    let printed: Value = serde_json::from_slice(&o.stdout).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["data"]["status"], "done");
    assert_eq!(
        saved["data"], printed["data"],
        "the file is the printed document"
    );
    assert!(!dir.join("out.json.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn track_refuses_what_it_cannot_do() {
    let e = Env::new("track-errors");
    assert!(e.fails("before", &["track"]).contains("nothing to track"));
    assert!(
        e.fails("before", &["track", "--times", "2", "--pid", "1"])
            .contains("--times repeats a command")
    );
    assert!(
        e.fails("before", &["track", "--every", "0", "code"])
            .contains("--every needs a duration above zero")
    );
    // Words that match nothing: exit 1, not a usage error.
    let o = e.run("before", &["track", "no-such-process-zzz", "--quiet"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no process matches"));
    let o = real(&["track", "--quiet", "--", "./no-such-binary-zzz"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot start ./no-such-binary-zzz"));
    // The config file sets the defaults; the flag wins.
    let cfg = std::env::temp_dir().join(format!("psm-track-cfg-{}.toml", std::process::id()));
    std::fs::write(&cfg, "[track]\nevery = \"2s\"\nwarmup = \"1s\"\n").unwrap();
    let with_cfg = |args: &[&str]| -> Output {
        e.command("after")
            .env("PSM_CONFIG", &cfg)
            .args(args)
            .output()
            .unwrap()
    };
    let o = with_cfg(&["track", "code", "--for", "100ms", "--warmup", "0", "--json"]);
    let d = data(&o);
    assert_eq!(d["settings"]["every_ms"], 2000, "from the file");
    assert_eq!(d["settings"]["warmup_ms"], 0, "the flag wins");
    let o = with_cfg(&[
        "track", "code", "--for", "1300ms", "--every", "100ms", "--json",
    ]);
    let d = data(&o);
    assert_eq!(d["settings"]["warmup_ms"], 1000);
    assert!(
        d["runs"][0]["dropped"].as_u64().unwrap() >= 9,
        "{}",
        d["runs"][0]["dropped"]
    );
    std::fs::write(&cfg, "[track]\nevery = \"soon\"\n").unwrap();
    let o = with_cfg(&["track", "code", "--for", "100ms"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("invalid duration"));
    let _ = std::fs::remove_file(&cfg);
}
