use super::*;
use std::cell::RefCell;

type OcrFn = Box<dyn Fn(&[u8], u32, u32) -> Vec<TextBlock>>;

/// Records every call; OCR answers with `lines`, AI with `answer` per text.
pub struct Fake {
    pub lines: OcrFn,
    pub answer: fn(&str) -> String,
    pub fail_ai: bool,
    /// `Deps::batch` (sentences per call when shown as they come).
    pub batch: usize,
    /// How long each translate call takes.
    pub delay: Duration,
    pub ocr_calls: RefCell<Vec<(u32, u32)>>,
    pub ai_calls: RefCell<Vec<Vec<String>>>,
}

impl Fake {
    pub fn new(lines: impl Fn(&[u8], u32, u32) -> Vec<TextBlock> + 'static) -> Self {
        Fake {
            lines: Box::new(lines),
            answer: |s| format!("ترجمة {s}"),
            fail_ai: false,
            batch: usize::MAX,
            delay: Duration::ZERO,
            ocr_calls: RefCell::default(),
            ai_calls: RefCell::default(),
        }
    }
}

impl Deps for Fake {
    fn recognize(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<TextBlock>, OcrError> {
        self.ocr_calls.borrow_mut().push((w, h));
        Ok((self.lines)(rgba, w, h))
    }

    fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
        self.ai_calls.borrow_mut().push(texts.to_vec());
        std::thread::sleep(self.delay);
        if self.fail_ai {
            return Err(AiError::Server(503, String::new()));
        }
        Ok(texts.iter().map(|t| (self.answer)(t)).collect())
    }

    fn batch(&self) -> usize {
        self.batch
    }
}

pub fn tb(text: &str, bounds: [f32; 4]) -> TextBlock {
    TextBlock {
        text: text.into(),
        bounds,
        confidence: 0.9,
    }
}

/// Dark frame with a light bar ("glyphs") under each OCR box.
pub fn frame(w: u32, h: u32, boxes: &[[f32; 4]]) -> Vec<u8> {
    let mut img = [20u8, 24, 32, 255].repeat((w * h) as usize);
    for b in boxes {
        let [x, y, bw, bh] = b.map(|v| v as u32);
        for py in y + bh / 4..y + bh * 3 / 4 {
            for px in (x..x + bw).step_by(3) {
                let i = ((py * w + px) * 4) as usize;
                img[i..i + 3].copy_from_slice(&[240, 236, 226]);
            }
        }
    }
    img
}

fn screen() -> Vec<TextBlock> {
    vec![
        tb("Bring the Sunstone", [40.0, 40.0, 300.0, 30.0]),
        tb("100 / 250", [40.0, 120.0, 120.0, 30.0]),
        tb("مرحبا بك", [40.0, 200.0, 140.0, 30.0]),
        tb("Quit", [40.0, 280.0, 80.0, 30.0]),
        tb("Bring the  Sunstone", [400.0, 360.0, 300.0, 30.0]),
    ]
}

#[test]
fn one_call_in_order_skipping_numbers_and_target_language() {
    let boxes: Vec<[f32; 4]> = screen().iter().map(|b| b.bounds).collect();
    let img = frame(800, 450, &boxes);
    let fake = Fake::new(|_, _, _| screen());
    let cache = Cache::new(64);
    let out = translate_frame(&img, 800, 450, "ar", &cache, &fake).unwrap();
    // the repeated line (other spacing) is sent once
    assert_eq!(
        *fake.ai_calls.borrow(),
        vec![vec!["Bring the Sunstone".to_string(), "Quit".into()]]
    );
    let tr: Vec<&str> = out.blocks.iter().map(|b| b.translation.as_str()).collect();
    assert_eq!(
        tr,
        [
            "ترجمة Bring the Sunstone",
            "",
            "",
            "ترجمة Quit",
            "ترجمة Bring the Sunstone"
        ]
    );
    assert_eq!(out.blocks[1].original, "100 / 250");
    // OCR boxes padded to line boxes
    assert_eq!(out.blocks[4].bounds, [397.0, 355.5, 306.0, 39.0]);
    assert!(out.rgba != img, "translated blocks are redrawn");
    let t = out.timings;
    assert!(t.total >= t.ocr + t.translate + t.render && t.total > Duration::ZERO);

    // second run: everything from the cache, no AI call at all
    let again = translate_frame(&img, 800, 450, "ar", &cache, &fake).unwrap();
    assert_eq!(fake.ai_calls.borrow().len(), 1);
    assert!(again.rgba == out.rgba);
}

/// #31: short headings and contents items are all sent to the model, one
/// each; the OS's activation watermark (bottom-right corner, any Windows
/// language) is never sent nor redrawn, but the same words mid-page are text.
#[test]
fn headings_and_contents_are_translated_and_the_os_watermark_is_left() {
    let page = |watermark: [f32; 4]| {
        vec![
            tb("Contents", [40.0, 40.0, 120.0, 28.0]),
            tb("1 History", [40.0, 90.0, 110.0, 28.0]),
            tb("2.1 Early life", [40.0, 130.0, 140.0, 28.0]),
            tb("See also", [40.0, 170.0, 90.0, 28.0]),
            tb("Notes", [40.0, 210.0, 70.0, 28.0]),
            tb("Activate Windows", watermark),
            tb(
                "Go to Settings to activate Windows.",
                [watermark[0], watermark[1] + 30.0, 300.0, 24.0],
            ),
        ]
    };
    for (corner, sent) in [
        ([650.0, 360.0, 200.0, 26.0], 5),
        ([300.0, 250.0, 200.0, 26.0], 7),
    ] {
        let lines = page(corner);
        let boxes: Vec<[f32; 4]> = lines.iter().map(|b| b.bounds).collect();
        let img = frame(900, 450, &boxes);
        let fake = Fake::new(move |_, _, _| lines.clone());
        let out = translate_frame(&img, 900, 450, "ar", &Cache::new(64), &fake).unwrap();
        let calls = fake.ai_calls.borrow();
        let got: Vec<&String> = calls.iter().flatten().collect();
        assert_eq!(got.len(), sent, "{got:?}");
        assert!(out.blocks.iter().all(|b| b.original != "Activate Windows") == (sent == 5));
    }
}

/// #27: top of the screen first, a few sentences, then more per call, and the first
/// part is shown before the rest is translated; the whole page is then
/// exactly the all-at-once result, and the part shown early stays put.
#[test]
fn blocks_arrive_top_first_and_are_shown_before_the_last_is_translated() {
    // OCR lists the bottom label first (a sidebar read before the page)
    let labels = ["Quit", "Load", "Save", "Help", "Exit", "Undo"];
    let ys = [400.0, 40.0, 100.0, 160.0, 220.0, 280.0];
    let lines = move || -> Vec<TextBlock> {
        (labels.iter().zip(ys))
            .map(|(t, y)| tb(t, [40.0, y, 120.0, 30.0]))
            .collect()
    };
    let boxes: Vec<[f32; 4]> = lines().iter().map(|b| b.bounds).collect();
    let img = frame(400, 480, &boxes);
    let mut fake = Fake::new(move |_, _, _| lines());
    fake.batch = 2;
    let mut shown = Vec::new();
    let out = translate_progressively(&img, 400, 480, "ar", &Cache::new(64), &fake, |p| {
        shown.push((p.rgba.clone(), p.blocks.clone()));
    })
    .unwrap();
    let calls = fake.ai_calls.borrow().clone();
    // 2 sentences, then twice as many; a part in reading order
    let want: [&[&str]; 2] = [&["Load", "Save"], &["Quit", "Help", "Exit", "Undo"]];
    assert_eq!(calls, want);
    // the first part is on screen, drawn while the second call ran
    let (first, blocks) = &shown[0];
    let done: Vec<&str> = (blocks.iter())
        .filter(|b| !b.translation.is_empty())
        .map(|b| b.original.as_str())
        .collect();
    assert_eq!(done, ["Load", "Save"]);
    assert!(out.timings.first > Duration::ZERO && out.timings.first <= out.timings.total);
    // the whole page = one call for everything
    let whole = translate_frame(
        &img,
        400,
        480,
        "ar",
        &Cache::new(64),
        &Fake::new(move |_, _, _| lines()),
    )
    .unwrap();
    assert!(out.rgba == whole.rgba);
    assert_eq!(out.blocks, whole.blocks);
    // rows 0..140 (Load, Save) look the same early and at the end; the
    // rest is still the original in the early frame
    let row = 400 * 4;
    assert!(first[..140 * row] == out.rgba[..140 * row]);
    assert!(first[150 * row..] == img[150 * row..]);
    assert!(out.rgba[150 * row..] != img[150 * row..]);
}

/// A partial frame is drawn and shown while the next call runs, not
/// between the calls (gate 0.1.2: 3.04 s for a page, 0.2 s of it partial
/// renders waiting for each other).
#[test]
fn a_partial_frame_is_drawn_while_the_next_call_runs() {
    let lines = || {
        vec![
            tb("Load", [40.0, 40.0, 120.0, 30.0]),
            tb("Quit", [40.0, 300.0, 120.0, 30.0]),
        ]
    };
    let boxes: Vec<[f32; 4]> = lines().iter().map(|b| b.bounds).collect();
    let img = frame(400, 480, &boxes);
    let mut fake = Fake::new(move |_, _, _| lines());
    (fake.batch, fake.delay) = (1, Duration::from_millis(600));
    let mut shown = 0;
    let out = translate_progressively(&img, 400, 480, "ar", &Cache::new(8), &fake, |_| {
        std::thread::sleep(Duration::from_millis(600)); // a slow draw
        shown += 1;
    })
    .unwrap();
    assert_eq!((shown, fake.ai_calls.borrow().len()), (1, 2));
    // two calls of 600 ms, the 600 ms partial beside the second: ~1.2 s;
    // drawn in turn it would be 1.8 s (wide margin: CI runners are slow)
    let total = out.timings.total;
    assert!(total < Duration::from_millis(1600), "{total:?}");
}

/// The one call of a cloud model (and of any `Deps` without a batch):
/// nothing is shown early.
#[test]
fn one_call_shows_nothing_early() {
    let fake = Fake::new(|_, _, _| screen());
    let mut shown = 0;
    let img = frame(800, 450, &[]);
    translate_progressively(&img, 800, 450, "ar", &Cache::new(8), &fake, |_| shown += 1).unwrap();
    assert_eq!((shown, fake.ai_calls.borrow().len()), (0, 1));
    let ai = Real::new(
        "auto",
        Via::Ai {
            ai: AiConfig::default(),
            key: None,
        },
    );
    assert_eq!(ai.batch(), usize::MAX);
    assert_eq!(Real::new("auto", Via::Offline).batch(), FIRST);
}

/// A plain model reply (no JSON array) goes on-device, not to an error card
/// (gate 0.1.2): translated there, or that model's download asked for.
#[test]
fn an_unreadable_ai_reply_falls_back_to_on_device() {
    let url = crate::features::ask_ai::ask::tests::stub(&["Sure! Voici la traduction : Bonjour."]);
    let real = Real::new(
        "en",
        Via::Ai {
            ai: crate::features::ask_ai::ask::tests::local_setup(url).cfg,
            key: None,
        },
    );
    let done = real.lines(&["Hello".into()], "fr", "");
    assert!(!matches!(done, Err(AiError::BadResponse(_))), "{done:?}");
    assert!(done.is_ok() || real.missing().is_some(), "{done:?}");
}

/// #107: one-shot translate reads with Windows OCR where it reads the
/// source (as main mode and live do), PP-OCR otherwise.
#[test]
fn windows_ocr_reads_where_it_has_the_language() {
    use TextReader::{Auto, PpOcr, Windows};
    for (reader, tag, source, system) in [
        (Auto, Some("en-US"), "auto", true),
        (Auto, Some("en-US"), "en", true),
        (Auto, Some("en-GB"), "en-US", true),
        (Windows, Some("en-US"), "auto", true),
        (Auto, Some("en-US"), "fr", false), // its recognizer can't read French
        (Auto, Some("ar-SA"), "auto", false), // right-to-left: PP-OCR reads it better
        (Windows, Some("ar-SA"), "ar", true), // ... unless chosen
        (Auto, None, "auto", false),        // no recognizer (and Linux)
        (PpOcr, Some("en-US"), "auto", false),
    ] {
        assert_eq!(
            system_reads(reader, tag, source),
            system,
            "{reader:?} {tag:?} {source}"
        );
    }
}

#[test]
fn markup_reaches_render_and_is_stripped_from_the_block_list() {
    let lines = vec![tb("Bring the Sunstone", [40.0, 40.0, 300.0, 30.0])];
    let img = frame(400, 120, &[lines[0].bounds]);
    let mut fake = Fake::new(move |_, _, _| lines.clone());
    fake.answer = |_| "أحضر [[حجر الشمس]]".into();
    let out = translate_frame(&img, 400, 120, "ar", &Cache::new(8), &fake).unwrap();
    assert_eq!(out.blocks[0].translation, "أحضر حجر الشمس");
    let line = to_line(
        tb("Bring the Sunstone", [40.0, 40.0, 300.0, 30.0]),
        400,
        120,
    );
    let blocks = group_blocks(&[line]);
    let mut direct = img.clone();
    render(
        &mut direct,
        400,
        120,
        &blocks,
        &["أحضر [[حجر الشمس]]".into()],
        true,
    );
    assert!(out.rgba == direct, "the [[ ]] spans went to render as is");
    // and the real client asks for them
    ask_ai::translate_with(&["x".into()], "auto", "ar", MARKUP, |m| {
        assert!(m[0].text.ends_with(MARKUP));
        Ok(r#"["y"]"#.into())
    })
    .unwrap();
}

#[test]
fn ai_error_means_no_render_and_nothing_cached() {
    let mut fake = Fake::new(|_, _, _| screen());
    fake.fail_ai = true;
    let cache = Cache::new(8);
    let img = frame(800, 450, &[]);
    let err = translate_frame(&img, 800, 450, "ar", &cache, &fake)
        .err()
        .unwrap();
    assert!(matches!(err, Error::Ai(AiError::Server(503, _))), "{err}");
    assert_eq!(cache.get("Quit", "ar"), None);
    // a bad frame fails before OCR
    assert!(matches!(
        translate_frame(&img[4..], 800, 450, "ar", &cache, &fake),
        Err(Error::Ocr(OcrError::BadImage(_)))
    ));
    assert_eq!(fake.ocr_calls.borrow().len(), 1);
}

#[test]
fn echoed_text_is_left_alone() {
    let mut fake = Fake::new(|_, _, _| vec![tb("Eldrin", [10.0, 10.0, 80.0, 30.0])]);
    fake.answer = |s| format!("[[{s}]]");
    let img = frame(200, 60, &[[10.0, 10.0, 80.0, 30.0]]);
    let out = translate_frame(&img, 200, 60, "en", &Cache::new(8), &fake).unwrap();
    assert_eq!(out.blocks[0].translation, "");
    assert!(out.rgba == img);
}

/// Stops with a citation after them end a sentence; abbreviations and
/// numbers don't; a very long sentence is cut at its middle comma.
#[test]
fn sentences_end_after_citations_and_long_ones_are_halved() {
    let t = "Haumea as dwarf planets.[12] Four others: Orcus and Sedna.1131 Less \
                 massive are clouds.[el The U.S. Patent of Dr. Who is 4.6 Million.";
    assert_eq!(
        sentences(t),
        [
            "Haumea as dwarf planets.[12]",
            "Four others: Orcus and Sedna.1131",
            "Less massive are clouds.[el",
            "The U.S. Patent of Dr. Who is 4.6 Million."
        ]
    );
    let long = format!("{}, {}.", ["word"; 30].join(" "), ["more"; 30].join(" "));
    let parts = sentences(&long);
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert!(parts[0].ends_with("word,") && parts[1].starts_with("more"));
    let words = ["word"; 60].join(" ");
    assert_eq!(sentences(&words), [words], "no comma: whole");
}

/// Footnote marks (as Windows OCR read them on the matrix pages) leave
/// the model's input and come back after the translation; brackets that
/// are text stay.
#[test]
fn footnote_marks_are_kept_out_of_the_model() {
    let cases = [
        (
            "cacti,[2] which are well-suited",
            "cacti, which are well-suited",
            "[2]",
        ),
        (
            "normal indoor conditions.[5",
            "normal indoor conditions.",
            "[5",
        ),
        (
            "cultivated indoors[ ] for aesthetic",
            "cultivated indoors for aesthetic",
            "[ ]",
        ),
        ("(composed in 1786) [24]", "(composed in 1786)", "[24]"),
        ("French original[c]", "French original", "[c]"),
        ("Lyrics [edit]", "Lyrics [edit]", ""),
        (
            "Culture [ modifier I modifier le code]",
            "Culture [ modifier I modifier le code]",
            "",
        ),
        (
            "[la sug da glwag e.t_a gi.ve II]",
            "[la sug da glwag e.t_a gi.ve II]",
            "",
        ),
        ("[24]", "", "[24]"),
    ];
    for (s, body, marks) in cases {
        assert_eq!(footnotes(s), (body.to_string(), marks.to_string()), "{s}");
    }
}

/// An Arabic model never reads a line with no Arabic letter (Latin OCR
/// noise on an Arabic page: ar-de made "LILI" into "LOLI", "Jho" into
/// "Joho!"); numbers still go.
#[test]
fn a_line_not_in_the_source_script_stays() {
    let route = mt::route("ar", "de").unwrap();
    let sent = RefCell::new(Vec::new());
    let run = |t: &[String], _: &'static mt::Pair| {
        sent.borrow_mut().extend(t.iter().cloned());
        Ok(t.iter().map(|s| format!("{s} [de]")).collect())
    };
    let src: Vec<String> = ["نبات منزلي", "LILI", "Jho", "12"].map(String::from).into();
    let out = hops(&src, &route, "de", run).unwrap();
    assert_eq!(out, ["نبات منزلي [de]", "LILI", "Jho", "12 [de]"]);
    assert_eq!(*sent.borrow(), ["نبات منزلي", "12"]);
}

#[test]
fn script_and_direction() {
    assert!(!needs_translation("12:30 — 4/5 !", "ar"));
    assert!(!needs_translation("القائمة الرئيسية", "ar-SA"));
    assert!(needs_translation("Main menu", "ar"));
    assert!(needs_translation("Главное меню", "ar"));
    assert!(!needs_translation("Главное меню", "ru"));
    assert!(needs_translation("Main menu", "fr"));
    assert!(is_rtl("ar") && is_rtl("he-IL") && !is_rtl("en") && !is_rtl("fr"));
}

/// Offline: the pair follows the source (or the script, for auto); one
/// that doesn't exist fails before anything runs, and says AI can.
#[test]
fn offline_pairs_and_the_not_available_path() {
    let texts = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let auto = Real::new(lang::AUTO, Via::Offline);
    assert!(auto.offline() && auto.missing().is_none());
    let pair = |r: &Real, t: &[&str], target| {
        let route = r.routes(&texts(t), target)?.remove(0).0;
        Ok::<_, AiError>((route[0].source, route[route.len() - 1].target, route.len()))
    };
    assert_eq!(pair(&auto, &["Main menu"], "ar").unwrap(), ("en", "ar", 1));
    assert_eq!(
        pair(&auto, &["القائمة الرئيسية"], "en").unwrap(),
        ("ar", "en", 1)
    );
    assert_eq!(pair(&auto, &["メニュー"], "en").unwrap(), ("ja", "en", 1));
    // a batch of short headings alone reads as English; the whole page doesn't
    let page = Real::new(lang::AUTO, Via::Offline);
    page.read(&["Un faro es una torre de señalización luminosa situada en el litoral"]);
    assert_eq!(
        pair(&page, &["Artículo", "Discusión"], "en").unwrap().0,
        "es"
    );
    let es = ["Un faro es una torre de señalización luminosa situada en el litoral"];
    assert_eq!(pair(&auto, &es, "en").unwrap(), ("es", "en", 1));
    let same = auto
        .lines(&texts(&["The menu is open"]), "en", "")
        .unwrap_err();
    assert_eq!(
        same,
        AiError::AlreadyIn("English"),
        "a quiet note, not an error card"
    );
    assert_eq!(
        pair(&Real::new("fr", Via::Offline), &["x"], "en").unwrap(),
        ("fr", "en", 1)
    );
    // no direct model: through English, two models
    let it = Real::new("it", Via::Offline);
    assert_eq!(pair(&it, &["x"], "ar").unwrap(), ("it", "ar", 2));
    assert_eq!(
        pair(&it, &["x"], "es").unwrap(),
        ("it", "es", 1),
        "direct first"
    );
    for (real, t, target, from) in [
        (
            &Real::new("tr", Via::Offline),
            &["Olá"][..],
            "en",
            "Turkish",
        ),
        (
            &Real::new("fr", Via::Offline),
            &["Bonjour"][..],
            "ko",
            "French",
        ),
    ] {
        let err = real.lines(&texts(t), target, "").unwrap_err().to_string();
        assert!(
            err.contains(&format!("no local model from {from}")),
            "{err}"
        );
        assert!(err.contains("Choose AI"), "offers AI: {err}");
        assert!(real.missing().is_none(), "nothing to download for it");
    }
}

/// "auto" on a paused video (the owner's Sintel take, read as one-shot
/// translate reads it): an English player banner and page around a
/// Spanish subtitle. Before: one guess for the whole screen (English, and
/// the subtitle had none of the common Spanish words) and "the text is
/// already in English". Now each line that says what it is goes its own
/// way; the rest follows the page.
#[test]
fn a_foreign_subtitle_in_an_english_page_is_translated() {
    let texts = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let screen = [
        "To exit full screen, move mouse to top of screen or press Esc",
        "Esta hoja tiene un pasado oscuro.",
        "Sintel is an independently produced short film, initiated by the Blender",
        "Ha derramado mucha sangre inocente.",
    ];
    let real = Real::new(lang::AUTO, Via::Offline);
    real.read(&screen);
    let routes = real.routes(&texts(&screen), "en").unwrap();
    let got: Vec<_> = (routes.iter())
        .map(|(r, idx)| (r[0].source, r[r.len() - 1].target, idx.clone()))
        .collect();
    assert_eq!(got, [("es", "en", vec![1, 3])]);
    // a part with only the page's English: left as it is, no error
    assert!(real.routes(&texts(&screen[..1]), "en").unwrap().is_empty());
    // into Arabic, everything goes, each from its own language
    let routes = real.routes(&texts(&screen), "ar").unwrap();
    let mut got: Vec<_> = (routes.iter())
        .map(|(r, i)| (r[0].source, i.clone()))
        .collect();
    got.sort();
    assert_eq!(got, [("en", vec![0, 2]), ("es", vec![1, 3])]);
    // a page all in English still says so
    let en = [screen[0], screen[2]];
    let page = Real::new(lang::AUTO, Via::Offline);
    page.read(&en);
    let same = page.routes(&texts(&en), "en").unwrap_err();
    assert_eq!(
        same,
        AiError::AlreadyIn("English"),
        "a quiet note, not an error card"
    );
}

/// Ubuntu gate: a French page into German asked for Spanish -> German
/// (112 MB) for one line its words took for Spanish, and drew nothing
/// until it was downloaded. On a page clearly in French, a line in the
/// same script whose own models aren't here goes the page's way.
#[test]
fn a_stray_line_never_holds_the_page_for_a_download() {
    let fr = [
        "Le Système solaire est le système planétaire du Soleil.",
        "Il est composé de cette étoile et des objets célestes qui tournent autour.",
        "Les planètes telluriques sont les plus proches du Soleil.",
        "Esta hoja tiene un pasado oscuro.",
    ];
    let texts: Vec<String> = fr.iter().map(|s| s.to_string()).collect();
    assert_eq!(lang::sure(fr[3]), Some("es"));
    assert_eq!(lang::certain(fr[3]), Some("es"));
    let real = Real::new(lang::AUTO, Via::Offline);
    real.read(&fr);
    let es_here = (mt::route("es", "de").unwrap().iter()).all(|p| p.missing_bytes() == 0);
    let routes = real.routes(&texts, "de").unwrap();
    let got: Vec<_> = routes
        .iter()
        .map(|(r, i)| (r[0].source, i.clone()))
        .collect();
    if es_here {
        assert_eq!(got, [("fr", vec![0, 1, 2]), ("es", vec![3])]);
        assert!(real.held().is_none());
    } else {
        // the Spanish line stays as it is (before: translated as French)
        // and its pair is offered, the page isn't held
        assert_eq!(got, [("fr", vec![0, 1, 2])]);
        let (pair, lines) = real.held().expect("offered");
        assert_eq!((pair.source, lines), ("es", vec![texts[3].clone()]));
    }
    // a page that isn't clearly one language keeps each line's own way
    let half = [fr[0], fr[3]];
    let real = Real::new(lang::AUTO, Via::Offline);
    real.read(&half);
    let texts: Vec<String> = half.iter().map(|s| s.to_string()).collect();
    assert_eq!(real.routes(&texts, "de").unwrap().len(), 2);
}

/// A Spanish subtitle on an English video page into Arabic, es-ar not
/// downloaded: it was translated as English (nonsense). It stays as it
/// is, the page is translated at once, and es-ar is offered.
#[test]
fn a_line_in_a_language_not_downloaded_is_kept_and_offered() {
    let page = [
        "Sintel is an independently produced short film, initiated by the Blender",
        "Foundation as a means to further improve and validate the free and open",
        "To exit full screen, move mouse to top of screen or press Esc",
        "Subscribe to the channel for more of the films from the studio",
        "Esta hoja tiene un pasado oscuro.",
        "Known dwarf planets",
    ];
    let texts: Vec<String> = page.iter().map(|s| s.to_string()).collect();
    let real = Real::new(lang::AUTO, Via::Offline);
    real.read(&page);
    let es_here = (mt::route("es", "ar").unwrap().iter()).all(|p| p.missing_bytes() == 0);
    let routes = real.routes(&texts, "ar").unwrap();
    let got: Vec<_> = (routes.iter())
        .map(|(r, i)| (r[0].source, i.clone()))
        .collect();
    if es_here {
        assert_eq!(got, [("en", vec![0, 1, 2, 3, 5]), ("es", vec![4])]);
    } else {
        // the detector's "Known dwarf planets" (Spanish, unsure) goes the page's way
        assert_eq!(got, [("en", vec![0, 1, 2, 3, 5])]);
        let (pair, lines) = real.held().expect("offered");
        assert_eq!(
            (pair.source, pair.target, lines),
            ("es", "ar", vec![texts[4].clone()])
        );
    }
}

/// Ubuntu regression: an Italian line on a French page went through the
/// fr model ("sistema sole"). It is Italian (not Spanish, as its endings
/// voted), so it is kept and its pair offered when it-en isn't here.
#[test]
fn an_italian_line_on_a_french_page_is_not_french() {
    let page = [
        "Le Système solaire est le système planétaire du Soleil, étoile autour de laquelle tourne la planète Terre.",
        "Il est composé de cette étoile et des objets célestes gravitant autour d'elle, dont les huit planètes.",
        "Il sistema solare è formato dal Sole e da tutti i corpi celesti che orbitano attorno ad esso.",
        "Les planètes telluriques internes sont principalement composées de roches et de métaux.",
        "La ceinture d'astéroïdes sépare les planètes telluriques des planètes géantes gazeuses.",
    ];
    assert_eq!(lang::sure(page[2]), Some("it"));
    assert_eq!(lang::certain(page[2]), Some("it"));
    for l in [0, 1, 3, 4] {
        assert_eq!(lang::sure(page[l]), Some("fr"), "{}", page[l]);
    }
    let texts: Vec<String> = page.iter().map(|s| s.to_string()).collect();
    let real = Real::new(lang::AUTO, Via::Offline);
    real.read(&page);
    let it_here = (mt::route("it", "en").unwrap().iter()).all(|p| p.missing_bytes() == 0);
    let routes = real.routes(&texts, "en").unwrap();
    let got: Vec<_> = (routes.iter())
        .map(|(r, i)| (r[0].source, i.clone()))
        .collect();
    if it_here {
        assert_eq!(got, [("fr", vec![0, 1, 3, 4]), ("it", vec![2])]);
    } else {
        assert_eq!(got, [("fr", vec![0, 1, 3, 4])]);
        let (pair, lines) = real.held().expect("offered");
        assert_eq!((pair.source, lines), ("it", vec![texts[2].clone()]));
        // Windows VM: Edge's own card over the page ("Translate page
        // from French?") made the page unclear, and it waited on it-en
        let mut edge = page.to_vec();
        let ru = "Солнечная система включает Солнце и все небесные тела, вращающиеся вокруг него.";
        edge.extend([ru, "Translate page from French?", "Translate to", "English"]);
        let texts: Vec<String> = edge.iter().map(|s| s.to_string()).collect();
        let real = Real::new(lang::AUTO, Via::Offline);
        real.read(&edge);
        let routes = real.routes(&texts, "ar").unwrap();
        let got: Vec<_> = (routes.iter()).map(|(r, _)| r[0].source).collect();
        assert!(!got.contains(&"it"), "{got:?}");
        assert_eq!(real.held().expect("offered").0.source, "it");
    }
}

#[test]
fn cache_hit_miss_and_eviction() {
    let c = Cache::new(2);
    assert_eq!(c.get("a", "ar"), None);
    c.put("a", "ar", "A".into());
    c.put("b", "ar", "B".into());
    assert_eq!(c.get("  a ", "ar").as_deref(), Some("A"), "normalised");
    assert_eq!(c.get("a", "fr"), None, "keyed by target too");
    c.put("c", "ar", "C".into()); // full: a, b move to the old generation
    assert_eq!(c.get("a", "ar").as_deref(), Some("A")); // hit moves a back
    c.put("d", "ar", "D".into()); // full again: b is dropped
    assert_eq!(c.get("b", "ar"), None);
    for (k, v) in [("a", "A"), ("c", "C"), ("d", "D")] {
        assert_eq!(c.get(k, "ar").as_deref(), Some(v), "{k}");
    }
}

/// Real OCR (in process; models from `text_reading::models_dir()`), a fake
/// translator (fixed Arabic, else the text reversed), the synthetic 1080p
/// game frame; one-shot, then live steps. PNGs in the temp dir. Run:
/// cargo test --release e2e_real_ocr -- --ignored --nocapture
#[test]
#[ignore]
fn e2e_real_ocr_fake_translator() {
    use super::super::tests::{ar, game_frame, save_png};
    use crate::features::live_translate::live::Live;

    struct RealOcr(Mutex<text_reading::Engine>);
    impl Deps for RealOcr {
        fn recognize(&self, rgba: &[u8], w: u32, h: u32) -> Result<Vec<TextBlock>, OcrError> {
            let mut engine = self.0.lock().unwrap();
            engine.recognize(rgba, w, h, text_reading::Read::One(0))
        }
        fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
            Ok(texts
                .iter()
                .map(|t| ar(t).map_or_else(|| t.chars().rev().collect(), String::from))
                .collect())
        }
    }

    let t = Instant::now();
    let deps = RealOcr(Mutex::new(
        text_reading::Engine::load(&text_reading::models_dir()).unwrap(),
    ));
    crate::log!("OCR engine load {:?}", t.elapsed());
    let (img, _) = game_frame();
    let (w, h) = (1920, 1080);
    let cache = Cache::new(256);
    for run in 1..=2 {
        let out = translate_frame(&img, w, h, "ar", &Cache::new(256), &deps).unwrap();
        crate::log!("one-shot run {run}: {:?}", out.timings);
    }
    let out = translate_frame(&img, w, h, "ar", &cache, &deps).unwrap();
    for b in &out.blocks {
        crate::log!("{:?} {:?} -> {:?}", b.bounds, b.original, b.translation);
    }
    let dir = std::env::temp_dir().join("opencircle-pipeline");
    std::fs::create_dir_all(&dir).unwrap();
    save_png(&dir.join("before.png"), (1920, 1080), &img);
    save_png(&dir.join("after.png"), (1920, 1080), &out.rgba);
    let translated = out.blocks.iter().filter(|b| !b.translation.is_empty());
    assert!(translated.count() >= 10, "{:?}", out.blocks);

    // live: first frame reads everything (translations from the cache)
    let mut live = Live::new();
    let t = live.step(&img, w, h, "ar", &cache, &deps).unwrap().unwrap();
    crate::log!("live first frame: {t:?}, {} patches", live.patches().len());
    let t = Instant::now();
    assert!(
        live.step(&img, w, h, "ar", &cache, &deps)
            .unwrap()
            .is_none()
    );
    crate::log!("live unchanged frame: {:?}", t.elapsed());
    // X11: the capture shows our layer -> still nothing to do
    let mut shown = img.clone();
    let mut layer = img.clone();
    for p in live.patches() {
        let [x, y, pw, _] = p.bounds.map(|v| v as usize);
        for (row, src) in p.rgba.chunks_exact(pw * 4).enumerate() {
            let i = ((y + row) * 1920 + x) * 4;
            shown[i..i + pw * 4].copy_from_slice(src);
            layer[i..i + pw * 4].copy_from_slice(src);
        }
    }
    save_png(&dir.join("live_layer.png"), (1920, 1080), &layer);
    assert!(
        live.step(&shown, w, h, "ar", &cache, &deps)
            .unwrap()
            .is_none()
    );
    // the prompt "Press E to interact" goes away: only its region is read
    let mut next = img.clone();
    for y in 590..640 {
        for x in 800..1120 {
            let i = (y * 1920 + x) * 4;
            let src = (y * 1920 + 790) * 4;
            next.copy_within(src..src + 4, i);
        }
    }
    let n = live.blocks().len();
    let t = live
        .step(&next, w, h, "ar", &cache, &deps)
        .unwrap()
        .unwrap();
    crate::log!(
        "live, one block gone: {t:?}; blocks {n} -> {}",
        live.blocks().len()
    );
    assert!(
        live.blocks()
            .iter()
            .all(|b| !b.original.starts_with("Press"))
    );
    crate::log!("PNGs in {}", dir.display());
}

fn blocks_of(texts: &[&str]) -> Vec<Block> {
    (texts.iter().enumerate())
        .map(|(i, t)| {
            let l = Line {
                bounds: [10.0, 40.0 * i as f32, 300.0, 20.0],
                text: t.to_string(),
            };
            Block {
                text: l.text.clone(),
                bounds: l.bounds,
                lines: vec![l],
            }
        })
        .collect()
}

/// The owner's take (French Wikipedia -> en/de/ar/es): the address bar
/// came out as "en.wikipedia.org/wiki/Plante_d%27inside". Addresses,
/// paths, e-mail addresses, file names and identifiers are left exactly
/// as they are; a sentence that mentions one is still translated.
#[test]
fn addresses_and_code_are_never_translated() {
    for code in [
        "https://fr.wikipedia.org/wiki/Plante_d%27intérieur",
        "fr.wikipedia.org/wiki/Plante_d%27intérieur",
        "ar.wikipedia.org",
        "www.example.com",
        "jean.dupont@exemple.fr",
        "C:\\Users\\test\\Documents",
        "rapport_final.pdf",
        "photo.jpg",
        "translate_lines",
        "?q=plante&lang=fr",
    ] {
        assert!(code_like(code), "{code}");
        for target in ["en", "ar", "de"] {
            assert!(!needs_translation(code, target), "{code} -> {target}");
        }
    }
    for word in [
        "etc.",
        "U.S.",
        "e.g.",
        "50%",
        "d'intérieur",
        "Wikipédia",
        "1.5",
    ] {
        assert!(!code_like(word), "{word}");
    }
    let mention = "Voir la page fr.wikipedia.org pour la liste complète des plantes";
    assert!(needs_translation(mention, "en"));
    // the address bar as the OCR engines read it (Ubuntu PP-OCR, Windows
    // OCR on the 0.1.3 gate): a gap, the logo's word, icons in front
    for bar in [
        "fr.wikipedia.org/wiki/Système_solaire WIKIPÉDIA",
        "fr.wikipedia.org /wiki/Système solaire",
        "C' I https://fr.wikipedia.org/wikißystérne_solaire",
        "المجموعة_الشمسيةar.wikipedia.org/wiki/",
        "netwechyweki/Sun",
    ] {
        for target in ["en", "ar", "de"] {
            assert!(!needs_translation(bar, target), "{bar} -> {target}");
        }
    }
    // the fake translates everything it gets: the address never reaches it
    let fake = Fake::new(|_, _, _| Vec::new());
    let blocks = blocks_of(&["https://fr.wikipedia.org/wiki/Plante_d%27intérieur", "Lire"]);
    let tr = translate_blocks(&blocks, "en", &Cache::new(8), &fake).unwrap();
    assert_eq!(tr[0], "", "left as it was, not redrawn");
    assert_eq!(*fake.ai_calls.borrow(), [vec!["Lire".to_string()]]);
}

/// "[modifier | modifier le code]" came out as "[amend | modifier le
/// code]" (OCR reads the bar as "I": one sentence to MT). Each item is
/// translated alone and put back between its brackets and bars, a
/// lower-case label as a label ("Modifier"), so the same label reads the
/// same everywhere: the tab "Modifier" and the link "modifier".
#[test]
fn link_items_are_translated_one_by_one() {
    let s = |p: &str, t: bool| (p.to_string(), t);
    assert_eq!(
        items("Culture [ modifier I modifier le code]"),
        [
            s("Culture", true),
            s(" [ ", false),
            s("modifier", true),
            s(" | ", false),
            s("modifier le code", true),
            s("]", false),
        ]
    );
    assert_eq!(items("I think so").len(), 1, "no brackets: English I stays");
    let mut fake = Fake::new(|_, _, _| Vec::new());
    fake.answer = |s| match s {
        "Modifier" => "Edit".into(),
        "Modifier le code" => "Edit code".into(),
        "Culture" => "Culture".into(),
        "Lire" => "Read".into(),
        other => format!("?{other}?"),
    };
    let blocks = blocks_of(&[
        "Culture [ modifier I modifier le code]",
        "Lumière [ modifier | modifier le code ]",
        "Modifier",
        "Lire",
    ]);
    let tr = translate_blocks(&blocks, "en", &Cache::new(64), &fake).unwrap();
    assert_eq!(tr[0], "Culture [ edit | edit code]");
    assert_eq!(tr[1], "?Lumière? [ edit | edit code ]");
    assert_eq!((tr[2].as_str(), tr[3].as_str()), ("Edit", "Read"));
    // each label once, as a label
    let sent: Vec<String> = fake.ai_calls.borrow().concat();
    assert_eq!(
        sent,
        ["Culture", "Modifier", "Modifier le code", "Lumière", "Lire"]
    );
    // into Arabic the case doesn't matter; the bars and brackets stay
    fake.answer = |s| format!("ع{}", s.chars().count());
    let tr = translate_blocks(&blocks[..1], "ar", &Cache::new(64), &fake).unwrap();
    assert_eq!(tr[0], "ع7 [ ع8 | ع16]");
}

/// Through English (here German -> Arabic; French was too): a label the first model gives
/// back unchanged ("40 langues") never goes on to English -> Arabic,
/// which made a name of it ("إيان غوس 40"); one plain word may be English
/// already ("Text"); an output that isn't a translation keeps the original.
#[test]
fn a_pivot_never_makes_up_a_translation() {
    let route = mt::route("de", "ar").unwrap();
    assert_eq!(route.len(), 2, "German -> Arabic goes through English");
    let calls = RefCell::new(Vec::new());
    let run = |t: &[String], p: &'static mt::Pair| {
        calls.borrow_mut().push((p.target, t.to_vec()));
        Ok(t.iter()
            .map(|s| match (p.target, s.as_str()) {
                ("en", "Plante d'intérieur") => "Interior plant".into(),
                ("en", "Sommaire") => "Contents".into(),
                ("en", other) => other.into(),
                ("ar", "Interior plant") => "نبتة داخلية".into(),
                ("ar", "Contents") => "المحتويات المحتويات المحتويات المحتويات".into(),
                ("ar", "Text") => "نص".into(),
                ("ar", _) => "إيان غوس 40".into(),
                _ => unreachable!(),
            })
            .collect())
    };
    let src: Vec<String> = [
        "Plante d'intérieur",
        "40 langues",
        "Lumiere",
        "Sommaire",
        "Text",
    ]
    .map(String::from)
    .into();
    let out = hops(&src, &route, "ar", run).unwrap();
    assert_eq!(
        out,
        ["نبتة داخلية", "40 langues", "Lumiere", "Sommaire", "نص"]
    );
    let second = &calls.borrow()[1];
    assert_eq!(
        second.1,
        ["Interior plant", "Lumiere", "Contents", "Text"],
        "a phrase given back unchanged stops after English, one plain word goes on"
    );
}

/// fr-ar on the Win10 VM: Marian cut a 33-word sentence after its first
/// clause, and gave a stock phrase for another; both are tried again in
/// halves (before: the second stayed French, the first lost its end).
#[test]
fn a_long_sentence_cut_short_is_translated_in_halves() {
    let route = mt::route("fr", "ar").unwrap();
    assert_eq!(route.len(), 1);
    let cut = "II s'est formé il y a un peu moins de 4,6 milliards d'années à partir de \
                   l'effondrement gravitationnel d'un nuage moléculaire, suivi de la \
                   constitution d'un disque protoplanétaire selon l'hypothèse de la nébuleuse.";
    let stock = "ainsi que des milliards de petits corps (la presque totalité des \
                     astéroides et autres planetes mineures, les cométes, les poussiéres etc.).";
    let calls = RefCell::new(0);
    let run = |t: &[String], _: &'static mt::Pair| {
        *calls.borrow_mut() += 1;
        Ok(t.iter()
            .map(|s| match s {
                s if s == cut => "لقد تكوّن قبل 4.6 مليار سنة من انهيار سحابة،".into(),
                s if s == stock => "( انظر الفقرة في مستهل المقالة . )".into(),
                s if s.ends_with("nébuleuse.") => "ثم تكون قرص حسب فرضية السديم.".into(),
                s if s.starts_with("II") => {
                    "لقد تكوّن قبل 4.6 مليار سنة من انهيار سحابة جزيئية،".into()
                }
                s if s.starts_with("ainsi") => {
                    "وبلايين الأجسام الصغيرة (كلها تقريباً من الكويكبات".into()
                }
                _ => "المذنبات، الغبار الكوني، الخ.".into(),
            })
            .collect())
    };
    let src = [cut.to_string(), stock.to_string(), "Lumière".to_string()];
    let out = whole(&src, &route, "ar", run).unwrap();
    assert_eq!(
        out[0],
        "لقد تكوّن قبل 4.6 مليار سنة من انهيار سحابة جزيئية، ثم تكون قرص حسب فرضية السديم."
    );
    assert_eq!(
        out[1],
        "وبلايين الأجسام الصغيرة (كلها تقريباً من الكويكبات المذنبات، الغبار الكوني، الخ."
    );
    assert_eq!(*calls.borrow(), 2, "one more call for all the halves");
    // fr-es: its only comma at the end; cut before the bracket instead
    let s = "Le Système solaire fait partie de la galaxie appelée Voie lactée (parfois « la \
                 Galaxie ») où il réside dans le bras d'Orion, est situé à environ 8 kpc du \
                 centre galactique,";
    let [a, b] = super::cut(s).unwrap();
    assert!(
        a.ends_with("bras d'Orion,") && b.starts_with("est situé"),
        "{a} | {b}"
    );
    let s = "Il est situé à environ 8 kpc (26 100 al) du centre galactique,";
    let [a, b] = super::cut(s).unwrap();
    assert_eq!(
        (a.as_str(), b.as_str()),
        (
            "Il est situé à environ 8 kpc",
            "(26 100 al) du centre galactique,"
        )
    );
}

#[test]
fn made_up_outputs_are_told_apart() {
    // offline outputs on the matrix pages before this check
    assert!(made_up("- .Jj.i.o Ü14j X", &"- ".repeat(30), "en"));
    let looped = format!("Ül-{}", "j".repeat(40));
    assert!(made_up("Ül-jß.o 'po ü4.JJj-A.Jl", &looped, "en"));
    assert!(made_up("40 langues", "إيان غوس 40 عام 1990", "ar"));
    // ar-en on the Win10 VM (ar.wikipedia "شمس", PP-OCR): syllables
    let sun = "The sun of the yahoo-hoo-hoo-hoo-hoo-hoo!";
    assert!(made_up("شمس الياهو", sun, "en"), "hyphenated loop");
    assert!(made_up("شمس الياهو", "The sun hoohoohoo", "en"), "chant");
    assert!(!made_up("Hahaha, dit-il", "Hahaha, he said", "en"));
    assert!(!made_up("bien-être", "well-being", "en"));
    assert!(made_up("40 langues", "أربعون لغة", "ar"), "a number lost");
    assert!(
        made_up("Standard", "Standard", "ar"),
        "not in Arabic letters"
    );
    assert!(made_up("Sommaire", "目次", "en"), "another script");
    // fr-ar on the Win10 VM (Edge, "Système solaire")
    assert!(made_up("247 langues •v", "247 لغة □ ت", "ar"), "a symbol");
    let title = "Systéme solaire — Wikipédia";
    assert!(made_up(title, "Sonnensystem ✓ Wikipedia", "de"), "a symbol");
    let s = "Vous Ilsez un « article de qualité » labemsé en 2021";
    assert!(made_up(s, "Usted Elige un", "es"), "fr-es: most of it lost");
    let menu = "Faire un don Créer un compte Se connecter";
    assert!(!made_up(menu, "Spenden Registrieren Einloggen", "de"));
    let s = "ainsi que des milliards de petits corps (la presque totalité des astéroides \
                 et autres planetes mineures, les cométes, les poussiéres cosmiquesv etc.).";
    assert!(
        made_up(s, "( انظر الفقرة في مستهل المقالة . )", "ar"),
        "lost"
    );
    let s = "II est composé de cette étoile et des objets célestes gravitant autour d'elle : \
                 les huit planetes confirmées accompagnées de plus de deux cents satellites";
    let t = "ويتألف من هذا النجم والأجسام السماوية المحيطة به: كوكبات الأرض الثمانية \
                 المؤكدة، التي يزيد عدد السواتل الطبيعية المعروفة عنها على 200 ساتل";
    assert!(!made_up(s, t, "ar"), "a sentence's number may be spelled");
    // Ubuntu: a Russian line read by the Latin reader, through en-ar
    let noise = "CnH ce BKn CN n Be He6eb Ten, BaLL Bpr Hero.";
    let out = "\"سن ه ه س س ب ن ن ن ن ه ه ه ه ه ع 10، بطل بلال ب ب ب.";
    assert!(made_up(noise, out, "ar"), "letters strewn as words");
    assert!(!needs_translation(noise, "ar") && !needs_translation(noise, "en"));
    // ar.wikipedia's radio buttons and the title's ⊙ read as "O" (PP-OCR)
    for icon in ["O صغير", "Oداكن", "الشُمْسO", "صغير o"] {
        assert!(!needs_translation(icon, "en"), "{icon}");
    }
    for fine in ["صغير", "الفئة G2V من النجوم", "OK", "O Canada"] {
        assert!(needs_translation(fine, "en"), "{fine}");
    }
    for fine in [
        "The iPhone and the MacBook are made by Apple",
        "Il est composé de cette étoile et des objets célestes",
        "B2B sales grew in 2024",
    ] {
        assert!(needs_translation(fine, "ar"), "{fine}");
    }
    let t = "و هي مكونة من هذا النجم و من أجسام سماوية تدور حوله و من بينها ثمانية كواكب";
    assert!(!made_up(
        "Il est composé de cette étoile et des objets célestes",
        t,
        "ar"
    ));
    // good ones
    assert!(!made_up("40 langues", "40 لغة", "ar"));
    assert!(!made_up("1:47 AM", "1: 47 صباحاً", "ar"));
    assert!(!made_up(
        "(novembre 2020)",
        "(تشرين الثاني/نوفمبر ٢٠٢٠)",
        "ar"
    ));
    assert!(!made_up("Plante d'intérieur", "Planta de interior", "es"));
    let caption = "Aquarelle de Carl Larsson (1894)";
    assert!(!made_up(caption, "لون الماء لكارل لارسون (1894)", "ar"));
    assert!(echo("40 langues •v", "40 Langues v"));
    // digits in a word OCR mangled are no number (es-ar on the matrix)
    let src = "cultivada en lugares bajo tech07 como casas u oficinas.";
    assert!(!made_up(
        src,
        "مزروعة في أماكن منخفضة التقنية مثل المنازل أو المكاتب.",
        "ar"
    ));
}

/// PP-OCR with the Latin and English readers only (Edge, Win10 VM,
/// 1024x768): the Arabic and Russian Wikipedia pages read as noise, the
/// French one as text.
#[test]
fn a_page_no_reader_reads_is_told_from_text() {
    let read = |t: &[(&str, f32)]| -> Vec<TextBlock> {
        (t.iter())
            .map(|&(text, confidence)| TextBlock {
                confidence,
                ..tb(text, [0.0, 0.0, 10.0, 10.0])
            })
            .collect()
    };
    let arabic = read(&[
        ("w", 0.88),
        ("L", 0.62),
        ("l", 0.68),
        ("X", 0.99),
        ("C", 0.98),
        ("https://ar.wikipedia.org/wiki/_...úJl_c.g.aJl", 0.88),
        ("=", 0.38),
        ("InPrivate", 1.0),
        ("Translate page from Arabic?", 0.98),
        ("6d Q", 0.79),
        ("Translate to", 0.96),
        ("C", 0.33),
        ("English", 1.0),
        ("Translate", 1.0),
        ("Not now", 1.0),
        ("247", 0.85),
        ("W", 0.32),
        ("W Jic La", 0.61),
        ("Jih Ths", 0.38),
        ("C", 0.32),
        ("[2] [1", 0.69),
        ("(Solar System :)", 0.92),
        ("Us", 0.36),
        ("515", 0.61),
        ("Jhl  4  4 i", 0.52),
        ("x5", 0.32),
        ("all1-ll:A", 0.59),
        ("1:150", 0.82),
        ("10:16 AM", 1.0),
        ("11/3/2026", 1.0),
    ]);
    assert!(unreadable(&arabic));
    let russian = read(&[
        ("ConHe4Hag cuctema — BuknneAM X", 0.78),
        ("¤", 0.23),
        ("X", 0.99),
        ("https://ru.wikipedia.org/wiki/CouHe4Haa_cncTema", 0.93),
        ("InPrivate", 1.0),
        ("BN", 0.34),
        ("Translate page from Russian?", 0.98),
        ("C3 4 3 B", 0.85),
        ("C06oHa 3HuKrnua", 0.69),
        ("247 A3bIKOB", 0.86),
        ("Translate", 1.0),
        ("[npabutb ko]", 0.82),
        ("CóNHeHA TéMA", 0.72),
        ("N  n6 B", 0.73),
        ("LeHpaJbyo 3Be3Δy CoJHLe n", 0.69),
        ("BCe eCTeCTBeHHbIe KoCMNYECKNE", 0.78),
        ("o6beKTbl Ha rnx", 0.67),
        ("op6ntax. OHa cφopnpoasacb", 0.83),
        ("ConHue", 0.86),
        ("ra3onbiJeBoro o6saKa npnmepo", 0.82),
        ("3eM/ Mep", 0.6),
        ("HenTyH", 0.95),
        ("IntTep", 0.63),
        ("i", 0.15),
        ("10:17 AM", 1.0),
        ("2", 0.19),
    ]);
    assert!(unreadable(&russian));
    // Firefox on the Ubuntu VM (1920x1440): the noise reads as words
    let firefox = read(&[
        ("ar.wikipedia.org/wiki/_acgl", 0.89),
        ("Try private translations in Firefox", 1.0),
        ("Translate from", 1.0),
        ("Arabic", 1.0),
        ("skiall", 0.73),
        ("uaill", 0.9),
        ("dosal", 0.68),
        ("Solar:) 2", 0.85),
        ("mail garall", 0.62),
        ("billg ll", 0.82),
        ("bwgll", 0.89),
        ("aill agarall", 0.69),
        ("iaill pu", 0.48),
        ("agill yg aiall?", 0.61),
        ("blial lei", 0.83),
        ("al gai", 0.65),
        ("quaill acgarall", 0.76),
        ("ll illluo", 0.64),
        ("jhas", 0.61),
    ]);
    assert!(unreadable(&firefox));
    let french = read(&[
        ("W", 0.54),
        ("Système solaire — Wikipédia", 1.0),
        ("X", 0.57),
        ("X", 1.0),
        ("https://fr.wikipedia.org/wiki/Système_solaire", 0.99),
        ("InPrivate", 0.97),
        (":", 0.72),
        ("WIKIPÉDIA", 0.97),
        ("Q 6d", 0.8),
        ("Faire un don Créer un compte Se connecter", 0.99),
        ("L'encyclopédie libre", 1.0),
        ("Système solaire", 1.0),
        ("247 langues", 1.0),
        ("Lire Modifier Modifier le code Voir I'historique", 0.98),
        ("W", 0.92),
        (
            "Cet article possède un paronyme, voir Système scolaire.",
            0.98,
        ),
    ]);
    assert!(!unreadable(&french));
    assert!(!unreadable(&arabic[..8]), "too little to judge");
    // Ubuntu, Firefox: an Arabic page the Arabic reader read well (its
    // icons read unsurely) was taken for noise, and the readers offered
    let read_well = read(&[
        ("U", 0.15),
        ("Nov 3 23:48", 0.95),
        ("0 0 *", 0.59),
        ("المجموعة الشمسية", 0.96),
        ("= Σ θ", 0.51),
        ("s{ ZA", 0.48),
        ("file://home/test/trn/ar.html", 0.99),
        ("c → < □", 0.47),
        ("النظام الشمسي", 0.85),
        ("A", 0.49),
        (
            "النظام الشمسي هو نظام كوكبي يضم الشمس وجميع الأجرامالتي تدور",
            0.86,
        ),
        ("?", 0.59),
        ("والغبار الكوني وسحب الغاز البعيدة.", 0.96),
        (
            "يتكون من ثمانية كواكب رئيسية، أقربها إلى الشمس عطارد وأبعدها",
            0.98,
        ),
        ("الكواكب الغازية", 1.0),
        (
            "المشتري وزحل هما أكبر الكواكب حجماً أما أورانوس ونبتون فهما",
            0.93,
        ),
        ("C", 0.42),
    ]);
    assert!(!unreadable(&read_well));
}

/// Windows OCR (English) on the Arabic Wikipedia page read noise: nothing
/// was translated into German, garbage into English. Such a read goes
/// to PP-OCR.
#[test]
fn a_page_windows_ocr_cannot_read_is_read_again() {
    let read = |t: &[&str]| -> Vec<TextBlock> {
        t.iter().map(|t| tb(t, [0.0, 0.0, 10.0, 10.0])).collect()
    };
    // lines of the real Windows OCR read (Win10 VM, Edge)
    let arabic = read(&[
        "d-l.-lss '4.3.1>-0-11 a-LI.>I-LII",
        "JjLi-AJl clio vol-LOJI",
        "axlj-i-o-ll üL.iLLLll",
        "Q! j ÜIju-6 öJlzll Q-ISLOJI ülju-i-ll p JS-üujs",
        "Qllj! 44-1>-0-11 ül-il-ü-ll ü.K.Aij -Lö I-OS",
        "Oil Iii",
    ]);
    assert!(misread(&arabic));
    let french = read(&[
        "Les plantes d'intérieur ou plantes d'appartement sont des plantes cultivées",
        "Faire un don Créer un compte Se connecter",
        "https://fr.wikipedia.org/wiki/Plante_d%27intérieur",
        "Culture [ modifier I modifier le code]",
    ]);
    assert!(!misread(&french));
    assert!(!misread(&read(&["OK", "1:47 AM"])), "too little to judge");
    // the address bar stays (Edge, a local file): it anchors the tabs
    let (keep, _) = words_read(
        read(&[
            "File C:/oc/trn/ar.html",
            "JjLi-AJl clio vol-LOJI",
            "Translate page from Arabic?",
        ]),
        Some("en-US"),
    );
    let keep: Vec<&str> = keep.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        keep,
        ["File C:/oc/trn/ar.html", "Translate page from Arabic?"]
    );
}
