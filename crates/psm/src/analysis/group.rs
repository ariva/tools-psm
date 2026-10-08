//! Grouping processes: by name, executable, command line, application,
//! user, cgroup or parent.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};

use crate::model::{Proc, Snapshot};

/// `name` is the program name; the kernel calls that field `comm`.
pub const GROUPS: &[&str] = &["name", "exe", "cmdline", "app", "user", "cgroup", "parent"];

/// Processes that start applications but are not the application: init,
/// login and session managers, desktop shells, terminals, shells, sandboxes.
// ponytail: a fixed list of names. A desktop shell or terminal missing here
// shows up as one big "app" holding everything it started. Make it a config
// key (display.launchers) when that happens.
const LAUNCHERS: &[&str] = &[
    "systemd",
    "init",
    "login",
    "sshd",
    "sudo",
    "su",
    "doas",
    "lightdm",
    "sddm",
    "xinit",
    "bash",
    "sh",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "tmux: server",
    "screen",
    "bwrap",
    "flatpak",
    "konsole",
    "xterm",
    "alacritty",
    "kitty",
    "wezterm-gui",
    "foot",
    "tilix",
    "terminator",
    "plasmashell",
    "ksmserver",
    "gnome-shell",
    "mate-session",
    "mate-terminal",
];
/// Families whose members all carry the prefix (names are cut at 15 characters).
const LAUNCHER_PREFIXES: &[&str] = &[
    "cinnamon",
    "gnome-session",
    "gnome-terminal",
    "gdm",
    "xfce4-",
];

fn is_launcher(name: &str) -> bool {
    LAUNCHERS.contains(&name) || LAUNCHER_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Flatpak runs an app in nested `bwrap` sandboxes and spawns helpers from
/// the portal, so its parent chain never leads to one root. systemd puts the
/// whole sandbox in `app-flatpak-<app id>-<pid>.scope`; that id is the app.
fn flatpak_app(cgroup: &str) -> Option<&str> {
    let leaf = cgroup.rsplit('/').next()?;
    let id = leaf.strip_prefix("app-flatpak-")?.strip_suffix(".scope")?;
    let (id, pid) = id.rsplit_once('-')?;
    (!pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// For every process of a snapshot, the name of the application it belongs to:
/// its Flatpak app id when it has one, else the ancestor closest to init that
/// is not a launcher. A process with only launchers above it (a shell, the
/// desktop itself) is its own application.
fn app_roots(s: &Snapshot) -> impl Iterator<Item = ((i64, i64), String)> + '_ {
    let by_pid: HashMap<i64, &Proc> = s.processes.iter().map(|p| (p.pid, p)).collect();
    s.processes.iter().map(move |p| {
        if let Some(id) = p.cgroup.as_deref().and_then(flatpak_app) {
            return ((p.pid, p.start_time), id.to_string());
        }
        let mut root = None;
        let mut current = p;
        // The bound only guards against a corrupt parent loop.
        for _ in 0..64 {
            if !is_launcher(&current.comm) {
                root = Some(current);
            }
            match by_pid.get(&current.ppid) {
                Some(parent) if parent.pid != current.pid => current = parent,
                _ => break,
            }
        }
        ((p.pid, p.start_time), root.unwrap_or(p).comm.clone())
    })
}

/// Maps a process to its group key. Built over every snapshot taking part in a
/// comparison so the same program gets the same key on both sides.
pub struct Grouper {
    pub kind: String,
    exe_fallback: HashSet<String>,
    parents: HashMap<i64, String>,
    /// Process instance `(pid, start_time)` -> application name; only for `app`.
    apps: HashMap<(i64, i64), String>,
}

impl Grouper {
    pub fn new(kind: &str, snaps: &[&Snapshot]) -> Result<Grouper> {
        if !GROUPS.contains(&kind) {
            bail!(
                "unknown group {kind:?} (expected pid, {})",
                GROUPS.join(", ")
            );
        }
        let procs = || snaps.iter().flat_map(|s| s.processes.iter());
        Ok(Grouper {
            kind: kind.to_string(),
            // A program whose exe is unreadable in any snapshot is keyed by comm everywhere.
            exe_fallback: procs()
                .filter(|p| p.exe.is_none())
                .map(|p| p.comm.clone())
                .collect(),
            parents: procs().map(|p| (p.pid, p.comm.clone())).collect(),
            apps: snaps
                .iter()
                .filter(|_| kind == "app")
                .flat_map(|s| app_roots(s))
                .collect(),
        })
    }

    pub fn key(&self, p: &Proc) -> String {
        match self.kind.as_str() {
            "exe" => match &p.exe {
                Some(e) if !self.exe_fallback.contains(&p.comm) => e.clone(),
                _ => p.comm.clone(),
            },
            "cmdline" => p.cmdline.clone().unwrap_or_else(|| p.comm.clone()),
            "user" => p
                .username
                .clone()
                .or(p.uid.map(|u| u.to_string()))
                .unwrap_or_else(|| "?".into()),
            "cgroup" => p.cgroup.clone().unwrap_or_else(|| "?".into()),
            "app" => self
                .apps
                .get(&(p.pid, p.start_time))
                .unwrap_or(&p.comm)
                .clone(),
            "parent" => match self.parents.get(&p.ppid) {
                Some(comm) => format!("{} {comm}", p.ppid),
                None => p.ppid.to_string(),
            },
            _ => p.comm.clone(),
        }
    }

    /// What is printed for a key: cgroups are shown by their last path component.
    pub fn display(&self, key: &str) -> String {
        if self.kind == "cgroup" {
            match key.rsplit('/').next() {
                Some(last) if !last.is_empty() => last.to_string(),
                _ => key.to_string(),
            }
        } else {
            key.to_string()
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind.as_str() {
            "app" => "APP",
            "user" => "USER",
            "cgroup" => "CGROUP",
            "parent" => "PARENT",
            _ => "PROGRAM",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i64, ppid: i64, comm: &str) -> Proc {
        Proc {
            pid,
            ppid,
            comm: comm.into(),
            start_time: pid,
            ..Default::default()
        }
    }

    #[test]
    fn app_is_the_topmost_ancestor_that_is_not_a_launcher() {
        let s = Snapshot {
            processes: vec![
                proc(1, 0, "systemd"),
                // An editor started through a shell wrapper, with nested helpers.
                proc(10, 1, "bash"),
                proc(20, 10, "zed-editor"),
                proc(30, 20, "MainThread"),
                proc(31, 30, "MainThread"),
                proc(32, 31, "rust-analyzer"),
                // A browser started from the desktop.
                proc(40, 1, "lightdm"),
                proc(41, 40, "cinnamon-sessio"),
                proc(42, 41, "cinnamon"),
                proc(50, 42, "chrome"),
                proc(51, 50, "chrome"),
                // A sandboxed app, and a command typed in a terminal.
                proc(60, 42, "bwrap"),
                proc(61, 60, "slack"),
                proc(70, 1, "gnome-terminal-"),
                proc(71, 70, "bash"),
                proc(72, 71, "npm exec vite b"),
                proc(73, 72, "sh"),
                proc(74, 73, "MainThread"),
            ],
            ..Default::default()
        };
        let g = Grouper::new("app", &[&s]).unwrap();
        let app = |pid: i64| g.key(s.processes.iter().find(|p| p.pid == pid).unwrap());
        assert_eq!(app(32), "zed-editor", "three levels below the editor");
        assert_eq!(app(30), "zed-editor");
        assert_eq!(
            app(51),
            "chrome",
            "the desktop shell is not the application"
        );
        assert_eq!(app(61), "slack", "the sandbox wrapper is skipped");
        assert_eq!(
            app(74),
            "npm exec vite b",
            "terminal and shells are skipped"
        );
        // Only launchers above it: it is its own application.
        assert_eq!(app(71), "bash");
        assert_eq!(app(42), "cinnamon");
        assert_eq!(app(1), "systemd");
    }

    #[test]
    fn flatpak_app_comes_from_the_systemd_scope() {
        let scope =
            |leaf: &str| format!("/user.slice/user-1000.slice/user@1000.service/app.slice/{leaf}");
        assert_eq!(
            flatpak_app(&scope("app-flatpak-com.slack.Slack-231634.scope")),
            Some("com.slack.Slack")
        );
        assert_eq!(
            flatpak_app(&scope("app-gnome-code-1234.scope")),
            None,
            "desktop launches keep the ancestry walk"
        );
        assert_eq!(flatpak_app(&scope("vte-spawn-3f1a.scope")), None);
        assert_eq!(flatpak_app(&scope("app-flatpak-x.scope")), None);
        assert_eq!(flatpak_app("/"), None);
        // Every layer of the sandbox, the crash handler started by one of them,
        // and a sandbox the portal spawned share the id; the parent chain does not.
        let mut s = Snapshot {
            processes: vec![
                proc(1, 0, "systemd"),
                proc(2, 1, "flatpak-portal"),
                proc(10, 1, "bwrap"),
                proc(11, 10, "bwrap"),
                proc(12, 11, "com.slack.Slack"),
                proc(13, 12, "slack"),
                proc(14, 11, "chrome_crashpad"),
                proc(20, 2, "bwrap"),
                proc(21, 20, "slack"),
            ],
            ..Default::default()
        };
        for p in &mut s.processes[1..] {
            p.cgroup = Some(scope("app-flatpak-com.slack.Slack-10.scope"));
        }
        let g = Grouper::new("app", &[&s]).unwrap();
        for p in &s.processes[1..] {
            assert_eq!(g.key(p), "com.slack.Slack", "pid {}", p.pid);
        }
        assert_eq!(g.key(&s.processes[0]), "systemd");
    }
}
