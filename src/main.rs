mod app;
mod claude_stream;
mod cli;
mod config;
mod diff;
mod editor;
mod git;
mod review;
mod run;
mod runline;
mod session;
mod terminal;
mod theme;
mod tree;
mod ui;

use anyhow::Result;
use app::{AppState, EditPurpose, Effect, Store};
use clap::Parser;
use config::{Config, Key};
use crossterm::event::{self, Event, KeyEventKind};
use git::{Target, snapshot};
use review::Review;
use std::{io::Write, path::Path, time::Duration};

/// How often the loop wakes without input to pick up agent edits.
const TICK: Duration = Duration::from_millis(250);

fn main() {
    if let Err(err) = start() {
        eprintln!("revmark: {err:#}");
        std::process::exit(1);
    }
}

fn start() -> Result<()> {
    let args = cli::Args::parse();
    if args.licenses {
        print!("{}", cli::licenses());
        return Ok(());
    }
    let stdout = args.stdout()?;
    let cwd = std::env::current_dir()?;
    let user_config = cli::user_config();
    let load_config =
        |root: &Path| Config::load(user_config.as_deref(), &root.join(".revmark.toml"));
    let newest = if args.resume {
        let repo = git::Repo::discover(&cwd)?;
        review::latest(&load_config(&repo.root)?.review_dir(&repo.root))
    } else {
        None
    };
    let nothing_to_continue = args.resume && newest.is_none();
    // An existing review is being picked up, not a fresh one started.
    let continuing = args.review.is_some() || newest.is_some();
    let (mut repo, target, mut review, store) =
        if let Some(path) = args.review.as_ref().or(newest.as_ref()) {
            let path = cwd.join(path);
            let (review, store) = app::persist_open(path.clone())?;
            // A review kept outside the repository belongs to the current one.
            let repo = match path.parent().map(git::Repo::discover) {
                Some(Ok(repo)) => repo,
                _ => git::Repo::discover(&cwd)?,
            };
            let target = Target::from_base(&review.base);
            (repo, target, review, store)
        } else {
            let (repo, target) = cli::resolve(args.target.as_deref(), args.path.as_deref(), &cwd)?;
            let review = Review::new("", &target.base());
            let store = if stdout {
                Store::Stdout
            } else {
                Store::File {
                    path: None,
                    synced: String::new(),
                }
            };
            (repo, target, review, store)
        };
    let config = load_config(&repo.root)?;
    repo.reviews = config.review_dir(&repo.root);
    // A file review is named when its file is created; a printed one needs a name now.
    if stdout && !continuing {
        review.name = review::next_name(&repo.review_dir(), &review::today());
    }
    let snap = snapshot(&repo, &target)?;
    let dark = dark_background();
    let theme = config.theme(dark);
    let config_files = app::ConfigFiles::new(cli::user_config(), &repo.root, dark);
    let mut state = AppState::new(repo, target, snap, config, review, store);
    state.multiplexers = run::Multiplexers::detect();
    state.layout.theme = theme;
    state.config_files = config_files;
    state.apply_instructions();
    if nothing_to_continue {
        let dir = state.repo.review_dir();
        state.info(format!(
            "no review to continue in {}: starting a new one",
            dir.display()
        ));
    }
    // The first file was highlighted before the theme was known.
    state.reload_file();
    let mut tui = terminal::start()?;
    let guard = terminal::Guard;
    let size = tui.size()?;
    state.resize(size.width, size.height);
    // After the resize, so the restored cursor is scrolled into a real view.
    let session_path = session::path(&state.repo.root);
    if let Some(saved) = session_path.as_deref().and_then(session::load) {
        state.restore(&saved, continuing);
    }
    let result = event_loop(&mut tui, &mut state);
    drop(guard);
    result?;
    if let Some(path) = &session_path
        && let Err(err) = session::save(path, &state.session())
    {
        eprintln!("revmark: session not saved: {err:#}");
    }
    if let Some(text) = matches!(state.store, Store::Stdout)
        .then(|| state.final_review())
        .flatten()
    {
        std::io::stdout().write_all(text.as_bytes())?;
    } else if let Some(path) = state.review_path() {
        eprintln!("review: {}", path.display());
    }
    Ok(())
}

fn event_loop(tui: &mut terminal::Tui, state: &mut AppState) -> Result<()> {
    // A run in the run pane: the process stays here, its output goes to the state.
    let mut background: Option<run::Background> = None;
    loop {
        pump(state, &mut background);
        terminal::draw(tui, |frame| ui::render(state, frame))?;
        if !event::poll(TICK)? {
            if state.poll_review() {
                refresh(state);
            }
            state.poll_config();
            continue;
        }
        let effect = match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                Key::from_event(key).map_or(Effect::None, |k| state.handle_key(k))
            }
            Event::Paste(text) => {
                state.paste(&text);
                Effect::None
            }
            Event::Resize(width, height) => {
                state.resize(width, height);
                Effect::None
            }
            _ => Effect::None,
        };
        match effect {
            Effect::None => {}
            Effect::Quit => return Ok(()),
            Effect::Editor {
                text,
                suffix,
                purpose,
            } => {
                let edited = terminal::suspend(tui, || editor::edit(&text, &suffix))?;
                match edited {
                    Ok(result) => state.finish_edit(purpose, result),
                    Err(err) => {
                        state.error(format!("editor: {err:#}"));
                        if let EditPurpose::Prompt(prompt) = purpose {
                            state.prompt = Some(prompt);
                        }
                    }
                }
            }
            Effect::EditFile(path) => {
                if let Err(err) = terminal::suspend(tui, || editor::open(&path))? {
                    state.error(format!("editor: {err:#}"));
                }
                state.after_edit();
            }
            Effect::OpenFile { path, line } => {
                let opened = terminal::suspend(tui, || editor::open_at(&path, line))?;
                state.poll_review();
                refresh(state);
                if let Err(err) = opened {
                    state.error(format!("editor: {err:#}"));
                }
            }
            Effect::StopRun => {
                if let Some(mut run) = background.take() {
                    run.stop();
                    state.run_finished(-1);
                    state.info("command stopped");
                }
            }
            Effect::Run { command, place } => {
                start_run(tui, state, &mut background, &command, place)?;
            }
        }
    }
}

/// Moves a background run's new output into the state, and finishes it.
fn pump(state: &mut AppState, background: &mut Option<run::Background>) {
    let Some(run) = background else {
        return;
    };
    state.run_elapsed(run.elapsed());
    let lines = run.take_lines();
    if !lines.is_empty() {
        state.run_output(&lines);
    }
    if let Some(code) = run.finished() {
        state.run_output(&run.take_lines());
        *background = None;
        state.run_finished(code);
    }
}

fn start_run(
    tui: &mut terminal::Tui,
    state: &mut AppState,
    background: &mut Option<run::Background>,
    command: &str,
    place: run::Place,
) -> Result<()> {
    let root = state.repo.root.clone();
    let result = match place {
        run::Place::Pane => {
            state.run_started(command);
            match run::Background::start(command, &root) {
                Ok(run) => *background = Some(run),
                Err(err) => state.run_failed(format!("{err:#}")),
            }
            return Ok(());
        }
        run::Place::Herdr => run::in_herdr_pane(command, &root),
        run::Place::Tmux => run::in_tmux_pane(command, &root),
        run::Place::Foreground => terminal::suspend(tui, || run::foreground(command, &root))?,
    };
    match result {
        Ok(()) if place == run::Place::Herdr => state.info("started in a new Herdr pane"),
        Ok(()) if place == run::Place::Tmux => state.info("started in a new tmux pane"),
        Ok(()) => {
            state.poll_review();
            refresh(state);
            state.info("command finished");
        }
        Err(err) => state.error(format!("{err:#}")),
    }
    Ok(())
}

fn refresh(state: &mut AppState) {
    if let Err(err) = state.refresh() {
        state.error(format!("{err:#}"));
    }
}

/// Reads `COLORFGBG` (`fg;bg`), which most terminals set; a background of 7
/// or 15 is light. Anything unknown counts as dark.
fn dark_background() -> bool {
    !std::env::var("COLORFGBG").is_ok_and(|v| matches!(v.rsplit(';').next(), Some("7" | "15")))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    fn sources(dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(fs::read_to_string(path).unwrap());
            }
        }
    }

    #[test]
    fn crate_forbids_unsafe_links_no_git_library_and_never_captures_the_mouse() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("unsafe_code = \"forbid\""));
        assert!(!manifest.contains("git2"));
        let mut code = Vec::new();
        sources(&root.join("src"), &mut code);
        let mouse = concat!("Enable", "MouseCapture");
        assert!(code.iter().all(|c| !c.contains(mouse)));
    }

    #[test]
    fn drawing_code_takes_every_colour_from_the_theme() {
        let mut code = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui"),
            &mut code,
        );
        let literal = concat!("Color", "::");
        assert!(code.iter().all(|c| !c.contains(literal)));
    }
}
