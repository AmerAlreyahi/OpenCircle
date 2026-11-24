//! W102 step 3: blur once, mask many. The numbers (`cargo test --release
//! w102_3 -- --ignored --nocapture`) are in the report; the plain test checks
//! the commit keeps what a full repaint would give.
use super::blur_tests::pt;
use super::tests::image_of;
use super::*;
use std::time::Instant;

fn busy(w: u32, h: u32) -> Image {
    image_of(w, h, |x, y| {
        if (x / 7 + y / 5) % 3 != 0 {
            [250, 250, 250, 255]
        } else {
            [10, 20, 30, 255]
        }
    })
}

fn stroke(e: &mut Annotator, y: f32, n: u32) -> (Vec<f32>, f32) {
    e.update(Message::Press(pt(100.0, y)));
    let mut frames = Vec::new();
    for i in 1..=n {
        let t = Instant::now();
        e.update(Message::Move(pt(100.0 + 20.0 * i as f32, y)));
        frames.push(t.elapsed().as_secs_f32() * 1000.0);
    }
    let t = Instant::now();
    e.update(Message::Release);
    (frames, t.elapsed().as_secs_f32() * 1000.0)
}

/// W102-3.a: after committed strokes the overlay holds exactly what a full
/// repaint paints over their region, and the picture itself was not flattened
/// again; the next repaint folds the overlay in and clears it.
#[test]
fn w102_3a_a_committed_stroke_matches_a_full_repaint() {
    let mut e = Annotator::new(busy(900, 400), 1.0);
    e.assist = false;
    e.update(Message::Tool(Tool::Blur));
    let before = e.base_pixels();
    stroke(&mut e, 100.0, 20);
    stroke(&mut e, 250.0, 15);
    assert!(
        before == e.base_pixels(),
        "the picture is not flattened again"
    );
    let (rect, patch) = e.patch_pixels().expect("an overlay");
    e.refresh();
    assert!(e.patch.is_none(), "the repaint folds it in");
    let full = e.base_pixels();
    let (x0, y0, w) = (rect.x as usize, rect.y as usize, rect.width as usize);
    let mut painted = 0;
    for (i, px) in patch.as_chunks::<4>().0.iter().enumerate() {
        if px[3] == 0 {
            continue; // between the strokes: the picture shows
        }
        painted += 1;
        let at = ((y0 + i / w) * 900 + x0 + i % w) * 4;
        assert_eq!(px[..], full[at..at + 4], "pixel {i}");
    }
    assert!(painted > 1000, "{painted} painted");
}

/// The measurement (not part of the gate).
#[test]
#[ignore]
fn w102_3_measure_the_drag_frame_and_the_commit() {
    for (w, h) in [(1920, 1080), (3840, 2160), (7680, 4320)] {
        let mut e = Annotator::new(busy(w, h), 1.0);
        e.assist = false;
        e.update(Message::Tool(Tool::Blur));
        let (mut frames, commit) = stroke(&mut e, h as f32 / 2.0, 60);
        frames.sort_by(f32::total_cmp);
        eprintln!(
            "{w}x{h}: drag frame median {:.2} ms, p95 {:.2} ms, last {:.2} ms; commit {:.1} ms",
            frames[frames.len() / 2],
            frames[frames.len() * 95 / 100],
            frames[frames.len() - 1],
            commit
        );
    }
}
