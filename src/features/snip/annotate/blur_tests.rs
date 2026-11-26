//! W102 step 1: Hide's consistent fill (background + a fixed pull toward the
//! ink, one texture per picture, strength in image px).
use super::tests::{image_of, item, px};
use super::*;

pub(super) fn pt(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn hide(points: Vec<Point>, size: f32, style: blur::Style) -> Item {
    item(Shape::Blur {
        points,
        size,
        cell: 6.0,
        style,
        kind: blur::Kind::Brush,
        outline: 0.0,
        ramp: Vec::new(),
    })
}

/// A card: `bg`, with three lines of dark text.
fn card(bg: [u8; 4]) -> Image {
    image_of(300, 150, |x, y| {
        let line = [30..40, 70..80, 110..120].iter().any(|r| r.contains(&y));
        if line && x % 5 < 3 && (10..280).contains(&x) {
            [20, 20, 30, 255]
        } else {
            bg
        }
    })
}

pub(super) fn rendered(image: Image, seed: u64, items: Vec<Item>) -> Crop {
    let mut doc = Document::new(image);
    doc.seed = seed;
    for i in items {
        doc.exec(Edit::Insert(doc.items.len(), i));
    }
    render(&doc)
}

fn mean(c: &Crop, x0: u32, x1: u32, y0: u32, y1: u32) -> [f32; 3] {
    let mut sum = [0.0; 3];
    for y in y0..y1 {
        for x in x0..x1 {
            let p = px(c, x, y);
            (0..3).for_each(|k| sum[k] += p[k] as f32);
        }
    }
    let n = ((x1 - x0) * (y1 - y0)) as f32;
    sum.map(|v| v / n)
}

/// W102-1.a: two Hides on different lines of one background match within four
/// levels on average (today: median 7, p90 19; the picture's texture itself is
/// +-10 and averages out only over its scale), in both styles, whatever the seed.
#[test]
fn w102_1a_two_strokes_on_one_background_match() {
    for style in [blur::Style::Soft, blur::Style::Frost, blur::Style::Pixelate] {
        for seed in [1, 2, 3, 4] {
            let line = |y: f32| hide(vec![pt(20.0, y), pt(280.0, y)], 16.0, style);
            let out = rendered(
                card([250, 250, 250, 255]),
                seed,
                vec![line(35.0), line(75.0)],
            );
            let (a, b) = (mean(&out, 20, 280, 30, 40), mean(&out, 20, 280, 70, 80));
            for k in 0..3 {
                assert!(
                    (a[k] - b[k]).abs() <= 4.0,
                    "{style:?} seed {seed}: {a:?} {b:?}"
                );
            }
        }
    }
}

/// W102-1.b: the fill is a background pulled 12 % toward the ink, and keeps a
/// pastel's own hue (today it snapped to grey).
#[test]
fn w102_1b_background_pulled_toward_ink_keeps_hue() {
    let lavender = [200, 190, 240, 255];
    let out = rendered(
        card(lavender),
        5,
        vec![hide(
            vec![pt(20.0, 35.0), pt(280.0, 35.0)],
            16.0,
            blur::Style::Frost,
        )],
    );
    let m = mean(&out, 20, 280, 30, 40);
    // 0.88 * bg + 0.12 * ink (20, 20, 30): about (178, 170, 215).
    for (k, want) in [178.0, 170.0, 215.0].into_iter().enumerate() {
        assert!((m[k] - want).abs() <= 4.0, "{m:?}");
    }
    assert!(m[2] - m[0] > 25.0, "still lavender: {m:?}");
}

/// W102-1.c: the texture belongs to the picture: a stroke drawn as two
/// overlapping ones gives the pixels of one stroke (no seam where they meet).
#[test]
fn w102_1c_overlapping_strokes_merge() {
    for style in [blur::Style::Soft, blur::Style::Frost, blur::Style::Pixelate] {
        let one = hide(vec![pt(20.0, 75.0), pt(280.0, 75.0)], 16.0, style);
        let a = hide(vec![pt(20.0, 75.0), pt(160.0, 75.0)], 16.0, style);
        let b = hide(vec![pt(120.0, 75.0), pt(280.0, 75.0)], 16.0, style);
        let whole = rendered(card([250, 250, 250, 255]), 9, vec![one]);
        let joined = rendered(card([250, 250, 250, 255]), 9, vec![a, b]);
        let mut worst = 0;
        for y in 68..82 {
            for x in 22..278 {
                let (p, q) = (px(&whole, x, y), px(&joined, x, y));
                worst = worst.max((0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap());
            }
        }
        assert!(
            worst <= 2,
            "{style:?}: two strokes vs one differ by {worst}"
        );
    }
}

/// W102-1.d: Hide never reads the content: the same line with other glyph
/// positions gives identical pixels (the lab: anything that reads it is
/// recovered 100 %).
#[test]
fn w102_1d_hide_does_not_read_the_text() {
    let shifted = |dx: u32| {
        let mut img = card([250, 250, 250, 255]);
        for y in 30..40 {
            for x in 0..300u32 {
                let i = ((y * 300 + x) * 4) as usize;
                let on = (x + dx) % 5 < 3 && (10..280).contains(&x);
                let v = if on {
                    [20, 20, 30, 255]
                } else {
                    [250, 250, 250, 255]
                };
                img.rgba[i..i + 4].copy_from_slice(&v);
            }
        }
        img
    };
    for style in [blur::Style::Soft, blur::Style::Frost, blur::Style::Pixelate] {
        let at = |dx| {
            let out = rendered(
                shifted(dx),
                3,
                vec![hide(vec![pt(20.0, 35.0), pt(280.0, 35.0)], 16.0, style)],
            );
            // Inside the footprint (x 12..288, y 27..43).
            (30..40)
                .flat_map(|y| (14..286).map(move |x| (x, y)))
                .map(|(x, y)| px(&out, x, y))
                .collect::<Vec<_>>()
        };
        assert_eq!(at(0), at(2), "{style:?}");
    }
}

/// W102-1.e: the strength is in image px times the capture scale, never the
/// zoom: the same stroke at zoom 0.5, 1 and 2 has the same texture scale.
#[test]
fn w102_1e_strength_does_not_follow_the_zoom() {
    let cell_at = |zoom: f32| {
        let mut e = Annotator::new(image_of(400, 200, |_, _| [250, 250, 250, 255]), 1.0);
        e.zoom = zoom;
        e.tool = Tool::Blur;
        e.update(Message::Press(pt(50.0, 100.0)));
        e.update(Message::Move(pt(150.0, 100.0)));
        e.update(Message::Release);
        let Shape::Blur { cell, .. } = e.doc.items[0].shape else {
            panic!("a blur")
        };
        cell
    };
    assert_eq!(cell_at(0.5), cell_at(1.0));
    assert_eq!(cell_at(1.0), cell_at(2.0));
}

/// W102-1.f: a saved Hide from before the style existed (and with the old
/// per-stroke seed) still loads, as the default look.
#[test]
fn w102_1f_old_saved_blur_loads_as_frost() {
    let old = r#"{"Blur":{"points":[[1.0,2.0],[30.0,2.0]],"size":20.0,"cell":6.0,"seed":12345}}"#;
    let shape: Shape = serde_json::from_str(old).expect("old blur loads");
    assert!(
        matches!(
            shape,
            Shape::Blur {
                style: blur::Style::Frost,
                ..
            }
        ),
        "{shape:?}"
    );
}

/// W102 (review): the ink term is read in coarse steps, so ink colours a fringe apart
/// (the same text, other anti-aliasing) give the same fill, and the background stays exact.
#[test]
fn w102_1g_the_ink_is_read_coarsely() {
    let fill = |ink: [u8; 4], bg: [u8; 4]| {
        let img = image_of(300, 150, |x, y| {
            if (30..40).contains(&y) && x % 5 < 3 && (10..280).contains(&x) {
                ink
            } else {
                bg
            }
        });
        let out = rendered(
            img,
            3,
            vec![hide(
                vec![pt(20.0, 35.0), pt(280.0, 35.0)],
                16.0,
                blur::Style::Frost,
            )],
        );
        mean(&out, 30, 270, 30, 40)
    };
    let bg = [243, 241, 250, 255];
    let (a, b) = (fill([20, 20, 30, 255], bg), fill([28, 24, 36, 255], bg));
    for k in 0..3 {
        assert!((a[k] - b[k]).abs() < 0.5, "{a:?} vs {b:?}");
    }
    // a different background shows through exactly (about 12 % of one level per level)
    let c = fill([20, 20, 30, 255], [233, 231, 240, 255]);
    assert!((a[0] - c[0] - 8.8).abs() < 1.5, "{a:?} {c:?}");
}

/// W114-1: the new secure Soft is the default Hide style (not a look); the old
/// content-reading blur stays as Blur (look only).
#[test]
fn w114_1_soft_is_default_and_blur_is_a_look() {
    assert_eq!(blur::Style::default(), blur::Style::Soft);
    assert!(!blur::Style::Soft.is_look());
    assert!(!blur::Style::Frost.is_look());
    assert!(!blur::Style::Pixelate.is_look());
    assert!(blur::Style::Blur.is_look());
    assert!(blur::Style::Frosted.is_look());
    assert!(blur::Style::Progressive.is_look());
    let e = Annotator::new(card([250, 250, 250, 255]), 1.0);
    assert_eq!(e.blur_style, blur::Style::Soft);
    let styles = blur_styles();
    assert_eq!(styles[0].0, blur::Style::Soft);
    assert_eq!(styles[0].1, "Soft");
    assert_eq!(styles[3].0, blur::Style::Blur);
    assert_eq!(styles[3].1, "Blur (look only)");
}

/// W114-2: two areas that differ ONLY in their text (different glyphs,
/// different ink colours, or text vs blank) on the same background produce
/// BIT-FOR-BIT IDENTICAL Soft Hide pixels (light, dark, gradient, and
/// full-frame fallback; Brush, Rect, and Ellipse).
#[test]
fn w114_2_two_areas_differing_only_in_text_produce_identical_soft_pixels() {
    type BgFn = fn(u32, u32) -> [u8; 4];
    let make = |bg: BgFn, text_variant: u8| {
        image_of(240, 140, |x, y| {
            let inside = (60..180).contains(&x) && (50..90).contains(&y);
            if inside {
                match text_variant {
                    0 => {
                        if (x + y) % 5 < 2 {
                            [20, 20, 30, 255]
                        } else {
                            bg(x, y)
                        }
                    }
                    1 => {
                        if (x * 3 + y * 7) % 6 < 3 {
                            [220, 30, 45, 255]
                        } else {
                            bg(x, y)
                        }
                    }
                    _ => bg(x, y), // blank (no text at all)
                }
            } else {
                bg(x, y)
            }
        })
    };
    let bgs: [(&str, BgFn); 3] = [
        ("light", |_, _| [250, 250, 252, 255]),
        ("dark", |_, _| [24, 28, 38, 255]),
        ("gradient", |x, y| {
            [
                (230.0 - 40.0 * x as f32 / 240.0) as u8,
                (195.0 + 25.0 * y as f32 / 140.0) as u8,
                (210.0 + 35.0 * x as f32 / 240.0) as u8,
                255,
            ]
        }),
    ];
    let shapes = [
        (
            "rect",
            item(Shape::Blur {
                points: vec![pt(56.0, 46.0), pt(184.0, 94.0)],
                size: 16.0,
                cell: 8.0,
                style: blur::Style::Soft,
                kind: blur::Kind::Rect,
                outline: 0.0,
                ramp: Vec::new(),
            }),
        ),
        (
            "ellipse",
            item(Shape::Blur {
                points: vec![pt(36.0, 30.0), pt(204.0, 110.0)],
                size: 16.0,
                cell: 8.0,
                style: blur::Style::Soft,
                kind: blur::Kind::Ellipse,
                outline: 0.0,
                ramp: Vec::new(),
            }),
        ),
        (
            "brush",
            item(Shape::Blur {
                points: vec![pt(60.0, 70.0), pt(180.0, 70.0)],
                size: 48.0,
                cell: 8.0,
                style: blur::Style::Soft,
                kind: blur::Kind::Brush,
                outline: 0.0,
                ramp: Vec::new(),
            }),
        ),
    ];
    for &(bg_name, bg) in &bgs {
        for (sh_name, sh) in &shapes {
            let a = rendered(make(bg, 0), 42, vec![sh.clone()]);
            let b = rendered(make(bg, 1), 42, vec![sh.clone()]);
            let c = rendered(make(bg, 2), 42, vec![sh.clone()]);
            assert_eq!(a.rgba, b.rgba, "{bg_name}/{sh_name}: different text & ink");
            assert_eq!(a.rgba, c.rgba, "{bg_name}/{sh_name}: text vs blank");
        }
    }
    // Full-frame fallback (footprint covers 100 % of the image, empty ring).
    let full_hide = item(Shape::Blur {
        points: vec![pt(0.0, 0.0), pt(96.0, 26.0)],
        size: 16.0,
        cell: 8.0,
        style: blur::Style::Soft,
        kind: blur::Kind::Rect,
        outline: 0.0,
        ramp: Vec::new(),
    });
    let strip = |phase: u32| {
        image_of(96, 26, |x, y| {
            if (6..90).contains(&x) && (5..21).contains(&y) && (x + phase) % 5 < 2 {
                [25, 25, 32, 255]
            } else {
                [255, 255, 255, 255]
            }
        })
    };
    let fa = rendered(strip(0), 77, vec![full_hide.clone()]);
    let fb = rendered(strip(3), 77, vec![full_hide]);
    assert_eq!(fa.rgba, fb.rgba, "full-frame fallback must never leak text");
}

/// W114-3: Soft builds a smooth gradient from the exterior ring across the
/// hidden area (pink on the left, blue on the right stays pink-to-blue), and
/// reads as softly blurred (not invisible) on uniform white and dark cards.
#[test]
fn w114_3_soft_interpolates_ring_gradient_and_stays_visible_on_solid_cards() {
    let split = image_of(240, 140, |x, y| {
        let t = x as f32 / 240.0;
        let bg = [
            (245.0 * (1.0 - t) + 175.0 * t) as u8,
            (185.0 * (1.0 - t) + 210.0 * t) as u8,
            (210.0 * (1.0 - t) + 248.0 * t) as u8,
            255,
        ];
        if (60..180).contains(&x) && (45..95).contains(&y) && x % 4 < 2 {
            [20, 20, 25, 255]
        } else {
            bg
        }
    });
    let box_hide = item(Shape::Blur {
        points: vec![pt(50.0, 35.0), pt(190.0, 105.0)],
        size: 16.0,
        cell: 8.0,
        style: blur::Style::Soft,
        kind: blur::Kind::Rect,
        outline: 0.0,
        ramp: Vec::new(),
    });
    let out = rendered(split, 11, vec![box_hide.clone()]);
    let left = mean(&out, 60, 80, 50, 90);
    let right = mean(&out, 160, 180, 50, 90);
    assert!(
        left[0] > right[0] + 15.0 && right[2] > left[2] + 6.0,
        "pink left -> blue right gradient preserved: left={left:?} right={right:?}"
    );

    // Uniform white & uniform dark: gentle visible veil (~10..20 levels from bg), not erased
    for (name, bg) in [("white", [252, 252, 252, 255]), ("dark", [22, 26, 36, 255])] {
        let out = rendered(card(bg), 11, vec![box_hide.clone()]);
        let m = mean(&out, 70, 170, 50, 90);
        let diff = (m[0] - bg[0] as f32).abs();
        assert!(
            (6.0..=26.0).contains(&diff),
            "{name}: soft veil visible & calm, got mean {m:?} vs bg {bg:?} (diff {diff:.1})"
        );
    }
}

/// W114-4: Soft over a baked red ring hides the border cleanly using the
/// white surroundings (just like w102_2h for Frost).
#[test]
fn w114_4_soft_ring_hides_a_border() {
    let picture = image_of(240, 160, |x, y| {
        let r = ((x as f32 - 120.0).powi(2) + (y as f32 - 80.0).powi(2)).sqrt();
        if (r - 60.0).abs() <= 4.0 {
            [230, 30, 60, 255]
        } else {
            [250, 250, 250, 255]
        }
    });
    let hide = item(Shape::Blur {
        points: vec![pt(60.0, 20.0), pt(180.0, 140.0)],
        size: 16.0,
        cell: 8.0,
        style: blur::Style::Soft,
        kind: blur::Kind::Ellipse,
        outline: 14.0,
        ramp: Vec::new(),
    });
    let out = rendered(picture, 4, vec![hide]);
    let p = px(&out, 180, 80);
    assert!(p[1] > 200 && p[2] > 200, "pale soft ring, got {p:?}");
}

/// Renders a crisp synthetic UI card with bundled fonts (`shot::render`),
/// returning an `Image` ready for `Document` / `blur::paint`.
fn card_image(
    w: u32,
    h: u32,
    rtl: bool,
    view: impl FnOnce() -> iced::Element<'static, ()>,
) -> Image {
    let _ar = rtl.then(crate::shared::kit::testing::arabic);
    let rgba = crate::shared::kit::testing::shot::render(view(), w, h);
    Image {
        width: w,
        height: h,
        rgba,
    }
}

fn light_card() -> Image {
    use iced::widget::{column, container, row, text};
    use iced::{Border, Color, Fill};
    card_image(360, 150, false, || {
        let f = crate::shared::kit::text::ui_font();
        let header = container(
            text("Billing summary")
                .size(13)
                .font(f)
                .color(Color::from_rgb8(54, 46, 92)),
        )
        .width(Fill)
        .padding([8, 14])
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(236, 233, 250).into()),
            ..Default::default()
        });
        let row_line = |k: &'static str, v: &'static str| {
            row![
                text(k)
                    .size(13)
                    .font(f)
                    .color(Color::from_rgb8(96, 102, 118))
                    .width(116),
                text(v).size(13).font(f).color(Color::from_rgb8(24, 28, 38)),
            ]
            .spacing(8)
        };
        let body = container(
            column![
                row_line("Card number", "4929 1234 5678 9012"),
                row_line("Recovery code", "KD8F-22QX-9104"),
                row_line("Account", "sara.h@example.com"),
            ]
            .spacing(12),
        )
        .padding([14, 14]);
        container(column![header, body])
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgb8(250, 250, 252).into()),
                border: Border::default()
                    .width(1)
                    .color(Color::from_rgb8(226, 229, 238)),
                ..Default::default()
            })
            .into()
    })
}

fn dark_card() -> Image {
    use iced::widget::{column, container, row, text};
    use iced::{Border, Color, Fill};
    card_image(360, 150, false, || {
        let f = crate::shared::kit::text::ui_font();
        let header = container(
            text("Workspace keys")
                .size(13)
                .font(f)
                .color(Color::from_rgb8(186, 198, 232)),
        )
        .width(Fill)
        .padding([8, 14])
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(33, 39, 56).into()),
            ..Default::default()
        });
        let row_line = |k: &'static str, v: &'static str| {
            row![
                text(k)
                    .size(13)
                    .font(f)
                    .color(Color::from_rgb8(142, 152, 178))
                    .width(106),
                text(v)
                    .size(13)
                    .font(f)
                    .color(Color::from_rgb8(236, 240, 248)),
            ]
            .spacing(8)
        };
        let body = container(
            column![
                row_line("License ID", "OC-4420-8819-7305"),
                row_line("Backup PIN", "482910"),
                row_line("Owner", "alex@example.com"),
            ]
            .spacing(12),
        )
        .padding([14, 14]);
        container(column![header, body])
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgb8(24, 28, 38).into()),
                border: Border::default()
                    .width(1)
                    .color(Color::from_rgb8(48, 56, 76)),
                ..Default::default()
            })
            .into()
    })
}

fn arabic_card() -> Image {
    use iced::widget::{column, container, row, text};
    use iced::{Color, Fill};
    card_image(360, 150, true, || {
        let f = crate::shared::kit::text::ui_font();
        let top = container(
            column![
                row![
                    text("4929 1234 5678")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(24, 28, 38)),
                    text("رقم البطاقة:")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(96, 102, 118)),
                ]
                .spacing(12),
                row![
                    text("KD8F-22QX")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(24, 28, 38)),
                    text("رمز الاسترداد:")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(96, 102, 118)),
                ]
                .spacing(12),
            ]
            .spacing(12),
        )
        .width(Fill)
        .height(75)
        .padding([14, 16])
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(250, 250, 252).into()),
            ..Default::default()
        });
        let bot = container(
            column![
                row![
                    text("482910")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(236, 240, 248)),
                    text("الرمز السري:")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(148, 158, 184)),
                ]
                .spacing(12),
                row![
                    text("sara@mail.com")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(236, 240, 248)),
                    text("حساب سارة:")
                        .size(13)
                        .font(f)
                        .color(Color::from_rgb8(148, 158, 184)),
                ]
                .spacing(12),
            ]
            .spacing(12),
        )
        .width(Fill)
        .height(75)
        .padding([14, 16])
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(24, 28, 38).into()),
            ..Default::default()
        });
        column![top, bot].into()
    })
}

fn gradient_avatar_card() -> Image {
    let mut img = image_of(360, 150, |x, y| {
        let tx = x as f32 / 360.0;
        let ty = y as f32 / 150.0;
        // Soft pastel pink-lavender to sky-blue gradient + avatar with red ring
        let mut bg = [
            (244.0 * (1.0 - tx) + 178.0 * tx) as u8,
            (196.0 * (1.0 - ty) + 224.0 * ty) as u8,
            (226.0 * (1.0 - tx) + 248.0 * tx) as u8,
            255,
        ];
        // Fake dark text bars on the left half
        let line = (42..54).contains(&y) || (88..100).contains(&y);
        if line && (24..204).contains(&x) && x % 7 < 4 {
            bg = [34, 30, 48, 255];
        }
        // Avatar circle on the right with a baked red border
        let dx = x as f32 - 286.0;
        let dy = y as f32 - 75.0;
        let dist = (dx * dx + dy * dy).sqrt();
        if (dist - 40.0).abs() <= 4.0 {
            bg = [228, 42, 68, 255];
        } else if dist < 36.0 {
            bg = [194, 178, 232, 255];
        }
        bg
    });
    // Ensure crisp text line stays inside
    let _ = &mut img;
    img
}

fn hide_with(
    points: Vec<Point>,
    size: f32,
    cell: f32,
    style: blur::Style,
    kind: blur::Kind,
    outline: f32,
) -> Item {
    item(Shape::Blur {
        points,
        size,
        cell,
        style,
        kind,
        outline,
        ramp: Vec::new(),
    })
}

fn card_hides(style: blur::Style, kind: u8) -> Vec<Item> {
    match kind {
        // Light & Dark cards: brush on line 1, rect on line 2, ellipse on line 3
        0 => vec![
            hide_with(
                vec![pt(138.0, 56.0), pt(326.0, 56.0)],
                20.0,
                8.0,
                style,
                blur::Kind::Brush,
                0.0,
            ),
            hide_with(
                vec![pt(134.0, 74.0), pt(302.0, 96.0)],
                16.0,
                8.0,
                style,
                blur::Kind::Rect,
                0.0,
            ),
            hide_with(
                vec![pt(132.0, 102.0), pt(308.0, 128.0)],
                16.0,
                8.0,
                style,
                blur::Kind::Ellipse,
                0.0,
            ),
        ],
        // Arabic light + dark card: brush on light line 1, rect on dark line 1
        1 => vec![
            hide_with(
                vec![pt(20.0, 22.0), pt(148.0, 22.0)],
                18.0,
                8.0,
                style,
                blur::Kind::Brush,
                0.0,
            ),
            hide_with(
                vec![pt(16.0, 86.0), pt(106.0, 108.0)],
                16.0,
                8.0,
                style,
                blur::Kind::Rect,
                0.0,
            ),
        ],
        // Gradient + avatar: rect on top gradient line, brush on bottom, ring on red avatar border
        _ => vec![
            hide_with(
                vec![pt(20.0, 38.0), pt(210.0, 58.0)],
                16.0,
                8.0,
                style,
                blur::Kind::Rect,
                0.0,
            ),
            hide_with(
                vec![pt(24.0, 94.0), pt(204.0, 94.0)],
                18.0,
                8.0,
                style,
                blur::Kind::Brush,
                0.0,
            ),
            hide_with(
                vec![pt(246.0, 35.0), pt(326.0, 115.0)],
                16.0,
                8.0,
                style,
                blur::Kind::Ellipse,
                14.0,
            ),
        ],
    }
}

/// W114-5: goldens for the new secure Soft style on light, dark and Arabic
/// scenes; with `OPENCIRCLE_W114_SHEETS=1`, also writes the owner before/after
/// comparison sheets into `docs/internal/reports-enh/w114-soft-*.png`.
#[test]
fn w114_5_soft_style_matches_its_golden_images() {
    use crate::shared::kit::testing::golden;
    let mut failed = Vec::new();
    let light = light_card();
    let dark = dark_card();
    let arabic = arabic_card();
    for (name, img, kind) in [
        ("w114-soft-light@1x", &light, 0u8),
        ("w114-soft-dark@1x", &dark, 0u8),
        ("w114-soft-arabic@1x", &arabic, 1u8),
    ] {
        let out = rendered(img.clone(), 42, card_hides(blur::Style::Soft, kind));
        if golden::enabled() {
            failed.extend(golden::compare("screens", name, out.width, out.height, &out.rgba).err());
        }
    }
    if std::env::var_os("OPENCIRCLE_W114_SHEETS").is_some() {
        write_w114_sheets(&light, &dark, &arabic);
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

fn blit(dst: &mut [u8], dw: u32, src: &[u8], sw: u32, sh: u32, ox: u32, oy: u32) {
    for y in 0..sh {
        let s = (y * sw * 4) as usize;
        let d = (((oy + y) * dw + ox) * 4) as usize;
        dst[d..d + (sw * 4) as usize].copy_from_slice(&src[s..s + (sw * 4) as usize]);
    }
}

fn label_strip(w: u32, h: u32, words: &'static str) -> Vec<u8> {
    use iced::widget::{container, text};
    use iced::{Center, Color, Fill};
    crate::shared::kit::testing::shot::render::<()>(
        container(
            text(words)
                .size(13)
                .font(crate::shared::kit::text::ui_font())
                .color(Color::from_rgb8(236, 240, 248)),
        )
        .width(Fill)
        .height(Fill)
        .align_y(Center)
        .padding([0, 10])
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(28, 33, 46).into()),
            ..Default::default()
        })
        .into(),
        w,
        h,
    )
}

fn write_w114_sheets(light: &Image, dark: &Image, arabic: &Image) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/internal/reports-enh");
    std::fs::create_dir_all(&dir).unwrap();
    let grad = gradient_avatar_card();

    // Sheet 1: Before / After (Original | Frost old default | Blur look-only | Soft new secure default)
    let (cw, ch, pad, hdr, rhdr) = (360u32, 150u32, 12u32, 30u32, 24u32);
    let cols = [
        ("Original", None),
        ("Frost (old default)", Some(blur::Style::Frost)),
        ("Blur (look only, old Soft)", Some(blur::Style::Blur)),
        ("Soft (new secure default)", Some(blur::Style::Soft)),
    ];
    let rows = [
        ("Light card (Brush / Rect / Ellipse)", light, 0u8),
        ("Dark card (Brush / Rect / Ellipse)", dark, 0u8),
        ("Arabic RTL (Light + Dark)", arabic, 1u8),
        ("Pastel gradient + red circle border (Ring)", &grad, 2u8),
    ];
    let sw = pad + cols.len() as u32 * (cw + pad);
    let sh = pad + hdr + rows.len() as u32 * (rhdr + ch + pad);
    let mut buf = vec![0u8; (sw * sh * 4) as usize];
    for px in buf.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[18, 21, 30, 255]);
    }
    for (ci, (title, _)) in cols.iter().enumerate() {
        let strip = label_strip(cw, hdr, title);
        blit(
            &mut buf,
            sw,
            &strip,
            cw,
            hdr,
            pad + ci as u32 * (cw + pad),
            pad,
        );
    }
    for (ri, (rtitle, img, kind)) in rows.iter().enumerate() {
        let ry = pad + hdr + ri as u32 * (rhdr + ch + pad);
        let rstrip = label_strip(sw - 2 * pad, rhdr, rtitle);
        blit(&mut buf, sw, &rstrip, sw - 2 * pad, rhdr, pad, ry);
        for (ci, (_, style)) in cols.iter().enumerate() {
            let out = match style {
                None => rendered((*img).clone(), 42, vec![]),
                Some(st) => rendered((*img).clone(), 42, card_hides(*st, *kind)),
            };
            blit(
                &mut buf,
                sw,
                &out.rgba,
                cw,
                ch,
                pad + ci as u32 * (cw + pad),
                ry + rhdr,
            );
        }
    }
    crate::shared::picture::write_png(&dir.join("w114-soft-before-after.png"), sw, sh, &buf)
        .unwrap();

    // Sheet 2: New secure Soft across Light / Medium / Strong strengths on Light, Dark, and Gradient
    let strengths = [
        ("Light (4 px)", 4.0f32),
        ("Medium (8 px, default)", 8.0f32),
        ("Strong (16 px)", 16.0f32),
    ];
    let srows = [
        ("Light card", light, 0u8),
        ("Dark card", dark, 0u8),
        ("Gradient + red border ring", &grad, 2u8),
    ];
    let sw2 = pad + strengths.len() as u32 * (cw + pad);
    let sh2 = pad + hdr + srows.len() as u32 * (rhdr + ch + pad);
    let mut buf2 = vec![0u8; (sw2 * sh2 * 4) as usize];
    for px in buf2.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[18, 21, 30, 255]);
    }
    for (ci, (title, _)) in strengths.iter().enumerate() {
        let strip = label_strip(cw, hdr, title);
        blit(
            &mut buf2,
            sw2,
            &strip,
            cw,
            hdr,
            pad + ci as u32 * (cw + pad),
            pad,
        );
    }
    for (ri, (rtitle, img, kind)) in srows.iter().enumerate() {
        let ry = pad + hdr + ri as u32 * (rhdr + ch + pad);
        let rstrip = label_strip(sw2 - 2 * pad, rhdr, rtitle);
        blit(&mut buf2, sw2, &rstrip, sw2 - 2 * pad, rhdr, pad, ry);
        for (ci, (_, cell)) in strengths.iter().enumerate() {
            let items: Vec<Item> = card_hides(blur::Style::Soft, *kind)
                .into_iter()
                .map(|mut it| {
                    if let Shape::Blur { cell: c, .. } = &mut it.shape {
                        *c = *cell;
                    }
                    it
                })
                .collect();
            let out = rendered((*img).clone(), 42, items);
            blit(
                &mut buf2,
                sw2,
                &out.rgba,
                cw,
                ch,
                pad + ci as u32 * (cw + pad),
                ry + rhdr,
            );
        }
    }
    crate::shared::picture::write_png(&dir.join("w114-soft-strengths.png"), sw2, sh2, &buf2)
        .unwrap();
}

/// W114-6 (HIGH security review): Hide B's exterior ring MUST NOT read pixels
/// covered by Hide A (or a Redact). Changing ink/content under A leaves B's
/// pixels bit-for-bit identical (reproduces the y=40 / y=62 size=24 cell=8
/// two-brush case and an adjacent-rect case).
#[test]
fn w114_6_adjacent_hide_never_reads_content_under_another_hide() {
    let make = |under_a: [u8; 4]| {
        image_of(260, 120, |x, y| {
            if (24..216).contains(&x) && (30..50).contains(&y) {
                under_a
            } else {
                [250, 250, 250, 255]
            }
        })
    };
    let a = hide_with(
        vec![pt(20.0, 40.0), pt(220.0, 40.0)],
        24.0,
        8.0,
        blur::Style::Soft,
        blur::Kind::Brush,
        0.0,
    );
    let b = hide_with(
        vec![pt(20.0, 62.0), pt(220.0, 62.0)],
        24.0,
        8.0,
        blur::Style::Soft,
        blur::Kind::Brush,
        0.0,
    );
    let out1 = rendered(make([20, 20, 30, 255]), 7, vec![a.clone(), b.clone()]);
    let out2 = rendered(make([250, 250, 250, 255]), 7, vec![a, b]);
    assert_eq!(out1.rgba, out2.rgba, "two adjacent brushes: A leaks into B");

    // Adjacent rects (1 px apart): B's top ring sits squarely inside A.
    let ra = hide_with(
        vec![pt(20.0, 26.0), pt(220.0, 54.0)],
        16.0,
        8.0,
        blur::Style::Soft,
        blur::Kind::Rect,
        0.0,
    );
    let rb = hide_with(
        vec![pt(20.0, 55.0), pt(220.0, 85.0)],
        16.0,
        8.0,
        blur::Style::Soft,
        blur::Kind::Rect,
        0.0,
    );
    let r1 = rendered(make([230, 30, 180, 255]), 7, vec![ra.clone(), rb.clone()]);
    let r2 = rendered(make([250, 250, 250, 255]), 7, vec![ra, rb]);
    assert_eq!(r1.rgba, r2.rgba, "two adjacent rects: A leaks into B");

    // Soft Hide adjacent to a Redact: Redact's covered pixels never leak into Soft.
    let redact = item(Shape::Redact {
        a: pt(20.0, 26.0),
        b: pt(220.0, 54.0),
        oval: false,
    });
    let soft = hide_with(
        vec![pt(20.0, 55.0), pt(220.0, 85.0)],
        16.0,
        8.0,
        blur::Style::Soft,
        blur::Kind::Rect,
        0.0,
    );
    let d1 = rendered(
        make([15, 210, 40, 255]),
        7,
        vec![redact.clone(), soft.clone()],
    );
    let d2 = rendered(make([250, 250, 250, 255]), 7, vec![redact, soft]);
    assert_eq!(
        d1.rgba, d2.rgba,
        "redact adjacent to Soft: Redact leaks into Soft"
    );
}

/// W114-7 (MEDIUM 1 review): when the exterior ring is empty (or near-empty),
/// Soft uses a content-free fallback (`SOFT_TARGET`), never reading pixels
/// inside the footprint. A full-frame Rect over solid (250,250,250) and over
/// solid (20,120,200) produces bit-for-bit identical output.
#[test]
fn w114_7_full_frame_fallback_is_content_free() {
    let full_hide = hide_with(
        vec![pt(0.0, 0.0), pt(120.0, 60.0)],
        16.0,
        8.0,
        blur::Style::Soft,
        blur::Kind::Rect,
        0.0,
    );
    let white = rendered(
        image_of(120, 60, |_, _| [250, 250, 250, 255]),
        13,
        vec![full_hide.clone()],
    );
    let blue = rendered(
        image_of(120, 60, |_, _| [20, 120, 200, 255]),
        13,
        vec![full_hide],
    );
    assert_eq!(
        white.rgba, blue.rgba,
        "empty-ring fallback must be content-free"
    );
}

/// W114-8 (re-review MEDIUM security): merging a pasted picture above Soft Hide B
/// while Soft Hide C sits ABOVE that picture must still exclude C from B's ring
/// during bake, so content under C never leaks into the baked pixels or exports.
#[test]
fn w114_8_merge_bake_excludes_hides_above_merged_picture() {
    let run = |under_c: [u8; 4]| {
        let img = image_of(260, 140, |x, y| {
            if (24..216).contains(&x) && (26..52).contains(&y) {
                under_c
            } else {
                [250, 250, 250, 255]
            }
        });
        let mut e = Annotator::new(img, 1.0);
        e.doc.seed = 21;
        // 0: Soft Hide B (below the merged picture; its top ring reaches into C)
        let b = hide_with(
            vec![pt(20.0, 54.0), pt(220.0, 84.0)],
            16.0,
            8.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        // 1: Pasted picture merged above B (placed in the corner so B stays visible)
        let pic = item(Shape::Picture {
            a: pt(230.0, 110.0),
            b: pt(246.0, 126.0),
            pic: document::Pic::new(16, 16, &vec![80u8; 16 * 16 * 4]),
            lock: true,
        });
        // 2: Soft Hide C above the merged picture
        let c = hide_with(
            vec![pt(20.0, 24.0), pt(220.0, 53.0)],
            16.0,
            8.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        e.doc.exec(Edit::Insert(0, b));
        e.doc.exec(Edit::Insert(1, pic));
        e.doc.exec(Edit::Insert(2, c));
        e.doc.select_one(Some(1));
        e.update(Message::Merge);
        // Extract baked pixels in B's region (permanently written into e.doc.image)
        let mut baked_b = Vec::new();
        for y in 54..84usize {
            for x in 20..220usize {
                let i = (y * 260 + x) * 4;
                baked_b.extend_from_slice(&e.doc.image.rgba[i..i + 4]);
            }
        }
        (baked_b, render(&e.doc).rgba)
    };
    let (baked1, exp1) = run([20, 20, 30, 255]);
    let (baked2, exp2) = run([250, 250, 250, 255]);
    assert_eq!(
        baked1, baked2,
        "baked picture pixels under B must not read under C"
    );
    assert_eq!(
        exp1, exp2,
        "exported picture must not depend on content under C"
    );
}

/// W114-9 (re-review LOW 1): while Hide A is dragged (hidden from the base layer),
/// `refresh()` must still exclude Hide A at its current geometry so an adjacent
/// Soft Hide B in the base does not read content under A or jump on release.
#[test]
fn w114_9_dragging_hide_stays_excluded_from_base_neighbours() {
    let run = |under_a: [u8; 4]| {
        let img = image_of(260, 140, |x, y| {
            if (24..216).contains(&x) && (26..52).contains(&y) {
                under_a
            } else {
                [250, 250, 250, 255]
            }
        });
        let mut e = Annotator::new(img, 1.0);
        e.doc.seed = 23;
        let a = hide_with(
            vec![pt(20.0, 24.0), pt(220.0, 53.0)],
            16.0,
            8.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        let b = hide_with(
            vec![pt(20.0, 54.0), pt(220.0, 84.0)],
            16.0,
            8.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        e.doc.exec(Edit::Insert(0, a));
        e.doc.exec(Edit::Insert(1, b));
        e.refresh();
        // Pick up Hide A and drag slightly so it becomes hidden from the base layer.
        e.update(Message::Tool(Tool::Select));
        e.update(Message::Press(pt(100.0, 38.0)));
        e.update(Message::Move(pt(102.0, 38.0)));
        assert!(
            e.pending.as_ref().is_some_and(|p| p.hidden),
            "dragged Hide A is hidden from base"
        );
        let base = e.base_pixels();
        let mut b_px = Vec::new();
        for y in 54..84usize {
            for x in 20..220usize {
                let i = (y * 260 + x) * 4;
                b_px.extend_from_slice(&base[i..i + 4]);
            }
        }
        b_px
    };
    assert_eq!(
        run([20, 20, 30, 255]),
        run([250, 250, 250, 255]),
        "base neighbour B must not read under dragged Hide A"
    );
}

/// W114-10 (re-review LOW 2): other Hide footprints in `blocked` are dilated by
/// their feather so a glyph sliver just outside A's hard edge (inside A's
/// visible outward feather) never changes an adjacent Hide B.
#[test]
fn w114_10_other_hide_feather_is_excluded_from_neighbours_ring() {
    let run = |sliver: [u8; 4]| {
        let img = image_of(260, 140, |x, y| {
            // Sliver at y=51..62 is strictly outside A's hard edge (y=20..50)
            // but inside A's 12 px outward feather (feather(16.0) = 12.0 -> y=50..62).
            if (24..216).contains(&x) && (51..62).contains(&y) {
                sliver
            } else {
                [250, 250, 250, 255]
            }
        });
        let a = hide_with(
            vec![pt(20.0, 20.0), pt(220.0, 50.0)],
            16.0,
            16.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        // B sits at y=63..95: its top ring covers y=51..63 (the sliver in A's feather).
        let b = hide_with(
            vec![pt(20.0, 63.0), pt(220.0, 95.0)],
            16.0,
            8.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        let out = rendered(img, 29, vec![a, b]);
        let mut b_px = Vec::new();
        for y in 63..95u32 {
            for x in 20..220u32 {
                b_px.extend_from_slice(&px(&out, x, y));
            }
        }
        b_px
    };
    assert_eq!(
        run([20, 20, 30, 255]),
        run([250, 250, 250, 255]),
        "glyph sliver inside A's feather must not influence B"
    );
}

/// W114-12 (re-review LOW 3): large-area Soft Hide (`bw * bh > SOFT_EXACT_AREA`,
/// exercising the `stride > 1` branch) excludes an adjacent Hide cleanly.
#[test]
fn w114_12_large_area_stride_excludes_adjacent_hide() {
    let run = |under_a: [u8; 4]| {
        let img = image_of(800, 620, |x, y| {
            if (24..776).contains(&x) && (24..180).contains(&y) {
                under_a
            } else {
                [250, 250, 250, 255]
            }
        });
        let a = hide_with(
            vec![pt(20.0, 20.0), pt(780.0, 184.0)],
            16.0,
            8.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        // B's padded box is 798x434 = 346,332 > SOFT_EXACT_AREA (262,144), forcing stride = 2.
        let b = hide_with(
            vec![pt(20.0, 185.0), pt(780.0, 580.0)],
            16.0,
            8.0,
            blur::Style::Soft,
            blur::Kind::Rect,
            0.0,
        );
        rendered(img, 37, vec![a, b]).rgba
    };
    assert_eq!(
        run([20, 20, 30, 255]),
        run([250, 250, 250, 255]),
        "large-area stride > 1 Soft Hide must exclude adjacent Hide A"
    );
}
