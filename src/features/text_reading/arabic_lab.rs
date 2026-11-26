//! R-15 lab: Arabic / mixed screen reading per path, scored on lines AND words
//! (truth from the DOM: lab/r15/tools/corpus-en.mjs, lab/w109-arabic/tools/corpus.mjs).
//! Measurement only; nothing here runs in the app:
//! `OPENCIRCLE_TEST_DOWNLOADS=1 XDG_DATA_HOME=<models> R15=<corpus dir> cargo test --release arabic_lab -- --ignored --nocapture`
//! `R15_PAGES` (comma list), `R15_OUT` (json dir).
#![allow(clippy::all, dead_code)] // a lab harness, run by hand
use super::app::word;
use super::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

struct GLine {
    text: String,
    bx: [f32; 4],
    words: Vec<(String, [f32; 4])>,
}
struct Page {
    name: String,
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    scale: f32,
    lines: Vec<GLine>,
}

fn load_png(path: &Path) -> (Vec<u8>, u32, u32) {
    let img = image::open(path).unwrap().to_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}
fn arr(v: &Value) -> [f32; 4] {
    std::array::from_fn(|i| v[i].as_f64().unwrap() as f32)
}

fn load(dir: &Path, name: &str) -> Page {
    let (rgba, w, h) = load_png(&dir.join(format!("{name}.png")));
    let gt: Value =
        serde_json::from_slice(&std::fs::read(dir.join(format!("{name}.json"))).unwrap()).unwrap();
    let lines = gt["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| GLine {
            text: l["text"].as_str().unwrap().into(),
            bx: arr(&l["box"]),
            words: l["words"]
                .as_array()
                .map(|ws| {
                    ws.iter()
                        .map(|x| (x["text"].as_str().unwrap().to_string(), arr(&x["box"])))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    Page {
        name: name.into(),
        rgba,
        w,
        h,
        scale: gt["scale"].as_f64().unwrap_or(1.0) as f32,
        lines,
    }
}

/// Windows OCR's raw read (lab/w109-arabic/tools/winocr.ps1), as the app's
/// `system::recognize` makes it: words per line -> `system::blocks`.
fn win_words(dir: &Path, name: &str, tag: &str) -> Option<Vec<Vec<system::Word>>> {
    let raw = std::fs::read(dir.join(format!("{name}.win-{tag}.json"))).ok()?;
    let raw = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw);
    let v: Value = serde_json::from_slice(raw).unwrap();
    Some(
        v["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                let ws = match l {
                    Value::Array(a) => a.clone(),
                    o => vec![o.clone()],
                };
                ws.iter()
                    .map(|x| (x["t"].as_str().unwrap().to_string(), arr(&x["b"])))
                    .collect()
            })
            .collect(),
    )
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x064B..=0x065F | 0x0670 | 0x0640))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' => 'ا',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn bare(w: &str) -> String {
    norm(w).chars().filter(|c| c.is_alphanumeric()).collect()
}
fn lev<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            cur.push(
                (prev[j] + usize::from(x != y))
                    .min(prev[j + 1] + 1)
                    .min(cur[j] + 1),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}
/// LCS pairs of equal items.
fn lcs(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut t = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i][j] = if a[i] == b[j] {
                t[i + 1][j + 1] + 1
            } else {
                t[i + 1][j].max(t[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((i, j));
            i += 1;
            j += 1
        } else if t[i + 1][j] >= t[i][j + 1] {
            i += 1
        } else {
            j += 1
        }
    }
    out
}
fn xiou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let i = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let u = (a[0] + a[2]).max(b[0] + b[2]) - a[0].min(b[0]);
    (i.max(0.0) / u.max(1e-3)).clamp(0.0, 1.0)
}

/// Scores `lines` (app lines, image px; page px = image px * scale).
fn score(p: &Page, lines: &[word::Line]) -> Value {
    let k = p.scale;
    let pb = |r: iced::Rectangle| [r.x * k, r.y * k, r.width * k, r.height * k];
    let (mut frag, mut one, mut found, mut cer_e, mut cer_n, mut wer_e, mut wer_n) =
        (0, 0, 0, 0, 0, 0, 0);
    let (mut matched, mut gwords, mut iou_sum, mut iou_ok, mut hit) = (0, 0, 0.0f32, 0, 0);
    let (mut lat_read, mut lat_all) = (0, 0);
    let mut per = Vec::new();
    for g in &p.lines {
        let mine: Vec<&word::Line> = lines
            .iter()
            .filter(|l| {
                let b = pb(l.bounds);
                let cy = b[1] + b[3] / 2.0;
                cy > g.bx[1]
                    && cy < g.bx[1] + g.bx[3]
                    && b[0] < g.bx[0] + g.bx[2]
                    && b[0] + b[2] > g.bx[0]
            })
            .collect();
        let gw: Vec<String> = g
            .words
            .iter()
            .map(|(t, _)| bare(t))
            .filter(|t| !t.is_empty())
            .collect();
        gwords += g.words.iter().filter(|(t, _)| !bare(t).is_empty()).count();
        if mine.is_empty() {
            cer_e += norm(&g.text).chars().count();
            cer_n += norm(&g.text).chars().count();
            wer_e += gw.len();
            wer_n += gw.len();
            per.push(json!([g.text, null]));
            continue;
        }
        found += 1;
        frag += mine.len();
        one += usize::from(mine.len() == 1);
        let text = mine
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let (a, b): (Vec<char>, Vec<char>) = (
            norm(&g.text).chars().collect(),
            norm(&text).chars().collect(),
        );
        cer_e += lev(&a, &b);
        cer_n += a.len();
        let pw: Vec<(String, [f32; 4])> = mine
            .iter()
            .flat_map(|l| word::words(l))
            .map(|(r, t)| (bare(&t), pb(r)))
            .filter(|(t, _)| !t.is_empty())
            .collect();
        let pt: Vec<String> = pw.iter().map(|(t, _)| t.clone()).collect();
        wer_e += lev(&gw, &pt);
        wer_n += gw.len();
        let gfull: Vec<&(String, [f32; 4])> = g
            .words
            .iter()
            .filter(|(t, _)| !bare(t).is_empty())
            .collect();
        let is_lat = |t: &str| t.chars().any(|c| c.is_ascii_alphanumeric());
        lat_all += gw.iter().filter(|t| is_lat(t)).count();
        for (i, j) in lcs(&gw, &pt) {
            lat_read += usize::from(is_lat(&gw[i]));
            matched += 1;
            let v = xiou(&gfull[i].1, &pw[j].1);
            iou_sum += v;
            iou_ok += usize::from(v >= 0.5);
        }
        // double-click on each true word's middle: is that word selected?
        for (t, b) in &gfull {
            let (cx, cy) = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
            let under = pw.iter().find(|(_, r)| {
                cx >= r[0] && cx <= r[0] + r[2] && cy >= r[1] - 4.0 && cy <= r[1] + r[3] + 4.0
            });
            hit += usize::from(under.is_some_and(|(pt, _)| *pt == bare(t)));
        }
        per.push(json!([g.text, text, mine.len()]));
    }
    let f = |a: usize, b: usize| (a as f64 / b.max(1) as f64 * 1000.0).round() / 1000.0;
    json!({
        "gt_lines": p.lines.len(), "lines_found": found, "pred_lines": lines.len(),
        "frag": f(frag, found), "one_piece": f(one, found),
        "cer": f(cer_e, cer_n), "wer": f(wer_e, wer_n),
        "words": gwords, "word_read": f(matched, gwords), "latin_read": f(lat_read, lat_all),
        "xiou": (iou_sum as f64 / matched.max(1) as f64 * 1000.0).round() / 1000.0,
        "box_ok": f(iou_ok, matched), "click_hit": f(hit, gwords),
        "per": per,
    })
}

/// main mode on the helper (Linux; Windows with an RTL / no Windows OCR
/// language): parts from mid-screen out (bench's `main_bands`).
fn helper_main(e: &mut Engine, p: &Page, known: &[[f32; 4]], latin: bool) -> Vec<TextBlock> {
    use super::screen::Rows;
    static KEY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);
    if latin {
        e.auto = 0;
    }
    let h = p.h as f32;
    let key = KEY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut img = (!latin).then(|| to_rgb(&p.rgba, p.w, p.h).unwrap());
    let mut rows = Rows::default();
    let mut out = Vec::new();
    let mut band = rows.next_band(h / 2.0, h);
    while let Some(b) = band {
        if let Some((top, bottom)) = rows.unread(b, h) {
            let img = match latin {
                true => {
                    let (y0, y1) = part_rows((top, bottom), p.h);
                    let row = p.w as usize * 4;
                    Some((
                        to_rgb(&p.rgba[y0 as usize * row..y1 as usize * row], p.w, y1 - y0)
                            .unwrap(),
                        y0,
                    ))
                }
                false => img.take().map(|i| (i, 0)),
            };
            let read = e
                .read_screen(key, img, (top, bottom), known, latin)
                .unwrap();
            if std::env::var_os("R15_DEBUG").is_some() {
                println!(
                    "W109DBG part rows=({top},{bottom}) lines={} done={}",
                    read.lines.len(),
                    read.done
                );
            }
            out.extend(read.lines);
            if read.done {
                break;
            }
        }
        rows.mark(b, h);
        band = rows.next_band(h / 2.0, h);
    }
    out
}

/// What the app shows: Windows OCR's lines, each replaced by a second-reader
/// line over it (either middle in the other's box).
fn shown(first: &[TextBlock], extra: Vec<TextBlock>) -> Vec<TextBlock> {
    let over =
        |a: &TextBlock, b: &TextBlock| holds(&b.bounds, &a.bounds) || holds(&a.bounds, &b.bounds);
    first
        .iter()
        .filter(|l| !extra.iter().any(|x| over(l, x)))
        .cloned()
        .chain(extra.iter().cloned())
        .collect()
}

/// The app's order (`Screen::lines`): rows by middle / 8, then left to right.
fn app_sort(mut v: Vec<TextBlock>) -> Vec<TextBlock> {
    v.sort_by_key(|l| {
        (
            ((l.bounds[1] + l.bounds[3] / 2.0).round() as i32) / 8,
            l.bounds[0] as i32,
        )
    });
    v
}
fn to_lines(p: &Page, blocks: Vec<TextBlock>) -> Vec<word::Line> {
    let mut lines = word::lines(blocks, [0, 0], p.scale);
    if std::env::var_os("R15_NOMEASURE").is_none() {
        word::measure(&mut lines, &p.rgba, p.w, p.h, p.scale);
    }
    lines
}

/// Ink-measured lines of `blocks` on page `p`.
fn run(name: &str, path: &str, p: &Page, blocks: Vec<TextBlock>, extra: &str) {
    let mut lines = to_lines(p, blocks);
    if path.contains("+second") {
        word::order_rows(&mut lines); // (as `Screen::lines`)
    }
    let s = score(p, &lines);
    println!(
        "R15 {name} {path} {}{extra}",
        json!({"lines": s["pred_lines"], "found": s["lines_found"], "gt": s["gt_lines"], "frag": s["frag"], "cer": s["cer"], "word_read": s["word_read"], "latin_read": s["latin_read"], "box_ok": s["box_ok"], "click": s["click_hit"]})
    );
    if let Ok(dir) = std::env::var("R15_OUT") {
        let out = PathBuf::from(dir).join(format!("{name}.{path}.json"));
        std::fs::write(out, serde_json::to_string_pretty(&json!({"score": s, "lines": lines.iter().map(|l| json!([l.text, [l.bounds.x, l.bounds.y, l.bounds.width, l.bounds.height]])).collect::<Vec<_>>()})).unwrap()).unwrap();
    }
}

fn peak_mb() -> i64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage only writes into the struct we pass.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    usage.ru_maxrss / 1024
}

#[test]
#[ignore]
fn arabic_lab() {
    let dir = PathBuf::from(std::env::var("R15").unwrap());
    let pages: Vec<String> = std::env::var("R15_PAGES")
        .unwrap_or("word-ar-1080,word-ar-s150,browser-ar-1080,chat-ar-1080".into())
        .split(',')
        .map(String::from)
        .collect();
    let tags = std::env::var("R15_WIN").unwrap_or("en-US-k2,ar-SA-k2".into());
    for id in ["en", "arabic"] {
        let rec = models::RECOGNIZERS.iter().find(|r| r.id == id).unwrap();
        models::ensure(&models::ocr_files(rec), |_, _| {}).unwrap();
    }
    let mut e = Engine::load(&models_dir()).unwrap();
    for name in pages {
        let p = load(&dir, &name);
        // oracle: the true text in the true line box (only `measure` can err)
        let oracle = p
            .lines
            .iter()
            .map(|g| TextBlock {
                text: g.text.clone(),
                bounds: g.bx,
                confidence: 1.0,
                words: Vec::new(),
            })
            .collect();
        run(&name, "oracle", &p, oracle, "");
        let t = std::time::Instant::now();
        let helper = helper_main(&mut e, &p, &[], true);
        let ms = t.elapsed().as_millis();
        run(
            &name,
            "helper",
            &p,
            helper,
            &format!(" helper_ms={ms} peak_mb={}", peak_mb()),
        );
        for tag in tags.split(',') {
            let Some(ws) = win_words(&dir, &name, tag) else {
                continue;
            };
            if tag.starts_with("ar-SA") {
                run(&name, &format!("win-{tag}"), &p, system::blocks_rtl(ws), "");
                continue;
            }
            let first = system::blocks(ws);
            let texts: Vec<&str> = first.iter().map(|b| b.text.as_str()).collect();
            let well = system::read_well(&texts, Some(&tag[..5]));
            let boxes: Vec<[f32; 4]> = first.iter().map(|b| b.bounds).collect();
            let known = system::known_boxes(&boxes, &well);
            let t2 = std::time::Instant::now();
            let forced = std::env::var_os("R15_FORCE").is_some();
            let extra = if well.iter().all(|w| *w) && !forced {
                Vec::new()
            } else {
                helper_main(&mut e, &p, &known, false)
            };
            let extra_ms = t2.elapsed().as_millis();
            println!(
                "R15 {name} second reader: {} lines, {extra_ms} ms, known {}",
                extra.len(),
                known.len()
            );
            run(
                &name,
                &format!("win-{tag}+second"),
                &p,
                shown(&first, extra),
                "",
            );
        }
    }
}
