//! Stamps the binary with the short commit hash and the build date, for
//! `psm version`. Both fall back to "unknown" outside a git checkout or
//! without the tools, so the build never fails because of them. A
//! non-empty `PSM_GIT_HASH` in the environment is used as the hash
//! instead of asking git: for builds without the checkout, like the
//! Docker images.

use std::process::Command;

/// Trimmed stdout of a successful command; `None` otherwise.
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-env-changed=PSM_GIT_HASH");
    let hash = match std::env::var("PSM_GIT_HASH").ok().filter(|h| !h.is_empty()) {
        Some(h) => h,
        None => match run("git", &["rev-parse", "--short", "HEAD"]).filter(|h| !h.is_empty()) {
            Some(h) if run("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty()) => {
                format!("{h}-dirty")
            }
            Some(h) => h,
            None => "unknown".into(),
        },
    };
    let date = run("date", &["-u", "+%Y-%m-%d"])
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=PSM_GIT_HASH={hash}");
    println!("cargo:rustc-env=PSM_BUILD_DATE={date}");
    // The crate lives under crates/, so ask git where the repository is
    // instead of assuming `.git` next to this file.
    if let Some(git_dir) = run("git", &["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        println!("cargo:rerun-if-changed={git_dir}/index");
    }
}
