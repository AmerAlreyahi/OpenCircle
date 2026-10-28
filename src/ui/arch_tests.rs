//! The UI's dependency and style rules as source scans (ARCHITECTURE.md, the
//! UI plan's "How to enforce them"): screens compose kit parts and never style;
//! the kit depends only on itself; platform (the OS) imports none of the UI.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, as (path relative to `src/ui`, source).
fn sources(dir: &str) -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let root = Path::new("src/ui");
    let mut files = Vec::new();
    walk(&root.join(dir), &mut files);
    let mut out: Vec<_> = files
        .into_iter()
        .map(|p| {
            let name = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect();
    out.sort();
    out
}

/// Code lines only (whole-line comments skipped).
fn code(src: &str) -> impl Iterator<Item = &str> {
    src.lines().filter(|l| !l.trim_start().starts_with("//"))
}

/// A style written in place: a colour, border, background, shadow, style
/// closure, weight, or a literal layout number.
fn styles(line: &str) -> bool {
    const WORDS: [&str; 10] = [
        "Color::",
        "Color {",
        "from_rgb",
        "Border {",
        "Border::",
        "Background::",
        "Shadow",
        ".style(",
        ".rounded(",
        "Weight::",
    ];
    WORDS.iter().any(|w| line.contains(w))
        || [".padding(", ".spacing(", ".size(", ".width(", ".height("]
            .iter()
            .any(|call| {
                line.match_indices(call).any(|(i, _)| {
                    let rest = line[i + call.len()..].trim_start();
                    rest.starts_with(|c: char| c.is_ascii_digit())
                })
            })
}

/// Not screens: the kit, the palettes and components the kit replaces (step
/// 5 / 23), OS glue (platform/), state and logic (app/), and tests.
const NOT_SCREENS: [&str; 7] = [
    "arch_tests.rs",
    "frame_times.rs",
    "glass.rs",
    "sapphire.rs",
    "theme.rs",
    "widgets.rs",
    "windows.rs",
];

/// Screen files that still style in place. A ratchet: a port removes its file
/// here, and the list only shrinks.
const LEGACY: &[&str] = &[
    "about.rs",
    "bar.rs",
    "border.rs",
    "edit.rs",
    "live_layer.rs",
    "onboarding.rs",
    "overlay.rs",
    "panel.rs",
    "settings.rs",
    "setup.rs",
    "snip.rs",
    "song.rs",
    "translate.rs",
    "update_card.rs",
];

#[test]
fn screens_never_style() {
    let styled: Vec<String> = sources("")
        .into_iter()
        .filter(|(name, _)| {
            !name.starts_with("kit/") && !name.starts_with("platform/") && !name.starts_with("app/")
        })
        .filter(|(name, _)| !NOT_SCREENS.contains(&name.as_str()))
        .filter(|(_, src)| code(src).any(styles))
        .map(|(name, _)| name)
        .collect();
    let new: Vec<_> = styled
        .iter()
        .filter(|n| !LEGACY.contains(&n.as_str()))
        .collect();
    assert!(
        new.is_empty(),
        "screens styling in place (use kit parts and tokens): {new:?}"
    );
    let clean: Vec<_> = LEGACY
        .iter()
        .filter(|n| !styled.iter().any(|s| s == *n))
        .collect();
    assert!(
        clean.is_empty(),
        "no longer styling, remove from LEGACY: {clean:?}"
    );
}

#[test]
fn kit_depends_only_on_tokens() {
    // ponytail: the test harness renders every screen (legacy ones too) on the
    // old palette's window colour and writes PNGs; exempt until theme.rs goes (step 38)
    for (name, src) in sources("kit") {
        if name.starts_with("kit/testing/") {
            continue;
        }
        for line in code(&src) {
            let outside = line
                .match_indices("crate::")
                .any(|(i, _)| !line[i..].starts_with("crate::ui::kit"));
            assert!(!outside, "{name} reaches outside the kit: {}", line.trim());
        }
    }
}

/// The OS layer imports no kit part, no screen and nothing of the app: a colour
/// or a label comes in as a parameter. The one exception is `kit::motion`, the
/// fade's clock and pace (no colours, no widgets), which every fade is timed by.
#[test]
fn platform_is_isolated() {
    for (name, src) in sources("platform") {
        for line in code(&src) {
            let ui = line
                .match_indices("crate::ui::")
                .any(|(i, _)| !line[i..].starts_with("crate::ui::kit::motion"));
            assert!(
                !ui && !line.contains("super::super"),
                "{name} reaches into the UI (take it as a parameter): {}",
                line.trim()
            );
        }
    }
}

/// Not components: the kit's data and clock.
const KIT_DATA: [&str; 5] = ["looks", "mod", "motion", "themes", "tokens"];

/// Each kit component has its gallery sheet (`kit/testing/gallery.rs`) and
/// its golden images in both themes (`tests/fixtures/kit/`).
#[test]
fn every_kit_component_has_a_gallery_sheet() {
    let gallery = std::fs::read_to_string("src/ui/kit/testing/gallery.rs").unwrap();
    let components: Vec<String> = sources("kit")
        .into_iter()
        .filter_map(|(name, _)| {
            let file = name.strip_prefix("kit/")?.strip_suffix(".rs")?;
            (!file.contains('/') && !KIT_DATA.contains(&file)).then(|| file.to_string())
        })
        .collect();
    assert!(components.contains(&"button".to_string()), "{components:?}");
    for c in components {
        assert!(
            gallery.contains(&format!("sheet(\"{c}\"")),
            "{c}: add its sheet to kit/testing/gallery.rs"
        );
        for theme in ["sapphire", "daylight"] {
            let png = format!("tests/fixtures/kit/{c}-{theme}@1x.png");
            assert!(
                std::path::Path::new(&png).exists(),
                "{png} missing (OPENCIRCLE_UPDATE_GOLDEN=1)"
            );
        }
    }
}

/// The scans see what they forbid.
#[test]
fn the_style_scan_finds_styles() {
    for line in [
        "text(s).color(Color::WHITE)",
        "container(x).padding(12)",
        ".spacing( 4)",
        "c.style(|_| s)",
    ] {
        assert!(styles(line), "{line}");
    }
    for line in [
        ".padding(PAGE)",
        ".width(Fill)",
        "let colour = theme.c.text;",
    ] {
        assert!(!styles(line), "{line}");
    }
}
