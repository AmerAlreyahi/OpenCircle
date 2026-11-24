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
    let first = e.blur_style;
    let mut seen = vec![first];
    for _ in 0..5 {
        e.update(Message::BlurCycle);
        seen.push(e.blur_style);
    }
    assert_eq!(seen[5], first);
    seen.pop();
    seen.sort_by_key(|s| *s as usize);
    seen.dedup();
    assert_eq!(seen.len(), 5, "every style once");
    // With another tool the keys are the tools' own.
    e.update(Message::Tool(Tool::Arrow));
    assert!(!matches!(
        press(&mut e, 'r', false),
        Some(Message::BlurShape(_))
    ));
}

/// W102-5.d: each style card shows what its style does to the area under the
/// pointer: five pictures, the left half untouched, the right half hidden
/// differently per style; they follow the pointer.
#[test]
fn w102_5d_style_cards_show_the_area_under_the_pointer() {
    let mut e = editor(1.0);
    let pic = |h: &iced::widget::image::Handle| match h {
        iced::widget::image::Handle::Rgba { pixels, width, .. } => (*width, pixels.to_vec()),
        _ => panic!("pixels"),
    };
    e.update(Message::Move(pt(100.0, 100.0)));
    let first = e.style_thumbs();
    assert_eq!(first.len(), blur_styles().len());
    let pics: Vec<_> = first.iter().map(pic).collect();
    for i in 0..pics.len() {
        for j in i + 1..pics.len() {
            assert_ne!(pics[i].1, pics[j].1, "styles {i} and {j} look alike");
        }
    }
    // The left half is the picture as it is, in every card.
    let (w, _) = pics[0];
    for (_, px) in &pics {
        assert_eq!(
            px[..(w as usize / 2 - 10) * 4],
            pics[0].1[..(w as usize / 2 - 10) * 4]
        );
    }
    e.update(Message::Move(pt(300.0, 40.0)));
    let moved: Vec<_> = e.style_thumbs().iter().map(pic).collect();
    assert_ne!(moved[0].1, pics[0].1, "follows the pointer");
}
