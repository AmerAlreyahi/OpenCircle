//! Things in the read text that deserve a chip: web links, email addresses and
//! phone numbers. Small hand-written scanners over ASCII bytes (no regex crate);
//! they favour missing a number over inventing one. Nothing here opens anything:
//! the chip does that, on a click.

/// At most this many chips (a page of text can hold hundreds of numbers).
const MAX: usize = 6;
/// Bare hosts ("example.com/x", no scheme, no "www.") count only with these endings.
// ponytail: a short list keeps "file.txt" and "e.g." out; add endings when users miss them.
const TLDS: [&str; 11] = [
    "com", "org", "net", "io", "dev", "app", "edu", "gov", "ai", "me", "info",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Link,
    Email,
    Phone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub kind: Kind,
    /// Link: the absolute http(s) URL. Email: the address. Phone: as written.
    pub text: String,
}

impl Entity {
    /// What the chip does.
    pub fn label(&self) -> &'static str {
        match self.kind {
            Kind::Link => "Open link",
            Kind::Email => "Write email",
            Kind::Phone => "Copy number",
        }
    }

    /// The value, short enough for a chip (a link without its scheme).
    pub fn shown(&self) -> String {
        let text = self.text.as_str();
        let text = match self.kind {
            Kind::Link => text.split_once("://").map_or(text, |(_, rest)| rest),
            _ => text,
        };
        shorten(text, 28)
    }
}

/// `s` cut to `max` characters, with an ellipsis when it was cut.
pub fn shorten(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", s[..end].trim_end()),
        None => s.to_string(),
    }
}

/// Links, then emails, then phone numbers in `text`, each once, at most [`MAX`].
pub fn find(text: &str) -> Vec<Entity> {
    // Each pass blanks what it took, so a number inside a link is not a phone.
    let mut bytes = text.as_bytes().to_vec();
    let mut out = Vec::new();
    for (kind, scan) in [
        (Kind::Link, links as fn(&mut [u8]) -> Vec<String>),
        (Kind::Email, emails),
        (Kind::Phone, phones),
    ] {
        for text in scan(&mut bytes) {
            let entity = Entity { kind, text };
            if !out.contains(&entity) {
                out.push(entity);
            }
        }
    }
    out.truncate(MAX);
    out
}

/// Whether `s` is an email address: what a `mailto:` may carry (no `?` headers).
pub fn is_email(s: &str) -> bool {
    let mut copy = s.as_bytes().to_vec();
    emails(&mut copy) == [s] && !s.contains(char::is_whitespace)
}

fn blank(bytes: &mut [u8], from: usize, to: usize) {
    bytes[from..to].fill(b' ');
}

/// The ASCII text of `bytes[from..to]` (callers only pass ASCII ranges).
fn ascii(bytes: &[u8], from: usize, to: usize) -> String {
    String::from_utf8_lossy(&bytes[from..to]).into_owned()
}

// ---- links -------------------------------------------------------------------

fn links(bytes: &mut [u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_graphic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_graphic() {
            i += 1;
        }
        if let Some((from, to, url)) = link_in(&bytes[start..i]) {
            found.push(url);
            blank(bytes, start + from, start + to);
        }
    }
    found
}

/// The link in one whitespace-free `token`: its range and absolute URL.
fn link_in(token: &[u8]) -> Option<(usize, usize, String)> {
    let token_str = std::str::from_utf8(token).ok()?;
    let lower = token_str.to_ascii_lowercase();
    let scheme = ["https://", "http://"]
        .iter()
        .filter_map(|s| lower.find(s).map(|at| (at, s.len())))
        .min();
    let (from, host_at, absolute) = match scheme {
        Some((at, len)) => (at, at + len, true),
        None => {
            let from = lower.find(|c: char| !"([{<\"'".contains(c))?;
            // Bare "example.com" next to an '@' is an email, not a link.
            if lower.contains('@') {
                return None;
            }
            (from, from, false)
        }
    };
    let to = host_at + trim_end(&token_str[host_at..]).len();
    let rest = &token_str[host_at..to];
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = host_name(&rest[..host_end])?;
    let tld_ok = host.rsplit('.').next().is_some_and(|t| TLDS.contains(&t));
    let path = &rest[host_end..];
    if !(absolute
        || host.starts_with("www.")
        || tld_ok && (path.is_empty() || path.starts_with('/')))
    {
        return None;
    }
    let scheme = if absolute && lower[from..].starts_with("http://") {
        "http://"
    } else {
        "https://"
    };
    let url = format!("{scheme}{rest}");
    Some((from, to, url))
}

/// `s` without trailing sentence punctuation and unbalanced closing brackets.
fn trim_end(s: &str) -> &str {
    let mut s = s;
    loop {
        let Some(last) = s.chars().next_back() else {
            return s;
        };
        let open = match last {
            ')' => Some('('),
            ']' => Some('['),
            '}' => Some('{'),
            _ => None,
        };
        let drop = match open {
            Some(open) => s.matches(open).count() < s.matches(last).count(),
            None => ".,;:!?'\"*>".contains(last),
        };
        if !drop {
            return s;
        }
        s = &s[..s.len() - 1];
    }
}

/// `host[:port]` as a lower-case host name, if it is one: dotted labels of
/// letters, digits and '-', a letters-only ending of 2+ (or four numbers).
fn host_name(authority: &str) -> Option<String> {
    let host = match authority.split_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => host,
        Some(_) => return None,
        None => authority,
    };
    let host = host.to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    let numeric = labels.iter().all(|l| l.bytes().all(|b| b.is_ascii_digit()));
    let last = labels.last()?;
    let ending_ok = (numeric && labels.len() == 4)
        || (last.len() >= 2 && last.bytes().all(|b| b.is_ascii_alphabetic()));
    (labels.len() >= 2 && labels.iter().all(label_ok) && ending_ok).then_some(host)
}

// ---- emails ------------------------------------------------------------------

fn emails(bytes: &mut [u8]) -> Vec<String> {
    let mut found = Vec::new();
    for at in 0..bytes.len() {
        if bytes[at] != b'@' {
            continue;
        }
        let local_char = |b: &u8| b.is_ascii_alphanumeric() || b"._%+-".contains(b);
        let domain_char = |b: &u8| b.is_ascii_alphanumeric() || b"-.".contains(b);
        let from = at
            - bytes[..at]
                .iter()
                .rev()
                .take_while(|b| local_char(b))
                .count();
        let mut to = at
            + 1
            + bytes[at + 1..]
                .iter()
                .take_while(|b| domain_char(b))
                .count();
        while to > at + 1 && matches!(bytes[to - 1], b'.' | b'-') {
            to -= 1;
        }
        let (local, domain) = (ascii(bytes, from, at), ascii(bytes, at + 1, to));
        let local_ok = !local.is_empty()
            && !local.starts_with('.')
            && !local.ends_with('.')
            && !local.contains("..");
        if local_ok && host_name(&domain).is_some() {
            found.push(ascii(bytes, from, to));
            blank(bytes, from, to);
        }
    }
    found
}

// ---- phone numbers -------------------------------------------------------------

fn phones(bytes: &mut [u8]) -> Vec<String> {
    let n = bytes.len();
    let mut found = Vec::new();
    let mut i = 0;
    while i < n {
        let starts = bytes[i] == b'+' || bytes[i] == b'(' || bytes[i].is_ascii_digit();
        // Not in the middle of a word, a price, a date or an id.
        let after_word = i > 0 && {
            let p = bytes[i - 1];
            p.is_ascii_alphanumeric() || p >= 0x80 || b"/.,:$#-_@".contains(&p)
        };
        match starts && !after_word {
            true => match phone_at(bytes, i) {
                Some(end) => {
                    found.push(ascii(bytes, i, end));
                    blank(bytes, i, end);
                    i = end;
                }
                None => i += 1,
            },
            false => i += 1,
        }
    }
    found
}

/// The end of a phone number starting at `bytes[i]`, if one does.
fn phone_at(bytes: &[u8], i: usize) -> Option<usize> {
    let n = bytes.len();
    let plus = bytes[i] == b'+';
    let mut j = i + usize::from(plus);
    let (mut digits, mut parens, mut open) = (0, false, false);
    let (mut dashes, mut spaces, mut dots) = (0, 0, 0);
    let first = bytes.get(j + usize::from(bytes.get(j) == Some(&b'(')));
    let mut end = i;
    loop {
        if !open && j + 1 < n && bytes[j] == b'(' && bytes[j + 1].is_ascii_digit() {
            open = true;
            parens = true;
            j += 1;
        }
        if !(j < n && bytes[j].is_ascii_digit()) {
            break;
        }
        let run = j;
        while j < n && bytes[j].is_ascii_digit() {
            j += 1;
        }
        digits += j - run;
        end = j;
        if open {
            if j < n && bytes[j] == b')' {
                j += 1;
                end = j;
                open = false;
            } else {
                return None;
            }
        }
        if j < n && bytes[j].is_ascii_digit() {
            continue; // "(555)123": digits right after the bracket
        }
        match bytes.get(j) {
            Some(&sep @ (b' ' | b'-' | b'.'))
                if bytes
                    .get(j + 1)
                    .is_some_and(|b| b.is_ascii_digit() || *b == b'(') =>
            {
                match sep {
                    b' ' => spaces += 1,
                    b'-' => dashes += 1,
                    _ => dots += 1,
                }
                j += 1;
            }
            _ => break,
        }
    }
    if digits == 0 || bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    let ok = if plus {
        (8..=15).contains(&digits)
    } else if parens {
        (10..=11).contains(&digits)
    } else if dots > 0 {
        false // "1.234.567.890" is a number; only "+" numbers may use dots
    } else if spaces == 0 {
        dashes > 0 && (10..=11).contains(&digits)
    } else {
        // "06 12 34 56 78": spaces only, national format with its leading 0.
        dashes == 0 && first == Some(&b'0') && (10..=11).contains(&digits)
    };
    ok.then_some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(kind: Kind, text: &str) -> Entity {
        Entity {
            kind,
            text: text.into(),
        }
    }

    fn links_in(text: &str) -> Vec<String> {
        find(text)
            .into_iter()
            .filter(|e| e.kind == Kind::Link)
            .map(|e| e.text)
            .collect()
    }

    fn phones_in(text: &str) -> Vec<String> {
        find(text)
            .into_iter()
            .filter(|e| e.kind == Kind::Phone)
            .map(|e| e.text)
            .collect()
    }

    #[test]
    fn links_lose_trailing_punctuation_but_keep_balanced_brackets() {
        for (text, url) in [
            (
                "see https://example.com/a?b=1.",
                "https://example.com/a?b=1",
            ),
            ("(https://example.com/x)", "https://example.com/x"),
            ("\"http://example.com\",", "http://example.com"),
            ("go to https://example.com!!", "https://example.com"),
            (
                "https://en.wikipedia.org/wiki/Rust_(language))",
                "https://en.wikipedia.org/wiki/Rust_(language)",
            ),
            ("HTTPS://Example.com/Path", "https://Example.com/Path"),
            ("www.example.org/docs;", "https://www.example.org/docs"),
            ("visit example.com today", "https://example.com"),
            ("docs.example.dev/start.", "https://docs.example.dev/start"),
            (
                "http://192.168.0.1:8080/admin",
                "http://192.168.0.1:8080/admin",
            ),
            ("see:https://a.io/x", "https://a.io/x"),
        ] {
            assert_eq!(links_in(text), [url], "{text:?}");
        }
    }

    #[test]
    fn not_links() {
        for text in [
            "e.g. this, i.e. that",
            "version 1.2.3 released",
            "notes.txt and main.rs",
            "price 3.50 or 1.299,00",
            "https:// nothing",
            "https://nodot/",
            "http://-bad-.com",
            "ftp://example.com",
            "javascript:alert(1)",
            "user@example.com",
            "Node.js and Next.js",
            "done.Then more",
        ] {
            assert!(
                links_in(text).is_empty(),
                "{text:?} -> {:?}",
                links_in(text)
            );
        }
    }

    #[test]
    fn links_stop_at_non_ascii_so_the_launcher_gets_plain_ascii() {
        assert_eq!(
            links_in("https://example.com/\u{645}\u{631}"),
            ["https://example.com/"]
        );
    }

    #[test]
    fn emails() {
        assert_eq!(
            find("Mail John.Smith+news@mail.example.co.uk, or (a_b@x.io)."),
            [
                of(Kind::Email, "John.Smith+news@mail.example.co.uk"),
                of(Kind::Email, "a_b@x.io")
            ]
        );
        for bad in [
            "@handle",
            "a@b",
            "a@b.c",
            "x@.com",
            "a..b@c.com",
            "me@@x.com",
            "2@3",
        ] {
            assert!(
                find(bad).iter().all(|e| e.kind != Kind::Email),
                "{bad:?} -> {:?}",
                find(bad)
            );
        }
        assert!(is_email("a@b.com") && !is_email("a@b.com?cc=x@y.com") && !is_email("a b@c.com"));
        // an address is not also a link
        assert_eq!(find("hi@example.com").len(), 1);
    }

    #[test]
    fn phones_in_international_and_local_formats() {
        for (text, number) in [
            ("Call +1 (555) 123-4567 now", "+1 (555) 123-4567"),
            ("tel: +44 20 7946 0958.", "+44 20 7946 0958"),
            ("+966 55 123 4567", "+966 55 123 4567"),
            ("+4915123456789", "+4915123456789"),
            ("(555) 123-4567", "(555) 123-4567"),
            ("(555)123-4567", "(555)123-4567"),
            ("555-123-4567,", "555-123-4567"),
            ("1-800-555-0199", "1-800-555-0199"),
            ("06 12 34 56 78", "06 12 34 56 78"),
            ("020 7946 0958", "020 7946 0958"),
            ("+33 6.12.34.56.78", "+33 6.12.34.56.78"),
        ] {
            assert_eq!(phones_in(text), [number], "{text:?}");
        }
    }

    #[test]
    fn prices_dates_and_ids_are_not_phones() {
        for text in [
            "$1,299.99 or 1.299,00 EUR",
            "total 12 345 678 due",
            "2026-10-24",
            "24/10/2026 and 10.24.2026",
            "10:30-11:45",
            "1.234.567.890",
            "order 2026102412",
            "ISBN 978-3-16-148410-0",
            "4111 1111 1111 1111",
            "4111-1111-1111-1111",
            "192.168.100.100",
            "3.14159265358979",
            "12 15 20 25 30 35",
            "+5.99 +12.50",
            "v2026-10-24-1234",
            "5551234567x",
            "call me",
            "+ 1 2 3",
        ] {
            assert!(
                phones_in(text).is_empty(),
                "{text:?} -> {:?}",
                phones_in(text)
            );
        }
    }

    #[test]
    fn a_number_inside_a_link_is_not_a_phone() {
        assert_eq!(
            find("https://example.com/call/555-123-4567 or 555-123-4567"),
            [
                of(Kind::Link, "https://example.com/call/555-123-4567"),
                of(Kind::Phone, "555-123-4567")
            ]
        );
    }

    #[test]
    fn chips_are_deduplicated_ordered_and_capped() {
        let text = "a@b.com a@b.com https://x.com +1 555 123 4567 https://x.com";
        let kinds: Vec<_> = find(text).iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [Kind::Link, Kind::Email, Kind::Phone]);
        let many: String = (0..20).map(|i| format!("u{i}@site.com ")).collect();
        assert_eq!(find(&many).len(), MAX);
        assert!(find("").is_empty() && find("   \n").is_empty());
    }

    #[test]
    fn chip_text_is_short() {
        let e = of(
            Kind::Link,
            "https://example.com/a/very/long/path/that/goes/on",
        );
        assert_eq!(e.label(), "Open link");
        assert_eq!(e.shown(), "example.com/a/very/long/path…");
        assert_eq!(of(Kind::Email, "a@b.com").shown(), "a@b.com");
        assert_eq!(shorten("héllo wörld", 5), "héllo…");
    }
}
