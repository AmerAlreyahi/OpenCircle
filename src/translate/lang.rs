//! The languages OpenCircle knows: the ONE table every part reads (the
//! pickers, the AI prompt, right-to-left layout, which OCR recognizer reads a
//! source; `mt::PAIRS` names its pairs by these codes). A code that isn't in
//! the table (a hand-edited config) still works with AI: it is sent as is,
//! read with the default recognizer and laid out left to right.
use std::fmt;

/// `translate.source` when the source language is detected.
pub const AUTO: &str = "auto";

/// A writing system: what OCR must read and how "already in the target
/// language" is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Arabic,
    Hebrew,
    Cyrillic,
    /// Han, kana: Chinese and Japanese.
    Cjk,
    Hangul,
    Thai,
    Devanagari,
}

impl Script {
    /// `c` is a letter of this script (Latin: ASCII and Latin-1/Extended letters).
    pub fn has(self, c: char) -> bool {
        match self {
            Script::Latin => {
                c.is_ascii_alphabetic()
                    || matches!(c, '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}')
            }
            Script::Arabic => {
                matches!(c, '\u{0600}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
            }
            Script::Hebrew => matches!(c, '\u{0590}'..='\u{05FF}'),
            Script::Cyrillic => matches!(c, '\u{0400}'..='\u{04FF}'),
            Script::Cjk => matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}'),
            Script::Hangul => matches!(c, '\u{1100}'..='\u{11FF}' | '\u{AC00}'..='\u{D7AF}'),
            Script::Thai => matches!(c, '\u{0E00}'..='\u{0E7F}'),
            Script::Devanagari => matches!(c, '\u{0900}'..='\u{097F}'),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// As in `translate.source` / `translate.target` ("ar", "pt-BR").
    pub code: &'static str,
    /// English name: the pickers and the AI prompt.
    pub name: &'static str,
    pub native: &'static str,
    pub script: Script,
    pub rtl: bool,
    /// The OCR recognizer that reads it (`ocr::models::RECOGNIZERS` id);
    /// `None` = it can't be read yet (not offered as a source).
    pub ocr: Option<&'static str>,
}

/// Pickers show the English name.
impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const fn row(
    code: &'static str,
    name: &'static str,
    native: &'static str,
    script: Script,
    ocr: Option<&'static str>,
) -> Language {
    let rtl = matches!(script, Script::Arabic | Script::Hebrew);
    Language {
        code,
        name,
        native,
        script,
        rtl,
        ocr,
    }
}

use Script::*;
const LATIN: Option<&str> = Some("latin");

/// Picker order (Arabic, the default target, first).
pub const LANGUAGES: &[Language] = &[
    row("ar", "Arabic", "العربية", Arabic, Some("arabic")),
    row("en", "English", "English", Latin, Some("en")),
    row("fr", "French", "Français", Latin, LATIN),
    row("de", "German", "Deutsch", Latin, LATIN),
    row("es", "Spanish", "Español", Latin, LATIN),
    row("pt", "Portuguese", "Português", Latin, LATIN),
    row(
        "pt-BR",
        "Portuguese (Brazil)",
        "Português (Brasil)",
        Latin,
        LATIN,
    ),
    row("it", "Italian", "Italiano", Latin, LATIN),
    row("nl", "Dutch", "Nederlands", Latin, LATIN),
    row("ru", "Russian", "Русский", Cyrillic, Some("cyrillic")),
    row("uk", "Ukrainian", "Українська", Cyrillic, Some("cyrillic")),
    row("pl", "Polish", "Polski", Latin, LATIN),
    row("tr", "Turkish", "Türkçe", Latin, LATIN),
    row("fa", "Persian", "فارسی", Arabic, Some("arabic")),
    row("ur", "Urdu", "اردو", Arabic, Some("arabic")),
    row("he", "Hebrew", "עברית", Hebrew, None),
    row("hi", "Hindi", "हिन्दी", Devanagari, None),
    row("id", "Indonesian", "Bahasa Indonesia", Latin, LATIN),
    row("vi", "Vietnamese", "Tiếng Việt", Latin, LATIN),
    row("th", "Thai", "ไทย", Thai, None),
    row("ja", "Japanese", "日本語", Cjk, Some("cjk")),
    row("ko", "Korean", "한국어", Hangul, None),
    row("zh", "Chinese (Simplified)", "简体中文", Cjk, Some("cjk")),
    row(
        "zh-Hant",
        "Chinese (Traditional)",
        "繁體中文",
        Cjk,
        Some("cjk"),
    ),
];

/// The table's row for `code`: the exact code (any case), else its base
/// language ("ar-SA" -> Arabic). `None` for "auto" and unknown codes.
pub fn find(code: &str) -> Option<&'static Language> {
    let base = code.split(['-', '_']).next().unwrap_or_default();
    (LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code)))
        .or_else(|| LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(base)))
}

/// Right-to-left: text is right-aligned when drawn.
pub fn is_rtl(code: &str) -> bool {
    find(code).is_some_and(|l| l.rtl)
}

/// The English name for prompts and labels ("auto" = "Detect", unknown = the code).
pub fn name(code: &str) -> &str {
    match find(code) {
        Some(l) => l.name,
        None if code == AUTO => "Detect",
        None => code,
    }
}

/// Common words that tell Latin-script languages apart (`latin`).
const WORDS: [(&str, &[&str]); 4] = [
    (
        "en",
        &[
            "the", "and", "of", "to", "is", "in", "that", "with", "for", "are", "was", "this",
        ],
    ),
    (
        "es",
        &[
            "el", "la", "los", "las", "de", "que", "y", "en", "es", "una", "por", "con", "del",
            "se",
        ],
    ),
    (
        "fr",
        &[
            "le", "la", "les", "des", "et", "est", "une", "du", "que", "dans", "pour", "pas",
            "sur", "au",
        ],
    ),
    (
        "de",
        &[
            "der", "die", "das", "und", "ist", "nicht", "ein", "eine", "mit", "den", "von", "zu",
            "auf", "sich",
        ],
    ),
];

/// Which Latin-script language `texts` are in, by their common words
/// (`WORDS`); English when none stands out.
// ponytail: four languages by stop words; more Latin pairs = more rows.
fn latin(texts: &[&str]) -> &'static str {
    let mut votes = [0usize; WORDS.len()];
    for word in texts
        .iter()
        .flat_map(|t| t.split(|c: char| !c.is_alphabetic()))
    {
        let word = word.to_lowercase();
        for (i, (_, list)) in WORDS.iter().enumerate() {
            votes[i] += list.contains(&word.as_str()) as usize;
        }
    }
    let (best, n) = (votes.iter().enumerate())
        .max_by_key(|(i, n)| (**n, usize::MAX - i))
        .unwrap_or((0, &0));
    match *n {
        0 => "en",
        _ => WORDS[best].0,
    }
}

/// What a detected source most likely is, from the script most of the
/// letters are in: Arabic, Russian, Japanese (any kana) or Chinese; Latin
/// letters by their common words (English, Spanish, French or German).
/// `None` = no letters.
pub fn guess<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    let texts: Vec<&str> = texts.into_iter().collect();
    let mut counts = [0usize; 4]; // latin, arabic, cyrillic, cjk
    let mut kana = false;
    for c in texts.iter().flat_map(|t| t.chars()) {
        kana |= matches!(c, '\u{3040}'..='\u{30FF}');
        let i = [Latin, Arabic, Cyrillic, Cjk].iter().position(|s| s.has(c));
        if let Some(i) = i {
            counts[i] += 1;
        }
    }
    let (best, n) = (counts.iter().enumerate()).max_by_key(|(_, n)| **n)?;
    if *n == 0 {
        return None;
    }
    Some(match best {
        1 => "ar",
        2 => "ru",
        3 if kana => "ja",
        3 => "zh",
        _ => latin(&texts),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_consistent() {
        for (i, l) in LANGUAGES.iter().enumerate() {
            assert!(
                LANGUAGES[..i]
                    .iter()
                    .all(|o| !o.code.eq_ignore_ascii_case(l.code)),
                "{} twice",
                l.code
            );
            assert_eq!(find(l.code), Some(l), "{} finds itself", l.code);
            assert!(!l.name.is_empty() && !l.native.is_empty());
            // its own native name is written in its script
            let letters: Vec<char> = l.native.chars().filter(|c| c.is_alphabetic()).collect();
            assert!(letters.iter().all(|c| l.script.has(*c)), "{}", l.code);
        }
        assert_eq!(
            LANGUAGES[0].code,
            crate::config::Config::default().translate.target
        );
    }

    #[test]
    fn lookup_direction_and_names() {
        assert_eq!(find("AR-sa").map(|l| l.code), Some("ar"));
        assert_eq!(find("zh-hant").map(|l| l.code), Some("zh-Hant"));
        assert_eq!(find("zh-CN").map(|l| l.code), Some("zh"));
        assert!(find(AUTO).is_none() && find("xx").is_none() && find("").is_none());
        assert!(is_rtl("ar") && is_rtl("he-IL") && is_rtl("fa") && !is_rtl("en"));
        assert!(!is_rtl("fr") && !is_rtl("xx") && !is_rtl(AUTO));
        assert_eq!(
            (name("ja"), name(AUTO), name("xx")),
            ("Japanese", "Detect", "xx")
        );
    }

    #[test]
    fn guess_by_script() {
        assert_eq!(guess(["Main menu", "Quit"]), Some("en"));
        assert_eq!(guess(["القائمة الرئيسية", "OK"]), Some("ar"));
        assert_eq!(guess(["Главное меню"]), Some("ru"));
        assert_eq!(guess(["主菜单"]), Some("zh"));
        assert_eq!(guess(["メインメニュー", "設定"]), Some("ja"));
        assert_eq!(guess(["12:30", ""]), None);
        // Latin letters: by their common words
        let es = "Un faro es una torre de señalización luminosa situada en el litoral";
        assert_eq!(guess([es]), Some("es"));
        assert_eq!(guess(["Le phare est une tour dans la mer"]), Some("fr"));
        assert_eq!(
            guess(["Der Leuchtturm ist ein Turm und steht an der Küste"]),
            Some("de")
        );
        assert_eq!(
            guess(["The lighthouse is a tower on the coast"]),
            Some("en")
        );
    }
}
