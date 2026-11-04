//! The owner's three game screens (`tests/fixtures/translate/games`, his
//! own generated art; frames 2560x1440 as on his screen): a German cutscene
//! with a quest HUD, a French gacha banner, a Spanish settings menu. Their
//! Windows OCR reads (Win10 VM) and PP-OCR reads are recorded; the model
//! outputs below are the real offline ones where a step needs one.

use super::super::pipeline::*;
use super::*;
use crate::ocr::TextBlock;
use std::cell::RefCell;

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/translate/games/"
);

fn read(name: &str) -> Vec<TextBlock> {
    serde_json::from_slice(&std::fs::read(format!("{DIR}{name}.json")).unwrap()).unwrap()
}

/// The screen as one-shot translate reads it on Windows (`Real::read_page`).
fn screen(name: &str) -> Vec<String> {
    let win = read(&format!("{name}.win"));
    assert!(win.len() <= SPARSE, "{name}: a game's screen is sparse");
    reread(win, read(&format!("{name}.pp")))
        .into_iter()
        .map(|b| to_line(b, 2560, 1440).text)
        .collect()
}

#[test]
fn windows_ocr_misses_are_read_again() {
    let quest = screen("quest");
    // Windows OCR: "Erreiche die Himmelsbrüc$gu—", "[kVJQuestgebiét{mzéigeni"
    for line in [
        "Erreiche die Himmelsbrücke (450m)",
        "[V] Questgebiet anzeigen",
        "Lyra",
        "Lv. 80",
    ] {
        assert!(quest.iter().any(|l| l == line), "{line}: {quest:?}");
    }
    let menu = screen("menu");
    // missed by Windows OCR (serif buttons), and accents it dropped
    for line in [
        "Aplicar",
        "Restablecer",
        "Gráficos",
        "Límite de FPS",
        "Sincronización vertical",
    ] {
        assert!(menu.iter().any(|l| l == line), "{line}: {menu:?}");
    }
    assert!(!menu.iter().any(|l| l.contains('å')), "{menu:?}");
    let gacha = screen("gacha");
    // the diamond icon read as "O" is no text; the star Windows OCR read stays
    // (PP-OCR: "4 ou"); the multipliers are numbers again
    for line in [
        "Invocation d'Événement",
        "garantit au moins un objet 4★ ou",
        "Invoquer ×1",
        "Invoquer ×10",
    ] {
        assert!(gacha.iter().any(|l| l == line), "{line}: {gacha:?}");
    }
    assert!(!gacha.iter().any(|l| l.starts_with("O ")), "{gacha:?}");
}

#[test]
fn a_garbled_read_is_told_from_text() {
    for bad in [
        "Erreiche die Himmelsbrüc$gu—",
        "[kVJQuestgebiét{mzéigeni",
        "Gråficos",
        "Sincronizaci6n vertical",
    ] {
        assert!(garbled(bad), "{bad}");
    }
    for fine in [
        "Brücke der Dämmerung",
        "Invocation d'Événement",
        "3240 +",
        "16400/16400",
        "Chaque Série de 10 invocations",
        "[V] Questgebiet anzeigen",
    ] {
        assert!(!garbled(fine), "{fine}");
    }
}

#[test]
fn counters_are_read_and_kept_exactly() {
    assert_eq!(game_read("Invoquer XIO"), "Invoquer ×10");
    assert_eq!(game_read("Invoquer xl"), "Invoquer ×1");
    assert_eq!(game_read("objet 4* ou"), "objet 4★ ou");
    for same in ["Chapter XI", "Size XL", "xo xo", "x", "Personnages 4 à"] {
        assert_eq!(game_read(same), same);
    }
    // offline outputs (fr-en, fr-ar; Win10 VM build 3acbcc9 and here)
    let route = crate::mt::route("fr", "ar").unwrap();
    let sent = RefCell::new(Vec::new());
    let run = |t: &[String], _: &'static crate::mt::Pair| {
        sent.borrow_mut().extend(t.iter().cloned());
        Ok(t.iter()
            .map(|s| match s.as_str() {
                "Invoquer 10" => "اضغط x 10".to_string(),
                "Chaque série de 10 invocations garantit au moins un objet 4 ou supérieur." => {
                    "وتكفل كل مجموعة من 10 حالات جسما واحدا على الأقل من الأشياء 4 أو أكثر."
                        .to_string()
                }
                "Personnages 4 taux augmenté" => "زيادة ٤ في عدد الأفراد".to_string(),
                s => s.to_string(),
            })
            .collect())
    };
    let src: Vec<String> = [
        "Invoquer ×10",
        "Chaque série de 10 invocations garantit au moins un objet 4★ ou supérieur.",
        "Personnages 4★ taux augmenté",
    ]
    .map(String::from)
    .into();
    let out = through(&src, &route, "ar", run).unwrap();
    assert_eq!(out[0], "اضغط ×10");
    assert!(out[1].contains("الأشياء 4★ أو"), "{}", out[1]);
    assert_eq!(out[2], "زيادة 4★ في عدد الأفراد");
    // the model never saw a star or a multiplier
    assert!(
        sent.borrow().iter().all(|s| !s.contains(['★', '×'])),
        "{:?}",
        sent.borrow()
    );
    // a counter or a badge alone is no text to translate
    for keep in ["21ms", "Lv. 80", "×10", "UP!", "3240 +", "16400/16400"] {
        assert!(!needs_translation(keep, "ar"), "{keep}");
    }
}

#[test]
fn key_hints_names_and_quotes_stay_as_the_source_has_them() {
    let route = crate::mt::route("de", "ar").unwrap();
    // de-en then en-ar as the models answered (Win10 VM, the owner's run)
    let run = |t: &[String], p: &'static crate::mt::Pair| {
        Ok(t.iter()
            .map(|s| match (p.source, s.as_str()) {
                ("de", "Questgebiet anzeigen") => "Show quest area".to_string(),
                ("en", "Show quest area") => "اعرض منطقة البحث".to_string(),
                ("de", "Lyra") => "Lyra".to_string(),
                ("en", "Lyra") => "لايرا".to_string(),
                ("de", "Brücke der Dämmerung") => "Bridge of dusk".to_string(),
                ("en", "Bridge of dusk") => "\"جسر الغسق\"".to_string(),
                (_, s) => s.to_string(),
            })
            .collect())
    };
    let src: Vec<String> = ["[V] Questgebiet anzeigen", "Lyra", "Brücke der Dämmerung"]
        .map(String::from)
        .into();
    let out = through(&src, &route, "ar", run).unwrap();
    // the hint first, as written (the model moved it: "اعرض [V] منطقة البحث")
    assert_eq!(out[0], "[V] اعرض منطقة البحث");
    // a name only spelled in Arabic letters stays (its redraw over the art
    // left the old letters under the new)
    assert_eq!(out[1], "Lyra");
    // no quotes the source doesn't have
    assert_eq!(out[2], "جسر الغسق");
}

#[test]
fn a_key_hint_line_is_an_item_of_its_own() {
    let lines: Vec<Line> = reread(read("quest.win"), read("quest.pp"))
        .into_iter()
        .map(|b| to_line(b, 2560, 1440))
        .collect();
    let blocks = group_blocks(&lines);
    let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
    assert!(
        texts.contains(&"Erreiche die Himmelsbrücke (450m)"),
        "{texts:?}"
    );
    assert!(texts.contains(&"[V] Questgebiet anzeigen"), "{texts:?}");
}

#[test]
fn a_title_s_noun_is_not_kept_as_a_name() {
    // fr-en kept "Givre" as a name in a Title Case label; in lower case it is
    // "frost" (real fr-en outputs); a real name stays a name
    let route = crate::mt::route("fr", "en").unwrap();
    let run = |t: &[String], _: &'static crate::mt::Pair| {
        Ok(t.iter()
            .map(|s| {
                match s.as_str() {
                    "Chevalier du Givre Éternel" => "Knight of the Eternal Givre",
                    "Chevalier du givre éternel" => "Knight of eternal frost",
                    "Le chevalier Aldric" => "The knight Aldric",
                    "Le chevalier aldric" => "The knight aldric",
                    s => s,
                }
                .to_string()
            })
            .collect())
    };
    let src: Vec<String> = ["Chevalier du Givre Éternel", "Le chevalier Aldric"]
        .map(String::from)
        .into();
    let out = through(&src, &route, "en", run).unwrap();
    assert_eq!(out, ["Knight of Eternal Frost", "The knight Aldric"]);
}

#[test]
fn menu_words_take_their_menu_sense_offline() {
    let real = Real::new("auto", Via::Offline);
    let texts: Vec<String> = ["Controles", "Brillo", "Invoquer ×10", "Audio"]
        .map(String::from)
        .into();
    // no model is called (none is on disk here): the glossary answers
    let out = real
        .on_device_with(&texts, "en", |_, _| panic!("no model for menu words"))
        .unwrap();
    assert_eq!(out, ["Controls", "Brightness", "Summon ×10", "Audio"]);
}
