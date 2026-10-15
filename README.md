# revmark

Terminal code review that ends in a file an agent can act on.

Open the working tree diff (or a commit, or a range) in a lazygit-shaped TUI: a
folding file tree on the left, the diff on the right. Stage the lines that are
right. Mark the lines that aren't with a comment, a question, or replacement
code. Every mark lands in `.revmark/<date>-<n>.md` as it happens, a Markdown
checklist with anchors an agent can follow. Point your agent at the file, let it
work, reopen the review to see what changed, accept or push back.

## Install

```sh
cargo install --path .
```

Needs `git` on `PATH`. revmark shells out to it and links no git library.

## Usage

```sh
revmark                 # working tree vs HEAD: staged, unstaged, untracked
revmark HEAD~1          # one commit
revmark main..HEAD      # a range; staging keys are off in range mode
revmark -- main         # a path called main, not the branch
revmark --review .revmark/2026-10-14-00.md  # reopen a review
revmark --continue      # reopen the newest review; start a new one if there is none
revmark --out -         # print the review on exit instead of writing .revmark/
```

A path that exists wins over a revision with the same name; `--` forces the
path reading.

The working tree view shows each file's unstaged lines and, under a `staged`
strip, the lines already in the index. Staged means accepted: staged lines never
appear in the review, and an annotation on lines you stage leaves the review
(revmark asks first).

In the files pane, each file starts with its git status: `M` modified, `A`
added, `D` deleted, `?` untracked (a rename shows as a `D` and an `A`). After
the name, `●` marks unstaged lines, `✓` staged ones, `✎ n` the annotations, and
`Δ` a file that changed since the review was written. A directory carries `●`
and `✓` for the files under it, so a directory showing only `✓` is fully
staged.

revmark never edits `.gitignore`. Add `.revmark/` to it yourself if you don't
want reviews tracked, or keep reviews out of the repository altogether with
`[review] dir`:

```toml
[review]
dir = "~/reviews/{repo}"   # {repo} is the repository's folder name
```

A relative path starts at the repository root (`dir = "docs/reviews"`), and a
folder inside the repository is kept out of the diff like `.revmark/` is. Set
it in your own config for every repository, or in a repository's
`.revmark.toml` for that one; it takes effect the next time revmark starts.
`--continue` looks there, and the agent gets the full path in `{file}`. A
repository's own `.revmark/instructions.md` stays where it is.

revmark picks up where you left off. On quit it saves the open file and cursor
line, the focused pane, folded directories, the files pane width, zoom and the
unified or side-by-side view, and restores them the next time you open it in
the same repository. The comments pane and the last run's output come back
only when you continue a review (`--continue` or `--review`); a fresh review
starts without them. A run still going when you quit is stopped and comes back
as `exited -1`. Sessions live in
`$XDG_STATE_HOME/revmark/sessions/` (else `~/.local/state/revmark/sessions/`),
one small file per repository and never inside it; delete one to start clean.

## Keys

| Key | Action |
|---|---|
| `j` `k` | next / previous line |
| `J` `K` | next / previous block of changed lines (added or removed), even when it is close enough to the last one to share its hunk |
| `gg` `G` | first / last line |
| `Ctrl-e` `Ctrl-y` | scroll one line; the cursor stays unless it would leave the screen |
| `Ctrl-d` `Ctrl-u` | half a page down / up |
| `Ctrl-f` `Ctrl-b` | a page down / up |
| `h` `l` `Tab` | focus files / diff / cycle in pane-number order: files → diff → comments → run (shown panes only) |
| `1` `2` `3` `4` | focus files / diff / comments / run pane (the last two shown if hidden); each pane's title starts with its number |
| `Enter` | fold a directory, open a file; on a separator, expand |
| `W` `E` | fold / unfold every directory (folding all leaves nested ones folded too) |
| `-` | flat file list |
| `L` | comments pane under the files pane: every comment in one list; moving through it shows each one in the diff, `Enter` jumps there. `L` again hides it |
| `v` | start or end a selection |
| `s` `u` `d` | stage, unstage, discard: in the diff, the selection or the hunk under the cursor; in the files pane, the whole file, or every file under a directory (`d` asks) |
| `gs` `gu` `gd` | stage, unstage, discard the whole open file from any pane (`gd` asks). Staging a whole file or directory (`gs`, or `s` in the files pane) moves on to the next file that still has unstaged changes |
| `c` | comment on the selection or line; on a done item, push back |
| `S` | suggest replacement code in `$EDITOR` |
| `C` | comment on the file |
| `n` | general comment |
| `e` `x` | edit, delete the annotation under the cursor |
| `q` | toggle fix / question |
| `z` | show 10 more hidden lines: from anywhere, the nearest omitted-lines separator grows toward the cursor; on a separator, both its edges grow |
| `+` `_` | grow / shrink the diff pane |
| `Z` | zoom the diff pane |
| `t` | unified / side by side |
| `r` | run the configured command on the review file (with `in = "pane"`: again while running offers to stop it) |
| `R` | show / hide the run pane |
| `4` | focus the run pane, to scroll back through the output |
| `,` | open your config in `$EDITOR` (created with every setting commented out the first time) |
| `i` | open the agent instructions in `$EDITOR` (see below) |
| `o` | open the file in `$EDITOR`: from the diff at the cursor line, from the files pane the file under the cursor; the diff is re-read when the editor closes |
| `a` | accept a done item: stage its hunk, drop it from the review |
| `Q` | quit; asks first in a popup, naming open items never run or a running command (`[ui] confirm_quit = false` to skip) |
| `?` | help |

Comments are typed in a popup that floats just under the lines you selected
(above them when there's no room), so the code stays in view. Its title shows
whether the comment is a `fix` or a `question`; `Tab` switches between them
(editing starts from the item's own kind). `Enter` saves,
`Alt-Enter` starts a new line (`Ctrl-j` too, if your multiplexer doesn't
take it), `Esc` cancels and `Ctrl-e` moves the text into
`$EDITOR`.

## The review file

````md
---
tool: revmark 0.1.0
status: open
base: HEAD
files:
  src/lib.rs: e2f07ff65001d6392e9538f78c5226f82c016b60
---

# Review 2026-10-14-00

> **Agent instructions**
>
> - Before you start, set `status: in-progress`.
> - Work through every unchecked item. Before working on one, mark it `[~]`, so the reviewer can follow along; mark it `[x]` only when the change is made and tests pass.
> - …the rest of the instructions in effect (see "Agent instructions" below)

## General

- [ ] fix: split the boundary test into one file per rule

## src/lib.rs

- [ ] fix `:42-48` — walks node_modules too, filter it
  ```rs
  for entry in read_dir(at)? {
  ```
- [ ] question `:61` — is 10 a real floor or a guess?
  ```rs
  if count > 10 {
  ```
- [ ] suggestion `:70` — replace with
  ```rs
  assert!(offenders.is_empty());
  ```
  in place of
  ```rs
  assert_eq!(offenders.len(), 0);
  ```
- [ ] fix `:80` — why was this removed?
  ```diff
  -validate(input)?;
  ```
- [ ] fix (file) — no docs on exported helpers
````

- Anchors are new-side line numbers, `:start-end` or `:n`. The quoted lines are
  the exact text at review time, so an agent finds them again after the numbers
  drift.
- Long quotes are capped: more than `quote_lines` (default 12) keeps the first
  and last 3 lines with a `⋯ N more lines ⋯` line between. The agent opens the
  file anyway; the ends are enough to find the place. Suggestions are always
  quoted in full.
- A quote of removed lines is a `diff` block of `-` lines, anchored to the new
  line the removal sits before.
- Fences grow longer than any backtick run inside them, so quoted Markdown
  cannot break the file.
- `files:` holds the blob id of each annotated file at review time.
- The agent ticks items and sets `status`. revmark never writes `done`. If the
  agent edits the file while revmark runs, revmark merges: the agent's ticks win
  and revmark's own changes go on top.

## Re-review

Reopen with `--review <file>`. A ticked item shows its quoted lines against
what is there now. `a` stages the hunks that touch it and removes it from the
file. `c` pushes back: the item becomes a new open one with your comment. A
file that changed since the review was written carries a `Δ` in the file tree. When
nothing is open and nothing is left unstaged, revmark says the review is
complete.

If the agent rewrote every quoted line and nothing changed at the anchor,
revmark marks the item "anchor lost"; stage the fix by hand and delete the item.

## Agent instructions

Every review opens with a `> **Agent instructions**` block telling the agent
how to work through the file. The text comes from the first of these that
exists:

| File | Applies to |
|---|---|
| `.revmark/instructions.md` in the repository | this workspace only |
| `~/.config/revmark/instructions.md` | every workspace |
| neither | the built-in default |

Press `i` to edit the file in effect. If there is none yet, revmark creates
the global one with the default text, so there is something to start from. To
give one repository its own instructions, put an `instructions.md` in its
`.revmark/` folder. On its own it replaces the global file and the default.
To add to them instead, put a line `{inherit}` where the inherited text should
go:

```md
{inherit}
- Run `pnpm test` before ticking an item.
```

In a repository file, `{inherit}` brings in the global file (or the default
when there is none); in the global file, it brings in the default. Levels
chain, so a repository file can extend a global file that extends the default,
and updates to either reach every repository that inherits them.

The text is copied into a review when the review file is created. Editing the
instructions later changes new reviews only: a review the agent may already
have read keeps what it was written with. Saved changes are picked up while
revmark runs, the same way config changes are.

`.revmark/` is usually ignored by git, so a repository's instructions stay on
your machine. To share them with your team, un-ignore that one file
(`!.revmark/instructions.md`).

Reviews from before the rename lived in `.review/`. revmark still keeps that
folder out of the diff; move or delete it whenever you like.

## Configuration

`~/.config/revmark/config.toml` (or `$XDG_CONFIG_HOME/revmark/config.toml`),
then `.revmark.toml` in the repository on top:

```toml
[keys]
general_comment = "N"        # one key
down = ["j", "Down"]         # several
top = "gg"                   # a sequence; C-x for Ctrl, Tab, Enter, Esc, Space, arrows

[run]
command = "claude 'fix every open item in {file}'"
in = "auto"                  # auto | herdr | tmux | pane | foreground
output = "plain"             # plain | claude-stream (run pane, see below)

[ui]
line_numbers = "both"        # both | new
confirm_quit = true          # Q asks before quitting

[review]
quote_lines = 12             # longer quotes keep their first & last 3 lines; 0 = never cap
dir = ".revmark"             # where reviews go (see below)

[theme]
mode = "auto"                # auto | light | dark
dark = "catppuccin.macchiato"
light = "catppuccin.latte"

[theme.colors]               # optional, on top of whichever theme is active
border_active = "#f5bde6"
```

Action names are the ones `?` lists. An unknown action, or a key that is the
start of another binding, stops revmark at startup with the names involved.

Changes apply without a restart. Press `,` to edit your config; revmark reloads
it when the editor closes. Saving either config file from anywhere else (another
pane, your usual editor) is picked up within a quarter of a second too. If the
new file has a mistake, revmark keeps the config it was running with and shows
the error in the status line.

### Themes

Built in: `catppuccin.macchiato` and `catppuccin.latte` (the defaults),
`default.dark` and `default.light`. `mode = "auto"` picks the dark or light
theme from the terminal background (`COLORFGBG`), falling back to dark.

Languages come from bat's syntax set (via `two-face`), so anything bat
colours, revmark colours, from TypeScript and TSX to Swift, Dart and
Dockerfiles. Files are matched by extension, then by name. In Vue, Svelte,
Astro and HTML files, `<script>` and `<style>` blocks are coloured in their own
language (`lang="ts"`, `lang="scss"` and so on), and so is Astro's `---`
frontmatter.

Each theme brings its own syntax colours. The Catppuccin ones use the official
[catppuccin/bat](https://github.com/catppuccin/bat) themes, bundled under
`assets/themes/` with their MIT notice. Colours follow Catppuccin's style
guide: lavender for the focused border, overlay0 for the others, green and red
for added and removed lines, peach for the omitted-lines separator. Diff
backgrounds are the accent blended over the base colour, since a terminal has no
transparency.

Slots for `[theme.colors]`: `text`, `muted`, `subtle`, `border_active`,
`border_inactive`, `cursor_bg`, `selection_bg`, `note_bg`, `added`, `removed`,
`added_bg`, `removed_bg`, `hunk`, `fix`, `question`, `suggestion`, `done`,
`error`, `warning`, `info`. Values are `#rrggbb`. An unknown theme, slot, or
colour stops revmark at startup with the name in the message.

### Your own theme

A theme is a folder in `themes/` next to your `config.toml`; the folder name is
the theme name:

```
~/.config/revmark/
  config.toml
  themes/
    gruvbox.dark/
      theme.toml
      gruvbox.tmTheme        # optional: syntax colours, any bat/Sublime theme
```

```toml
# themes/gruvbox.dark/theme.toml
extends = "default.dark"     # built-in or another of your themes; default.dark if omitted
syntax = "gruvbox.tmTheme"   # a file in this folder, or a syntect theme name; inherited if omitted

[colors]                     # only the slots you change; the rest come from extends
border_active = "#d79921"
added = "#98971a"
removed = "#cc241d"
```

```toml
# config.toml
[theme]
dark = "gruvbox.dark"
```

- A folder with the same name as a built-in theme replaces it. Put
  `extends = "catppuccin.macchiato"` in `themes/catppuccin.macchiato/` to tweak
  the built-in one rather than start over.
- Themes can extend each other. A chain that loops back on itself is refused.
- The `.tmTheme` path must stay inside the theme folder.
- Everything is read and checked once at startup. A bad slot, a bad colour, a
  missing or unreadable `.tmTheme`, or an unknown `extends` stops revmark and
  names the theme and the problem.

`r` substitutes the absolute review path for `{file}`. Where it runs depends on
`[run] in`:

| `in` | Runs the command |
|---|---|
| `auto` (default) | in a new Herdr pane under Herdr, else a new tmux pane under tmux, else in the foreground |
| `herdr` / `tmux` | always in a new pane of that one; an error if revmark isn't running inside it |
| `pane` | in the background, its output streamed into a run pane under the diff; revmark stays usable |
| `foreground` | revmark steps aside, runs it, and comes back when it exits |

A tmux pane opens to the right of revmark, which keeps the focus. The command is
typed into the pane's shell, so the shell stays after the agent exits and its
output stays readable. Inside tmux running in Herdr (or the other way round),
`auto` picks Herdr; set `in` to choose.

`in = "pane"` suits an agent's non-interactive mode, such as
`claude -p "fix every open item in {file}"` or `codex exec …`: the command gets
no terminal and no keyboard, its output (colours stripped) scrolls in the run
pane, and the diff stays live while it works. One run at a time; pressing `r`
again offers to stop it, and quitting revmark stops it too. For an agent you
talk to while it works, use tmux, Herdr or the foreground.

The run pane's title shows how long the command has been running, then how
long it took. `4` (or `Tab`, which comes to it after the diff and comments) focuses it: `j`/`k` and the scroll
keys move through the output, long lines wrap, `gg` goes to the top and `G`
back to the newest output, which then stays in view as more arrives.

Some agents print nothing until they finish when they have no terminal;
`claude -p` prints only its final answer. To follow Claude as it works, have
it stream its events and let revmark turn them into short lines:

```toml
[run]
in = "pane"
output = "claude-stream"
command = "claude -p --output-format stream-json --verbose 'fix every open item in {file}'"
```

```
· started · claude-opus-5-5
Reading the review file.
· read src/lib.rs
· edit src/lib.rs
· bash cargo test
  ✗ error: 1 test failed
· finished · 7 turns · $0.42
```

Each tool call shows as `· <tool> <file or command>`, with the tool name in
colour, paths relative to the repository and the usual `cd <repo> &&` in front
of shell commands dropped. Failed tool calls show as `✗` in red. Claude's own
text is rendered as Markdown: headings, `**bold**`, `` `code` `` and lists
lose their markers and get colour, code blocks show as code, and long lines
wrap between words with list items indented under their text. Lines that are
not stream events (stderr, or any other agent) pass through unchanged, so the
setting is safe to leave on.

Either way, the review file is the progress report: the default instructions ask the agent to set
`status: in-progress` first and to mark each item `[~]` before working on it.
revmark shows those marks as `~` in the comments pane and the diff, and counts
them in the status line (`2 open · 1 working`).

`$VISUAL`, then `$EDITOR`, then `vi` edits suggestions and long comments, and
opens files with `o`. The cursor line is passed as `+N` to vi, Vim, Neovim,
nano, Emacs, micro and Kakoune, as `path:N` to Helix, Zed and Sublime Text,
and as `--goto path:N` to VS Code, VSCodium and Cursor; any other editor gets
the plain path.

## License

MIT. Copyright (c) 2026 Konstantin Kai.
