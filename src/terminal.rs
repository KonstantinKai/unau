//! Raw mode and the alternate screen. Release builds abort on panic, so
//! destructors never run then: a panic hook restores the terminal instead.
//!
//! The interface draws on stderr, so `--out -` can send the review down
//! stdout into a file or a pipe while the screen stays on the terminal.

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute, queue,
    terminal::{
        BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};
use std::io::{self, Stderr};

pub type Tui = Terminal<CrosstermBackend<Stderr>>;

/// Installs the restoring panic hook, then takes over the terminal. The hook
/// goes in first so a panic during setup still leaves a usable shell.
pub fn start() -> io::Result<Tui> {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = leave();
        previous(info);
    }));
    enter()?;
    Terminal::new(CrosstermBackend::new(io::stderr()))
}

pub fn enter() -> io::Result<()> {
    enable_raw_mode()?;
    // The cursor goes before anything is drawn; otherwise it is seen running
    // across the first frame.
    execute!(
        io::stderr(),
        EnterAlternateScreen,
        Hide,
        EnableBracketedPaste
    )
}

/// Draws one frame as a synchronized update, so terminals that support it
/// show the frame whole instead of line by line; others ignore the markers.
pub fn draw(tui: &mut Tui, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
    queue!(tui.backend_mut(), BeginSynchronizedUpdate)?;
    tui.draw(render)?;
    execute!(tui.backend_mut(), EndSynchronizedUpdate)
}

pub fn leave() -> io::Result<()> {
    let raw = disable_raw_mode();
    execute!(
        io::stderr(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    )?;
    raw
}

/// Gives the terminal to another program, then takes it back.
pub fn suspend<T>(tui: &mut Tui, run: impl FnOnce() -> T) -> io::Result<T> {
    leave()?;
    let result = run();
    enter()?;
    tui.clear()?;
    Ok(result)
}

/// Restores the terminal when `main` returns early with an error.
pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = leave();
    }
}
