//! The UI's dependency and style rules as source scans (docs/ARCHITECTURE.md, the
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
    // (an image made from pixels, `Handle::from_rgba`, is no colour)
    let line = &line.replace("Handle::from_rgba", "Handle::");
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

/// Not screens: the kit, the config-to-kit glue (theme.rs), OS glue
/// (platform/, windows.rs), state and logic (app/), and tests.
const NOT_SCREENS: [&str; 7] = [
    "arch_tests.rs",
    "frame_times.rs",
    "screens/ai_chat/tests.rs",
    "screens/translate/tests.rs",
    "screens/settings/tests.rs",
    "theme.rs",
    "windows.rs",
];

/// Screen files that still style in place. A ratchet: a port removes its file
/// here, and the list only shrinks.
const LEGACY: &[&str] = &[];

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

/// No `iced::Shadow` with a blur anywhere: tiny-skia draws it outside the
/// part being redrawn, so it piles up into black blocks (and a floating
/// window can't show one: no alpha there).
#[test]
fn no_drawn_shadows() {
    fn scan(dir: &Path, found: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                scan(&p, found);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let src = std::fs::read_to_string(&p).unwrap();
                let n = src.matches(concat!("blur", "_radius:")).count();
                if n > 0 {
                    found.push(format!("{} ({n})", p.display()));
                }
            }
        }
    }
    let mut found = Vec::new();
    scan(Path::new("src"), &mut found);
    assert!(found.is_empty(), "drawn shadows in {found:?}");
}

#[test]
fn kit_depends_only_on_tokens() {
    // the test harness (goldens, gallery) writes PNGs with `actions::write_png`
    for (name, src) in sources("kit") {
        if name.starts_with("kit/testing/") {
            continue;
        }
        for line in code(&src) {
            let outside = line.match_indices("crate::").any(|(i, _)| {
                // (`dir_row!` is the kit's own macro, exported at the crate root)
                !line[i..].starts_with("crate::ui::kit")
                    && !line[i..].starts_with("crate::dir_row!")
                    // (`kit::dir` is the one bridge to the app's language: its direction)
                    && !(name == "kit/dir.rs" && line[i..].starts_with("crate::app_language::rtl"))
            });
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
const KIT_DATA: [&str; 6] = ["dir", "looks", "mod", "motion", "themes", "tokens"];

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
        "image::Handle::from_rgba(w, h, px)",
    ] {
        assert!(!styles(line), "{line}");
    }
}

/// The backend (everything in `src` outside `ui/` and `main.rs`) never imports the
/// UI; `main` wires the two together. Test code may (it looks at pictures).
#[test]
fn backend_never_imports_ui() {
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
    let root = Path::new("src");
    let mut files = Vec::new();
    walk(root, &mut files);
    for p in files {
        let name = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if name.starts_with("ui/") || name == "main.rs" || name.ends_with("tests.rs") {
            continue;
        }
        // LF everywhere (Windows checks out CRLF), so both platforms scan the same code
        let src = std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n");
        // the production part: up to an INLINE test module (`mod x {`); an
        // out-of-line `mod tests;` ends nothing, the rest of the file is production
        let end = src
            .match_indices("#[cfg(test)]\nmod ")
            .map(|(i, m)| i + m.len())
            .find(|&i| {
                src[i..]
                    .lines()
                    .next()
                    .is_some_and(|l| l.trim_end().ends_with('{'))
            })
            .unwrap_or(src.len());
        let prod = &src[..end];
        let hits: Vec<&str> = code(prod).filter(|l| l.contains("crate::ui")).collect();
        assert!(
            hits.is_empty(),
            "{name} imports the UI (the backend never does): {}",
            hits[0].trim()
        );
    }
}

/// One scrollbar and one dropdown (owner, 2026-10-28: one design per object):
/// only `kit/scroll.rs` makes an iced scroll area (every other one is
/// `kit::scroll`), and nothing uses iced's pick list or combo box (their
/// menus bring iced's own scrollbar): `kit::dropdown` is built from kit parts.
#[test]
fn one_scrollbar_and_one_dropdown() {
    const IRON: [&str; 5] = [
        "scrollable(",
        "scrollable::default",
        "Scrollable::new",
        "pick_list",
        "combo_box",
    ];
    let mut found = Vec::new();
    for (name, src) in sources("") {
        if name == "kit/scroll.rs" || name == "arch_tests.rs" {
            continue;
        }
        for line in code(&src) {
            // (an `Operation`'s `fn scrollable` only looks at one)
            if IRON.iter().any(|w| line.contains(w)) && !line.contains("fn scrollable(") {
                found.push(format!("{name}: {}", line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// Our windows are shown by `platform::windows::show` (X11: "always on top"
/// again once mapped, #172). The overlay's hiding (update.rs) leaves full-screen
/// on its way out, not to show it.
#[test]
fn only_the_window_helper_shows_windows() {
    for (name, src) in sources("") {
        if ["platform/windows.rs", "app/update.rs", "arch_tests.rs"].contains(&name.as_str()) {
            continue;
        }
        for l in code(&src) {
            assert!(
                !(l.contains("set_mode(") && (l.contains("Windowed") || l.contains("Fullscreen"))),
                "{name}: show a window with windows::show: {l}"
            );
        }
    }
}

/// Every runtime resize of our windows goes through `platform::monitor::resize`
/// (Wayland sends no `Resized` for a program's resize: one place to handle it).
#[test]
fn only_the_monitor_helper_calls_window_resize() {
    for (name, src) in sources("") {
        if name == "platform/monitor.rs" || name == "arch_tests.rs" {
            continue;
        }
        for l in code(&src) {
            assert!(
                !l.contains("window::resize("),
                "{name}: use platform::monitor::resize, not window::resize: {l}"
            );
        }
    }
}

/// Every text the app draws names a bundled face through the kit (`kit::text`:
/// its roles, `ui_font`, `weight`, `mono`): nothing asks for the renderer's
/// default or a generic family, which the machine's fonts answer (or none: a
/// title drawn as nothing). Outside the kit no screen builds iced's text
/// widgets itself. Test code is not scanned. The one generic family left is
/// the Snip text tool's "Serif" (the user's pick; no serif is bundled).
#[test]
fn every_text_names_a_bundled_face() {
    const GENERIC: [&str; 7] = [
        "Font::DEFAULT",
        "Font::default()",
        "Font::MONOSPACE",
        "Family::SansSerif",
        "Family::Monospace",
        "Family::Cursive",
        "Family::Fantasy",
    ];
    const WIDGETS: [&str; 2] = ["widget::text(", "rich_text("];
    let snip: Vec<(String, String)> = ["src/snip/annotate/text.rs", "src/snip/annotate/style.rs"]
        .iter()
        .map(|p| (p.to_string(), std::fs::read_to_string(p).unwrap()))
        .collect();
    let mut found = Vec::new();
    for (name, src) in sources("").into_iter().chain(snip) {
        let test_only = name.starts_with("kit/testing/") || name.ends_with("tests.rs");
        if name == "arch_tests.rs" || name == "frame_times.rs" || test_only {
            continue;
        }
        let src = src.replace("\r\n", "\n");
        let shipped = src.split("#[cfg(test)]").next().unwrap_or("");
        let lines: Vec<&str> = code(shipped).collect();
        for (i, line) in lines.iter().enumerate() {
            let generic = GENERIC.iter().any(|w| line.contains(w));
            let widget = !name.starts_with("kit/") && WIDGETS.iter().any(|w| line.contains(w));
            // a text widget without a face: drawn in the renderer's default
            let faceless = name != "kit/text.rs"
                && bare_text_call(line)
                && !lines[i..(i + 6).min(lines.len())]
                    .iter()
                    .any(|l| l.contains(".font("));
            if generic || widget || faceless {
                found.push(format!("{name}: {}", line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// `line` calls iced's `text(..)` itself (not the kit's `text(Role::..)`, a
/// method, or a function named `..text`).
fn bare_text_call(line: &str) -> bool {
    !line.contains("fn ")
        && line.match_indices("text(").any(|(i, _)| {
            let role = line[i + 5..].starts_with("Role") || line[i + 5..].starts_with("role");
            let before = line[..i].chars().next_back();
            !role && !before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '.'))
        })
}
