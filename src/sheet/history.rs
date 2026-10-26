//! OpenCircle search history (owner, 2026-10-24): Google refuses sign-in inside
//! an embedded web view, so the card keeps its own list of past searches. Opt-in
//! (`general.search_history`, off by default), only in the user's data dir:
//! the query text, a small thumbnail of the crop, the time and the kind.
//! Turning it off deletes it. The card's History button shows the list as our
//! own in-memory page; a click reopens that search in the card.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::Kind;
use crate::actions::Crop;

/// Searches kept (newest first).
pub const KEEP: usize = 50;
/// The thumbnail's long edge (px).
const THUMB: u32 = 96;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub time: u64,
    /// "lens" | "ai".
    pub kind: String,
    /// The question (AI Mode); empty for Lens.
    pub query: String,
    /// Where the search reopens: the AI Mode or Lens results page (https only).
    pub url: String,
    /// A small PNG of the crop, base64 ("" = none).
    pub thumb: String,
}

impl Entry {
    /// A search of `kind` for `query` on `crop`; `url` once known.
    pub fn new(kind: Kind, query: &str, crop: &Crop) -> Entry {
        Entry {
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            kind: kind.arg().into(),
            query: query.into(),
            url: String::new(),
            thumb: thumb(crop).unwrap_or_default(),
        }
    }
}

/// `<data dir>/OpenCircle/search_history.json`.
pub fn path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("OpenCircle").join("search_history.json"))
}

/// The user turned history on, and its Labs switch is on (hidden: nothing is
/// recorded or shown, and the saved list is left alone).
pub fn enabled() -> bool {
    let c = crate::config::current();
    c.general.search_history && c.labs.has(crate::labs::Feature::History)
}

/// Newest first; nothing when the file is missing or unreadable.
pub fn read(path: &Path) -> Vec<Entry> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Adds `entry` at the front (a repeat of the same page moves up), keeps `KEEP`.
pub fn add(path: &Path, entry: Entry) -> std::io::Result<()> {
    let mut list = read(path);
    list.retain(|e| e.url != entry.url);
    list.insert(0, entry);
    list.truncate(KEEP);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec(&list)?)
}

/// Deletes the history (Clear, or history turned off).
pub fn clear(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Keeps `entry` when history is on (`enabled`); off deletes any old history.
pub fn record(path: &Path, enabled: bool, entry: Entry) {
    if !enabled {
        return clear(path);
    }
    if let Err(e) = add(path, entry) {
        eprintln!("[opencircle] search history not saved: {e}");
    }
}

/// [`clear`] at the usual place (Settings turned history off, or Clear).
pub fn clear_default() {
    if let Some(p) = path() {
        clear(&p);
    }
}

/// A `THUMB`-sized PNG of `crop`, base64.
fn thumb(crop: &Crop) -> Option<String> {
    let (w, h) = (crop.width, crop.height);
    if w == 0 || h == 0 || crop.rgba.len() < (w * h * 4) as usize {
        return None;
    }
    let k = (THUMB as f32 / w.max(h) as f32).min(1.0);
    let (tw, th) = (
        ((w as f32 * k) as u32).max(1),
        ((h as f32 * k) as u32).max(1),
    );
    // ponytail: nearest pixel, enough for a 96 px reminder.
    let mut px = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        for x in 0..tw {
            let i = (((y * h / th) * w + x * w / tw) * 4) as usize;
            px.extend_from_slice(&crop.rgba[i..i + 4]);
        }
    }
    let png = crate::actions::encode_png(tw, th, &px).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(png))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// "just now", "5 min ago", "3 h ago", "2 d ago".
fn ago(then: u64, now: u64) -> String {
    match now.saturating_sub(then) {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

/// The History page the card shows (our in-memory page): one link per search,
/// newest first, and Clear. `colours`: the card's (`sheet::colours`). Only
/// plain https links: an entry with anything else is left out.
pub fn page(list: &[Entry], [bg, border, hover, text, muted]: [u32; 5], now: u64) -> String {
    let mut items = String::new();
    for e in list {
        if !e.url.starts_with("https://") || crate::lens::web_host(&e.url).is_none() {
            continue;
        }
        let img = match e
            .thumb
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        {
            true if !e.thumb.is_empty() => {
                format!("<img src=\"data:image/png;base64,{}\">", e.thumb)
            }
            _ => "<span class=\"noimg\"></span>".into(),
        };
        let (what, title) = if e.kind == "lens" {
            ("Lens", "Image search".to_string())
        } else {
            ("AI Mode", e.query.clone())
        };
        items.push_str(&format!(
            "<a href=\"{}\">{img}<span class=\"t\"><b>{}</b><i>{what} · {}</i></span></a>",
            esc(&e.url),
            esc(&title),
            ago(e.time, now)
        ));
    }
    if items.is_empty() {
        items = "<p>No searches yet.</p>".into();
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"referrer\" content=\"no-referrer\"><title>History</title><style>\
         body{{margin:0;padding:16px;font:14px system-ui,sans-serif;background:#{bg:06x};color:#{text:06x}}}\
         h1{{font-size:16px;font-weight:600;margin:0 0 4px}}\
         .top{{display:flex;align-items:center;justify-content:space-between;margin-bottom:12px}}\
         button{{font:inherit;color:#{text:06x};background:#{hover:06x};border:1px solid #{border:06x};\
         border-radius:999px;padding:4px 12px;cursor:pointer}}\
         a{{display:flex;gap:12px;align-items:center;padding:8px;border-radius:12px;\
         color:inherit;text-decoration:none}}a:hover{{background:#{hover:06x}}}\
         img,.noimg{{width:48px;height:48px;object-fit:cover;border-radius:8px;\
         background:#{hover:06x};flex:none}}\
         .t{{display:flex;flex-direction:column;min-width:0}}\
         b{{font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}\
         i{{font-style:normal;font-size:12px;color:#{muted:06x}}}p,.note{{color:#{muted:06x}}}\
         .note{{font-size:12px;margin:0 0 12px}}\
         </style></head><body><div class=\"top\"><h1>Your searches</h1>\
         <button onclick=\"window.ipc.postMessage('clear-history');\
         document.getElementById('l').innerHTML='<p>No searches yet.</p>'\">Clear</button></div>\
         <p class=\"note\">Kept only on this PC. Turn it off in Settings › General.</p>\
         <div id=\"l\">{items}</div></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str) -> Entry {
        Entry {
            time: 100,
            kind: "ai".into(),
            query: format!("q {url}"),
            url: url.into(),
            thumb: String::new(),
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-search-hist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("h.json")
    }

    #[test]
    fn history_is_off_by_default_and_off_deletes_it() {
        assert!(!crate::config::Config::default().general.search_history);
        let path = scratch("off");
        record(&path, true, entry("https://www.google.com/a"));
        assert_eq!(read(&path).len(), 1);
        record(&path, false, entry("https://www.google.com/b"));
        assert!(!path.exists(), "turning it off deletes it");
        assert!(read(&path).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn newest_first_repeats_move_up_and_only_keep_are_kept() {
        let path = scratch("ring");
        for i in 0..KEEP + 7 {
            add(&path, entry(&format!("https://www.google.com/{i}"))).unwrap();
        }
        let list = read(&path);
        assert_eq!(list.len(), KEEP);
        assert_eq!(list[0].url, format!("https://www.google.com/{}", KEEP + 6));
        add(&path, entry("https://www.google.com/30")).unwrap();
        let list = read(&path);
        assert_eq!(
            (list.len(), list[0].url.as_str()),
            (KEEP, "https://www.google.com/30")
        );
        assert_eq!(list.iter().filter(|e| e.url.ends_with("/30")).count(), 1);
        clear(&path);
        assert!(read(&path).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_page_escapes_and_lists_only_https_links() {
        let mut evil = entry("https://www.google.com/search?q=x");
        evil.query = "<script>alert(1)</script>".into();
        evil.thumb = "\"><script>".into();
        let list = [
            evil,
            entry("javascript:alert(1)"),
            entry("http://plain.example/"),
            Entry {
                kind: "lens".into(),
                ..entry("https://lens.google.com/search?p=1")
            },
        ];
        let html = page(&list, [0; 5], 100 + 7200);
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("javascript:") && !html.contains("plain.example"));
        assert!(html.contains("href=\"https://lens.google.com/search?p=1\""));
        assert!(html.contains("Image search") && html.contains("2 h ago"));
        assert!(html.contains("clear-history"));
        assert!(page(&[], [0; 5], 0).contains("No searches yet."));
    }

    #[test]
    fn thumbnails_are_small_pngs() {
        let crop = Crop {
            width: 400,
            height: 200,
            rgba: vec![200; 400 * 200 * 4],
        };
        let png = base64::engine::general_purpose::STANDARD
            .decode(thumb(&crop).unwrap())
            .unwrap();
        let info = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        assert_eq!((info.info().width, info.info().height), (96, 48));
        let empty = Crop {
            width: 0,
            height: 0,
            rgba: vec![],
        };
        assert!(thumb(&empty).is_none());
        assert_eq!(ago(0, 30), "just now");
        assert_eq!(ago(0, 300), "5 min ago");
        assert_eq!(ago(0, 3 * 86_400), "3 d ago");
    }
}
