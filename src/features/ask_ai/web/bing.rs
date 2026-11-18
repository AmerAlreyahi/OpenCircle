//! Bing's web results page (HTML, no key, no cookies).

use super::page::{decode, text_of};
use super::{Engine, Hit};

pub(super) const ENGINE: Engine = Engine {
    name: "Bing",
    url: |q| {
        let q = crate::features::ask_ai::browser::encode(q);
        format!("https://www.bing.com/search?q={q}")
    },
    parse,
};

/// Each `<li class="b_algo">`: its `<h2><a href>` title and link, the first
/// `<p>` after it as the snippet. Bing's own redirect links
/// (`/ck/a?...&u=a1<base64>`) are turned back into the page's address.
fn parse(_query: &str, html: &str) -> Vec<Hit> {
    (html.split("class=\"b_algo\"").skip(1))
        .filter_map(|block| {
            let h2 = &block[block.find("<h2")?..];
            let a = &h2[h2.find("<a ")?..];
            let href = a.split("href=\"").nth(1)?.split('"').next()?;
            let url = real_url(&decode(href))?;
            let title = text_of(&a[a.find('>')? + 1..a.find("</a>")?]);
            let after = &h2[h2.find("</h2>")?..];
            let snippet = match after.find("<p") {
                Some(p) => {
                    let p = &after[p..];
                    text_of(&p[p.find('>')? + 1..p.find("</p>").unwrap_or(p.len())])
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

/// A result's own address: Bing's redirect decoded, any other http(s) link as is.
fn real_url(href: &str) -> Option<String> {
    use base64::Engine as _;
    let url = match href.contains("bing.com/ck/a") {
        true => {
            let u = (href.split(['?', '&'])).find_map(|kv| kv.strip_prefix("u=a1"))?;
            let bytes = (base64::engine::general_purpose::URL_SAFE_NO_PAD)
                .decode(u.trim_end_matches('='))
                .ok()?;
            String::from_utf8(bytes).ok()?
        }
        false => href.to_string(),
    };
    crate::shared::platform::sys::open::web_host(&url).map(|_| url)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of Bing's page (2026-11-18), cut down to three results.
    const PAGE: &str = r#"<ol id="b_results"><li class="b_algo" data-id iid=SERP.1><div class="b_tpcn"><a class="tilk" href="https://www.bing.com/ck/a?!&amp;&amp;p=x&amp;u=a1aHR0cHM6Ly91YnVudHUuY29tL2Rvd25sb2Fk&amp;ntb=1"><div>ubuntu.com</div></a></div><h2><a target="_blank" href="https://www.bing.com/ck/a?!&amp;&amp;p=34e5&amp;ptn=3&amp;u=a1aHR0cHM6Ly91YnVudHUuY29tL2Rvd25sb2Fk&amp;ntb=1" h="ID=SERP,5114.2"><span dir="ltr">Download - Get <strong>Ubuntu</strong></span></a></h2><div class="b_caption"><p class="b_lineclamp2"><span dir="ltr">Download Ubuntu Desktop &amp; Server.</span></p></div></li><li class="b_algo"><h2><a href="https://example.org/a?b=1">Plain &quot;link&quot;</a></h2></li><li class="b_algo"><h2><a href="javascript:alert(1)">Bad</a></h2></li></ol>"#;

    #[test]
    fn reads_titles_real_links_and_snippets() {
        let hits = parse("", PAGE);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].title, "Download - Get Ubuntu");
        assert_eq!(hits[0].url, "https://ubuntu.com/download");
        assert_eq!(hits[0].snippet, "Download Ubuntu Desktop & Server.");
        assert_eq!(hits[1].title, "Plain \"link\"");
        assert_eq!(hits[1].url, "https://example.org/a?b=1");
        assert!(parse("", "<html>no results</html>").is_empty());
    }
}
