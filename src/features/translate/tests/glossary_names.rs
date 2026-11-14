//! Glossary placeholder survival with the REAL offline models (GLOSSARY_SPEC.md
//! "How the glossary is applied"; pass line: names consistent in >= 95 % of
//! lines, no drop in the line score). Real runs only:
//! `cargo test --release glossary_names -- --ignored --nocapture`
//! (downloads the pairs on first run). Lines: the 13-image game corpus's
//! sentences, each with an invented name as a speaker label ("Name: line")
//! and, per language, in a sentence ("Talk to Name in the village").

use super::corpus::screens;
use crate::features::text_reading::models_dir;
use crate::features::translate::glossary::{Glossary, Term, restore};
use crate::features::translate::mt::{self, Translator};
use std::collections::BTreeMap;

/// A sentence with the name in it, per source language.
const SENTENCE: [(&str, &str); 8] = [
    ("de", "Sprich mit {} im Dorf."),
    ("fr", "Parle à {} au village."),
    ("es", "Habla con {} en la aldea."),
    ("it", "Parla con {} nel villaggio."),
    ("pt", "Fale com {} na aldeia."),
    ("ru", "Поговори с {} в деревне."),
    ("ja", "{}に村で話しかけてください。"),
    ("ar", "تحدث مع {} في القرية."),
];

fn game() -> Glossary {
    let term = |t: &str, ar: &str| Term {
        text: t.into(),
        translations: (!ar.is_empty())
            .then(|| ("ar".to_string(), ar.to_string()))
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
        case: false,
    };
    Glossary {
        name: "Test".into(),
        windows: Vec::new(),
        terms: vec![
            term("Vyrnhold", ""),
            term("Kaelith", ""),
            term("Emberglass", "زجاج الجمر"),
        ],
    }
}

/// `lines` through every pair of the route, in turn.
fn run(lines: &[String], route: &[&'static mt::Pair]) -> Vec<String> {
    let mut t = lines.to_vec();
    for pair in route {
        let mut tr = Translator::load(pair, &models_dir()).unwrap();
        t = tr.translate(&t).unwrap();
    }
    t
}

#[test]
#[ignore]
fn glossary_names_survive_the_real_models() {
    let g = game();
    let all = screens();
    println!("src -> target | form | lines | plain keeps the name | placeholder back | fallback needed | consistent after");
    let (mut total, mut good) = (0, 0);
    for (src, target) in [
        ("de", "en"),
        ("fr", "en"),
        ("es", "en"),
        ("it", "en"),
        ("pt", "en"),
        ("ru", "en"),
        ("ja", "en"),
        ("de", "ar"),
        ("fr", "ar"),
        ("es", "ar"),
    ] {
        let Some(route) = mt::route(src, target) else {
            println!("{src} -> {target}: no offline route");
            continue;
        };
        for pair in &route {
            pair.ensure(|_, _| {}).unwrap();
        }
        let sentences: Vec<String> = (all.values().filter(|s| s.src == src))
            .flat_map(|s| s.elements.iter())
            .filter(|e| !e.keep && e.t.split_whitespace().count() >= 3)
            .map(|e| e.t.clone())
            .take(12)
            .collect();
        let names = ["Vyrnhold", "Kaelith", "Emberglass"];
        let template = SENTENCE.iter().find(|(l, _)| *l == src).unwrap().1;
        let mut forms: Vec<(&str, Vec<(String, &str)>)> = vec![
            (
                "label",
                (sentences.iter().enumerate())
                    .map(|(i, s)| (format!("{}: {s}", names[i % 3]), names[i % 3]))
                    .collect(),
            ),
            (
                "sentence",
                names.iter().map(|n| (template.replace("{}", n), *n)).collect(),
            ),
        ];
        forms.retain(|(_, l)| !l.is_empty());
        for (form, lines) in forms {
            let src_lines: Vec<String> = lines.iter().map(|(l, _)| l.clone()).collect();
            let plain = run(&src_lines, &route);
            let applied: Vec<_> = src_lines.iter().map(|l| g.apply(l, target)).collect();
            let sent: Vec<String> = applied.iter().map(|a| a.text.clone()).collect();
            let out = run(&sent, &route);
            let n = lines.len();
            let (mut kept, mut back, mut fallback, mut ok) = (0, 0, 0, 0);
            for (i, (_, name)) in lines.iter().enumerate() {
                let fixed = g.terms.iter().find(|t| t.text == *name).and_then(|t| t.fixed(target));
                let want = fixed.unwrap_or(name).to_lowercase();
                kept += plain[i].to_lowercase().contains(&want) as usize;
                let (text, restored) = match restore(&out[i], &applied[i]) {
                    Some(t) => (t, true),
                    None => (applied[i].fix(&plain[i]), false),
                };
                back += restored as usize;
                fallback += !restored as usize;
                let consistent = text.to_lowercase().contains(&want);
                ok += consistent as usize;
                if i < 3 || !consistent {
                    println!("  [{src}->{target} {form}] {} | {} | {text}", src_lines[i], plain[i]);
                }
            }
            total += n;
            good += ok;
            println!("{src} -> {target} | {form} | {n} | {kept} | {back} | {fallback} | {ok}");
        }
    }
    println!("consistent after: {good} of {total} ({:.1} %)", 100.0 * good as f32 / total.max(1) as f32);
}
