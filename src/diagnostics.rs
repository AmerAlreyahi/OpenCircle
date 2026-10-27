//! "Copy diagnostics" and "Report a problem" (Settings > About, #57). Nothing is
//! sent: the report goes to the clipboard when the user clicks, and the issue
//! link opens a pre-filled GitHub page with only the version and the OS (the
//! user pastes the diagnostics himself). Everything passes `log::redact`.

use crate::config::Config;
use crate::log::{os, redact, tail};

const ISSUES: &str = "https://github.com/AmerAlreyahi/OpenCircle/issues/new";

/// The report: version, OS, screens, a settings summary (no keys, no folders,
/// no URLs, no prompts), Labs, OCR, and the last 50 log lines. Redacted.
pub fn report(c: &Config) -> String {
    let screens = crate::capture::monitor_list()
        .iter()
        .map(|m| {
            let (x, y, w, h) = m.rect;
            let primary = if m.primary { ", primary" } else { "" };
            format!("  {w}x{h} at {x},{y}, scale {}{primary}", m.scale)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let windows_ocr = match crate::ocr::system::known() {
        Some(Some(tag)) => format!("reads {tag}"),
        Some(None) => "no recognizer for the Windows languages".into(),
        None => "not asked yet".into(),
    };
    let l = c.labs;
    let text = format!(
        "OpenCircle {version} ({build})\nOS: {os}\n\n\
         Screens:\n{screens}\n\n\
         Settings:\n  theme {theme:?}, widget theme {widget:?}, default action {action}\n  \
         AI provider {provider}, model set: {model}\n  \
         translate {src} -> {dst}, engine {engine:?}, live every {ms} ms\n  \
         text reader {reader:?}, snip after {after:?}, format {format:?}\n  \
         start with the OS {autostart}, auto update {update:?}, quick actions {quick}\n\
         Labs: snip {}, editor {}, song {}, history {}, pins {}\n\n\
         OCR:\n  Windows OCR: {windows_ocr}\n  text reader files downloaded: {ready}\n\n\
         Last log lines:\n{log}\n",
        l.snip,
        l.editor,
        l.song,
        l.history,
        l.pins,
        version = env!("CARGO_PKG_VERSION"),
        build = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        os = os(),
        theme = c.theme,
        widget = c.widget_theme,
        action = c.general.default_action.id(),
        provider = c.ai.provider.id(),
        model = !c.ai.model.is_empty(),
        src = c.translate.source,
        dst = c.translate.target,
        engine = c.translate.engine,
        ms = c.translate.live_interval_ms,
        reader = c.selection.text_reader,
        after = c.snip.after,
        format = c.snip.format,
        autostart = c.general.autostart,
        update = c.update.auto,
        quick = c.quick_actions.len(),
        ready = crate::ocr::ready(),
        log = tail(50).join("\n"),
    );
    redact(&text)
}

/// A pre-filled "new issue" page: the title and a body with the version and OS.
pub fn issue_url() -> String {
    let body = format!(
        "OpenCircle {}\nOS: {}\n\nWhat happened?\n\n\nPlease paste the diagnostics here \
         (Settings > About > Copy diagnostics).\n",
        env!("CARGO_PKG_VERSION"),
        redact(&os()),
    );
    format!(
        "{ISSUES}?title={}&body={}",
        encode("Problem: "),
        encode(&body)
    )
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_has_no_secrets() {
        let (mut c, _) = Config::parse("");
        c.ai.model = "gpt-secret-model".into();
        c.ai.base_url = "https://example.com/v1?key=sk-live-0123456789abcdefXYZ".into();
        c.general.save_folder = "/home/amer/Pictures".into();
        crate::log!(
            "[opencircle] failed: Bearer {}",
            crate::log::tests::fake("sk-ant-api03-")
        );
        crate::log!("[opencircle] cannot read /home/amer/.config/x");
        let r = report(&c);
        for want in [
            "OpenCircle ",
            "OS: ",
            "Screens:",
            "Labs: snip",
            "Last log lines:",
        ] {
            assert!(r.contains(want), "{want}\n{r}");
        }
        for secret in [
            "sk-live-0123",
            "sk-ant-api03",
            "/home/amer",
            "gpt-secret-model",
            "example.com",
            "Pictures",
        ] {
            assert!(!r.contains(secret), "{secret}\n{r}");
        }
        assert!(r.contains("cannot read ~/.config/x"), "{r}");
    }

    #[test]
    fn the_issue_link_is_one_encoded_github_url() {
        let url = issue_url();
        assert!(url.starts_with(
            "https://github.com/AmerAlreyahi/OpenCircle/issues/new?title=Problem%3A%20&body="
        ));
        assert_eq!(crate::lens::web_host(&url).as_deref(), Some("github.com"));
        assert!(!url.contains([' ', '\n', '"']), "{url}");
        let body = url.split("&body=").nth(1).unwrap();
        let version = format!("OpenCircle%20{}", env!("CARGO_PKG_VERSION"));
        assert!(body.contains(&version), "{body}");
        assert!(body.contains("%0A"), "newlines are encoded");
        assert_eq!(encode("a b&c=é"), "a%20b%26c%3D%C3%A9");
    }
}
