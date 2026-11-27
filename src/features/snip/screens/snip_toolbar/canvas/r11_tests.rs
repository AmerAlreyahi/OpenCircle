//! R-11: a right press on the picked area opens its menu; mid-stroke it cancels the stroke.
use iced::mouse::{self, Cursor};
use iced::widget::canvas::Program;
use iced::{Point, Rectangle, Size};

use super::{Input, Pointer, ScreenMap};
use crate::features::snip::annotate::{Annotator, Image, Message, Tool};

const AREA: Rectangle = Rectangle {
    x: 50.0,
    y: 50.0,
    width: 200.0,
    height: 150.0,
};

fn snip() -> Annotator {
    let mut e = Annotator::new(
        Image {
            width: 400,
            height: 300,
            rgba: [255, 255, 255, 255].repeat(400 * 300),
        },
        1.0,
    );
    e.doc.crop = Some(AREA);
    e.movable = true;
    e.tool = Tool::Pen;
    e.refresh();
    e
}

/// Mouse events fed to the real canvas, each message it publishes applied to the editor.
fn drive(e: &mut Annotator, events: &[mouse::Event]) -> Vec<Message> {
    drive_with(e, &mut Pointer::default(), events)
}

fn drive_with(e: &mut Annotator, state: &mut Pointer, events: &[mouse::Event]) -> Vec<Message> {
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(400.0, 300.0));
    let mut cursor = Cursor::Unavailable;
    let mut out = Vec::new();
    for ev in events {
        if let mouse::Event::CursorMoved { position } = ev {
            cursor = Cursor::Available(*position);
        }
        let ev = iced::Event::Mouse(*ev);
        let input = Input {
            editor: e,
            grown: 0.0,
            map: ScreenMap::ONE,
        };
        let message = input
            .update(state, &ev, bounds, cursor)
            .and_then(|a| a.into_inner().0);
        if let Some(m) = message {
            e.update(m.clone());
            out.push(m);
        }
    }
    out
}

fn at(x: f32, y: f32) -> mouse::Event {
    mouse::Event::CursorMoved {
        position: Point::new(x, y),
    }
}

fn button(b: mouse::Button) -> mouse::Event {
    mouse::Event::ButtonPressed(b)
}

/// R-11.1: a right press inside the picked area opens the menu there; outside it, and in the
/// Studio, nothing (today's behaviour).
#[test]
fn r11_1_a_right_press_on_the_area_opens_the_menu_there() {
    let mut e = snip();
    let got = drive(&mut e, &[at(100.0, 100.0), button(mouse::Button::Right)]);
    assert_eq!(got, [Message::AreaMenu(Some(Point::new(100.0, 100.0)))]);
    assert_eq!(e.area_menu, Some(Point::new(100.0, 100.0)));
    assert_eq!(e.row, None, "a pointer's menu has no keyboard row yet");
    let mut e = snip();
    assert!(drive(&mut e, &[at(10.0, 10.0), button(mouse::Button::Right)]).is_empty());
    assert_eq!(e.area_menu, None);
    let mut e = snip();
    e.studio = true;
    assert!(drive(&mut e, &[at(100.0, 100.0), button(mouse::Button::Right)]).is_empty());
}

/// R-11.2: a right press while a stroke is being drawn cancels that stroke: nothing is drawn,
/// no menu; releasing the left button afterwards adds nothing.
#[test]
fn r11_2_a_right_press_mid_stroke_cancels_the_stroke() {
    let mut e = snip();
    let got = drive(
        &mut e,
        &[
            at(100.0, 100.0),
            button(mouse::Button::Left),
            at(130.0, 120.0),
            button(mouse::Button::Right),
            at(150.0, 130.0),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ],
    );
    assert!(got.contains(&Message::Cancel), "{got:?}");
    assert!(e.doc.items.is_empty(), "the stroke is gone");
    assert_eq!(e.area_menu, None, "no menu");
    assert!(!got.iter().any(|m| matches!(m, Message::AreaMenu(_))));
}

/// R-11.2: the same while the area itself is being moved: the move is cancelled.
#[test]
fn r11_2_a_right_press_mid_move_puts_the_area_back() {
    let mut e = snip();
    e.tool = Tool::Select;
    let mut state = Pointer::default();
    drive_with(
        &mut e,
        &mut state,
        &[
            at(100.0, 100.0),
            button(mouse::Button::Left),
            at(160.0, 140.0),
        ],
    );
    assert_ne!(e.doc.crop, Some(AREA), "the area moved with the pointer");
    let got = drive_with(&mut e, &mut state, &[button(mouse::Button::Right)]);
    assert_eq!(got, [Message::Cancel]);
    assert_eq!(e.doc.crop, Some(AREA));
    assert_eq!(e.area_menu, None);
}

/// R-11.3: in the Studio a right press mid-stroke is not the Snip cancel: the stroke goes on and
/// the Studio's own context menu opens as on main.
#[test]
fn r11_3_the_studio_keeps_its_right_press() {
    let mut e = snip();
    e.studio = true;
    let mut state = Pointer::default();
    let got = drive_with(
        &mut e,
        &mut state,
        &[
            at(100.0, 100.0),
            button(mouse::Button::Left),
            at(130.0, 120.0),
            button(mouse::Button::Right),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ],
    );
    assert!(!got.contains(&Message::Cancel), "{got:?}");
    assert!(
        got.contains(&Message::Release),
        "the stroke was not cancelled: {got:?}"
    );
    assert_eq!(e.area_menu, None);
}
