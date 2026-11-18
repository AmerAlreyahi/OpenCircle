//! Maths in an AI answer made readable without a math engine (W52): models
//! still send LaTeX (`$...$`, `$$...$$`, `\(...\)`, `\[...\]`) although the
//! system prompt asks for plain Unicode. Before the Markdown is parsed, each
//! formula becomes Unicode: Greek, operators (· ≤ ≥ ≠ ∑ ∈ ∧ ⌊ ⌋ ...), sub- and
//! superscripts where Unicode has them, fractions as a/b, `\text{}` as its
//! text, `\begin{cases}` as aligned lines behind a tall brace. A display
//! formula (or one over several lines) is a ```` ```math ```` block, which the
//! kit draws as a quiet monospace block (`kit::markdown`); an inline one stays
//! in its line. Code (fenced or inline) is never touched; an unknown command is
//! left as written; a lone `$` (a price) stays text.
use std::borrow::Cow;

/// The code block language a display formula is given (`kit::markdown`).
pub const LANGUAGE: &str = "math";

/// `md` with its formulas made readable (borrowed when it has none).
pub fn readable(md: &str) -> Cow<'_, str> {
    if !md.contains('$') && !md.contains("\\(") && !md.contains("\\[") {
        return Cow::Borrowed(md);
    }
    let mut out = String::with_capacity(md.len());
    let mut prose = String::new();
    let mut fence: Option<(char, usize)> = None;
    for line in md.split_inclusive('\n') {
        let marker = fence_marker(line);
        match (fence, marker) {
            (None, Some(m)) => {
                out.push_str(&formulas(&std::mem::take(&mut prose)));
                fence = Some(m);
                out.push_str(line);
            }
            (Some((c, n)), Some((c2, n2))) if c == c2 && n2 >= n && line.trim().len() == n2 => {
                fence = None;
                out.push_str(line);
            }
            (Some(_), _) => out.push_str(line),
            (None, None) => prose.push_str(line),
        }
    }
    out.push_str(&formulas(&prose));
    Cow::Owned(out)
}

/// A line opening or closing a code fence: its character and length.
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

/// Prose (no fenced code) with its formulas converted; inline code kept.
fn formulas(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let find = |from: usize, pat: &[char]| {
        (from..cs.len().saturating_sub(pat.len() - 1)).find(|&j| cs[j..].starts_with(pat))
    };
    while i < cs.len() {
        let c = cs[i];
        let next = cs.get(i + 1).copied();
        // inline code: as written, up to the same run of backticks
        if c == '`' {
            let n = cs[i..].iter().take_while(|&&x| x == '`').count();
            let run = vec!['`'; n];
            let end = (find(i + n, &run)).filter(|&j| cs.get(j + n) != Some(&'`'));
            let to = end.map_or(i + n, |j| j + n);
            out.extend(&cs[i..to]);
            i = to;
            continue;
        }
        let (open, close, display): (usize, &[char], bool) = match (c, next) {
            ('\\', Some('[')) => (2, &['\\', ']'], true),
            ('\\', Some('(')) => (2, &['\\', ')'], false),
            ('\\', Some(n)) => {
                out.push(c);
                out.push(n);
                i += 2;
                continue;
            }
            ('$', Some('$')) => (2, &['$', '$'], true),
            ('$', Some(n)) if !n.is_whitespace() => (1, &['$'], false),
            _ => {
                out.push(c);
                i += 1;
                continue;
            }
        };
        let end = match close {
            // a single `$` closes on its line, after a non-space, not before a digit
            ['$'] => (i + 2..cs.len()).take_while(|&j| cs[j] != '\n').find(|&j| {
                cs[j] == '$'
                    && cs[j - 1] != '\\'
                    && !cs[j - 1].is_whitespace()
                    && !cs.get(j + 1).is_some_and(|d| d.is_ascii_digit())
            }),
            _ => find(i + open, close),
        };
        let Some(end) = end else {
            out.extend(&cs[i..i + open]);
            i += open;
            continue;
        };
        let source: String = cs[i + open..end].iter().collect();
        let lines = latex(&source);
        i = end + close.len();
        if display || lines.len() > 1 {
            out.truncate(out.trim_end_matches([' ', '\t']).len());
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```");
            out.push_str(LANGUAGE);
            out.push('\n');
            for line in &lines {
                out.push_str(line.trim_end());
                out.push('\n');
            }
            out.push_str("```\n");
            while cs.get(i).is_some_and(|c| *c == ' ' || *c == '\t') {
                i += 1;
            }
            if cs.get(i) == Some(&'\n') {
                i += 1;
            }
        } else {
            for ch in lines.concat().trim().chars() {
                if matches!(ch, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '|' | '~') {
                    out.push('\\');
                }
                out.push(ch);
            }
        }
    }
    out
}

/// Groups, arguments and commands nested deeper than this, or a formula
/// longer than `MAX_LEN` characters: the formula is left as written (a model's
/// answer is untrusted: no stack overflow, no runaway work).
const MAX_DEPTH: usize = 64;
const MAX_LEN: usize = 20_000;

/// A formula's LaTeX as Unicode lines (one, unless it has rows).
pub fn latex(source: &str) -> Vec<String> {
    latex_in(source, 0)
}

/// `latex`, `depth` levels down already (a formula inside `\text{}`).
fn latex_in(source: &str, depth: usize) -> Vec<String> {
    let cs: Vec<char> = source.chars().collect();
    if cs.len() > MAX_LEN {
        return vec![source.to_string()];
    }
    let mut p = Parser {
        s: &cs,
        i: 0,
        depth,
        too_deep: false,
    };
    let mut rows = Vec::new();
    loop {
        rows.push(p.seq(false));
        if p.eat_str("\\\\") {
            p.skip_optional();
            continue;
        }
        if p.i >= cs.len() {
            break;
        }
        p.i += 1; // a stray `}`, `&` or `\end`: skipped
        if p.i >= cs.len() {
            break;
        }
    }
    if p.too_deep {
        return vec![source.to_string()];
    }
    let lines: Vec<String> = rows
        .into_iter()
        .flat_map(|b| match b.lines.len() {
            // (one line: its spaces as one; rows keep their alignment)
            1 => vec![squeeze(&b.lines[0])],
            _ => b.lines.iter().map(|l| l.trim_end().to_string()).collect(),
        })
        .filter(|l| !l.trim().is_empty())
        .collect();
    match lines.is_empty() {
        true => vec![String::new()],
        false => lines,
    }
}

/// Runs of spaces in a line's text as one (its leading indent kept).
fn squeeze(line: &str) -> String {
    let body = line.trim_start();
    let mut out: String = line[..line.len() - body.len()].into();
    let mut space = false;
    for c in body.chars() {
        if c == ' ' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out.trim_end().into()
}

/// Lines of text laid side by side on a common baseline (a brace's middle).
#[derive(Debug, Clone, Default)]
struct Block {
    lines: Vec<String>,
    base: usize,
}

impl Block {
    fn text(s: impl Into<String>) -> Block {
        Block {
            lines: vec![s.into()],
            base: 0,
        }
    }

    fn width(&self) -> usize {
        self.lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
    }

    fn flat(&self) -> String {
        self.lines.join(" ")
    }

    fn push_str(&mut self, s: &str) {
        self.push(Block::text(s));
    }

    /// `other` at the right, baselines aligned.
    fn push(&mut self, other: Block) {
        if self.lines.is_empty() {
            *self = other;
            return;
        }
        if self.lines.len() == 1 && other.lines.len() == 1 {
            self.lines[0].push_str(&other.lines[0]);
            return;
        }
        let base = self.base.max(other.base);
        let below = (self.lines.len() - self.base).max(other.lines.len() - other.base);
        let width = self.width();
        let at = |b: &Block, row: usize| -> String {
            (row + b.base)
                .checked_sub(base)
                .and_then(|r| b.lines.get(r))
                .cloned()
                .unwrap_or_default()
        };
        let lines = (0..base + below)
            .map(|row| {
                let left = at(self, row);
                let pad = width - left.chars().count();
                format!("{left}{}{}", " ".repeat(pad), at(&other, row))
            })
            .collect();
        *self = Block { lines, base };
    }
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    /// How deep in groups / arguments / commands it is (`MAX_DEPTH`).
    depth: usize,
    /// Past `MAX_DEPTH`: the formula stays as written.
    too_deep: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn eat_str(&mut self, pat: &str) -> bool {
        let pat: Vec<char> = pat.chars().collect();
        let hit = self.s[self.i.min(self.s.len())..].starts_with(&pat);
        if hit {
            self.i += pat.len();
        }
        hit
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    /// An optional `[...]` (a row's spacing, a root's degree): skipped.
    fn skip_optional(&mut self) -> Option<String> {
        self.skip_spaces();
        if self.peek() != Some('[') {
            return None;
        }
        let from = self.i + 1;
        let end = (from..self.s.len()).find(|&j| self.s[j] == ']')?;
        self.i = end + 1;
        Some(self.s[from..end].iter().collect())
    }

    /// The text of a `{...}` taken as it is (an environment's name, a column spec).
    fn raw_group(&mut self) -> String {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return String::new();
        }
        let mut depth = 0;
        let from = self.i + 1;
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return self.s[from..self.i - 1].iter().collect();
                    }
                }
                _ => {}
            }
        }
        self.s[from..].iter().collect()
    }

    /// Until the end, a `}`, and in an environment `&`, `\\` or `\end`.
    fn seq(&mut self, env: bool) -> Block {
        let mut out = Block::default();
        while let Some(c) = self.peek() {
            let rest = &self.s[self.i..];
            if c == '}'
                || (rest.starts_with(&['\\', '\\']))
                || (env && (c == '&' || rest.starts_with(&['\\', 'e', 'n', 'd'])))
            {
                break;
            }
            if c == '&' {
                self.i += 1;
                out.push_str(" ");
                continue;
            }
            let atom = self.atom();
            out.push(atom);
        }
        out
    }

    /// `step` one level deeper; past `MAX_DEPTH` nothing more is read.
    fn nested(&mut self, step: fn(&mut Self) -> Block) -> Block {
        if self.depth >= MAX_DEPTH {
            self.too_deep = true;
            self.i = self.s.len();
            return Block::default();
        }
        self.depth += 1;
        let block = step(self);
        self.depth -= 1;
        block
    }

    /// One piece: a group, a script, a command or a character.
    fn atom(&mut self) -> Block {
        self.nested(Self::atom_here)
    }

    fn atom_here(&mut self) -> Block {
        let Some(c) = self.peek() else {
            return Block::default();
        };
        self.i += 1;
        match c {
            '{' => {
                let inner = self.seq(false);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                inner
            }
            '^' | '_' => {
                let arg = self.arg().flat();
                Block::text(script(&arg, c == '^'))
            }
            '~' => Block::text(" "),
            c if c.is_whitespace() => Block::text(" "),
            '\\' => self.command(),
            '\'' => Block::text("′"),
            c => Block::text(c.to_string()),
        }
    }

    /// A command's argument: a group or one piece.
    fn arg(&mut self) -> Block {
        self.nested(Self::arg_here)
    }

    fn arg_here(&mut self) -> Block {
        self.skip_spaces();
        match self.peek() {
            Some('{') => self.atom(),
            Some('\\') => {
                self.i += 1;
                self.command()
            }
            Some(c) => {
                self.i += 1;
                Block::text(c.to_string())
            }
            None => Block::default(),
        }
    }

    /// After a `\`: the command's text (an environment too).
    fn command(&mut self) -> Block {
        self.nested(Self::command_here)
    }

    fn command_here(&mut self) -> Block {
        let from = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i == from {
            // a one-character command: `\,` `\{` `\%` ...
            let Some(c) = self.peek() else {
                return Block::text("\\");
            };
            self.i += 1;
            return Block::text(match c {
                ',' | ':' | ';' | ' ' => " ".to_string(),
                '!' => String::new(),
                '|' => "‖".into(),
                c => c.to_string(),
            });
        }
        let name: String = self.s[from..self.i].iter().collect();
        if let Some(sym) = symbol(&name) {
            return Block::text(sym);
        }
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let (a, b) = (self.arg().flat(), self.arg().flat());
                Block::text(format!("{}/{}", wrap(a.trim()), wrap(b.trim())))
            }
            "binom" | "dbinom" | "tbinom" => {
                let (n, k) = (self.arg().flat(), self.arg().flat());
                Block::text(format!("C({}, {})", n.trim(), k.trim()))
            }
            "sqrt" => {
                let degree = self.skip_optional();
                let x = self.arg().flat();
                let root = match degree.as_deref().map(str::trim) {
                    Some("3") => "∛",
                    Some("4") => "∜",
                    _ => "√",
                };
                Block::text(format!("{root}{}", wrap(x.trim())))
            }
            "text" | "textrm" | "textit" | "textbf" | "mathrm" | "mathit" | "mathbf" | "mathsf"
            | "mathtt" | "boldsymbol" | "operatorname" | "mathcal" | "mathfrak" | "mbox" => {
                self.skip_spaces();
                match self.peek() {
                    Some('{') => Block::text(self.raw_text()),
                    _ => self.arg(),
                }
            }
            "mathbb" => {
                let x = self.arg().flat();
                Block::text(x.chars().map(double_struck).collect::<String>())
            }
            "hat" | "widehat" => accent(self.arg().flat(), '\u{302}'),
            "bar" | "overline" => accent(self.arg().flat(), '\u{305}'),
            "vec" => accent(self.arg().flat(), '\u{20D7}'),
            "tilde" | "widetilde" => accent(self.arg().flat(), '\u{303}'),
            "dot" => accent(self.arg().flat(), '\u{307}'),
            "ddot" => accent(self.arg().flat(), '\u{308}'),
            "pmod" => Block::text(format!(" (mod {})", self.arg().flat().trim())),
            "left" | "right" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "big"
            | "Big" | "bigg" | "Bigg" | "middle" => {
                self.skip_spaces();
                if self.peek() == Some('.') {
                    self.i += 1;
                }
                Block::default()
            }
            "displaystyle" | "textstyle" | "limits" | "nolimits" | "notag" | "nonumber" => {
                Block::default()
            }
            "begin" => self.environment(),
            // an unknown command: as written
            _ => Block::text(format!("\\{name}")),
        }
    }

    /// A `{...}` read as text (`\text{if }`: its spaces kept, `$..$` inside converted).
    fn raw_text(&mut self) -> String {
        let raw = self.raw_group();
        let mut out = String::new();
        let mut parts = raw.split('$');
        if let Some(first) = parts.next() {
            out.push_str(first);
        }
        for (k, part) in parts.enumerate() {
            match k % 2 {
                0 => out.push_str(&latex_in(part, self.depth + 1).concat()),
                _ => out.push_str(part),
            }
        }
        out.replace('~', " ")
    }

    /// `\begin{name} ... \end{name}`: rows of cells, aligned; cases behind a brace.
    fn environment(&mut self) -> Block {
        let name = self.raw_group();
        if name.starts_with("array") || name == "alignat" || name == "alignat*" {
            let _ = self.raw_group(); // its column spec
        }
        let mut rows: Vec<Vec<String>> = Vec::new();
        loop {
            let mut cells = Vec::new();
            loop {
                let cell = self.seq(true);
                cells.push(squeeze(cell.flat().trim()));
                if self.peek() == Some('&') {
                    self.i += 1;
                    continue;
                }
                break;
            }
            rows.push(cells);
            if self.eat_str("\\\\") {
                let _ = self.skip_optional();
                continue;
            }
            if self.eat_str("\\end") {
                let _ = self.raw_group();
            } else if self.peek() == Some('}') {
                self.i += 1; // (a stray brace)
                continue;
            }
            break;
        }
        while rows.last().is_some_and(|r| r.iter().all(String::is_empty)) {
            rows.pop();
        }
        if rows.is_empty() {
            return Block::default();
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|c| {
                (rows.iter().filter_map(|r| r.get(c)))
                    .map(|s| s.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let gap = match name.trim_end_matches('*') {
            "cases" | "dcases" | "rcases" | "matrix" | "pmatrix" | "bmatrix" | "vmatrix"
            | "Vmatrix" | "array" => "   ",
            _ => " ",
        };
        let lines: Vec<String> = rows
            .iter()
            .map(|r| {
                let cells = (0..columns).map(|c| {
                    let s = r.get(c).map_or("", String::as_str);
                    format!("{s}{}", " ".repeat(widths[c] - s.chars().count()))
                });
                cells.collect::<Vec<_>>().join(gap).trim_end().to_string()
            })
            .collect();
        let n = lines.len();
        let (open, close): (Piece, Piece) = match name.trim_end_matches('*') {
            "cases" | "dcases" => (brace, |_, _| ""),
            "pmatrix" => (
                |i, n| tall(i, n, ["(", "⎛", "⎜", "⎝"]),
                |i, n| tall(i, n, [")", "⎞", "⎟", "⎠"]),
            ),
            "bmatrix" => (
                |i, n| tall(i, n, ["[", "⎡", "⎢", "⎣"]),
                |i, n| tall(i, n, ["]", "⎤", "⎥", "⎦"]),
            ),
            "vmatrix" => (|_, _| "|", |_, _| "|"),
            _ => (|_, _| "", |_, _| ""),
        };
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let lines = (lines.iter().enumerate())
            .map(|(i, l)| {
                let (o, c) = (open(i, n), close(i, n));
                let pad = " ".repeat(width - l.chars().count());
                match (o.is_empty(), c.is_empty()) {
                    (true, true) => l.clone(),
                    (false, true) => format!("{o} {l}"),
                    _ => format!("{o} {l}{pad} {c}"),
                }
            })
            .collect();
        Block {
            lines,
            base: (n - 1) / 2,
        }
    }
}

/// A tall delimiter's piece for row `i` of `n`.
type Piece = fn(usize, usize) -> &'static str;

/// Row `i` of `n` of a tall delimiter: one row `one`, else top, middle, bottom.
fn tall(i: usize, n: usize, [one, top, mid, bottom]: [&'static str; 4]) -> &'static str {
    match (n, i) {
        (1, _) => one,
        (_, 0) => top,
        (n, i) if i + 1 == n => bottom,
        _ => mid,
    }
}

/// The cases brace's piece for row `i` of `n` (its middle points at the rows).
fn brace(i: usize, n: usize) -> &'static str {
    match (n, i) {
        (1, _) => "{",
        (_, 0) => "⎧",
        (n, i) if i + 1 == n => "⎩",
        (n, i) if i == (n - 1) / 2 => "⎨",
        _ => "⎪",
    }
}

/// A fraction's part: as it is when one word or number, else in brackets.
fn wrap(s: &str) -> String {
    let number = s.chars().all(|c| c.is_ascii_digit() || c == '.');
    let word = s.chars().all(char::is_alphabetic);
    match s.chars().count() <= 1 || number || word {
        true => s.to_string(),
        false => format!("({s})"),
    }
}

/// `x` with a combining mark on each letter (`\hat`, `\bar`, `\vec` ...).
fn accent(x: String, mark: char) -> Block {
    Block::text(x.trim().chars().flat_map(|c| [c, mark]).collect::<String>())
}

/// A super- (`up`) or subscript: Unicode's own letters where all exist, else
/// `^x` / `_x` (one word) or `^(...)` / `_(...)`.
fn script(arg: &str, up: bool) -> String {
    let arg = arg.trim();
    let mapped: Option<String> = arg
        .chars()
        .map(|c| match up {
            true => superscript(c),
            false => subscript(c),
        })
        .collect();
    let mark = if up { '^' } else { '_' };
    match mapped {
        Some(m) if !m.is_empty() => m,
        _ if arg.chars().all(char::is_alphanumeric) => format!("{mark}{arg}"),
        _ => format!("{mark}({arg})"),
    }
}

fn superscript(c: char) -> Option<char> {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    Some(match c {
        '0'..='9' => DIGITS[c as usize - '0' as usize],
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'T' => 'ᵀ',
        '′' => '′',
        '*' | '∗' => '*',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    const DIGITS: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
    Some(match c {
        '0'..='9' => DIGITS[c as usize - '0' as usize],
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        _ => return None,
    })
}

/// `\mathbb`'s letters.
fn double_struck(c: char) -> char {
    match c {
        'N' => 'ℕ',
        'Z' => 'ℤ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'C' => 'ℂ',
        'P' => 'ℙ',
        'E' => '𝔼',
        c => c,
    }
}

/// A command that is one symbol or word.
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "cdot" | "cdotp" | "centerdot" => "·",
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "ast" => "∗",
        "star" => "⋆",
        "circ" => "∘",
        "bullet" => "•",
        "oplus" => "⊕",
        "otimes" => "⊗",
        "le" | "leq" | "leqslant" => "≤",
        "ge" | "geq" | "geqslant" => "≥",
        "lt" => "<",
        "gt" => ">",
        "ne" | "neq" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "cong" => "≅",
        "sim" => "∼",
        "simeq" => "≃",
        "propto" => "∝",
        "ll" => "≪",
        "gg" => "≫",
        "infty" => "∞",
        "sum" => "∑",
        "prod" => "∏",
        "coprod" => "∐",
        "int" => "∫",
        "iint" => "∬",
        "oint" => "∮",
        "partial" => "∂",
        "nabla" => "∇",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        "setminus" => "∖",
        "emptyset" | "varnothing" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "neg" | "lnot" => "¬",
        "land" | "wedge" => "∧",
        "lor" | "vee" => "∨",
        "to" | "rightarrow" => "→",
        "leftarrow" | "gets" => "←",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "leftrightarrow" => "↔",
        "Leftrightarrow" | "iff" => "⇔",
        "implies" => "⟹",
        "impliedby" => "⟸",
        "mapsto" => "↦",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lvert" | "rvert" | "vert" | "mid" => "|",
        "lVert" | "rVert" | "Vert" | "parallel" => "‖",
        "perp" => "⊥",
        "angle" => "∠",
        "triangle" => "△",
        "degree" => "°",
        "prime" => "′",
        "ell" => "ℓ",
        "hbar" => "ℏ",
        "aleph" => "ℵ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "ldots" | "dots" | "dotsc" | "dotsb" => "…",
        "cdots" => "⋯",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "quad" => "  ",
        "qquad" => "    ",
        "colon" => ":",
        "lbrace" => "{",
        "rbrace" => "}",
        "bmod" | "mod" => " mod ",
        "gcd" => "gcd",
        "lcm" => "lcm",
        "max" => "max",
        "min" => "min",
        "sup" => "sup",
        "inf" => "inf",
        "lim" => "lim",
        "log" => "log",
        "ln" => "ln",
        "lg" => "lg",
        "exp" => "exp",
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "arcsin" => "arcsin",
        "arccos" => "arccos",
        "arctan" => "arctan",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "det" => "det",
        "deg" => "deg",
        "dim" => "dim",
        "ker" => "ker",
        "arg" => "arg",
        "Pr" => "Pr",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(s: &str) -> String {
        latex(s).join("\n")
    }

    #[test]
    fn symbols_scripts_and_fractions() {
        assert_eq!(one(r"\alpha \le \beta \neq \gamma"), "α ≤ β ≠ γ");
        assert_eq!(one(r"x^2 + y_{n-1} = \frac{1}{2}"), "x² + yₙ₋₁ = 1/2");
        assert_eq!(one(r"\frac{n+1}{2k}"), "(n+1)/(2k)");
        assert_eq!(one(r"\sum_{i=1}^{n} i \cdot 2"), "∑ᵢ₌₁ⁿ i · 2");
        assert_eq!(one(r"\lfloor n/10 \rfloor \bmod 3"), "⌊ n/10 ⌋ mod 3");
        assert_eq!(one(r"a \land b \in S"), "a ∧ b ∈ S");
        assert_eq!(one(r"C_{new} \leftarrow x"), "C_new ← x");
        assert_eq!(one(r"\gcd(a, b) \ge 2"), "gcd(a, b) ≥ 2");
        assert_eq!(one(r"\text{if } n = 0"), "if n = 0");
        assert_eq!(one(r"\sqrt{x^2+1}"), "√(x²+1)");
        assert_eq!(one(r"\mathbb{R}"), "ℝ");
        // an unknown command stays as written
        assert_eq!(one(r"\foo{x}"), r"\foox");
    }

    /// The owner's screen (2026-11-18): cases as rows behind a brace, the
    /// left-hand side on the brace's middle.
    #[test]
    fn cases_are_rows_behind_a_brace() {
        let lines = latex(
            r"f(n) = \begin{cases} 1 & \text{if } n = 0 \\ n \cdot f(n-1) & \text{if } n > 0 \end{cases}",
        );
        assert_eq!(
            lines,
            [
                "f(n) = ⎧ 1            if n = 0",
                "       ⎩ n · f(n-1)   if n > 0"
            ],
        );
        let three = latex(r"\begin{cases} a & x \\ b & y \\ c & z \end{cases}");
        assert_eq!(three, ["⎧ a   x", "⎨ b   y", "⎩ c   z"]);
    }

    /// A model's answer is untrusted: braces or commands nested past the
    /// limit, or a huge formula, come back as written, never a stack overflow.
    #[test]
    fn deep_or_huge_formulas_stay_as_written() {
        // (under the length cap: the depth limit stops it)
        let braces = format!("{}x{}", "{".repeat(5_000), "}".repeat(5_000));
        assert_eq!(latex(&braces), std::slice::from_ref(&braces));
        let ten = format!("{}x{}", "{".repeat(10_000), "}".repeat(10_000));
        assert_eq!(latex(&ten), std::slice::from_ref(&ten));
        let fracs = format!("{}1{}", r"\frac{".repeat(5_000), "}{2}".repeat(5_000));
        assert_eq!(latex(&fracs), std::slice::from_ref(&fracs));
        let begins = r"\begin{cases}".repeat(5_000);
        assert_eq!(latex(&begins), std::slice::from_ref(&begins));
        let md = format!("$${braces}$$");
        assert!(readable(&md).contains(&braces));
        let long = "x+".repeat(MAX_LEN);
        assert_eq!(latex(&long), std::slice::from_ref(&long));
        // shallow nesting still converts
        assert_eq!(one(r"\frac{\frac{1}{2}}{3}"), "(1/2)/3");
    }

    #[test]
    fn formulas_in_markdown_and_code_left_alone() {
        // (the block the kit draws as a quiet one)
        assert_eq!(LANGUAGE, crate::shared::kit::markdown::MATH);
        let md = "Let $s = c_1 c_2$. Then:\n$$F(n) = F(n-1) + F(n-2)$$\nDone.";
        assert_eq!(
            readable(md),
            "Let s = c₁ c₂. Then:\n```math\nF(n) = F(n-1) + F(n-2)\n```\nDone."
        );
        // prices, code and inline code stay as written
        let plain = "It costs $5 and $10.\n```sh\necho $HOME $x$\n```\nRun `$PATH$` now.";
        assert_eq!(readable(plain), plain);
        assert_eq!(
            readable(r"\(a \le b\) and \[x^2\]"),
            "a ≤ b and\n```math\nx²\n```\n"
        );
        // an unfinished formula (still streaming) stays as it came
        assert_eq!(readable("so $$x = \\alpha"), "so $$x = \\alpha");
        // markdown's own characters in an inline formula are escaped
        assert_eq!(readable("$a*b*c$"), r"a\*b\*c");
    }
}
