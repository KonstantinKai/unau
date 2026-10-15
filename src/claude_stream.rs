//! Claude Code's `--output-format stream-json` events, one JSON object per
//! line, turned into short lines a person can follow in the run pane. Lines
//! that are not JSON pass through untouched, so any other agent still shows.

use crate::runline::RunLine;
use serde_json::Value;
use std::fmt::Write as _;

/// Longest subject shown after a tool name.
const SUBJECT_CHARS: usize = 120;

/// Formatter state carried between lines: the working directory from the
/// first event, to show paths relative to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeStream {
    cwd: Option<String>,
}

impl ClaudeStream {
    /// The lines to show for one line of output; empty for events not worth
    /// showing.
    pub fn format(&mut self, line: &str) -> Vec<RunLine> {
        let Ok(Value::Object(event)) = serde_json::from_str::<Value>(line) else {
            return vec![RunLine::Plain(line.to_owned())];
        };
        let text = |key: &str| event.get(key).and_then(Value::as_str);
        match text("type") {
            Some("system") if text("subtype") == Some("init") => {
                self.cwd = text("cwd").map(str::to_owned);
                vec![RunLine::Status(match text("model") {
                    Some(model) => format!("· started · {model}"),
                    None => "· started".to_owned(),
                })]
            }
            Some("assistant") => blocks(&event)
                .flat_map(|block| self.assistant_block(block))
                .collect(),
            Some("user") => blocks(&event).filter_map(tool_error).collect(),
            Some("result") => vec![RunLine::Status(result(&event))],
            _ => Vec::new(),
        }
    }

    fn assistant_block(&self, block: &Value) -> Vec<RunLine> {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => block
                .get("text")
                .and_then(Value::as_str)
                .map(prose)
                .unwrap_or_default(),
            Some("tool_use") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                let subject = block
                    .get("input")
                    .map(|input| self.subject(input))
                    .unwrap_or_default();
                vec![RunLine::Tool {
                    name: name.to_lowercase(),
                    subject,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// What a tool call is about, in the fewest words its input offers.
    fn subject(&self, input: &Value) -> String {
        let field = |key: &str| input.get(key).and_then(Value::as_str);
        let subject = if let Some(path) = field("file_path").or_else(|| field("path")) {
            self.relative(path)
        } else if let Some(command) = field("command") {
            self.command(command.lines().next().unwrap_or_default())
        } else {
            ["pattern", "url", "query", "description", "prompt"]
                .iter()
                .find_map(|key| field(key))
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned()
        };
        shorten(&subject)
    }

    /// A shell command without the `cd` into the project it usually starts
    /// with, and with the project's own paths made relative.
    fn command(&self, command: &str) -> String {
        let Some(cwd) = self.cwd.as_deref() else {
            return command.to_owned();
        };
        let mut rest = command;
        for quoted in [cwd.to_owned(), format!("\"{cwd}\""), format!("'{cwd}'")] {
            for joint in [" && ", "; ", " ; "] {
                if let Some(after) = rest.strip_prefix(&format!("cd {quoted}{joint}")) {
                    rest = after;
                }
            }
        }
        rest.replace(&format!("{cwd}/"), "")
    }

    fn relative(&self, path: &str) -> String {
        self.cwd
            .as_deref()
            .and_then(|cwd| path.strip_prefix(cwd))
            .map_or(path, |rest| rest.trim_start_matches('/'))
            .to_owned()
    }
}

/// The content blocks of an `assistant` or `user` event.
fn blocks(event: &serde_json::Map<String, Value>) -> impl Iterator<Item = &Value> {
    event
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// An agent's words, line by line: Markdown, except inside code fences,
/// whose fence lines are dropped.
fn prose(text: &str) -> Vec<RunLine> {
    let mut code = false;
    text.trim_end()
        .lines()
        .filter_map(|line| {
            if line.trim_start().starts_with("```") {
                code = !code;
                return None;
            }
            Some(if code {
                RunLine::Code(line.to_owned())
            } else {
                RunLine::Prose(line.to_owned())
            })
        })
        .collect()
}

/// A failed tool call, by the first line of what it said.
fn tool_error(block: &Value) -> Option<RunLine> {
    if block.get("type")?.as_str()? != "tool_result" || block.get("is_error")? != true {
        return None;
    }
    let content = block.get("content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .find_map(|p| p.get("text").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    };
    Some(RunLine::Error(format!(
        "  ✗ {}",
        shorten(text.lines().next().unwrap_or_default())
    )))
}

fn result(event: &serde_json::Map<String, Value>) -> String {
    let failed = event.get("is_error") == Some(&Value::Bool(true));
    if failed {
        let why = event
            .get("subtype")
            .and_then(Value::as_str)
            .unwrap_or("error");
        return format!("· failed · {why}");
    }
    let mut line = "· finished".to_owned();
    if let Some(turns) = event.get("num_turns").and_then(Value::as_u64) {
        let _ = write!(line, " · {turns} turns");
    }
    if let Some(cost) = event.get("total_cost_usd").and_then(Value::as_f64) {
        let _ = write!(line, " · ${cost:.2}");
    }
    line
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= SUBJECT_CHARS {
        return text.to_owned();
    }
    let cut: String = text.chars().take(SUBJECT_CHARS - 1).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines as they read on screen, Markdown markers gone.
    fn run(lines: &[&str]) -> Vec<String> {
        let mut stream = ClaudeStream::default();
        lines
            .iter()
            .flat_map(|l| stream.format(l))
            .map(|line| line.shown().segs.into_iter().map(|(t, _)| t).collect())
            .collect()
    }

    const INIT: &str = r#"{"type":"system","subtype":"init","cwd":"/work/app"}"#;

    #[test]
    fn shell_commands_drop_the_cd_into_the_project() {
        let lines = run(&[
            INIT,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"cd /work/app && sed -n 1,9p /work/app/src/a.rs"}}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"cd /work/app; ls packages"}}]}}"#,
        ]);
        assert_eq!(
            lines[1..],
            ["· bash sed -n 1,9p src/a.rs", "· bash ls packages"]
        );
    }

    #[test]
    fn code_fences_in_the_agent_text_are_shown_as_code() {
        let mut stream = ClaudeStream::default();
        let lines = stream.format(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Run:\n```sh\npnpm test\n```\n**Done**"}]}}"#,
        );
        assert_eq!(
            lines,
            [
                RunLine::Prose("Run:".into()),
                RunLine::Code("pnpm test".into()),
                RunLine::Prose("**Done**".into()),
            ]
        );
    }

    #[test]
    fn a_session_reads_as_a_short_log() {
        let lines = run(&[
            r#"{"type":"system","subtype":"init","cwd":"/work/app","model":"claude-opus-5-5","tools":[]}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Looking at the file.\n"},{"type":"tool_use","name":"Read","input":{"file_path":"/work/app/src/a.rs"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"fine","is_error":false}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"pnpm test\n--watch=false","description":"run tests"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"Exit code 1\nmore"}],"is_error":true}]}}"#,
            r#"{"type":"stream_event","event":{}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"num_turns":7,"total_cost_usd":0.4213,"result":"Done."}"#,
        ]);
        assert_eq!(
            lines,
            [
                "· started · claude-opus-5-5",
                "Looking at the file.",
                "· read src/a.rs",
                "· bash pnpm test",
                "  ✗ Exit code 1",
                "· finished · 7 turns · $0.42",
            ]
        );
    }

    #[test]
    fn plain_lines_pass_through_and_failures_say_so() {
        assert_eq!(
            run(&[
                "warning: something on stderr",
                "[1, 2]",
                r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#,
            ]),
            [
                "warning: something on stderr",
                "[1, 2]",
                "· failed · error_max_turns"
            ]
        );
    }

    #[test]
    fn long_subjects_are_cut() {
        let long = "x".repeat(300);
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Grep","input":{{"pattern":"{long}"}}}}]}}}}"#
        );
        let shown = &run(&[&line])[0];
        assert!(
            shown.ends_with('…') && shown.chars().count() < 140,
            "{shown}"
        );
    }
}
