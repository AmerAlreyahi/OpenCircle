//! The owner's three game screens (`tests/fixtures/translate/games`, his
//! own generated art; frames 2560x1440 as on his screen): a German cutscene
//! with a quest HUD, a French gacha banner, a Spanish settings menu. Their
//! Windows OCR reads (Win10 VM) and PP-OCR reads are recorded; the model
//! outputs below are the real offline ones where a step needs one.

use super::super::pipeline::*;
use super::*;
use crate::features::text_reading::TextBlock;
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
    // the skill tree's level, Ubuntu PP-OCR (Cyrillic reader)
    let level = "ПЛАЗМЕННЫЙ ЩИТ - УРОВЕНЬ З";
    assert_eq!(game_read(level), "ПЛАЗМЕННЫЙ ЩИТ - УРОВЕНЬ 3");
    assert_eq!(game_read("З нами"), "З нами");
    assert_eq!(game_read("Invoquer xl"), "Invoquer ×1");
    assert_eq!(game_read("objet 4* ou"), "objet 4★ ou");
    // the key hint at 1080p (PP-OCR read its "]" as "I")
    let hint = game_read("[VI Questgebiet anzeigen");
    assert_eq!(hint, "[V] Questgebiet anzeigen");
    for same in [
        "Chapter XI",
        "Size XL",
        "xo xo",
        "x",
        "Personnages 4 à",
        "[ii",
        "[VII]",
    ] {
        assert_eq!(game_read(same), same);
    }
    // offline outputs (fr-en, fr-ar; Win10 VM build 3acbcc9 and here)
    let route = crate::features::translate::mt::route("fr", "ar").unwrap();
    let sent = RefCell::new(Vec::new());
    let run = |t: &[String], _: &'static crate::features::translate::mt::Pair| {
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
    // a stat's short name with its numbers too (the corpus' battle HUD)
    for keep in [
        "21ms",
        "Lv. 80",
        "×10",
        "UP!",
        "3240 +",
        "16400/16400",
        "PV 1 240/1 500",
        "PM 80/120",
    ] {
        assert!(!needs_translation(keep, "ar"), "{keep}");
    }
    for word in ["OK", "PM", "B2B sales grew in 2024"] {
        assert!(needs_translation(word, "ar"), "{word}");
    }
}

#[test]
fn a_number_in_groups_of_three_is_kept_as_written() {
    // es-ar wrote "- 3.482" for the battle's "-3 482"
    let route = crate::features::translate::mt::route("es", "en").unwrap();
    let sent = RefCell::new(Vec::new());
    let run = |t: &[String], _: &'static crate::features::translate::mt::Pair| {
        sent.borrow_mut().extend(t.iter().cloned());
        Ok(t.iter()
            .map(|s| match s.as_str() {
                "-3482 ¡Golpe crítico!" => "-3482 Critical hit!".to_string(),
                // its unit too (pt-en-ar dropped "XP": "1 200 مهمة")
                "Recompensa: 500 monedas, 1200" => "Reward: 500 coins, 1200".to_string(),
                s => s.to_string(),
            })
            .collect())
    };
    let src: Vec<String> = [
        "-3 482 ¡Golpe crítico!",
        "Recompensa: 500 monedas, 1 200 XP",
    ]
    .map(String::from)
    .into();
    let out = through(&src, &route, "en", run).unwrap();
    assert_eq!(out, ["-3 482 Critical hit!", "Reward: 500 coins, 1 200 XP"]);
    assert!(
        !sent
            .borrow()
            .iter()
            .any(|s| s.contains("3 482") || s.contains("XP")),
        "{:?}",
        sent.borrow()
    );
}

#[test]
fn key_hints_names_and_quotes_stay_as_the_source_has_them() {
    let route = crate::features::translate::mt::route("de", "ar").unwrap();
    // de-en then en-ar as the models answered (Win10 VM, the owner's run)
    let run = |t: &[String], p: &'static crate::features::translate::mt::Pair| {
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
    // a speaker's name in quotes is a name too (fr-ar: "كاميل")
    assert!(spelled_out("“Camille”", "\"كاميل\"", "ar"));
    assert!(!spelled_out("Brücke der Dämmerung", "جسر الغسق", "ar"));
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
    let route = crate::features::translate::mt::route("fr", "en").unwrap();
    let run = |t: &[String], _: &'static crate::features::translate::mt::Pair| {
        Ok(t.iter()
            .map(|s| {
                match s.as_str() {
                    "Chevalier du Givre Éternel" => "Knight of the Eternal Givre",
                    "Chevalier du givre éternel" => "Knight of eternal frost",
                    "Le chevalier Aldric" => "The knight Aldric",
                    "Le chevalier aldric" => "The knight aldric",
                    // a word English has too ("dragon") is no name
                    "Le Réveil du Dragon de Givre" => "The Revival of the Dragon of Givre",
                    "Le réveil du dragon de givre" => "The waking up of the frost dragon",
                    s => s,
                }
                .to_string()
            })
            .collect())
    };
    let src: Vec<String> = [
        "Chevalier du Givre Éternel",
        "Le chevalier Aldric",
        "Le Réveil du Dragon de Givre",
    ]
    .map(String::from)
    .into();
    let out = through(&src, &route, "en", run).unwrap();
    assert_eq!(
        out,
        [
            "Knight of Eternal Frost",
            "The knight Aldric",
            "The Waking up of the Frost Dragon"
        ]
    );
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

#[test]
fn a_line_both_readers_read_is_kept_once() {
    // the battle menu (Win10 VM, 1080p): Windows OCR's box ~20 px above
    // PP-OCR's for the same serif item; both were kept and both drawn
    let tb = |t: &str, b: [f32; 4], confidence: f32| TextBlock {
        bounds: b,
        text: t.into(),
        confidence,
    };
    let win = vec![tb("• Atacar", [68.0, 719.0, 141.0, 39.0], 1.0)];
    let pp = vec![tb("Atacar", [98.0, 743.0, 117.0, 41.0], 0.99)];
    let got = reread(win, pp);
    assert_eq!(got.len(), 1, "{got:?}");
}

#[test]
fn a_reading_pp_ocr_is_sure_of_replaces_a_quiet_misread() {
    // the corpus (Win10 VM): Windows OCR misread game fonts with no sign of
    // it; PP-OCR read them surely. Its star stays, two items stay two.
    let tb = |t: &str, x: f32, confidence: f32| TextBlock {
        bounds: [x, 100.0, 300.0, 40.0],
        text: t.into(),
        confidence,
    };
    for (win, pp, want) in [
        (
            "Sammter der Sterne —",
            "Sammler der Sterne -",
            "Sammler der Sterne -",
        ),
        ("0:49.172", "0:49.112", "0:49.112"),
        ("Meil/eur tour", "Meilleur tour", "Meilleur tour"),
        ("G61em Ancestral", "Gólem Ancestral", "Gólem Ancestral"),
        (
            "garantit au moins un objet 4* ou",
            "garantit au moins un objet 4 ou",
            "garantit au moins un objet 4* ou",
        ),
        ("Ausrüsten Verkaufen", "Verkaufen", "Ausrüsten Verkaufen"),
    ] {
        let got = reread(vec![tb(win, 0.0, 1.0)], vec![tb(pp, 0.0, 0.98)]);
        assert_eq!(got[0].text, want);
    }
    // not sure enough: Windows OCR's stays ("PLAYER", PP-OCR "YER" at 0.8)
    let got = reread(vec![tb("PLAYER", 0.0, 1.0)], vec![tb("YER", 0.0, 0.8)]);
    assert_eq!(got[0].text, "PLAYER");
}
