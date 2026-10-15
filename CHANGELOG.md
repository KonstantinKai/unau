# Changelog

## 0.1.0

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
  written in `$EDITOR`. Each one is written to `.revmark/<date>-<n>.md` the
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
- Agent edits made to the review file while revmark runs are merged, so the
  agent's ticks are never overwritten.
- `--out -` prints the review on exit instead of writing a file.
- Long quotes keep only their first and last lines (`[review] quote_lines`,
  default 12), so a big selection does not cost the agent its tokens twice.
- `L` opens a comments pane under the file tree listing every comment;
  moving through it shows each one in the diff.
- revmark reopens where you left off: the open file and line, the focused
  pane, folded directories and the layout. The comments pane and the last
  run's output come back when you continue a review.
- `W` and `E` fold and unfold every directory in the file tree.
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
  `status: in-progress`; the default instructions ask them to, and revmark
  shows the marks while the agent works.
- The file tree shows each file's git status (`M`, `A`, `D`, `?`) before its
  name, and directories show whether the files under them are staged.
- `o` opens the file in `$EDITOR` at the line under the cursor; the diff is
  re-read when the editor closes.
- `[review] dir` moves review files out of `.revmark/`, to another folder in
  the repository or outside it (`~/reviews/{repo}`), so a repository can stay
  free of review files.
- `--licenses` prints revmark's license and every third-party notice it
  carries, including those of the bundled syntax definitions and themes.
- `--continue` reopens the newest review in `.revmark/`, or starts a new one
  when there is none.
- Every key can be rebound in `config.toml` or a repository `.revmark.toml`.
  `,` opens your config in `$EDITOR`, and saved changes apply without a
  restart; a broken config is reported and the previous one kept.
- Agent instructions at the top of every review, from
  `.revmark/instructions.md` in the repository, else
  `~/.config/revmark/instructions.md`, else a built-in default; `i` edits
  them. A review keeps the instructions it was created with. A `{inherit}`
  line extends the next level instead of replacing it.
- Themes: Catppuccin Macchiato and Latte by default, with matching syntax
  colours, picked by terminal background. Highlighting uses bat's syntax set,
  so TypeScript, TSX, JSX, TOML, Vue, Swift, Kotlin, Dart, SCSS and
  Dockerfiles are coloured too, script and style blocks in Vue, Svelte, Astro
  and HTML files in their own language, and only the lines on screen are
  highlighted; the earlier colours stay available
  as `default.dark` and `default.light`. Any single colour can be overridden in
  `[theme.colors]`.
- Your own themes: a folder in `themes/` next to `config.toml` with a
  `theme.toml` and, optionally, any bat or Sublime `.tmTheme` for syntax
  colours. Themes can extend a built-in or each other, and a folder named after
  a built-in theme replaces it.
