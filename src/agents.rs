//! Finding an agent session the user already started in a multiplexer pane,
//! so `[run] send` can go to it instead of a new agent.
//!
//! unau still knows nothing about agents: the agent is whatever program
//! `[run] command` starts, by the name its first word gives. A pane counts
//! when that program runs in it, in this repository, and it is not unau's own
//! pane. zellij says neither what runs inside a shell pane nor its process,
//! so it has no such sessions to find.

use crate::run::{AgentPane, Place};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// A pane running the agent, with a label to tell it apart in a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub pane: AgentPane,
    pub label: String,
}

/// The program `command` starts: its first word that is not a `NAME=value`
/// assignment, without its folder. `claude 'fix {file}'` → `claude`.
pub fn agent_name(command: &str) -> Option<String> {
    let word = command.split_whitespace().find(|w| !w.contains('='))?;
    let name = Path::new(word.trim_matches(['\'', '"'])).file_name()?;
    Some(name.to_string_lossy().into_owned())
}

/// Every pane of `place` running `agent` in the repository at `root`.
pub fn find(place: Place, agent: &str, root: &Path) -> Vec<Session> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    match place {
        Place::Tmux => {
            let panes = output(
                "tmux",
                &[
                    "list-panes",
                    "-a",
                    "-F",
                    "#{pane_id}\t#{pane_pid}\t#{pane_current_path}\t#{session_name}:#{window_index}.#{pane_index}\t#{window_name}",
                ],
            );
            let processes = output("ps", &["-A", "-o", "pid=,ppid=,args="]);
            let own = std::env::var("TMUX_PANE").ok();
            match (panes, processes) {
                (Some(panes), Some(processes)) => {
                    tmux_sessions(&panes, &processes, agent, &root, own.as_deref())
                }
                _ => Vec::new(),
            }
        }
        Place::Herdr => output("herdr", &["pane", "list"]).map_or_else(Vec::new, |json| {
            herdr_sessions(
                &json,
                agent,
                &root,
                std::env::var("HERDR_PANE_ID").ok().as_deref(),
            )
        }),
        Place::Zellij | Place::Pane | Place::Foreground => Vec::new(),
    }
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether `dir` is `root` or inside it, after resolving links: tmux reports
/// `/private/tmp/…` where the repository was opened as `/tmp/…`.
fn inside(dir: &str, root: &Path) -> bool {
    let dir = PathBuf::from(dir);
    let dir = dir.canonicalize().unwrap_or(dir);
    dir.starts_with(root)
}

/// Panes from `tmux list-panes -a` (id, shell pid, folder, address, window
/// name, tab-separated) whose shell has `agent` among the processes under it,
/// by the name it was started as: Claude Code's binary is named after its
/// version, so tmux's own `pane_current_command` does not say `claude`.
fn tmux_sessions(
    panes: &str,
    processes: &str,
    agent: &str,
    root: &Path,
    own: Option<&str>,
) -> Vec<Session> {
    // (pid, parent, program name) of every process.
    let table: Vec<(u32, u32, String)> = processes
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let parent = fields.next()?.parse().ok()?;
            let program = Path::new(fields.next()?)
                .file_name()?
                .to_string_lossy()
                .into_owned();
            Some((pid, parent, program))
        })
        .collect();
    let runs_agent = |shell: u32| {
        let mut frontier = vec![shell];
        // Agents run a few levels under the shell at most (a wrapper, a
        // runtime); the bound also stops a malformed table from looping.
        for _ in 0..4 {
            let children: Vec<u32> = table
                .iter()
                .filter(|(_, parent, _)| frontier.contains(parent))
                .map(|(pid, _, _)| *pid)
                .collect();
            if table
                .iter()
                .any(|(pid, _, program)| children.contains(pid) && program == agent)
            {
                return true;
            }
            frontier = children;
        }
        false
    };
    panes
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next()?;
            let shell = fields.next()?.parse().ok()?;
            let dir = fields.next()?;
            let address = fields.next()?;
            let window = fields.next().unwrap_or_default();
            (Some(id) != own && inside(dir, root) && runs_agent(shell)).then(|| Session {
                pane: AgentPane {
                    place: Place::Tmux,
                    id: id.to_owned(),
                },
                label: format!("{agent} in {address} ({window})"),
            })
        })
        .collect()
}

/// Panes from `herdr pane list` that Herdr itself reports as running
/// `agent`, in the repository.
fn herdr_sessions(json: &str, agent: &str, root: &Path, own: Option<&str>) -> Vec<Session> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(panes) = value["result"]["panes"].as_array() else {
        return Vec::new();
    };
    panes
        .iter()
        .filter_map(|pane| {
            let id = pane["pane_id"].as_str()?;
            let runs = pane["agent"].as_str() == Some(agent)
                || pane["agent_session"]["agent"].as_str() == Some(agent);
            let dir = pane["foreground_cwd"]
                .as_str()
                .or_else(|| pane["cwd"].as_str())?;
            if !runs || Some(id) == own || !inside(dir, root) {
                return None;
            }
            let name = pane["label"]
                .as_str()
                .or_else(|| pane["terminal_title_stripped"].as_str())
                .filter(|n| !n.is_empty())
                .unwrap_or(id);
            Some(Session {
                pane: AgentPane {
                    place: Place::Herdr,
                    id: id.to_owned(),
                },
                label: format!("{agent} · {name}"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_is_named_by_the_command_s_first_program() {
        assert_eq!(
            agent_name("claude 'fix every open item in {file}'").as_deref(),
            Some("claude")
        );
        assert_eq!(
            agent_name("DEBUG=1 /opt/bin/codex exec {file}").as_deref(),
            Some("codex")
        );
        assert_eq!(agent_name("   "), None);
    }

    #[test]
    fn tmux_panes_count_when_the_agent_runs_under_their_shell_in_this_repository() {
        let root = Path::new("/work/shop");
        let panes = "%1\t100\t/work/shop\tmain:0.0\tcode\n\
                     %2\t200\t/work/shop/src\tmain:0.1\tcode\n\
                     %3\t300\t/work/other\tmain:1.0\tother\n\
                     %4\t400\t/work/shop\tmain:2.0\tshell\n";
        // %1 runs the agent through a wrapper, %2 directly (as its version-
        // named binary started as `claude`), %3 elsewhere, %4 only a shell.
        let processes = "  100     1 -zsh\n\
                         \x20 101   100 node /usr/lib/wrapper\n\
                         \x20 102   101 claude --resume\n\
                         \x20 200     1 -zsh\n\
                         \x20 201   200 /home/me/.local/bin/claude\n\
                         \x20 300     1 -zsh\n\
                         \x20 301   300 claude\n\
                         \x20 400     1 -zsh\n\
                         \x20 401   400 vim notes.md\n";
        let found = tmux_sessions(panes, processes, "claude", root, None);
        let ids: Vec<&str> = found.iter().map(|s| s.pane.id.as_str()).collect();
        assert_eq!(ids, ["%1", "%2"]);
        assert_eq!(found[0].label, "claude in main:0.0 (code)");
        let others = tmux_sessions(panes, processes, "claude", root, Some("%1"));
        assert_eq!(others.len(), 1, "unau's own pane is never a match");
    }

    #[test]
    fn herdr_panes_count_when_herdr_reports_the_agent_in_this_repository() {
        let root = Path::new("/work/shop");
        let json = r#"{"result": {"panes": [
            {"pane_id": "w1:p1", "agent": "claude", "cwd": "/work/shop", "label": "opus"},
            {"pane_id": "w1:p2", "agent": "claude", "foreground_cwd": "/work/shop/apps", "terminal_title_stripped": "Fix tests"},
            {"pane_id": "w1:p3", "agent": "codex", "cwd": "/work/shop"},
            {"pane_id": "w2:p1", "agent": "claude", "cwd": "/work/other"},
            {"pane_id": "w1:p4", "cwd": "/work/shop"}
        ]}}"#;
        let found = herdr_sessions(json, "claude", root, Some("w1:p9"));
        let labels: Vec<&str> = found.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["claude · opus", "claude · Fix tests"]);
        assert_eq!(herdr_sessions(json, "claude", root, Some("w1:p1")).len(), 1);
        assert!(herdr_sessions("not json", "claude", root, None).is_empty());
    }
}
