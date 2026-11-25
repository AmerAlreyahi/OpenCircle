//! Drawing stays inside the picture (planning #200, `inside`).
use super::*;
use iced::{Point, Rectangle, Size};

fn pt(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

/// A 200 x 120 frozen frame with Snip's live area (40, 20)-(140, 80).
fn snip() -> Annotator {
    let image = Image {
        width: 200,
        height: 120,
        rgba: vec![255; 200 * 120 * 4],
    };
    let mut e = Annotator::new(image, 1.0);
    e.assist = false;
    e.movable = true;
    e.doc.crop = Some(Rectangle::new(pt(40.0, 20.0), Size::new(100.0, 60.0)));
    e
}

fn draw(e: &mut Annotator, tool: Tool, from: Point, to: Point) {
    e.update(Message::Tool(tool));
    e.update(Message::Press(from));
    e.update(Message::Move(to));
    e.update(Message::Release);
}

/// The whole of the item (its frame with half its line) lies in `view`.
fn shows_whole(e: &Annotator, view: Rectangle) -> bool {
    let item = e.doc.items.last().expect("an item");
    let f = item.frame().expand(item.width / 2.0);
    f.x >= view.x - 0.01
        && f.y >= view.y - 0.01
        && f.x + f.width <= view.x + view.width + 0.01
        && f.y + f.height <= view.y + view.height + 0.01
}

/// 200-11: in a Snip area a drag past the edge stops at the area's last
/// pixels, so the whole shape shows: a rectangle, an ellipse, an arrow, a
/// stroke.
#[test]
fn g200_11_shapes_and_strokes_stop_at_the_areas_last_pixels() {
    for tool in [
        Tool::Rect,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Line,
        Tool::Pen,
    ] {
        let mut e = snip();
        draw(&mut e, tool, pt(60.0, 40.0), pt(900.0, 700.0));
        let view = e.doc.view();
        assert_eq!(e.doc.items.len(), 1, "{tool:?}");
        assert!(
            shows_whole(&e, view),
            "{tool:?}: {:?}",
            e.doc.items[0].frame()
        );
        // and it reached the edge (within its own line's room), not stopped early
        let f = e.doc.items[0].frame();
        assert!(f.x + f.width > view.x + view.width - 14.0, "{tool:?} {f:?}");
        assert!(
            f.y + f.height > view.y + view.height - 14.0,
            "{tool:?} {f:?}"
        );
        // the other way too
        let mut e = snip();
        draw(&mut e, tool, pt(120.0, 70.0), pt(-900.0, -700.0));
        assert!(shows_whole(&e, e.doc.view()), "{tool:?} back");
    }
}

/// 200-12: typed text wraps before the area's right edge when drawn, EVERY line of it (the saved
/// text has no break the user did not type); a text that fits stays one line.
#[test]
fn g200_12_typed_text_wraps_before_the_areas_edge_at_draw_time() {
    let mut e = snip();
    e.update(Message::Tool(Tool::Text));
    e.update(Message::Press(pt(50.0, 30.0)));
    e.update(Message::Release);
    let item = |e: &Annotator| match &e.pending.as_ref().unwrap().item.shape {
        Shape::Text {
            at,
            text,
            size,
            style,
            wrap,
        } => (*at, text.clone(), *size, *style, *wrap),
        other => panic!("{other:?}"),
    };
    e.update(Message::Type("ok".into()));
    let (at, typed, size, style, wrap) = item(&e);
    assert_eq!(typed, "ok");
    let one = text::measure_styled("ok", size, style, wrap).height;
    // a long text with a typed line break: both lines wrap
    e.update(Message::Type(
        " a long sentence that cannot fit one line of this area\nand a second long line that wraps too".into(),
    ));
    let (_, typed, _, _, wrap) = item(&e);
    assert_eq!(
        typed.matches('\n').count(),
        1,
        "only the typed break: {typed:?}"
    );
    let room = wrap.expect("soft wrap set");
    let right = e.doc.view().x + e.doc.view().width;
    assert!(at.x + room <= right, "{room} from {}", at.x);
    let m = text::measure_styled(&typed, size, style, wrap);
    assert!(m.width <= room + 1.0, "{} wide, room {room}", m.width);
    assert!(
        m.height > 3.0 * one,
        "wrapped over several lines: {}",
        m.height / one
    );
    // unwrapped it would be wider than the area: that is what the wrap is for
    assert!(text::measure_styled(&typed, size, style, None).width > room);
}

/// 200-13: the soft wrap is not saved as breaks, follows a move, and is off where the picture grows.
#[test]
fn g200_13_soft_wrap_follows_the_area_and_is_not_a_break_in_the_text() {
    let mut e = snip();
    e.update(Message::Tool(Tool::Text));
    e.update(Message::Press(pt(50.0, 30.0)));
    e.update(Message::Release);
    e.update(Message::Type(
        "a long sentence that cannot fit one line".into(),
    ));
    e.update(Message::Finish);
    let Shape::Text { text, wrap, .. } = &e.doc.items[0].shape else {
        panic!()
    };
    assert!(!text.contains('\n') && wrap.is_some(), "{text:?} {wrap:?}");
    // a picture that grows: no wrap
    let mut item = e.doc.items[0].clone();
    inside::fit_wrap(&mut item, e.doc.view(), false);
    assert!(matches!(item.shape, Shape::Text { wrap: None, .. }));
}

/// 200-14: where the picture grows (the Studio, "Grow the canvas" on, no
/// crop) nothing is held back; with it off the picture's own edge holds.
#[test]
fn g200_14_the_studio_holds_at_the_picture_only_when_it_cannot_grow() {
    let studio = |grow: bool| {
        let image = Image {
            width: 200,
            height: 120,
            rgba: vec![255; 200 * 120 * 4],
        };
        let mut e = Annotator::new(image, 1.0);
        e.assist = false;
        e.studio = true;
        e.grow_canvas = grow;
        e
    };
    let mut e = studio(true);
    draw(&mut e, Tool::Rect, pt(50.0, 50.0), pt(260.0, 100.0));
    let Shape::Rect { b, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert!(
        b.x > 200.0 - 1.0,
        "the grown canvas keeps the rectangle: {b:?}"
    );
    let mut e = studio(false);
    draw(&mut e, Tool::Rect, pt(50.0, 50.0), pt(260.0, 400.0));
    assert!(shows_whole(&e, e.doc.view()));
}
