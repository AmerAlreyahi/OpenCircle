//! The no-key "provider": the question opens in Google AI Mode in the user's own
//! browser (`https://www.google.com/search?udm=50&q=...`, udm=50 = AI Mode). No
//! API, no answer comes back to us; the only network action is the user's
//! browser loading that page, and only when they pressed Send.

use std::io;

/// Longest question we put in the URL, in characters.
const MAX_CHARS: usize = 1800;
/// Longest percent-encoded query. Non-Latin text costs 6+ characters a letter,
/// and servers and browsers refuse URLs past a few thousand bytes.
const MAX_ENCODED: usize = 6000;
const BASE: &str = "https://www.google.com/search?udm=50&q=";

/// The question for AI Mode: the user's prompt, then the text read from the
/// selection (`None` = no text; the image itself cannot be passed).
pub fn question(prompt: &str, text: Option<&str>) -> String {
    // Blank lines and indentation only cost characters.
    let text = text
        .map(|t| {
            let lines: Vec<_> = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            lines.join("\n")
        })
        .filter(|t| !t.is_empty());
    match text {
        Some(text) => format!("{}\n\nText from my screen:\n{text}", prompt.trim()),
        None => prompt.trim().to_string(),
    }
}

/// The AI Mode URL for `question`, and whether the question was cut to fit.
/// Cuts at a word boundary (a hard cut only inside one very long word).
pub fn url(question: &str) -> (String, bool) {
    link(BASE, question)
}

/// A plain Google search for `text` (Search in browser on a text selection),
/// cut like [`url`].
// ponytail: lives here with the URL builder; moves when this file goes (N-05).
pub fn search_url(text: &str) -> String {
    link("https://www.google.com/search?q=", text).0
}

fn link(base: &str, question: &str) -> (String, bool) {
    let mut max = MAX_CHARS;
    loop {
        let (q, cut) = cut_words(question.trim(), max);
        let enc = encode(q);
        if enc.len() <= MAX_ENCODED || max <= 1 {
            return (format!("{base}{enc}"), cut);
        }
        // Too many bytes once encoded (Arabic, CJK, ...): shorten and retry.
        let used = q.chars().count();
        max = (used - used / 8 - 1).max(1);
    }
}

/// `s` cut to at most `max` characters, at the last whitespace before the limit.
fn cut_words(s: &str, max: usize) -> (&str, bool) {
    let Some((end, _)) = s.char_indices().nth(max) else {
        return (s, false);
    };
    // Whitespace right at the limit means the cut is already between words.
    let at = if s[end..].starts_with(char::is_whitespace) {
        end
    } else {
        s[..end]
            .rfind(char::is_whitespace)
            .filter(|&i| i > 0)
            .unwrap_or(end)
    };
    (s[..at].trim_end(), true)
}

/// Percent-encoding of the UTF-8 bytes; only RFC 3986 unreserved characters stay.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Opens `question` in AI Mode with `open` (the browser launcher). Returns the
/// line for the panel: what happened, and that it was cut if it was.
pub fn ask_with(
    question: &str,
    open: impl FnOnce(&str) -> io::Result<()>,
) -> Result<String, String> {
    let (url, cut) = url(question);
    open(&url).map_err(|e| format!("Could not open the browser: {e}"))?;
    let mut note = String::from("Opened in your browser.");
    if cut {
        note.push_str(" Your question was long, so it was shortened.");
    }
    Ok(note)
}

/// [`ask_with`] through the user's default browser.
pub fn ask(question: &str) -> Result<String, String> {
    ask_with(question, crate::sys::open::open_google)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q_of(url: &str) -> &str {
        url.strip_prefix(BASE).expect("fixed host and AI Mode flag")
    }

    #[test]
    fn url_is_the_fixed_host_with_an_encoded_query() {
        let (u, cut) = url("Explain\n\nText from my screen:\nPastel de nata 1,40 €");
        assert!(!cut);
        assert_eq!(
            u,
            "https://www.google.com/search?udm=50&q=Explain%0A%0AText%20from%20my%20screen%3A%0A\
             Pastel%20de%20nata%201%2C40%20%E2%82%AC"
        );
        // Nothing the user typed can leave the q= parameter or the host.
        let (u, _) = url("a&udm=1#x?y=z/..\\ \"'<>%");
        let q = q_of(&u);
        assert!(
            q.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'%' || b"-._~".contains(&b))
        );
        assert!(u.starts_with("https://www.google.com/search?udm=50&q="));
    }

    #[test]
    fn a_text_search_is_one_plain_google_link() {
        let u = search_url("  Roter Sand & lighthouse #1?  ");
        assert_eq!(
            u,
            "https://www.google.com/search?q=Roter%20Sand%20%26%20lighthouse%20%231%3F"
        );
        // what lens::open_link accepts: one plain link on Google's host
        assert_eq!(
            crate::sys::open::web_host(&u).as_deref(),
            Some("www.google.com")
        );
        assert!(!search_url("x&udm=50").contains("&udm"));
    }

    #[test]
    fn long_questions_are_cut_at_a_word() {
        let long = "abcdefghi ".repeat(300); // 3000 chars
        let (u, cut) = url(&long);
        assert!(cut);
        let q = q_of(&u).replace("%20", " ");
        assert!(q.chars().count() <= MAX_CHARS);
        assert!(q.ends_with("abcdefghi"), "cut on a word edge: {q:?}");
        assert!(q.len() >= MAX_CHARS - 10);
        // a short one is untouched
        assert_eq!(url("hi there"), (format!("{BASE}hi%20there"), false));
        // exactly at the limit is not a cut
        assert!(!url(&"a".repeat(MAX_CHARS)).1);
        assert!(url(&"a".repeat(MAX_CHARS + 1)).1, "one long word: hard cut");
        assert_eq!(cut_words("one two three", 7), ("one two", true));
        assert_eq!(cut_words("one two three", 8), ("one two", true));
        assert_eq!(cut_words("one two three", 9), ("one two", true));
        assert_eq!(cut_words("one two three", 13), ("one two three", false));
    }

    #[test]
    fn non_latin_text_is_cut_by_encoded_size() {
        let arabic = "مرحبا بالعالم ".repeat(300);
        let (u, cut) = url(&arabic);
        assert!(cut);
        assert!(q_of(&u).len() <= MAX_ENCODED, "{}", q_of(&u).len());
        assert!(q_of(&u).len() > MAX_ENCODED / 2, "does not over-cut");
        // never splits a character: the query decodes as valid UTF-8
        let decoded: Vec<u8> = q_of(&u)
            .split('%')
            .skip(1)
            .map(|h| u8::from_str_radix(&h[..2], 16).unwrap())
            .collect();
        assert!(std::str::from_utf8(&decoded).is_ok());
    }

    #[test]
    fn question_joins_prompt_and_text() {
        assert_eq!(question(" Explain ", None), "Explain");
        assert_eq!(question("Explain", Some("  \n \n")), "Explain");
        assert_eq!(
            question("What is this?", Some("  Sunstone \n\n   Rare gem\n")),
            "What is this?\n\nText from my screen:\nSunstone\nRare gem"
        );
    }

    #[test]
    fn ask_with_reports_cut_and_launch_errors() {
        let mut seen = String::new();
        let note = ask_with("hello", |u| {
            seen = u.into();
            Ok(())
        });
        assert_eq!(note.unwrap(), "Opened in your browser.");
        assert_eq!(seen, format!("{BASE}hello"));
        let note = ask_with(&"word ".repeat(1000), |_| Ok(())).unwrap();
        assert!(note.contains("shortened"));
        let err = ask_with("x", |_| Err(io::Error::other("no xdg-open"))).unwrap_err();
        assert_eq!(err, "Could not open the browser: no xdg-open");
    }
}
