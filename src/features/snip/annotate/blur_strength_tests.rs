//! W102 step 5: strength Light / Medium / Strong (0.25 / 0.5 / 1.0 of the
//! reference text height), the Studio's custom slider, the keys, the style
//! cards' live pictures.
use super::blur_tests::pt;
use super::tests::image_of;
use super::*;
use iced::keyboard::{Key, Modifiers};

fn editor(zoom: f32) -> Annotator {
    let mut e = Annotator::new(
        image_of(400, 200, |x, y| {
            if (x / 5 + y / 5) % 2 == 0 {
                [250, 250, 250, 255]
            } else {
                [30, 40, 60, 255]
            }
        }),
        zoom,
    );
    e.update(Message::Tool(Tool::Blur));
    e
}

fn cell_after(e: &mut Annotator) -> f32 {
    e.update(Message::Press(pt(50.0, 100.0)));
    e.update(Message::Move(pt(150.0, 100.0)));
    e.update(Message::Release);
    let Shape::Blur { cell, .. } = e.doc.items.last().unwrap().shape else {
        panic!("a blur")
    };
    cell
}

fn press(e: &mut Annotator, letter: char, shift: bool) -> Option<Message> {
    let mods = if shift {
        Modifiers::SHIFT
    } else {
        Modifiers::empty()
    };
    e.key(
        &Key::Character(letter.to_string().into()),
        Some(letter),
        mods,
        None,
    )
}

/// W102-5.a: Light, Medium, Strong are 0.25, 0.5 and 1.0 of the 16 px reference
/// text height (the capture's scale applied), the same at any zoom.
#[test]
fn w102_5a_strength_is_a_fraction_of_the_text_height() {
    let want = [4.0, 8.0, 16.0];
    for zoom in [0.5, 1.0, 2.0] {
        for (w, px) in Width::ALL.iter().zip(want) {
            let mut e = editor(zoom);
            e.update(Message::BlurCell(*w));
            let scale = e.capture_scale;
            assert_eq!(cell_after(&mut e), px * scale, "{w:?} at zoom {zoom}");
        }
    }
    assert_eq!(editor(1.0).blur_cell, Width::Medium, "Medium by default");
}

/// W102-5.b: the Studio's custom strength overrides the presets; a preset
/// takes it back.
#[test]
fn w102_5b_custom_strength_overrides_and_a_preset_takes_it_back() {
    let mut e = editor(1.0);
    e.update(Message::BlurCustom(Some(11)));
    assert_eq!(cell_after(&mut e), 11.0 * e.capture_scale);
    e.update(Message::BlurCell(Width::Thin));
    assert_eq!(e.blur_custom, None);
    assert_eq!(cell_after(&mut e), 4.0 * e.capture_scale);
}

/// W102-5.c: while Hide is on, 1-3 pick the strength and Shift+B cycles the
/// styles (plain B stays the tool's key; R / E / P keep their tools).
#[test]
fn w102_5c_hide_keys() {
    let mut e = editor(1.0);
    // R / O / P stay the Rect / Ellipse / Pen tools (the shape is an option).
    for (letter, tool) in [('r', Tool::Rect), ('o', Tool::Ellipse), ('p', Tool::Pen)] {
        let m = press(&mut e, letter, false);
        assert!(matches!(m, Some(Message::Tool(t)) if t == tool), "{letter}");
    }
    for (d, w) in ['1', '2', '3'].into_iter().zip(Width::ALL) {
        let m = press(&mut e, d, false);
        assert!(matches!(m, Some(Message::BlurCell(x)) if x == w), "{d}");
    }
    assert!(matches!(press(&mut e, 'B', true), Some(Message::BlurCycle)));
    assert!(matches!(
        press(&mut e, 'b', false),
        Some(Message::Tool(Tool::Blur))
    ));
    // The cycle walks the whole list and comes back round.
    let n = blur_styles().len();
    let first = e.blur_style;
    let mut seen = vec![first];
    for _ in 0..n {
        e.update(Message::BlurCycle);
        seen.push(e.blur_style);
    }
    assert_eq!(seen[n], first);
    seen.pop();
    seen.sort_by_key(|s| *s as usize);
    seen.dedup();
    assert_eq!(seen.len(), n, "every style once");
    // With another tool the keys are the tools' own.
    e.update(Message::Tool(Tool::Arrow));
    assert!(!matches!(
        press(&mut e, 'r', false),
        Some(Message::BlurShape(_))
    ));
}

/// W117: Hide style swatches are fixed built-in samples (never from the user's screenshot),
/// rendered once per light/dark theme and cached, with each style visually distinct.
#[test]
fn w117_style_swatches_are_fixed_cached_and_theme_aware() {
    let pic = |h: &iced::widget::image::Handle| match h {
        iced::widget::image::Handle::Rgba { pixels, width, .. } => {
            (*width as usize, pixels.to_vec())
        }
        _ => panic!("pixels"),
    };
    let light_a = style_swatches(true);
    let light_b = style_swatches(true);
    let dark = style_swatches(false);
    assert_eq!(light_a.len(), blur_styles().len());
    assert_eq!(dark.len(), blur_styles().len());
    for (a, b) in light_a.iter().zip(&light_b) {
        assert_eq!(a.id(), b.id(), "cached handle reused");
    }
    for (mode, set) in [("light", &light_a), ("dark", &dark)] {
        let pics: Vec<_> = set.iter().map(pic).collect();
        for i in 0..pics.len() {
            for j in i + 1..pics.len() {
                assert_ne!(
                    pics[i].1, pics[j].1,
                    "{mode}: styles {i} and {j} look alike"
                );
            }
        }
        let w = pics[0].0;
        for (_, px) in &pics {
            assert_eq!(px[..(w / 2 - 10) * 4], pics[0].1[..(w / 2 - 10) * 4]);
        }
    }
    assert_ne!(
        pic(&light_a[0]).1,
        pic(&dark[0]).1,
        "light and dark cards differ"
    );
}

/// W102 (review): Shift+B changes the style for the NEXT Hide; an existing Hide keeps its
/// own (a secure Hide never silently turns into a look); a click on a card restyles it.
#[test]
fn w102_5f_cycling_never_restyles_an_existing_hide() {
    let mut e = editor(1.0);
    e.studio = true;
    e.update(Message::Press(pt(40.0, 100.0)));
    e.update(Message::Move(pt(200.0, 100.0)));
    e.update(Message::Release);
    e.doc.select_one(Some(0));
    for _ in 0..5 {
        e.update(Message::BlurCycle);
        let Shape::Blur { style, .. } = e.doc.items[0].shape else {
            panic!()
        };
        assert_eq!(style, blur::Style::Soft, "after {:?}", e.blur_style);
    }
    assert_ne!(e.blur_style, blur::Style::Soft);
    e.update(Message::BlurStyle(blur::Style::Blur));
    let Shape::Blur { style, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!(style, blur::Style::Blur, "a click on a card restyles it");
}
