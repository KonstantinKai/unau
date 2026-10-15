//! The configured run command: in a new Herdr or tmux pane when the tool runs
//! inside one, otherwise in the foreground while the interface is suspended.

use crate::config::RunIn;
use anyhow::{Context, Result, bail};
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

/// Where a run goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Herdr,
    Tmux,
    /// In the background, output shown in revmark's run pane.
    Pane,
    Foreground,
}

/// The multiplexers the tool is running inside, read once at startup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Multiplexers {
    /// `HERDR_ENV=1`.
    pub herdr: bool,
    /// `TMUX` is set, as tmux does in every pane.
    pub tmux: bool,
}

impl Multiplexers {
    pub fn detect() -> Self {
        Self {
            herdr: std::env::var("HERDR_ENV").is_ok_and(|v| v == "1"),
            tmux: std::env::var_os("TMUX").is_some_and(|v| !v.is_empty()),
        }
    }
}

/// Picks where to run. `auto` prefers Herdr, then tmux; when both are around
/// the environment cannot say which is innermost, so `[run] in` decides.
pub fn place(run_in: RunIn, around: Multiplexers) -> Result<Place, String> {
    let wanted = match run_in {
        RunIn::Auto if around.herdr => Place::Herdr,
        RunIn::Auto if around.tmux => Place::Tmux,
        RunIn::Auto | RunIn::Foreground => Place::Foreground,
        RunIn::Herdr => Place::Herdr,
        RunIn::Tmux => Place::Tmux,
        RunIn::Pane => Place::Pane,
    };
    match wanted {
        Place::Herdr if !around.herdr => {
            Err("[run] in = \"herdr\", but this is not a Herdr pane".to_owned())
        }
        Place::Tmux if !around.tmux => {
            Err("[run] in = \"tmux\", but this is not a tmux pane".to_owned())
        }
        place => Ok(place),
    }
}

/// Splits the current tmux pane to the right and types `command` into the new
/// pane's shell. Focus stays on the tool, and the shell outlives the command
/// so its output stays readable.
pub fn in_tmux_pane(command: &str, cwd: &Path) -> Result<()> {
    tmux_split(command, cwd, &[], None).map(|_| ())
}

/// `server` adds arguments before the subcommand (a test socket) and `target`
/// names the pane to split when not running inside it. Returns the new pane id.
fn tmux_split(command: &str, cwd: &Path, server: &[&str], target: Option<&str>) -> Result<String> {
    let tmux = |args: &[&str]| -> Result<String> {
        let output = Command::new("tmux")
            .args(server)
            .args(args)
            .output()
            .context("run tmux")?;
        if !output.status.success() {
            bail!(
                "tmux {} failed: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let cwd = cwd.to_string_lossy();
    let mut split = vec![
        "split-window",
        "-h",
        "-d",
        "-P",
        "-F",
        "#{pane_id}",
        "-c",
        &cwd,
    ];
    if let Some(target) = target {
        split.extend(["-t", target]);
    }
    let pane = tmux(&split)?;
    tmux(&["send-keys", "-t", &pane, "-l", command])?;
    tmux(&["send-keys", "-t", &pane, "Enter"])?;
    Ok(pane)
}

/// Splits the current Herdr pane and runs `command` in the new one. Returns
/// immediately; the interface keeps running.
pub fn in_herdr_pane(command: &str, cwd: &Path) -> Result<()> {
    let output = Command::new("herdr")
        .args([
            "pane",
            "split",
            "--current",
            "--direction",
            "right",
            "--no-focus",
            "--cwd",
        ])
        .arg(cwd)
        .output()
        .context("run herdr")?;
    if !output.status.success() {
        bail!(
            "herdr pane split failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let pane = pane_id(&String::from_utf8_lossy(&output.stdout))?;
    let status = Command::new("herdr")
        .args(["pane", "run", &pane, command])
        .output()
        .context("run herdr")?;
    if !status.status.success() {
        bail!(
            "herdr pane run failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }
    Ok(())
}

/// The new pane's id from `herdr pane split` output: `.result.pane.pane_id`.
fn pane_id(json: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(json).context("parse herdr output")?;
    value["result"]["pane"]["pane_id"]
        .as_str()
        .map(str::to_owned)
        .context("herdr output has no result.pane.pane_id")
}

/// A command running in the background with its output collected line by
/// line. Lives in the event loop, never in the state.
pub struct Background {
    child: Child,
    lines: Receiver<String>,
    started: Instant,
}

impl Background {
    /// Starts `command` through the shell in `cwd`, without a terminal.
    pub fn start(command: &str, cwd: &Path) -> Result<Self> {
        let mut child = Command::new("sh")
            .args(["-c", command])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start the configured command")?;
        let (sender, lines) = mpsc::channel();
        let readers: [Option<Box<dyn Read + Send>>; 2] = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ];
        for reader in readers.into_iter().flatten() {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(reader).lines() {
                    let Ok(line) = line else { break };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Ok(Self {
            child,
            lines,
            started: Instant::now(),
        })
    }

    /// Time since the command started.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Output lines that arrived since the last call.
    pub fn take_lines(&self) -> Vec<String> {
        self.lines.try_iter().collect()
    }

    /// The exit code once the command has ended; `-1` when killed by a signal.
    pub fn finished(&mut self) -> Option<i32> {
        self.child
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.code().unwrap_or(-1))
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Quitting revmark mid-run stops the run rather than orphaning it.
impl Drop for Background {
    fn drop(&mut self) {
        if self.finished().is_none() {
            self.stop();
        }
    }
}

/// Runs `command` through the shell in `cwd`, attached to the terminal.
pub fn foreground(command: &str, cwd: &Path) -> Result<()> {
    let status = Command::new("sh")
        .args(["-c", command])
        .current_dir(cwd)
        .status()
        .context("run the configured command")?;
    if !status.success() {
        bail!("command exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_run_collects_both_streams_and_the_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let mut run = Background::start("echo out; echo err >&2; exit 3", dir.path()).unwrap();
        let mut lines = Vec::new();
        let mut code = None;
        for _ in 0..500 {
            lines.extend(run.take_lines());
            if let Some(c) = run.finished() {
                code = Some(c);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        lines.extend(run.take_lines());
        assert_eq!(code, Some(3));
        lines.sort();
        assert_eq!(lines, ["err", "out"]);
    }

    #[test]
    fn stopping_a_background_run_ends_it_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut run = Background::start("sleep 30", dir.path()).unwrap();
        let started = std::time::Instant::now();
        run.stop();
        assert!(run.finished().is_some());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn auto_prefers_herdr_then_tmux_and_forced_places_must_exist() {
        let none = Multiplexers::default();
        let tmux = Multiplexers {
            herdr: false,
            tmux: true,
        };
        let both = Multiplexers {
            herdr: true,
            tmux: true,
        };
        assert_eq!(place(RunIn::Auto, none), Ok(Place::Foreground));
        assert_eq!(place(RunIn::Auto, tmux), Ok(Place::Tmux));
        assert_eq!(place(RunIn::Auto, both), Ok(Place::Herdr));
        assert_eq!(place(RunIn::Tmux, both), Ok(Place::Tmux));
        assert_eq!(place(RunIn::Foreground, both), Ok(Place::Foreground));
        assert!(
            place(RunIn::Tmux, none)
                .unwrap_err()
                .contains("not a tmux pane")
        );
        assert!(
            place(RunIn::Herdr, tmux)
                .unwrap_err()
                .contains("not a Herdr pane")
        );
    }

    /// Runs against a private tmux server, so no real session is touched.
    #[test]
    fn tmux_split_opens_a_pane_in_the_repository_and_runs_the_command() {
        if Command::new("tmux").arg("-V").output().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let socket = format!("revmark-test-{}", std::process::id());
        let server = ["-L", socket.as_str(), "-f", "/dev/null"];
        let tmux = |args: &[&str]| {
            Command::new("tmux")
                .args(server)
                .args(args)
                .output()
                .unwrap()
        };
        assert!(
            tmux(&["new-session", "-d", "-s", "t", "/bin/sh"])
                .status
                .success()
        );
        tmux(&["set-option", "-g", "default-shell", "/bin/sh"]);
        let pane = tmux_split("touch ran", dir.path(), &server, Some("t")).unwrap();
        let ran = dir.path().join("ran");
        for _ in 0..200 {
            if ran.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let panes =
            String::from_utf8_lossy(&tmux(&["list-panes", "-t", "t", "-F", "#{pane_id}"]).stdout)
                .into_owned();
        tmux(&["kill-server"]);
        assert!(ran.exists(), "command did not run in the new pane");
        assert!(panes.lines().any(|p| p == pane), "{panes}");
        assert_eq!(panes.lines().count(), 2, "{panes}");
    }

    #[test]
    fn pane_id_comes_from_the_split_result() {
        let json = r#"{"result":{"pane":{"pane_id":"w1:p3","cwd":"/x"}}}"#;
        assert_eq!(pane_id(json).unwrap(), "w1:p3");
        assert!(pane_id(r#"{"result":{}}"#).is_err());
    }

    #[test]
    fn foreground_runs_in_the_given_directory() {
        let dir = tempfile::tempdir().unwrap();
        foreground("touch ran", dir.path()).unwrap();
        assert!(dir.path().join("ran").exists());
        assert!(foreground("exit 3", dir.path()).is_err());
    }
}
