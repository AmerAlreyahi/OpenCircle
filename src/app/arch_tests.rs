//! The UI's dependency and style rules as source scans (docs/ARCHITECTURE.md, the
//! UI plan's "How to enforce them"): screens compose kit parts and never style;
//! the kit depends only on itself; platform (the OS) imports none of the UI.

use std::path::{Path, PathBuf};

/// Where the UI lives (paths under `src`): the app shell, the kit and its
/// config glue, the OS's windows, and each feature's screens and App parts.
const UI: &[&str] = &[
    "app",
    "shared/kit",
    "shared/theme.rs",
    "shared/touch.rs",
    "shared/platform/monitor.rs",
    "shared/platform/shape.rs",
    "shared/platform/tray.rs",
    "shared/platform/windows.rs",
];

/// The UI's modules (`UI` as paths): what the backend and the OS layer never name.
const UI_MODULES: &[&str] = &[
    "crate::app",
    "crate::shared::kit",
    "crate::shared::theme",
    "crate::shared::touch",
    "crate::shared::platform::monitor",
    "crate::shared::platform::shape",
    "crate::shared::platform::tray",
    "crate::shared::platform::windows",
];

/// `line` names a module of the UI.
fn names_ui(line: &str) -> bool {
    // a feature's screens and its part of the App
    let feature_ui = line.match_indices("crate::features::").any(|(i, m)| {
        let mut parts = line[i + m.len()..].split("::");
        let feature = parts.next().unwrap_or("");
        let part: String = parts
            .next()
            .unwrap_or("")
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (UI_FEATURES.contains(&feature) && part != "labs") || part == "screens" || part == "app"
    });
    feature_ui
        || UI_MODULES.iter().any(|m| {
            line.match_indices(m).any(|(i, _)| {
                !line[i + m.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
            })
        })
}

/// Features that are all UI (no logic of their own besides their screens).
const UI_FEATURES: &[&str] = &["onboarding", "settings"];

/// `name` (a path under `src`) is a file of the UI.
fn in_ui(name: &str) -> bool {
    let mut parts = name.split('/');
    let feature_ui = parts.next() == Some("features") && {
        let feature = parts.next().unwrap_or("");
        let part = parts.next().unwrap_or("");
        (UI_FEATURES.contains(&feature) && part != "labs.rs") || part == "screens" || part == "app"
    };
    feature_ui
        || UI
            .iter()
            .any(|r| name == *r || name.starts_with(&format!("{r}/")))
}

/// Every `.rs` file of the UI (`dir` = "") or under `src/<dir>`, as (path
/// relative to `src`, source).
fn sources(dir: &str) -> Vec<(String, String)> {
    if !dir.is_empty() {
        return tree(dir);
    }
    let mut out: Vec<_> = UI.iter().flat_map(|root| tree(root)).collect();
    out.extend(tree("features").into_iter().filter(|(name, _)| in_ui(name)));
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
    "app/arch_tests.rs",
    "app/frame_times.rs",
    "features/ask_ai/screens/ai_chat/tests.rs",
    "features/translate/screens/translate/tests.rs",
    "features/settings/tests.rs",
    "shared/theme.rs",
    "app/windows.rs",
];

/// Screen files that still style in place. A ratchet: a port removes its file
/// here, and the list only shrinks.
const LEGACY: &[&str] = &[];

#[test]
fn screens_never_style() {
    let styled: Vec<String> = sources("")
        .into_iter()
        .filter(|(name, _)| {
            !name.starts_with("shared/kit/")
                && !name.starts_with("shared/platform/")
                // the app shell's state and logic (its screens and its start are screens)
                && !(name.starts_with("app/")
                    && !name.starts_with("app/screens/")
                    && name != "app/shell.rs")
                && !(name.starts_with("features/") && name.split('/').nth(2) == Some("app"))
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
    for (name, src) in sources("shared/kit") {
        if name.starts_with("shared/kit/testing/") {
            continue;
        }
        for line in code(&src) {
            let outside = line.match_indices("crate::").any(|(i, _)| {
                // (`dir_row!` is the kit's own macro, exported at the crate root)
                !line[i..].starts_with("crate::shared::kit")
                    && !line[i..].starts_with("crate::dir_row!")
                    // (`kit::dir` is the one bridge to the app's language: its direction)
                    && !(name == "shared/kit/dir.rs" && line[i..].starts_with("crate::shared::app_language::rtl"))
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
    for (name, src) in sources("") {
        if !name.starts_with("shared/platform/") {
            continue;
        }
        for line in code(&src) {
            let line = line
                .replace("crate::shared::kit::motion", "")
                .replace("crate::shared::platform::", "");
            let ui = names_ui(&line);
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
    let gallery = std::fs::read_to_string("src/shared/kit/testing/gallery.rs").unwrap();
    let components: Vec<String> = sources("shared/kit")
        .into_iter()
        .filter_map(|(name, _)| {
            let file = name.strip_prefix("shared/kit/")?.strip_suffix(".rs")?;
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
        if in_ui(&name) || name == "main.rs" || name.ends_with("tests.rs") {
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
        let hits: Vec<&str> = code(prod).filter(|l| names_ui(l)).collect();
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
        if name == "shared/kit/scroll.rs" || name == "app/arch_tests.rs" {
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

/// Keyboard focus is ONE mechanism (`kit::focus`, v0.1.8): every kit control a
/// press reaches is a focus stop, so Tab, Enter / Space and the focus look work the
/// same on all of them. A kit press is made only by `focus::press` (button +
/// stop) or, mouse-only by nature (a drag, a resize edge, a caption button),
/// marked `// mouse only: <why>`; the dropdown is its own stop on
/// `focus::State` / `focus::key` / `focus::shade`.
#[test]
fn every_kit_press_is_a_focus_stop() {
    let mut found = Vec::new();
    for (name, src) in sources("shared/kit") {
        if name == "shared/kit/focus.rs" || name.starts_with("shared/kit/testing/") {
            continue;
        }
        for line in code(&src) {
            let press = line.contains(".on_press(") || line.contains(".on_press_maybe(");
            if press && !line.contains("// mouse only: ") {
                found.push(format!("{name}: {}", line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "use focus::press, or say why it is mouse only: {found:#?}"
    );
    let dropdown = std::fs::read_to_string("src/shared/kit/dropdown.rs").unwrap();
    for part in ["focus::State", "focus::key(", "focus::shade("] {
        assert!(dropdown.contains(part), "the dropdown's stop lacks {part}");
    }
}

/// Our windows are shown by `platform::windows::show` (X11: "always on top"
/// again once mapped, #172). The overlay's hiding (update.rs) leaves full-screen
/// on its way out, not to show it.
#[test]
fn only_the_window_helper_shows_windows() {
    for (name, src) in sources("") {
        if [
            "shared/platform/windows.rs",
            "app/update.rs",
            "app/arch_tests.rs",
        ]
        .contains(&name.as_str())
        {
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
        if name == "shared/platform/monitor.rs" || name == "app/arch_tests.rs" {
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
    let snip: Vec<(String, String)> = [
        "src/features/snip/annotate/text.rs",
        "src/features/snip/annotate/style.rs",
    ]
    .iter()
    .map(|p| (p.to_string(), std::fs::read_to_string(p).unwrap()))
    .collect();
    let mut found = Vec::new();
    for (name, src) in sources("").into_iter().chain(snip) {
        let test_only = name.starts_with("shared/kit/testing/") || name.ends_with("tests.rs");
        if name == "app/arch_tests.rs" || name == "app/frame_times.rs" || test_only {
            continue;
        }
        let src = src.replace("\r\n", "\n");
        let shipped = src.split("#[cfg(test)]").next().unwrap_or("");
        let lines: Vec<&str> = code(shipped).collect();
        for (i, line) in lines.iter().enumerate() {
            let generic = GENERIC.iter().any(|w| line.contains(w));
            let widget =
                !name.starts_with("shared/kit/") && WIDGETS.iter().any(|w| line.contains(w));
            // a text widget without a face: drawn in the renderer's default
            let faceless = name != "shared/kit/text.rs"
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

/// Every `.rs` file under `src/<dir>`, as (path relative to `src`, source with LF).
fn tree(dir: &str) -> Vec<(String, String)> {
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
    let mut files = Vec::new();
    let root = Path::new("src").join(dir);
    if root.is_file() {
        files.push(root);
    } else {
        walk(&root, &mut files);
    }
    let mut out: Vec<_> = files
        .into_iter()
        .map(|p| {
            let name = p
                .strip_prefix("src")
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let src = std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n");
            (name, src)
        })
        .collect();
    out.sort();
    out
}

/// The paths a source names: each `use` tree flattened to whole paths
/// (`use a::{b, c::d}` -> `a::b`, `a::c::d`), plus every `crate::..` path in
/// the code. Comments are skipped.
fn named_paths(src: &str) -> Vec<String> {
    fn expand(prefix: &str, tree: &str, out: &mut Vec<String>) {
        let tree = tree.trim();
        let Some(open) = tree.find('{') else {
            let leaf = tree.split(" as ").next().unwrap_or("").trim();
            out.push(match (prefix.is_empty(), leaf) {
                (_, "self") => prefix.trim_end_matches("::").to_string(),
                (true, _) => leaf.to_string(),
                (false, _) => format!("{prefix}{leaf}"),
            });
            return;
        };
        let head = format!("{prefix}{}", &tree[..open]);
        let inner = &tree[open + 1..tree.rfind('}').unwrap_or(tree.len())];
        let (mut depth, mut start) = (0, 0);
        for (i, c) in inner.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    if !inner[start..i].trim().is_empty() {
                        expand(&head, &inner[start..i], out);
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        if !inner[start..].trim().is_empty() {
            expand(&head, &inner[start..], out);
        }
    }
    let code: String = code(src).collect::<Vec<_>>().join("\n");
    let mut out = Vec::new();
    let mut rest = code.as_str();
    while let Some(i) = rest.find("use ") {
        let word = i == 0 || !rest[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
        let after = &rest[i + 4..];
        match after.find(';') {
            Some(end)
                if word
                    && after.starts_with(|c: char| c.is_alphabetic() || c == '$')
                    && after[..end]
                        .chars()
                        .all(|c| c.is_alphanumeric() || "_:{}, *$\n\t\r".contains(c)) =>
            {
                expand("", &after[..end].replace('\n', " "), &mut out);
                rest = &after[end..];
            }
            _ => rest = after,
        }
    }
    for (i, _) in code.match_indices("crate::") {
        let path: String = code[i..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
            .collect();
        out.push(path);
    }
    out
}

/// `path` names the module `module` (`crate::app`, not `crate::shared::app_language`).
fn names_module(path: &str, module: &str) -> bool {
    path.strip_prefix(module)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

/// Shared files that still reach into a feature or the app shell (code that
/// belongs elsewhere: a follow-up moves it). A ratchet: the list only shrinks.
const SHARED_LEGACY: &[&str] = &[
    "shared/app_language/mod.rs",
    "shared/config/mod.rs",
    "shared/config/parse.rs",
    "shared/diagnostics.rs",
    "shared/history/mod.rs",
    "shared/history/recent.rs",
    "shared/platform/hotkey/kde.rs",
    "shared/platform/hotkey/mod.rs",
    "shared/platform/hotkey/socket.rs",
    "shared/platform/sys/open.rs",
    "shared/platform/tray.rs",
    "shared/secrets.rs",
    "shared/setup/mod.rs",
    "shared/theme.rs",
];

/// `shared/` (kit, platform, config, ...) never imports `features/` or `app/`:
/// what every feature uses depends on no feature.
#[test]
fn shared_never_imports_features_or_app() {
    let mut reaching = Vec::new();
    for (name, src) in tree("shared") {
        // test code (tests, the kit's golden harness) may use anything
        if name.ends_with("tests.rs") || name.contains("/testing/") {
            continue;
        }
        let prod = src.split("#[cfg(test)]\nmod tests {").next().unwrap_or("");
        let bad = named_paths(prod).into_iter().find(|p| {
            ["crate::features", "crate::app"]
                .iter()
                .any(|m| names_module(p, m))
                || p.starts_with("super::super::super")
        });
        if let Some(p) = bad {
            reaching.push((name, p));
        }
    }
    let new: Vec<_> = reaching
        .iter()
        .filter(|(n, _)| !SHARED_LEGACY.contains(&n.as_str()))
        .collect();
    assert!(
        new.is_empty(),
        "shared/ reaches into a feature or the app: {new:#?}"
    );
    let clean: Vec<_> = SHARED_LEGACY
        .iter()
        .filter(|n| !reaching.iter().any(|(r, _)| r == *n))
        .collect();
    assert!(
        clean.is_empty(),
        "now clean, remove from SHARED_LEGACY: {clean:?}"
    );
}

/// Feature files that still reach into another feature's own files. A ratchet:
/// the list only shrinks.
const FEATURE_LEGACY: &[&str] = &[
    "features/ask_ai/app/ai_chat.rs",
    "features/ask_ai/ask.rs",
    "features/ask_ai/mod.rs",
    "features/ask_ai/screens/ai_chat/mod.rs",
    "features/circle/actions/mod.rs",
    "features/circle/actions/search_in_browser/page.rs",
    "features/circle/actions/share.rs",
    "features/circle/app/actions.rs",
    "features/circle/app/bar.rs",
    "features/circle/app/selection.rs",
    "features/circle/screens/bar/geometry.rs",
    "features/circle/screens/bar/view.rs",
    "features/live_translate/live.rs",
    "features/live_translate/screens/live/controller.rs",
    "features/live_translate/screens/live_layer.rs",
    "features/onboarding/flow.rs",
    "features/onboarding/steps/done.rs",
    "features/onboarding/steps/language.rs",
    "features/onboarding/tests.rs",
    "features/settings/labs.rs",
    "features/settings/model_list.rs",
    "features/settings/pages/ai.rs",
    "features/settings/pages/general.rs",
    "features/settings/pages/snip.rs",
    "features/settings/pages/text_reading.rs",
    "features/settings/pages/translate.rs",
    "features/settings/previews/online.rs",
    "features/settings/previews/player.rs",
    "features/settings/state.rs",
    "features/settings/tests.rs",
    "features/snip/action.rs",
    "features/snip/annotate/document.rs",
    "features/snip/annotate/dragout.rs",
    "features/snip/annotate/mod.rs",
    "features/snip/annotate/tests.rs",
    "features/snip/app/snip.rs",
    "features/snip/app/snip_more.rs",
    "features/snip/app/snip_toolbar.rs",
    "features/snip/app/tools.rs",
    "features/snip/capture/live.rs",
    "features/snip/capture/scroll.rs",
    "features/snip/screens/long_capture.rs",
    "features/snip/screens/long_capture_popup.rs",
    "features/snip/screens/snip.rs",
    "features/snip/screens/snip_shots.rs",
    "features/snip/screens/snip_toolbar/mod.rs",
    "features/snip/screens/snip_toolbar/toolbar.rs",
    "features/snip/screens/steps.rs",
    "features/snip/screens/tools/mod.rs",
    "features/snip/screens/tools/picker.rs",
    "features/snip/screens/tools/ruler.rs",
    "features/snip/snip_more.rs",
    "features/song/screens/song.rs",
    "features/text_reading/app/text.rs",
    "features/text_reading/bench.rs",
    "features/text_reading/mod.rs",
    "features/text_reading/models.rs",
    "features/text_reading/session.rs",
    "features/text_reading/system.rs",
    "features/text_reading/text.rs",
    "features/text_reading/worker.rs",
    "features/translate/action.rs",
    "features/translate/mt/mod.rs",
    "features/translate/pipeline.rs",
    "features/translate/screens/translate/mod.rs",
    "features/translate/screens/translate/tests.rs",
    "features/translate/screens/translate/view.rs",
    "features/translate/tests.rs",
];

/// A feature uses another feature only through that feature's `mod.rs` (its
/// public face: `crate::features::snip::Thing`), never its inner files
/// (`crate::features::snip::flow::..`).
#[test]
fn features_meet_only_through_mod_rs() {
    let features: Vec<String> = std::fs::read_dir("src/features")
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(features.contains(&"snip".to_string()), "{features:?}");
    let inner = |feature: &str, part: &str| {
        let dir = Path::new("src/features").join(feature);
        dir.join(format!("{part}.rs")).exists() || dir.join(part).is_dir()
    };
    let mut reaching = Vec::new();
    for (name, src) in tree("features") {
        let own = name.split('/').nth(1).unwrap_or("");
        let bad = named_paths(&src).into_iter().find(|p| {
            let mut parts = p
                .strip_prefix("crate::features::")
                .unwrap_or("")
                .split("::");
            let (f, part) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            !f.is_empty() && f != own && inner(f, part)
        });
        if let Some(p) = bad {
            reaching.push((name, p));
        }
    }
    let new: Vec<_> = reaching
        .iter()
        .filter(|(n, _)| !FEATURE_LEGACY.contains(&n.as_str()))
        .collect();
    assert!(
        new.is_empty(),
        "a feature reaches into another one's files: {new:#?}"
    );
    let clean: Vec<_> = FEATURE_LEGACY
        .iter()
        .filter(|n| !reaching.iter().any(|(r, _)| r == *n))
        .collect();
    assert!(
        clean.is_empty(),
        "now clean, remove from FEATURE_LEGACY: {clean:?}"
    );
}

/// The path scan sees `use` trees and inline paths, and tells `app` from
/// `app_language`.
#[test]
fn the_path_scan_finds_paths() {
    let src = "use crate::{features::snip::{flow, mod_x as y}, shared::kit};\n\
               // use crate::features::song::x;\n\
               fn f() { crate::app::App::go(); }\n";
    let paths = named_paths(src);
    for p in [
        "crate::features::snip::flow",
        "crate::features::snip::mod_x",
        "crate::shared::kit",
        "crate::app::App::go",
    ] {
        assert!(paths.iter().any(|x| x == p), "{p} in {paths:?}");
    }
    assert!(!paths.iter().any(|x| x.contains("song")), "{paths:?}");
    assert!(names_module("crate::app::App", "crate::app"));
    assert!(!names_module("crate::app_language::S", "crate::app"));
}
