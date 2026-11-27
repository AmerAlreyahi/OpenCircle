//! B-01: Studio drawing speed at every zoom (above 100 % was slow while below was smooth).
use super::r19::{StudioFrame, StudioLab, summarize};
use super::*;

const ZOOMS: [f32; 6] = [0.485, 1.0, 1.105, 1.5, 2.0, 3.0];

fn open_at(w: u32, h: u32, z: f32) -> StudioLab {
    let mut lab = StudioLab::open(w, h, Size::new(3440.0, 1440.0), 1.0);
    let _ = lab.s.zoom_to(z, None);
    let _ = lab.s.take_scroll();
    lab.reset_scroll();
    lab.settle_mip();
    let _ = lab.render(0.0);
    lab
}

#[test]
#[ignore = "timing: run by hand, in release"]
fn b01_lab_measure() {
    for (w, h) in [(3120u32, 1440u32), (5120, 2880)] {
        for z in ZOOMS {
            let mut lab = open_at(w, h, z);
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
            let _ = lab.render(0.0);
            let mut fr = Vec::new();
            let u = lab.send(Msg::Editor(Message::Press(Point::new(100.0, 80.0))));
            fr.push(lab.render(u));
            for i in 1..=30 {
                let t = i as f32 / 30.0;
                let u = lab.send(Msg::Editor(Message::Move(Point::new(
                    100.0 + 600.0 * t,
                    80.0 + 320.0 * t,
                ))));
                fr.push(lab.render(u));
            }
            let draw: Vec<f64> = fr.iter().map(StudioFrame::total_ms).collect();
            let cu = lab.send(Msg::Editor(Message::Release));
            let cf = lab.render(cu);
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Select)));
            let hit = Point::new(100.0, 240.0);
            let u = lab.send(Msg::Editor(Message::Press(hit)));
            let _ = lab.render(u);
            let mut mv = Vec::new();
            for i in 1..=20 {
                let t = i as f32 / 20.0;
                let u = lab.send(Msg::Editor(Message::Move(Point::new(
                    hit.x + 300.0 * t,
                    hit.y + 150.0 * t,
                ))));
                mv.push(lab.render(u));
            }
            let move_t: Vec<f64> = mv.iter().map(StudioFrame::total_ms).collect();
            let split = |v: &[StudioFrame]| {
                let n = v.len() as f64;
                (
                    v.iter().map(|f| f.update_ms).sum::<f64>() / n,
                    v.iter().map(|f| f.draw_ms).sum::<f64>() / n,
                    v.iter().map(|f| f.raster_ms).sum::<f64>() / n,
                )
            };
            let (a, b, c) = split(&mv);
            println!(
                "{w}x{h} @ {:5.1}%  rect [{}]  move [{}] (upd {a:.1} draw {b:.1} rast {c:.1})  commit {:.1}",
                z * 100.0,
                summarize(&draw),
                summarize(&move_t),
                cf.total_ms()
            );
        }
    }
}

/// B-01: the picture at 110.5 % and 150 % no longer goes through tiny-skia's per-pixel
/// bilinear pipeline (~100 ms a frame), and a drag leaves every pixel outside the shape alone.
#[test]
fn b01_enlarged_picture_skips_the_pipeline_and_drag_keeps_outside_pixels() {
    for z in [1.105f32, 1.5] {
        let mut lab = open_at(1600, 900, z);
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
        let _ = lab.render(0.0);
        let before = lab.pixels();
        let (pic, stride) = (lab.picture_phys_rect(), lab.pixmap.width());
        let piped = iced_tiny_skia::PIPELINE_PIXELS.get();
        let _ = lab.send(Msg::Editor(Message::Press(Point::new(100.0, 80.0))));
        let _ = lab.render(0.0);
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(
                100.0 + 300.0 * t,
                80.0 + 160.0 * t,
            ))));
            let _ = lab.render(0.0);
        }
        let piped = (iced_tiny_skia::PIPELINE_PIXELS.get() - piped) as f64;
        assert!(
            piped == 0.0,
            "zoom {z}: {piped} px through the pipeline over 9 frames (the slow path)"
        );
        let excl = lab.image_rect_to_phys(
            Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 160.0)),
            80.0,
        );
        let (d, md, n) = super::r19::diff_outside(&before, &lab.pixels(), stride, pic, &[excl]);
        assert_eq!(d, 0, "zoom {z}: {d}/{n} outside px changed (max {md})");
    }
}

/// B-01 parity: an enlarged Linear picture is byte for byte what tiny-skia's own
/// `draw_pixmap` (the path every such frame took before) writes: many zooms, whole and
/// fractional offsets, odd sizes, DPI 1 / 1.5 / 2, edges on a pixel middle, clipped cases.
#[test]
fn b01_enlarged_linear_picture_matches_tiny_skia_byte_for_byte() {
    use iced::advanced::Renderer as _;
    use iced::advanced::graphics::Viewport;
    use iced::advanced::image::Renderer as _;
    use iced::advanced::image::{FilterMethod, Handle, Image};
    let (pw, ph) = (180u32, 130u32);
    let bg = crate::shared::kit::themes::c().bg;
    let frame = |r: &mut iced::Renderer, k: f32| {
        let mut pix = tiny_skia::Pixmap::new(pw, ph).unwrap();
        let mut mask = tiny_skia::Mask::new(pw, ph).unwrap();
        let vp = Viewport::with_physical_size(Size::new(pw, ph), k);
        let all = Rectangle::with_size(Size::new(pw as f32 / k, ph as f32 / k));
        r.draw(&mut pix.as_mut(), &mut mask, &vp, &[all], bg);
        pix
    };
    let (mut bad, mut cases, mut seed) = (Vec::new(), 0, 7u32);
    for (iw, ih) in [(37u32, 23u32), (64, 48), (101, 57)] {
        let mut rgba = Vec::new();
        for _ in 0..iw * ih {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let b = seed.to_le_bytes();
            rgba.extend([b[1], b[2], b[3], 255]);
        }
        for k in [1.0f32, 1.5, 2.0] {
            for z in [1.01f32, 1.105, 1.3333, 1.5, 1.75, 1.99] {
                for (ox, oy) in [
                    (0.0f32, 0.0f32),
                    (3.0, 3.0),
                    (2.3, 7.7),
                    (-5.5, -3.25),
                    (0.5, 0.5),
                    (40.0, 12.0),
                ] {
                    for clip in [
                        None,
                        Some(Rectangle::new(
                            Point::new(20.0, 10.0),
                            Size::new(70.0, 50.0),
                        )),
                    ] {
                        let s = z / k;
                        let bounds = Rectangle::new(
                            Point::new(ox / k, oy / k),
                            Size::new(iw as f32 * s, ih as f32 * s),
                        );
                        let clip_r = clip.map_or(bounds, |c| {
                            Rectangle::new(
                                Point::new(c.x / k, c.y / k),
                                Size::new(c.width / k, c.height / k),
                            )
                        });
                        let mut r = crate::shared::kit::testing::shot::renderer();
                        r.with_layer(clip_r, |r| {
                            let img = Image::new(Handle::from_rgba(iw, ih, rgba.clone()))
                                .filter_method(FilterMethod::Linear);
                            r.draw_image(img, bounds, clip_r);
                        });
                        let p0 = iced_tiny_skia::PIPELINE_PIXELS.get();
                        let got = frame(&mut r, k);
                        let fast = iced_tiny_skia::PIPELINE_PIXELS.get() == p0;
                        // the old path: tiny-skia's own bilinear draw, through the same renderer
                        iced_tiny_skia::TINY_SKIA_ENLARGE.set(true);
                        let want = frame(&mut r, k);
                        let slow = iced_tiny_skia::PIPELINE_PIXELS.get() > p0;
                        iced_tiny_skia::TINY_SKIA_ENLARGE.set(false);
                        cases += 1;
                        if !(fast && slow) {
                            bad.push(format!(
                                "{iw}x{ih} k{k} z{z} at ({ox},{oy}): fast {fast}, slow {slow}"
                            ));
                        }
                        let n = got
                            .data()
                            .iter()
                            .zip(want.data())
                            .filter(|(a, b)| a != b)
                            .count();
                        if n > 0 {
                            bad.push(format!(
                                "{iw}x{ih} k{k} z{z} at ({ox},{oy}) clip {}: {n} bytes",
                                clip.is_some()
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{}/{cases} cases differ, first: {:#?}",
        bad.len(),
        &bad[..bad.len().min(8)]
    );
}
