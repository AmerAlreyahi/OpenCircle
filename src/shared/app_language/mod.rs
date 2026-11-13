//! App language (owner, 2026-11-11): the UI's words in English and Arabic
//! from ONE table (`strings.rs`), and the language the app speaks.
//!
//! A string is a key of `S` (`tr(S::g_lang)`, or `t!(g_lang)`): a missing
//! key does not compile, a missing Arabic string does not compile, and a
//! lookup is an index into a const table (no hashing, no file, no network).
//! Values put into a sentence go through `fmt` (`{}` placeholders): in
//! Arabic each one is a first-strong isolate (U+2068 … U+2069), so a
//! shortcut, a path or "8 MB" keeps its own order and the sentence's period
//! stays at its end. Every Arabic string starts with a right-to-left mark
//! (added by the table), so a line that starts with "OpenCircle" or
//! "Windows" is still a right-to-left line.
//!
//! The language is per UI thread (like the kit's theme: tests draw English
//! and Arabic side by side), read from the config the first time a thread
//! asks, and set again when Settings or onboarding change it. Not to be
//! confused with `translate::lang`, the languages text is translated between.
mod strings;

use std::cell::Cell;

pub use strings::S;
use strings::TABLE;

/// The UI languages, in the order Settings lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En = 0,
    Ar = 1,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ar];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ar => "ar",
        }
    }

    /// The UI language of a language code ("ar", "ar-SA"), if the app has it.
    pub fn of(code: &str) -> Option<Lang> {
        let base = code.split(['-', '_']).next().unwrap_or_default();
        Lang::ALL
            .into_iter()
            .find(|l| l.code().eq_ignore_ascii_case(base))
    }

    /// Its name in itself (the language lists: "English", "العربية").
    pub fn endonym(self) -> &'static str {
        TABLE[self.name_key() as usize][self as usize]
    }

    fn name_key(self) -> S {
        match self {
            Lang::En => S::lang_en,
            Lang::Ar => S::lang_ar,
        }
    }

    pub fn rtl(self) -> bool {
        self == Lang::Ar
    }
}

thread_local! {
    static CURRENT: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// The UI language: Settings' App language, else the language picked in
/// onboarding (`general.language`, "" = the OS's), when the app has it; else English.
pub fn of_config(general: &crate::shared::config::GeneralConfig) -> Lang {
    let picked = match general.language.as_str() {
        "" => crate::shared::config::os_language(),
        code => code,
    };
    [general.app_language.as_str(), picked]
        .into_iter()
        .find(|c| !c.is_empty())
        .and_then(Lang::of)
        .unwrap_or(Lang::En)
}

/// The language this thread speaks (read from the config once).
pub fn current() -> Lang {
    CURRENT.get().unwrap_or_else(|| {
        let lang = of_config(&crate::shared::config::current().general);
        CURRENT.set(Some(lang));
        lang
    })
}

/// Speak `lang` from now on (the next redraw of every window is in it).
pub fn set(lang: Lang) {
    CURRENT.set(Some(lang));
}

/// Speak what `general` says (Settings' choice, else "your language"), and
/// remember which language "your language" is (the App language list).
pub fn apply(general: &crate::shared::config::GeneralConfig) {
    let yours = crate::shared::config::GeneralConfig {
        app_language: String::new(),
        ..general.clone()
    };
    YOURS.set(of_config(&yours));
    set(of_config(general));
}

/// The UI language "your language" means now (the onboarding pick, else
/// the OS's; English when the app doesn't speak it).
pub fn yours() -> Lang {
    YOURS.get()
}

thread_local! {
    static YOURS: Cell<Lang> = const { Cell::new(Lang::En) };
}

/// A choice of Settings' App language: "" = your language, else a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice(pub &'static str);

/// The App language list: your language, then each language in its own name.
pub const CHOICES: [Choice; 3] = [Choice(""), Choice("en"), Choice("ar")];

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match Lang::of(self.0) {
            Some(lang) => f.write_str(lang.endonym()),
            None => f.write_str(&fmt(
                S::g_lang_yours,
                &[&yours().endonym().trim_start_matches(RLM)],
            )),
        }
    }
}

/// The current language is written right to left.
pub fn rtl() -> bool {
    current().rtl()
}

/// `s` in the current language.
pub fn tr(s: S) -> &'static str {
    TABLE[s as usize][current() as usize]
}

/// `s` in English whatever the app speaks (the log and diagnostics stay English).
#[cfg(any(target_os = "linux", test))]
pub fn english(s: S) -> &'static str {
    TABLE[s as usize][Lang::En as usize]
}

/// `tr(s)` without its leading direction mark (tests compare names with it).
#[cfg(test)]
pub fn bare(s: S) -> &'static str {
    tr(s).trim_start_matches(RLM)
}

/// The right-to-left mark every Arabic string starts with.
pub const RLM: char = '\u{200F}';
const FSI: char = '\u{2068}';
const PDI: char = '\u{2069}';

/// `s` with each `{}` replaced by the next of `args` (in Arabic, isolated).
pub fn fmt(s: S, args: &[&dyn std::fmt::Display]) -> String {
    let isolate = rtl();
    let mut out = String::new();
    let mut args = args.iter();
    let mut parts = tr(s).split("{}");
    out.push_str(parts.next().unwrap_or_default());
    for part in parts {
        if let Some(a) = args.next() {
            // (a value from the table brings its own mark: one per line is enough)
            let a = a.to_string().replace(RLM, "");
            // a Latin value (a shortcut, a path, "OpenRouter") is isolated;
            // an Arabic word or a bare number reads right as it is
            match isolate && a.chars().any(|c| c.is_ascii_alphabetic()) {
                true => out.push_str(&format!("{FSI}{a}{PDI}")),
                false => out.push_str(&a),
            }
        }
        out.push_str(part);
    }
    out
}

/// A count in a sentence: `forms` = [one, two, few (3-10), many (11+ and
/// the rest)], each a key whose `{}` is the count. English uses one and
/// many; Arabic's forms follow its plural rules (CLDR: 0 and 100 take
/// "many" here, which Arabic writes as its "other").
pub fn count(n: usize, forms: [S; 4]) -> String {
    let form = match (current(), n) {
        (_, 1) => forms[0],
        (Lang::En, _) => forms[3],
        (Lang::Ar, 2) => forms[1],
        (Lang::Ar, n) if (3..=10).contains(&(n % 100)) => forms[2],
        (Lang::Ar, _) => forms[3],
    };
    fmt(form, &[&n])
}

/// A translate language's name in the app's language ("ar" -> "Arabic" /
/// "العربية"); one the table lacks keeps `translate::lang`'s English name.
pub fn language_name(code: &str) -> &'static str {
    let key = match code {
        "ar" => S::lang_ar,
        "en" => S::lang_en,
        "fr" => S::lang_fr,
        "de" => S::lang_de,
        "es" => S::lang_es,
        "pt" => S::lang_pt,
        "pt-BR" => S::lang_pt_br,
        "it" => S::lang_it,
        "nl" => S::lang_nl,
        "ru" => S::lang_ru,
        "uk" => S::lang_uk,
        "pl" => S::lang_pl,
        "tr" => S::lang_tr,
        "fa" => S::lang_fa,
        "ur" => S::lang_ur,
        "he" => S::lang_he,
        "hi" => S::lang_hi,
        "id" => S::lang_id,
        "vi" => S::lang_vi,
        "th" => S::lang_th,
        "ja" => S::lang_ja,
        "ko" => S::lang_ko,
        "zh" => S::lang_zh,
        "zh-Hant" => S::lang_zh_hant,
        _ => return crate::translate::lang::find(code).map_or("", |l| l.name),
    };
    tr(key)
}

/// A string a backend names (a pill's text) and the UI says in its language
/// when it shows it: `Said(S::pill_copied).to_string()` / `.into()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Said(pub S);

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(tr(self.0))
    }
}

impl From<Said> for String {
    fn from(s: Said) -> String {
        tr(s.0).to_string()
    }
}

/// A key prints as its string in the current language.
impl std::fmt::Display for S {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(tr(*self))
    }
}

/// `tr(S::…)` by the key's name.
#[macro_export]
macro_rules! t {
    ($key:ident) => {
        $crate::shared::app_language::tr($crate::shared::app_language::S::$key)
    };
}

#[cfg(test)]
mod tests;
