# Changelog

## 0.1.0 - 2026-10-16

First release.

### Added

- Review the working tree (staged, unstaged and untracked), one commit, or a
  range, in a folding file tree and a diff pane with old and new line numbers,
  syntax highlighting, side-by-side view, zoom, and "expand nearby" on the
  omitted-lines separator.
- Stage, unstage and discard single lines or whole hunks from the diff, or
  whole files and directories from the file tree; `gs`, `gu` and `gd` take
  the whole open file from anywhere. Staging a whole file moves on to the
  next file that still has unstaged changes. Untracked files can be
  staged line by line. Staging is off for commits and ranges.
- Comments, questions, file and general comments, and replacement suggestions
  written in `$EDITOR`. Each one is written to `.unau/<date>-<n>.md` the
  moment it is made, with new-side anchors and the exact lines quoted.
- Staged lines count as accepted: annotations on lines you stage leave the
  review, after a confirmation.
- `r` runs a configured command on the review file: in a new Herdr pane under
  Herdr, a new tmux pane under tmux, otherwise in the foreground. `[run] in`
  picks one explicitly; `in = "pane"` runs it in the background with its
  output in a pane under the diff (`R` shows or hides it).
- Re-review with `--review`: ticked items show what changed at their lines, `a`
  stages the fix and closes the item, `c` pushes back with a new open item.
  Files that changed since the review was written are marked in the tree.
- Agent edits made to the review file while unau runs are merged, so the
  agent's ticks are never overwritten.
- `--out -` prints the review on exit instead of writing a file.
- Long quotes keep only their first and last lines (`[review] quote_lines`,
  default 12), so a big selection does not cost the agent its tokens twice.
- `L` opens a comments pane under the file tree listing every comment;
  moving through it shows each one in the diff.
- unau reopens where you left off: the open file and line, the focused
  pane, folded directories and the layout. The comments pane and the last
  run's output come back when you continue a review.
- `W` and `E` fold and unfold every directory in the file tree.
- `/` filters the file tree by path as you type.
- The scroll keys (`Ctrl-e`/`Ctrl-y`, `Ctrl-d`/`Ctrl-u`, `Ctrl-f`/`Ctrl-b`)
  scroll the diff from the files and comments panes too, so a file can be read
  without leaving the list; the run pane keeps them for its output.
- Staging, unstaging or discarding whole files and folders takes one `git`
  call, and untracked files are read without starting `git` for each one, so
  a repository with hundreds of changes stays quick. A file rewritten from
  top to bottom opens at once, however long it is.
- `1` to `4` focus the files, diff, comments and run panes; each pane's title
  shows its number in its own colour.
- The run pane shows how long the command has run, can be focused and
  scrolled back, and wraps long lines.
- `[run] output = "claude-stream"` shows Claude's `stream-json` events in the
  run pane as short lines (each file read or edited, each command, failures,
  the final cost), so a long run is no longer a blank pane. Claude's text is
  rendered as Markdown, tool names are coloured, and long lines wrap between
  words.
- Agents can mark the item they are working on `[~]` and set
  `status: in-progress`; the default instructions ask them to, and unau
  shows the marks while the agent works.
- The file tree shows each file's git status (`M`, `A`, `D`, `?`) before its
  name, and directories show whether the files under them are staged.
- `o` opens the file in `$EDITOR` at the line under the cursor; the diff is
  re-read when the editor closes.
- `[review] dir` moves review files out of `.unau/`, to another folder in
  the repository or outside it (`~/reviews/{repo}`), so a repository can stay
  free of review files.
- `--licenses` prints unau's license and every third-party notice it
  carries, including those of the bundled syntax definitions and themes.
- `--continue` reopens the newest review in the review folder, or starts a
  new one when there is none.
- Every key can be rebound in `config.toml` or a repository `.unau.toml`.
  `,` opens your config in `$EDITOR`, and saved changes apply without a
  restart; a broken config is reported and the previous one kept.
- Agent instructions at the top of every review, from `instructions.md` in
  the review folder (or the file `[review] instructions` names), else
  `~/.config/unau/instructions.md`, else a built-in default; `i` edits
  them. A review keeps the instructions it was created with. A `{inherit}`
  line extends the next level instead of replacing it.
- Themes: Catppuccin Macchiato and Latte by default, with matching syntax
  colours, picked by terminal background; plain colours are available as
  `default.dark` and `default.light`. Any single colour can be overridden in
  `[theme.colors]`.
- Syntax highlighting uses bat's syntax set, so TypeScript, TSX, JSX, TOML,
  Vue, Swift, Kotlin, Dart, SCSS and Dockerfiles are coloured too, and script
  and style blocks in Vue, Svelte, Astro and HTML files in their own language.
  Only the lines on screen are highlighted, so long files open at once.
- Seven more theme families: Gruvbox, Dracula, Nord, Solarized, One Half,
  Tokyo Night and Rosé Pine, each in its dark and light flavours where it has
  them.
- The cursor and selection keep an added or removed line green or red, so a
  selected line's kind shows without reading its sign.
- Popups stand out: a raised, accent-tinted surface with a rounded border,
  and the screen behind a question or the help fades.
- Themes paint their own background, so any theme works on any terminal;
  `[ui] background = "terminal"` keeps the terminal's.
- `T` opens a theme picker that previews each theme live and saves the pick
  to your config, keeping the rest of the file as it was.
- Your own themes: a folder in `themes/` next to `config.toml` with a
  `theme.toml` and, optionally, any bat or Sublime `.tmTheme` for syntax
  colours. Themes can extend a built-in or each other, and a folder named after
  a built-in theme replaces it.
