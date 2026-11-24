//! Studio object editing, second part (review fixes): every option on a
//! single and a mixed selection, the "is it ours" probe, cut / paste, merge
//! order, several-selected markers. Ids as in `objects_tests`.
use super::objects_tests::{CLIP, click, rect, studio};
use super::tests::{RED, item, px};
use super::*;

const GREEN: [u8; 4] = [52, 199, 89, 255];
const BLUE: [u8; 4] = [10, 132, 255, 255];

/// Two boxes that differ in every option, then a text; all selected.
fn mixed() -> Annotator {
    let mut e = studio();
    e.doc.items.truncate(2);
    e.doc.items[1].colour = GREEN;
    e.doc.items[1].width = 8.0;
    e.doc.items[1].outline = true;
    e.doc.items[1].fill = true;
    let text = item(Shape::Text {
        at: Point::new(20.0, 200.0),
        text: "hi".into(),
        size: 14.0,
        style: TextStyle::default(),
    });
    e.doc.exec(Edit::Insert(2, text));
    e.refresh();
    e.update(Message::SelectAll);
    e
}

type Look = ([u8; 4], f32, bool, bool);

fn state(e: &Annotator) -> Vec<Look> {
    (e.doc.items.iter())
        .map(|i| (i.colour, i.width, i.outline, i.fill))
        .collect()
}

#[test]
fn s1_4_every_option_writes_to_a_single_selection() {
    let mut e = studio();
    click(&mut e, 30.0, 30.0);
    e.update(Message::Outline(true));
    assert!(e.doc.items[0].outline);
    e.update(Message::Fill(true));
    assert!(e.doc.items[0].fill);
    e.update(Message::Width(Width::Thick));
    assert_eq!(e.doc.items[0].width, Width::Thick.stroke());
    e.update(Message::Colour(BLUE));
    assert_eq!(e.doc.items[0].colour, BLUE);
    e.update(Message::Outline(false));
    assert!(!e.doc.items[0].outline);
}

#[test]
fn s1_4_every_option_writes_to_a_mixed_selection_and_only_it() {
    let boxes = |e: &Annotator| state(e)[..2].to_vec();
    // outline On: the second box already is, the first must follow
    let mut e = mixed();
    let before = boxes(&e);
    let depth = e.doc.depth();
    e.update(Message::Outline(true));
    let after = boxes(&e);
    assert!(after.iter().all(|s| s.2), "{after:?}");
    let rest = |v: &[Look]| v.iter().map(|s| (s.0, s.1, s.3)).collect::<Vec<_>>();
    assert_eq!(
        rest(&after),
        rest(&before),
        "colour, width, fill stay mixed"
    );
    assert_eq!(e.doc.depth(), depth + 1);
    // fill
    let mut e = mixed();
    let before = boxes(&e);
    e.update(Message::Fill(true));
    let after = boxes(&e);
    assert!(after.iter().all(|s| s.3));
    assert_eq!(after[0].2, before[0].2);
    assert_eq!(after[1].1, 8.0);
    // stroke
    let mut e = mixed();
    e.update(Message::Width(Width::Thick));
    let thick = Width::Thick.stroke();
    assert_eq!(
        boxes(&e).iter().map(|s| s.1).collect::<Vec<_>>(),
        [thick, thick]
    );
    assert_eq!(boxes(&e)[1].0, GREEN, "colour stays mixed");
    assert!(boxes(&e)[1].2 && !boxes(&e)[0].2, "outline stays mixed");
    // colour
    let mut e = mixed();
    e.update(Message::Colour(BLUE));
    assert!(state(&e).iter().all(|s| s.0 == BLUE));
    assert_eq!(
        boxes(&e).iter().map(|s| s.1).collect::<Vec<_>>(),
        [4.0, 8.0]
    );
    // text size touches the text only
    let mut e = mixed();
    let before = state(&e);
    e.update(Message::TextSize(30.0));
    assert!(matches!(e.doc.items[2].shape, Shape::Text { size, .. } if size == 30.0));
    assert_eq!(&state(&e)[..2], &before[..2], "the boxes are untouched");
}

#[test]
fn s1_2_cut_pastes_in_place_first_then_offsets() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    click(&mut e, 30.0, 30.0);
    let Some(Output::Copy(png)) = e.update(Message::CutSelection) else {
        panic!("cut")
    };
    assert!(e.paste_clipboard(Some(png.clone())));
    let last = e.doc.items.len() - 1;
    assert_eq!(e.doc.items[last].frame().position(), Point::new(10.0, 10.0));
    assert!(e.paste_clipboard(Some(png)));
    assert_eq!(
        e.doc.items[last + 1].frame().position(),
        Point::new(20.0, 20.0)
    );
    // after a copy the first paste is offset
    e.update(Message::SelectAll);
    let Some(Output::Copy(png)) = e.update(Message::CopySelection) else {
        panic!("copy")
    };
    let n = e.doc.items.len();
    e.paste_clipboard(Some(png));
    assert_eq!(
        e.doc.items[n].frame().position(),
        e.doc.items[0].frame().position() + Vector::new(10.0, 10.0)
    );
    clear_clipboard();
    assert!(
        e.paste_clipboard(Some(Crop {
            width: 5,
            height: 5,
            rgba: [1, 2, 3, 255].repeat(25)
        })) && matches!(
            e.doc.items.last().map(|i| &i.shape),
            Some(Shape::Picture { .. })
        )
    );
}

fn strips(w: u32, h: u32) -> Crop {
    Crop {
        width: w,
        height: h,
        rgba: (0..w * h)
            .flat_map(|i| {
                if (i / w).is_multiple_of(2) {
                    [200, 30, 30, 255]
                } else {
                    [0, 0, 0, 0]
                }
            })
            .collect(),
    }
}

fn flatten_on(c: &Crop, bg: u8) -> Crop {
    Crop {
        width: c.width,
        height: c.height,
        rgba: (c.rgba.chunks(4))
            .flat_map(|p| {
                let a = u32::from(p[3]);
                let mix = |v: u8| ((u32::from(v) * a + u32::from(bg) * (255 - a)) / 255) as u8;
                [mix(p[0]), mix(p[1]), mix(p[2]), 255]
            })
            .collect(),
    }
}

#[test]
fn s1_2_the_ours_check_survives_a_dropped_alpha() {
    let ours = strips(40, 30);
    assert!(objects::similar(&ours, &ours.clone()), "as it went out");
    assert!(objects::similar(&ours, &flatten_on(&ours, 255)), "on white");
    assert!(objects::similar(&ours, &flatten_on(&ours, 0)), "on black");
}

#[test]
fn s1_2_the_ours_check_refuses_foreign_pictures() {
    let ours = strips(40, 30);
    let blank = Crop {
        width: 40,
        height: 30,
        rgba: [0, 0, 0, 0].repeat(1200),
    };
    assert!(!objects::similar(&ours, &blank), "all see-through");
    assert!(
        !objects::similar(&blank, &blank.clone()),
        "nothing to tell apart"
    );
    let white = Crop {
        width: 40,
        height: 30,
        rgba: [255, 255, 255, 255].repeat(1200),
    };
    assert!(!objects::similar(&ours, &white));
    assert!(!objects::similar(&ours, &strips(41, 30)), "another size");
}

#[test]
fn s1_2_merge_keeps_what_the_user_saw() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    e.doc.items.truncate(1); // a box under the picture
    let green = Crop {
        width: 30,
        height: 30,
        rgba: [0, 200, 0, 255].repeat(900),
    };
    e.paste_clipboard(Some(green)); // the picture over it
    e.doc
        .exec(Edit::Insert(2, rect(300.0, 200.0, 340.0, 240.0))); // a box over it
    e.doc.select(&[1]);
    let depth = e.doc.depth();
    e.update(Message::Merge);
    assert_eq!(e.doc.items.len(), 1, "only the box above stays an object");
    assert_eq!(e.doc.items[0].frame().x, 300.0);
    assert_eq!(e.doc.depth(), depth + 1);
    let at = (30 * 400 + 10) * 4;
    assert_eq!(
        e.doc.image.rgba[at..at + 3],
        RED[..3],
        "the lower box is baked in"
    );
    e.update(Message::Undo);
    assert_eq!(e.doc.items.len(), 3);
    assert_eq!(px(&render(&e.doc), 10, 30), RED);
}

#[test]
fn s5_several_selected_show_ends_and_corners() {
    let arrow = item(Shape::Arrow {
        a: Point::new(10.0, 10.0),
        b: Point::new(50.0, 40.0),
        bend: Point::new(30.0, 25.0),
        head: true,
        text: String::new(),
    });
    assert_eq!(select::marks(&arrow).len(), 2, "its two ends");
    let pen = item(Shape::Pen(vec![
        Point::new(5.0, 5.0),
        Point::new(40.0, 30.0),
    ]));
    assert_eq!(select::marks(&pen).len(), 4, "a light marker: its corners");
    assert_eq!(select::marks(&rect(0.0, 0.0, 10.0, 10.0)).len(), 4);
    let mut e = studio();
    e.doc.exec(Edit::Insert(3, pen));
    e.update(Message::SelectAll);
    assert_eq!(e.handle_marks().len(), 16);
}
