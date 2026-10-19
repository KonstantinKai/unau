# unau — rationale

Why unau is shaped the way it is. What it does and how to use it is in the
README; this file holds only the reasons. This file changes when a decision does, not
with every feature.

## The itch

An agent writes code. I want to review it in a terminal the way lazygit
reviews: a tree of changed files, a diff pane, select lines, stage what is
right. For what is wrong I want to mark the lines with feedback or hand over
replacement code, and end up with **one file** I can point any agent at: "fix
every open item in this". Then reopen the same review, see what the agent
changed, and accept it or push back.

## Prior art

| Tool | Stack | Has | Lacks |
| --- | --- | --- | --- |
| [annodiff](https://github.com/MelsRoughSketch/annodiff) | Rust + ratatui | tree with folding, line select, comments, `$EDITOR`, send to Codex | staging, suggestions, an agent-agnostic file, status lifecycle, re-review, run |
| [critique](https://github.com/remorses/critique) | Bun | a fine viewer, PR URLs, web share, watch | any annotation |
| [hunkr](https://github.com/wvvb/hunkr) | Go | terminal PR review for humans and agents | small, PR-shaped |
| [reslop](https://github.com/tshemsedinov/reslop) | Node | a `.review/*.md` checklist with a status, among a dozen other things | a fit for my preferences and personal workflow |
| [revu](https://github.com/eddmann/revu) | TypeScript desktop | inline commenting for agent workflows | a terminal interface |

annodiff was about 60% of this. Forking it was considered and dropped the
same day: it would have meant splitting a 2,400-line `app.rs`, removing the
Codex layer and replacing its JSON state anyway. unau was written from its own
spec instead, with annodiff read as a reference. Ideas were taken, not code,
with one exception: pairing removed and added lines side by side is adapted
from its `split_rows` (MIT; notice in `assets/notices/`).

The review file's shape (a dated Markdown checklist with a frontmatter
`status`) comes from reslop on purpose, so habits built around it carry over.

## Decisions

1. **Review surface only.** Diff tree, line staging, annotations, the review
   file, run, re-review. No dashboard, commits, branches or PR URLs: other
   tools do those, and each one would blur what this is for.
2. **The working tree by default; commits and ranges from day one.** Staged,
   unstaged and untracked changes are what an agent leaves behind. A commit
   or range uses the same engine read-only, so staging keys are off there.
3. **Staged means accepted.** Anything staged is done, so it never appears in
   the review. The review file is the to-do list, not a transcript; accepting
   a fix in re-review stages it, and a review is finished when nothing is open
   and nothing is left unstaged.
4. **The review file is the state.** Markdown, written on every change, no
   save key, no JSON sidecar or database. Reopening the file gives the review
   back, a person can read and edit it, and any agent that reads text can act
   on it. annodiff's JSON state was the shape rejected.
5. **Anchors on the new side, with the lines quoted.** `path:start-end` names
   lines the agent can open; the quote lets it re-find them after line numbers
   drift, which they always do once it starts editing.
6. **Agent-agnostic.** The tool knows one thing about agents: a command line
   with `{file}` in it. No SDKs, sockets or sessions, so any agent, script or
   person can do the work, and nothing breaks when an agent changes its API.
   Display helpers for one agent's output (Claude's stream) stay optional
   filters on what the command prints.
7. **Run in a pane, not an embedded terminal.** `r` opens a tmux, zellij or
   Herdr pane when unau runs in one, else steps aside for the foreground, or
   streams a non-interactive agent into a run pane. An embedded terminal for
   interactive agents was rejected: that is a terminal multiplexer, which
   tmux, zellij and Herdr already are, and it would put child processes inside
   the state.
8. **`git` shelled out, never linked.** Line staging is `git apply --cached` on
   a patch built for the selection, the way lazygit does it. No `git2`: the
   `git` on the machine already has every behaviour the user expects.
9. **Pure state, pure render.** `AppState` holds no terminal or processes, so
   every behaviour is tested without a terminal; drawing reads state and never
   changes it.
10. **Keyboard only, every key remappable.** No mouse: a review is a sequence
    of small, repeatable moves, and those belong on keys.
11. **Writes stay in the review folder.** Apart from staging, discarding, the
    files a person opens from inside unau, the theme picked with `T` (saved to
    the user's config), and a per-repository view session in the user's state
    directory, unau writes nothing outside its review folder and never
    touches `.gitignore`.
12. **Themes like yazi's.** One named theme for dark backgrounds and one for
    light, overrides per colour on top. Every built-in family is a palette
    mapped onto the interface the way Catppuccin's style guide places colours,
    so they all read the same way. A theme paints its own background: left to
    the terminal, a light theme on a dark terminal is dark text on black.
    Keeping the terminal's background stays one setting away for people who
    want its transparency.
