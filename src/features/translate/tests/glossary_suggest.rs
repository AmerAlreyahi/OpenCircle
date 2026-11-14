//! Name suggestions on the game corpus (13 screens, one game each): what the
//! extractor finds on each screen. A name must show on 3 screens of one game
//! to be suggested, which these single screens can't reach, so this scores the
//! per-screen candidates: precision = candidates that are really names (the
//! corpus' kept elements). Run with `--nocapture` for the list.

use super::corpus::screens;
use crate::features::translate::glossary::suggest::candidates;

/// Names the corpus' screens really hold: its kept Latin words, and two
/// names inside sentences it doesn't mark.
const NAMES: [&str; 8] = [
    "Lyra", "Camille", "Elena", "Marco", "Nova", "Kaito", "Aldric", "Irene",
];

#[test]
fn the_game_corpus_candidates_are_names() {
    let (mut found, mut right) = (Vec::new(), 0);
    for (screen, s) in screens() {
        let texts: Vec<String> = s.elements.iter().map(|e| e.t.clone()).collect();
        for n in candidates(&texts) {
            let good = NAMES.contains(&n.as_str());
            right += good as usize;
            found.push(format!(
                "{screen}: {n} ({})",
                if good { "name" } else { "FALSE" }
            ));
        }
    }
    eprintln!(
        "corpus suggestions: {found:#?}\nprecision {right}/{}, names found {right}/{}",
        found.len(),
        NAMES.len()
    );
    // (quiet beats complete: a name plate or a name in a sentence counts, a
    // name in a table of numbers does not)
    assert!(
        right * 10 >= found.len() * 7,
        "precision under 70 %: {found:?}"
    );
    assert!(right >= 4, "fewer names found than before: {found:?}");
}
