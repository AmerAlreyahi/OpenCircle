use super::strings::KEYS;
use super::*;

/// Every key has both languages, the same `{}` values, and no bidi control
/// of its own (the table adds the one mark; isolates come from `fmt`).
#[test]
fn every_key_has_english_and_arabic() {
    const CONTROLS: [char; 11] = [
        '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
        '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ];
    assert_eq!(TABLE.len(), KEYS.len());
    for (key, [en, ar]) in KEYS.iter().zip(TABLE) {
        let ar = ar
            .strip_prefix(RLM)
            .expect("the table leads Arabic with the mark");
        assert!(
            !en.trim().is_empty() && !ar.trim().is_empty(),
            "{key}: empty"
        );
        assert_eq!(
            en.matches("{}").count(),
            ar.matches("{}").count().max(usize::from(*key == "ab_bio")),
            "{key}: the values differ"
        );
        for s in [en, ar] {
            assert!(!s.contains(CONTROLS), "{key}: a bidi control in {s:?}");
        }
    }
}

/// Every key is used by the code (a dead string is deleted, not kept).
#[test]
fn every_key_is_used() {
    fn walk(dir: &std::path::Path, out: &mut String) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("strings.rs") {
                out.push_str(&std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    let mut src = String::new();
    walk(std::path::Path::new("src"), &mut src);
    let unused: Vec<_> = (KEYS.iter())
        .filter(|k| !src.contains(&format!("S::{k}")) && !src.contains(&format!("t!({k})")))
        .collect();
    assert!(unused.is_empty(), "unused strings: {unused:?}");
}

#[test]
fn a_lookup_is_the_current_language() {
    set(Lang::En);
    assert_eq!(tr(S::nav_general), "General");
    assert!(!rtl());
    set(Lang::Ar);
    assert_eq!(bare(S::nav_general), "عام");
    assert!(rtl());
    assert_eq!(Lang::Ar.endonym(), "\u{200F}العربية");
    set(Lang::En);
    assert_eq!(
        Lang::Ar.endonym(),
        "\u{200F}العربية",
        "its own name in any UI"
    );
    assert_eq!(language_name("ar"), "Arabic");
}

/// In Arabic a value is isolated, so "Ctrl+Shift+Space." keeps its order
/// and the sentence's period stays at its end; English is untouched.
#[test]
fn values_are_isolated_in_arabic_only() {
    set(Lang::En);
    assert_eq!(fmt(S::k_used_by, &[&"Snagit"]), "Used by Snagit");
    set(Lang::Ar);
    assert_eq!(
        fmt(S::k_used_by, &[&"Snagit"]),
        "\u{200F}يستخدمه \u{2068}Snagit\u{2069}"
    );
    set(Lang::En);
}

#[test]
fn counts_take_arabics_plural_forms() {
    let forms = [
        S::win_pill_one,
        S::win_pill_two,
        S::win_pill_few,
        S::win_pill_many,
    ];
    set(Lang::En);
    assert_eq!(count(1, forms), "1 problem to fix before saving");
    assert_eq!(count(3, forms), "3 problems to fix before saving");
    set(Lang::Ar);
    let ar = |n| count(n, forms).replace(['\u{200F}', '\u{2068}', '\u{2069}'], "");
    assert!(ar(1).starts_with("مشكلة واحدة"));
    assert!(ar(2).starts_with("مشكلتان"));
    assert!(ar(3).starts_with("3 مشكلات"));
    assert!(ar(10).starts_with("10 مشكلات"));
    assert!(ar(11).starts_with("11 مشكلة"));
    assert!(ar(103).starts_with("103 مشكلات"));
    set(Lang::En);
}

/// Settings' choice wins, then the onboarding pick, then the OS language;
/// a language the app doesn't speak is English.
#[test]
fn the_ui_language_comes_from_the_config() {
    let g = |app: &str, picked: &str| crate::config::GeneralConfig {
        app_language: app.into(),
        language: picked.into(),
        ..Default::default()
    };
    assert_eq!(of_config(&g("", "")), Lang::En, "the OS (English in tests)");
    assert_eq!(of_config(&g("", "ar")), Lang::Ar);
    assert_eq!(of_config(&g("", "fr")), Lang::En);
    assert_eq!(of_config(&g("en", "ar")), Lang::En);
    assert_eq!(of_config(&g("ar", "fr")), Lang::Ar);
    assert_eq!(Lang::of("ar-SA"), Some(Lang::Ar));
}

/// The screens that speak the table write no words of their own: every
/// user-visible literal is in `strings.rs`. What remains is not UI text
/// (ids, URLs, key names, product names) or stays English on purpose
/// (About's legal credits).
#[test]
fn settings_and_onboarding_write_no_words_of_their_own() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("tests.rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(std::path::Path::new("src/ui/screens/settings"), &mut files);
    walk(
        std::path::Path::new("src/ui/screens/onboarding"),
        &mut files,
    );
    let found = words_outside_the_table(&files);
    assert!(found.is_empty(), "words outside the table: {found:#?}");
}

/// Part 2a's surfaces too: the tray menu, the update widgets, the pills and
/// toasts, the model download card and list, the translate pill and the
/// language picker. (The Linux-only AppIndicator labels come from the same
/// `Entry::label`; backend sentences are typed and worded in the UI.)
#[test]
fn the_part_2a_surfaces_write_no_words_of_their_own() {
    let files: Vec<std::path::PathBuf> = [
        "src/ui/screens/tray_menu.rs",
        "src/ui/screens/update_pill.rs",
        "src/ui/screens/update_card.rs",
        "src/ui/screens/toast.rs",
        "src/ui/screens/model_download.rs",
        "src/ui/screens/model_list.rs",
        "src/ui/screens/translate/view.rs",
        "src/ui/screens/common/language_picker.rs",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .collect();
    let found = words_outside_the_table(&files);
    assert!(found.is_empty(), "words outside the table: {found:#?}");
}

/// The user-visible-looking literals of `files` (production code) that are
/// not in `NOT_UI`: two words or more, or one capitalised word.
fn words_outside_the_table(files: &[std::path::PathBuf]) -> Vec<String> {
    let words = |s: &str| {
        let letters = s.chars().filter(|c| c.is_alphabetic()).count();
        letters >= 2
            && (s
                .split(' ')
                .filter(|w| w.chars().any(char::is_alphabetic))
                .count()
                >= 2
                || s.starts_with(|c: char| c.is_uppercase()))
    };
    let mut found = Vec::new();
    for f in files {
        let name = f.to_string_lossy().replace('\\', "/");
        if name.ends_with("about.rs") {
            continue; // the credits stay English (legal attribution)
        }
        // (a Windows checkout has CRLF line ends: the split below must still find the test module)
        let src = std::fs::read_to_string(f).unwrap().replace("\r\n", "\n");
        // production code only: up to an inline test module
        let src = src.split("#[cfg(test)]\nmod tests {").next().unwrap();
        for line in src.lines() {
            let l = line.trim_start();
            if l.starts_with("//") || l.starts_with("#[") || l.contains("log!(") {
                continue;
            }
            for lit in literals(l) {
                if words(&lit) && !NOT_UI.contains(&lit.as_str()) {
                    found.push(format!("{name}: {lit:?}"));
                }
            }
        }
    }
    found
}

/// The string literals of a line.
fn literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    let mut prev = ' ';
    while let Some(c) = chars.next() {
        if c == '"' && prev != '\'' {
            let mut s = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        chars.next();
                    }
                    '"' => break,
                    c => s.push(c),
                }
            }
            out.push(s);
        }
        prev = c;
    }
    out
}

/// Literals in the two screens that are not words for the user.
const NOT_UI: &[&str] = &[
    // build values, OS names, file magic, panics
    "CARGO_PKG_VERSION",
    "OUT_DIR",
    "Windows",
    "Linux",
    "DKIF",
    "AV01",
    "KeyJob({})",
    "every section has a page",
    "every step has a page",
    "keep editing",
    // key names (a shortcut is written in Latin key names in every language)
    "Control",
    "Shift",
    "Alt",
    "Meta",
    "Super",
    "Fn",
    "CapsLock",
    "Key",
    "Digit",
    "Ctrl",
    "PrintScreen",
    "PrtSc",
    "Escape",
    "Esc",
    "Delete",
    "Backspace",
    // the CLI (frozen contract)
    "opencircle capture",
    "opencircle run {}",
    // "key: reason" problems: the reason shows under its row. They mirror the
    // config's own checks (src/config, English): translated with them, later
    "ai.provider: Choose a provider (the settings file couldn't be read)",
    "config: {e}",
    "ai.model: choose a model",
    "ai.base_url: enter your server's URL",
    "ai.base_url: {why}",
    // part 2a: service names, the file's formats and the window titles' ids
    "GitHub",
    "Hugging Face",
    "OpenRouter",
    "OpenCircle",
    "Ollama",
    "LM Studio",
    "Google",
    "language-search",
    "model-search",
    "auto",
    "detect language",
    "png",
    "jpg",
    "OpenCircle-{stamp}{suffix}.{ext}",
    "OpenCircle-{stamp}.{ext}",
    // numbers with their unit; a panic message; a test-only label
    "{done:.1} / {} MB",
    "ModelsJob({:?})",
    "just checked",
    "Auto",
];

/// Every character the Arabic strings use has a glyph in the bundled fonts
/// (Noto Sans Arabic for the Arabic, Inter for the Latin and signs): a
/// missing one would fall to whatever face the OS has, unjoined or boxed.
#[test]
fn every_character_of_the_arabic_table_has_a_bundled_glyph() {
    let noto = ttf_parser::Face::parse(include_bytes!("../../assets/fonts/NotoSansArabic.ttf"), 0)
        .unwrap();
    let inter = ttf_parser::Face::parse(include_bytes!("../../assets/fonts/Inter.ttf"), 0).unwrap();
    let mut missing = std::collections::BTreeSet::new();
    for [_, ar] in TABLE {
        for c in ar.chars() {
            let control = matches!(c, '\u{200F}' | '\u{2066}'..='\u{2069}') || c == '\n';
            let has = |f: &ttf_parser::Face| f.glyph_index(c).is_some();
            if !control && !has(&noto) && !has(&inter) {
                missing.insert(c);
            }
        }
    }
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}
