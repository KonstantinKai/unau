//! The configured run command: in a new Herdr, tmux or zellij pane when the
//! tool runs inside one, otherwise in the foreground while the interface is
//! suspended.

use crate::config::RunIn;
use anyhow::{Context, Result, bail};
use std::{
    ffi::{OsStr, OsString},
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
    Zellij,
    /// In the background, output shown in unau's run pane.
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
    /// `ZELLIJ` is set, as zellij does in every pane.
    pub zellij: bool,
}

impl Multiplexers {
    pub fn detect() -> Self {
        Self {
            herdr: std::env::var("HERDR_ENV").is_ok_and(|v| v == "1"),
            tmux: std::env::var_os("TMUX").is_some_and(|v| !v.is_empty()),
            zellij: std::env::var_os("ZELLIJ").is_some(),
        }
    }
}

/// Picks where to run. `auto` prefers Herdr, then tmux, then zellij; when
/// more than one is around the environment cannot say which is innermost, so
/// `[run] in` decides.
pub fn place(run_in: RunIn, around: Multiplexers) -> Result<Place, String> {
    let wanted = match run_in {
        RunIn::Auto if around.herdr => Place::Herdr,
        RunIn::Auto if around.tmux => Place::Tmux,
        RunIn::Auto if around.zellij => Place::Zellij,
        RunIn::Auto | RunIn::Foreground => Place::Foreground,
        RunIn::Herdr => Place::Herdr,
        RunIn::Tmux => Place::Tmux,
        RunIn::Zellij => Place::Zellij,
        RunIn::Pane => Place::Pane,
    };
    match wanted {
        Place::Herdr if !around.herdr => {
            Err("[run] in = \"herdr\", but this is not a Herdr pane".to_owned())
        }
        Place::Tmux if !around.tmux => {
            Err("[run] in = \"tmux\", but this is not a tmux pane".to_owned())
        }
        Place::Zellij if !around.zellij => {
            Err("[run] in = \"zellij\", but this is not a zellij pane".to_owned())
        }
        place => Ok(place),
    }
}

/// A multiplexer pane that `r` opened for the agent, by the id its
/// multiplexer gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentPane {
    pub place: Place,
    pub id: String,
}

/// Opens a new pane in `place` running `command` and returns it. `place` is
/// one of the multiplexers; the run pane and the foreground have no pane.
pub fn open_pane(place: Place, command: &str, cwd: &Path) -> Result<AgentPane> {
    let id = match place {
        Place::Herdr => in_herdr_pane(command, cwd)?,
        Place::Tmux => tmux_split(command, cwd, &[], None)?,
        Place::Zellij => in_zellij_pane(command, cwd)?,
        Place::Pane | Place::Foreground => bail!("{place:?} runs have no pane"),
    };
    Ok(AgentPane { place, id })
}

/// Whether `pane` is still there to type into. A zellij pane whose command
/// has exited is kept on screen but counts as gone: Enter there would start
/// the command again instead of reaching the agent.
pub fn pane_open(pane: &AgentPane) -> bool {
    let output = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    match pane.place {
        Place::Tmux => output(
            "tmux",
            &["display-message", "-p", "-t", &pane.id, "#{pane_dead}"],
        )
        .is_some_and(|dead| dead.trim() == "0"),
        Place::Herdr => output("herdr", &["pane", "get", &pane.id]).is_some(),
        Place::Zellij => output("zellij", &["action", "list-panes", "--json", "--state"])
            .is_some_and(|json| zellij_pane_open(&json, &pane.id)),
        Place::Pane | Place::Foreground => false,
    }
}

/// Whether the terminal pane `id` (`terminal_3`) is listed in zellij's
/// `list-panes --json --state` output with its command still running.
fn zellij_pane_open(json: &str, id: &str) -> bool {
    let Some(number) = id
        .strip_prefix("terminal_")
        .and_then(|n| n.parse::<u64>().ok())
    else {
        return false;
    };
    let Ok(serde_json::Value::Array(panes)) = serde_json::from_str(json) else {
        return false;
    };
    panes.iter().any(|p| {
        p["is_plugin"] == false
            && p["id"].as_u64() == Some(number)
            && p["exited"] != true
            && p["is_held"] != true
    })
}

/// Types `text` into the agent's pane and presses Enter, as if it were typed
/// at the agent's prompt.
pub fn send_to_pane(pane: &AgentPane, text: &str) -> Result<()> {
    let run = |program: &str, args: &[&str]| -> Result<()> {
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("run {program}"))?;
        if !output.status.success() {
            bail!(
                "{program} {} failed: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    };
    let id = pane.id.as_str();
    match pane.place {
        Place::Tmux => run("tmux", &["send-keys", "-t", id, "-l", text])?,
        Place::Herdr => run("herdr", &["pane", "send-text", id, text])?,
        Place::Zellij => run("zellij", &["action", "write-chars", "--pane-id", id, text])?,
        Place::Pane | Place::Foreground => bail!("{:?} runs have no pane", pane.place),
    }
    // Agents that take a quick burst of characters for a paste would read an
    // Enter inside that burst as a new line rather than as sending it.
    std::thread::sleep(ENTER_DELAY);
    match pane.place {
        Place::Tmux => run("tmux", &["send-keys", "-t", id, "Enter"]),
        Place::Herdr => run("herdr", &["pane", "send-keys", id, "enter"]),
        Place::Zellij => run("zellij", &["action", "write", "--pane-id", id, "13"]),
        Place::Pane | Place::Foreground => Ok(()),
    }
}

/// The pause between typing a follow-up and pressing Enter.
const ENTER_DELAY: Duration = Duration::from_millis(150);

/// Splits the current tmux pane to the right and types `command` into the new
/// pane's shell. Focus stays on the tool, and the shell outlives the command
/// so its output stays readable.
///
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
/// the new pane's id at once; the interface keeps running.
fn in_herdr_pane(command: &str, cwd: &Path) -> Result<String> {
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
    Ok(pane)
}

/// Opens a zellij pane to the right running `command`, then gives focus back
/// to the tool. zellij keeps the pane after the command exits, with its output
/// and exit code, and Enter runs it again.
///
/// Unlike tmux, the command is not typed into a shell: zellij starts a new
/// pane's terminal after `new-pane` returns, and characters written before it
/// is up are lost.
fn in_zellij_pane(command: &str, cwd: &Path) -> Result<String> {
    let zellij = |args: &[&OsStr]| -> Result<String> {
        let output = Command::new("zellij")
            .args(args)
            .output()
            .context("run zellij")?;
        if !output.status.success() {
            bail!(
                "zellij action {} failed: {}",
                args.get(1)
                    .map_or_else(String::new, |a| a.to_string_lossy().into_owned()),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let args = zellij_new_pane(command, cwd);
    // `new-pane` prints the new pane's id, `terminal_<n>`.
    let pane = zellij(&args.iter().map(OsString::as_os_str).collect::<Vec<_>>())?;
    zellij(&["action", "move-focus", "left"].map(OsStr::new))?;
    Ok(pane)
}

/// Arguments to `zellij` for a pane to the right of this one, in `cwd`,
/// running `command` through `sh` like every other place a run goes.
fn zellij_new_pane(command: &str, cwd: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["action", "new-pane", "--direction", "right", "--cwd"]
        .map(OsString::from)
        .to_vec();
    args.push(cwd.as_os_str().to_owned());
    args.extend(["--", "sh", "-c", command].map(OsString::from));
    args
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

/// Quitting unau mid-run stops the run rather than orphaning it.
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
    fn auto_prefers_herdr_then_tmux_then_zellij_and_forced_places_must_exist() {
        let none = Multiplexers::default();
        let tmux = Multiplexers { tmux: true, ..none };
        let zellij = Multiplexers {
            zellij: true,
            ..none
        };
        let all = Multiplexers {
            herdr: true,
            tmux: true,
            zellij: true,
        };
        assert_eq!(place(RunIn::Auto, none), Ok(Place::Foreground));
        assert_eq!(place(RunIn::Auto, tmux), Ok(Place::Tmux));
        assert_eq!(place(RunIn::Auto, zellij), Ok(Place::Zellij));
        assert_eq!(
            place(
                RunIn::Auto,
                Multiplexers {
                    herdr: false,
                    ..all
                }
            ),
            Ok(Place::Tmux)
        );
        assert_eq!(place(RunIn::Auto, all), Ok(Place::Herdr));
        assert_eq!(place(RunIn::Tmux, all), Ok(Place::Tmux));
        assert_eq!(place(RunIn::Zellij, all), Ok(Place::Zellij));
        assert_eq!(place(RunIn::Foreground, all), Ok(Place::Foreground));
        assert!(
            place(RunIn::Zellij, tmux)
                .unwrap_err()
                .contains("not a zellij pane")
        );
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

    #[test]
    fn a_zellij_pane_opens_to_the_right_in_the_repository_running_the_command_through_sh() {
        let args = zellij_new_pane("claude 'fix {x}'", Path::new("/work/my repo"));
        let args: Vec<_> = args.iter().map(|a| a.to_string_lossy()).collect();
        assert_eq!(
            args,
            [
                "action",
                "new-pane",
                "--direction",
                "right",
                "--cwd",
                "/work/my repo",
                "--",
                "sh",
                "-c",
                "claude 'fix {x}'"
            ]
        );
    }

    #[test]
    fn a_zellij_pane_counts_as_open_only_while_its_command_runs() {
        let json = r#"[
            {"id": 1, "is_plugin": true, "exited": false, "is_held": false},
            {"id": 1, "is_plugin": false, "exited": false, "is_held": false},
            {"id": 2, "is_plugin": false, "exited": true, "is_held": true}
        ]"#;
        assert!(zellij_pane_open(json, "terminal_1"));
        assert!(!zellij_pane_open(json, "terminal_2"), "exited and held");
        assert!(!zellij_pane_open(json, "terminal_3"), "not listed");
        assert!(!zellij_pane_open(json, "plugin_1"));
        assert!(!zellij_pane_open("not json", "terminal_1"));
    }

    /// Runs against a private tmux server, so no real session is touched.
    #[test]
    fn tmux_split_opens_a_pane_in_the_repository_and_runs_the_command() {
        if Command::new("tmux").arg("-V").output().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let socket = format!("unau-test-{}", std::process::id());
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
