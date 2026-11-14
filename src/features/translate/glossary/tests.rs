use super::*;

fn term(text: &str, tr: &[(&str, &str)]) -> Term {
    Term {
        text: text.into(),
        translations: tr
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        case: false,
    }
}

fn game() -> Glossary {
    Glossary {
        name: "Ashen Tides".into(),
        windows: vec!["Ashen Tides".into()],
        terms: vec![
            term("Vyrnhold", &[]),
            term("Emberglass", &[("ar", "زجاج الجمر"), ("fr", "Verbraise")]),
            term("Ash Warden", &[("ar", "حارس الرماد")]),
            term("Ash", &[("ar", "رماد")]),
        ],
    }
}

#[test]
fn matches_whole_words_case_insensitive_longest_first() {
    let g = game();
    let h = g.hits("the ASH WARDEN of vyrnhold, not Vyrnholds or ashes");
    let found: Vec<&str> = h.iter().map(|h| h.term.text.as_str()).collect();
    // "Ash Warden" beats "Ash"; the suffixed words are not whole words
    assert_eq!(found, ["Ash Warden", "Vyrnhold"]);
    // a Latin name stands alone next to Japanese (no spaces there)
    assert_eq!(g.hits("Vyrnholdに村で話してください").len(), 1);
    // accents fold; case mode is exact
    let mut c = term("Élan", &[]);
    let g2 = Glossary {
        terms: vec![c.clone()],
        ..Default::default()
    };
    assert_eq!(g2.hits("un elan vif").len(), 1);
    c.case = true;
    let g3 = Glossary {
        terms: vec![c],
        ..Default::default()
    };
    assert_eq!(g3.hits("un élan vif").len(), 0);
}

#[test]
fn a_line_that_is_a_term_skips_the_model() {
    let g = game();
    assert_eq!(g.whole("Emberglass", "ar").as_deref(), Some("زجاج الجمر"));
    assert_eq!(g.whole(" vyrnhold ", "ar").as_deref(), Some("vyrnhold"));
    // no fixed translation for German: kept
    assert_eq!(g.whole("Emberglass", "de").as_deref(), Some("Emberglass"));
    assert_eq!(g.whole("Emberglass shards", "ar"), None);
}

#[test]
fn placeholders_survive_a_model_and_come_back_as_the_term() {
    let g = game();
    let a = g.apply("The Ash Warden guards Vyrnhold and 2 Emberglass", "ar");
    assert!(!a.is_empty());
    // numbers not in the line: the line's own "2" is left alone
    assert_eq!(a.text, "The 701 guards 702 and 2 703");
    assert_eq!(
        restore(&a.text, &a).unwrap(),
        "The حارس الرماد guards Vyrnhold and 2 زجاج الجمر"
    );
    // a model into Arabic may write the numbers in Arabic-Indic digits
    let arabic = a.text.replace("703", "٧٠٣");
    assert!(restore(&arabic, &a).unwrap().ends_with("زجاج الجمر"));
}

#[test]
fn a_lost_or_doubled_placeholder_falls_back_to_the_plain_line() {
    let g = game();
    let a = g.apply("Take Emberglass to Vyrnhold", "ar");
    assert_eq!(a.text, "Take 701 to 702");
    assert_eq!(restore(&a.text.replace("702", ""), &a), None);
    assert_eq!(restore(&format!("{} 701", a.text), &a), None);
    // plain output that kept the names: they are fixed up; else it stays
    assert_eq!(
        a.fix("خذ Emberglass إلى Vyrnhold"),
        "خذ زجاج الجمر إلى Vyrnhold"
    );
    assert_eq!(a.fix("خذ الزجاج إلى القلعة"), "خذ الزجاج إلى القلعة");
}

#[test]
fn a_line_without_terms_is_untouched() {
    let a = game().apply("Nothing here", "ar");
    assert!(a.is_empty());
    assert_eq!(a.text, "Nothing here");
    assert_eq!(restore("anything", &a).as_deref(), Some("anything"));
}

#[test]
fn the_cache_stamp_changes_only_for_lines_holding_the_edited_term() {
    let mut g = game();
    let (with, without) = ("Take Emberglass home", "Take the key home");
    let (s1, n1) = (g.stamp(with, "ar"), g.stamp(without, "ar"));
    assert!(!s1.is_empty() && n1.is_empty());
    g.terms[1].translations.insert("ar".into(), "جمر".into());
    assert_ne!(g.stamp(with, "ar"), s1);
    assert_eq!(g.stamp(without, "ar"), n1);
    // another target: its own stamp
    assert_ne!(g.stamp(with, "fr"), g.stamp(with, "ar"));
}

#[test]
fn ai_gets_only_the_terms_of_its_batch_softly() {
    let g = game();
    let p = g.ai_terms(&["Go to Vyrnhold".into(), "Emberglass!".into()], "ar");
    assert_eq!(
        p,
        "Glossary, use exactly: Vyrnhold -> keep as is; Emberglass -> زجاج الجمر"
    );
    assert_eq!(g.ai_terms(&["Hello".into()], "ar"), "");
    // a missing fixed translation is reported, never forced
    assert_eq!(g.missed("Emberglass", "جمر", "ar"), ["Emberglass"]);
    assert!(g.missed("Emberglass", "زجاج الجمر هنا", "ar").is_empty());
}

#[test]
fn a_file_round_trips_and_import_is_validated() {
    let g = game();
    assert_eq!(store::parse(&store::render(&g)).unwrap(), g);
    // unknown keys and language codes are ignored, an empty translation = keep
    let t = "name = \"X\"\nnote = 1\n[[term]]\ntext = \"A\"\nxx = \"b\"\nar = \"\"\nfoo = 3\n";
    assert_eq!(store::parse(t).unwrap().terms, vec![term("A", &[])]);
    assert_eq!(
        store::parse("[[term]]\ntext='A'"),
        Err(store::Error::NoName)
    );
    assert!(matches!(
        store::parse("name = ["),
        Err(store::Error::Bad(_))
    ));
    let big = format!("name = \"X\"\n{}", "#".repeat(store::MAX_BYTES));
    assert_eq!(store::parse(&big), Err(store::Error::TooBig));
    let many = format!(
        "name = \"X\"\n{}",
        "[[term]]\ntext = \"a\"\n".repeat(store::MAX_TERMS + 1)
    );
    assert_eq!(store::parse(&many), Err(store::Error::TooMany));
    let long = format!("name = \"X\"\n[[term]]\ntext = \"{}\"", "a".repeat(201));
    assert_eq!(store::parse(&long), Err(store::Error::TermTooLong));
}

#[test]
fn files_save_load_pick_by_window_and_stay_in_the_folder() {
    let d = std::env::temp_dir().join(format!("oc-gloss-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let g = game();
    let s = store::free_slug(&d, &g.name);
    assert_eq!(s, "ashen-tides");
    store::save(&d, &s, &g).unwrap();
    assert_eq!(store::free_slug(&d, &g.name), "ashen-tides-2");
    assert_eq!(store::load(&d, &s), Some(g.clone()));
    assert_eq!(store::load(&d, "../ashen-tides"), None);
    let other = Glossary {
        name: "Other".into(),
        ..Default::default()
    };
    store::save(&d, "other", &other).unwrap();
    let all = store::load_all(&d);
    assert_eq!(all.len(), 2);
    // the title picks; no title (Wayland) or no match keeps the user's pick
    assert_eq!(
        store::for_window(&all, Some("ASHEN TIDES - v1.2")),
        Some("ashen-tides")
    );
    assert_eq!(store::for_window(&all, None), None);
    assert_eq!(store::active(&d, "other", None).unwrap().name, "Other");
    assert_eq!(
        store::active(&d, "other", Some("Ashen Tides"))
            .unwrap()
            .name,
        "Ashen Tides"
    );
    assert_eq!(store::active(&d, "", Some("Notepad")), None);
    store::delete(&d, "other");
    assert_eq!(store::load_all(&d).len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}
