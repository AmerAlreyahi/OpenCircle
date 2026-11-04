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
/// `c` without its accent, lower case ("É" -> 'e', "ñ" -> 'n'; others as
/// they are): the letters of a Latin word as an English Windows OCR, which
/// drops accents ("Limite", "Grafico"), compares them.
pub fn plain(c: char) -> char {
    const FROM: &str = "àáâãäåāăąçćčďèéêëēėęěìíîïīįñńňòóôõöøōőŕřśšşťùúûüūůűųýÿźżž";
    const TO: &str = "aaaaaaaaacccdeeeeeeeeiiiiiinnnoooooooorrssstuuuuuuuuyyzzz";
    let c = c.to_lowercase().next().unwrap_or(c);
    match FROM.chars().position(|f| f == c) {
        Some(i) => TO.chars().nth(i).unwrap_or(c),
        None => c,
    }
}

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

/// What tells Latin-script languages apart (`latin`): common words, word
/// endings (of words of 4+ letters) and letters only that language uses.
/// A subtitle has few common words ("Ha derramado mucha sangre inocente."
/// has none of the first list): its endings and accents decide.
const WORDS: [(&str, &[&str], &[&str], &str); 4] = [
    (
        "en",
        &[
            "the", "and", "of", "to", "is", "in", "that", "with", "for", "are", "was", "this",
            "it", "you", "be", "on", "not", "have", "or", "by", "from", "into", "also",
        ],
        &["ing", "ght", "th", "ck", "sh", "ly", "ness", "ful"],
        "",
    ),
    (
        "es",
        &[
            "el", "la", "los", "las", "de", "que", "y", "en", "es", "una", "por", "con", "del",
            "se", "un", "esta", "este", "está", "su", "para", "como", "pero", "al", "lo", "muy",
            "ha", "han", "hay", "mi", "tu", "te",
        ],
        &[
            "ado", "ada", "ido", "ida", "ción", "mente", "amos", "aba", "o", "a", "os", "as",
        ],
        "ñáíóú¿¡",
    ),
    (
        "fr",
        &[
            "le", "la", "les", "des", "et", "est", "une", "du", "que", "dans", "pour", "pas",
            "sur", "au", "un", "il", "elle", "je", "vous", "ce", "qui", "avec", "de", "en", "plus",
            "dont", "aux",
        ],
        &["eux", "eau", "aux", "ment", "ait", "ais", "ée", "ées"],
        "èêçàùâîôûëïœ",
    ),
    (
        "de",
        &[
            "der", "die", "das", "und", "ist", "nicht", "ein", "eine", "mit", "den", "von", "zu",
            "auf", "sich", "ich", "du", "wir", "sie", "auch",
        ],
        &["sch", "ung", "keit", "heit", "lich", "chen"],
        "äöüß",
    ),
];

/// Votes per `WORDS` language for `texts`: a common word, an ending or an
/// accent each count once per word.
// ponytail: four languages by words, endings and accents; more Latin pairs =
// more rows (a trigram model if this ever misreads too often).
fn votes(texts: &[&str]) -> [usize; WORDS.len()] {
    let mut votes = [0usize; WORDS.len()];
    for word in texts
        .iter()
        .flat_map(|t| t.split(|c: char| !c.is_alphabetic()))
        .filter(|w| !w.is_empty())
    {
        let word = word.to_lowercase();
        let long = word.chars().count() >= 4;
        for (i, (_, list, ends, marks)) in WORDS.iter().enumerate() {
            votes[i] += list.contains(&word.as_str()) as usize
                + (long && ends.iter().any(|e| word.ends_with(e))) as usize
                + word.chars().any(|c| marks.contains(c)) as usize;
        }
    }
    votes
}

/// Words in `texts`, and how many are common words of the `WORDS`
/// languages ("the", "de", "und"): a page of real text has 1 in 5 or more,
/// Latin noise read from another script ("aill agarall") about none.
pub fn common<'a>(texts: impl IntoIterator<Item = &'a str>) -> (usize, usize) {
    let mut n = (0, 0);
    for t in texts {
        for w in t
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
        {
            let w = w.to_lowercase();
            n.0 += 1;
            n.1 += WORDS
                .iter()
                .any(|(_, list, _, _)| list.contains(&w.as_str())) as usize;
        }
    }
    n
}

/// Which Latin-script language `texts` are in (`votes`); English when none
/// stands out.
fn latin(texts: &[&str]) -> &'static str {
    // Italian has no `WORDS` row: its endings voted Spanish and an Italian
    // page was offered "Spanish -> Arabic" (Win10 VM and Ubuntu, it.wikipedia)
    if italian(&texts.join("\n")) {
        return "it";
    }
    let votes = votes(texts);
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
/// letters by their common words (English, Spanish, French or German;
/// Italian by `italian`).
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

/// The language of ONE text when it says so itself: its script (Arabic,
/// Russian, Japanese, Chinese), or Latin words that clearly lean one way
/// (two votes ahead of any other). `None` = too little to tell ("Gracias.",
/// "Blender Studio", any Latin line under three words: "Pluto Haumea" has two
/// Spanish endings): the page's language applies. A line of three or more
/// plain words the votes can't settle goes to `detected`.
pub fn sure(text: &str) -> Option<&'static str> {
    match guess([text])? {
        "en" | "es" | "fr" | "de" | "it" => {
            let words = text.split_whitespace();
            if words.filter(|w| w.chars().any(char::is_alphabetic)).count() < 3 {
                return None;
            }
            if italian(text) {
                return Some("it");
            }
            let v = votes(&[text]);
            let mut sorted = v;
            sorted.sort_unstable();
            let best = v.iter().position(|n| *n == sorted[3])?;
            if sorted[3] >= sorted[2] + 2 {
                return Some(WORDS[best].0);
            }
            detected(text, &v)
        }
        other => Some(other),
    }
}

/// An Italian line: the detector says so and two of its words are only
/// Italian. On a French page "Il sistema solare è formato dal Sole e da
/// tutti i corpi celesti..." had five Spanish endings: it went through the
/// page's fr model and came back altered ("sistema sole").
// ponytail: Italian only (`WORDS` has no row for it, so its votes go to
// Spanish); a language row of its own if more pages need it.
fn italian(text: &str) -> bool {
    const ONLY: [&str; 16] = [
        "è", "che", "della", "delle", "degli", "dei", "gli", "sono", "nel", "nella", "dal", "alla",
        "anche", "questo", "questa", "molto",
    ];
    let words = (text.split(|c: char| !c.is_alphabetic()))
        .filter(|w| ONLY.contains(&w.to_lowercase().as_str()))
        .count();
    words >= 2 && whichlang::detect_language(text) == whichlang::Lang::Ita
}

/// [`sure`], and certain enough to keep a line out of the page's language
/// (`pipeline::Real::routes`): another script, or 3+ words whose votes put
/// one language strictly ahead AND the detector says the same. The lines
/// the detector alone misjudged ("Known dwarf planets" as Spanish) are not.
pub fn certain(text: &str) -> Option<&'static str> {
    let s = sure(text)?;
    if !matches!(s, "en" | "es" | "fr" | "de") {
        return Some(s);
    }
    let v = votes(&[text]);
    let i = WORDS.iter().position(|w| w.0 == s)?;
    let ahead = (v.iter().enumerate()).all(|(k, n)| k == i || *n < v[i]);
    (ahead && detected(text, &v) == Some(s)).then_some(s)
}

/// A short Latin line's language by a small trained detector (whichlang:
/// MIT, a linear model over n-grams, exe +0.3 MB, microseconds a line):
/// French lyric lines in an English page ("Le jour de gloire est arrivé"
/// has one French and one Spanish vote). Only for 3+ plain words making 3/4
/// of the line (never IPA "[a.löz_ö.fä da la]", menus, "Text"), and only
/// when that language has a vote of its own: on the matrix pages the
/// detector alone said "Modifier le code" is Spanish, on English Wikipedia
/// (Linux, PP-OCR) "Known dwarf planets" and "Local Interstellar Cloud".
fn detected(text: &str, votes: &[usize; WORDS.len()]) -> Option<&'static str> {
    let plain = |w: &&&str| {
        let w = w.trim_end_matches([',', ';', ':', '.', '!', '?']);
        !w.is_empty()
            && w.split(['\'', '’', '-'])
                .all(|p| !p.is_empty() && p.chars().all(char::is_alphabetic))
    };
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let words = tokens.iter().filter(plain).count();
    if words < 3 || words * 4 < tokens.len() * 3 {
        return None;
    }
    use whichlang::Lang;
    let code = match whichlang::detect_language(text) {
        Lang::Eng => "en",
        Lang::Fra => "fr",
        Lang::Deu => "de",
        Lang::Spa => "es",
        _ => return None,
    };
    let i = WORDS.iter().position(|w| w.0 == code)?;
    (votes[i] > 0 && spelled(text, code)).then_some(code)
}

/// Every accented letter of `text` is one `code` writes: a French gacha
/// banner's "Chevalier du Givre Éternel" (Windows OCR, Win10 VM) has only
/// "du", which German shares, and the detector said German; German has no "é".
fn spelled(text: &str, code: &str) -> bool {
    let own = match code {
        "es" => "ñáéíóúü",
        "fr" => "éèêëàâîïôûùüÿçœæ",
        "de" => "äöüß",
        _ => "",
    };
    (text.chars().flat_map(char::to_lowercase))
        .filter(|c| c.is_alphabetic() && !c.is_ascii())
        .all(|c| own.contains(c))
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
        // film subtitles: few common words, their endings and accents decide
        // (before: no listed word, so English)
        for es in [
            "Esta hoja tiene un pasado oscuro.",
            "Ha derramado mucha sangre inocente.",
            "Eres una tonta por viajar sola,",
            "Éstas son tierras de dragones, Sintel.",
        ] {
            assert_eq!((es, guess([es])), (es, Some("es")));
            assert_eq!((es, sure(es)), (es, Some("es")));
            assert_eq!((es, certain(es)), (es, Some("es")));
        }
        let banner = "To exit full screen, move mouse to top of screen or press Esc";
        assert_eq!(sure(banner), Some("en"));
        // too little to tell: the page decides
        assert_eq!(sure("Blender Studio"), None);
        assert_eq!(sure("Gracias."), None);
        assert_eq!(sure("Главное меню"), Some("ru"));
    }

    /// it.wikipedia "Sole" in Edge on the Win10 VM, read by PP-OCR: an
    /// Italian page (before: Spanish, offered as "Spanish -> Arabic").
    #[test]
    fn an_italian_page_is_italian() {
        let page = [
            "QCerca in Wikipedia",
            "Ricerca",
            "L'enciclopedia libera",
            "Indice",
            "nascondi",
            "Modifica Modifica wikitesto Cronolog",
            ">Osservazione",
            "Posizione allinterno della Galassia",
            "Ciclo vitale",
            "Il Sole (dal latino Sol) è la stella madre del sistema solare,[6]attorno alla quale \
             orbitano gli otto pianeti principali (tra cui la Terra), i pianeti nani, i loro satelliti",
            "Caratteristiche morfologiche e roazione",
            "massa del Sole, che ammonta a circa 1,9891 × 1030 kg,²] rappresenta da sola il 99,86% \
             della massa complessiva del sistema solare.[7][8]",
            "Il Sole è una stella di dimensioni medio-piccole costituita principalmente da idrogeno \
             (circa il 74% della sua massa, il 92,1% del suo volume) ed elio (circa il 24-25% della",
            "Composizione chimica",
            "Produzione di energia: le reazioni nucleari",
            "Tale processo genera ogni secondo una grande quantità di energia (equivalente a una",
        ];
        assert_eq!(guess(page), Some("it"));
        // a Spanish and a French page stay theirs
        let es = "El Sol es la estrella del sistema solar y la más cercana a la Tierra";
        assert_eq!(
            guess([es, "Buscar en Wikipedia", "Herramientas"]),
            Some("es")
        );
        let fr = "Le Soleil est l'étoile du Système solaire, dont la Terre fait partie";
        assert_eq!(guess([fr, "Rechercher sur Wikipédia"]), Some("fr"));
    }

    /// "La Marseillaise" on English Wikipedia (Windows OCR on the VM): short
    /// French lyric lines are French on their own (before: none of these; the
    /// page's English applied and they stayed untranslated); English lines,
    /// menus, IPA and the other pages' short labels the detector misjudged
    /// stay with the page.
    #[test]
    fn short_foreign_lines_are_told_by_the_detector() {
        for fr in [
            "Allons enfants de la Patrie,",
            "Le jour de gloire est arrivé",
            "Ils viennent jusque dans vos bras Égorger vos fils, vos compagnes",
            "I} Aux armes, citoyens;",
            "Qu'un sang impur",
        ] {
            assert_eq!((fr, sure(fr)), (fr, Some("fr")));
        }
        for (page, line) in [
            ("en", "Against us; tyranny's"),
            ("en", "Form your battalions"),
            ("en", "Let's marchl let's marchl"),
            ("en", "Water our furrowsl 41"),
            ("en", "IPA transcription"),
            ("en", "[fog.me vo ba-ta.jö I]"),
            ("en", "[il vje.na sys.ke dd vo bga l]"),
            // whichlang alone: Spanish, German, English
            ("fr", "Modifier le code"),
            ("fr", "Q Rechercher sur Wikipédia"),
            ("es", "Q Buscaren Wikipedia"),
            // no word of its own: the detector alone said French; on the
            // Ubuntu gate it said Spanish for the English lines below
            ("en", "Abreuve nos sillons 41"),
            // Ubuntu, Firefox, PP-OCR (English, French Wikipedia): read as
            // Spanish (the detector alone, two "-o"/"-a" endings, three "de")
            ("en", "Known dwarf planets"),
            ("en", "Known minor planets"),
            ("en", "Local Interstellar Cloud"),
            ("en", "Pluto aumea"),
            ("en", "View source View history"),
            (
                "fr",
                "Nb. de satellites plus de 600, dont 205 naturels connus de planètes (150",
            ),
            // a French gacha banner (Win10 VM, Windows OCR): its only common
            // word, "du", is German too, and the detector said German
            ("fr", "Chevalier du Givre Éternel"),
        ] {
            let got = sure(line);
            assert!(got.is_none() || got == Some(page), "{line}: {got:?}");
            let got = certain(line);
            assert!(got.is_none() || got == Some(page), "{line}: {got:?}");
        }
    }
}
