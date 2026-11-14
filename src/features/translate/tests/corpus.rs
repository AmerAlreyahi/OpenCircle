//! The game corpus: 13 game screens made for these tests (the owner's own
//! generated art, `tests/fixtures/translate/games`) and, per screen, how
//! one-shot translate must handle each element (`corpus.json`): translate it
//! (whole, carrying its numbers, multipliers, stars, key hints exactly) or
//! keep it exactly (numbers, timers, names). [`score`] counts the elements
//! handled right and the blocks drawn outside their own box (over the art).
//! The real runs (Windows OCR + offline models, Win10 VM) go through
//! `real_matrix`; `corpus_scores` scores what it wrote.

use super::super::lang::{self, Script};
use serde::Deserialize;
use std::collections::BTreeMap;

const SPEC: &str = include_str!("../../../../tests/fixtures/translate/games/corpus.json");

#[derive(Deserialize)]
pub(super) struct Screen {
    pub src: String,
    pub elements: Vec<Element>,
}

#[derive(Deserialize)]
pub(super) struct Element {
    pub t: String,
    #[serde(default)]
    pub keep: bool,
    #[serde(default)]
    pub tokens: Vec<String>,
    #[serde(default)]
    pub en: Vec<String>,
    /// Targets that write the source's words as they are (French "Audio").
    #[serde(default)]
    pub same: Vec<String>,
}

/// One block as `real_matrix` writes it: bounds, original, translation ("" =
/// left as it was), the box its drawn text spans (zeros = nothing drawn).
pub(super) type Row = ([i32; 4], String, String, [f32; 4]);

pub(super) fn screens() -> BTreeMap<String, Screen> {
    let mut all: BTreeMap<String, serde_json::Value> = serde_json::from_str(SPEC).unwrap();
    all.remove("_");
    (all.into_iter())
        .map(|(k, v)| (k, serde_json::from_value(v).unwrap()))
        .collect()
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Score {
    pub right: usize,
    pub total: usize,
    /// Elements OCR never read (no block holds them).
    pub missed: usize,
    /// Drawn blocks whose text reaches well past their box (over the art).
    pub outside: usize,
    /// What went wrong, one line per element.
    pub wrong: Vec<String>,
}

/// Letters and digits only, lower case, no accents, Arabic-Indic digits as
/// ASCII: how an element is found in a block.
fn key(s: &str) -> String {
    (s.chars())
        .map(|c| match c {
            '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
            '\u{06F0}'..='\u{06F9}' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
            c => lang::plain(c),
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// `text` with Arabic-Indic digits as ASCII (tokens are compared so).
fn digits(text: &str) -> String {
    (text.chars())
        .map(|c| match c {
            '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
            c => c,
        })
        .collect()
}

/// How `rows` (one screen into `target`) handle the screen's elements.
pub(super) fn score(screen: &Screen, rows: &[Row], target: &str) -> Score {
    let mut s = Score::default();
    for e in &screen.elements {
        s.total += 1;
        let k = key(&e.t);
        // the block that is it, else the blocks holding it, else the blocks
        // it is made of (a wrapped line; most of it)
        let mut mine: Vec<&Row> = rows.iter().filter(|r| key(&r.1) == k).collect();
        if mine.is_empty() {
            mine = rows.iter().filter(|r| key(&r.1).contains(&k)).collect();
        }
        if mine.is_empty() {
            mine = (rows.iter())
                .filter(|r| {
                    let b = key(&r.1);
                    b.chars().count() >= 3 && k.contains(&b)
                })
                .collect();
            let n: usize = mine.iter().map(|r| key(&r.1).chars().count()).sum();
            if n * 5 < k.chars().count() * 4 {
                mine.clear();
            }
        }
        if mine.is_empty() {
            s.missed += 1;
            s.wrong.push(format!("{:?}: not read", e.t));
            continue;
        }
        let out: String = mine
            .iter()
            .map(|r| r.2.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let why = if e.keep {
            let kept = mine
                .iter()
                .all(|r| r.2.is_empty() || key(&r.2).contains(&k));
            (!kept).then(|| format!("{:?} changed: {out:?}", e.t))
        } else {
            judge(e, &mine, &out, &screen.src, target)
        };
        match why {
            Some(w) => s.wrong.push(w),
            None => s.right += 1,
        }
    }
    s.outside = (rows.iter())
        .filter(|r| !r.2.is_empty() && r.3[2] > 0.0)
        .filter(|r| {
            // a label may grow into free space (half its width and a line
            // each way: Arabic to its column's edge), never further
            let [x, y, w, h] = r.0.map(|v| v as f32);
            let (mx, my) = (0.5 * w + h, 0.5 * h.min(60.0));
            let [ix, iy, iw, ih] = r.3;
            ix < x - mx || iy < y - my || ix + iw > x + w + mx || iy + ih > y + h + my
        })
        .count();
    s
}

/// Why a translated element is wrong, or `None`.
fn judge(e: &Element, mine: &[&Row], out: &str, src: &str, target: &str) -> Option<String> {
    let same = |w: &str| e.en.iter().any(|x| key(x) == key(w));
    let theirs = e.same.iter().any(|t| t == target);
    // the same word in the target already ("Audio", "Garage" into English,
    // "Pause" into French)
    if mine.iter().all(|r| r.2.is_empty()) {
        return match (target == "en" && same(&e.t)) || theirs {
            true => None,
            false => Some(format!("{:?} left as it was", e.t)),
        };
    }
    if let Some(r) = mine.iter().find(|r| r.2.is_empty()) {
        return Some(format!("{:?}: part left as it was ({:?})", e.t, r.1));
    }
    let out_d = digits(out);
    // as a whole number: "×1" is not in "×10"
    let has = |t: &str| {
        (out_d.match_indices(t)).any(|(i, _)| {
            let next = out_d[i + t.len()..].chars().next();
            let prev = out_d[..i].chars().next_back();
            !next.is_some_and(|c| c.is_ascii_digit()) && !prev.is_some_and(|c| c.is_ascii_digit())
        })
    };
    if let Some(t) = e.tokens.iter().find(|t| !has(t)) {
        return Some(format!("{:?}: {t:?} lost in {out:?}", e.t));
    }
    let script = lang::find(target).map_or(Script::Latin, |l| l.script);
    if script != Script::Latin {
        let letters: Vec<char> = out.chars().filter(|c| c.is_alphabetic()).collect();
        let kept: usize = (e.tokens.iter())
            .map(|t| t.chars().filter(|c| c.is_alphabetic()).count())
            .sum();
        let own = letters.iter().filter(|c| script.has(**c)).count();
        if own * 2 < letters.len().saturating_sub(kept) {
            return Some(format!("{:?}: not in the target's letters: {out:?}", e.t));
        }
        return None;
    }
    // into a Latin language: not half of the source's words left in it (half
    // translated: "Benvenuto! Oggi carote sono in offerta."); one shared with
    // English ("chances", "dragon") is no sign
    let src_latin = lang::find(src).is_some_and(|l| l.script == Script::Latin);
    if src_latin && lang::find(src) != lang::find(target) && !theirs {
        let words = |s: &str| -> Vec<String> {
            (s.split(|c: char| !c.is_alphabetic()))
                .filter(|w| w.chars().count() >= 5)
                .map(key)
                .collect()
        };
        let tokens: Vec<String> = e.tokens.iter().map(|t| key(t)).collect();
        let theirs: Vec<String> = (words(&e.t).into_iter())
            .filter(|w| !same(w) && !tokens.contains(w))
            .collect();
        let left: Vec<String> = words(out)
            .into_iter()
            .filter(|w| theirs.contains(w))
            .collect();
        if !theirs.is_empty() && left.len() * 2 >= theirs.len() {
            return Some(format!("{:?}: {left:?} left in {out:?}", e.t));
        }
    }
    let low = out.to_lowercase();
    if target == "en" && !e.en.is_empty() && !e.en.iter().any(|w| low.contains(&w.to_lowercase())) {
        return Some(format!("{:?}: {out:?} (want {:?})", e.t, e.en));
    }
    None
}

#[test]
fn the_corpus_is_well_formed() {
    let all = screens();
    assert_eq!(all.len(), 13);
    for (name, s) in &all {
        assert!(lang::find(&s.src).is_some(), "{name}: {}", s.src);
        assert!(!s.elements.is_empty(), "{name}");
        let img = format!(
            "{}/tests/fixtures/translate/games/{name}.jpg",
            env!("CARGO_MANIFEST_DIR")
        );
        assert!(std::path::Path::new(&img).exists(), "{img}");
    }
}

#[test]
fn the_score_counts_what_each_element_needs() {
    let all = screens();
    let gacha = &all["gacha"];
    // drawn inside its box at `y`
    let row = |y: i32, o: &str, t: &str| -> Row {
        let ink = [10.0, y as f32 + 10.0, 80.0, 20.0];
        ([0, y, 100, 40], o.into(), t.into(), ink)
    };
    let rows = vec![
        row(0, "Invoquer ×10", "Summon ×10"),
        // a lost multiplier, the source's word left in, a name kept
        row(50, "Invoquer ×1", "Invoking XIO"),
        row(
            100,
            "Chevalier du Givre Éternel",
            "Knight of the Eternal Givre",
        ),
        ([0, 150, 100, 40], "UP!".into(), String::new(), [0.0; 4]),
        // drawn far past its box, over the art
        (
            [0, 200, 100, 40],
            "ÉVÉNEMENT".into(),
            "EVENT".into(),
            [0.0, 200.0, 400.0, 40.0],
        ),
    ];
    let s = score(gacha, &rows, "en");
    assert_eq!(s.total, gacha.elements.len());
    assert_eq!((s.right, s.outside), (3, 1), "{:#?}", s.wrong);
    assert_eq!(s.missed, s.total - 5);
    assert!(
        s.wrong.iter().any(|w| w.contains("\"×1\" lost")),
        "{:#?}",
        s.wrong
    );
    assert!(
        s.wrong.iter().any(|w| w.contains("Givre Éternel")),
        "{:#?}",
        s.wrong
    );
    // into Arabic: Arabic letters, the counter carried
    let rows = vec![row(0, "Invoquer ×10", "استدعاء ×10")];
    let s = score(gacha, &rows, "ar");
    assert_eq!(s.right, 1, "{:#?}", s.wrong);
    // French writes "Pause" so: left as it was is right there only
    let rows = vec![([0, 0, 100, 40], "Pause".into(), String::new(), [0.0; 4])];
    assert_eq!(score(&all["pause"], &rows, "fr").right, 1);
    assert_eq!(score(&all["pause"], &rows, "ar").right, 0);
}

/// The scores of a `real_matrix` run over the corpus (frames named
/// `c_<screen>.png`): `OC_REAL_OUT=<its out dir> cargo test corpus_scores -- --ignored --nocapture`
#[test]
#[ignore]
fn corpus_scores() {
    let dir = std::path::PathBuf::from(std::env::var("OC_REAL_OUT").unwrap());
    let (mut right, mut total) = (0, 0);
    for (name, screen) in screens() {
        for target in ["en", "ar", "fr"] {
            if lang::find(target) == lang::find(&screen.src) {
                continue;
            }
            let path = dir.join(format!("c_{name}-{target}.json"));
            let Ok(bytes) = std::fs::read(&path) else {
                eprintln!("{name:>9} -> {target}: no run ({})", path.display());
                continue;
            };
            let rows: Vec<Row> = serde_json::from_slice(&bytes).unwrap();
            let s = score(&screen, &rows, target);
            right += s.right;
            total += s.total;
            eprintln!(
                "{name:>9} -> {target}: {:>2}/{:<2} right, {} not read, {} drawn outside its box",
                s.right, s.total, s.missed, s.outside
            );
            for w in &s.wrong {
                eprintln!("              {w}");
            }
        }
    }
    eprintln!("ALL: {right}/{total}");
}
