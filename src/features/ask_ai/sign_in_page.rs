//! The page the browser shows after the OpenRouter sign-in comes back (served once by
//! `serve_once::receive`): a calm centred card in OpenCircle's look (the tokens of the Sapphire
//! theme on a dark OS, Daylight on a light one), our logo, a title and a line, in the app's
//! language (right to left in Arabic). Everything is inline: no font, script or image from anywhere.
use crate::shared::app_language::{S, rtl, tr};

/// The new design's logo (the same inline SVG the Lens hand-off page shows).
const LOGO: &str = include_str!("../../../assets/lens/logo.svg");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The key is being exchanged: "you can close this tab".
    Connected,
    /// OpenRouter answered without access (the user said no): nothing was saved.
    Cancelled,
    /// The answer wasn't ours or didn't come right.
    Failed,
}

const TEMPLATE: &str = r#"<!doctype html>
<html lang="{{LANG}}" dir="{{DIR}}">
<head>
<meta charset="utf-8">
<meta name="referrer" content="no-referrer">
<meta name="color-scheme" content="dark light">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>OpenCircle</title>
<style>
:root{color-scheme:dark light;--bg:#0c172e;--glow:rgba(56,189,248,.13);--card:#101f3d;--rim:rgba(155,205,255,.17);--text:#f0f7ff;--sub:rgba(198,222,255,.66);--ok:#34d399;--no:#f87171;--idle:#94a3b8;--shadow:0 18px 48px rgba(2,6,16,.5)}
@media (prefers-color-scheme:light){:root{--bg:#f5f7fa;--glow:rgba(37,99,235,.10);--card:#ffffff;--rim:rgba(15,23,42,.11);--text:#0f141e;--sub:rgba(15,20,30,.62);--ok:#059669;--no:#dc2626;--idle:#64748b;--shadow:0 18px 48px rgba(15,23,42,.12)}}
html,body{height:100%;margin:0}
body{display:flex;align-items:center;justify-content:center;background:radial-gradient(60% 55% at 50% 45%,var(--glow),transparent 70%),var(--bg);color:var(--sub);font:14px/1.55 "Segoe UI Variable","Segoe UI",system-ui,-apple-system,"Noto Sans Arabic",sans-serif}
.card{display:flex;flex-direction:column;align-items:center;gap:14px;box-sizing:border-box;width:min(340px,calc(100vw - 32px));padding:32px 28px 28px;text-align:center;background:var(--card);border:1px solid var(--rim);border-radius:16px;box-shadow:var(--shadow);animation:rise .19s cubic-bezier(.2,.8,.2,1)}
.logo{position:relative;width:56px;height:56px}
.logo::before{content:"";position:absolute;inset:-14px;border-radius:50%;background:var(--glow);filter:blur(14px)}
.logo svg{position:relative;width:100%;height:100%}
.mark{width:26px;height:26px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:15px;font-weight:700;line-height:1;color:var(--card);background:var(--c)}
.ok{--c:var(--ok)}.failed{--c:var(--no)}.cancelled{--c:var(--idle)}
h1{margin:0;font-size:17px;font-weight:600;color:var(--text);letter-spacing:-.01em}
p{margin:0;max-width:26em}
@keyframes rise{from{opacity:0;transform:translateY(6px)}}
@media (prefers-reduced-motion:reduce){.card{animation:none}}
</style>
</head>
<body>
<main class="card" role="status">
<div class="logo">{{LOGO}}</div>
<div class="mark {{KIND}}" aria-hidden="true">{{MARK}}</div>
<h1>{{TITLE}}</h1>
<p>{{LINE}}</p>
</main>
</body>
</html>
"#;

/// The page for `outcome`, in the app's language.
pub fn page(outcome: Outcome) -> String {
    let (kind, mark, title, line) = match outcome {
        Outcome::Connected => ("ok", "&#10003;", S::signin_ok_title, S::signin_page),
        Outcome::Cancelled => (
            "cancelled",
            "&#8211;",
            S::signin_cancel_title,
            S::signin_cancel_line,
        ),
        Outcome::Failed => ("failed", "!", S::or_err_title, S::signin_fail_line),
    };
    let (lang, direction) = if rtl() { ("ar", "rtl") } else { ("en", "ltr") };
    TEMPLATE
        .replace("{{LANG}}", lang)
        .replace("{{DIR}}", direction)
        .replace("{{KIND}}", kind)
        .replace("{{MARK}}", mark)
        .replace("{{TITLE}}", &escape(tr(title)))
        .replace("{{LINE}}", &escape(tr(line)))
        .replace("{{LOGO}}", LOGO)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The served page: our logo inline, no request to anywhere, every outcome's own words, and
    /// Arabic right to left.
    #[test]
    fn the_page_has_the_logo_no_network_and_both_languages() {
        let all = [Outcome::Connected, Outcome::Cancelled, Outcome::Failed];
        let en: Vec<String> = all.iter().map(|o| page(*o)).collect();
        let _ar = crate::shared::kit::testing::arabic();
        let ar: Vec<String> = all.iter().map(|o| page(*o)).collect();
        for html in en.iter().chain(&ar) {
            assert!(html.contains("<svg"), "the logo is inline");
            assert!(
                html.contains("prefers-color-scheme:light"),
                "light and dark"
            );
            // (the logo's own xmlns is a name, not a request)
            let rest = html.replace("http://www.w3.org/2000/svg", "");
            assert!(
                !rest.contains("http:") && !rest.contains("https:"),
                "no URL"
            );
            assert!(
                !rest.contains("<script") && !rest.contains("src="),
                "nothing loaded"
            );
        }
        assert!(
            en[0].contains(r#"lang="en" dir="ltr""#) && en[0].contains("You can close this tab")
        );
        assert!(en[1].contains("cancelled") && en[2].contains("Couldn’t sign in"));
        assert!(ar.iter().all(|h| h.contains(r#"lang="ar" dir="rtl""#)));
        assert!(ar[0].contains("يمكنك إغلاق هذا التبويب") && ar[2].contains("تعذّر تسجيل الدخول"));
        assert_eq!(en.iter().collect::<std::collections::HashSet<_>>().len(), 3);
    }
}
