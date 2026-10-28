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
        state: State::Ask {
            thumb: image::Handle::from_rgba(1, 1, crop.rgba.clone()),
            setup: None,
            chat: Chat::default(),
            browse: Browse::Idle,
        },
        crop: Arc::new(crop),
        input: "Explain".into(), // e.g. the Lens follow-up, sent once ready
        work: None,
        translation: None,
        entities: Vec::new(),
        note: None,
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
            let x = a.x + a.width * 0.9;
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

/// The Ask panel's browser-consent card is as tall as its content on a
/// big screen too, all inside the window (#127).
#[test]
fn ask_consent_is_a_short_card() {
    let crop = sample_crop();
    for (w, h) in [(1097.0, 617.0), (1920.0, 1080.0)] {
        let size = Size::new(w, h);
        let panel = panel_in(State::Ask {
            thumb: image::Handle::from_rgba(crop.width, crop.height, crop.rgba.clone()),
            setup: Some(Ok(Arc::new(ask::tests::browser_setup()))),
            chat: Chat::default(),
            browse: Browse::Consent("What is this?".into()),
        });
        let lost = widgets::shot::out_of_reach(panel.view(at(size), size, false), size);
        assert!(lost.is_empty(), "{w}x{h}: out of reach {lost:?}");
        let px = widgets::shot::save(
            &format!("ask-consent-{w}x{h}"),
            iced::widget::container(panel.view(at(size), size, false))
                .style(|_| {
                    iced::widget::container::Style::default()
                        .background(iced::Color::from_rgb8(240, 240, 240))
                })
                .into(),
            size,
        );
        let a = at(size);
        let x = (a.x + a.width * 0.97) as usize;
        let dark = |y: usize| {
            px[((y * w as usize) + x) * 4..][..3]
                .iter()
                .all(|&v| v < 100)
        };
        let rows: Vec<usize> = (0..h as usize).filter(|&y| dark(y)).collect();
        let tall = (rows[rows.len() - 1] - rows[0]) as f32;
        assert!(
            tall < a.height - 40.0,
            "{w}x{h}: a slab {tall}px of {}",
            a.height
        );
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
    }
}

const SCREEN: Size = Size::new(1000.0, 700.0);

fn not_blank(px: &[u8]) -> bool {
    px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4])
}

#[test]
fn ask_panel_offers_the_six_presets() {
    let crop = sample_crop();
    let mut panel = panel_in(State::Ask {
        thumb: image::Handle::from_rgba(crop.width, crop.height, crop.rgba.clone()),
        setup: None,
        chat: Chat::default(),
        browse: Browse::Idle,
    });
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
    let crop = sample_crop();
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
                let mut p = panel_in(State::Ask {
                    thumb: image::Handle::from_rgba(crop.width, crop.height, crop.rgba.clone()),
                    setup: None,
                    chat: Chat::default(),
                    browse: Browse::Idle,
                });
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
