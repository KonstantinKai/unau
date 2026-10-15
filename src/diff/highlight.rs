//! Syntax highlighting with syntect, done lazily: only lines that are about to
//! be shown are highlighted, so opening a file costs a screenful, not the file.
//!
//! Each run of visible lines is parsed from its own first line, the way a diff
//! hunk is. The two sides of a diff are parsed separately, so removed lines get
//! old-side state and added lines new-side state. The parser state at the end
//! of what was highlighted is kept, so scrolling further continues from it.

use super::{SectionView, embedded};
use crate::{
    git::{Line, LineKind, Section},
    theme::{LATTE_TMTHEME, MACCHIATO_TMTHEME, Syntax},
};
use ratatui::style::Color;
use std::{collections::HashMap, io::Cursor, ops::Range, path::Path, sync::LazyLock};
use syntect::{
    highlighting::{HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet},
    parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet},
};

// bat's syntax set: syntect's own defaults lack TypeScript, TSX, TOML, Swift,
// Kotlin, Dart, Vue and more.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);
static MACCHIATO: LazyLock<Option<Theme>> = LazyLock::new(|| load(MACCHIATO_TMTHEME));
static LATTE: LazyLock<Option<Theme>> = LazyLock::new(|| load(LATTE_TMTHEME));

/// Syntect theme used when a bundled one fails to load.
const FALLBACK: &str = "base16-ocean.dark";

/// Longest stretch parsed from one starting point. A new file is a single run
/// of visible lines; without this, jumping to its end would parse all of it.
/// The cost is that a chunk starts with fresh parser state, like a hunk does.
pub const HIGHLIGHT_CHUNK: usize = 256;

fn load(bytes: &[u8]) -> Option<Theme> {
    ThemeSet::load_from_reader(&mut Cursor::new(bytes)).ok()
}

pub fn syntax_theme(syntax: &Syntax) -> &Theme {
    let found = match syntax {
        Syntax::CatppuccinMacchiato => MACCHIATO.as_ref(),
        Syntax::CatppuccinLatte => LATTE.as_ref(),
        Syntax::Builtin(name) => THEMES.themes.get(name.as_ref()),
        Syntax::Loaded(theme) => Some(theme.as_ref()),
    };
    found.unwrap_or(&THEMES.themes[FALLBACK])
}

/// Whether syntect ships a theme called `name`.
pub fn builtin_syntax_exists(name: &str) -> bool {
    THEMES.themes.contains_key(name)
}

/// Colour spans of one source line, as byte ranges of its text.
pub type Spans = Vec<(Color, Range<usize>)>;

/// Parser and highlighter state for one side of a diff.
#[derive(Clone, Debug)]
struct SideState {
    parse: ParseState,
    highlight: HighlightState,
}

/// Where highlighting of one run of visible lines stopped.
#[derive(Clone, Debug)]
struct Resume {
    /// Next line of the section to highlight.
    next: usize,
    /// Old side, then new side.
    sides: [SideState; 2],
}

/// Highlighting of one file, filled in as lines come into view.
#[derive(Clone, Debug, Default)]
pub struct Highlighting {
    /// Spans per section, per line; empty until the line is first shown.
    pub spans: Vec<Vec<Spans>>,
    /// Lines highlighted so far, per section.
    done: Vec<Vec<bool>>,
    /// Keyed by section and the first line of a run of visible lines.
    resume: HashMap<(usize, usize), Resume>,
    /// The file's language is unknown: nothing to highlight.
    plain: bool,
}

impl Highlighting {
    pub fn new(path: &str, sections: &[Section]) -> Self {
        Self {
            spans: sections
                .iter()
                .map(|s| vec![Vec::new(); s.diff.lines.len()])
                .collect(),
            done: sections
                .iter()
                .map(|s| vec![false; s.diff.lines.len()])
                .collect(),
            resume: HashMap::new(),
            plain: language(path).is_none(),
        }
    }

    /// Number of lines highlighted so far, across sections.
    #[cfg(test)]
    pub fn highlighted(&self) -> usize {
        self.done.iter().flatten().filter(|d| **d).count()
    }

    /// Makes sure every line in `wanted` (section, line) has its spans.
    pub fn ensure(
        &mut self,
        path: &str,
        sections: &[Section],
        views: &[SectionView],
        wanted: impl IntoIterator<Item = (usize, usize)>,
        theme: &Syntax,
    ) {
        if self.plain {
            return;
        }
        let Some(own) = language(path) else {
            self.plain = true;
            return;
        };
        // Furthest wanted line per parse start, with the language parsed there.
        let mut targets: HashMap<(usize, usize), (usize, &SyntaxReference)> = HashMap::new();
        for (s, i) in wanted {
            if self
                .done
                .get(s)
                .and_then(|d| d.get(i))
                .copied()
                .unwrap_or(true)
            {
                continue;
            }
            let lines = &sections[s].diff.lines;
            // In a component file, a line is parsed in its block's language
            // and never from before the block starts.
            let (block_start, block_language) = match embedded::block_of(path, lines, i) {
                Some((start, Some(ext))) => (start, language(&format!("block.{ext}"))),
                Some((start, None)) => (start, None),
                None => (0, None),
            };
            let start = run_start(&views[s].visible, i).max(block_start);
            let entry = targets
                .entry((s, start))
                .or_insert((i, block_language.unwrap_or(own)));
            entry.0 = entry.0.max(i);
        }
        let highlighter = Highlighter::new(syntax_theme(theme));
        for ((s, start), (last, language)) in targets {
            let lines = &sections[s].diff.lines;
            let mut resume = self.resume.remove(&(s, start)).unwrap_or_else(|| Resume {
                next: start,
                sides: [fresh(language, &highlighter), fresh(language, &highlighter)],
            });
            while resume.next <= last {
                let i = resume.next;
                self.spans[s][i] = line_spans(&lines[i], &mut resume.sides, &highlighter);
                self.done[s][i] = true;
                resume.next += 1;
            }
            self.resume.insert((s, start), resume);
        }
    }
}

/// Extensions bat maps through its own configuration rather than through the
/// syntax definitions, so the syntax set alone does not know them.
/// `astro` has no grammar in the set; its template is HTML with expressions,
/// and its frontmatter and blocks are handled as embedded languages.
const EXTENSION_ALIASES: &[(&str, &str)] = &[("jsx", "js"), ("astro", "html")];

/// The syntax for `path`: by extension, then by whole file name, which is how
/// files like `Dockerfile` and `Makefile` are listed.
fn language(path: &str) -> Option<&'static SyntaxReference> {
    let path = Path::new(path);
    let by_extension = path.extension().and_then(|e| e.to_str()).and_then(|e| {
        let e = EXTENSION_ALIASES
            .iter()
            .find(|(alias, _)| *alias == e)
            .map_or(e, |(_, target)| target);
        SYNTAXES.find_syntax_by_extension(e)
    });
    by_extension.or_else(|| {
        let name = path.file_name().and_then(|n| n.to_str())?;
        SYNTAXES.find_syntax_by_extension(name)
    })
}

fn fresh(language: &SyntaxReference, highlighter: &Highlighter) -> SideState {
    SideState {
        parse: ParseState::new(language),
        highlight: HighlightState::new(highlighter, ScopeStack::new()),
    }
}

/// Where parsing for line `i` starts: the first line of its run of visible
/// lines, or of its chunk when the run is longer than one.
fn run_start(visible: &[bool], i: usize) -> usize {
    let floor = i - i % HIGHLIGHT_CHUNK;
    let mut start = i;
    while start > floor && visible.get(start - 1).copied().unwrap_or(false) {
        start -= 1;
    }
    start
}

/// Advances the side states past `line` and returns its spans. A removed line
/// is coloured by the old side, everything else by the new side.
fn line_spans(line: &Line, sides: &mut [SideState; 2], highlighter: &Highlighter) -> Spans {
    let text = format!("{}\n", line.text);
    let mut spans = Spans::new();
    for (side, state) in sides.iter_mut().enumerate() {
        let on_side = match line.kind {
            LineKind::Context => true,
            LineKind::Removed => side == 0,
            LineKind::Added => side == 1,
        };
        if !on_side {
            continue;
        }
        let Ok(ops) = state.parse.parse_line(&text, &SYNTAXES) else {
            return Spans::new();
        };
        let ranges = HighlightIterator::new(&mut state.highlight, &ops, &text, highlighter);
        let coloured_here = (side == 0) == (line.kind == LineKind::Removed);
        if !coloured_here {
            // Context lines are parsed on the old side only to keep its state right.
            ranges.for_each(drop);
            continue;
        }
        let mut at = 0;
        for (style, piece) in ranges {
            let end = (at + piece.len()).min(line.text.len());
            if at < end {
                let c = style.foreground;
                spans.push((Color::Rgb(c.r, c.g, c.b), at..end));
            }
            at += piece.len();
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FileDiff, SectionKind};

    fn section(count: usize) -> Vec<Section> {
        let lines = (0..count)
            .map(|i| Line {
                kind: LineKind::Added,
                old: None,
                new: Some(i + 1),
                text: format!("fn f{i}() -> u32 {{ {i} }}"),
                no_newline: false,
            })
            .collect();
        vec![Section {
            kind: SectionKind::Unstaged,
            diff: FileDiff {
                path: "a.rs".into(),
                lines,
                ..FileDiff::default()
            },
        }]
    }

    #[test]
    fn only_wanted_lines_are_highlighted_and_later_ones_continue() {
        let sections = section(20_000);
        let views = crate::diff::section_views(&sections);
        let mut h = Highlighting::new("a.rs", &sections);
        let theme = Syntax::CatppuccinMacchiato;
        h.ensure("a.rs", &sections, &views, (0..40).map(|i| (0, i)), &theme);
        assert_eq!(h.highlighted(), 40);
        assert!(!h.spans[0][39].is_empty() && h.spans[0][40].is_empty());
        h.ensure("a.rs", &sections, &views, [(0, 60)], &theme);
        assert_eq!(h.highlighted(), 61);
        // Jumping to the end of one long run parses only the last chunk.
        h.ensure("a.rs", &sections, &views, [(0, 19_999)], &theme);
        assert!(h.highlighted() <= 61 + HIGHLIGHT_CHUNK);
        assert!(!h.spans[0][19_999].is_empty());
    }

    #[test]
    fn common_languages_and_named_files_are_recognised() {
        let paths = [
            "a.rs",
            "a.ts",
            "a.tsx",
            "a.js",
            "a.jsx",
            "a.json",
            "a.toml",
            "a.yaml",
            "a.md",
            "a.py",
            "a.go",
            "a.sh",
            "a.css",
            "a.scss",
            "a.html",
            "a.vue",
            "a.svelte",
            "a.astro",
            "a.swift",
            "a.kt",
            "a.dart",
            "a.sql",
            "a.lua",
            "Dockerfile",
            "src/Makefile",
        ];
        let missing: Vec<&str> = paths
            .into_iter()
            .filter(|p| language(p).is_none())
            .collect();
        assert!(missing.is_empty(), "no syntax for {missing:?}");
    }

    #[test]
    fn vue_script_lines_deep_in_a_file_are_highlighted_as_typescript() {
        let mut text = vec![
            "<template>".to_owned(),
            "  <div/>".to_owned(),
            "</template>".to_owned(),
        ];
        text.push("<script setup lang=\"ts\">".to_owned());
        text.extend((0..600).map(|n| format!("const value{n}: number = {n}")));
        text.push("</script>".to_owned());
        let lines: Vec<Line> = text
            .iter()
            .enumerate()
            .map(|(n, t)| Line {
                kind: LineKind::Added,
                old: None,
                new: Some(n + 1),
                text: t.clone(),
                no_newline: false,
            })
            .collect();
        let sections = vec![Section {
            kind: SectionKind::Unstaged,
            diff: FileDiff {
                path: "a.vue".into(),
                lines,
                ..FileDiff::default()
            },
        }];
        let views = crate::diff::section_views(&sections);
        let mut h = Highlighting::new("a.vue", &sections);
        // A line far past the chunk size, inside the script block.
        h.ensure(
            "a.vue",
            &sections,
            &views,
            [(0, 500)],
            &Syntax::CatppuccinMacchiato,
        );
        let colours: std::collections::HashSet<_> =
            h.spans[0][500].iter().map(|(c, _)| *c).collect();
        assert!(colours.len() >= 3, "{:?}", h.spans[0][500]);
        assert!(h.highlighted() <= HIGHLIGHT_CHUNK);
    }

    #[test]
    fn typescript_lines_get_colours() {
        let mut sections = section(1);
        sections[0].diff.lines[0].text = "const x: number = 1;".into();
        let views = crate::diff::section_views(&sections);
        let mut h = Highlighting::new("a.ts", &sections);
        h.ensure(
            "a.ts",
            &sections,
            &views,
            [(0, 0)],
            &Syntax::CatppuccinMacchiato,
        );
        let colours: std::collections::HashSet<_> = h.spans[0][0].iter().map(|(c, _)| *c).collect();
        assert!(colours.len() > 1, "{:?}", h.spans[0][0]);
    }

    #[test]
    fn unknown_languages_stay_plain() {
        let sections = section(10);
        let views = crate::diff::section_views(&sections);
        let mut h = Highlighting::new("a.unknownext", &sections);
        h.ensure(
            "a.unknownext",
            &sections,
            &views,
            [(0, 3)],
            &Syntax::CatppuccinMacchiato,
        );
        assert_eq!(h.highlighted(), 0);
        assert!(h.spans[0][3].is_empty());
    }

    #[test]
    fn bundled_catppuccin_syntax_themes_load() {
        let macchiato = load(MACCHIATO_TMTHEME).expect("macchiato loads");
        let latte = load(LATTE_TMTHEME).expect("latte loads");
        assert_eq!(macchiato.name.as_deref(), Some("Catppuccin Macchiato"));
        assert_eq!(latte.name.as_deref(), Some("Catppuccin Latte"));
        assert!(!macchiato.scopes.is_empty() && !latte.scopes.is_empty());
        for theme in crate::theme::BUILTIN {
            let _ = syntax_theme(&theme.syntax);
        }
        assert_eq!(
            syntax_theme(&Syntax::Builtin("no-such-theme".into())).name,
            THEMES.themes[FALLBACK].name
        );
    }
}
