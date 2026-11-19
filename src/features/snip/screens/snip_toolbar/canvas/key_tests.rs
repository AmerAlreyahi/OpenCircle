//! The Studio's keys on the REAL path: a keyboard event into the canvas, and
//! what comes out (a message, captured) or nothing (ignored, so the window's
//! Esc ladder and shortcuts see it).
use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::mouse::Cursor;
use iced::widget::canvas::Program;
use iced::{Point, Rectangle, Size, event::Status};

use super::{Input, Pointer, ScreenMap, grid_span};
use crate::features::snip::annotate::{Annotator, Edit, Image, Item, Message, Shape, Tool};

fn studio() -> Annotator {
    let mut e = Annotator::new(
        Image {
            width: 100,
            height: 80,
            rgba: [255, 255, 255, 255].repeat(8000),
        },
        1.0,
    );
    e.studio = true;
    e.tool = Tool::Select;
    e
}

fn with_box(e: &mut Annotator) {
    e.doc.exec(Edit::Insert(
        0,
        Item {
            shape: Shape::Rect {
                a: Point::new(10.0, 10.0),
                b: Point::new(40.0, 40.0),
            },
            colour: [255, 0, 0, 255],
            width: 2.0,
            outline: false,
            fill: false,
            hidden: false,
        },
    ));
    e.doc.deselect();
}

/// The key pressed: the message it publishes and whether the canvas took it.
fn press(e: &Annotator, key: Key, latin: char, modifiers: Modifiers) -> (Option<Message>, bool) {
    let input = Input {
        editor: e,
        grown: 0.0,
        map: ScreenMap::ONE,
    };
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(100.0, 80.0));
    let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    });
    let _ = latin;
    let mut state = Pointer::default();
    match input.update(&mut state, &event, bounds, Cursor::Unavailable) {
        Some(a) => {
            let (m, _, status) = a.into_inner();
            (m, status == Status::Captured)
        }
        None => (None, false),
    }
}

fn ch(c: &str) -> Key {
    Key::Character(c.into())
}

#[test]
fn esc_is_the_canvas_s_only_when_something_is_selected() {
    let mut e = studio();
    with_box(&mut e);
    let esc = || Key::Named(Named::Escape);
    // nothing selected, Select tool: left for the window's ladder
    assert_eq!(press(&e, esc(), ' ', Modifiers::empty()), (None, false));
    // a selection: Deselect, captured
    e.doc.select(&[0]);
    assert_eq!(
        press(&e, esc(), ' ', Modifiers::empty()),
        (Some(Message::Deselect), true)
    );
    // nothing selected but another tool: back to Select, captured
    e.doc.deselect();
    e.tool = Tool::Rect;
    assert_eq!(
        press(&e, esc(), ' ', Modifiers::empty()),
        (Some(Message::Tool(Tool::Select)), true)
    );
}

#[test]
fn a_blocked_canvas_takes_no_key() {
    let mut e = studio();
    with_box(&mut e);
    e.doc.select(&[0]);
    e.keys_blocked = true;
    for (key, mods) in [
        (Key::Named(Named::Escape), Modifiers::empty()),
        (Key::Named(Named::Delete), Modifiers::empty()),
        (Key::Named(Named::ArrowLeft), Modifiers::empty()),
        (ch("r"), Modifiers::empty()),
        (ch("c"), Modifiers::CTRL),
        (ch("a"), Modifiers::CTRL),
    ] {
        assert_eq!(press(&e, key.clone(), ' ', mods), (None, false), "{key:?}");
    }
    e.keys_blocked = false;
    assert!(press(&e, Key::Named(Named::Delete), ' ', Modifiers::empty()).1);
}

#[test]
fn save_as_is_the_window_s_and_ctrl_enter_adds_to_steps() {
    let e = studio();
    // Ctrl+Shift+S: not Save (the window opens Save as)
    assert_eq!(
        press(&e, ch("s"), 's', Modifiers::CTRL | Modifiers::SHIFT),
        (None, false)
    );
    assert_eq!(
        press(&e, Key::Named(Named::Enter), ' ', Modifiers::CTRL),
        (Some(Message::AddToSteps), true)
    );
}

#[test]
fn ctrl_b_is_bold_while_a_text_is_typed() {
    let mut e = studio();
    e.update(Message::Tool(Tool::Text));
    e.update(Message::Press(Point::new(20.0, 20.0)));
    e.update(Message::Release);
    assert!(e.typing());
    let bold = e.text_style.bold;
    let (m, captured) = press(&e, ch("b"), 'b', Modifiers::CTRL);
    assert!(captured);
    let Some(Message::TextStyle(style)) = m else {
        panic!("a text style, got {m:?}")
    };
    assert_eq!(style.bold, !bold);
}

#[test]
fn the_pixel_grid_draws_only_what_the_stage_shows() {
    let all = Rectangle::new(Point::ORIGIN, Size::new(8000.0, 6000.0)); // a 1000x750 picture at 800 %
    let ((c0, c1), (r0, r1)) = grid_span(all, 8.0);
    assert_eq!((c1 - c0, r1 - r0), (1000, 750));
    let seen = Rectangle::new(Point::new(800.0, 400.0), Size::new(400.0, 200.0));
    let ((c0, c1), (r0, r1)) = grid_span(seen, 8.0);
    assert_eq!(
        (c0, c1, r0, r1),
        (100, 150, 50, 75),
        "only the visible pixels"
    );
}
