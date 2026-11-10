//! A release's notes (GitHub markdown) for the What's new card (owner,
//! 2026-11-10): the summary (the first paragraph, usually bold), the
//! Highlights (at most `HIGHLIGHTS` shown, the rest counted), and the Fixes and
//! Upgrade notes (folded on the card, with their counts). The other sections
//! (Install, Security, Known issues, Thanks) stay on GitHub ("Full release
//! notes"). Sections are found by their `##` names; a bullet hard-wrapped onto
//! the next lines is joined. Inline: bold, code (drawn as key caps) and links,
//! shown by a short label (`#153`, the site's name) and opened only when clicked.

/// How a run of text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    Bold,
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// An http(s) link: opened in the browser only when clicked.
    pub link: Option<String>,
}

/// One paragraph or bullet.
pub type Line = Vec<Span>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    pub summary: Line,
    pub highlights: Vec<Line>,
    /// Highlights past `HIGHLIGHTS` (in the full notes).
    pub more: usize,
    pub fixes: Vec<Line>,
    pub upgrade: Vec<Line>,
}

/// The card shows at most this many highlights.
pub const HIGHLIGHTS: usize = 6;
/// A folded section keeps at most this many lines.
const MAX_ITEMS: usize = 40;
/// Notes with nothing to show.
const EMPTY: &str = "See the release page for what changed.";

/// A block of the notes: a bullet or a paragraph (its lines joined).
#[derive(Debug, PartialEq)]
enum Block {
    Bullet(String),
    Para(String),
}

/// The sections in order: `None` = before the first heading.
fn sections(markdown: &str) -> Vec<(Option<String>, Vec<Block>)> {
    let mut out: Vec<(Option<String>, Vec<Block>)> = vec![(None, Vec::new())];
    let mut open = false; // the last block takes the next line
    for raw in markdown.lines() {
        let line = raw.trim();
        if line.is_empty() {
            open = false;
            continue;
        }
        if let Some(name) = heading(line) {
            out.push((Some(name.trim().to_string()), Vec::new()));
            open = false;
            continue;
        }
        let blocks = &mut out.last_mut().expect("never empty").1;
        if let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|b| line.strip_prefix(b)) {
            blocks.push(Block::Bullet(rest.trim().to_string()));
        } else if let (true, Some(Block::Bullet(s) | Block::Para(s))) = (open, blocks.last_mut()) {
            s.push(' ');
            s.push_str(line);
        } else {
            blocks.push(Block::Para(line.to_string()));
        }
        open = true;
    }
    out
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    (1..=6)
        .contains(&hashes)
        .then(|| line[hashes..].strip_prefix(' '))
        .flatten()
}

pub fn parse(markdown: &str) -> Notes {
    let sections = sections(markdown);
    let named = |want: &[&str]| {
        (sections.iter()).find(|(name, _)| {
            name.as_deref().is_some_and(|n| {
                let n = n.to_lowercase();
                want.iter().any(|w| n.starts_with(w))
            })
        })
    };
    let bullets = |blocks: &[Block]| -> Vec<Line> {
        (blocks.iter())
            .filter_map(|b| match b {
                Block::Bullet(s) => Some(inline(s)),
                Block::Para(_) => None,
            })
            .take(MAX_ITEMS)
            .collect()
    };
    let paragraph = (sections.iter())
        .flat_map(|(_, blocks)| blocks)
        .find_map(|b| match b {
            Block::Para(s) => Some(s.as_str()),
            Block::Bullet(_) => None,
        });
    // no Highlights section: the bullets before the first heading (plain notes)
    let highlights = match named(&["highlight"]) {
        Some((_, blocks)) => bullets(blocks),
        None => bullets(&sections[0].1),
    };
    let more = highlights.len().saturating_sub(HIGHLIGHTS);
    let mut notes = Notes {
        summary: paragraph
            .map(|p| inline(&drop_title(p)))
            .unwrap_or_default(),
        highlights: highlights.into_iter().take(HIGHLIGHTS).collect(),
        more,
        fixes: named(&["fix", "bug fix"]).map_or_else(Vec::new, |(_, b)| bullets(b)),
        upgrade: named(&["upgrade"]).map_or_else(Vec::new, |(_, b)| bullets(b)),
    };
    if notes.summary.is_empty() && notes.highlights.is_empty() {
        notes.summary = inline(EMPTY);
    }
    notes
}

/// "**OpenCircle v0.1.4: Ask AI ...**" -> "**Ask AI ...**": the card's title
/// already names the version.
fn drop_title(p: &str) -> String {
    let (marks, rest) = match p.strip_prefix("**") {
        Some(rest) => ("**", rest),
        None => ("", p),
    };
    let tail = (rest.strip_prefix("OpenCircle "))
        .and_then(|after| after.split_once(": "))
        .filter(|(version, _)| {
            version
                .trim_start_matches('v')
                .starts_with(|c: char| c.is_ascii_digit())
                && !version.contains(' ')
        });
    match tail {
        Some((_, tail)) => format!("{marks}{tail}"),
        None => p.to_string(),
    }
}

/// Bold (`**`, `__`), code (backticks), `[label](url)`, `![alt](url)` and bare
/// `https://` links. A link that isn't a web link keeps only its text.
fn inline(text: &str) -> Line {
    let mut out: Line = Vec::new();
    let mut bold = false;
    let mut push = |text: &str, style: Style, link: Option<String>| {
        if text.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.link.is_none() && link.is_none() && last.style == style => {
                last.text.push_str(text)
            }
            _ => out.push(Span {
                text: text.to_string(),
                style,
                link,
            }),
        }
    };
    let style = |bold: bool| if bold { Style::Bold } else { Style::Plain };
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**").or_else(|| rest.strip_prefix("__")) {
            bold = !bold;
            rest = after;
            continue;
        }
        if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
        {
            push(&after[..end], Style::Code, None);
            rest = &after[end + 1..];
            continue;
        }
        let image = rest.starts_with("![");
        if let Some((label, url, after)) = md_link(rest.strip_prefix('!').unwrap_or(rest)) {
            let link = web(url).filter(|_| !image);
            push(label, style(bold), link);
            rest = after;
            continue;
        }
        if rest.starts_with("https://") || rest.starts_with("http://") {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(['.', ',', ')', ';', ':']);
            push(&short(url), style(bold), Some(url.to_string()));
            rest = &rest[url.len()..];
            continue;
        }
        // up to the next place something could start
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let next = rest[first..]
            .find(['[', '!', 'h', '*', '_', '`'])
            .map_or(rest.len(), |i| i + first);
        push(&rest[..next], style(bold), None);
        rest = &rest[next..];
    }
    out
}

/// A link's short label: a GitHub issue or pull request is `#<n>`, anything
/// else its site's name.
fn short(url: &str) -> String {
    let path = url.split_once("://").map_or(url, |(_, p)| p);
    let parts: Vec<&str> = path.split('/').collect();
    if let [host, _, _, "issues" | "pull", n, ..] = parts[..]
        && host.ends_with("github.com")
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        return format!("#{n}");
    }
    parts[0].trim_start_matches("www.").to_string()
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

/// A line's text without its styles (sizes, tests).
pub fn plain(line: &Line) -> String {
    line.iter().map(|s| s.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of v0.1.4's real notes (shortened): summary, Highlights,
    /// Fixes, Security, Install, Upgrade notes, Known issues, Thanks.
    const V014: &str = "**OpenCircle v0.1.4: Ask AI sends what you circled, touch and pen work.**\n\n## Highlights\n- **Ask AI (Browser) sends the circled picture.** With Include screen on, the picture opens in your browser.\n- **Snip edges snap.** Dragging lands on edges; hold **Shift** to turn it off.\n- **A pick copies nothing** until you press **Ctrl+C**.\n- **Blur follows the pointer back.**\n- **Finger and pen** work everywhere.\n- **Every screen:** drag the picture\n  out onto any screen.\n- **Search in browser** opens your default browser.\n- **Lighter translation:** a third less memory.\n\n## Fixes\n- Touch screen and pen did nothing (https://github.com/AmerAlreyahi/OpenCircle/issues/153).\n- The Lens page opened in Edge (https://github.com/AmerAlreyahi/OpenCircle/pull/155).\n- Wayland: resizing a pin now takes effect.\n\n## Security\n- SQLite updated: `rusqlite` 0.41.\n\n## Install\n- **Windows:** `OpenCircle-Setup-0.1.4-x64.exe`.\n\n## Upgrade notes\n- The settings file moves to schema 11 (`config.toml.bak-v9`).\n- A \"Copy when picked\" moves to off once.\n\n## Known issues\n- Linux (Wayland): no window picking.\n\n## Thanks\nTo the users who reported #153.";

    fn span(text: &str, style: Style) -> Span {
        Span {
            text: text.into(),
            style,
            link: None,
        }
    }

    /// Summary without the version, Highlights cut at 6 (2 more counted),
    /// Fixes and Upgrade notes kept for folding, Install / Security / Known
    /// issues / Thanks dropped. Before: every line of every section, flat.
    #[test]
    fn the_card_gets_the_summary_six_highlights_and_the_folded_sections() {
        let n = parse(V014);
        assert_eq!(
            n.summary,
            vec![span(
                "Ask AI sends what you circled, touch and pen work.",
                Style::Bold
            )]
        );
        assert_eq!(n.highlights.len(), HIGHLIGHTS);
        assert_eq!(n.more, 2);
        assert_eq!(n.fixes.len(), 3);
        assert_eq!(n.upgrade.len(), 2);
        let all: String = (n.highlights.iter().chain(&n.fixes).chain(&n.upgrade))
            .map(plain)
            .collect();
        for gone in ["rusqlite", "Setup-0.1.4", "window picking", "reported"] {
            assert!(!all.contains(gone), "{gone}");
        }
        // the hard-wrapped bullet is one line
        assert_eq!(
            plain(&n.highlights[5]),
            "Every screen: drag the picture out onto any screen."
        );
    }

    /// Bold labels and keys stay bold, code is code, a bare issue link shows
    /// as `#153`. Before: `**` and backticks stripped, the whole URL shown.
    #[test]
    fn bold_code_and_short_links_are_kept() {
        let n = parse(V014);
        assert_eq!(
            n.highlights[1],
            vec![
                span("Snip edges snap.", Style::Bold),
                span(" Dragging lands on edges; hold ", Style::Plain),
                span("Shift", Style::Bold),
                span(" to turn it off.", Style::Plain),
            ]
        );
        let fix = &n.fixes[0];
        let link = fix.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(link.text, "#153");
        assert_eq!(
            link.link.as_deref(),
            Some("https://github.com/AmerAlreyahi/OpenCircle/issues/153")
        );
        assert_eq!(plain(fix), "Touch screen and pen did nothing (#153).");
        assert_eq!(plain(&n.fixes[1]), "The Lens page opened in Edge (#155).");
        assert_eq!(n.upgrade[0][1], span("config.toml.bak-v9", Style::Code));
        assert_eq!(short("https://www.example.com/a/b"), "example.com");
    }

    #[test]
    fn plain_or_empty_notes_still_show_something() {
        let n = parse("Small fixes.\n- One\n- Two");
        assert_eq!(plain(&n.summary), "Small fixes.");
        assert_eq!(n.highlights.len(), 2);
        assert_eq!(plain(&parse("").summary), EMPTY);
        // not a web link: its text only; an image is never a link
        let n = parse("- [x](javascript:alert(1)) ![img](https://e.x/i.png)");
        assert!(n.highlights[0].iter().all(|s| s.link.is_none()));
        assert_eq!(
            plain(&parse("مرحبا [رابط](https://a.b)").summary),
            "مرحبا رابط"
        );
        assert_eq!(parse(&"- x\n".repeat(500)).more, MAX_ITEMS - HIGHLIGHTS);
    }
}
