//! What the run pane shows: the output and status of a background run. The
//! process itself lives in the event loop; this is only data.

use super::{AppState, Effect, Pane, Pending};
use crate::{claude_stream::ClaudeStream, config::RunOutput, runline::RunLine};
use std::time::Duration;

/// Output lines kept; older ones scroll away.
pub const RUN_LOG_LINES: usize = 5000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Exited(i32),
    /// The command could not be started.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLog {
    pub command: String,
    pub lines: Vec<RunLine>,
    pub status: RunStatus,
    /// The pane is on screen.
    pub visible: bool,
    /// How long the command has run, or ran; the clock is the event loop's.
    pub elapsed: Duration,
    /// First line shown; `None` follows the newest output.
    pub scroll: Option<usize>,
    /// Turns Claude's stream events into readable lines, when configured.
    pub stream: Option<ClaudeStream>,
}

/// First line to show so the newest lines fill a `width` × `height` view,
/// long lines wrapped the way the pane draws them.
pub fn follow_top(lines: &[RunLine], width: usize, height: usize) -> usize {
    let mut used = 0;
    for (i, line) in lines.iter().enumerate().rev() {
        used += line.rows(width).len().max(1);
        if used > height {
            return i + 1;
        }
    }
    0
}

/// `42s`, `3m 05s`, `1h 02m`.
pub fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, secs % 3600 / 60),
    }
}

impl RunLog {
    pub fn running(&self) -> bool {
        self.status == RunStatus::Running
    }
}

/// A line as a plain-text pane can show it: colour and cursor escapes
/// removed, and only what follows the last carriage return, the way a
/// terminal would leave a progress line.
pub fn clean_line(raw: &str) -> String {
    let line = raw.rsplit('\r').find(|part| !part.is_empty()).unwrap_or("");
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if c == '\t' || !c.is_control() {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            // CSI: parameters until a final byte in @..~.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: until BEL or the string terminator.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

impl AppState {
    /// `r` asked for a background run while one is going: offer to stop it.
    pub fn ask_stop_run(&mut self) {
        self.ask(
            "A command is still running — stop it?".to_owned(),
            Pending::StopRun,
        );
    }

    pub fn run_started(&mut self, command: &str) {
        self.run_log = Some(RunLog {
            command: command.to_owned(),
            lines: Vec::new(),
            status: RunStatus::Running,
            visible: true,
            elapsed: Duration::ZERO,
            scroll: None,
            stream: (self.config.run_output == RunOutput::ClaudeStream).then(ClaudeStream::default),
        });
        self.relayout();
        self.info("running in the run pane; R hides it");
    }

    pub fn run_failed(&mut self, error: String) {
        if let Some(log) = &mut self.run_log {
            log.status = RunStatus::Failed(error.clone());
        }
        self.error(error);
    }

    pub fn run_elapsed(&mut self, elapsed: Duration) {
        if let Some(log) = &mut self.run_log {
            log.elapsed = elapsed;
        }
    }

    pub fn run_output(&mut self, lines: &[String]) {
        let Some(log) = &mut self.run_log else {
            return;
        };
        for line in lines.iter().map(|l| clean_line(l)) {
            match &mut log.stream {
                Some(stream) => log.lines.extend(stream.format(&line)),
                None => log.lines.push(RunLine::Plain(line)),
            }
        }
        let excess = log.lines.len().saturating_sub(RUN_LOG_LINES);
        log.lines.drain(..excess);
        // A fixed view stays on the same text as old lines scroll away.
        log.scroll = log.scroll.map(|top| top.saturating_sub(excess));
    }

    /// The command ended: pick up what it changed.
    pub fn run_finished(&mut self, code: i32) {
        if let Some(log) = &mut self.run_log {
            log.status = RunStatus::Exited(code);
        }
        self.poll_review();
        if let Err(err) = self.refresh() {
            self.error(format!("{err:#}"));
            return;
        }
        if code == 0 {
            self.info("command finished");
        } else {
            self.error(format!("command exited with {code}"));
        }
    }

    pub fn toggle_run_pane(&mut self) -> Effect {
        match &mut self.run_log {
            Some(log) => log.visible = !log.visible,
            None => self.info("nothing has run yet"),
        }
        if self.focus == Pane::Run && !self.run_log.as_ref().is_some_and(|l| l.visible) {
            self.focus_diff_or_files();
        }
        self.relayout();
        Effect::None
    }

    /// Focuses the run pane, showing it first when it is hidden.
    pub fn focus_run(&mut self) {
        let Some(log) = &mut self.run_log else {
            self.info("nothing has run yet");
            return;
        };
        if !log.visible {
            log.visible = true;
            self.relayout();
        }
        self.focus = Pane::Run;
    }

    /// Focus for when the run pane goes away: the diff, else the files.
    fn focus_diff_or_files(&mut self) {
        self.focus = if self.file.is_some() {
            Pane::Diff
        } else {
            Pane::Tree
        };
    }

    /// Width and height of the run pane's text area.
    pub fn run_view_size(&self) -> (usize, usize) {
        let (width, _) = self.diff_size();
        let height = usize::from(self.run_pane_height().saturating_sub(2));
        (width, height.max(1))
    }

    /// Moves the run pane's view by `delta` lines. Moving up stops following
    /// new output; reaching the bottom follows it again.
    pub fn run_scroll_by(&mut self, delta: isize) {
        let (width, height) = self.run_view_size();
        let Some(log) = &mut self.run_log else {
            return;
        };
        let bottom = follow_top(&log.lines, width, height);
        let top = log.scroll.unwrap_or(bottom).saturating_add_signed(delta);
        log.scroll = (top < bottom).then_some(top);
    }

    /// `gg` and `G` in the run pane: the first line, or the newest and follow.
    pub fn run_scroll_to(&mut self, end: bool) {
        if let Some(log) = &mut self.run_log {
            log.scroll = (!end).then_some(0);
        }
    }

    /// Lines the run pane takes from the diff pane, borders included.
    pub fn run_pane_height(&self) -> u16 {
        if !self.run_log.as_ref().is_some_and(|l| l.visible) {
            return 0;
        }
        let main = self.layout.height.saturating_sub(1);
        (main * RUN_PANE_PERCENT / 100)
            .max(RUN_PANE_MIN)
            .min(main / 2)
    }
}

/// Share of the screen height the run pane takes, and its smallest height.
pub const RUN_PANE_PERCENT: u16 = 30;
pub const RUN_PANE_MIN: u16 = 5;

#[cfg(test)]
mod tests {
    use super::super::tests::state;
    use super::*;
    use crate::git::fixture::Fixture;

    #[test]
    fn escapes_and_progress_rewrites_are_cleaned() {
        assert_eq!(clean_line("\u{1b}[1;32mok\u{1b}[0m done"), "ok done");
        assert_eq!(clean_line("10%\r50%\r100%"), "100%");
        assert_eq!(clean_line("\u{1b}]0;title\u{7}text"), "text");
        assert_eq!(clean_line("a\tb"), "a\tb");
    }

    #[test]
    fn claude_stream_output_is_shown_as_readable_lines() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        s.config
            .merge("[run]\noutput = \"claude-stream\"\n")
            .unwrap();
        s.run_started("claude -p --output-format stream-json --verbose");
        s.run_output(&[
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"a.txt"}}]}}"#.to_owned(),
            "plain stderr".to_owned(),
        ]);
        assert_eq!(
            s.run_log.as_ref().unwrap().lines,
            [
                RunLine::Tool {
                    name: "edit".into(),
                    subject: "a.txt".into()
                },
                RunLine::Plain("plain stderr".into())
            ]
        );
    }

    #[test]
    fn elapsed_time_reads_in_the_largest_two_units() {
        assert_eq!(format_elapsed(Duration::from_secs(42)), "42s");
        assert_eq!(format_elapsed(Duration::from_secs(185)), "3m 05s");
        assert_eq!(format_elapsed(Duration::from_secs(3720)), "1h 02m");
    }

    #[test]
    fn following_keeps_wrapped_newest_lines_in_view() {
        let lines = ["a", "b", "0123456789"].map(|l| RunLine::Plain(l.to_owned()));
        // The long line takes three rows at width 4, so only it fits in 3.
        assert_eq!(follow_top(&lines, 4, 3), 2);
        assert_eq!(follow_top(&lines, 4, 4), 1);
        assert_eq!(follow_top(&lines, 40, 10), 0);
    }

    #[test]
    fn the_run_pane_takes_focus_scrolls_and_follows_again_at_the_bottom() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        s.focus = Pane::Diff;
        s.focus_run();
        assert_eq!(s.focus, Pane::Diff, "nothing has run yet");
        s.run_started("agent");
        let lines: Vec<String> = (0..100).map(|n| n.to_string()).collect();
        s.run_output(&lines);
        crate::app::tests::press(&mut s, "4");
        assert_eq!(s.focus, Pane::Run);
        assert_eq!(s.run_log.as_ref().unwrap().scroll, None);
        crate::app::tests::press(&mut s, "kk");
        let (width, height) = s.run_view_size();
        let bottom = follow_top(&s.run_log.as_ref().unwrap().lines, width, height);
        assert_eq!(s.run_log.as_ref().unwrap().scroll, Some(bottom - 2));
        s.run_output(&["more".to_owned()]);
        assert_eq!(s.run_log.as_ref().unwrap().scroll, Some(bottom - 2));
        crate::app::tests::press(&mut s, "gg");
        assert_eq!(s.run_log.as_ref().unwrap().scroll, Some(0));
        crate::app::tests::press(&mut s, "G");
        assert_eq!(s.run_log.as_ref().unwrap().scroll, None);
        crate::app::tests::press(&mut s, "R");
        assert_eq!(s.focus, Pane::Diff);
    }

    #[test]
    fn output_is_kept_up_to_the_limit_and_the_pane_takes_room_from_the_diff() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        let (_, full) = s.diff_size();
        s.run_started("agent");
        let (_, shared) = s.diff_size();
        assert!(shared < full);
        let lines: Vec<String> = (0..RUN_LOG_LINES + 10).map(|n| n.to_string()).collect();
        s.run_output(&lines);
        let log = s.run_log.as_ref().unwrap();
        assert_eq!(log.lines.len(), RUN_LOG_LINES);
        assert_eq!(log.lines[0], RunLine::Plain("10".into()));
        s.toggle_run_pane();
        assert_eq!(s.diff_size().1, full);
        s.run_finished(3);
        assert_eq!(s.run_log.as_ref().unwrap().status, RunStatus::Exited(3));
        assert!(s.message.as_ref().unwrap().text.contains("exited with 3"));
    }
}
