//! Translate in place: its sessions, partials, cards and picker pill.
use super::*;
use crate::ocr;
use crate::ui::widgets;
use iced::widget::container as c;

fn block(text: &str, tr: &str, bounds: [f32; 4]) -> Translation {
    Translation {
        bounds,
        original: text.into(),
        translation: tr.into(),
    }
}

fn frame(w: u32, h: u32) -> image::Handle {
    image::Handle::from_rgba(w, h, vec![7u8; (w * h * 4) as usize])
}

fn shown(blocks: Vec<Translation>) -> Msg {
    Msg::Done(Ok(Shown {
        image: frame(4, 4),
        blocks,
    }))
}

/// A session as `start` leaves it, without a thread.
fn session(models: bool) -> Session {
    Session {
        frame: frame(200, 100),
        area: [0, 0, 200, 100],
        whole: true,
        phase: if models {
            Phase::Translating
        } else {
            Phase::Consent(None)
        },
        peek: false,
        card: None,
        cache: Arc::new(Cache::new(8)),
        work: None,
        offline: None,
        picker: Picker::new("auto", "ar", false),
        scale: 1.0,
        partial: None,
    }
}

/// A redraw of part of the screen (a hovered button, the progress bar)
/// leaves the rest as it was. Before: the card's shadow was drawn whole,
/// outside the redrawn part too (tiny-skia doesn't clip shadows), so it
/// piled up into a black block over the download prompt (VMs, 1125x575
/// and 1280x800).
#[test]
fn a_partial_redraw_leaves_the_rest_of_the_download_prompt() {
    let mut s = session(false);
    s.phase = Phase::Consent(Some((crate::mt::pair("en", "ar").unwrap(), 160_000_000)));
    for (w, h) in [(1280.0, 800.0), (1125.0, 575.0)] {
        let size = Size::new(w, h);
        let at = card::place(size, None, None);
        // the Download button's row
        let damage = Rectangle::new(Point::new(at.x + 20.0, 150.0), Size::new(140.0, 60.0));
        let (first, again) =
            widgets::shot::render_twice(s.view(size, at, false), w as u32, h as u32, Some(damage));
        let outside = |i: usize| {
            let p = Point::new((i % w as usize) as f32, (i / w as usize) as f32);
            !damage.expand(1.0).contains(p)
        };
        let changed = (first.chunks(4).zip(again.chunks(4)).enumerate())
            .filter(|&(i, (a, b))| a != b && outside(i))
            .count();
        assert_eq!(changed, 0, "{w}x{h}: {changed} pixels changed outside");
    }
}

/// The download question, its progress and an error are short cards:
/// as tall as their text and buttons, on any screen down to 1125x575 (a
/// narrow one too), every text and button inside the window, nothing
/// painted outside the card but its shadow. Before: a full-height slab
/// of empty dark panel with a few lines at its top (the "black block").
#[test]
fn the_prompts_are_short_cards_on_small_screens() {
    let mut s = session(false);
    let phases = [
        ("consent", Phase::Consent(None)),
        (
            "offline consent",
            Phase::Consent(Some((crate::mt::pair("en", "ar").unwrap(), 160_000_000))),
        ),
        ("downloading", Phase::Downloading(0.4)),
        (
            "failed",
            Phase::Failed(problem(
                "The AI provider has a problem right now (HTTP 503). Try again in a minute, \
                 or pick another provider in Settings."
                    .into(),
                true,
            )),
        ),
    ];
    let backdrop = iced::Color::from_rgb8(240, 240, 240);
    for (name, phase) in phases {
        s.phase = phase;
        for (w, h) in [
            (1125.0, 575.0),
            (1280.0, 800.0),
            (1920.0, 1080.0),
            (700.0, 500.0),
            (600.0, 1000.0),
        ] {
            let size = Size::new(w, h);
            let at = card::place(size, None, None);
            let lost = widgets::shot::out_of_reach(s.view(size, at, false), size);
            assert!(lost.is_empty(), "{name} at {w}x{h}: out of reach {lost:?}");
            let view =
                c(s.view(size, at, false)).style(move |_| c::Style::default().background(backdrop));
            let px = widgets::shot::save(
                &format!("prompt-{}-{w}x{h}", name.replace(' ', "-")),
                view.into(),
                size,
            );
            let at_px = |x: f32, y: f32| &px[((y as usize * w as usize) + x as usize) * 4..][..3];
            let dark = |p: &[u8]| p.iter().all(|&v| v < 100);
            // the card's rows: dark panel at a point right of its middle
            // near its left edge: the download card is narrower than the spot
            let x = at.x + 10.0;
            let rows: Vec<f32> = (0..h as usize)
                .map(|y| y as f32)
                .filter(|&y| dark(at_px(x, y)))
                .collect();
            assert!(!rows.is_empty(), "{name} at {w}x{h}: no card");
            let (top, bottom) = (rows[0], rows[rows.len() - 1]);
            assert!(
                bottom - top < 330.0 && bottom - top < 0.7 * h,
                "{name} at {w}x{h}: a slab {}px tall",
                bottom - top
            );
            // ink down to its last quarter at least: no big empty panel
            let panel = at_px(x, top + 40.0).to_vec();
            let last = (top as usize..=bottom as usize - 6)
                .filter(|&y| {
                    (at.x as usize + 24..(at.x + at.width) as usize - 24)
                        .any(|px_x| at_px(px_x as f32, y as f32) != &panel[..])
                })
                .max()
                .unwrap();
            assert!(
                bottom - last as f32 <= 80.0,
                "{name} at {w}x{h}: {}px of empty panel under the content",
                bottom - last as f32
            );
            // nothing outside the card, 14 px for its shadow, but the backdrop
            let b = backdrop.into_rgba8();
            for (qx, qy) in [(at.x - 20.0, top + 30.0), (x, bottom + 20.0)] {
                if qx >= 0.0 && qy < h {
                    assert_eq!(at_px(qx, qy), &b[..3], "{name} at {w}x{h}: ({qx}, {qy})");
                }
            }
        }
    }
}

#[test]
fn translating_then_shown_then_peek() {
    let mut s = session(true);
    assert!(s.full());
    assert!(matches!(&s.phase, Phase::Translating));
    assert!(s.image().is_none(), "the original until it is ready");
    let _ = s.update(shown(vec![block("Quit", "خروج", [10.0, 10.0, 50.0, 20.0])]));
    assert!(matches!(&s.phase, Phase::Shown(_)));
    let translated = s.image().cloned().unwrap();
    let _ = s.update(Msg::Peek(true));
    assert!(s.image().is_none(), "Alt: the original");
    let _ = s.update(Msg::Peek(false));
    assert_eq!(s.image(), Some(&translated));
    // a late result changes nothing
    let _ = s.update(Msg::Done(Err(problem("late".into(), false))));
    assert_eq!(s.image(), Some(&translated));
}

/// #27: what is translated so far shows while the rest is translated;
/// an error afterwards goes back to the original.
#[test]
fn parts_show_while_translating() {
    let mut s = session(true);
    let part = Part {
        image: frame(2, 2),
        area: [1, 1, 3, 3],
    };
    let _ = s.update(Msg::Partial(part.clone()));
    assert!(
        s.image().is_none(),
        "the frame stays; the part goes over it"
    );
    assert_eq!(s.part(), Some((&part.image, [1, 1, 3, 3])));
    let _ = s.update(Msg::Peek(true));
    assert!(s.part().is_none(), "Alt: the original");
    let _ = s.update(Msg::Peek(false));
    let _ = s.update(Msg::Done(Err(problem("helper stopped".into(), false))));
    assert!(s.part().is_none(), "failed: the original");
    let _ = s.update(Msg::Partial(part));
    assert!(s.part().is_none(), "a late part changes nothing");
}

#[test]
fn errors_keep_the_original_frame() {
    for p in [
        problem("AI is not set up yet: no API key is saved.".into(), true),
        problem(
            "The AI provider has a problem right now (HTTP 503).".into(),
            false,
        ),
    ] {
        let mut s = session(true);
        let _ = s.update(Msg::Done(Err(p.clone())));
        assert!(matches!(&s.phase, Phase::Failed(f) if *f == p));
        assert!(s.image().is_none(), "nothing painted");
        s.click(Point::new(20.0, 20.0));
        assert!(!s.has_card());
    }
}

#[test]
fn missing_models_ask_first() {
    let mut s = session(false);
    assert!(matches!(&s.phase, Phase::Consent(None)));
    // nothing runs before the user agrees
    let _ = s.update(shown(vec![]));
    assert!(matches!(&s.phase, Phase::Consent(None)));
    let _ = s.update(Msg::Progress(0.5));
    assert!(matches!(&s.phase, Phase::Consent(None)));
    let _ = s.update(Msg::Offline(en_ar(), 1));
    assert!(
        matches!(&s.phase, Phase::Consent(None)),
        "only while translating"
    );
}

fn en_ar() -> &'static Pair {
    crate::mt::pair("en", "ar").unwrap()
}

#[test]
fn offline_models_are_asked_for_with_their_size() {
    // the thread found translation will be offline, without its models
    let mut s = session(true);
    let _ = s.update(Msg::Offline(en_ar(), 117_600_822));
    assert!(matches!(&s.phase, Phase::Consent(Some((_, 117_600_822)))));
    assert!(s.work.is_none(), "the thread is done");
    let texts = widgets::shot::texts(s.view(
        Size::new(1920.0, 1080.0),
        card::place(Size::new(1920.0, 1080.0), None, None),
        false,
    ));
    assert!(
        texts
            .iter()
            .any(|t| t == "Offline translation · 118 MB · from Hugging Face"),
        "{texts:?}"
    );
    for button in ["Download", "Use an AI provider instead", "Not now"] {
        assert!(texts.iter().any(|t| t == button), "{button}: {texts:?}");
    }
    // Download: that pair is the one fetched
    s.offline = Some(en_ar());
    s.phase = Phase::Downloading(0.3);
    let texts = widgets::shot::texts(s.view(
        Size::new(1920.0, 1080.0),
        card::place(Size::new(1920.0, 1080.0), None, None),
        false,
    ));
    assert!(texts.iter().any(|t| t == "Downloading offline translation"));
}

/// A download that failed says why on the download card and offers Try
/// again, which fetches the same models (not a translate error card).
#[test]
fn a_failed_download_offers_try_again() {
    let mut s = session(true);
    s.offline = Some(en_ar());
    s.phase = Phase::Downloading(0.3);
    let _ = s.update(Msg::DownloadFailed("No internet connection.".into()));
    assert!(
        matches!(&s.phase, Phase::DownloadFailed(_)),
        "{:?}",
        s.phase
    );
    let size = Size::new(1920.0, 1080.0);
    let texts = widgets::shot::texts(s.view(size, card::place(size, None, None), false));
    for want in [
        "Download could not finish",
        "No internet connection.",
        "Try again",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    let _ = s.update(Msg::Download);
    assert!(matches!(s.phase, Phase::Downloading(_)));
    assert!(
        s.offline.is_some_and(|p| std::ptr::eq(p, en_ar())),
        "the same pair again"
    );
}

/// Light theme over a dark frame: the consent and the download progress
/// are opaque panels (`panel_solid`), their text the theme's; before,
/// the see-through glass turned dark there and hid the label.
#[test]
fn notes_are_opaque_in_the_light_theme() {
    use iced::widget::container as c;
    crate::ui::theme::set_light(true);
    let palette = crate::ui::theme::current();
    let (text, solid) = (palette.text, crate::ui::kit::themes::c().widget);
    let mut s = session(false);
    for phase in [Phase::Consent(None), Phase::Downloading(0.5)] {
        s.phase = phase;
        let dark = c(s.view(
            Size::new(720.0, 300.0),
            card::place(Size::new(720.0, 300.0), None, None),
            false,
        ))
        .style(|_| c::Style::default().background(iced::Color::from_rgb8(10, 12, 30)));
        let px = widgets::shot::render(dark.into(), 720, 300);
        let at = |x: usize, y: usize| &px[(y * 720 + x) * 4..][..3];
        let want = solid.into_rgba8();
        assert_eq!(at(360, 262), &want[..3], "the panel is solid");
        // and the label is drawn in the theme's text colour
        let ink = text.into_rgba8();
        let near = |p: &[u8]| p.iter().zip(&ink).all(|(a, b)| a.abs_diff(*b) < 40);
        assert!(px.chunks(4).any(near), "a visible label");
    }
    crate::ui::theme::set_light(false);
}

/// The "Translating…" pill holds the picker; a new language translates
/// again (here: its recognizer is missing, so it asks first). PNGs via
/// the headless renderer (`OPENCIRCLE_RENDER_DIR=<dir>`).
#[test]
fn the_pill_picks_languages_and_translates_again() {
    use crate::ui::picker::{Msg as P, Side};
    let mut s = session(true);
    let size = Size::new(720.0, 420.0);
    let texts = widgets::shot::texts(s.view(size, card::place(size, None, None), false));
    for t in ["Detect", "Arabic", "Translating…"] {
        assert!(texts.iter().any(|x| x == t), "{t}: {texts:?}");
    }
    widgets::shot::save(
        "translate-pill",
        s.view(size, card::place(size, None, None), false),
        size,
    );
    let _ = s.update(Msg::Picker(P::Open(Side::Target)));
    widgets::shot::save(
        "translate-pill-picker",
        s.view(size, card::place(size, None, None), false),
        size,
    );
    let texts = widgets::shot::texts(s.view(size, card::place(size, None, None), false));
    assert!(texts.iter().any(|t| t == "English"), "{texts:?}");
    // the same target again: nothing restarts
    let _ = s.update(Msg::Picker(P::Pick("ar".into())));
    assert!(matches!(s.phase, Phase::Translating));
    // a shown translation keeps the pill; a new target starts over
    let _ = s.update(shown(vec![block("Quit", "خروج", [10.0, 10.0, 50.0, 20.0])]));
    let texts = widgets::shot::texts(s.view(size, card::place(size, None, None), false));
    assert!(texts.iter().any(|t| t == "Arabic") && !texts.iter().any(|t| t == "Translating…"));
    let _ = s.update(Msg::Picker(P::Open(Side::Source)));
    let _ = s.update(Msg::Picker(P::Query("kyr".into())));
    assert!(!s.picker.update(P::First), "nothing matches: no change");
    let _ = s.update(Msg::Picker(P::Query("".into())));
    // a source whose recognizer is (in this sandbox) missing: ask first
    let _ = s.update(Msg::Picker(P::Pick("zh".into())));
    assert_eq!(s.picker.label(), "ZH → AR");
    let ready = models::installed(models::recognizer_for("zh"));
    assert!(ready || matches!(s.phase, Phase::Consent(None)));
    assert!(s.image().is_none(), "the old translation is gone");
}
/// OCR finds one line at (20, 10) of what it is given; AI answers or fails.
struct Fake(Result<&'static str, AiError>);
impl Deps for Fake {
    fn recognize(&self, _: &[u8], _: u32, _: u32) -> Result<Vec<ocr::TextBlock>, ocr::OcrError> {
        Ok(vec![ocr::TextBlock {
            text: "Quit".into(),
            bounds: [20.0, 10.0, 40.0, 20.0],
            confidence: 0.9,
        }])
    }
    fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
        let answer = self.0.clone()?;
        Ok(texts.iter().map(|_| answer.to_string()).collect())
    }
}

fn pixels(h: &image::Handle) -> &[u8] {
    match h {
        image::Handle::Rgba { pixels, .. } => pixels,
        _ => unreachable!(),
    }
}

#[test]
fn the_result_is_pasted_in_place_or_nothing_is() {
    // "glyphs" where OCR finds "Quit": (20, 10) of the area
    let mut px = vec![7u8; 200 * 100 * 4];
    for y in 64..76 {
        for x in (120..160).step_by(3) {
            px[(y * 200 + x) * 4..][..3].copy_from_slice(&[230; 3]);
        }
    }
    let original = image::Handle::from_rgba(200, 100, px);
    let cache = Cache::new(8);
    let area = [100, 50, 200, 100];
    let ok = |a: Result<&'static str, AiError>| Ok((Fake(a), "ar".to_string()));

    // no key: the steps' first line, Settings offered
    let no_key: Result<(Fake, String), String> =
        Err("AI is not set up yet: no API key is saved.\n\n1. ...".into());
    let e = translate(&original, area, &cache, &no_key, |_| {}).unwrap_err();
    let first = "AI is not set up yet: no API key is saved.";
    assert_eq!(e, problem(first.into(), true));
    // the provider fails: an error, nothing painted; a rejected key: Settings
    let e = translate(
        &original,
        area,
        &cache,
        &ok(Err(AiError::Server(503))),
        |_| {},
    )
    .unwrap_err();
    assert!(!e.setup && e.message.contains("503"), "{e:?}");
    let e = translate(
        &original,
        area,
        &cache,
        &ok(Err(AiError::Auth(401))),
        |_| {},
    )
    .unwrap_err();
    assert!(e.setup);

    let shown = translate(&original, area, &cache, &ok(Ok("خروج")), |_| {}).unwrap();
    // blocks in frame pixels: OCR saw the crop, offset by the area's corner
    let b = &shown.blocks[0];
    assert_eq!(
        (b.original.as_str(), b.translation.as_str()),
        ("Quit", "خروج")
    );
    assert!((b.bounds[0] - 118.0).abs() < 0.01 && (b.bounds[1] - 57.0).abs() < 0.01);
    // only the area changed
    let (before, after) = (pixels(&original), pixels(&shown.image));
    assert_eq!(before.len(), after.len());
    let changed: Vec<usize> = (0..before.len() / 4)
        .filter(|i| before[i * 4..i * 4 + 4] != after[i * 4..i * 4 + 4])
        .collect();
    assert!(!changed.is_empty());
    let inside = |i: &usize| i % 200 >= 100 && i / 200 >= 50;
    assert!(changed.iter().all(inside), "only inside the area");
}

/// Two lines far apart, one sentence per call: the first part is shown
/// before the second is translated.
struct Two;
impl Deps for Two {
    fn recognize(&self, _: &[u8], _: u32, _: u32) -> Result<Vec<ocr::TextBlock>, ocr::OcrError> {
        Ok(["Quit", "Play"]
            .into_iter()
            .zip([10.0, 100.0])
            .map(|(text, y)| ocr::TextBlock {
                text: text.into(),
                bounds: [20.0, y, 40.0, 20.0],
                confidence: 0.9,
            })
            .collect())
    }
    fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
        Ok(texts.iter().map(|_| "خروج".to_string()).collect())
    }
    fn batch(&self) -> usize {
        1
    }
}

/// Audit 12: a partial is just the translated area (never a copy of the whole
/// frame), and it is the same picture the full render has there.
#[test]
fn a_partial_is_the_area_only_and_matches_the_full_render() {
    let (w, h) = (300u32, 200u32);
    let px: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
    let original = image::Handle::from_rgba(w, h, px);
    let area = [100, 50, 300, 200];
    let (cw, ch) = (area[2] - area[0], area[3] - area[1]);
    let mut parts = Vec::new();
    let deps: Result<(Two, String), String> = Ok((Two, "ar".into()));
    let done = translate(&original, area, &Cache::new(8), &deps, |p| parts.push(p)).unwrap();
    assert_eq!(parts.len(), 1, "the first line is shown before the second");
    let part = &parts[0];
    assert_eq!(part.area, area);
    let (frame_len, part_len) = (pixels(&original).len(), pixels(&part.image).len());
    assert_eq!(part_len, (cw * ch * 4) as usize);
    assert!(part_len < frame_len, "not a frame-sized picture");
    // the part is the full render's crop where only the first line differs
    // (the rows above the second line)
    let full = pixels(&done.image);
    let row = (cw * 4) as usize;
    for y in 0..90usize {
        let at = ((area[1] as usize + y) * w as usize + area[0] as usize) * 4;
        assert_eq!(
            pixels(&part.image)[y * row..(y + 1) * row],
            full[at..at + row],
            "row {y}"
        );
    }
    // and the second line is not drawn yet in the part
    let at = ((area[1] as usize + 100) * w as usize + area[0] as usize) * 4;
    assert_ne!(
        pixels(&part.image)[100 * row..110 * row],
        full[at..at + 10 * row]
    );
}

#[test]
fn clicks_hit_translated_blocks_only() {
    let blocks = vec![
        block("Inventory", "المخزون", [0.0, 0.0, 100.0, 40.0]),
        block("100 / 250", "", [0.0, 50.0, 100.0, 20.0]),
        block("Potion", "جرعة", [10.0, 5.0, 30.0, 10.0]), // inside the first
    ];
    assert_eq!(hit(&blocks, Point::new(80.0, 30.0)), Some(0));
    assert_eq!(hit(&blocks, Point::new(20.0, 10.0)), Some(2), "smallest");
    assert_eq!(hit(&blocks, Point::new(20.0, 60.0)), None, "not translated");
    assert_eq!(hit(&blocks, Point::new(150.0, 10.0)), None);
    assert_eq!(hit(&blocks, Point::new(100.0, 10.0)), None, "end exclusive");

    let mut s = session(true);
    let _ = s.update(shown(blocks));
    s.click(Point::new(80.0, 30.0));
    let (crop, chat) = s.take_chat().unwrap();
    assert_eq!(
        (crop.width, crop.height),
        (108, 48),
        "block + context, clipped"
    );
    assert!(chat.turns.is_empty(), "nothing is sent before Explain");
    s.click(Point::new(80.0, 30.0));
    // the word card is the shared floating card, the pill beside it
    let size = Size::new(1280.0, 720.0);
    let view = s.view(size, card::place(size, None, None), false);
    let texts = widgets::shot::texts(view);
    for t in ["المخزون · Inventory", "Inventory", "المخزون", "Explain"] {
        assert!(texts.iter().any(|x| x == t), "{t}: {texts:?}");
    }
    let view = s.view(size, card::place(size, None, None), false);
    widgets::shot::save("translate-word-card", view, size);
    s.click(Point::new(150.0, 90.0));
    assert!(!s.has_card(), "a click elsewhere closes the card");
}

#[test]
fn explain_streams_into_the_card() {
    let mut s = session(true);
    let _ = s.update(shown(vec![block(
        "Sunstone",
        "حجر الشمس",
        [0.0, 0.0, 50.0, 20.0],
    )]));
    s.click(Point::new(5.0, 5.0));
    let card = s.card.as_mut().unwrap();
    // what Explain starts (without the thread)
    card.chat
        .ask(&explain_question(&card.block, "ar"), &[])
        .unwrap();
    let _ = s.update(Msg::Delta("حجر ".into()));
    let _ = s.update(Msg::Delta("نادر".into()));
    let _ = s.update(Msg::Explained(Ok(())));
    let card = s.card.as_ref().unwrap();
    assert_eq!(card.chat.status, Status::Done);
    assert_eq!(card.chat.turns[0].answer, "حجر نادر");
    assert!(card.chat.turns[0].question.contains("\"Sunstone\""));
    // explained already: a second Explain sends nothing
    assert!(s.explain().units() == 0);
}

#[test]
fn paste_puts_the_area_back_in_place() {
    let mut f = vec![0u8; 4 * 3 * 4];
    paste(&mut f, 4, [1, 1, 3, 3], &[9u8; 2 * 2 * 4]);
    let px = |x: usize, y: usize| f[(y * 4 + x) * 4];
    assert_eq!((px(0, 0), px(1, 1), px(2, 2), px(3, 2)), (0, 9, 9, 0));
}

/// Every translate state over the frame in Sapphire and Daylight: the
/// download questions and progress, the pill while translating, an error, the
/// pill with the target list open, and the word card (Arabic translation).
#[test]
fn every_translate_state_matches_its_golden_image() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    const SCREEN: Size = Size::new(960.0, 600.0);
    let at = card::place(SCREEN, None, None);
    let make = |name: &str| {
        let mut s = session(true);
        match name {
            "consent" => s.phase = Phase::Consent(None),
            "offline" => s.phase = Phase::Consent(Some((en_ar(), 117_600_822))),
            "downloading" => s.phase = Phase::Downloading(0.4),
            "failed" => s.phase = Phase::Failed(problem("No AI provider is set up.".into(), true)),
            "picker" => {
                let _ = s.update(shown(Vec::new()));
                s.picker.update(picker::Msg::Open(picker::Side::Target));
            }
            "word" => {
                let blocks = vec![block("Inventory", "المخزون", [0.0, 0.0, 100.0, 40.0])];
                let _ = s.update(shown(blocks));
                s.click(Point::new(50.0, 20.0));
            }
            _ => {} // translating
        }
        s
    };
    let names = [
        "consent",
        "offline",
        "downloading",
        "translating",
        "failed",
        "picker",
        "word",
    ];
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        for name in names {
            let s = make(name);
            let file = format!("translate-{name}-{}", theme.id);
            failed.extend(golden::check("screens", &file, SCREEN, &[1.0], || {
                s.view(SCREEN, at, false)
            }));
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Picking another language leaves no part of the old name on screen: each
/// change redrawn only where it differs, as the window does, equals the same
/// view drawn whole, at 100 % and 150 %.
#[test]
fn a_changed_language_leaves_no_ghost_text() {
    const CODES: [&str; 5] = ["ar", "en", "ja", "de", "zh-Hant"];
    let pickers: Vec<Picker> = (CODES.iter())
        .map(|code| Picker::new("auto", code, false))
        .collect();
    for scale in [1.0, 1.5] {
        let view = |i: usize| {
            (i <= 10).then(|| {
                let chips = pickers[i % pickers.len()].chips(picker::CHIP_HEIGHT);
                c(chips).padding(20).into()
            })
        };
        let (window, fresh) = widgets::shot::redrawn(view, Size::new(320.0, 80.0), scale);
        let differ = window.iter().zip(&fresh).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "{differ} stale bytes at {scale}x");
    }
}
