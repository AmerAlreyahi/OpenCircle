//! A release's notes (GitHub markdown) as plain lines: headings, bullets, text,
//! with links kept apart so the card opens them only on a click. Everything else
//! of markdown (bold, code, images, HTML) is shown as its text.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Heading,
    Bullet,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// An http(s) link: opened in the browser only when clicked.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub spans: Vec<Span>,
}

/// The card shows at most this many lines.
const MAX_LINES: usize = 200;

pub fn parse(markdown: &str) -> Vec<Line> {
    markdown
        .lines()
        .filter_map(|raw| {
            let line = raw.trim();
            let (kind, rest) = if let Some(rest) = heading(line) {
                (Kind::Heading, rest)
            } else if let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|b| line.strip_prefix(b))
            {
                (Kind::Bullet, rest)
            } else {
                (Kind::Text, line)
            };
            let spans = spans(rest);
            (!spans.is_empty()).then_some(Line { kind, spans })
        })
        .take(MAX_LINES)
        .collect()
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    (1..=6)
        .contains(&hashes)
        .then(|| line[hashes..].strip_prefix(' '))
        .flatten()
}

/// `[text](url)` and bare `https://` words become links; `**`, `__` and backticks go.
fn spans(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut push = |text: &str, link: Option<String>| {
        let text = text.replace("**", "").replace("__", "").replace('`', "");
        if text.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.link.is_none() && link.is_none() => last.text.push_str(&text),
            _ => out.push(Span { text, link }),
        }
    };
    let mut rest = text;
    while !rest.is_empty() {
        if let Some((label, url, after)) = md_link(rest) {
            push(label, web(url)); // not a web link: its text only
            rest = after;
            continue;
        }
        if rest.starts_with("https://") || rest.starts_with("http://") {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(['.', ',', ')', ';']);
            push(url, Some(url.to_string()));
            rest = &rest[url.len()..];
            continue;
        }
        // up to the next place a link could start
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let next = rest[first..]
            .find(['[', 'h'])
            .map_or(rest.len(), |i| i + first);
        push(&rest[..next], None);
        rest = &rest[next..];
    }
    out
}

/// `[label](url)rest` at the start of `s`.
fn md_link(s: &str) -> Option<(&str, &str, &str)> {
    let s = s.strip_prefix('[')?;
    let close = s.find("](")?;
    let label = &s[..close];
    let tail = &s[close + 2..];
    let end = tail.find(')')?;
    Some((label, &tail[..end], &tail[end + 1..]))
}

fn web(url: &str) -> Option<String> {
    (url.starts_with("https://") || url.starts_with("http://")).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Span {
        Span {
            text: text.into(),
            link: None,
        }
    }

    #[test]
    fn headings_bullets_and_text_with_links_apart() {
        let md = "## What's new\r\n\n- **Faster** OCR, see [the docs](https://example.com/d)\n* `code` stays\nSee https://github.com/x/y/compare/a...b.\n#nohash\n![img](javascript:alert(1))";
        let lines = parse(md);
        assert_eq!(
            lines[0],
            Line {
                kind: Kind::Heading,
                spans: vec![plain("What's new")]
            }
        );
        assert_eq!(lines[1].kind, Kind::Bullet);
        assert_eq!(
            lines[1].spans,
            vec![
                plain("Faster OCR, see "),
                Span {
                    text: "the docs".into(),
                    link: Some("https://example.com/d".into())
                },
            ]
        );
        assert_eq!(
            lines[2],
            Line {
                kind: Kind::Bullet,
                spans: vec![plain("code stays")]
            }
        );
        assert_eq!(
            lines[3].spans[1].link.as_deref(),
            Some("https://github.com/x/y/compare/a...b")
        );
        assert_eq!(lines[3].spans[2], plain("."));
        assert_eq!(
            lines[4],
            Line {
                kind: Kind::Text,
                spans: vec![plain("#nohash")]
            }
        );
        // a non-web link keeps only its text: nothing to click
        assert!(
            lines[5].spans.iter().all(|s| s.link.is_none()),
            "{:?}",
            lines[5]
        );
        assert_eq!(lines.len(), 6, "blank lines dropped");
    }

    #[test]
    fn long_notes_are_cut() {
        assert_eq!(parse(&"- x\n".repeat(500)).len(), MAX_LINES);
        assert!(parse("").is_empty());
        assert_eq!(
            parse("مرحبا [رابط](https://a.b)")[0].spans[0],
            plain("مرحبا ")
        );
    }
}
