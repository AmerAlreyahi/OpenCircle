//! The result cards' behaviour and looks (Search, Ask AI).
use super::*;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::panel_frame;
use crate::ui::kit::panel_frame::Event;
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
        input: text_editor::Content::with_text("Explain"), // e.g. the Lens follow-up, sent once ready
        queued: true,
        ..Panel::new(Arc::new(crop), State::ask(Chat::default()))
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
    let texts = crate::ui::kit::testing::shot::texts(panel.view(
        at(Size::new(1920.0, 1080.0)),
        Size::new(1920.0, 1080.0),
        false,
    ));
    assert!(texts.contains(&msg), "{texts:?}");
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
    Panel::new(Arc::new(sample_crop()), state)
}

/// The composer's text set to `text` (as a paste).
pub(crate) fn typed(text: &str) -> Msg {
    Msg::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(
        Arc::new(text.into()),
    )))
}

const SCREEN: Size = Size::new(1000.0, 700.0);

fn not_blank(px: &[u8]) -> bool {
    px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4])
}

#[test]
fn ask_panel_offers_the_six_presets() {
    let mut panel = panel_in(State::ask(Chat::default()));
    let _ = panel.update(Msg::Ready(Ok(Arc::new(ask::tests::browser_setup()))));
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    for preset in PRESETS {
        assert!(texts.iter().any(|t| t == preset.label), "{texts:?}");
    }
    assert!(not_blank(&crate::ui::kit::testing::shot::save(
        "panel-ask-presets",
        panel.view(at(SCREEN), SCREEN, false),
        SCREEN
    )));
    // Once a question was asked the suggestions are gone.
    let State::Ask { chat, .. } = &mut panel.state else {
        unreachable!()
    };
    chat.ask("Explain", &[]);
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(!texts.iter().any(|t| t == "Fact-check"), "{texts:?}");
}

/// The result cards in Sapphire and Daylight: Search (Lens) and Ask AI's suggestions.
#[test]
fn every_result_card_matches_its_golden_image() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    const CARD: Size = Size::new(420.0, 560.0);
    let at = Rectangle::with_size(CARD);
    type Make = Box<dyn Fn() -> Panel>;
    let states: Vec<(&str, Make)> = vec![
        ("lens", Box::new(|| panel_in(State::Lens(Some(Ok(())))))),
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
    let _ = panel.update(Msg::Edit(text_editor::Action::SelectAll));
    let _ = panel.update(typed(question));
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
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    let stop = crate::ui::kit::icon::Icon::Stop.glyph().to_string();
    assert!(texts.contains(&stop), "the Stop button: {texts:?}");
    // Stop mid-stream: no panic, the part so far stays, a late piece is dropped.
    let _ = panel.update(Msg::Stop);
    let _ = panel.update(Msg::Delta(rest.into()));
    let _ = panel.update(Msg::Answered(Err("Stopped.".into())));
    assert_eq!(chat(&panel).status, Status::Cancelled);
    assert_eq!(chat(&panel).turns[0].answer, first);
    assert!(panel.work.is_none());
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
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
    assert_eq!(panel.input.text(), "One?");
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
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(texts.contains(&why), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Retry"), "{texts:?}");
    let _ = panel.update(Msg::Regenerate);
    assert_eq!(chat(&panel).status, Status::Streaming);
    assert_eq!(chat(&panel).turns.len(), 1);
}

/// Opened with Include screen off (the bar's AI field switched it off): the
/// question the field handed over goes without the picture.
#[test]
fn opened_with_include_screen_off_the_question_goes_alone() {
    let question = Some("What is this?".to_string());
    let (mut panel, _) = Panel::open(actions::Panel::Ask, sample_crop(), question, false);
    assert_eq!(panel.include(), Some(false));
    let setup = ask::tests::local_setup("http://127.0.0.1:9/v1".into());
    let _ = panel.update(Msg::Ready(Ok(Arc::new(setup))));
    let turns = &chat(&panel).turns;
    assert_eq!(turns.len(), 1, "sent once ready");
    assert!(!turns[0].screen, "no image in the request");
    assert_eq!(panel.include(), Some(false));
}

#[test]
fn the_screen_goes_only_while_include_screen_is_on() {
    let mut panel = api_chat();
    let shown = |panel: &Panel| panel.include() == Some(true);
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
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(texts.iter().any(|t| t == "Thinking…"), "{texts:?}");
    motion::set_reduced(false);
}

/// One answer and its end.
fn answer(panel: &mut Panel, text: &str) {
    let _ = panel.update(Msg::Delta(text.into()));
    let _ = panel.update(Msg::Answered(Ok(())));
}

/// A quick question stays unpinned (Esc / a click outside close it); a
/// follow-up pins it; "Always" pins from the start.
#[test]
fn a_follow_up_pins_the_chat_and_always_pins_at_once() {
    let mut panel = api_chat();
    ask(&mut panel, "One?");
    answer(&mut panel, "1");
    assert!(!panel.pinned(), "one answer: closes as before");
    ask(&mut panel, "And two?");
    assert!(panel.pinned(), "a follow-up pins it");
    // editing the only question again is no follow-up
    let mut panel = api_chat();
    ask(&mut panel, "One?");
    answer(&mut panel, "1");
    let _ = panel.update(Msg::EditTurn(0));
    ask(&mut panel, "One, edited?");
    assert!(!panel.pinned());
    // the header's pin pins by hand
    let _ = panel.update(Msg::Pin);
    assert!(panel.pinned());
    assert!(super::starts_pinned(StaysOpen::Always));
    assert!(!super::starts_pinned(StaysOpen::FollowUp));
}

/// The header's pin reads at a glance: "pin" while open, "pin-off" on the
/// filled button once pinned, each with its own tooltip.
#[test]
fn the_pin_looks_different_once_pinned() {
    use crate::ui::kit::testing::shot;
    use crate::ui::kit::tooltip::tests_support::shown;
    let glyphs = |panel: &Panel| {
        let texts = shot::texts(panel.view(at(SCREEN), SCREEN, false));
        let has = |i: Icon| texts.iter().any(|t| t.contains(i.glyph()));
        (has(Icon::Pin), has(Icon::Unpin))
    };
    let (size, header) = (
        Size::new(420.0, 120.0),
        Rectangle::with_size(Size::new(420.0, 400.0)),
    );
    let mut panel = api_chat();
    shot::save("chat-header-open", panel.view(header, SCREEN, false), size);
    crate::ui::kit::tooltip::SHOWN.with(|s| s.borrow_mut().clear());
    assert_eq!(glyphs(&panel), (true, false), "open: the plain pin");
    assert!(shown().iter().any(|t| t == "Pin chat"));
    let _ = panel.update(Msg::Pin);
    crate::ui::kit::tooltip::SHOWN.with(|s| s.borrow_mut().clear());
    assert_eq!(glyphs(&panel), (false, true), "pinned: pin-off");
    assert!(shown().iter().any(|t| t == "Unpin and close"));
    shot::save(
        "chat-header-pinned",
        panel.view(header, SCREEN, false),
        size,
    );
}

/// "Thought for N s" only when the model sent reasoning (an empty piece from
/// the stream), never for the wait alone.
#[test]
fn the_thought_chip_shows_only_when_the_model_reasoned() {
    let thought = |panel: &Panel| {
        let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
        texts.iter().any(|t| t.starts_with("Thought for"))
    };
    let mut panel = api_chat();
    ask(&mut panel, "Hi");
    std::thread::sleep(std::time::Duration::from_millis(20)); // a slow first word
    answer(&mut panel, "Hello");
    assert!(chat(&panel).turns[0].thought.is_none());
    assert!(!thought(&panel), "no reasoning: no chip");
    ask(&mut panel, "Why?");
    let _ = panel.update(Msg::Delta(String::new())); // reasoning
    answer(&mut panel, "Because");
    assert!(thought(&panel));
}

/// The next capture asks into the same chat: its crop goes with that question.
#[test]
fn a_new_crop_goes_with_the_next_question_into_the_same_chat() {
    let mut panel = api_chat();
    ask(&mut panel, "First?");
    answer(&mut panel, "1");
    let other = Crop {
        width: 32,
        height: 16,
        rgba: vec![9; 32 * 16 * 4],
    };
    let _ = panel.ask_about(other, Some("About this one?".into()), true);
    assert_eq!(
        chat(&panel).turns.len(),
        1,
        "nothing goes before the crop is ready"
    );
    let mut setup = ask::tests::local_setup("http://127.0.0.1:9/v1".into());
    setup.png = b"second png".to_vec();
    let _ = panel.update(Msg::Ready(Ok(Arc::new(setup))));
    let turns = &chat(&panel).turns;
    assert_eq!(turns.len(), 2, "the same chat");
    assert_eq!(turns[1].question, "About this one?");
    assert_eq!(turns[1].shot, Some((32, 16)));
    assert_eq!(turns[1].image.as_deref(), Some(&b"second png"[..]));
    // a draft in the composer is never sent by a reload
    let _ = panel.update(Msg::Answered(Ok(())));
    let _ = panel.update(typed("draft"));
    let setup = ask::tests::local_setup("http://127.0.0.1:9/v1".into());
    let _ = panel.update(Msg::Ready(Ok(Arc::new(setup))));
    assert_eq!(chat(&panel).turns.len(), 2);
}

/// The composer is one row: field, Include screen, Send.
#[test]
fn the_composer_is_one_row() {
    let panel = api_chat();
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(
        !texts
            .iter()
            .any(|t| t.starts_with("Screen selection attached")),
        "{texts:?}"
    );
    // the field (its text is the editor's), then the switch and Send, last
    let glyph = |i: crate::ui::kit::icon::Icon| i.glyph().to_string();
    let n = texts.len();
    assert_eq!(
        texts[n - 3..n - 1],
        [glyph(Icon::Screen), glyph(Icon::Send)],
        "{texts:?}"
    );
}

/// The drawer opens from the logo on History, switches to Settings, closes.
#[test]
fn the_drawer_opens_on_history_and_shows_the_chat_settings() {
    let mut panel = api_chat();
    let _ = panel.update(Msg::Card(Event::Menu));
    assert_eq!(panel.drawer, Some(Tab::History));
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    assert!(texts.iter().any(|t| t == "Workspace"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "+ New chat"), "{texts:?}");
    let _ = panel.update(Msg::Drawer(Some(Tab::Settings)));
    let texts = crate::ui::kit::testing::shot::texts(panel.view(at(SCREEN), SCREEN, false));
    for want in [
        "AI chat stays open",
        "After a follow-up",
        "Always",
        "Open AI settings →",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    let _ = panel.update(Msg::Card(Event::Menu));
    assert_eq!(panel.drawer, None, "the logo again closes it");
}

#[test]
fn safety_classifier_models_are_flagged() {
    for m in [
        "meta-llama/llama-guard-4-12b",
        "openai/gpt-oss-safeguard-20b",
        "omni-moderation-latest",
        "ShieldGemma-Safety",
    ] {
        assert!(super::classifier(m), "{m}");
    }
    for m in ["gpt-4o-mini", "claude-sonnet-5-5", "llama3.2-vision"] {
        assert!(!super::classifier(m), "{m}");
    }
}

/// The picker lists Settings' providers by name (the browser first, no Custom
/// server), and a pick writes what Settings' list writes.
#[test]
fn the_picker_follows_settings_list() {
    use crate::ai::providers::{ANTHROPIC, CUSTOM, PRESETS};
    assert_eq!(PICKS.len(), PRESETS.len()); // + the browser, - Custom server
    assert_eq!(PICKS[0].to_string(), "Browser (Google AI Mode)");
    assert!(PICKS.iter().all(|p| p.to_string() != CUSTOM.name));
    let mut cfg = config::AiConfig::default();
    assert_eq!(picked(&cfg), Some(PICKS[0]));
    let router = *PICKS
        .iter()
        .find(|p| p.to_string() == "OpenRouter")
        .unwrap();
    super::apply_pick(&mut cfg, router);
    assert_eq!(cfg.provider, Provider::OpenaiCompatible);
    assert_eq!(cfg.base_url, "https://openrouter.ai/api/v1");
    assert_eq!(picked(&cfg), Some(router));
    // its own key: the one stored for OpenRouter, never another provider's
    assert_eq!(
        crate::ai::providers::key_of(&cfg).map(|k| k.entry()),
        Some("ai-key-openrouter".into())
    );
    let anthropic = *PICKS
        .iter()
        .find(|p| p.to_string() == ANTHROPIC.name)
        .unwrap();
    super::apply_pick(&mut cfg, anthropic);
    assert_eq!(cfg.provider, Provider::Anthropic);
    assert_eq!(
        cfg.model, ANTHROPIC.model,
        "a preset's model follows the pick"
    );
    cfg.model = "my-own-model".into();
    super::apply_pick(&mut cfg, router);
    assert_eq!(cfg.model, "my-own-model", "a typed model stays");
    cfg.base_url = "http://my-own:9/v1".into();
    assert_eq!(
        picked(&cfg).map(|p| p.to_string()),
        Some(CUSTOM.name.into())
    );
}

/// An answered chat is saved, text only: the database holds no image bytes,
/// and Keep chat history off writes nothing at all.
#[test]
fn answered_chats_are_saved_as_text_only_and_off_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("oc-panel-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = dir.join("opencircle.db");
    let mut panel = api_chat();
    ask(&mut panel, "What is this?");
    let _ = panel.update(Msg::Delta("A cat".into()));
    let State::Ask { chat, .. } = &mut panel.state else {
        unreachable!()
    };
    chat.finish(Ok(()));
    assert!(chat.turns[0].image.is_some(), "the picture went with it");
    drop(super::keep_in(Some(db.clone()), false, chat));
    assert!(!dir.exists(), "off: nothing written");
    drop(super::keep_in(Some(db.clone()), true, chat));
    let saved = (0..200)
        .find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            chats::list(&db).into_iter().next()
        })
        .expect("saved on its thread");
    assert_eq!(saved.title, "What is this?");
    let back = chats::load(&db, &saved.id).unwrap();
    assert!(back.turns[0].screen && back.turns[0].answer == "A cat");
    let mut bytes = std::fs::read(&db).unwrap();
    bytes.extend(std::fs::read(db.with_extension("db-wal")).unwrap_or_default());
    assert!(
        !bytes.windows(3).any(|w| w == b"png"),
        "no image bytes on disk"
    );
    // opened again: the marker stays, the picture is gone
    let restored = Chat::from_saved(back);
    assert!(restored.turns[0].screen && restored.turns[0].shot.is_none());
    let _ = std::fs::remove_dir_all(dir);
}
