<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/KonstantinKai/unau/main/assets/brand/header-dark.png">
  <img alt="unau: terminal code review that ends in a file an agent can act on." src="https://raw.githubusercontent.com/KonstantinKai/unau/main/assets/brand/header-light.png">
</picture>

**unau** (say *YOO-now*, /ˈjuː.naʊ/) is the two-toed sloth: slow down, read the
diff, then hand it off.

Open the working tree diff (or a commit, or a range) in a lazygit-shaped TUI: a
folding file tree on the left, the diff on the right. Stage the lines that are
right. Mark the lines that aren't with a comment, a question, or replacement
code. Every mark lands in `.unau/<date>-<n>.md` as it happens, a Markdown
checklist with anchors an agent can follow. Point your agent at the file, let it
work, reopen the review to see what changed, accept or push back.

unau is agent-agnostic. It knows no agent and links no SDK: the review is
plain Markdown, and the hand-off is a command line you configure with `{file}`
in it, so Claude Code, Codex, Aider, Gemini CLI, a script of your own or a
person can all work from it.

## Demo

<p align="center">
  <a href="https://raw.githubusercontent.com/KonstantinKai/unau/main/assets/demo.gif" target="_blank">
    <img alt="unau in use: stage a new file, comment on two lines of a TypeScript change, run the agent in the run pane, then accept each fix it ticked until the review is complete." src="https://raw.githubusercontent.com/KonstantinKai/unau/main/assets/demo.gif">
  </a>
</p>

## How to work with unau

<a href="https://raw.githubusercontent.com/KonstantinKai/unau/main/assets/workflow-light.png">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/KonstantinKai/unau/main/assets/workflow-dark.png">
  <img alt="Who does what: you read the diff, stage what is right and mark what is wrong; unau writes each mark to the review file and runs your agent on it; the agent works each item from [~] to [x]; unau picks up the ticks and shows each one against the code now; you accept or push back until nothing is open or unstaged." src="https://raw.githubusercontent.com/KonstantinKai/unau/main/assets/workflow-light.png">
</picture>
</a>

1. **Review.** Walk the diff. Stage what is right (`s` lines or hunks, `gs` a
   whole file); staged means accepted, so it never reaches the agent.
2. **Mark.** Comment on what is wrong: `c` a fix or a question, `S` replacement
   code, `C` the whole file, `n` a general note. Each one is written to the
   review file the moment you make it, anchored to new-side lines with the code
   quoted.
3. **Hand off.** `r` runs your agent on the review file, in a tmux, zellij or
   Herdr pane, the foreground, or the run pane inside unau. The agent marks the
   item it is on `[~]`, ticks it `[x]` with a note when done, and never stages.
4. **Re-review.** unau picks the ticks up live (or later with `--continue`).
   Each ticked item shows its quoted lines against the code now. `a` accepts:
   the fix is staged and the item leaves the file. `c` pushes back with a new
   open item for the next run.
5. **Done** when no item is open and nothing is left unstaged. Commit as you
   normally would; unau never commits.

## Install

Pick one.

**Homebrew** (macOS and Linux):

```sh
brew install KonstantinKai/tap/unau
```

**Cargo**, built from source (Rust 1.92 or newer, and a C compiler for the
bundled Oniguruma regex library: Xcode's command line tools on macOS, `gcc` or
`clang` on Linux):

```sh
cargo install unau
```

**Installer script**, a prebuilt binary for macOS or Linux (Intel or ARM),
put in `~/.cargo/bin`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/KonstantinKai/unau/releases/latest/download/unau-installer.sh | sh
```

The [releases page](https://github.com/KonstantinKai/unau/releases) has the
archives and checksums.

unau needs `git` on `PATH`: it shells out to it and links no git library. On
Windows, use it inside WSL with the Linux build; there is no native Windows
build yet.

## Usage

```sh
unau                 # working tree vs HEAD: staged, unstaged, untracked
unau HEAD~1          # one commit
unau main..HEAD      # a range; staging keys are off in range mode
unau -- main         # a path called main, not the branch
unau --review .unau/2026-10-14-00.md  # reopen a review
unau --continue      # reopen the newest review; start a new one if there is none
unau --out -         # print the review on exit instead of writing a review file
unau ~/Dev/other     # the repository at that path
unau --licenses      # print the license and third-party notices
```

A path that exists wins over a revision with the same name; `--` forces the
path reading.

The working tree view shows each file's unstaged lines and, under a `staged`
strip, the lines already in the index. Staged means accepted: staged lines never
appear in the review, and an annotation on lines you stage leaves the review
(unau asks first).

In the files pane, each file starts with its git status: `M` modified, `A`
added, `D` deleted, `?` untracked (a rename shows as a `D` and an `A`). After
the name, `●` marks unstaged lines, `✓` staged ones, `✎ n` the annotations, and
`Δ` a file that changed since the review was written. A directory carries `●`
and `✓` for the files under it, so a directory showing only `✓` is fully
staged.

unau never edits `.gitignore`. Add `.unau/*` to it yourself if you don't
want reviews tracked, or keep reviews out of the repository altogether with
`[review] dir`:

```toml
[review]
dir = "~/reviews/{repo}"   # {repo} is the repository's folder name
```

A relative path starts at the repository root (`dir = "docs/reviews"`), and a
folder inside the repository is kept out of the diff like `.unau/` is. Set
it in your own config for every repository, or in a repository's
`.unau.toml` for that one; it takes effect the next time unau starts.
`--continue` looks there, and the agent gets the full path in `{file}`. A
repository's `instructions.md` moves with it; `[review] instructions` points
elsewhere.

Changes made outside unau show up within about a second: an agent editing in
another pane, your usual editor, `git add` in another shell. While you are
typing a comment or have lines selected, unau waits until you finish, so the
lines don't move under you. While you are in another pane, tab or app it
stops checking and catches up the moment you come back. That needs a terminal
or multiplexer that reports focus; under tmux, add `set -g focus-events on`.
Without focus reports, unau simply keeps checking.

unau picks up where you left off. On quit it saves the open file and cursor
line, the focused pane, folded directories, the files pane width, zoom and the
unified or side-by-side view, and restores them the next time you open it in
the same repository. The comments pane and the last run's output come back
only when you continue a review (`--continue` or `--review`); a fresh review
starts without them. A run still going when you quit is stopped and comes back
as `exited -1`. Sessions live in
`$XDG_STATE_HOME/unau/sessions/` (else `~/.local/state/unau/sessions/`),
one small file per repository and never inside it; delete one to start clean.

## Keys

| Key | Action |
|---|---|
| `j` `k` (or `Down` `Up`) | next / previous line |
| `J` `K` | next / previous block of changed lines (added or removed), even when it is close enough to the last one to share its hunk |
| `gg` `G` | first / last line |
| `:` | go to a line: type its number in the popup, `Enter`. A line inside an "unchanged lines omitted" stretch lands on that separator; `z` opens it |
| `Ctrl-e` `Ctrl-y` | scroll the diff one line; the cursor stays unless it would leave the screen |
| `Ctrl-d` `Ctrl-u` | scroll the diff half a page down / up |
| `Ctrl-f` `Ctrl-b` | scroll the diff a page down / up |
| `h` `l` (or `Left` `Right`) `Tab` | focus files / diff / cycle in pane-number order: files → diff → comments → run (shown panes only) |
| `1` `2` `3` `4` | focus files / diff / comments / run pane (the last two shown if hidden); each pane's title starts with its number |
| `Enter` | fold a directory, open a file; on a separator, expand |
| `W` `E` | fold / unfold every directory (folding all leaves nested ones folded too) |
| `-` | flat file list |
| `/` | filter the files pane by path, live and ignoring case; `Enter` keeps it, `Esc` clears it. Staging a folder then takes only the files shown |
| `L` | comments pane under the files pane: every comment in one list; moving through it shows each one in the diff, `Enter` jumps there. `L` again hides it |
| `v` | start or end a selection |
| `s` `u` `d` | stage, unstage, discard: in the diff, the selection or the hunk under the cursor, then the cursor moves on to the next change; in the files pane, the whole file, or every file under a directory (`d` asks) |
| `gs` `gu` `gd` | stage, unstage, discard the whole open file from any pane (`gd` asks). Staging a whole file or directory (`gs`, or `s` in the files pane) moves on to the next file that still has unstaged changes, opening its folder if it is folded |
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
| `,` | open your config in `$EDITOR` (created with every setting commented out the first time) |
| `T` | pick a theme, previewed live; `Enter` saves it to your config |
| `i` | open the agent instructions in `$EDITOR` (see below) |
| `o` | open the file in `$EDITOR`: from the diff at the cursor line, from the files pane the file under the cursor; the diff is re-read when the editor closes |
| `a` | accept a done item: stage its hunk, drop it from the review |
| `Q` | quit; asks first in a popup, naming open items never run or a running command (`[ui] confirm_quit = false` to skip) |
| `?` | help |

The scroll keys (`Ctrl-e`/`Ctrl-y`, `Ctrl-d`/`Ctrl-u`, `Ctrl-f`/`Ctrl-b`) move
the diff from the files and comments panes too, without leaving them; in the
run pane they scroll its output.

Comments are typed in a popup that floats just under the lines you selected
(above them when there's no room), so the code stays in view. Its title shows
whether the comment is a `fix` or a `question`; `Tab` switches between them
(editing starts from the item's own kind). `Enter` saves,
`Alt-Enter` starts a new line (`Ctrl-j` too, if your multiplexer doesn't
take it), `Esc` cancels and `Ctrl-e` moves the text into
`$EDITOR`. Editing works as in a shell: `Ctrl-w` deletes the word before the
cursor, `Ctrl-u` everything before it, and `Ctrl-a` jumps to the start.
`Ctrl-w` works in the files filter too.

## The review file

````md
---
tool: unau 0.4.0
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
- The agent ticks items and sets `status`. unau never writes `done`. If the
  agent edits the file while unau runs, unau merges: the agent's ticks win
  and unau's own changes go on top.

## Re-review

Reopen with `--review <file>`. A ticked item shows its quoted lines against
what is there now. `a` stages the hunks that touch it and removes it from the
file. `c` pushes back: the item becomes a new open one with your comment. A
file that changed since the review was written carries a `Δ` in the file
tree. When nothing is open and nothing is left unstaged, unau says the review
is complete.

If the agent rewrote every quoted line and nothing changed at the anchor,
unau marks the item "anchor lost"; stage the fix by hand and delete the item.

## Agent instructions

Every review opens with a `> **Agent instructions**` block telling the agent
how to work through the file. The text comes from the first of these that
exists:

| File | Applies to |
|---|---|
| `instructions.md` in the review folder (`.unau/`, or `[review] dir`) | this workspace only |
| `~/.config/unau/instructions.md` | every workspace |
| neither | the built-in default |

Press `i` to edit the file in effect. If there is none yet, unau creates
the global one with the default text, so there is something to start from. To
give one repository its own instructions, put an `instructions.md` in its
review folder. On its own it replaces the global file and the default.
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

To keep a repository's instructions somewhere else, point
`[review] instructions` at the file, in the repository's `.unau.toml` or your
own config (a relative path starts at the repository root, `~/` and `{repo}`
work as in `dir`):

```toml
[review]
instructions = "docs/agents/review.md"
```

The text is copied into a review when the review file is created. Editing the
instructions later changes new reviews only: a review the agent may already
have read keeps what it was written with. Saved changes are picked up while
unau runs, the same way config changes are.

### Writing your own

The instructions sit in a quote block at the top of every review, under
**Agent instructions**, and the agent reads them before the items. A few
things keep them working:

- **Keep the default's rules, or `{inherit}` them.** The review only moves
  when the agent marks items `[~]` and `[x]` and sets `status`; a file that
  replaces the default without those rules leaves the review looking
  untouched.
- **No `#` headings.** Inside the quote they render bigger than the review's
  own `## General` and `## path` sections, and the instructions read as part
  of the review. Start a group with a bold line instead, and use lists under
  it.
- **Short, and pointing elsewhere.** Every review carries the full text, and
  the agent reads it on every run. Long procedures belong in a file in the
  repository that the instructions name (`Read docs/agents/review.md before
  you start.`).
- **Paths from the repository root**, the way the agent sees them.

```md
{inherit}

**Before you fix a comment, decide what kind it is.**

- *A bug:* write the failing test first, then fix it; the steps are in
  `docs/agents/bugs.md`.
- *A style or architecture note:* fix the instance, then add the rule to
  `docs/agents/rules/` so the next one cannot happen.
- Run `pnpm test` before ticking an item.
```

With `.unau/*` in `.gitignore`, a repository's instructions stay on your
machine. To share them with your team, add `!.unau/instructions.md` under it.
That needs the `/*`: git never looks inside a folder ignored as `.unau/`, so
nothing in it can be un-ignored. A file moved with `[review] dir` or
`[review] instructions` is tracked or ignored wherever it lives.

## Configuration

`~/.config/unau/config.toml` (or `$XDG_CONFIG_HOME/unau/config.toml`),
then `.unau.toml` in the repository on top:

```toml
[keys]
general_comment = "N"        # one key
down = ["j", "Down"]         # several
top = "gg"                   # a sequence; C-x for Ctrl, Tab, Enter, Esc, Space, arrows

[run]
command = "claude 'fix every open item in {file}'"
# send = "fix every open item in {file}"   # later r presses type this into the agent's pane
# reuse = "ask"              # ask | session | new: use an agent session you started yourself
in = "auto"                  # auto | herdr | tmux | zellij | pane | foreground
output = "plain"             # plain | claude-stream (run pane, see below)

[ui]
line_numbers = "both"        # both | new
confirm_quit = true          # Q asks before quitting
check_updates = true         # look for a new release once a day (see Updates)
background = "theme"         # theme | terminal (keep the terminal's own, transparency included)

[review]
quote_lines = 12             # longer quotes keep their first & last 3 lines; 0 = never cap
dir = ".unau"                # where reviews go (see Usage)
# instructions = "…"         # this repository's agent instructions; default: instructions.md in the review folder

[theme]
mode = "auto"                # auto | light | dark
dark = "catppuccin.macchiato"
light = "catppuccin.latte"

[theme.colors]               # optional, on top of whichever theme is active
border_active = "#f5bde6"
```

Action names are the ones `?` lists. An unknown action, or a key that is the
start of another binding, stops unau at startup with the names involved.

Changes apply without a restart. Press `,` to edit your config; unau reloads
it when the editor closes. Saving either config file from anywhere else (another
pane, your usual editor) is picked up within a quarter of a second too. If the
new file has a mistake, unau keeps the config it was running with and shows
the error in the status line.

### Themes

Built in:

| Family | Dark | Light |
|---|---|---|
| Catppuccin (default) | `catppuccin.macchiato` | `catppuccin.latte` |
| Gruvbox | `gruvbox.dark` | `gruvbox.light` |
| Dracula | `dracula` | |
| Nord | `nord` | |
| Solarized | `solarized.dark` | `solarized.light` |
| One Half | `onehalf.dark` | `onehalf.light` |
| Tokyo Night | `tokyonight.night` | `tokyonight.day` |
| Rosé Pine | `rosepine.main` | `rosepine.dawn` |
| Plain colours | `default.dark` | `default.light` |

`mode = "auto"` picks the dark or light theme from the terminal background
(`COLORFGBG`), falling back to dark.

Press `T` to pick one: every built-in and user theme, dark and light grouped,
previewed live as you move. `Enter` writes it to your config (`dark = …` or
`light = …`, plus `mode` when your background would otherwise hide it), leaving
the rest of the file and its comments alone; `Esc` puts back what you had. A
repository's `.unau.toml` that sets `[theme]` still wins there, and the
picker says so.

Languages come from bat's syntax set (via `two-face`), so anything bat
colours, unau colours, from TypeScript and TSX to Swift, Dart and
Dockerfiles. Files are matched by extension, then by name. In Vue, Svelte,
Astro and HTML files, `<script>` and `<style>` blocks are coloured in their own
language (`lang="ts"`, `lang="scss"` and so on), and so is Astro's `---`
frontmatter.

Each theme brings its own syntax colours. The Catppuccin, Tokyo Night and Rosé
Pine ones are the official syntax themes, bundled under `assets/themes/` with
their licenses; the others are bat's, through `two-face`. Every family's own
palette fills the interface the way Catppuccin's style guide places colours:
lavender for the focused border, overlay0 for the others, green and red for
added and removed lines, peach for the omitted-lines separator. Diff
backgrounds are the accent blended over the base colour, since terminal
colours have no alpha channel. Under the cursor or a selection, an added or
removed line keeps a touch of its green or red, so you can still tell which
it is without looking for the sign.

Every theme paints its own background, so a light theme reads right on a dark
terminal and the other way round. Set `[ui] background = "terminal"` to keep
your terminal's background instead, transparency and all; themes are then
best matched to it.

Slots for `[theme.colors]`: `background`, `text`, `muted`, `subtle`, `border_active`,
`border_inactive`, `cursor_bg`, `selection_bg`, `note_bg`, `added`, `removed`,
`added_bg`, `removed_bg`, `hunk`, `fix`, `question`, `suggestion`, `done`,
`error`, `warning`, `info`. Values are `#rrggbb`. An unknown theme, slot, or
colour stops unau at startup with the name in the message.

### Your own theme

A theme is a folder in `themes/` next to your `config.toml`; the folder name is
the theme name:

```
~/.config/unau/
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
  missing or unreadable `.tmTheme`, or an unknown `extends` stops unau and
  names the theme and the problem.

### Running an agent

`r` runs `[run] command` with `{file}` replaced by the absolute path of the
review file. Where it runs depends on `[run] in`:

| `in` | Runs the command |
|---|---|
| `auto` (default) | in a new Herdr pane under Herdr, else a new tmux pane under tmux, else a new zellij pane under zellij, else in the foreground |
| `herdr` / `tmux` / `zellij` | always in a new pane of that one; an error if unau isn't running inside it |
| `pane` | in the background, its output streamed into a run pane under the diff; unau stays usable |
| `foreground` | unau steps aside, runs it, and comes back when it exits |

A tmux pane opens to the right of unau, which keeps the focus. The command is
typed into the pane's shell, so the shell stays after the agent exits and its
output stays readable. A zellij pane also opens to the right and hands the
focus back to unau; zellij runs the command itself rather than typing it into
a shell, and keeps the pane open when it exits, with its output and exit code
(Enter runs it again). When unau runs inside more than one of these, `auto`
picks Herdr, then tmux, then zellij; set `in` to choose.

To keep working with one agent session instead of starting a new one on every
`r`, set `send`:

```toml
[run]
command = "claude 'fix every open item in {file}'"   # the first r: opens the pane
send = "fix every open item in {file}"               # every later r: typed into it
```

The first `r` opens the pane as usual. Every later `r` types `send` into that
same pane and presses Enter, as if you had typed it at the agent's prompt, so
the agent keeps what it learned in earlier rounds: what it changed, what failed,
what you told it. A follow-up is quicker and costs fewer tokens than a fresh
start. If the pane is gone, or zellij shows its command as exited, `r` opens a
new one. `send` must be one line (a new line would send the message early), and
`{file}` goes in as a plain path, since it is read by the agent, not a shell.
It applies to Herdr, tmux and zellij panes, not to `pane` or `foreground` runs.

It also works with an agent you started yourself, before unau. With `send` set
and no pane of its own yet, `r` looks for panes running the program your
`command` starts (its first word, `claude` above) in this repository: Herdr
says which agent each pane runs, and in tmux unau looks at the processes under
each pane's shell. unau never types into one of those unasked: the first time
it finds any, it asks, listing them with `+ new session` below (`j`/`k`,
`Enter`, `Esc`), and saves the answer as `[run] reuse` in the repository's
`.unau.toml`, creating the file if needed:

| `reuse` | `r` with your session running |
|---|---|
| `"ask"` (default) | asks, then saves `session` or `new` |
| `"session"` | sends to it; asks which only when several run here |
| `"new"` | starts a new session in a new pane, never looking |

With none found, `r` opens a new pane as usual. The session it sends to is
remembered for the rest of the run. zellij does not say what runs inside its
panes, so there only a pane unau opened itself is reused.

unau can't tell whether the agent is still running in the pane: if you quit
the agent but keep the pane, the next `r` types the message into the shell
there. Close the pane and `r` starts over.

`in = "pane"` suits an agent's non-interactive mode, such as
`claude -p "fix every open item in {file}"` or `codex exec …`: the command gets
no terminal and no keyboard, its output (colours stripped) scrolls in the run
pane, and the diff stays live while it works. One run at a time; pressing `r`
again offers to stop it, and quitting unau stops it too. For an agent you
talk to while it works, use tmux, zellij, Herdr or the foreground.

The run pane's title shows how long the command has been running, then how
long it took. `4` focuses it (so does `Tab`, after the diff and comments):
`j`/`k` and the scroll keys move through the output, long lines wrap, `gg`
goes to the top and `G` back to the newest output, which then stays in view as
more arrives.

While the agent works, the review file is the progress report: the default
instructions ask the agent to set `status: in-progress` first and to mark each
item `[~]` before working on it. unau shows those marks as `~` in the
comments pane and the diff, and counts them in the status line
(`2 open · 1 working`).

### Following Claude as it works

Some agents print nothing until they finish when they have no terminal;
`claude -p` prints only its final answer. To follow Claude as it works, have
it stream its events and let unau turn them into short lines:

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

### Updates

unau checks for a new release once a day, in the background, with `curl`;
an offline machine just gets no notice. When there is one, the right end of
the status line says so in green, before the version:

- **Installed with the installer script:** unau installs the update itself,
  quietly, and says `Update available, restart to update`. Quit and start it
  again; your place comes back with the session.
- **Installed with Homebrew or cargo:** it shows the command to run, such as
  `Update 0.3.0: brew upgrade unau`. unau never runs your package manager for
  you.

`[ui] check_updates = false` turns off both the check and the install.

### Editor

`$VISUAL`, then `$EDITOR`, then `vi` edits suggestions and long comments, and
opens files with `o`. The cursor line is passed as `+N` to vi, Vim, Neovim,
nano, Emacs, micro and Kakoune, as `path:N` to Helix, Zed and Sublime Text,
and as `--goto path:N` to VS Code, VSCodium and Cursor; any other editor gets
the plain path.

## Credits

- [annodiff](https://github.com/MelsRoughSketch/annodiff) by MelsRoughSketch:
  the main reference; some code adapted (MIT, see `assets/notices/`)
- [reslop](https://github.com/tshemsedinov/reslop) by Timur Shemsedinov: the
  review file format
- [lazygit](https://github.com/jesseduffield/lazygit) by Jesse Duffield: the
  layout and line staging
- [Catppuccin](https://github.com/catppuccin/catppuccin): the default themes
- [Tokyo Night](https://github.com/folke/tokyonight.nvim) by Folke Lemaitre
  (Apache 2.0) and [Rosé Pine](https://github.com/rose-pine/tm-theme) (MIT):
  the bundled syntax themes, licenses in `assets/themes/`
- [Oniguruma](https://github.com/kkos/oniguruma) by K. Kosako: the regular
  expressions behind syntax highlighting (BSD 2-Clause, see `assets/notices/`)

## License

MIT. Copyright (c) 2026 Konstantin Kai. Dependencies and bundled code and
themes keep their own licenses; see
[third-party notices](https://github.com/KonstantinKai/unau/blob/main/THIRD_PARTY_NOTICES.md) and `assets/notices/`.

Run `unau --licenses` to print every included license text and
attribution.
