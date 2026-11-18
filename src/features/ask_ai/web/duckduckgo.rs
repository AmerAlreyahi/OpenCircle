//! DuckDuckGo's lite page (plain HTML for low-bandwidth browsers; no key, no
//! cookies), asked by POST: W62 measured 97 % relevant top-5 (EN 100, AR 90)
//! with requests >= 2 s apart; GET and faster requests come back empty
//! (throttled). An empty or anomaly page rests it (`blocked`).

use std::time::Duration;

use super::page::{decode, text_of};
use super::{Engine, Hit};

pub(super) const ENGINE: Engine = Engine {
    name: "DuckDuckGo",
    url: |_| "https://lite.duckduckgo.com/lite/".to_string(),
    form: |q| {
        // the region from the query's language, like Wikipedia's edition
        let kl = match super::lang(q) {
            "ar" => "xa-ar",
            _ => "us-en",
        };
        Some(format!(
            "q={}&kl={kl}",
            crate::features::ask_ai::browser::encode(q)
        ))
    },
    parse,
    blocked,
    gap: Duration::from_secs(2),
};

/// No hits and not its "No results found" page: an empty (throttled) or
/// anomaly page. Never solved or dodged: the engine rests.
fn blocked(body: &str) -> bool {
    super::blocked(body) || !body.contains("No results found")
}

/// Each `class='result-link'` anchor: its text and link; the next
/// `class='result-snippet'` cell (before the next result) as the snippet.
fn parse(_query: &str, html: &str) -> Vec<Hit> {
    (html
        .split("class='result-link'")
        .skip(1)
        .zip(html.split("class='result-link'")))
    .filter_map(|(after, before)| {
        // the href sits before the class attribute, at the end of `before`
        let href = before.rsplit("href=\"").next()?.split('"').next()?;
        let url = real_url(&decode(href))?;
        let title = text_of(&after[after.find('>')? + 1..after.find("</a>")?]);
        let snippet = match after.find("class='result-snippet'") {
            Some(s) => {
                let cell = &after[s..];
                text_of(&cell[cell.find('>')? + 1..cell.find("</td>").unwrap_or(cell.len())])
            }
            None => String::new(),
        };
        (!title.is_empty()).then_some(Hit {
            title,
            url,
            snippet,
        })
    })
    .collect()
}

/// A result's own address: DuckDuckGo's redirect (`//duckduckgo.com/l/?uddg=<encoded>`)
/// decoded, any other http(s) link as is; ads (`y.js`) and other links dropped.
fn real_url(href: &str) -> Option<String> {
    let url = match href.contains("duckduckgo.com/l/") {
        true => unpercent(
            href.split(['?', '&'])
                .find_map(|kv| kv.strip_prefix("uddg="))?,
        )?,
        false => href.to_string(),
    };
    if url.contains("duckduckgo.com/") {
        return None;
    }
    crate::shared::platform::sys::open::web_host(&url).map(|_| url)
}

/// Percent-decoding (`+` stays: DuckDuckGo encodes spaces as %20).
fn unpercent(s: &str) -> Option<String> {
    let (b, mut out, mut i) = (s.as_bytes(), Vec::with_capacity(s.len()), 0);
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real lite pages (2026-11-18), cut to their first results.
    const EN: &str = include_str!("fixtures/ddg-lite-en.html");
    const AR: &str = include_str!("fixtures/ddg-lite-ar.html");

    #[test]
    fn reads_titles_links_and_snippets_in_english_and_arabic() {
        let hits = parse("", EN);
        assert_eq!(hits.len(), 3, "{hits:?}");
        assert_eq!(hits[1].title, "Linus Torvalds - Wikipedia");
        assert_eq!(hits[1].url, "https://en.wikipedia.org/wiki/Linus_Torvalds");
        assert!(
            hits[1]
                .snippet
                .starts_with("Linus Benedict Torvalds[a] (born 28 December 1969)"),
            "{}",
            hits[1].snippet
        );
        assert_eq!(hits[2].url, "https://github.com/torvalds");
        let ar = parse("", AR);
        assert_eq!(ar.len(), 2, "{ar:?}");
        assert_eq!(ar[0].title, "طوكيو - ويكيبيديا");
        assert!(
            ar[0]
                .url
                .starts_with("https://ar.wikipedia.org/wiki/%D8%B7")
        );
        assert!(ar[1].snippet.contains("عاصمتها"), "{}", ar[1].snippet);
    }

    #[test]
    fn redirects_are_unwrapped_and_ads_dropped() {
        let page = r#"<a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fa%3Fb%3D1&amp;rut=x" class='result-link'>A</a>
<a href="https://duckduckgo.com/y.js?ad=1" class='result-link'>Ad</a>
<a href="javascript:alert(1)" class='result-link'>Bad</a>"#;
        let hits = parse("", page);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].url, "https://example.org/a?b=1");
    }

    #[test]
    fn an_empty_or_anomaly_page_is_a_block_no_results_is_not() {
        assert!(blocked("<html><body></body></html>"));
        assert!(blocked(
            "<div class=\"anomaly-modal\">Please complete the captcha</div>"
        ));
        assert!(!blocked("<h1>No results found for <strong>x</strong></h1>"));
    }

    #[test]
    fn the_region_follows_the_query() {
        assert!(
            (ENGINE.form)("Linus Torvalds")
                .unwrap()
                .ends_with("&kl=us-en")
        );
        let ar = (ENGINE.form)("عاصمة اليابان").unwrap();
        assert!(
            ar.starts_with("q=%D8%B9") && ar.ends_with("&kl=xa-ar"),
            "{ar}"
        );
    }
}
