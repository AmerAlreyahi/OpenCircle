//! #169: how well the PP-OCR paths read screen text, scored on the rendered
//! corpus of `scripts/ocr-corpus.mjs` (each page's lines and their boxes,
//! from the DOM). Measurement only: nothing here runs in the app. The models
//! download into the app's data dir on first run (use an empty XDG_DATA_HOME):
//! `OPENCIRCLE_TEST_DOWNLOADS=1 OC_CORPUS=<dir> OC_OUT=<file.jsonl> cargo test --release ocr::bench -- --ignored --nocapture`
//! Optional: `OC_PAGES` (comma list of name parts), `OC_PATHS` (comma list,
//! see `PATHS`), `OC_REPS` (timed runs per path, default 3; median kept).
//! "Gently" (2 cores) is the helper pinning itself: run under `taskset -c`.
//! Paths named `x-*` are experiments, not shipped behaviour.
use super::*;
use crate::features::translate::pipeline;
use serde_json::{Value, json};
use std::io::Write;
use std::time::Instant;

const PATHS: &[&str] = &[
    "det", // the detector alone (recall of its boxes)
    "en",  // each recognizer on the whole page
    "latin",
    "arabic",
    "auto",           // Read::Auto = one-shot translate's read on Linux (source "auto")
    "linux-main", // Linux main mode: the whole screen band by band (`ocr::screen`), a fresh helper
    "linux-frame", // main mode's own read (Linux; Windows' helper route): band by band, each part's rows sent (`Engine::read_screen`)
    "except",      // second reader, nothing known
    "except-known", // second reader, EN PP-OCR's plain lines as Windows OCR's stand-in
    "main-today",  // what main mode shows: the stand-in's lines + the second reader's
    "translate-win", // translate on Windows, source auto: misread -> words_read -> second reader
    "x-det-full",  // experiment: detector at full size
    "x-sorted",    // experiment: second reader with its lines sorted by shape
    "x-split",     // experiment: ... and long lines cut into pieces
    "x-route",     // experiment: per-line reader choice
    "x-route-split", // per-line choice + long lines cut
    "x-route-full", // per-line choice + long lines cut + detector at full size
    "x-route-s75", // ... detector at 3/4 size
    "x-route-strips", // ... detector at full size in strips (less memory)
    "x-q",         // per-line `quality` vs correctness (the second reader's filter)
];

struct Line {
    text: String,
    bx: [f32; 4],
    rtl: bool,
}

struct Page {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    lines: Vec<Line>,
}

fn load(dir: &Path, name: &str) -> Page {
    let file = std::fs::File::open(dir.join(format!("{name}.png"))).unwrap();
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => (px.chunks(3))
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        c => panic!("{name}: {c:?}"),
    };
    let gt: Value =
        serde_json::from_slice(&std::fs::read(dir.join(format!("{name}.json"))).unwrap()).unwrap();
    let lines = (gt["lines"].as_array().unwrap().iter())
        .map(|l| Line {
            text: l["text"].as_str().unwrap().into(),
            bx: std::array::from_fn(|i| l["box"][i].as_f64().unwrap() as f32),
            rtl: l["dir"] == "rtl",
        })
        .collect();
    Page {
        rgba,
        w: info.width,
        h: info.height,
        lines,
    }
}

fn is_mark(c: char) -> bool {
    matches!(c, '\u{610}'..='\u{61A}' | '\u{64B}'..='\u{65F}' | '\u{670}' | '\u{6D6}'..='\u{6ED}')
}

/// Text as compared: no tatweel, the Allah ligature spelled out, spaces as
/// one; `marks` keeps the diacritics, `spaces` the spaces.
fn norm(s: &str, marks: bool, spaces: bool) -> Vec<char> {
    let s = s.replace('\u{FDF2}', "الله");
    let mut out: Vec<char> = Vec::new();
    for c in s.chars() {
        if c == '\u{640}' || (!marks && is_mark(c)) {
            continue;
        }
        if c.is_whitespace() {
            if spaces && out.last().is_some_and(|l| *l != ' ') {
                out.push(' ');
            }
            continue;
        }
        out.push(c);
    }
    while out.last() == Some(&' ') {
        out.pop();
    }
    out
}

fn lev(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

fn overlaps(a: &[f32; 4], b: &[f32; 4]) -> bool {
    let ox = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let oy = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    oy >= 0.5 * a[3].min(b[3]) && ox >= 0.5 * a[2].min(b[2])
}

/// Share of `l`'s width that boxes on its row cover.
fn coverage(l: &[f32; 4], boxes: &[[f32; 4]]) -> f32 {
    let mut spans: Vec<(f32, f32)> = (boxes.iter())
        .filter(|b| {
            let oy = (l[1] + l[3]).min(b[1] + b[3]) - l[1].max(b[1]);
            oy >= 0.5 * l[3].min(b[3])
        })
        .map(|b| (b[0].max(l[0]), (b[0] + b[2]).min(l[0] + l[2])))
        .filter(|(a, b)| b > a)
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut covered, mut end) = (0.0, f32::MIN);
    for (a, b) in spans {
        let a = a.max(end);
        if b > a {
            covered += b - a;
            end = b;
        }
    }
    covered / l[2].max(1.0)
}

/// Each ground-truth line against what a path read. Lines and blocks that
/// overlap form one group (the detector joins a row of tabs into one line,
/// or cuts a line in two): a group's texts are compared as one, in reading
/// order, and its result is each of its lines'. Class 0 = read right (CER
/// <= 5 %, diacritics aside), 1 = read with errors, 2 = missed.
fn score(p: &Page, blocks: &[TextBlock]) -> Value {
    let n = p.lines.len();
    let mut group: Vec<usize> = (0..n + blocks.len()).collect();
    fn root(g: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    }
    for (i, l) in p.lines.iter().enumerate() {
        for (j, b) in blocks.iter().enumerate() {
            if overlaps(&l.bx, &b.bounds) {
                let (a, c) = (root(&mut group, i), root(&mut group, n + j));
                group[a] = c;
            }
        }
    }
    let roots: Vec<usize> = (0..n + blocks.len()).map(|i| root(&mut group, i)).collect();
    // reading order inside a group: rows top to bottom, then by x (right to
    // left on a right-to-left line)
    let order = |bx: &[f32; 4], rtl: bool| {
        let row = ((bx[1] + bx[3] / 2.0) / 12.0) as i32;
        (row, if rtl { -bx[0] } else { bx[0] })
    };
    let (mut edits, mut lens, mut all_len, mut all_edits) = ([0usize; 3], [0usize; 3], 0, 0);
    let mut lines = vec![Value::Null; n];
    let mut classes = [0usize; 3];
    let mut done = vec![false; n];
    for i in 0..n {
        if done[i] {
            continue;
        }
        let mine: Vec<usize> = (0..n).filter(|&k| roots[k] == roots[i]).collect();
        let rtl = p.lines[i].rtl;
        let mut gt: Vec<&Line> = mine.iter().map(|&k| &p.lines[k]).collect();
        gt.sort_by(|a, b| order(&a.bx, rtl).partial_cmp(&order(&b.bx, rtl)).unwrap());
        let mut on: Vec<&TextBlock> = (0..blocks.len())
            .filter(|&j| roots[n + j] == roots[i])
            .map(|j| &blocks[j])
            .collect();
        on.sort_by(|a, b| {
            order(&a.bounds, rtl)
                .partial_cmp(&order(&b.bounds, rtl))
                .unwrap()
        });
        let want = gt
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let got = on
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let variants = [(true, true), (false, true), (false, false)];
        let cer: Vec<f32> = (variants.iter().enumerate())
            .map(|(k, &(m, s))| {
                let (g, t) = (norm(&want, m, s), norm(&got, m, s));
                let e = lev(&t, &g);
                if !got.is_empty() {
                    edits[k] += e;
                    lens[k] += g.len();
                }
                if k == 1 {
                    all_len += g.len();
                    all_edits += if got.is_empty() { g.len() } else { e };
                }
                e as f32 / g.len().max(1) as f32
            })
            .collect();
        let class = match (got.is_empty(), cer[1] <= 0.05) {
            (true, _) => 2,
            (false, true) => 0,
            (false, false) => 1,
        };
        for &k in &mine {
            done[k] = true;
            classes[class] += 1;
            lines[k] = json!({"c": class, "cer": cer[1], "got": got, "group": mine.len()});
        }
    }
    let rate = |k: usize| edits[k] as f32 / lens[k].max(1) as f32;
    json!({
        "gt": n, "green": classes[0], "amber": classes[1], "red": classes[2],
        "cer": [rate(0), rate(1), rate(2)], "cer_all": all_edits as f32 / all_len.max(1) as f32,
        "lines": lines,
    })
}

/// The second reader kept a line read at least this well (before #169).
const OTHER_QUALITY: f32 = 0.6;

fn index(id: &str) -> usize {
    models::RECOGNIZERS.iter().position(|r| r.id == id).unwrap()
}

fn arabic_share(text: &str) -> Option<f32> {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    let ar = letters.iter().filter(|c| Script::Arabic.has(**c)).count();
    (!letters.is_empty()).then(|| ar as f32 / letters.len() as f32)
}

/// `OC_CUT`: the longest line (in heights) the Arabic reader reads whole.
fn cut_at() -> f32 {
    (std::env::var("OC_CUT").ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(60.0)
}

/// Recognizer `i` on `boxes`, each cut into pieces first (`split`: pieces of
/// at most that many heights); one
/// block (or none: nothing read) per box, its pieces joined in reading order.
fn read_joined(
    e: &mut Engine,
    i: usize,
    img: &RgbImage,
    boxes: &[[f32; 4]],
    split: Option<f32>,
) -> Vec<Option<TextBlock>> {
    let mut pieces: Vec<([f32; 4], usize)> = Vec::new();
    for (k, b) in boxes.iter().enumerate() {
        let parts = match split {
            Some(most) => cut(img, b, most),
            None => vec![*b],
        };
        pieces.extend(parts.into_iter().map(|p| (p, k)));
    }
    pieces.sort_by(|a, b| (a.0[2] / a.0[3]).total_cmp(&(b.0[2] / b.0[3])));
    let only: Vec<[f32; 4]> = pieces.iter().map(|p| p.0).collect();
    let read = e.read_lines(i, img, &only).unwrap();
    let mut per: Vec<Vec<TextBlock>> = vec![Vec::new(); boxes.len()];
    for b in read {
        let j = only.iter().position(|p| *p == b.bounds).unwrap();
        per[pieces[j].1].push(b);
    }
    let rtl = models::RECOGNIZERS[i].rtl();
    (per.into_iter().enumerate())
        .map(|(k, mut parts)| {
            if parts.is_empty() {
                return None;
            }
            parts.sort_by(|a, b| a.bounds[0].total_cmp(&b.bounds[0]));
            if rtl {
                parts.reverse();
            }
            let n: f32 = parts.iter().map(|p| p.text.chars().count() as f32).sum();
            let conf = parts
                .iter()
                .map(|p| p.confidence * p.text.chars().count() as f32)
                .sum::<f32>()
                / n.max(1.0);
            let text = parts
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            Some(TextBlock {
                text,
                bounds: boxes[k],
                confidence: conf,
            })
        })
        .collect()
}

fn full_size(w: u32, h: u32) -> (u32, u32) {
    (w.div_ceil(32) * 32, h.div_ceil(32) * 32)
}

/// How the experiments find the lines.
#[derive(Clone, Copy)]
enum Det {
    /// As the app does (`detect_size`).
    Default,
    /// The image shrunk by this factor at most (1.0 = full size).
    Scale(f32),
    /// Full size, in horizontal strips (peak memory of one strip).
    Strips,
}

/// The overlap above and below a strip: a line up to twice as tall has its
/// middle in one strip and lies whole in it.
const OVERLAP: u32 = 64;

fn detect_with(e: &mut Engine, img: &RgbImage, det: Det) -> Vec<[f32; 4]> {
    let (w, h) = img.dimensions();
    match det {
        Det::Default => e.detect(img, w, h).unwrap().0,
        Det::Scale(s) => {
            let side = |v: u32| (((v as f32 * s) as u32 + 16) / 32 * 32).max(32);
            e.detect_at(img, w, h, (side(w), side(h))).unwrap().0
        }
        Det::Strips => {
            // rows each strip owns (`OC_STRIP`, default 448): fewer = less
            // memory per detector run, more runs
            let strip: u32 = (std::env::var("OC_STRIP").ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(448);
            let mut out = Vec::new();
            let mut y0 = 0;
            while y0 < h {
                let top = y0.saturating_sub(OVERLAP);
                let bottom = (y0 + strip + OVERLAP).min(h);
                let part = image::imageops::crop_imm(img, 0, top, w, bottom - top).to_image();
                let (sw, sh) = part.dimensions();
                let owned = y0 as f32..(y0 + strip).min(h) as f32;
                let boxes = e.detect_at(&part, sw, sh, full_size(sw, sh)).unwrap().0;
                out.extend(
                    (boxes.into_iter())
                        .map(|b| [b[0], b[1] + top as f32, b[2], b[3]])
                        .filter(|b| owned.contains(&(b[1] + b[3] / 2.0))),
                );
                y0 += strip;
            }
            out
        }
    }
}

/// Experiment: find the lines once, read each with the Arabic reader (its
/// list has Latin letters and digits too); a line it read as mostly Latin
/// letters, or not at all, is read again by the English reader. A mixed
/// Arabic line keeps the Arabic reading.
fn route(e: &mut Engine, p: &Page, det: Det, split: bool) -> Vec<TextBlock> {
    let img = to_rgb(&p.rgba, p.w, p.h).unwrap();
    let boxes = detect_with(e, &img, det);
    let mut out = read_joined(e, index("arabic"), &img, &boxes, split.then(cut_at));
    // read again in English: a line read as mostly Latin letters, as no
    // letters at all (digits or punctuation only), or not at all; the English
    // reading replaces it if it has letters (Arabic-Indic digits are the
    // Arabic reader's) or the Arabic one read nothing
    let share = |b: &Option<TextBlock>| b.as_ref().map(|b| arabic_share(&b.text));
    let latin: Vec<usize> = (0..boxes.len())
        .filter(|&k| share(&out[k]).is_none_or(|s| s.is_none_or(|s| s < 0.3)))
        .collect();
    if !latin.is_empty() {
        let sub: Vec<[f32; 4]> = latin.iter().map(|&k| boxes[k]).collect();
        // the English reader reads lines up to its input cap well (3.3) and
        // cut pieces worse: only lines past the cap are cut for it
        let en = read_joined(e, 0, &img, &sub, split.then_some(60.0));
        for (j, &k) in latin.iter().enumerate() {
            let letters = share(&en[j]).is_some_and(|s| s.is_some());
            if std::env::var_os("OC_DEBUG").is_some() {
                let t = |b: &Option<TextBlock>| b.as_ref().map(|b| b.text.clone());
                eprintln!(
                    "route {:?}: arabic {:?} -> en {:?}",
                    boxes[k],
                    t(&out[k]),
                    t(&en[j])
                );
            }
            if en[j].is_some() && (letters || out[k].is_none()) {
                out[k] = en[j].clone();
            }
        }
    }
    reading_order(out.into_iter().flatten().collect(), false)
}

/// The second reader with nothing known, its lines sorted by shape before
/// they are batched (as `recognize_rows` does; `recognize_except` doesn't),
/// long lines cut when `split`, then filtered at `OTHER_QUALITY` as
/// `recognize_except` does (Arabic: the only non-Latin reader installed).
fn except_split(e: &mut Engine, p: &Page, split: bool) -> Vec<TextBlock> {
    let img = to_rgb(&p.rgba, p.w, p.h).unwrap();
    let (boxes, _) = e.detect(&img, p.w, p.h).unwrap();
    (read_joined(e, index("arabic"), &img, &boxes, split.then(cut_at))
        .into_iter()
        .flatten())
    .filter(|b| quality(std::slice::from_ref(b), 1, Script::Arabic) >= OTHER_QUALITY)
    .collect()
}

/// Windows OCR's stand-in (it doesn't exist on Linux): what the English
/// PP-OCR reader returns, filtered as main mode filters Windows OCR's lines
/// before the second reader (`plain`).
fn plain_boxes(en: &[TextBlock]) -> Vec<[f32; 4]> {
    // ponytail: a copy of features::text_reading::app::text::plain (private to the UI): the
    // stand-in's input only, not a second implementation in the app.
    let plain = |text: &str| {
        let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
        let fine = |c: &&char| c.is_alphanumeric() || ".,:;!?'\"()-/&%@#+".contains(**c);
        let run = text
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w.chars().count() >= 2);
        run && chars.iter().filter(fine).count() * 10 >= chars.len() * 8
    };
    en.iter()
        .filter(|l| plain(&l.text))
        .map(|l| l.bounds)
        .collect()
}

fn run(
    e: &mut Engine,
    path: &str,
    p: &Page,
    en: &[TextBlock],
    known: &[[f32; 4]],
    known3: &[[f32; 4]],
) -> Vec<TextBlock> {
    let (rgba, w, h) = (&p.rgba[..], p.w, p.h);
    match path {
        "en" | "latin" | "arabic" => e.recognize(rgba, w, h, Read::One(index(path))).unwrap(),
        "auto" => {
            e.auto = 0; // a fresh helper: English first
            e.recognize(rgba, w, h, Read::Auto).unwrap()
        }
        "except" => e.recognize_except(rgba, w, h, &[]).unwrap(),
        "linux-main" => linux_main(e, p),
        "linux-frame" => main_bands(e, p, &[], true, false),
        "linux-first" => main_bands(e, p, &[], true, true),
        "main-new" => {
            let extra = main_bands(e, p, known3, false, false);
            shown(en, extra)
        }
        "win-first" => main_bands(e, p, known3, false, true),
        "except-known" => e.recognize_except(rgba, w, h, known).unwrap(),
        "except-known3" => e.recognize_except(rgba, w, h, known3).unwrap(),
        // what main mode shows: the stand-in's lines, each replaced by a
        // second-reader line over it (`Screen::iter`: either one's middle in
        // the other's box)
        "main-today" | "main-r3" => {
            let known = if path == "main-today" { known } else { known3 };
            let extra = e.recognize_except(rgba, w, h, known).unwrap();
            shown(en, extra)
        }
        // Windows OCR reads an Arabic page as noise (`misread`, measured 0.32
        // words on the VM); the English reader's noise here may pass as words,
        // so the stand-in always takes the misread branch
        "translate-win" => {
            let (keep, known) = pipeline::words_read(en.to_vec(), Some("en-US"));
            let other = e.recognize_except(rgba, w, h, &known).unwrap();
            if other.is_empty() {
                e.auto = 0;
                return e.recognize(rgba, w, h, Read::Auto).unwrap();
            }
            pipeline::merged(keep, other)
        }
        "x-sorted" => except_split(e, p, false),
        "x-split" => except_split(e, p, true),
        "x-route" => route(e, p, Det::Default, false),
        "x-route-split" => route(e, p, Det::Default, true),
        "x-route-full" => route(e, p, Det::Scale(1.0), true),
        "x-route-s75" => route(e, p, Det::Scale(0.75), true),
        "x-route-strips" => route(e, p, Det::Strips, true),
        _ => unreachable!("{path}"),
    }
}

/// Linux main mode as the app reads a screen once text is used (as before
/// #169): bands from the pointer (mid-screen) out (`ocr::screen`), each
/// band's crop sent to the helper with the rows it is read for
/// (`features::text_reading::app::text::read_band`, `Engine::recognize_band`).
fn linux_main(e: &mut Engine, p: &Page) -> Vec<TextBlock> {
    use super::screen::{Next, Rows, Wants, next};
    e.band_auto = 0; // a fresh helper: English first
    let (w, h) = (p.w, p.h);
    let mut rows = Rows::default();
    let mut out = Vec::new();
    let wants = Wants {
        source: Source::Helper,
        started: true,
        touches: &[],
        waits: false,
        text_used: true,
        pointer: h as f32 / 2.0,
    };
    while let Next::Band { band, .. } = next(&rows, h as f32, &wants) {
        let Some((top, bottom)) = rows.unread(band, h as f32) else {
            rows.mark(band, h as f32);
            continue;
        };
        let y0 = band.top as u32;
        let bh = (band.height as u32).min(h - y0);
        let crop = &p.rgba[(y0 * w * 4) as usize..((y0 + bh) * w * 4) as usize];
        let r = (top as u32 - y0, bottom as u32 - y0);
        let got = e.recognize_band(crop, w, bh, Read::Auto, r).unwrap();
        out.extend(got.into_iter().map(|mut b| {
            b.bounds[1] += y0 as f32;
            b
        }));
        rows.mark(band, h as f32);
    }
    out
}

/// Main mode as the app reads a screen (#169): bands from the pointer
/// (mid-screen) out (`ocr::screen`), each band's rows read by
/// `Engine::read_screen`. Main mode's own read (`latin`): `known` empty,
/// each part's rows sent with it (`part_rows`; a fresh helper: English
/// first). The second reader: the screen sent with the first band only.
/// Windows: the second reader after Windows OCR (`known`: its lines read as
/// words, the stand-in's here), Latin lines left to it. `first`: the band
/// under the pointer only ("first lines under the pointer").
fn main_bands(
    e: &mut Engine,
    p: &Page,
    known: &[[f32; 4]],
    latin: bool,
    first: bool,
) -> Vec<TextBlock> {
    use super::screen::Rows;
    use std::sync::atomic::{AtomicU64, Ordering};
    static KEY: AtomicU64 = AtomicU64::new(1);
    if latin {
        e.auto = 0;
    }
    let h = p.h as f32;
    let key = KEY.fetch_add(1, Ordering::Relaxed);
    let mut img = (!latin).then(|| to_rgb(&p.rgba, p.w, p.h).unwrap());
    let mut rows = Rows::default();
    let mut out = Vec::new();
    let mut band = Some(Rows::touched(h / 2.0, h));
    while let Some(b) = band {
        if let Some((top, bottom)) = rows.unread(b, h) {
            let img = match latin {
                true => {
                    let (y0, y1) = part_rows((top, bottom), p.h);
                    let row = p.w as usize * 4;
                    let part = &p.rgba[y0 as usize * row..y1 as usize * row];
                    Some((to_rgb(part, p.w, y1 - y0).unwrap(), y0))
                }
                false => img.take().map(|img| (img, 0)),
            };
            let read = e
                .read_screen(key, img, (top, bottom), known, latin)
                .unwrap();
            out.extend(read.lines);
            if read.done {
                break; // (the app marks every row read)
            }
        }
        rows.mark(b, h);
        if first {
            break;
        }
        band = rows.next_band(h / 2.0, h);
    }
    out
}

/// What main mode shows on Windows: Windows OCR's (the stand-in's) lines,
/// each replaced by a second-reader line over it (`Screen::iter`: either
/// one's middle in the other's box).
fn shown(en: &[TextBlock], extra: Vec<TextBlock>) -> Vec<TextBlock> {
    let over =
        |a: &TextBlock, b: &TextBlock| holds(&b.bounds, &a.bounds) || holds(&a.bounds, &b.bounds);
    let first = en
        .iter()
        .filter(|l| !extra.iter().any(|x| over(l, x)))
        .cloned();
    first.chain(extra.iter().cloned()).collect()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn peak_mb() -> i64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage only writes into the struct we pass.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    usage.ru_maxrss / 1024
}

#[test]
#[ignore]
fn screen_text_corpus() {
    let dir = PathBuf::from(std::env::var("OC_CORPUS").expect("OC_CORPUS: the corpus folder"));
    let out = std::env::var("OC_OUT").expect("OC_OUT: the results file (JSON lines)");
    let reps: usize = std::env::var("OC_REPS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3);
    let want = |var: &str, all: Vec<String>| match std::env::var(var) {
        Ok(v) => {
            let parts: Vec<String> = v.split(',').map(String::from).collect();
            all.into_iter()
                .filter(|a| parts.iter().any(|p| a.contains(p.as_str())))
                .collect()
        }
        Err(_) => all,
    };
    let names: Vec<String> =
        serde_json::from_slice(&std::fs::read(dir.join("index.json")).unwrap()).unwrap();
    let names = want("OC_PAGES", names);
    let paths: Vec<String> = match std::env::var("OC_PATHS") {
        Ok(v) => v.split(',').map(String::from).collect(),
        Err(_) => PATHS.iter().map(|p| p.to_string()).collect(),
    };

    // the owner's readers: English, Latin, Arabic
    for id in ["en", "latin", "arabic"] {
        models::ensure(
            &models::ocr_files(models::RECOGNIZERS[index(id)]),
            |_, _| {},
        )
        .unwrap();
    }
    let mut file = std::fs::File::create(&out).unwrap();
    let mut emit = |v: Value| writeln!(file, "{v}").unwrap();
    let t = Instant::now();
    let mut e = Engine::load(&models_dir()).unwrap();
    let load_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut rec_ms = Vec::new();
    for id in ["en", "latin", "arabic"] {
        e.recs.clear();
        let t = Instant::now();
        e.recognizer(index(id)).unwrap();
        rec_ms.push((id, t.elapsed().as_secs_f64() * 1e3));
    }
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    println!("engine load {load_ms:.0} ms, recognizers {rec_ms:?}, {cores} cores");
    emit(json!({"setup": {"engine_ms": load_ms, "rec_ms": rec_ms, "cores": cores}}));

    for name in &names {
        let p = load(&dir, name);
        // the stand-in for Windows OCR (not timed; skipped when no path needs
        // it, so a peak RSS is the path's own)
        let standin = (paths.iter()).any(|p| {
            p.starts_with("except-known")
                || p.starts_with("main-")
                || p == "translate-win"
                || p == "win-first"
        });
        let en = match standin {
            true => e.recognize(&p.rgba, p.w, p.h, Read::One(0)).unwrap(),
            false => Vec::new(),
        };
        let known = plain_boxes(&en);
        // #169 (R3): the lines Windows OCR (English) read as real words
        let texts: Vec<&str> = en.iter().map(|b| b.text.as_str()).collect();
        let well = system::read_well(&texts, Some("en-US"));
        let known3: Vec<[f32; 4]> = (en.iter().zip(well))
            .filter(|(_, w)| *w)
            .map(|(b, _)| b.bounds)
            .collect();
        emit(json!({"page": name, "w": p.w, "h": p.h, "known": known,
            "en": en.iter().map(|b| json!([b.text, b.bounds])).collect::<Vec<_>>()}));
        for path in &paths {
            let img = to_rgb(&p.rgba, p.w, p.h).unwrap();
            if path == "det" || path == "x-det-full" {
                let size = match path.as_str() {
                    "det" => detect_size(p.w, p.h),
                    _ => full_size(p.w, p.h),
                };
                let mut ms = Vec::new();
                let mut boxes = Vec::new();
                for _ in 0..reps {
                    let t = Instant::now();
                    boxes = e.detect_at(&img, p.w, p.h, size).unwrap().0;
                    ms.push(t.elapsed().as_secs_f64() * 1e3);
                }
                let cov: Vec<f32> = p.lines.iter().map(|l| coverage(&l.bx, &boxes)).collect();
                let found = cov.iter().filter(|c| **c >= 0.7).count();
                println!(
                    "{name} {path} {size:?}: {found}/{} lines found, {:.0} ms",
                    p.lines.len(),
                    median(ms.clone())
                );
                emit(
                    json!({"page": name, "path": path, "size": size, "ms": ms, "median": median(ms),
                    "gt": p.lines.len(), "found": found, "coverage": cov, "boxes": boxes}),
                );
                continue;
            }
            if path == "x-q" {
                let (boxes, _) = e.detect(&img, p.w, p.h).unwrap();
                let read = read_joined(&mut e, index("arabic"), &img, &boxes, None);
                let blocks: Vec<TextBlock> = read.into_iter().flatten().collect();
                let per: Vec<Value> = (blocks.iter())
                    .map(|b| {
                        let q = quality(std::slice::from_ref(b), 1, Script::Arabic);
                        let s = score(&p, std::slice::from_ref(b));
                        // the ground-truth line(s) this block is on
                        let on: Vec<usize> = (0..p.lines.len())
                            .filter(|&i| overlaps(&p.lines[i].bx, &b.bounds))
                            .collect();
                        let cer: Vec<f64> = on.iter().map(|&i| s["lines"][i]["cer"].as_f64().unwrap()).collect();
                        json!({"q": q, "conf": b.confidence, "text": b.text, "bounds": b.bounds, "on": on, "cer": cer})
                    })
                    .collect();
                emit(json!({"page": name, "path": path, "blocks": per}));
                continue;
            }
            let mut ms = Vec::new();
            let mut got = Vec::new();
            for _ in 0..reps {
                let t = Instant::now();
                got = run(&mut e, path, &p, &en, &known, &known3);
                ms.push(t.elapsed().as_secs_f64() * 1e3);
            }
            let mut s = score(&p, &got);
            println!(
                "{name} {path}: {} blocks; green {} amber {} red {} of {}; CER {:.3} (all {:.3}); {:.0} ms",
                got.len(),
                s["green"],
                s["amber"],
                s["red"],
                s["gt"],
                s["cer"][1].as_f64().unwrap(),
                s["cer_all"].as_f64().unwrap(),
                median(ms.clone())
            );
            s["page"] = json!(name);
            s["path"] = json!(path);
            s["median"] = json!(median(ms.clone()));
            s["ms"] = json!(ms);
            s["blocks"] = json!(
                got.iter()
                    .map(|b| json!([b.text, b.bounds, b.confidence]))
                    .collect::<Vec<_>>()
            );
            emit(s);
        }
    }
    let peak = peak_mb();
    println!("peak RSS {peak} MB");
    emit(json!({"peak_mb": peak}));
}

#[test]
fn scoring_basics() {
    assert_eq!(lev(&norm("abc", true, true), &norm("abd", true, true)), 1);
    // diacritics and tatweel aside
    assert_eq!(norm("العِلْمُ", false, true), norm("العلـم", false, true));
    assert_eq!(norm(" a  b ", true, true), vec!['a', ' ', 'b']);
    assert_eq!(norm("a b", true, false), vec!['a', 'b']);
    assert!(
        (coverage(
            &[0.0, 0.0, 100.0, 10.0],
            &[[0.0, 0.0, 40.0, 10.0], [30.0, 1.0, 40.0, 10.0]]
        ) - 0.7)
            .abs()
            < 1e-3
    );
    assert!(overlaps(&[0.0, 0.0, 100.0, 10.0], &[50.0, 2.0, 30.0, 10.0]));
    assert!(!overlaps(
        &[0.0, 0.0, 100.0, 10.0],
        &[0.0, 9.0, 100.0, 10.0]
    ));
}
