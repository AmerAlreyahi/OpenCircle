use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

fn lines(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

/// A fresh empty folder.
fn dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "oc-names-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn glossary() -> Glossary {
    Glossary {
        name: "A".into(),
        ..Glossary::default()
    }
}

/// A screen of dialogue naming `who`, a different one each `n`.
fn screen_of(who: &str, n: usize) -> Vec<String> {
    lines(&[
        &format!("We must reach {who} before the tide is rising, page {n}."),
        &format!("the road is long, step {n}"),
    ])
}

#[test]
fn a_name_on_three_different_screens_is_suggested() {
    let (d, g) = (dir(), glossary());
    for n in 0..2 {
        observe_in(&d, "a", &g, &screen_of("Vyrnhold", n));
    }
    assert!(
        pending(&d, "a", &g).is_empty(),
        "two screens are not enough"
    );
    observe_in(&d, "a", &g, &screen_of("Vyrnhold", 2));
    assert_eq!(pending(&d, "a", &g), ["Vyrnhold"]);
}

#[test]
fn the_same_screen_counts_once() {
    let (d, g) = (dir(), glossary());
    for _ in 0..5 {
        observe_in(&d, "a", &g, &screen_of("Vyrnhold", 1));
    }
    assert!(pending(&d, "a", &g).is_empty());
}

#[test]
fn speaker_labels_count_and_sentence_starts_do_not() {
    let l = lines(&[
        "Kaelith: we must go now",
        "then we go to Vyrnhold.",
        "Vyrnhold is far",
        "Elara",
        "Come back before dark, my friend",
    ]);
    assert_eq!(candidates(&l), ["Kaelith", "Vyrnhold", "Elara"]);
    // a button list, a reward line and a name plate with nothing after it
    let l = lines(&["Attack", "Items", "Reward: 500 coins", "Kaelith"]);
    assert!(candidates(&l).is_empty());
}

#[test]
fn labels_caps_digits_and_common_words_are_not_names() {
    // a Title Case line, CAPS, a word with a digit, menu words, a common word
    let l = lines(&[
        "Start New Game",
        "go to the SHOP now",
        "meet Ex2 and the Settings or The end",
        "Audio",
        "Controles",
    ]);
    assert_eq!(candidates(&l), Vec::<String>::new());
}

#[test]
fn german_nouns_are_not_names_only_labels() {
    let l = lines(&[
        "Lyra",
        "Wir müssen die Brücke vor Einbruch der Nacht erreichen und das Tor",
    ]);
    assert_eq!(candidates(&l), ["Lyra"]);
}

#[test]
fn a_word_also_seen_in_lower_case_is_a_common_word() {
    let (d, g) = (dir(), glossary());
    for n in 0..2 {
        observe_in(&d, "a", &g, &screen_of("Lantern", n));
    }
    observe_in(&d, "a", &g, &lines(&["light the lantern now please"]));
    observe_in(&d, "a", &g, &screen_of("Lantern", 2));
    assert!(pending(&d, "a", &g).is_empty());
}

#[test]
fn a_known_or_rejected_name_is_never_suggested() {
    let (d, mut g) = (dir(), glossary());
    g.terms.push(Term {
        text: "vyrnhold".into(),
        ..Term::default()
    });
    for n in 0..3 {
        observe_in(&d, "a", &g, &screen_of("Vyrnhold", n));
        observe_in(&d, "a", &g, &screen_of("Kaelith", n));
    }
    assert_eq!(pending(&d, "a", &g), ["Kaelith"]);
    // rejecting is written into the glossary file; the name never comes back
    store::save(&d, "a", &g).unwrap();
    reject(&d, "a", "Kaelith").unwrap();
    let g = store::load(&d, "a").unwrap();
    assert_eq!(g.rejected, ["kaelith"]);
    assert!(pending(&d, "a", &g).is_empty());
    for n in 3..9 {
        observe_in(&d, "a", &g, &screen_of("Kaelith", n));
    }
    assert!(pending(&d, "a", &g).is_empty());
}

#[test]
fn each_game_counts_for_itself() {
    let (d, g) = (dir(), glossary());
    for (game, n) in [("a", 0), ("b", 1), ("a", 2), ("b", 3)] {
        observe_in(&d, game, &g, &screen_of("Vyrnhold", n));
    }
    assert!(pending(&d, "a", &g).is_empty());
    observe_in(&d, "a", &g, &screen_of("Vyrnhold", 4));
    assert_eq!(pending(&d, "a", &g), ["Vyrnhold"]);
    assert!(pending(&d, "b", &g).is_empty());
    // a deleted glossary takes its counts along
    forget(&d, "a");
    assert!(pending(&d, "a", &g).is_empty());
}

/// Distinct letters-only names: Zaaa, Zbaa, ...
fn name(i: usize) -> String {
    let l = |k: usize| (b'a' + (k % 26) as u8) as char;
    format!("Z{}{}{}", l(i), l(i / 26), l(i / 676))
}

#[test]
fn the_candidate_store_is_capped() {
    let (d, g) = (dir(), glossary());
    let l: Vec<String> = (0..MAX_CANDIDATES + 50)
        .map(|i| format!("we met {} there", name(i)))
        .collect();
    observe_in(&d, "a", &g, &l);
    assert_eq!(read(&d, "a").seen.len(), MAX_CANDIDATES);
}

#[test]
fn the_rejected_list_is_capped_and_kept_in_the_file() {
    let (d, g) = (dir(), glossary());
    store::save(&d, "a", &g).unwrap();
    for i in 0..MAX_REJECTED + 5 {
        reject(&d, "a", &name(i)).unwrap();
    }
    let g = store::load(&d, "a").unwrap();
    assert_eq!(g.rejected.len(), MAX_REJECTED);
    // the newest are kept
    assert!(g.rejected.contains(&Term::key_of(&name(MAX_REJECTED + 4))));
    assert!(!g.rejected.contains(&Term::key_of(&name(0))));
}

#[test]
fn old_glossary_files_have_no_rejections_and_new_ones_round_trip() {
    let g = store::parse("name = \"A\"\n[[term]]\ntext = \"X\"\n").unwrap();
    assert!(g.rejected.is_empty());
    let mut g = g;
    g.rejected = vec!["kaelith".into()];
    let back = store::parse(&store::render(&g)).unwrap();
    assert_eq!(back.rejected, ["kaelith"]);
}
