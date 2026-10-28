//! The result cards' behaviour and looks (Search, Text, Ask AI).
use super::*;
use crate::ui::kit::panel_frame;
use crate::ui::widgets;
use iced::{Rectangle, Size};

/// Where a panel shows on a `screen` (no selection).
fn at(screen: Size) -> Rectangle {
    panel_frame::place(screen, None, None)
}

#[test]
fn browser_turn_sends_text_or_says_why_not() {
    let (q, note) = browser_turn("Explain", Some(Ok("Sunstone\nRare gem".into())));
    assert_eq!(q, "Explain\n\nText from my screen:\nSunstone\nRare gem");
    assert!(note.contains("Sent with the text"));
    // Declined (no model download), empty, failed: the prompt alone, and the reason.
    for (read, why) in [
        (None, "cannot be passed"),
        (Some(Ok("  \n".into())), "No text was found"),
        (Some(Err("helper crashed".into())), "helper crashed"),
    ] {
        let (q, note) = browser_turn("Explain", read);
        assert_eq!(q, "Explain");
        assert!(note.contains(why) && note.contains("Only your question was sent"));
    }
}

#[test]
fn unknown_provider_shows_the_fix_and_sends_nothing() {
    let crop = Crop {
        width: 1,
        height: 1,
        rgba: vec![0; 4],
    };
    let mut panel = Panel {
        state: State::ask(Chat::default()),
        crop: Arc::new(crop),
        input: "Explain".into(), // e.g. the Lens follow-up, sent once ready
        work: None,
        translation: None,
        entities: Vec::new(),
        note: None,
        spin: None,
    };
    let msg = ask::tests::invalid_provider_error();
    let _ = panel.update(Msg::Ready(Err(msg.clone())));
    let _ = panel.update(Msg::Preset(0));
    let _ = panel.update(Msg::Submit);
    let State::Ask { chat, browse, .. } = &panel.state else {
        unreachable!()
    };
    assert!(chat.turns.is_empty() && chat.status == Status::Idle);
    assert!(matches!(browse, Browse::Idle) && panel.work.is_none());
    let texts = widgets::shot::texts(panel.view(
        at(Size::new(1920.0, 1080.0)),
        Size::new(1920.0, 1080.0),
        false,
    ));
    assert!(texts.contains(&msg), "{texts:?}");
}

/// The Text panel's download question, progress and error are short
/// cards: every text and button inside the window, no empty slab (#127).
#[test]
fn text_prompts_are_short_cards_on_small_screens() {
    let backdrop = iced::Color::from_rgb8(240, 240, 240);
    for (name, reading) in [
        ("consent", Reading::Consent),
        ("downloading", Reading::Downloading(0.4)),
        (
            "failed",
            Reading::Failed("The model could not be downloaded (offline).".into()),
        ),
    ] {
        let panel = panel_in(State::Text(reading));
        for (w, h) in [
            (1097.0, 617.0),
            (1125.0, 575.0),
            (700.0, 500.0),
            (1920.0, 1080.0),
        ] {
            let size = Size::new(w, h);
            let lost = widgets::shot::out_of_reach(panel.view(at(size), size, false), size);
            assert!(lost.is_empty(), "{name} at {w}x{h}: out of reach {lost:?}");
            let view = iced::widget::container(panel.view(at(size), size, false))
                .style(move |_| iced::widget::container::Style::default().background(backdrop));
            let px = widgets::shot::save(&format!("text-{name}-{w}x{h}"), view.into(), size);
            let a = at(size);
            // near its left edge: the download card is narrower than the spot
            let x = a.x + 10.0;
            let dark = |y: usize| {
                px[((y * w as usize) + x as usize) * 4..][..3]
                    .iter()
                    .all(|&v| v < 100)
            };
            let rows: Vec<usize> = (0..h as usize).filter(|&y| dark(y)).collect();
            assert!(!rows.is_empty(), "{name} at {w}x{h}: no card");
            let tall = rows[rows.len() - 1] - rows[0];
            assert!(
                tall < 330 && (tall as f32) < 0.7 * h,
                "{name} at {w}x{h}: a slab {tall}px tall"
            );
        }
    }
}

fn sample_crop() -> Crop {
    let rgba = (0..120u32)
        .flat_map(|y| (0..240u32).flat_map(move |x| [x as u8, (y * 2) as u8, 160, 255]))
        .collect();
    Crop {
        width: 240,
        height: 120,
        rgba,
    }
}

fn panel_in(state: State) -> Panel {
    Panel {
        state,
        crop: Arc::new(sample_crop()),
        input: String::new(),
        work: None,
        translation: None,
        entities: Vec::new(),
        note: None,
        spin: None,
    }
}

const SCREEN: Size = Size::new(1000.0, 700.0);

fn not_blank(px: &[u8]) -> bool {
    px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4])
}

#[test]
fn ask_panel_offers_the_six_presets() {
    let mut panel = panel_in(State::ask(Chat::default()));
    let _ = panel.update(Msg::Ready(Ok(Arc::new(ask::tests::browser_setup()))));
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    for preset in PRESETS {
        assert!(texts.iter().any(|t| t == preset.label), "{texts:?}");
    }
    assert!(not_blank(&widgets::shot::save(
        "panel-ask-presets",
        panel.view(at(SCREEN), SCREEN, false),
        SCREEN
    )));
    // Once a question was asked the suggestions are gone.
    let State::Ask { chat, .. } = &mut panel.state else {
        unreachable!()
    };
    chat.ask("Explain", &[]);
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(!texts.iter().any(|t| t == "Fact-check"), "{texts:?}");
}

fn text_panel(text: &str) -> Panel {
    let mut panel = panel_in(State::Text(Reading::Busy));
    let _ = panel.update(Msg::Read(Ok(text.into())));
    panel
}

const SAMPLE: &str = "Order #2026102412 shipped\n\
    Questions? Call +1 (555) 123-4567 or write to help@shop.example.\n\
    Track it: https://shop.example/track/4412. Total $1,299.99 on 2026-10-24.";

#[test]
fn text_panel_shows_chips_for_links_emails_and_numbers_only() {
    let mut panel = text_panel(SAMPLE);
    let kinds: Vec<_> = panel.entities.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [Kind::Link, Kind::Email, Kind::Phone]);
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    for chip in [
        "Open link · shop.example",
        "Write email · help@shop.example",
        "Copy number · +1 (555) 123-4567",
    ] {
        assert!(texts.iter().any(|t| t == chip), "{chip}: {texts:?}");
    }
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("1,299.99") && t.contains(": "))
    );
    // Reading the text does nothing with the chips: no click, no action, no note.
    assert!(panel.note.is_none());
    assert!(not_blank(&widgets::shot::save(
        "panel-text-chips",
        panel.view(at(SCREEN), SCREEN, false),
        SCREEN
    )));

    // Editing the text updates the chips.
    let paste = text_editor::Edit::Paste(Arc::new("new@site.io ".into()));
    let _ = panel.update(Msg::Edit(text_editor::Action::Edit(paste)));
    assert!(panel.entities.iter().any(|e| e.text == "new@site.io"));
    // A click outside the list does nothing.
    let _ = panel.update(Msg::Chip(99));
    assert!(panel.note.is_none());
}

#[test]
fn text_without_entities_has_no_chips() {
    let panel = text_panel("Just some words, 3.50 and e.g. a note");
    assert!(panel.entities.is_empty());
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(!texts.iter().any(|t| t.starts_with("Open link")));
}

/// The result cards in Sapphire and Daylight: Search (Lens), Text (Latin and
/// Arabic OCR text, with its link chips; the download question and its
/// progress) and Ask AI's suggestions.
#[test]
fn every_result_card_matches_its_golden_image() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    const CARD: Size = Size::new(420.0, 560.0);
    let at = Rectangle::with_size(CARD);
    type Make = Box<dyn Fn() -> Panel>;
    let states: Vec<(&str, Make)> = vec![
        ("lens", Box::new(|| panel_in(State::Lens(Some(Ok(())))))),
        ("text", Box::new(|| text_panel(SAMPLE))),
        (
            "text-arabic",
            Box::new(|| text_panel("مرحبا بك في المتجر\nاتصل بنا: help@shop.example")),
        ),
        (
            "text-consent",
            Box::new(|| panel_in(State::Text(Reading::Consent))),
        ),
        (
            "text-downloading",
            Box::new(|| panel_in(State::Text(Reading::Downloading(0.4)))),
        ),
        (
            "ask",
            Box::new(move || {
                let mut p = panel_in(State::ask(Chat::default()));
                let _ = p.update(Msg::Ready(Ok(Arc::new(ask::tests::browser_setup()))));
                p
            }),
        ),
    ];
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        for (name, panel) in &states {
            let panel = panel();
            let file = format!("result-card-{name}-{}", theme.id);
            failed.extend(golden::check("screens", &file, CARD, &[1.0], || {
                panel.view(at, Size::new(1920.0, 1080.0), false)
            }));
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A chat with an API provider, ready (a local server that is never called:
/// the answer is fed in as `Delta`s, as the stream's thread sends them).
fn api_chat() -> Panel {
    let mut panel = panel_in(State::ask(Chat::default()));
    let setup = ask::tests::local_setup("http://127.0.0.1:9/v1".into());
    let _ = panel.update(Msg::Ready(Ok(Arc::new(setup))));
    panel
}

fn ask(panel: &mut Panel, question: &str) {
    let _ = panel.update(Msg::Input(question.into()));
    let _ = panel.update(Msg::Submit);
}

fn chat(panel: &Panel) -> &Chat {
    let State::Ask { chat, .. } = &panel.state else {
        unreachable!()
    };
    chat
}

/// What the answer's Markdown has parsed so far, per turn.
fn parsed(panel: &Panel) -> Vec<usize> {
    let State::Ask { answers, .. } = &panel.state else {
        unreachable!()
    };
    answers
        .iter()
        .map(|(n, md)| n * (!md.items().is_empty()) as usize)
        .collect()
}

const ANSWER: &str = "## Shortcuts\n\n| Key | Action |\n|---|---|\n| Ctrl+C | Copy |\n\n\
    ```rust\nfn main() {}\n```\n";

#[test]
fn a_streamed_answer_is_markdown_and_stop_keeps_the_part_so_far() {
    let mut panel = api_chat();
    ask(&mut panel, "What are the keys?");
    assert_eq!(chat(&panel).status, Status::Streaming);
    let (first, rest) = ANSWER.split_at(30);
    let _ = panel.update(Msg::Delta(first.into()));
    assert_eq!(parsed(&panel), [30]);
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    let stop = crate::ui::kit::icon::Icon::Stop.glyph().to_string();
    assert!(texts.contains(&stop), "the Stop button: {texts:?}");
    // Stop mid-stream: no panic, the part so far stays, a late piece is dropped.
    let _ = panel.update(Msg::Stop);
    let _ = panel.update(Msg::Delta(rest.into()));
    let _ = panel.update(Msg::Answered(Err("Stopped.".into())));
    assert_eq!(chat(&panel).status, Status::Cancelled);
    assert_eq!(chat(&panel).turns[0].answer, first);
    assert!(panel.work.is_none());
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(texts.iter().any(|t| t == "Stopped."), "{texts:?}");
    assert_eq!(parsed(&panel), [30], "the part so far stays as Markdown");
}

#[test]
fn edit_resends_and_regenerate_asks_again() {
    let mut panel = api_chat();
    for (q, a) in [("One?", "**1**"), ("Two?", "2")] {
        ask(&mut panel, q);
        let _ = panel.update(Msg::Delta(a.into()));
        let _ = panel.update(Msg::Answered(Ok(())));
    }
    // Regenerate: the last question again, its old answer gone.
    let _ = panel.update(Msg::Regenerate);
    assert_eq!(chat(&panel).status, Status::Streaming);
    assert_eq!(chat(&panel).turns.len(), 2);
    assert_eq!(chat(&panel).turns[1].question, "Two?");
    assert_eq!(chat(&panel).turns[1].answer, "");
    assert_eq!(parsed(&panel), [5, 0]);
    let _ = panel.update(Msg::Delta("Two!".into()));
    let _ = panel.update(Msg::Answered(Ok(())));
    // Edit the first: back in the input; sending replaces it and what followed.
    let _ = panel.update(Msg::EditTurn(0));
    assert_eq!(panel.input, "One?");
    assert_eq!(chat(&panel).turns.len(), 2, "nothing goes before Send");
    ask(&mut panel, "One, edited?");
    let turns = &chat(&panel).turns;
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].question, "One, edited?");
    assert!(turns[0].answer.is_empty() && parsed(&panel) == [0]);
}

#[test]
fn an_error_says_why_and_offers_retry() {
    let mut panel = api_chat();
    ask(&mut panel, "Hi");
    let why = ai::AiError::Auth(401).to_string();
    let _ = panel.update(Msg::Answered(Err(why.clone())));
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(texts.contains(&why), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Retry"), "{texts:?}");
    let _ = panel.update(Msg::Regenerate);
    assert_eq!(chat(&panel).status, Status::Streaming);
    assert_eq!(chat(&panel).turns.len(), 1);
}

#[test]
fn the_screen_goes_only_while_include_screen_is_on() {
    let mut panel = api_chat();
    let shown = |panel: &Panel| {
        let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
        texts
            .iter()
            .any(|t| t.starts_with("Screen selection attached"))
    };
    assert!(shown(&panel), "on by default");
    ask(&mut panel, "With");
    let _ = panel.update(Msg::Answered(Ok(())));
    let _ = panel.update(Msg::IncludeScreen);
    assert!(!shown(&panel));
    ask(&mut panel, "Without");
    let turns = &chat(&panel).turns;
    assert!(turns[0].screen && !turns[1].screen);
    // A new chat starts empty, the switch kept.
    let _ = panel.update(Msg::NewChat);
    assert!(chat(&panel).turns.is_empty() && panel.work.is_none());
    assert!(parsed(&panel).is_empty());
}

/// The AI chat in Sapphire and Daylight: empty with its suggestions, a
/// Markdown answer (heading, table, code), an Arabic one, stopped, failed.
#[test]
fn the_chat_matches_its_golden_images() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    const CARD: Size = Size::new(504.0, 760.0);
    let at = Rectangle::with_size(CARD);
    let answered = |q: &str, a: &str, end: Option<Result<(), String>>| {
        let mut p = api_chat();
        ask(&mut p, q);
        let _ = p.update(Msg::Delta(a.into()));
        match end {
            Some(Ok(())) => {
                let _ = p.update(Msg::Answered(Ok(())));
            }
            Some(Err(e)) => {
                let _ = p.update(Msg::Answered(Err(e)));
            }
            None => {
                let _ = p.update(Msg::Stop);
            }
        }
        p
    };
    type Make = Box<dyn Fn() -> Panel>;
    let states: Vec<(&str, Make)> = vec![
        ("empty", Box::new(api_chat)),
        (
            "answer",
            Box::new(move || {
                let mut p = answered("What are the keys?", "", None);
                p.state = State::ask(Chat::default());
                let setup = ask::tests::local_setup("http://127.0.0.1:9/v1".into());
                let _ = p.update(Msg::Ready(Ok(Arc::new(setup))));
                ask(&mut p, "What are the keys?");
                let _ = p.update(Msg::Delta(String::new())); // it thinks: the logo chip
                let _ = p.update(Msg::Delta(ANSWER.into()));
                let _ = p.update(Msg::Answered(Ok(())));
                p
            }),
        ),
        (
            "waiting",
            Box::new(move || {
                let mut p = api_chat();
                ask(&mut p, "What are the keys?");
                p.spin = Some((Instant::now(), 0.125)); // an eighth of a turn
                p
            }),
        ),
        (
            "arabic",
            Box::new(move || {
                answered(
                    "ما هذا؟",
                    "هذه **نافذة** إعدادات.\n\n- اضغط `Ctrl+C` للنسخ\n- ثم الصق النص في Notepad",
                    Some(Ok(())),
                )
            }),
        ),
        (
            "stopped",
            Box::new(move || answered("Explain", "The first part of an", None)),
        ),
        (
            "error",
            Box::new(move || {
                answered("Explain", "", Some(Err(ai::AiError::Auth(401).to_string())))
            }),
        ),
    ];
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        for (name, panel) in &states {
            let panel = panel();
            let file = format!("chat-{name}-{}", theme.id);
            failed.extend(golden::check("screens", &file, CARD, &[1.0], || {
                panel.view(at, Size::new(1920.0, 1080.0), false)
            }));
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn the_logo_spins_until_the_first_words_then_stops() {
    let mut panel = api_chat();
    assert!(!panel.spinning(), "nothing asked");
    ask(&mut panel, "Explain");
    assert!(panel.waiting() && panel.spinning());
    let t0 = Instant::now();
    panel.tick(t0);
    panel.tick(t0 + spinner::TURN / 2);
    let (_, turn) = panel.spin.unwrap();
    assert!((turn - 0.5).abs() < 1e-3, "steady: {turn}");
    // thinking (an empty piece) keeps it turning; the first words stop it
    let _ = panel.update(Msg::Delta(String::new()));
    assert!(panel.spinning());
    let _ = panel.update(Msg::Delta("Hello".into()));
    assert!(!panel.waiting() && !panel.spinning(), "no frames wanted");
    panel.tick(t0 + spinner::TURN);
    assert!(panel.spin.is_none());
}

#[test]
fn with_reduced_motion_the_logo_stands_still() {
    motion::set_reduced(true);
    let mut panel = api_chat();
    ask(&mut panel, "Explain");
    assert!(panel.waiting() && !panel.spinning(), "no frame clock");
    panel.tick(Instant::now());
    assert!(panel.spin.is_none());
    let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(texts.iter().any(|t| t == "Thinking…"), "{texts:?}");
    motion::set_reduced(false);
}
