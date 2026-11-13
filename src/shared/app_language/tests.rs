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
    let g = |app: &str, picked: &str| crate::shared::config::GeneralConfig {
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
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("tests.rs")
                // (labs.rs is the Labs switches' logic, not a screen)
                && !p.ends_with("settings/labs.rs")
            {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(std::path::Path::new("src/features/settings"), &mut files);
    walk(std::path::Path::new("src/features/onboarding"), &mut files);
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
        "src/features/text_reading/screens/model_download.rs",
        "src/features/ask_ai/screens/model_list.rs",
        "src/ui/screens/translate/view.rs",
        "src/ui/screens/common/language_picker.rs",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .collect();
    let found = words_outside_the_table(&files);
    assert!(found.is_empty(), "words outside the table: {found:#?}");
}

/// The whole UI (owner, 2026-11-11, part 2b): every file of `src/ui/` and
/// `src/actions/` writes no word of its own: what the user reads comes from
/// the table. The files below hold text that is not for the user (a prompt for
/// the model, a page for the browser, OS and font names, the key names a
/// shortcut is written in) or is listed in `UNCONVERTED`.
#[test]
fn the_whole_ui_writes_no_words_of_its_own() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let name = p.to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                if !name.ends_with("/testing") {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs")
                && !EXEMPT_FILES.iter().any(|f| name.ends_with(f))
            {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    for root in UI_SOURCES {
        let root = std::path::Path::new(root);
        if root.is_file() {
            files.push(root.to_path_buf());
        } else {
            walk(root, &mut files);
        }
    }
    files.retain(|p| {
        let name = p.to_string_lossy().replace('\\', "/");
        !EXEMPT_FILES.iter().any(|f| name.ends_with(f))
    });
    let found = words_outside_the_table(&files);
    assert!(found.is_empty(), "words outside the table: {found:#?}");
}

/// The UI's files (what was `src/ui/` and `src/actions/` before the feature
/// folders): folders and single files.
const UI_SOURCES: &[&str] = &[
    "src/ui",
    "src/actions",
    "src/shared/kit",
    "src/shared/theme.rs",
    "src/shared/touch.rs",
    "src/shared/platform/monitor.rs",
    "src/shared/platform/shape.rs",
    "src/shared/platform/tray.rs",
    "src/shared/platform/windows.rs",
    "src/features/song/screens",
    "src/features/onboarding",
    "src/features/settings/mod.rs",
    "src/features/settings/model_list.rs",
    "src/features/settings/pages",
    "src/features/settings/previews",
    "src/features/settings/state.rs",
    "src/features/settings/window.rs",
    "src/features/ask_ai/screens",
    "src/features/ask_ai/app",
    "src/features/ask_ai/ask.rs",
    "src/features/text_reading/screens",
    "src/features/text_reading/app",
    "src/features/text_reading/text.rs",
];

/// Files of the UI that hold no user-visible words, and why.
const EXEMPT_FILES: &[&str] = &[
    // tests and their harness
    "/tests.rs",
    "/arch_tests.rs",
    "/frame_times.rs",
    "/snip_shots.rs",
    // the model's own prompts (English, sent to the provider), the AI setup steps
    "src/features/ask_ai/ask.rs",
    // the page the browser posts the picture from (no words), QR content tests
    "src/actions/search_in_browser/page.rs",
    "src/actions/qr.rs",
    // OS, window-class and font names; the tray icon's decode checks
    "src/shared/platform/shape.rs",
    "src/shared/platform/tray.rs",
    "src/shared/platform/monitor.rs",
    "src/shared/platform/windows.rs",
    "src/shared/kit/text.rs",
    // the look and theme names: ids and English fallbacks; Settings shows them from the table
    "src/shared/kit/looks.rs",
    "src/shared/kit/themes.rs",
    // the kit's English default words (tests): Settings always passes its own
    "src/shared/kit/hero.rs",
    // the installer's own window (its words are the Setup block of the table)
    "src/ui/screens/setup/mod.rs",
];

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
        let src = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .unwrap()
            .split("#[cfg(test)]\npub(crate) mod tests {")
            .next()
            .unwrap()
            .split("#[cfg(test)]\npub(crate) mod tests {")
            .next()
            .unwrap()
            .split("\n#[test]\n")
            .next()
            .unwrap();
        for line in src.lines() {
            let l = line.trim_start();
            if l.starts_with("//") || l.starts_with("#[") || l.contains("log!(") {
                continue;
            }
            for lit in literals(l.split(" // ").next().unwrap_or(l)) {
                if words(&lit)
                    && !NOT_UI.contains(&lit.as_str())
                    && !UI_NOT_WORDS.contains(&lit.as_str())
                    && !UNCONVERTED.contains(&lit.as_str())
                    && !lit.starts_with("[opencircle]")
                {
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
    // the OCR recognizers' ids (`ocr::models`), mapped to table keys
    "Arabic",
    "Cyrillic",
    "Chinese and Japanese",
    // build values, OS names, file magic, panics
    "CARGO_PKG_VERSION",
    "LANG",
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

/// Literals of the rest of the UI that are not words for the user: key names
/// and shortcuts (written in Latin key names in every language), the targets
/// of a hover chip (`Snip`'s ladder ids), the labels frame-time measurements
/// print, environment names, test-hook content, the model's prompts (English,
/// sent to the provider) and numbers with their unit.
const UI_NOT_WORDS: &[&str] = &[
    // key names and shortcuts
    "Enter",
    "Shift+Enter",
    "Ctrl+C",
    "Ctrl+S",
    "Ctrl+P",
    "Ctrl+Z",
    "Ctrl+Enter",
    "Ctrl+Shift+Space",
    "RGB",
    // Latin technical / product names on a badge or chip
    "AI",
    "UI",
    "Google AI Mode",
    "OpenCircle AI",
    // measurements and format-only text
    "{px} px",
    "{w}×{h} px",
    "x {}  y {}",
    "{label} · {keys}",
    "rgb({r}, {g}, {b})",
    "{dx} × {dy} px",
    "{dx} × {dy} px · {} × {} at {at} %",
    "{len} px · Δx {dx} · Δy {dy}",
    "{n} px",
    "{n} px · {} at {at} %",
    " · {} at {at} %",
    "Step({} B)",
    "Crop({}x{})",
    // the info chip's target ids (the words drawn come from the table)
    "Element",
    "Window",
    "Screen",
    "Window: {t}",
    "Element: {what}",
    // frame-time measurement labels and internal states (logs, never drawn)
    "bar drag",
    "bar glide",
    "card drag",
    "card resize",
    "chat drag",
    "chat glide",
    "toolbar drag",
    "ruler drag",
    "snip drag",
    "snip area drag",
    "drag, screen at {:?}",
    "drag, starting screen",
    "colour picker",
    "a tool runs",
    "opening animation",
    "screen text ready",
    "first screen text",
    "still waiting",
    "live layer",
    "live pin",
    "beside the area",
    "moved beside the area",
    "over the area (kept out of captures)",
    "in the area's corner (left out: {cover:?})",
    "none (tray only)",
    "Ctrl+{letter}",
    // errors and panics that only reach the log
    "capture failed: no screenshot after {} s; try again",
    "no text found",
    "no monitor found",
    "an empty band",
    "no reader in tests",
    "the overlay's state changed while closed",
    "Windows OCR: a right-to-left language",
    "a Crop's buffer is width * height * 4 bytes",
    "a bundled PNG decodes",
    "the bundled tones decode",
    // OCR noise the screen-text reader filters out (Windows' activation watermark)
    "Activate Windows",
    "Go to Settings, now!",
    // environment variables and the pop-up helper's wire format
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "WAYLAND_SOCKET",
    "OPENCIRCLE_TEST_SHOT",
    "OPENCIRCLE_TEST_LIVE_SHOT",
    "OPENCIRCLE_TEST_DIR",
    "OPENCIRCLE_TEST_OPEN",
    "frame {} {} {} {} {} {} {} {height} {} {} {pw} {ph} {lw} {lh}",
    // test-hook content (`OPENCIRCLE_TEST_OPEN`)
    "Copied",
    "Couldn't save the picture",
    "## Fixes- [A crash](https://example.com) when the screen sleeps- Text selection on a second screen",
    // the model's prompts for a game-scene explanation (sent to the provider)
    "In this game scene, {} was translated as {}. Answer in the language ",
    "{target}: the meaning of the words, then ONE line on what they ",
];

/// Text in the UI's files that looks like words but is not for the user (it
/// is only logged, or is the settings file's own lines). The translate and
/// live backend's sentences, the share sheet's title, the model names in a
/// download question and the AI setup steps are all in the table now.
const UNCONVERTED: &[&str] = &[
    // logged only (`share::start`'s errors go to the log)
    "share: no share sheet on this platform yet",
    "share: no window",
    "share: {what}: {err}",
];

/// Every character the Arabic strings use has a glyph in the bundled fonts
/// (Noto Sans Arabic for the Arabic, Inter for the Latin and signs): a
/// missing one would fall to whatever face the OS has, unjoined or boxed.
#[test]
fn every_character_of_the_arabic_table_has_a_bundled_glyph() {
    let noto = ttf_parser::Face::parse(
        include_bytes!("../../../assets/fonts/NotoSansArabic.ttf"),
        0,
    )
    .unwrap();
    let inter =
        ttf_parser::Face::parse(include_bytes!("../../../assets/fonts/Inter.ttf"), 0).unwrap();
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

/// The language names the pickers show (each language's own name and its
/// name in English and Arabic): which of them the bundled fonts (Noto Sans
/// Arabic, Inter) cover. A language not covered is drawn from the system's
/// fallback face in the app (and as a "missing glyph" box in the goldens, which
/// never read system fonts), so this list is a decision, not an accident:
/// adding a bundled face (or a language) changes it on purpose.
#[test]
fn the_language_names_the_pickers_show_are_covered_or_listed() {
    let noto = ttf_parser::Face::parse(
        include_bytes!("../../../assets/fonts/NotoSansArabic.ttf"),
        0,
    )
    .unwrap();
    let inter =
        ttf_parser::Face::parse(include_bytes!("../../../assets/fonts/Inter.ttf"), 0).unwrap();
    let covered = |s: &str| {
        s.chars().all(|c| {
            matches!(c, '\u{200F}' | '\u{2066}'..='\u{2069}')
                || noto.glyph_index(c).is_some()
                || inter.glyph_index(c).is_some()
        })
    };
    let mut uncovered = Vec::new();
    for l in crate::translate::lang::LANGUAGES {
        let names = [l.native, l.name, language_name_in(l.code, Lang::Ar)];
        if !names.iter().all(|n| covered(n)) {
            uncovered.push(l.code);
        }
    }
    uncovered.sort_unstable();
    // Latin and Arabic-script names (Urdu, Persian included) are bundled; these
    // draw from the system's own fonts until a face is bundled for them
    assert_eq!(
        uncovered, UNCOVERED,
        "the bundled fonts' coverage of the language names changed"
    );
}

/// `code`'s name in `lang` (the table; the app's current language is restored).
fn language_name_in(code: &str, lang: Lang) -> &'static str {
    let was = current();
    set(lang);
    let name = language_name(code);
    set(was);
    name
}

/// Languages whose native name the bundled fonts don't cover (see above).
const UNCOVERED: &[&str] = &[
    "he", "hi", "ja", "ko", "ru", "th", "uk", "vi", "zh", "zh-Hant",
];
