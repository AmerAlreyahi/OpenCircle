//! A code block's colours (the AI chat's answers): syntect's default syntaxes
//! (pure Rust regexes) split each line into kinds (keyword, string, ...), and
//! the view colours a kind from the kit's tokens (`fixed::CODE_*_KINDS`), so
//! every theme, light or dark, gets its own colours and the cache holds none.
//!
//! Highlighting is cached per block: a streamed block only grows, so its
//! finished lines are highlighted once (the parser's state kept after the
//! last one) and only the open last line again for each new piece of text.
//! The syntaxes load on the first code block and go with `release`.
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use syntect::highlighting::{
    Color, HighlightIterator, HighlightState, Highlighter, ScopeSelectors, StyleModifier, Theme,
    ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// What a piece of code is; the view's colour index (`fixed::CODE_*_KINDS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    Text,
    Number,
    Comment,
    Function,
    Type,
}

const KINDS: [Kind; 7] = [
    Kind::Plain,
    Kind::Keyword,
    Kind::Text,
    Kind::Number,
    Kind::Comment,
    Kind::Function,
    Kind::Type,
];

/// The scopes of each kind (TextMate names, the most specific match wins).
const SCOPES: [(Kind, &str); 6] = [
    (
        Kind::Keyword,
        "keyword, storage, constant.language, variable.language",
    ),
    (Kind::Text, "string, constant.character"),
    (Kind::Number, "constant.numeric"),
    (Kind::Comment, "comment, punctuation.definition.comment"),
    (
        Kind::Function,
        "entity.name.function, support.function, variable.function",
    ),
    (
        Kind::Type,
        "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, support.type, support.class",
    ),
];

/// A highlighted line: its pieces, in order.
pub type Line = Vec<(Kind, String)>;

/// Blocks kept (a chat shows a few; older ones are highlighted again if shown).
const KEPT: usize = 48;

struct Loaded {
    syntaxes: SyntaxSet,
    theme: Theme,
    blocks: Vec<Block>,
}

/// One block's progress: the text highlighted so far (whole lines), its lines,
/// and the parser's state after them.
struct Block {
    language: String,
    done: String,
    lines: Vec<Line>,
    state: (ParseState, HighlightState),
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

/// The theme that marks each kind: its index as the foreground's red.
fn theme() -> Theme {
    let mark = |kind: Kind| Color {
        r: kind as u8,
        g: 0,
        b: 0,
        a: 0xff,
    };
    Theme {
        settings: ThemeSettings {
            foreground: Some(mark(Kind::Plain)),
            ..ThemeSettings::default()
        },
        scopes: (SCOPES.iter())
            .map(|(kind, scopes)| ThemeItem {
                scope: ScopeSelectors::from_str(scopes).expect("valid scope selectors"),
                style: StyleModifier {
                    foreground: Some(mark(*kind)),
                    ..StyleModifier::default()
                },
            })
            .collect(),
        ..Theme::default()
    }
}

/// `code`'s lines (no line ends) in kinds, or `None` for a language the
/// syntaxes don't know (plain text then).
pub fn highlight(language: &str, code: &str) -> Option<Arc<Vec<Line>>> {
    let language = language.trim();
    if language.is_empty() {
        return None;
    }
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    let loaded = loaded.get_or_insert_with(|| Loaded {
        syntaxes: SyntaxSet::load_defaults_newlines(),
        theme: theme(),
        blocks: Vec::new(),
    });
    let syntax = loaded.syntaxes.find_syntax_by_token(language)?;
    let highlighter = Highlighter::new(&loaded.theme);
    // the whole lines, and the open last one
    let whole = code.rfind('\n').map_or(0, |i| i + 1);
    let (finished, open) = code.split_at(whole);
    let at = (loaded.blocks.iter())
        .rposition(|b| b.language == language && finished.starts_with(&b.done));
    let mut block = match at {
        Some(i) => loaded.blocks.remove(i),
        None => Block {
            language: language.to_string(),
            done: String::new(),
            lines: Vec::new(),
            state: (
                ParseState::new(syntax),
                HighlightState::new(&highlighter, ScopeStack::new()),
            ),
        },
    };
    for line in finished[block.done.len()..].split_inclusive('\n') {
        let parsed = line_kinds(
            line,
            &mut block.state,
            syntax,
            &loaded.syntaxes,
            &highlighter,
        );
        block.lines.push(parsed);
        block.done.push_str(line);
    }
    let mut lines = block.lines.clone();
    if !open.is_empty() {
        let mut state = block.state.clone();
        let line = format!("{open}\n");
        lines.push(line_kinds(
            &line,
            &mut state,
            syntax,
            &loaded.syntaxes,
            &highlighter,
        ));
    }
    loaded.blocks.push(block);
    if loaded.blocks.len() > KEPT {
        loaded.blocks.remove(0);
    }
    Some(Arc::new(lines))
}

/// One line (with its line end) in kinds, the state moved past it.
fn line_kinds(
    line: &str,
    (parse, highlight): &mut (ParseState, HighlightState),
    syntax: &SyntaxReference,
    syntaxes: &SyntaxSet,
    highlighter: &Highlighter,
) -> Line {
    let plain = || vec![(Kind::Plain, line.trim_end_matches(['\n', '\r']).to_string())];
    let Ok(ops) = parse.parse_line(line, syntaxes) else {
        // (a syntax's regex failed: this line plain, the next from scratch)
        *parse = ParseState::new(syntax);
        return plain();
    };
    let mut out: Line = Vec::new();
    for (style, piece) in HighlightIterator::new(highlight, &ops, line, highlighter) {
        let piece = piece.trim_end_matches(['\n', '\r']);
        if piece.is_empty() {
            continue;
        }
        let kind = KINDS
            .get(style.foreground.r as usize)
            .copied()
            .unwrap_or(Kind::Plain);
        match out.last_mut() {
            Some((last, text)) if *last == kind => text.push_str(piece),
            _ => out.push((kind, piece.to_string())),
        }
    }
    out
}

/// Frees the syntaxes and the cache (the chat closed).
pub fn release() {
    *LOADED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lines: &[Line]) -> Vec<Vec<Kind>> {
        lines
            .iter()
            .map(|l| l.iter().map(|(k, _)| *k).collect())
            .collect()
    }

    #[test]
    fn a_known_language_is_split_into_kinds() {
        let lines = highlight("rust", "// hi\nfn main() { let x = \"a\"; 42 }\n").unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(kinds(&lines)[0], vec![Kind::Comment]);
        let line = &lines[1];
        let text: String = line.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(
            text, "fn main() { let x = \"a\"; 42 }",
            "nothing lost or added"
        );
        for (kind, piece) in [
            (Kind::Keyword, "fn"),
            (Kind::Function, "main"),
            (Kind::Text, "\"a\""),
            (Kind::Number, "42"),
        ] {
            assert!(
                line.iter().any(|(k, s)| *k == kind && s.contains(piece)),
                "{piece} is {kind:?}: {line:?}"
            );
        }
    }

    #[test]
    fn an_unknown_or_missing_language_is_plain() {
        assert!(highlight("no-such-language", "x").is_none());
        assert!(highlight("", "x").is_none());
    }

    /// A streamed block: each piece gives what a whole parse gives, the
    /// finished lines from the kept state.
    #[test]
    fn a_growing_block_matches_a_whole_parse() {
        let code = "def f(x):\n    \"\"\"doc\n    more\"\"\"\n    return x + 1  # one\n";
        let whole = highlight("python", code).unwrap();
        release();
        let mut last = None;
        for end in 1..=code.len() {
            if code.is_char_boundary(end) {
                last = highlight("python", &code[..end]);
            }
        }
        assert_eq!(last.unwrap(), whole);
        let loaded = LOADED.lock().unwrap();
        let blocks = (loaded.as_ref().unwrap().blocks.iter()).filter(|b| b.language == "python");
        assert_eq!(blocks.count(), 1, "one block, carried forward");
    }
}
