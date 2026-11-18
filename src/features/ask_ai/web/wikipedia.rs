//! Wikipedia's official search API (keyless, never a captcha): the fallback
//! for facts. A query in Arabic asks the Arabic Wikipedia (`super::lang`).

use super::page::text_of;
use super::{Engine, Hit, lang};
use serde_json::Value;

pub(super) const ENGINE: Engine = Engine {
    name: "Wikipedia",
    url: |q| {
        let (lang, q) = (lang(q), crate::features::ask_ai::browser::encode(q));
        // 10: `relevant` keeps the top 5 that are about the query
        format!(
            "https://{lang}.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit=10&srsearch={q}"
        )
    },
    form: |_| None,
    parse,
    blocked: super::blocked,
    gap: std::time::Duration::ZERO,
};

fn parse(query: &str, body: &str) -> Vec<Hit> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let lang = lang(query);
    (v["query"]["search"].as_array().into_iter().flatten())
        .filter_map(|r| {
            let title = r["title"].as_str()?.to_string();
            let path = crate::features::ask_ai::browser::encode(&title.replace(' ', "_"));
            Some(Hit {
                url: format!("https://{lang}.wikipedia.org/wiki/{path}"),
                snippet: text_of(r["snippet"].as_str().unwrap_or_default()),
                title,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_api_answer() {
        let body = r#"{"batchcomplete":"","query":{"searchinfo":{"totalhits":2},"search":[{"ns":0,"title":"Ubuntu version history","pageid":1,"snippet":"<span class=\"searchmatch\">Ubuntu</span> 22.04 &quot;Jammy&quot;"}]}}"#;
        let hits = parse("ubuntu", body);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].url,
            "https://en.wikipedia.org/wiki/Ubuntu_version_history"
        );
        assert_eq!(hits[0].snippet, "Ubuntu 22.04 \"Jammy\"");
        assert!((ENGINE.url)("عاصمة اليابان").starts_with("https://ar.wikipedia.org/"));
        assert!(parse("q", "<html>").is_empty());
    }
}
