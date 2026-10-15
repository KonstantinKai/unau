# revmark — design

Copied from `~/Dev/rnd/ideas/revmark.md` on 2026-10-14; this copy is the one that gets edited. Name `revmark` (crates.io free, GitHub quiet). `revu` was the runner-up: free on crates.io, shadowed by `eddmann/revu`, a desktop tool with the same pitch.

Grilled 2026-10-14; decisions below are KK's.

## The itch

Agent writes code. I review it in a terminal the way I review in lazygit: tree of changed files, diff pane, select lines, stage what's right. For what's wrong I want to mark the lines with feedback, or hand over replacement code, and end up with **one file** I can point an agent at: "fix every open item in this". Then re-open the same review, see what the agent changed, accept or push back.

reslop does the second half but does twelve other things (dashboard, npm, branches, agents, deps) and its DX didn't fit me. critique (1.3K★, Bun) is a viewer with no annotations. annodiff (Rust, Oct 2026) is the closest: tree, line select, comments, `$EDITOR`, send to Codex. Tried it, it's good, it's 60% of this. Considered forking it; **decision (same day): write our own, annodiff as the reference.** A fork would mean splitting a 2.4K-line `app.rs`, deleting the Codex layer and replacing the JSON state model anyway; cleaner to start from the spec and borrow the ideas and the rendering tricks by reading, not by inheriting. Keep annodiff installed next to it for side-by-side comparison while building.

## Decisions (grill, 2026-10-14)

1. **Scope**: review surface only. Diff tree, line staging, annotations, review file, run, re-review. No dashboard, no commit, no branches, no npm, no PR URLs.
2. **Input**: working tree vs HEAD by default (staged + unstaged + untracked, like annodiff); `revmark <ref>` and `revmark <a>..<b>` for commit/range from day one, same engine. Staging keys disabled in range mode.
3. **Annotations**: range comment, file comment, general comment, suggestion (range + replacement block). Kind on each: `fix` (default) | `question`.
4. **Text entry**: a popup floating under the commented lines (above them when there's no room), multi-line with `Alt-Enter` (`Ctrl-j` as a fallback; tmux and Herdr usually take it for panes); it started as a single-line prompt at the bottom and moved next to the code so the lines stay in view; `$EDITOR` for suggestions and for any comment you expand. Suggestion temp file is prefilled with the selected lines.
5. **Output**: `.revmark/YYYY-MM-DD-NN.md` (the folder was `.review/` at first and became `.revmark/` so it can also hold the repository's `instructions.md`), written automatically on every change (no save key). The tool never touches `.gitignore`; do it by hand. `--out -` prints to stdout on exit; the interface draws on stderr so the redirect stays clean Markdown.
6. **Staged = accepted.** Staged hunks don't appear in the file. Unannotated unstaged hunks don't either. The file is the to-do list, not a transcript. In the working tree each file shows its unstaged lines, then its staged lines under a `staged` strip; only unstaged lines take annotations.
7. **Run**: `r` runs a configured command with `{file}` substituted. Under Herdr (`HERDR_ENV=1`) it opens a new Herdr pane; under tmux (`TMUX` set) a new tmux pane to the right, focus staying on revmark, with the command typed into the pane's shell so its output survives; otherwise the interface steps aside and the command runs in the foreground, the same way `$EDITOR` does. `[run] in` forces one of them; with both multiplexers around, `auto` picks Herdr since the environment can't tell which is innermost. `in = "pane"` (opt-in) runs the command in the background and streams its output into a pane under the diff: for non-interactive agent modes. An embedded terminal for interactive agents was considered and rejected: it is a terminal multiplexer, which tmux and Herdr already are, and it would need child processes inside the state.
8. **Re-review**: a `[x]` item shows as done with its quoted lines against what is there now, whether it was ticked during the session or the review was reopened with `--review`; `a` accepts (stages the touched hunk, item leaves the file), `c` pushes back with a new open item. A file whose blob differs from the one in `files:` gets a Δ badge. Review is finished when zero open items and everything is staged.
9. **Stack**: Rust 2024 + ratatui + crossterm, shell out to `git` (annodiff already does). Line staging via `git apply --cached` on a synthesised patch, lazygit's method.
10. **No mouse. Tree view with fold/unfold** (annodiff has it, including chained single-child dirs).
13. **Borrow from annodiff, by design not by code** (KK's list after trying it):
    - `z` **expand nearby**: hunks show only changed lines plus minimal context, with a dim `··· unchanged lines omitted · z: expand nearby ···` separator above and below; `z` on the separator pulls in more of the real file. Diff stays dense, full file is one key away.
    - **Zoom**: maximise the diff pane (`+` / `_` to grow and shrink, one key to toggle full-width). Files pane comes back on demand.
    - **Unified / side-by-side toggle** in the diff header, same keys in both.
    - **Dual line-number gutter**: old and new numbers side by side (`264 264`, `267  -`, ` 267 +`), so an anchor in the review file is readable against either side. Our anchors use the new side; the gutter shows both.
    - Header line `[2] path · n/N · <mode>`: the key that focuses the diff, path, file n of N, mode in one strip. Every pane title starts with its focus key: `[1] files`, `[2]` diff, `[3] comments`.
    - Syntect highlighting with a muted palette; selected line is a solid highlight band, removed lines dark red, added lines dark green, context plain. Keep that restraint.
11. **Pagers (delta, diff-so-fancy)**: parked. Own renderer (syntect) in v1.
12. **Home**: `~/Dev/revmark`, cavekit workflow, crates.io + `cargo binstall`, Homebrew tap later.

## Review file format

````md
---
tool: revmark 0.1
status: open            # open | in-progress | partial | done   (agent flips it)
base: HEAD              # or abc123..def456
files:                  # blob SHAs per annotated file at review time, for re-review
  apps/x/y.ts: 3f2a…
---

# Review 2026-10-14-00

> **Agent instructions**
>
> - Before you start, set `status: in-progress`.
> - Work through every unchecked item. Before working on one, mark it `[~]`, so the reviewer can follow along; mark it `[x]` only when the change is made and tests pass.
> - …

## General

- [ ] fix: split the boundary test into one file per rule

## apps/creator-admin/tests/comments-boundary.test.ts

- [ ] fix `:42-48` — walks node_modules too, filter it
  ```ts
  for (const entry of readdirSync(at)) {
    const full = path.join(at, entry);
  ```
- [ ] question `:61` — is `>10` a real floor or a guess?
- [ ] suggestion `:70-72` — replace with
  ```ts
  expect(offenders).toEqual([]);
  ```
  in place of
  ```ts
  expect(offenders.length).toBe(0);
  ```
- [ ] fix `:80` — why was the guard removed?
  ```diff
  -if (!root) return;
  ```
- [ ] fix (file) — no JSDoc on exported helpers
````

Anchors are `path:start-end` on the new side (`:n` for one line), with the exact lines quoted so the agent re-finds them after drift. A suggestion quotes the lines it replaces after `in place of`, so re-review can find them too. A selection of removed lines only is quoted as a `diff` block of `-` lines, anchored to the new line the removal sits before. Fences grow longer than any backtick run inside them. Quotes longer than `[review] quote_lines` (default 12) keep their first and last 3 lines with a `⋯ N more lines ⋯` marker between: the agent reads the file anyway, and the ends are enough to find the place again. Suggestions are never capped, since the replacement is applied against them. Frontmatter `files:` blob SHAs tell re-review which annotated files changed since.

Re-finding an anchor tries, in order: the anchor itself, the quoted lines anywhere in the file (nearest wins), the first or last quoted line alone, and finally the anchor itself when an unstaged change lands on it, which covers an agent rewriting every quoted line in place. Anything else is "anchor lost".

Agent instructions sit in a blockquote under the title: it marks them off from the items for the agent and for the parser, allows lists and blank lines, and renders as a shaded box for humans. No bold-caps "IMPORTANT": position and a clear boundary do the work for current models. The text comes from `.revmark/instructions.md` in the repository, else `instructions.md` next to the user config, else a built-in default that explains anchors, `question` and `suggestion` items, and what the agent may edit. A file replaces the levels below it unless it has a `{inherit}` line, which is replaced by the next level's text (repository → global → default), so a repository can add a line or two without copying, and freezing, the rest. It is copied in when the review file is created, so changing the instructions never changes a review an agent may already have read. `i` opens the file in effect. Reviews written before the block existed, with an `Agent instructions:` paragraph, still parse and are rewritten in the new shape.

## Keys

```
1/2/3/4 focus files/diff/comments/run   j/k move   J/K block of changes (stops even inside one staging hunk)   gg/G   h/l or Tab pane   Enter fold (files) / expand (diff)   W/E fold/unfold all   - flat tree   L comments pane (under files)
C-e/C-y scroll a line   C-d/C-u half page   C-f/C-b page   (both panes, as in nvim)
v select   s stage    u unstage   d discard (asks)   (diff: selection or hunk; files: whole file or every file under a directory)   gs/gu/gd whole file from anywhere
c comment  S suggestion ($EDITOR)   C file comment   n general
e edit     x delete    q toggle fix/question on the annotation under cursor   ? help
z expand nearest hidden lines (from any row, like annodiff)   +/_ grow/shrink   Z zoom   t unified/side by side
r run      , edit config (reloads on save)   i edit agent instructions   o open file in $EDITOR   a accept (re-review)   Q quit (asks in a popup unless [ui] confirm_quit = false)
```

Config `$XDG_CONFIG_HOME/revmark/config.toml` (else `~/.config/revmark/config.toml`) + `.revmark.toml` in the repo: keys remappable, `[run] command`, `[run] output = "claude-stream"` (a display filter for the run pane: Claude's `stream-json` events become one short line each, anything that is not an event passes through; the user's command carries the flags, so the tool still knows nothing about agents beyond a command line), editor from `VISUAL`/`EDITOR`. `o` opens the file under review in it at the cursor line, passing the line in whatever form the editor is known to take (`+N`, `path:N`, `--goto path:N`) and only the path to editors it does not recognise.

Reviews live in `<repo>/.revmark/` unless `[review] dir` (user config or the repository's `.revmark.toml`) names another folder: relative to the repository root, `~/…`, or absolute, with `{repo}` for the repository's folder name so one global setting keeps repositories apart. It is resolved once at start; a folder inside the repository is kept out of the diff. The repository's `instructions.md` stays in `<repo>/.revmark/` either way.

The last view of each repository (open file and new-side cursor line, focused pane, comments pane, folded directories, flat tree, files pane width, zoom, split) is saved on quit to `$XDG_STATE_HOME/revmark/sessions/<FNV-1a of the repo root>.toml` and applied on the next start. It is view state, not review state, so it lives in the user's state directory rather than in `.revmark/` or the review file: it never shows in a diff or a commit. The comments pane and the last run's output (as shown, with its exit code and time) are restored only when an existing review is continued. `AppState` produces and accepts a `Session` value; `main` does the file I/O.

Themes follow yazi's model: one named theme for dark and one for light (`[theme] dark`, `light`, `mode = auto|light|dark`), per-colour overrides in `[theme.colors]` on top. Built in: `catppuccin.macchiato` and `catppuccin.latte` (the default pair, matching nvim and Ghostty), plus `default.dark` and `default.light`. Each theme brings its own syntax colours; Catppuccin uses the official tmTheme files from `catppuccin/bat`, bundled with their MIT notice. Colours map to Catppuccin's style guide: lavender active border, overlay0 inactive, overlay2 selection, green/red diff, peach hunk header. Diff background tints are blended over the base colour ahead of time, since a terminal has no transparency.

User themes are folders in `themes/` next to `config.toml`, named after the folder: a `theme.toml` with `extends` (a built-in or another user theme, `default.dark` if omitted), `syntax` (a `.tmTheme` inside the folder or a syntect theme name) and the `[colors]` slots it changes. A folder named after a built-in theme replaces it, and may extend the original by its own name. Chains that loop, unknown slots or colours, `.tmTheme` paths that leave the folder, and unreadable syntax files stop revmark at startup with the theme named. Everything is parsed once at startup, never per file.

## annodiff as reference — what to read before writing each part

annodiff at 2026-10-06: 6.5K LOC Rust, 8 files, one author. Deps: ratatui 0.30, crossterm, syntect, ratatui-textarea, serde/toml, tungstenite (Codex only). Local clone at `~/Dev/annodiff`; re-clone from GitHub if it's missing, it's public.

| Our module | Read in annodiff | Take the idea, not the code |
|---|---|---|
| `diff/` row model, wrap, split view, hunk nav, `expand_near` | `src/diff.rs` (765) | the display-row ↔ source-line mapping (`visual_for_source`, `locate`, `selectable`) is the one piece worth copying almost verbatim; it's the hard math |
| `ui/` drawing, palette, gutter | `src/render.rs` (1439), `src/palette.rs` | layout proportions, the omitted-lines separator, dual gutter |
| `tree/` with folding, chained dirs | `src/app.rs` `FileRow`, tree code | chain rule: collapse `aaa/bbb/ccc/` when each level has one child and no files |
| `git/` snapshot, parse, expand file, commits | `src/review.rs` lines 415–860 | shelling out to `git` works; add `git apply --cached` for staging |
| `editor` suspend/resume | `src/main.rs` `suspend()` | restore terminal on drop |
| review model | nothing | ours is the Markdown file; annodiff's JSON `annodiff.json` + `--prompt` is the shape we rejected |
| agent | nothing | `agent.rs` is Codex-only websocket; we have `[run] command` |

Greenfield layout:

```
src/
  main.rs          CLI: [path | ref | a..b], --out, --version
  app/             state machine: mode (browse | select | annotate | rereview), effects
  git/             diff snapshot, file expand, apply --cached, blob SHAs
  diff/            parse, rows, wrap, split, hunk nav, expand nearby
  review/          Markdown model: frontmatter, items, kinds, anchors; load/save; rereview diff
  tree/            file tree, fold state, chained dirs, filters
  ui/              ratatui widgets: tree pane, diff pane, comment popup, help, palette
  run.rs           configured command, Herdr pane detection
  config.rs        toml: keys, run, theme, editor
tests/             engine tests on fixture repos; no terminal harness in v1
```

## Milestones

| M | Deliverable | Evenings |
|---|---|---|
| M0 | Crate skeleton, CLI, `git/` snapshot + parse, `diff/` rows (unified), tree pane with folding, `j/k`, `z` expand nearby, zoom. Read-only viewer that already feels right. | 4 |
| M1 | Review file: Markdown model with frontmatter, items, kinds; auto-write; range/file/general comments via prompt line; `--out -` | 3 |
| M2 | Line staging (`v`/`s`/`u`/`d`) via `git apply --cached`, staged hunks hidden from the review; range mode `a..b`; side-by-side toggle | 3 |
| M3 | Suggestions via `$EDITOR` with prefilled lines | 1 |
| M4 | `r` run command, Herdr pane, config file with remappable keys | 1 |
| M5 | Re-review: reopen file, done items with agent diff, `a` accept = stage, `c` reopen | 3 |
| M6 | Theme, docs, crates.io release | 2 |

~17 evenings to a daily-driver, three more than the fork would have cost, with no inherited monolith. M0–M2 is the point where it already beats reslop and annodiff for me.

## Parked

- Pagers (delta, diff-so-fancy) in the read-only view, lazygit's model: pager for viewing, own renderer when selecting. Decide after living with syntect.
- GitHub/GitLab PR input.
- Mouse.
- Name: `revmark` vs `revu` vs other.

## Prior art (checked 2026-10-14)

| Tool | Stack | Has | Lacks |
|---|---|---|---|
| [annodiff](https://github.com/MelsRoughSketch/annodiff) (crates.io, Oct 2026) | Rust + ratatui | tree w/ folding, line select, comments, `$EDITOR`, save, send to Codex | staging, suggestions, agent-agnostic file, status lifecycle, re-review, run |
| [critique](https://github.com/remorses/critique) 1.3K★ | Bun | beautiful viewer, PR URLs, web share, watch | any annotation |
| [hunkr](https://github.com/wvvb/hunkr) 2★ | Go | "terminal PR review for humans and agents" | tiny, PR-shaped |
| [reslop](https://github.com/tshemsedinov/reslop) | Node | everything incl. `.review/*.md` with checkboxes + status, which KK already fed to `/backprop` | focus; DX |
| [eddmann/revu](https://github.com/eddmann/revu) 32★ | TS desktop | inline commenting for agent workflows | TUI |

The `.review/YYYY-MM-DD-NN.md` + frontmatter `status` + `- [ ]` item shape is borrowed from reslop on purpose: the `/backprop check .review/… fix each item` habit carries over unchanged.
