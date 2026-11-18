//! `open_page`: a page's readable text, from public hosts only, capped.

/// Most bytes read from a page.
const MAX_BYTES: u64 = 2_000_000;
/// Most characters of text the model gets.
const MAX_CHARS: usize = 8_000;

/// The readable text of `url` (http/https, public hosts only).
pub fn open(url: &str) -> Result<String, String> {
    if crate::shared::platform::sys::open::web_host(url).is_none() {
        return Err("only http(s) links can be opened".into());
    }
    let agent = crate::shared::net::public_agent(crate::shared::net::PAGE);
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    let kind = (resp.headers().get("content-type"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .to_ascii_lowercase();
    if !kind.starts_with("text/") && !kind.contains("xhtml") {
        return Err(format!("not a text page ({kind})"));
    }
    let body = (resp.body_mut().with_config().limit(MAX_BYTES))
        .lossy_utf8(true)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let text = if kind.starts_with("text/plain") {
        body
    } else {
        text_of(&body)
    };
    Ok(text.chars().take(MAX_CHARS).collect())
}

/// The visible text of an HTML page or fragment: no scripts, styles or
/// tags, entities decoded, one line per block, blank lines dropped.
// ponytail: a small tag stripper, not a readability port (no new dependency);
// menus and footers stay in. Add dom_smoothie if answers suffer from it.
pub fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 4);
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while let Some(off) = html[i..].find('<') {
        out.push_str(&html[i..i + off]);
        let start = i + off;
        let Some(end) = html[start..].find('>').map(|e| start + e + 1) else {
            i = html.len(); // an unclosed tag: dropped
            break;
        };
        let tag = lower[start + 1..end - 1].trim_start_matches('/');
        let name: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        i = end;
        if ["script", "style", "noscript", "svg", "template", "head"].contains(&name.as_str())
            && !lower[start + 1..].starts_with('/')
        {
            // skip to its closing tag
            i = lower[end..]
                .find(&format!("</{name}"))
                .map_or(html.len(), |c| end + c);
            continue;
        }
        if [
            "p",
            "div",
            "br",
            "li",
            "tr",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "section",
            "article",
            "header",
            "footer",
            "pre",
            "blockquote",
            "td",
        ]
        .contains(&name.as_str())
        {
            out.push('\n');
        }
    }
    out.push_str(&html[i..]);
    let text = decode(&out);
    let lines: Vec<String> = (text.lines())
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

/// HTML entities: the named ones that matter and every numeric one.
pub fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let ch = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        });
        match (ch, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
