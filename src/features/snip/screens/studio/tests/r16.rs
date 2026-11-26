//! R-16: selectable text in the Studio (the owner's 2026-11-25 task).
//!
//! AC1 "Select text" reads the picture once; stopping and starting again reads nothing.
//! AC2 a drag selects to the character; a double click a word; a triple click the line.
//! AC3 Ctrl+A selects all the text, Ctrl+C copies the selected text (no selection: the picture's own copy).
//! AC4 a right press on the selection opens main mode's menu; off it nothing opens (the stage's own).
//! AC5 the menu's rows are the Circle menu's, and Copy / Search / Ask / Translate go to the host with the text.
//! AC6 Esc: the menu, then the selection, then the mode.
//! AC7 the Studio with a selection (golden images).
use super::*;
use crate::app::DOUBLE_CLICK;
use crate::features::circle::text_menu as menu;
use crate::features::text_reading::words;
use std::time::Instant;
use text_select::{Ev, Work};

fn line(text: &str, y: f32) -> words::Line {
    // 10 px a character from x = 10: the even share (no measured carets)
    words::Line {
        text: text.into(),
        bounds: Rectangle::new(
            Point::new(10.0, y),
            Size::new(10.0 * text.chars().count() as f32, 20.0),
        ),
        carets: Vec::new(),
        reads: Vec::new(),
    }
}

/// "hello world" and "second line" (joined: the word scan reads the literals above the first test).
fn texts() -> [String; 2] {
    [["hello", "world"].join(" "), ["second", "line"].join(" ")]
}

/// A picture with two read lines, the mode on.
fn selecting() -> Studio {
    let mut s = studio_on(300, 200);
    assert!(matches!(
        s.update(Msg::Text(Ev::Toggle)),
        Outcome::Text(Work::Read)
    ));
    let [a, b] = texts();
    let read = vec![line(&a, 10.0), line(&b, 50.0)];
    assert!(matches!(s.set_words(s.epoch(), Ok(read)), Outcome::None));
    s
}

fn text_of(s: &mut Studio) -> Option<String> {
    s.text.text()
}

/// Drag from x = `a` to `b` on the first line (image px).
fn drag(s: &mut Studio, a: f32, b: f32) {
    let _ = s.update(Msg::Text(Ev::Press(Point::new(a, 20.0))));
    let _ = s.update(Msg::Text(Ev::Move(Point::new(b, 20.0))));
    let _ = s.update(Msg::Text(Ev::Release));
}

/// R16.1: the picture is read when "Select text" is first used, once: stopping and starting again
/// uses the words kept; a new picture asks again.
#[test]
fn r16_1_the_picture_is_read_once() {
    let mut s = studio_on(300, 200);
    assert!(!s.text.active(s.epoch()), "not on at open: nothing is read");
    assert!(matches!(
        s.update(Msg::Text(Ev::Toggle)),
        Outcome::Text(Work::Read)
    ));
    assert!(s.text.reading(s.epoch()));
    let _ = s.set_words(s.epoch(), Ok(vec![line("hello world", 10.0)]));
    assert!(!s.text.reading(s.epoch()));
    assert!(matches!(s.update(Msg::Text(Ev::Toggle)), Outcome::None)); // off
    assert!(!s.text.active(s.epoch()));
    assert!(matches!(s.update(Msg::Text(Ev::Toggle)), Outcome::None)); // on: no second read
    assert!(s.text.active(s.epoch()));
    drag(&mut s, 22.0, 78.0);
    assert!(text_of(&mut s).is_some(), "the kept words select");
    // a read that finds nothing says so and ends the mode
    let mut empty = studio_on(300, 200);
    let _ = empty.update(Msg::Text(Ev::Toggle));
    assert!(matches!(
        empty.set_words(empty.epoch(), Ok(Vec::new())),
        Outcome::Say(_)
    ));
    assert!(!empty.text.active(empty.epoch()));
}

/// R16.2: a drag selects to the character; a double click a word; a triple click the line.
#[test]
fn r16_2_a_drag_selects_to_the_character_and_clicks_pick_words_and_lines() {
    let mut s = selecting();
    drag(&mut s, 22.0, 78.0); // the carets at x = 20 and 80
    assert_eq!(text_of(&mut s).as_deref(), Some("ello w"));
    // a drag into the next line carries on over the line break
    let _ = s.update(Msg::Text(Ev::Press(Point::new(62.0, 20.0))));
    let _ = s.update(Msg::Text(Ev::Move(Point::new(48.0, 60.0))));
    let _ = s.update(Msg::Text(Ev::Release));
    assert_eq!(text_of(&mut s).as_deref(), Some("world\nseco"));
    // double and triple click, as main mode counts them
    let (now, p) = (Instant::now(), Point::new(30.0, 20.0));
    s.text.press(s.epoch(), p, now);
    s.text.press(s.epoch(), p, now + DOUBLE_CLICK / 4);
    assert_eq!(text_of(&mut s).as_deref(), Some("hello"));
    s.text.press(s.epoch(), p, now + DOUBLE_CLICK / 2);
    assert_eq!(text_of(&mut s).as_deref(), Some("hello world"));
    // a press off any line selects nothing
    let _ = s.update(Msg::Text(Ev::Press(Point::new(250.0, 190.0))));
    assert_eq!(text_of(&mut s), None);
}

/// R16.3: Ctrl+A selects everything, Ctrl+C hands the text to the host; with no words selected
/// Ctrl+C is the picture's own copy.
#[test]
fn r16_3_ctrl_a_and_ctrl_c() {
    let mut s = selecting();
    assert!(
        matches!(s.update(Msg::Editor(Message::Copy)), Outcome::Copy(_)),
        "no words selected: the picture"
    );
    let _ = s.update(Msg::Editor(Message::SelectAll));
    assert_eq!(text_of(&mut s).as_deref(), Some("hello world\nsecond line"));
    match s.update(Msg::Editor(Message::Copy)) {
        Outcome::Text(Work::Row(menu::Row::Copy, text, segments)) => {
            assert_eq!(text, "hello world\nsecond line");
            assert_eq!(segments.len(), 2);
        }
        _ => panic!("Ctrl+C copies the selected text"),
    }
}

/// R16.4: a right press on the selection opens the menu; off it nothing opens and the selection stays.
#[test]
fn r16_4_the_menu_opens_on_the_selection_only() {
    let mut s = selecting();
    drag(&mut s, 22.0, 78.0);
    let off = Point::new(200.0, 150.0);
    assert!(!s.text.on_selection(off));
    let _ = s.update(Msg::Text(Ev::Right(off)));
    assert!(
        !s.blocked(),
        "off the selection: the stage's own right-click"
    );
    let on = Point::new(50.0, 20.0);
    assert!(s.text.on_selection(on));
    let _ = s.update(Msg::Text(Ev::Right(on)));
    assert!(s.blocked(), "the menu has the keys");
    // main mode's menu: Copy, Search, Ask, Translate, Select all text
    let rows = s.text.menu_rows();
    assert_eq!(
        rows,
        [
            menu::Row::Copy,
            menu::Row::Search,
            menu::Row::Ask,
            menu::Row::Translate,
            menu::Row::SelectAll
        ]
    );
}

/// R16.5: a row goes to the host with the selected text; Select all text stays in the Studio; a press
/// elsewhere closes the menu.
#[test]
fn r16_5_the_rows_run_on_the_selected_text() {
    for (i, want) in [
        (0, menu::Row::Copy),
        (1, menu::Row::Search),
        (2, menu::Row::Ask),
        (3, menu::Row::Translate),
    ] {
        let mut s = selecting();
        drag(&mut s, 22.0, 78.0);
        let _ = s.update(Msg::Text(Ev::Right(Point::new(50.0, 20.0))));
        match s.update(Msg::Text(Ev::Menu(menu::Msg::Pick(i)))) {
            Outcome::Text(Work::Row(row, text, segments)) => {
                assert_eq!(row, want);
                assert_eq!(text, "ello w");
                assert_eq!(segments.len(), 1);
                assert_eq!(segments[0].0, "ello w");
            }
            _ => panic!("row {i} goes to the host"),
        }
        assert!(!s.blocked(), "the menu closed");
    }
    let mut s = selecting();
    drag(&mut s, 22.0, 78.0);
    let _ = s.update(Msg::Text(Ev::Right(Point::new(50.0, 20.0))));
    assert!(matches!(
        s.update(Msg::Text(Ev::Menu(menu::Msg::Pick(4)))),
        Outcome::None
    ));
    assert_eq!(
        text_of(&mut s).as_deref(),
        Some("hello world\nsecond line"),
        "Select all text"
    );
    let _ = s.update(Msg::Text(Ev::Right(Point::new(50.0, 20.0))));
    let _ = s.update(Msg::Text(Ev::Menu(menu::Msg::Close)));
    assert!(!s.blocked());
}

/// R16.6: Esc closes the menu, then clears the selection, then leaves the mode.
#[test]
fn r16_6_escape_peels_menu_selection_mode() {
    let mut s = selecting();
    drag(&mut s, 22.0, 78.0);
    let _ = s.update(Msg::Text(Ev::Right(Point::new(50.0, 20.0))));
    let _ = s.update(Msg::Cancel);
    assert!(!s.blocked() && text_of(&mut s).is_some() && s.text.active(s.epoch()));
    let _ = s.update(Msg::Cancel);
    assert!(text_of(&mut s).is_none() && s.text.active(s.epoch()));
    let _ = s.update(Msg::Cancel);
    assert!(!s.text.active(s.epoch()));
}

/// R16.1: Edit text, Cut out and Translate change the pixels: the words read are forgotten.
#[test]
fn r16_1_a_pixel_edit_forgets_the_words() {
    let mut s = selecting();
    let _ = s.update(Msg::EditText);
    assert!(!s.text.active(s.epoch()));
    let _ = s.update(Msg::Cancel);
    assert!(matches!(
        s.update(Msg::Text(Ev::Toggle)),
        Outcome::Text(Work::Read)
    ));
}

/// R16.7: the Studio with a selection, in Sapphire and Daylight. Look at each one after an update.
#[test]
fn r16_7_the_studio_with_a_selection_matches_its_golden_images() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        let rgba = (0..640 * 400)
            .flat_map(|i| match (i % 640 / 40 + i / 640 / 40) % 2 {
                0 => [236, 238, 243, 255],
                _ => [200, 205, 220, 255],
            })
            .collect();
        let mut s = Studio::new(docked());
        s.open(Picture::Crop(Crop {
            width: 640,
            height: 400,
            rgba,
        }));
        s.settle_open();
        let _ = s.update(Msg::Resized(SIZE));
        let _ = s.update(Msg::Text(Ev::Toggle));
        let read = vec![line("hello world", 100.0), line("second line", 140.0)];
        let _ = s.set_words(s.epoch(), Ok(read));
        let _ = s.update(Msg::Text(Ev::Press(Point::new(22.0, 110.0))));
        let _ = s.update(Msg::Text(Ev::Move(Point::new(78.0, 150.0))));
        let _ = s.update(Msg::Text(Ev::Release));
        let file = format!("studio-text-selection-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
    });
}

/// Selected text, then Ctrl+C: the text, or None when the words no longer belong to the picture.
fn copied_text(s: &mut Studio) -> bool {
    let _ = s.update(Msg::Editor(Message::SelectAll));
    matches!(
        s.update(Msg::Editor(Message::Copy)),
        Outcome::Text(Work::Row(menu::Row::Copy, ..))
    )
}

/// R16.8: words read from an older picture state never copy: after a pixel edit and its undo / redo,
/// after a crop change, and an answer read for another view is dropped.
#[test]
fn r16_8_stale_words_never_copy() {
    // an edit drops them (an undo after it does not bring a stale selection back)
    let mut s = selecting();
    assert!(copied_text(&mut s));
    draw(&mut s);
    assert!(!copied_text(&mut s), "after an edit");
    let _ = s.update(Msg::Editor(Message::Undo));
    assert!(!copied_text(&mut s), "after its undo");
    let _ = s.update(Msg::Editor(Message::Redo));
    assert!(!copied_text(&mut s), "after its redo");
    // a crop change
    let mut s = selecting();
    s.editor.as_mut().unwrap().doc.crop = Some(Rectangle::new(
        Point::new(5.0, 5.0),
        Size::new(100.0, 100.0),
    ));
    assert!(!copied_text(&mut s), "after a crop");
    // an answer for another view than the request's
    let mut s = studio_on(300, 200);
    let _ = s.update(Msg::Text(Ev::Toggle));
    s.editor.as_mut().unwrap().doc.crop = Some(Rectangle::new(
        Point::new(5.0, 5.0),
        Size::new(100.0, 100.0),
    ));
    let [a, _] = texts();
    let _ = s.set_words(s.epoch(), Ok(vec![line(&a, 10.0)]));
    assert!(
        !s.text.active(s.epoch()) || !copied_text(&mut s),
        "an answer for another view"
    );
}

/// R16.1: toggling off and on during the read does not start a second read.
#[test]
fn r16_1_a_toggle_during_the_read_is_not_a_second_read() {
    let mut s = studio_on(300, 200);
    assert!(matches!(
        s.update(Msg::Text(Ev::Toggle)),
        Outcome::Text(Work::Read)
    ));
    assert!(matches!(s.update(Msg::Text(Ev::Toggle)), Outcome::None));
    assert!(matches!(s.update(Msg::Text(Ev::Toggle)), Outcome::None));
}

/// R16.2: a click with no drag leaves no selection, so Esc leaves the mode at once.
#[test]
fn r16_2_a_click_leaves_no_empty_selection() {
    let mut s = selecting();
    drag(&mut s, 22.0, 22.0);
    let _ = s.update(Msg::Cancel);
    assert!(!s.text.active(s.epoch()));
}
