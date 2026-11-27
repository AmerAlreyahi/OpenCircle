//! R-19: Studio drawing speed on 5K/8K pictures and pixel stability outside the drawn shape.
use super::*;
use crate::shared::kit::motion;
use iced::Theme;
use iced::advanced::graphics::{Viewport, damage};
use iced::advanced::{mouse, renderer};
use iced_runtime::user_interface::{Cache, UserInterface};

/// Detailed picture with sharp 1-px stripes, text-like bars and gradient so any scaling-filter
/// change, rounding shift, or stale copy shows up immediately in pixel diffs.
fn detailed_crop(width: u32, height: u32) -> Crop {
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        let row = y as usize * width as usize * 4;
        for x in 0..width {
            let off = row + x as usize * 4;
            let bar = (y / 7) % 3 == 0 && (x / 5) % 4 != 0;
            let grid = (x % 2 == 0) ^ (y % 2 == 0);
            let px = if bar {
                [36, 42, 58, 255]
            } else if (x / 64 + y / 64) % 2 == 0 && grid {
                [180, 195, 225, 255]
            } else {
                [
                    (205 + (x * 40 / width)) as u8,
                    (200 + (y * 40 / height)) as u8,
                    (225 + ((x ^ y) % 24)) as u8,
                    255,
                ]
            };
            rgba[off..off + 4].copy_from_slice(&px);
        }
    }
    Crop {
        width,
        height,
        rgba,
    }
}

pub(super) struct StudioFrame {
    pub(super) update_ms: f64,
    pub(super) draw_ms: f64,
    pub(super) raster_ms: f64,
    pub(super) damage_ms: f64,
    pub(super) damage_area: f64,
}

impl StudioFrame {
    pub(super) fn total_ms(&self) -> f64 {
        self.update_ms + self.draw_ms + self.raster_ms
    }
}

/// Full Studio window compositor harness: persistent iced widget Cache, Renderer, Pixmap,
/// damage tracking and background mip settling.
pub(super) struct StudioLab {
    pub(super) s: Studio,
    pub(super) size: Size,
    pub(super) scale: f32,
    pub(super) renderer: iced::Renderer,
    pub(super) cache: Cache,
    pub(super) pixmap: tiny_skia::Pixmap,
    pub(super) mask: tiny_skia::Mask,
    pub(super) last_layers: Option<Vec<iced_tiny_skia::Layer>>,
}

impl StudioLab {
    pub(super) fn open(width: u32, height: u32, window: Size, scale: f32) -> Self {
        motion::set_reduced(true);
        crate::shared::kit::text::load();
        let mut s = Studio::new(StudioConfig::default());
        s.set_window_size(window);
        if (scale - 1.0).abs() > 1e-3 {
            let _ = s.update(Msg::Scale(scale));
        }
        s.open(Picture::Crop(detailed_crop(width, height)));
        s.settle_open();
        let _ = s.take_scroll();
        let pw = (window.width * scale).round() as u32;
        let ph = (window.height * scale).round() as u32;
        let mut lab = StudioLab {
            s,
            size: window,
            scale,
            renderer: crate::shared::kit::testing::shot::renderer(),
            cache: Cache::default(),
            pixmap: tiny_skia::Pixmap::new(pw, ph).unwrap(),
            mask: tiny_skia::Mask::new(pw, ph).unwrap(),
            last_layers: None,
        };
        lab.settle_mip();
        lab
    }

    pub(super) fn settle_mip(&mut self) {
        for _ in 0..2000 {
            if !self.s.moving() {
                break;
            }
            let _ = self.s.update(Msg::Frame(std::time::Instant::now()));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    pub(super) fn apply_scroll(&mut self, to: iced::widget::scrollable::AbsoluteOffset) {
        use iced::advanced::widget::{Id, operation::scrollable::scroll_to};
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build(
            view(&self.s, &[], false),
            self.size,
            cache,
            &mut self.renderer,
        );
        let mut op = scroll_to(Id::new(super::CANVAS), to.into());
        ui.operate(&self.renderer, &mut op);
        self.cache = ui.into_cache();
    }

    pub(super) fn reset_scroll(&mut self) {
        self.apply_scroll(iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 });
        let _ = self.s.update(Msg::Scrolled(Vector::ZERO));
    }

    pub(super) fn send(&mut self, msg: Msg) -> f64 {
        let t0 = std::time::Instant::now();
        let out = self.s.update(msg);
        let queued = self.s.take_scroll();
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        if let Outcome::ScrollTo(to) = out {
            self.apply_scroll(to);
        }
        if let Some(to) = queued {
            self.apply_scroll(to);
        }
        ms
    }

    pub(super) fn render(&mut self, update_ms: f64) -> StudioFrame {
        let t0 = std::time::Instant::now();
        let view = view(&self.s, &[], false);
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build(view, self.size, cache, &mut self.renderer);
        let style = renderer::Style {
            text_color: crate::shared::kit::themes::c().text,
        };
        ui.draw(
            &mut self.renderer,
            &Theme::Dark,
            &style,
            mouse::Cursor::Unavailable,
        );
        self.cache = ui.into_cache();
        let t1 = std::time::Instant::now();
        let layers = self.renderer.layers().to_vec();
        let all = Rectangle::with_size(self.size);
        let changed = match &self.last_layers {
            Some(prev) => iced_tiny_skia::Layer::frame_damage(prev, &layers),
            None => vec![all],
        };
        let changed = damage::group(changed, all);
        let t_dmg = std::time::Instant::now();
        let pw = self.pixmap.width();
        let ph = self.pixmap.height();
        let viewport = Viewport::with_physical_size(Size::new(pw, ph), self.scale);
        let bg = crate::shared::kit::themes::c().bg;
        if !changed.is_empty() {
            self.renderer.draw(
                &mut self.pixmap.as_mut(),
                &mut self.mask,
                &viewport,
                &changed,
                bg,
            );
        }
        let t2 = std::time::Instant::now();
        self.last_layers = Some(layers);
        let damage_area = changed
            .iter()
            .map(|r| f64::from(r.width * r.height))
            .sum::<f64>();
        StudioFrame {
            update_ms,
            draw_ms: (t1 - t0).as_secs_f64() * 1e3,
            raster_ms: (t2 - t1).as_secs_f64() * 1e3,
            damage_ms: (t_dmg - t1).as_secs_f64() * 1e3,
            damage_area,
        }
    }

    pub(super) fn pixels(&self) -> Vec<u8> {
        self.pixmap.data().to_vec()
    }

    pub(super) fn picture_phys_rect(&self) -> [u32; 4] {
        let e = self.s.editor.as_ref().unwrap();
        let room = self.s.stage();
        let shown = e.size();
        let corner = stage_corner(room, shown);
        let left = 1.0 + RAIL + 1.0;
        let top = 1.0 + HEADER + OPTIONS + 1.0;
        let sx = (left + corner.x.max(0.0)) * self.scale;
        let sy = (top + corner.y.max(0.0)) * self.scale;
        let sw = shown.width.min(room.width) * self.scale;
        let sh = shown.height.min(room.height) * self.scale;
        [
            sx.ceil() as u32 + 2,
            sy.ceil() as u32 + 2,
            ((sx + sw).floor() as u32)
                .saturating_sub(2)
                .min(self.pixmap.width()),
            ((sy + sh).floor() as u32)
                .saturating_sub(2)
                .min(self.pixmap.height()),
        ]
    }

    pub(super) fn image_rect_to_phys(&self, r: Rectangle, pad_px: f32) -> [i32; 4] {
        let e = self.s.editor.as_ref().unwrap();
        let room = self.s.stage();
        let shown = e.size();
        let corner = stage_corner(room, shown);
        let left = 1.0 + RAIL + 1.0 + corner.x - self.s.offset.x;
        let top = 1.0 + HEADER + OPTIONS + 1.0 + corner.y - self.s.offset.y;
        let view = e.doc.view();
        let x0 = (left + (r.x - view.x) * e.zoom) * self.scale - pad_px;
        let y0 = (top + (r.y - view.y) * e.zoom) * self.scale - pad_px;
        let x1 = (left + (r.x + r.width - view.x) * e.zoom) * self.scale + pad_px;
        let y1 = (top + (r.y + r.height - view.y) * e.zoom) * self.scale + pad_px;
        [
            x0.floor() as i32,
            y0.floor() as i32,
            x1.ceil() as i32,
            y1.ceil() as i32,
        ]
    }
}

pub(super) fn diff_outside(
    before: &[u8],
    after: &[u8],
    stride_px: u32,
    pic_rect: [u32; 4],
    exclude: &[[i32; 4]],
) -> (usize, u8, usize) {
    let [px0, py0, px1, py1] = pic_rect;
    let (mut checked, mut diff, mut max_d) = (0usize, 0usize, 0u8);
    for y in py0..py1 {
        for x in px0..px1 {
            let (xi, yi) = (x as i32, y as i32);
            if exclude
                .iter()
                .any(|e| xi >= e[0] && xi < e[2] && yi >= e[1] && yi < e[3])
            {
                continue;
            }
            checked += 1;
            let o = (y * stride_px + x) as usize * 4;
            let mut d = 0u8;
            for c in 0..4 {
                d = d.max(before[o + c].abs_diff(after[o + c]));
            }
            if d > 0 {
                diff += 1;
                max_d = max_d.max(d);
            }
        }
    }
    (diff, max_d, checked)
}

fn drag_points(tool: Tool, w: f32, h: f32, zoom_actual: bool, n: usize) -> Vec<Point> {
    let (x0, y0, dx, dy) = if zoom_actual {
        (100.0, 80.0, 750.0, 420.0)
    } else {
        (w * 0.12, h * 0.15, w * 0.65, h * 0.65)
    };
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            match tool {
                Tool::Pen | Tool::Highlighter => {
                    Point::new(x0 + dx * t, y0 + dy * (0.5 + 0.35 * (t * 10.0).sin()))
                }
                _ => Point::new(x0 + dx * t, y0 + dy * t),
            }
        })
        .collect()
}

pub(super) fn summarize(vals: &[f64]) -> String {
    let mut s = vals.to_vec();
    s.sort_by(f64::total_cmp);
    let mean = s.iter().sum::<f64>() / s.len().max(1) as f64;
    let p95 = s[(s.len() * 95 / 100).min(s.len().saturating_sub(1))];
    let max = *s.last().unwrap_or(&0.0);
    format!("{mean:5.1}/{p95:5.1}/{max:5.1}")
}

#[test]
fn r19_zoom_90_and_150_keep_outside_pixels_identical_and_pan_clean() {
    // 1) 90 % zoom: full downsampled display copy active and crisp, 0 outside pixels changed.
    {
        let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
        let _ = lab.s.zoom_to(0.90, None);
        let _ = lab.s.take_scroll();
        lab.reset_scroll();
        lab.settle_mip();
        let _ = lab.render(0.0);
        let e = lab.s.editor.as_ref().unwrap();
        assert!(
            e.display.is_some() && e.crisp,
            "zoom 90%: display copy active and crisp"
        );
        let pic_rect = lab.picture_phys_rect();
        let stride = lab.pixmap.width();
        let before = lab.pixels();

        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
        let _ = lab.send(Msg::Editor(Message::Press(Point::new(180.0, 140.0))));
        let _ = lab.send(Msg::Editor(Message::Move(Point::new(420.0, 320.0))));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let excl_r = lab.image_rect_to_phys(
            Rectangle::new(Point::new(180.0, 140.0), Size::new(240.0, 180.0)),
            48.0,
        );
        let (dr, mdr, cr) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl_r]);
        assert_eq!(
            dr, 0,
            "zoom 90% Rect commit: {dr}/{cr} outside px changed (max={mdr})"
        );

        let before_blur = lab.pixels();
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
        let _ = lab.send(Msg::Editor(Message::Press(Point::new(560.0, 150.0))));
        let _ = lab.send(Msg::Editor(Message::Move(Point::new(720.0, 290.0))));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let blur_bounds = lab
            .s
            .editor
            .as_ref()
            .unwrap()
            .doc
            .items
            .last()
            .unwrap()
            .bounds();
        let excl_b = lab.image_rect_to_phys(blur_bounds, 8.0);
        let (db, mdb, cb) = diff_outside(&before_blur, &lab.pixels(), stride, pic_rect, &[excl_b]);
        assert_eq!(
            db, 0,
            "zoom 90% Blur commit: {db}/{cb} outside px changed (max={mdb})"
        );
    }

    // 2) 150 % zoom: keeps origin/main smooth Linear path on base (no viewport tile seam);
    //    panning after an edit must match a fresh render bit-for-bit.
    {
        let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
        let _ = lab.s.zoom_to(1.50, None);
        let _ = lab.s.take_scroll();
        lab.reset_scroll();
        lab.settle_mip();
        let _ = lab.render(0.0);
        let e = lab.s.editor.as_ref().unwrap();
        assert!(
            e.display.is_none() && !e.crisp,
            "zoom 150%: smooth Linear path on base"
        );
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
        let _ = lab.send(Msg::Editor(Message::Press(Point::new(1800.0, 1100.0))));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);

        // Pan across the canvas to where the Step sits.
        let pan = iced::widget::scrollable::AbsoluteOffset {
            x: 2100.0,
            y: 1200.0,
        };
        lab.apply_scroll(pan);
        let _ = lab.send(Msg::Scrolled(Vector::new(pan.x, pan.y)));
        let _ = lab.render(0.0);
        let panned = lab.pixels();
        let pic_rect = lab.picture_phys_rect();
        let stride = lab.pixmap.width();

        lab.s.editor.as_mut().unwrap().refresh();
        let _ = lab.render(0.0);
        let fresh = lab.pixels();
        let (diff, md, checked) = diff_outside(&fresh, &panned, stride, pic_rect, &[]);
        assert_eq!(
            diff, 0,
            "zoom 150% pan after edit: {diff}/{checked} px differed from fresh render (max={md})"
        );
    }
}

#[test]
#[ignore = "timing: run by hand, in release"]
fn r19_lab_measure() {
    let win = SIZE;
    let win_area = f64::from(win.width * win.height);
    for (name, w, h) in [("5K", 5120u32, 2880u32), ("8K", 7680u32, 4320u32)] {
        for actual in [false, true] {
            let zoom_label = if actual { "100%" } else { "Fit" };
            println!(
                "\n=== {name} ({w}x{h}) @ {zoom_label} (window {}x{}) ===",
                win.width, win.height
            );
            for (tname, tool) in [
                ("rect", Tool::Rect),
                ("arrow", Tool::Arrow),
                ("pen", Tool::Pen),
                ("ellipse", Tool::Ellipse),
                ("highlighter", Tool::Highlighter),
            ] {
                let mut lab = StudioLab::open(w, h, win, 1.0);
                if actual {
                    let _ = lab.send(Msg::Actual);
                    lab.reset_scroll();
                    lab.settle_mip();
                }
                let _ = lab.send(Msg::Editor(Message::Tool(tool)));
                let _ = lab.render(0.0);
                let before_px = lab.pixels();
                let pic_rect = lab.picture_phys_rect();
                let stride = lab.pixmap.width();

                let pts = drag_points(tool, w as f32, h as f32, actual, 30);
                let mut frames = Vec::new();
                let mut max_drag_diff = 0usize;
                let mut max_drag_d = 0u8;

                let u0 = lab.send(Msg::Editor(Message::Press(pts[0])));
                frames.push(lab.render(u0));
                for &p in &pts[1..] {
                    let u = lab.send(Msg::Editor(Message::Move(p)));
                    let f = lab.render(u);
                    let span_r = annotate::span(&pts);
                    let excl = lab.image_rect_to_phys(span_r, 80.0);
                    let (diff, md, _) =
                        diff_outside(&before_px, &lab.pixels(), stride, pic_rect, &[excl]);
                    if diff > max_drag_diff {
                        max_drag_diff = diff;
                        max_drag_d = md;
                    }
                    frames.push(f);
                }

                let commit_u = lab.send(Msg::Editor(Message::Release));
                let commit_f = lab.render(commit_u);
                let disp_dropped = lab.s.editor.as_ref().unwrap().display.is_none() && !actual;
                let span_r = annotate::span(&pts);
                let excl = lab.image_rect_to_phys(span_r, 80.0);
                let (commit_diff, commit_md, checked) =
                    diff_outside(&before_px, &lab.pixels(), stride, pic_rect, &[excl]);

                let mip_t0 = std::time::Instant::now();
                lab.settle_mip();
                let mip_wait_ms = mip_t0.elapsed().as_secs_f64() * 1e3;
                let _ = lab.render(0.0);
                let (post_mip_diff, post_mip_md, _) =
                    diff_outside(&before_px, &lab.pixels(), stride, pic_rect, &[excl]);

                let totals: Vec<f64> = frames.iter().map(StudioFrame::total_ms).collect();
                let draws: Vec<f64> = frames.iter().map(|f| f.update_ms + f.draw_ms).collect();
                let rasters: Vec<f64> = frames.iter().map(|f| f.raster_ms).collect();
                let dmgs: Vec<f64> = frames.iter().map(|f| f.damage_ms).collect();
                let dmg_pct = frames.iter().map(|f| f.damage_area).sum::<f64>()
                    / frames.len() as f64
                    / win_area
                    * 100.0;

                println!(
                    "{tname:<12} drag total [{}]  upd+draw [{}]  raster [{}] (dmg_ms [{}])  dmg {:4.1}% | commit {:5.1}ms (upd {:5.1} + draw {:4.1} + rast {:5.1}) disp_gone={} mip_wait={:5.1}ms | diff_out: drag={}(max{}) commit={}(max{}) post_mip={}(max{}) / {}",
                    summarize(&totals),
                    summarize(&draws),
                    summarize(&rasters),
                    summarize(&dmgs),
                    dmg_pct,
                    commit_f.total_ms(),
                    commit_f.update_ms,
                    commit_f.draw_ms,
                    commit_f.raster_ms,
                    disp_dropped,
                    mip_wait_ms,
                    max_drag_diff,
                    max_drag_d,
                    commit_diff,
                    commit_md,
                    post_mip_diff,
                    post_mip_md,
                    checked,
                );
            }

            let mut lab = StudioLab::open(w, h, win, 1.0);
            if actual {
                let _ = lab.send(Msg::Actual);
                lab.reset_scroll();
                lab.settle_mip();
            }
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
            let (ra, rb) = if actual {
                (Point::new(120.0, 100.0), Point::new(420.0, 320.0))
            } else {
                (
                    Point::new(w as f32 * 0.15, h as f32 * 0.15),
                    Point::new(w as f32 * 0.45, h as f32 * 0.45),
                )
            };
            let _ = lab.send(Msg::Editor(Message::Press(ra)));
            let _ = lab.send(Msg::Editor(Message::Move(rb)));
            let _ = lab.send(Msg::Editor(Message::Release));
            lab.settle_mip();
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Select)));
            let _ = lab.render(0.0);
            let before_px = lab.pixels();
            let pic_rect = lab.picture_phys_rect();
            let stride = lab.pixmap.width();

            let hit = Point::new(ra.x, (ra.y + rb.y) * 0.5);
            let u0 = lab.send(Msg::Editor(Message::Press(hit)));
            let _ = lab.render(u0);
            let shift_x = if actual { 300.0 } else { w as f32 * 0.30 };
            let shift_y = if actual { 150.0 } else { h as f32 * 0.25 };
            let mut drag_frames = Vec::new();
            let mut max_diff = 0usize;
            let mut max_d = 0u8;
            let excl_all = lab.image_rect_to_phys(
                Rectangle::new(ra, Size::new(rb.x - ra.x + shift_x, rb.y - ra.y + shift_y)),
                80.0,
            );
            for i in 1..=20 {
                let t = i as f32 / 20.0;
                let u = lab.send(Msg::Editor(Message::Move(Point::new(
                    hit.x + shift_x * t,
                    hit.y + shift_y * t,
                ))));
                let f = lab.render(u);
                let (d, md, _) =
                    diff_outside(&before_px, &lab.pixels(), stride, pic_rect, &[excl_all]);
                if d > max_diff {
                    max_diff = d;
                    max_d = md;
                }
                drag_frames.push(f);
            }
            let cu = lab.send(Msg::Editor(Message::Release));
            let cf = lab.render(cu);
            let (cd, cmd, checked) =
                diff_outside(&before_px, &lab.pixels(), stride, pic_rect, &[excl_all]);
            let totals: Vec<f64> = drag_frames.iter().map(StudioFrame::total_ms).collect();
            println!(
                "select-drag  drag total [{}] (f1={:5.1}ms) | commit {:5.1}ms | diff_out: drag={}(max{}) commit={}(max{}) / {}",
                summarize(&totals),
                drag_frames[0].total_ms(),
                cf.total_ms(),
                max_diff,
                max_d,
                cd,
                cmd,
                checked,
            );
        }
    }
}

#[test]
fn r19_pixels_outside_shape_stay_identical_during_drag_and_commit() {
    for (label, scale, actual) in [
        ("fit@1x", 1.0f32, false),
        ("fit@1.5x", 1.5f32, false),
        ("100%@1x", 1.0f32, true),
    ] {
        for tool in [Tool::Rect, Tool::Arrow, Tool::Pen, Tool::Ellipse] {
            let mut lab = StudioLab::open(1600, 900, SIZE, scale);
            if actual {
                let _ = lab.send(Msg::Actual);
                lab.reset_scroll();
                lab.settle_mip();
            }
            let _ = lab.send(Msg::Editor(Message::Tool(tool)));
            let _ = lab.render(0.0);
            let before = lab.pixels();
            let pic_rect = lab.picture_phys_rect();
            let stride = lab.pixmap.width();

            let pts = drag_points(tool, 1600.0, 900.0, actual, 12);
            let excl = lab.image_rect_to_phys(annotate::span(&pts), 48.0 * scale);

            let u0 = lab.send(Msg::Editor(Message::Press(pts[0])));
            let _ = lab.render(u0);
            for (step, &p) in pts[1..].iter().enumerate() {
                let u = lab.send(Msg::Editor(Message::Move(p)));
                let _ = lab.render(u);
                let (diff, md, checked) =
                    diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
                assert_eq!(
                    diff, 0,
                    "{label} {tool:?} drag step {step}: {diff}/{checked} pixels outside shape changed (max_diff={md})"
                );
            }

            let cu = lab.send(Msg::Editor(Message::Release));
            let _ = lab.render(cu);
            let (diff, md, checked) =
                diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
            assert_eq!(
                diff, 0,
                "{label} {tool:?} right after commit: {diff}/{checked} pixels outside shape changed (max_diff={md})"
            );

            lab.settle_mip();
            let _ = lab.render(0.0);
            let (diff, md, checked) =
                diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
            assert_eq!(
                diff, 0,
                "{label} {tool:?} after mip settle: {diff}/{checked} pixels outside shape changed (max_diff={md})"
            );
        }
    }
}

#[test]
fn r19_dragging_existing_shape_leaves_picture_pixels_identical() {
    let mut lab = StudioLab::open(1600, 900, SIZE, 1.0);
    let _ = lab.render(0.0);
    let clean_px = lab.pixels();
    let pic_rect = lab.picture_phys_rect();
    let stride = lab.pixmap.width();

    let (ra, rb) = (Point::new(200.0, 140.0), Point::new(700.0, 480.0));
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = lab.send(Msg::Editor(Message::Press(ra)));
    let _ = lab.send(Msg::Editor(Message::Move(rb)));
    let _ = lab.send(Msg::Editor(Message::Release));
    lab.settle_mip();

    let interior = Rectangle::new(Point::new(300.0, 220.0), Size::new(300.0, 180.0));
    let int_phys = lab.image_rect_to_phys(interior, 0.0);
    let int_rect = [
        int_phys[0].max(0) as u32,
        int_phys[1].max(0) as u32,
        int_phys[2].max(0) as u32,
        int_phys[3].max(0) as u32,
    ];

    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Select)));
    let _ = lab.render(0.0);
    let hit = Point::new(ra.x, (ra.y + rb.y) * 0.5);
    let u0 = lab.send(Msg::Editor(Message::Press(hit)));
    let _ = lab.render(u0);

    let shift = Vector::new(600.0, 240.0);
    let u1 = lab.send(Msg::Editor(Message::Move(hit + shift)));
    let _ = lab.render(u1);

    let (diff_int, md_int, checked_int) =
        diff_outside(&clean_px, &lab.pixels(), stride, int_rect, &[]);
    assert_eq!(
        diff_int, 0,
        "during select-drag: {diff_int}/{checked_int} pixels in old rect interior changed vs untouched picture (max_diff={md_int})"
    );

    let excl_old = lab.image_rect_to_phys(
        Rectangle::new(ra, Size::new(rb.x - ra.x, rb.y - ra.y)),
        40.0,
    );
    let excl_new = lab.image_rect_to_phys(
        Rectangle::new(ra + shift, Size::new(rb.x - ra.x, rb.y - ra.y)),
        40.0,
    );
    let (diff_out, md_out, checked_out) = diff_outside(
        &clean_px,
        &lab.pixels(),
        stride,
        pic_rect,
        &[excl_old, excl_new],
    );
    assert_eq!(
        diff_out, 0,
        "during select-drag: {diff_out}/{checked_out} pixels outside old+new shape changed (max_diff={md_out})"
    );

    let cu = lab.send(Msg::Editor(Message::Release));
    let _ = lab.render(cu);
    let (diff_c, md_c, checked_c) =
        diff_outside(&clean_px, &lab.pixels(), stride, pic_rect, &[excl_new]);
    assert_eq!(
        diff_c, 0,
        "right after select-drag commit: {diff_c}/{checked_c} pixels outside new shape changed (max_diff={md_c})"
    );
}

#[test]
fn r19_downsample_rect_matches_full_downsample_bit_for_bit() {
    for (bw, bh, dw, dh, sub) in [
        (1600u32, 900u32, 1093u32, 615u32, [120u32, 80, 480, 390]),
        (1600, 900, 1093, 615, [0, 0, 17, 19]),
        (1600, 900, 1093, 615, [1050, 580, 1093, 615]),
        (997, 553, 331, 187, [41, 23, 211, 149]),
    ] {
        let img = detailed_crop(bw, bh);
        let full = annotate::downsample(&img.rgba, bw, bh, dw, dh);
        let mut partial = vec![0u8; dw as usize * dh as usize * 4];
        annotate::downsample_rect(&img.rgba, bw, bh, &mut partial, dw, dh, sub);
        for dy in sub[1]..sub[3] {
            for dx in sub[0]..sub[2] {
                let o = (dy as usize * dw as usize + dx as usize) * 4;
                assert_eq!(
                    &partial[o..o + 4],
                    &full[o..o + 4],
                    "({bw}x{bh} -> {dw}x{dh}) mismatch at ({dx},{dy})"
                );
            }
        }
    }
}

#[test]
fn r19_vector_edits_keep_base_and_display_handles_stable_and_zero_live_paints() {
    let mut lab = StudioLab::open(1600, 900, SIZE, 1.0);
    let _ = lab.render(0.0);
    let base_id0 = lab.s.editor.as_ref().unwrap().base.id();
    let disp_id0 = lab
        .s
        .editor
        .as_ref()
        .unwrap()
        .display
        .as_ref()
        .expect("mip ready")
        .id();

    for (tool, a, b) in [
        (
            Tool::Rect,
            Point::new(150.0, 120.0),
            Point::new(650.0, 460.0),
        ),
        (
            Tool::Arrow,
            Point::new(200.0, 600.0),
            Point::new(900.0, 300.0),
        ),
        (
            Tool::Pen,
            Point::new(300.0, 200.0),
            Point::new(1100.0, 550.0),
        ),
        (
            Tool::Ellipse,
            Point::new(800.0, 200.0),
            Point::new(1250.0, 620.0),
        ),
    ] {
        let _ = lab.send(Msg::Editor(Message::Tool(tool)));
        let _ = lab.send(Msg::Editor(Message::Press(a)));
        let _ = lab.render(0.0);
        let _ = lab.send(Msg::Editor(Message::Move(Point::new(
            (a.x + b.x) * 0.5,
            (a.y + b.y) * 0.5,
        ))));
        let _ = lab.render(0.0);
        let _ = lab.send(Msg::Editor(Message::Move(b)));
        let _ = lab.render(0.0);
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.render(0.0);

        let e = lab.s.editor.as_ref().unwrap();
        assert_eq!(
            e.live_paints(),
            0,
            "{tool:?}: must not allocate raster live layers"
        );
        assert_eq!(
            e.base.id(),
            base_id0,
            "{tool:?}: base handle must stay unchanged"
        );
        assert_eq!(
            e.display.as_ref().map(image::Handle::id),
            Some(disp_id0),
            "{tool:?}: display mip handle must stay unchanged"
        );
    }

    // Undo, Redo, Duplicate, Shove, Delete: still zero base/display churn
    for m in [
        Message::Undo,
        Message::Redo,
        Message::Duplicate,
        Message::Shove((1, 0), true),
        Message::Delete,
    ] {
        let _ = lab.send(Msg::Editor(m));
        let _ = lab.render(0.0);
        let e = lab.s.editor.as_ref().unwrap();
        assert_eq!(
            e.base.id(),
            base_id0,
            "base handle must stay unchanged across vector edits"
        );
        assert_eq!(
            e.display.as_ref().map(image::Handle::id),
            Some(disp_id0),
            "display handle must stay unchanged across vector edits"
        );
    }
}

#[test]
fn r19_mixed_layers_magnifier_and_blur_keep_zorder_and_outside_pixels() {
    // 1. Rect then Magnifier over Rect: display stays alive, outside pixels stay identical
    let mut lab = StudioLab::open(1600, 900, SIZE, 1.0);
    let _ = lab.render(0.0);
    let clean = lab.pixels();
    let pic_rect = lab.picture_phys_rect();
    let stride = lab.pixmap.width();

    let (ra, rb) = (Point::new(200.0, 150.0), Point::new(500.0, 400.0));
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = lab.send(Msg::Editor(Message::Press(ra)));
    let _ = lab.send(Msg::Editor(Message::Move(rb)));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.render(0.0);

    let mc = Point::new(350.0, 275.0);
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Magnifier)));
    let _ = lab.send(Msg::Editor(Message::Press(mc)));
    let _ = lab.send(Msg::Editor(Message::Move(Point::new(mc.x + 120.0, mc.y))));
    let _ = lab.render(0.0);
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.render(0.0);

    assert!(
        lab.s.editor.as_ref().unwrap().display.is_some(),
        "committing Magnifier over Rect keeps crisp display mip"
    );
    let excl = lab.image_rect_to_phys(
        Rectangle::new(Point::new(160.0, 110.0), Size::new(400.0, 340.0)),
        40.0,
    );
    let (diff, md, checked) = diff_outside(&clean, &lab.pixels(), stride, pic_rect, &[excl]);
    assert_eq!(
        diff, 0,
        "Magnifier over Rect: {diff}/{checked} outside pixels changed (max={md})"
    );

    // 2. Blur (Hide) then Rect on top: Rect draws/commits/undoes without changing base/display
    let mut lab = StudioLab::open(1600, 900, SIZE, 1.0);
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
    let _ = lab.send(Msg::Editor(Message::Press(Point::new(200.0, 200.0))));
    let _ = lab.send(Msg::Editor(Message::Move(Point::new(500.0, 200.0))));
    let _ = lab.send(Msg::Editor(Message::Release));
    lab.settle_mip();
    let _ = lab.render(0.0);
    let after_blur = lab.pixels();
    let base_id = lab.s.editor.as_ref().unwrap().base.id();
    let disp_id = lab
        .s
        .editor
        .as_ref()
        .unwrap()
        .display
        .as_ref()
        .expect("mip")
        .id();

    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = lab.send(Msg::Editor(Message::Press(Point::new(700.0, 300.0))));
    let _ = lab.send(Msg::Editor(Message::Move(Point::new(1100.0, 600.0))));
    let _ = lab.render(0.0);
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.render(0.0);
    assert_eq!(lab.s.editor.as_ref().unwrap().base.id(), base_id);
    assert_eq!(
        lab.s
            .editor
            .as_ref()
            .unwrap()
            .display
            .as_ref()
            .map(image::Handle::id),
        Some(disp_id)
    );
    let excl_r = lab.image_rect_to_phys(
        Rectangle::new(Point::new(700.0, 300.0), Size::new(400.0, 300.0)),
        40.0,
    );
    let (diff, md, checked) = diff_outside(&after_blur, &lab.pixels(), stride, pic_rect, &[excl_r]);
    assert_eq!(
        diff, 0,
        "Rect over Blur: {diff}/{checked} outside pixels changed (max={md})"
    );
}

#[test]
fn r19_snip_toolbar_big_screen_drawing_clips_and_keeps_outside_pixels_identical() {
    use crate::features::snip::screens::snip_toolbar::SnipToolbar;
    use crate::features::snip::screens::snip_toolbar::canvas::{self, ScreenMap};
    use crate::shared::kit::testing::shot;

    // 1) SnipToolbar::fitted (own_image = true, margin > 0): base is already painted before drag.
    let frame = detailed_crop(1920, 1080);
    let mut snip = SnipToolbar::fitted(frame.clone(), Size::new(1920.0, 1080.0), 1.0);
    let area = snip.annotator.doc.view();
    let m = canvas::margin(&snip.annotator);
    assert!(m > 0.0, "Snip live area has handle margin");
    assert!(!snip.annotator.bare(), "fitted Snip owns its base");
    let shown = snip.annotator.size();
    let outer = Size::new(shown.width + 2.0 * m, shown.height + 2.0 * m);
    let (ow, oh) = (outer.width.ceil() as u32, outer.height.ceil() as u32);

    // Draw a thick rectangle right up to the crop right/bottom edge to verify margin clipping
    // and zero outside pixel change during drag and commit.
    snip.update(Message::Tool(Tool::Rect));
    let before = shot::render(canvas::canvas(&snip.annotator, 1.0, ScreenMap::ONE), ow, oh);
    let a = Point::new(area.x + area.width * 0.45, area.y + area.height * 0.45);
    let b = Point::new(area.x + area.width - 1.0, area.y + area.height - 1.0);
    snip.update(Message::Press(a));
    snip.update(Message::Move(b));
    assert_eq!(
        snip.annotator.live_paints(),
        0,
        "Snip Rect drag uses vector path"
    );
    let during = shot::render(canvas::canvas(&snip.annotator, 1.0, ScreenMap::ONE), ow, oh);
    snip.update(Message::Release);
    let after = shot::render(canvas::canvas(&snip.annotator, 1.0, ScreenMap::ONE), ow, oh);

    // Untouched top-left region inside the live area must stay bit-identical.
    for (phase, px) in [("drag", &during), ("commit", &after)] {
        let mut diff = 0usize;
        for y in (m as u32 + 16)..(m as u32 + 80) {
            for x in (m as u32 + 16)..(m as u32 + 120) {
                let o = (y * ow + x) as usize * 4;
                if px[o..o + 4] != before[o..o + 4] {
                    diff += 1;
                }
            }
        }
        assert_eq!(
            diff, 0,
            "Snip {phase}: {diff} untouched pixels inside live area changed"
        );
        // Right margin strip away from corner/mid handles must not get stroke bleed.
        let mx = (m + shown.width).ceil() as u32 + 2;
        if mx < ow {
            let mut bleed = 0usize;
            for y in (m as u32 + 60)..(m as u32 + 140) {
                let o = (y * ow + mx) as usize * 4;
                if px[o..o + 4] != before[o..o + 4] {
                    bleed += 1;
                }
            }
            assert_eq!(
                bleed, 0,
                "Snip {phase}: stroke bled {bleed} px into right handle margin"
            );
        }
    }

    // 2) SnipToolbar::live (own_image = false, cropped live area): after the first stroke starts,
    // dragging and committing keep untouched pixels bit-identical and use the vector path.
    let mut live = SnipToolbar::live(
        annotate::Image {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
        },
        [200, 120, 1400, 820],
        1.0,
        Point::ORIGIN,
    );
    let larea = live.annotator.doc.view();
    let lm = canvas::margin(&live.annotator);
    let louter = Size::new(larea.width + 2.0 * lm, larea.height + 2.0 * lm);
    let (lw, lh) = (louter.width.ceil() as u32, louter.height.ceil() as u32);
    live.update(Message::Tool(Tool::Rect));
    let la = Point::new(larea.x + 200.0, larea.y + 150.0);
    let lb = Point::new(larea.x + 700.0, larea.y + 500.0);
    live.update(Message::Press(la));
    live.update(Message::Move(lb));
    assert_eq!(live.annotator.live_paints(), 0);
    let live_drag = shot::render(canvas::canvas(&live.annotator, 1.0, ScreenMap::ONE), lw, lh);
    live.update(Message::Release);
    let live_commit = shot::render(canvas::canvas(&live.annotator, 1.0, ScreenMap::ONE), lw, lh);
    let mut ldiff = 0usize;
    for y in (lm as u32 + 16)..(lm as u32 + 80) {
        for x in (lm as u32 + 16)..(lm as u32 + 120) {
            let o = (y * lw + x) as usize * 4;
            if live_drag[o..o + 4] != live_commit[o..o + 4] {
                ldiff += 1;
            }
        }
    }
    assert_eq!(
        ldiff, 0,
        "Snip live drag->commit: {ldiff} untouched px changed"
    );
}

#[test]
fn r19_step_blur_spotlight_highlighter_commit_undo_redo_outside_pixels_identical() {
    let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
    let pic_rect = lab.picture_phys_rect();
    let stride = lab.pixmap.width();
    let _ = lab.render(0.0);

    // Draw an earlier vector Rect in top-left so we also catch earlier shapes getting baked
    // and changing screen pixels when Step / Blur / Spotlight / Redact commits.
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = lab.send(Msg::Editor(Message::Press(Point::new(220.0, 200.0))));
    let _ = lab.send(Msg::Editor(Message::Move(Point::new(720.0, 560.0))));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.send(Msg::Editor(Message::Deselect));
    let _ = lab.render(0.0);

    // 1) Step: click, commit (Deselect closes note typing), undo, redo (NO settle_mip!)
    {
        let before = lab.pixels();
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
        let at = Point::new(2400.0, 1200.0);
        let _ = lab.send(Msg::Editor(Message::Press(at)));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let after_commit = lab.pixels();
        let excl = lab.image_rect_to_phys(
            Rectangle::new(
                Point::new(at.x - 160.0, at.y - 160.0),
                Size::new(320.0, 320.0),
            ),
            48.0,
        );
        let (d, md, c) = diff_outside(&before, &after_commit, stride, pic_rect, &[excl]);
        assert_eq!(d, 0, "Step commit: {d}/{c} outside px changed (max={md})");

        let _ = lab.send(Msg::Editor(Message::Undo));
        let _ = lab.render(0.0);
        let (du, mdu, cu) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[]);
        assert_eq!(
            du, 0,
            "Step undo: {du}/{cu} px differed from before (max={mdu})"
        );

        let _ = lab.send(Msg::Editor(Message::Redo));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let (dr, mdr, cr) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
        assert_eq!(dr, 0, "Step redo: {dr}/{cr} outside px changed (max={mdr})");
    }

    // 2) Spotlight (before any Blur — pure Layer-2 vector spotlight): commit, undo, redo (NO settle_mip!)
    {
        let before = lab.pixels();
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Spotlight)));
        let a = Point::new(3800.0, 1700.0);
        let b = Point::new(4700.0, 2400.0);
        let _ = lab.send(Msg::Editor(Message::Press(a)));
        let _ = lab.send(Msg::Editor(Message::Move(b)));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let hole_in = lab.image_rect_to_phys(
            Rectangle::new(
                Point::new(a.x + 80.0, a.y + 80.0),
                Size::new(b.x - a.x - 160.0, b.y - a.y - 160.0),
            ),
            0.0,
        );
        let hole_rect = [
            hole_in[0].max(0) as u32,
            hole_in[1].max(0) as u32,
            hole_in[2].max(0) as u32,
            hole_in[3].max(0) as u32,
        ];
        let (d, md, c) = diff_outside(&before, &lab.pixels(), stride, hole_rect, &[]);
        assert_eq!(
            d, 0,
            "Spotlight(vec) commit: {d}/{c} inside-hole px changed (max={md})"
        );

        let _ = lab.send(Msg::Editor(Message::Undo));
        let _ = lab.render(0.0);
        let (du, mdu, cu) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[]);
        assert_eq!(
            du, 0,
            "Spotlight(vec) undo: {du}/{cu} px differed from before (max={mdu})"
        );

        let _ = lab.send(Msg::Editor(Message::Redo));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let (dr, mdr, cr) = diff_outside(&before, &lab.pixels(), stride, hole_rect, &[]);
        assert_eq!(
            dr, 0,
            "Spotlight(vec) redo: {dr}/{cr} inside-hole px changed (max={mdr})"
        );
        // Undo before Blur tests so the Blur/Redact checks run on an undimmed canvas
        let _ = lab.send(Msg::Editor(Message::Undo));
        let _ = lab.render(0.0);
    }

    // 3) Blur: drag, commit, undo, redo (NO settle_mip!)
    {
        let before = lab.pixels();
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
        let a = Point::new(3000.0, 800.0);
        let b = Point::new(3700.0, 1400.0);
        let _ = lab.send(Msg::Editor(Message::Press(a)));
        let _ = lab.send(Msg::Editor(Message::Move(b)));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let after_commit = lab.pixels();
        let excl = lab.image_rect_to_phys(
            Rectangle::new(Point::new(2750.0, 550.0), Size::new(1200.0, 1100.0)),
            64.0,
        );
        let (d, md, c) = diff_outside(&before, &after_commit, stride, pic_rect, &[excl]);
        assert_eq!(d, 0, "Blur commit: {d}/{c} outside px changed (max={md})");

        let _ = lab.send(Msg::Editor(Message::Undo));
        let _ = lab.render(0.0);
        let (du, mdu, cu) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[]);
        assert_eq!(
            du, 0,
            "Blur undo: {du}/{cu} px differed from before (max={mdu})"
        );

        let _ = lab.send(Msg::Editor(Message::Redo));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let (dr, mdr, cr) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
        assert_eq!(dr, 0, "Blur redo: {dr}/{cr} outside px changed (max={mdr})");
    }

    // 4) Redact while Blur exists: commit, undo, redo (NO settle_mip!)
    {
        let before = lab.pixels();
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Redact)));
        let a = Point::new(1200.0, 1700.0);
        let b = Point::new(1900.0, 2200.0);
        let _ = lab.send(Msg::Editor(Message::Press(a)));
        let _ = lab.send(Msg::Editor(Message::Move(b)));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let excl = lab.image_rect_to_phys(Rectangle::new(a, Size::new(b.x - a.x, b.y - a.y)), 48.0);
        let (d, md, c) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
        assert_eq!(
            d, 0,
            "Redact-with-Blur commit: {d}/{c} outside px changed (max={md})"
        );

        let _ = lab.send(Msg::Editor(Message::Undo));
        let _ = lab.render(0.0);
        let (du, mdu, cu) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[]);
        assert_eq!(
            du, 0,
            "Redact-with-Blur undo: {du}/{cu} px differed from before (max={mdu})"
        );

        let _ = lab.send(Msg::Editor(Message::Redo));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let (dr, mdr, cr) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
        assert_eq!(
            dr, 0,
            "Redact-with-Blur redo: {dr}/{cr} outside px changed (max={mdr})"
        );
    }

    // 5) Highlighter: drag, commit, undo, redo (NO settle_mip!)
    {
        let before = lab.pixels();
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Highlighter)));
        let a = Point::new(2200.0, 1800.0);
        let b = Point::new(3200.0, 2000.0);
        let _ = lab.send(Msg::Editor(Message::Press(a)));
        let _ = lab.send(Msg::Editor(Message::Move(b)));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let excl = lab.image_rect_to_phys(
            Rectangle::new(Point::new(2050.0, 1650.0), Size::new(1300.0, 500.0)),
            48.0,
        );
        let (d, md, c) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
        assert_eq!(
            d, 0,
            "Highlighter commit: {d}/{c} outside px changed (max={md})"
        );

        let _ = lab.send(Msg::Editor(Message::Undo));
        let _ = lab.render(0.0);
        let (du, mdu, cu) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[]);
        assert_eq!(
            du, 0,
            "Highlighter undo: {du}/{cu} px differed from before (max={mdu})"
        );

        let _ = lab.send(Msg::Editor(Message::Redo));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let (dr, mdr, cr) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[excl]);
        assert_eq!(
            dr, 0,
            "Highlighter redo: {dr}/{cr} outside px changed (max={mdr})"
        );
    }

    // 6) Spotlight (with Blur present — baked Spotlight + synchronous display rebuild): commit, undo, redo!
    {
        let before = lab.pixels();
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Spotlight)));
        let a = Point::new(3800.0, 1700.0);
        let b = Point::new(4700.0, 2400.0);
        let _ = lab.send(Msg::Editor(Message::Press(a)));
        let _ = lab.send(Msg::Editor(Message::Move(b)));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let hole_in = lab.image_rect_to_phys(
            Rectangle::new(
                Point::new(a.x + 80.0, a.y + 80.0),
                Size::new(b.x - a.x - 160.0, b.y - a.y - 160.0),
            ),
            0.0,
        );
        let hole_rect = [
            hole_in[0].max(0) as u32,
            hole_in[1].max(0) as u32,
            hole_in[2].max(0) as u32,
            hole_in[3].max(0) as u32,
        ];
        let (d, md, c) = diff_outside(&before, &lab.pixels(), stride, hole_rect, &[]);
        assert_eq!(
            d, 0,
            "Spotlight(baked) commit: {d}/{c} inside-hole px changed (max={md})"
        );

        let _ = lab.send(Msg::Editor(Message::Undo));
        let _ = lab.render(0.0);
        let (du, mdu, cu) = diff_outside(&before, &lab.pixels(), stride, pic_rect, &[]);
        assert_eq!(
            du, 0,
            "Spotlight(baked) undo: {du}/{cu} px differed from before (max={mdu})"
        );

        let _ = lab.send(Msg::Editor(Message::Redo));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let _ = lab.render(0.0);
        let (dr, mdr, cr) = diff_outside(&before, &lab.pixels(), stride, hole_rect, &[]);
        assert_eq!(
            dr, 0,
            "Spotlight(baked) redo: {dr}/{cr} inside-hole px changed (max={mdr})"
        );
    }
}

#[test]
fn r19_canvas_grow_commit_keeps_display_crisp_without_filter_flip() {
    let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
    lab.s.editor.as_mut().unwrap().grow_canvas = true;
    let _ = lab.render(0.0);
    assert!(
        lab.s.editor.as_ref().unwrap().display.is_some() && lab.s.editor.as_ref().unwrap().crisp
    );
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = lab.send(Msg::Editor(Message::Press(Point::new(4800.0, 1000.0))));
    let _ = lab.send(Msg::Editor(Message::Move(Point::new(5400.0, 1500.0))));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.render(0.0);
    let e = lab.s.editor.as_ref().unwrap();
    assert!(e.doc.image.width > 5120, "canvas grew");
    assert!(
        e.display.is_some() && e.crisp,
        "canvas grow must not drop display/crisp for a frame (filter flip)"
    );
}

#[test]
fn r19_live_vector_matches_raster_paint_pixel_for_pixel_at_1x() {
    use crate::features::snip::annotate::{self, Annotator, Image};
    use crate::features::snip::screens::snip_toolbar::canvas::{self, ScreenMap};
    use crate::shared::kit::testing::shot;

    let (w, h) = (320u32, 220u32);
    let base_crop = detailed_crop(w, h);
    let make_editor = || {
        let mut e = Annotator::new(
            Image {
                width: w,
                height: h,
                rgba: base_crop.rgba.clone(),
            },
            1.0,
        );
        e.studio = true;
        e.outline = false;
        e.fill = false;
        e
    };

    type DrawCase = (&'static str, fn(&mut Annotator));
    let cases: &[DrawCase] = &[
        ("pen", |e| {
            e.update(Message::Tool(Tool::Pen));
            e.update(Message::Press(Point::new(30.0, 40.0)));
            e.update(Message::Move(Point::new(110.0, 95.0)));
            e.update(Message::Move(Point::new(220.0, 60.0)));
            e.update(Message::Move(Point::new(270.0, 150.0)));
        }),
        ("highlighter", |e| {
            e.update(Message::Tool(Tool::Highlighter));
            e.update(Message::Press(Point::new(40.0, 110.0)));
            e.update(Message::Move(Point::new(160.0, 115.0)));
            e.update(Message::Move(Point::new(260.0, 125.0)));
        }),
        ("rect_plain", |e| {
            e.update(Message::Tool(Tool::Rect));
            e.update(Message::Press(Point::new(45.0, 35.0)));
            e.update(Message::Move(Point::new(235.0, 165.0)));
        }),
        ("rect_outline_fill", |e| {
            e.outline = true;
            e.fill = true;
            e.update(Message::Tool(Tool::Rect));
            e.update(Message::Press(Point::new(50.0, 40.0)));
            e.update(Message::Move(Point::new(240.0, 170.0)));
        }),
        ("ellipse_plain", |e| {
            e.update(Message::Tool(Tool::Ellipse));
            e.update(Message::Press(Point::new(55.0, 35.0)));
            e.update(Message::Move(Point::new(255.0, 175.0)));
        }),
        ("ellipse_outline_fill", |e| {
            e.outline = true;
            e.fill = true;
            e.update(Message::Tool(Tool::Ellipse));
            e.update(Message::Press(Point::new(55.0, 35.0)));
            e.update(Message::Move(Point::new(255.0, 175.0)));
        }),
        ("arrow", |e| {
            e.outline = true;
            e.update(Message::Tool(Tool::Arrow));
            e.update(Message::Press(Point::new(40.0, 170.0)));
            e.update(Message::Move(Point::new(260.0, 50.0)));
        }),
        ("line", |e| {
            e.outline = true;
            e.update(Message::Tool(Tool::Line));
            e.update(Message::Press(Point::new(40.0, 50.0)));
            e.update(Message::Move(Point::new(260.0, 170.0)));
        }),
        ("pointer", |e| {
            e.outline = true;
            e.update(Message::Tool(Tool::Pointer));
            e.update(Message::Press(Point::new(160.0, 110.0)));
            e.update(Message::Move(Point::new(210.0, 140.0)));
        }),
        ("redact_rect", |e| {
            e.update(Message::Tool(Tool::Redact));
            e.update(Message::Press(Point::new(60.0, 50.0)));
            e.update(Message::Move(Point::new(230.0, 160.0)));
        }),
        ("redact_oval", |e| {
            e.update(Message::Tool(Tool::Redact));
            e.update(Message::BlurShape(annotate::blur::Kind::Ellipse));
            e.update(Message::Press(Point::new(60.0, 50.0)));
            e.update(Message::Move(Point::new(230.0, 160.0)));
        }),
    ];

    for (name, draw) in cases {
        let mut e = make_editor();
        draw(&mut e);
        assert!(e.live_vector().is_some(), "{name}: uses live_vector");
        e.update(Message::Release);
        e.update(Message::Deselect);
        assert_eq!(
            e.vector_tail_start(),
            0,
            "{name}: rendered via live_vector (unbaked)"
        );
        let vec_px = shot::render(canvas::canvas(&e, 1.0, ScreenMap::ONE), w, h);
        let raster = annotate::render(&e.doc);
        assert_eq!(raster.rgba.len(), vec_px.len());
        let mut diff = 0usize;
        let mut max_d = 0u8;
        for px in 0..(w as usize * h as usize) {
            let o = px * 4;
            let mut d = 0u8;
            for ch in 0..4 {
                d = d.max(vec_px[o + ch].abs_diff(raster.rgba[o + ch]));
            }
            if d > 0 {
                diff += 1;
                max_d = max_d.max(d);
            }
        }
        assert_eq!(
            diff, 0,
            "{name} live_vector vs annotate::render: {diff} px differed (max={max_d})"
        );
    }
}

#[test]
#[ignore = "lab measurement for R-19 review items 1, 3, 4"]
fn r19_lab_review_measure() {
    let win = Size::new(1280.0, 800.0);
    for (tag, w, h) in [("5K", 5120u32, 2880u32), ("8K", 7680u32, 4320u32)] {
        eprintln!(
            "
=== REVIEW LAB {tag} ({w}x{h}) ==="
        );

        // Part A: Item 3 — 30 mixed annotations: single-item select-drag + 2-item group-drag
        {
            let mut lab = StudioLab::open(w, h, win, 1.0);
            let _ = lab.render(0.0);
            let tools = [
                Tool::Rect,
                Tool::Ellipse,
                Tool::Arrow,
                Tool::Pen,
                Tool::Highlighter,
                Tool::Pointer,
                Tool::Redact,
            ];
            for i in 0..30usize {
                let tool = tools[i % tools.len()];
                let _ = lab.send(Msg::Editor(Message::Tool(tool)));
                let col = (i % 6) as f32;
                let row = (i / 6) as f32;
                let x0 = (w as f32) * (0.06 + col * 0.15);
                let y0 = (h as f32) * (0.06 + row * 0.17);
                let x1 = x0 + (w as f32) * 0.10;
                let y1 = y0 + (h as f32) * 0.11;
                let _ = lab.send(Msg::Editor(Message::Press(Point::new(x0, y0))));
                if matches!(tool, Tool::Pen | Tool::Highlighter) {
                    for s in 1..=8 {
                        let t = s as f32 / 8.0;
                        let _ = lab.send(Msg::Editor(Message::Move(Point::new(
                            x0 + (x1 - x0) * t,
                            y0 + (y1 - y0) * t + (t * 6.0).sin() * 20.0,
                        ))));
                    }
                } else {
                    let _ = lab.send(Msg::Editor(Message::Move(Point::new(x1, y1))));
                }
                let _ = lab.send(Msg::Editor(Message::Release));
            }
            let _ = lab.render(0.0);
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Select)));
            let idx = 29usize;
            let frame = lab.s.editor.as_ref().unwrap().doc.items[idx].frame();
            let grab = Point::new(frame.x + 8.0, frame.y + 8.0);
            let _ = lab.send(Msg::Editor(Message::Press(grab)));
            let mut frames = Vec::with_capacity(30);
            for s in 1..=30 {
                let t = s as f32 / 30.0;
                let p = Point::new(
                    grab.x - t * (w as f32) * 0.12,
                    grab.y - t * (h as f32) * 0.08,
                );
                let u = lab.send(Msg::Editor(Message::Move(p)));
                frames.push(lab.render(u));
            }
            let uc = lab.send(Msg::Editor(Message::Release));
            let cf = lab.render(uc);
            let tot: Vec<f64> = frames.iter().map(StudioFrame::total_ms).collect();
            let rast: Vec<f64> = frames.iter().map(|f| f.raster_ms).collect();
            let ud: Vec<f64> = frames.iter().map(|f| f.update_ms + f.draw_ms).collect();
            eprintln!(
                "  [30-items single-drag] total [{}] upd+draw [{}] rast [{}] f1={:.1}ms commit={:.1}ms",
                summarize(&tot),
                summarize(&ud),
                summarize(&rast),
                tot[0],
                cf.total_ms(),
            );

            // Multi-item group-drag (items 28 and 29 selected together)
            let _ = lab.send(Msg::Editor(Message::SelectIndex(28, false)));
            let _ = lab.send(Msg::Editor(Message::SelectIndex(29, true)));
            let _ = lab.render(0.0);
            let f28 = lab.s.editor.as_ref().unwrap().doc.items[28].frame();
            let g2 = Point::new(f28.x + 12.0, f28.y + 12.0);
            let _ = lab.send(Msg::Editor(Message::Press(g2)));
            let mut gframes = Vec::with_capacity(30);
            for s in 1..=30 {
                let t = s as f32 / 30.0;
                let p = Point::new(g2.x + t * (w as f32) * 0.05, g2.y + t * (h as f32) * 0.04);
                let u = lab.send(Msg::Editor(Message::Move(p)));
                gframes.push(lab.render(u));
            }
            let gu = lab.send(Msg::Editor(Message::Release));
            let gf = lab.render(gu);
            let gtot: Vec<f64> = gframes.iter().map(StudioFrame::total_ms).collect();
            let grast: Vec<f64> = gframes.iter().map(|f| f.raster_ms).collect();
            let gud: Vec<f64> = gframes.iter().map(|f| f.update_ms + f.draw_ms).collect();
            eprintln!(
                "  [30-items group-drag]  total [{}] upd+draw [{}] rast [{}] f1={:.1}ms commit={:.1}ms",
                summarize(&gtot),
                summarize(&gud),
                summarize(&grast),
                gtot[0],
                gf.total_ms(),
            );
        }

        // Part B: Item 4 — 90% and 150% zoom
        for z in [0.90f32, 1.50f32] {
            let mut lab = StudioLab::open(w, h, win, 1.0);
            let _ = lab.s.zoom_to(z, None);
            let _ = lab.s.take_scroll();
            lab.reset_scroll();
            lab.settle_mip();
            let _ = lab.render(0.0);
            let e = lab.s.editor.as_ref().unwrap();
            let (has_disp, crisp) = (e.display.is_some(), e.crisp);
            for (tname, tool) in [
                ("rect", Tool::Rect),
                ("arrow", Tool::Arrow),
                ("pen", Tool::Pen),
            ] {
                let _ = lab.send(Msg::Editor(Message::Tool(tool)));
                let a = Point::new(120.0, 120.0);
                let _ = lab.send(Msg::Editor(Message::Press(a)));
                let mut frames = Vec::with_capacity(20);
                for s in 1..=20 {
                    let t = s as f32 / 20.0;
                    let p = Point::new(120.0 + 420.0 * t, 120.0 + 260.0 * t);
                    let u = lab.send(Msg::Editor(Message::Move(p)));
                    frames.push(lab.render(u));
                }
                let uc = lab.send(Msg::Editor(Message::Release));
                let cf = lab.render(uc);
                let tot: Vec<f64> = frames.iter().map(StudioFrame::total_ms).collect();
                let rast: Vec<f64> = frames.iter().map(|f| f.raster_ms).collect();
                eprintln!(
                    "  [zoom {:3.0}% disp={} crisp={}] {:6} drag [{}] rast [{}] commit={:.1}ms",
                    z * 100.0,
                    has_disp,
                    crisp,
                    tname,
                    summarize(&tot),
                    summarize(&rast),
                    cf.total_ms(),
                );
            }
        }

        // Part C: Commit / Undo / Redo timings at Fit for Rect, Pen, Step, Text, Blur, Spotlight
        {
            for name in ["rect", "pen", "step", "text", "blur", "spotlight"] {
                let mut lab = StudioLab::open(w, h, win, 1.0);
                let _ = lab.render(0.0);
                let (a, b) = (
                    Point::new(w as f32 * 0.45, h as f32 * 0.35),
                    Point::new(w as f32 * 0.62, h as f32 * 0.55),
                );
                let mut blur_paint_ms = 0.0f64;
                match name {
                    "rect" => {
                        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
                        let _ = lab.send(Msg::Editor(Message::Press(a)));
                        let _ = lab.send(Msg::Editor(Message::Move(b)));
                    }
                    "pen" => {
                        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Pen)));
                        let _ = lab.send(Msg::Editor(Message::Press(a)));
                        for i in 1..=8 {
                            let t = i as f32 / 8.0;
                            let _ = lab.send(Msg::Editor(Message::Move(Point::new(
                                a.x + (b.x - a.x) * t,
                                a.y + (b.y - a.y) * t,
                            ))));
                        }
                    }
                    "step" => {
                        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
                        let _ = lab.send(Msg::Editor(Message::Press(a)));
                        let _ = lab.send(Msg::Editor(Message::Release));
                    }
                    "text" => {
                        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Text)));
                        let _ = lab.send(Msg::Editor(Message::Press(a)));
                        let _ = lab.send(Msg::Editor(Message::Release));
                        let _ = lab.send(Msg::Editor(Message::Type("Hello".into())));
                    }
                    "blur" => {
                        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
                        let _ = lab.send(Msg::Editor(Message::Press(a)));
                        let _ = lab.send(Msg::Editor(Message::Move(b)));
                        let t0 = std::time::Instant::now();
                        let _ = lab.s.editor.as_ref().unwrap().live_layer();
                        blur_paint_ms = t0.elapsed().as_secs_f64() * 1e3;
                    }
                    "spotlight" => {
                        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Spotlight)));
                        let _ = lab.send(Msg::Editor(Message::Press(a)));
                        let _ = lab.send(Msg::Editor(Message::Move(b)));
                    }
                    _ => unreachable!(),
                }
                let uc = match name {
                    "step" | "text" => lab.send(Msg::Editor(Message::Deselect)),
                    _ => lab.send(Msg::Editor(Message::Release)),
                };
                let fc = lab.render(uc);
                let disp_c = lab.s.editor.as_ref().unwrap().display.is_some();
                let uu = lab.send(Msg::Editor(Message::Undo));
                let fu = lab.render(uu);
                let disp_u = lab.s.editor.as_ref().unwrap().display.is_some();
                let ur = lab.send(Msg::Editor(Message::Redo));
                let fr = lab.render(ur);
                let disp_r = lab.s.editor.as_ref().unwrap().display.is_some();
                if name == "blur" {
                    eprintln!(
                        "  [commit/undo/redo] {:9} commit={:5.1}ms(disp={}, own_blur_paint={:.1}ms) undo={:5.1}ms(disp={}) redo={:5.1}ms(disp={})",
                        name,
                        fc.total_ms(),
                        disp_c,
                        blur_paint_ms,
                        fu.total_ms(),
                        disp_u,
                        fr.total_ms(),
                        disp_r,
                    );
                } else {
                    eprintln!(
                        "  [commit/undo/redo] {:9} commit={:5.1}ms(disp={}) undo={:5.1}ms(disp={}) redo={:5.1}ms(disp={})",
                        name,
                        fc.total_ms(),
                        disp_c,
                        fu.total_ms(),
                        disp_u,
                        fr.total_ms(),
                        disp_r,
                    );
                }
            }
        }
    }
}

#[test]
fn r19_screen_matches_export_for_mixed_spotlight_documents() {
    use crate::features::snip::annotate::{self, Annotator, Image};
    use crate::features::snip::screens::snip_toolbar::canvas::{self, ScreenMap};
    use crate::shared::kit::testing::shot;

    // 1) 1:1 pixel-for-pixel check: visible screen MUST equal annotate::render export
    //    for mixed documents (Spotlight + Step + Text + Rect, with and without Blur).
    let (w, h) = (480u32, 320u32);
    let base_crop = detailed_crop(w, h);
    let make = || {
        let mut e = Annotator::new(
            Image {
                width: w,
                height: h,
                rgba: base_crop.rgba.clone(),
            },
            1.0,
        );
        e.studio = true;
        e
    };

    type Case = (&'static str, fn(&mut Annotator));
    let cases: &[Case] = &[
        ("step_then_spotlight", |e| {
            e.update(Message::Tool(Tool::Step));
            e.update(Message::Press(Point::new(90.0, 90.0)));
            e.update(Message::Release);
            e.update(Message::Deselect);
            e.update(Message::Tool(Tool::Spotlight));
            e.update(Message::Press(Point::new(260.0, 150.0)));
            e.update(Message::Move(Point::new(420.0, 270.0)));
            e.update(Message::Release);
            e.update(Message::Deselect);
        }),
        ("rect_text_step_spotlight", |e| {
            e.update(Message::Tool(Tool::Rect));
            e.update(Message::Press(Point::new(30.0, 200.0)));
            e.update(Message::Move(Point::new(150.0, 290.0)));
            e.update(Message::Release);
            e.update(Message::Tool(Tool::Text));
            e.update(Message::Press(Point::new(200.0, 40.0)));
            e.update(Message::Release);
            e.update(Message::Type("Hi".into()));
            e.update(Message::Deselect);
            e.update(Message::Tool(Tool::Step));
            e.update(Message::Press(Point::new(90.0, 90.0)));
            e.update(Message::Release);
            e.update(Message::Deselect);
            e.update(Message::Tool(Tool::Spotlight));
            e.update(Message::Press(Point::new(260.0, 150.0)));
            e.update(Message::Move(Point::new(420.0, 270.0)));
            e.update(Message::Release);
            e.update(Message::Deselect);
        }),
        ("rect_step_blur_spotlight", |e| {
            e.update(Message::Tool(Tool::Rect));
            e.update(Message::Press(Point::new(30.0, 200.0)));
            e.update(Message::Move(Point::new(150.0, 290.0)));
            e.update(Message::Release);
            e.update(Message::Tool(Tool::Step));
            e.update(Message::Press(Point::new(90.0, 90.0)));
            e.update(Message::Release);
            e.update(Message::Deselect);
            e.update(Message::Tool(Tool::Blur));
            e.update(Message::Press(Point::new(200.0, 40.0)));
            e.update(Message::Move(Point::new(300.0, 100.0)));
            e.update(Message::Release);
            e.update(Message::Tool(Tool::Spotlight));
            e.update(Message::Press(Point::new(260.0, 150.0)));
            e.update(Message::Move(Point::new(420.0, 270.0)));
            e.update(Message::Release);
            e.update(Message::Deselect);
        }),
        ("rect_pen_spotlight_pure_vector", |e| {
            e.update(Message::Tool(Tool::Rect));
            e.update(Message::Press(Point::new(30.0, 40.0)));
            e.update(Message::Move(Point::new(150.0, 130.0)));
            e.update(Message::Release);
            e.update(Message::Tool(Tool::Pen));
            e.update(Message::Press(Point::new(40.0, 220.0)));
            e.update(Message::Move(Point::new(160.0, 260.0)));
            e.update(Message::Release);
            e.update(Message::Tool(Tool::Spotlight));
            e.update(Message::Press(Point::new(260.0, 150.0)));
            e.update(Message::Move(Point::new(420.0, 270.0)));
            e.update(Message::Release);
            e.update(Message::Deselect);
        }),
    ];

    for (name, build) in cases {
        let mut e = make();
        build(&mut e);
        let screen = shot::render(canvas::canvas(&e, 1.0, ScreenMap::ONE), w, h);
        let export = annotate::render(&e.doc);
        let mut diff = 0usize;
        let mut max_d = 0u8;
        for px in 0..(w as usize * h as usize) {
            let o = px * 4;
            let mut d = 0u8;
            for ch in 0..4 {
                d = d.max(screen[o + ch].abs_diff(export.rgba[o + ch]));
            }
            if d > 0 {
                diff += 1;
                max_d = max_d.max(d);
            }
        }
        assert_eq!(
            diff, 0,
            "{name} at 1x: screen != export ({diff} px differed, max={max_d})"
        );
    }

    // 2) Full-frame Fit screen == export check across ALL 4 mixed documents on a 5K Studio canvas.
    //    Compares every pixel of the visible Fit picture rect against annotate::downsample(export)
    //    (exact 0-diff on the entire frame for step_then_spotlight, and 0-diff everywhere outside
    //    the crisp Layer-2 vector strokes plus solid stroke interior parity when Layer-2 vectors exist).
    type FitCase = (&'static str, fn(&mut StudioLab));
    let fit_cases: &[FitCase] = &[
        ("step_then_spotlight", |lab| {
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(800.0, 700.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Spotlight)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(3000.0, 1500.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(4200.0, 2400.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
        }),
        ("rect_text_step_spotlight", |lab| {
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(400.0, 1800.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(1400.0, 2500.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Text)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(1800.0, 400.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Type("Hello".into())));
            let _ = lab.send(Msg::Editor(Message::Deselect));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(800.0, 700.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Spotlight)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(3000.0, 1500.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(4200.0, 2400.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
        }),
        ("rect_step_blur_spotlight", |lab| {
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(400.0, 1800.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(1400.0, 2500.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(800.0, 700.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(1900.0, 400.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(2600.0, 900.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Spotlight)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(3000.0, 1500.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(4200.0, 2400.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
        }),
        ("rect_pen_spotlight", |lab| {
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Rect)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(400.0, 400.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(1400.0, 1100.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Pen)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(500.0, 1900.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(1600.0, 2300.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Tool(Tool::Spotlight)));
            let _ = lab.send(Msg::Editor(Message::Press(Point::new(3000.0, 1500.0))));
            let _ = lab.send(Msg::Editor(Message::Move(Point::new(4200.0, 2400.0))));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
        }),
    ];

    for (name, build) in fit_cases {
        let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
        let pic_rect = lab.picture_phys_rect();
        let stride = lab.pixmap.width();
        let _ = lab.render(0.0);
        build(&mut lab);
        let _ = lab.render(0.0);
        let screen = lab.pixels();

        // Build exact reference: render export into a clean StudioLab at the exact same zoom/size
        // so display downsampling, alignment, and widget framing are identical.
        let export = annotate::render(&lab.s.editor.as_ref().unwrap().doc);
        let mut ref_lab =
            StudioLab::open(export.width, export.height, Size::new(1280.0, 800.0), 1.0);
        ref_lab.s.open(Picture::Crop(export));
        ref_lab.s.settle_open();
        let _ = ref_lab.s.take_scroll();
        ref_lab.settle_mip();
        let _ = ref_lab.render(0.0);
        let expected = ref_lab.pixels();

        // Exclude only unbaked Layer-2 vector items (drawn sharply at display resolution instead
        // of downsampled from 5K); EVERY other pixel of the entire Fit frame (Spotlight dim,
        // Spotlight hole, Step, Text, Blur, picture) MUST match export bit-for-bit!
        let vec_excl: Vec<[i32; 4]> = lab
            .s
            .editor
            .as_ref()
            .unwrap()
            .committed_vector_items()
            .into_iter()
            .map(|(_, it)| lab.image_rect_to_phys(it.bounds(), 4.0))
            .collect();
        let (diff, md, checked) = diff_outside(&expected, &screen, stride, pic_rect, &vec_excl);
        assert_eq!(
            diff, 0,
            "Fit {name}: full-frame screen != export ({diff}/{checked} px differed, max={md})"
        );
        // For any unbaked Layer-2 vector item, also verify its stroke is visibly painted on screen
        // (matching export's colour, not missing or dimmed).
        for rect in &vec_excl {
            let mut lit = 0usize;
            for y in rect[1].max(pic_rect[1] as i32)..rect[3].min(pic_rect[3] as i32) {
                for x in rect[0].max(pic_rect[0] as i32)..rect[2].min(pic_rect[2] as i32) {
                    let o = (y as u32 * stride + x as u32) as usize * 4;
                    let d = (0..3)
                        .map(|c| screen[o + c].abs_diff(expected[o + c]))
                        .max()
                        .unwrap();
                    if d <= 24 {
                        lit += 1;
                    }
                }
            }
            assert!(lit > 0, "Fit {name}: vector item missing on screen");
        }
    }
}

#[test]
fn r19_base_deltas_stay_exact_across_100_50_and_150_zoom_transitions() {
    let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
    let _ = lab.render(0.0);

    // Commit a Step and a Blur at Fit -> populates base_deltas without cloning 59 MB base.
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
    let _ = lab.send(Msg::Editor(Message::Press(Point::new(400.0, 320.0))));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.send(Msg::Editor(Message::Deselect));
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
    let _ = lab.send(Msg::Editor(Message::Press(Point::new(620.0, 260.0))));
    let _ = lab.send(Msg::Editor(Message::Move(Point::new(820.0, 420.0))));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.send(Msg::Editor(Message::Deselect));
    assert!(
        !lab.s.editor.as_ref().unwrap().base_deltas.is_empty(),
        "local edits used copy-on-write base_deltas"
    );

    // Check 1) 100 % zoom (renders base + base_deltas), 2) 50 % zoom (background mip uses
    // downsample_with_deltas), 3) 150 % zoom (flushes base_deltas for seam-free Linear).
    for z in [1.0f32, 0.50f32, 1.50f32, 2.44f32, 3.05f32] {
        let _ = lab.s.zoom_to(z, None);
        let _ = lab.s.take_scroll();
        lab.reset_scroll();
        lab.settle_mip();
        if z > 1.0 {
            assert!(
                lab.s.editor.as_ref().unwrap().base_deltas.is_empty(),
                "zoom {z}: deltas flushed on non-100% direct-base zoom"
            );
            // Also make a new baked edit directly at 244 % / 305 %: must stay delta-free!
            if z > 2.0 {
                let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
                let _ = lab.send(Msg::Editor(Message::Press(Point::new(260.0, 220.0))));
                let _ = lab.send(Msg::Editor(Message::Release));
                let _ = lab.send(Msg::Editor(Message::Deselect));
                assert!(
                    lab.s.editor.as_ref().unwrap().base_deltas.is_empty(),
                    "zoom {z}: new edit at fractional >200% folds into base"
                );
            }
        }
        let _ = lab.render(0.0);
        let shown = lab.pixels();
        let pic_rect = lab.picture_phys_rect();
        let stride = lab.pixmap.width();

        // Compare against a fresh full refresh at this zoom.
        let mut ref_lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
        ref_lab.s.editor.as_mut().unwrap().doc = lab.s.editor.as_ref().unwrap().doc.clone();
        ref_lab.s.editor.as_mut().unwrap().refresh();
        let _ = ref_lab.s.zoom_to(z, None);
        let _ = ref_lab.s.take_scroll();
        ref_lab.reset_scroll();
        ref_lab.settle_mip();
        let _ = ref_lab.render(0.0);
        let fresh = ref_lab.pixels();

        let (diff, md, checked) = diff_outside(&fresh, &shown, stride, pic_rect, &[]);
        assert_eq!(
            diff, 0,
            "zoom {z} with base_deltas: {diff}/{checked} px differed from fresh refresh (max={md})"
        );
    }
}

#[test]
fn r19_stale_mip_thread_result_is_dropped_after_edit() {
    let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
    let pic_rect = lab.picture_phys_rect();
    let stride = lab.pixmap.width();
    let _ = lab.render(0.0);

    // Zoom slightly to spawn a background mip thread for the pre-edit picture.
    let fit_z = lab.s.editor.as_ref().unwrap().zoom;
    let _ = lab.s.zoom_to(fit_z * 1.35, None);
    let _ = lab.s.take_scroll();
    lab.reset_scroll();
    assert!(
        lab.s.mips.as_ref().is_some_and(|m| m.pending.is_some()),
        "zoom change spawned a background mip thread"
    );

    // Immediately commit a Step BEFORE the background mip thread finishes.
    let at = Point::new(900.0, 700.0);
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
    let _ = lab.send(Msg::Editor(Message::Press(at)));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.send(Msg::Editor(Message::Deselect));

    // Let any in-flight mip thread finish and drain Msg::Frame.
    lab.settle_mip();
    let _ = lab.render(0.0);
    let shown = lab.pixels();

    // Compare against a fresh full refresh at the exact same state.
    lab.s.editor.as_mut().unwrap().refresh();
    lab.settle_mip();
    let _ = lab.render(0.0);
    let fresh = lab.pixels();

    let (diff, md, checked) = diff_outside(&fresh, &shown, stride, pic_rect, &[]);
    assert_eq!(
        diff, 0,
        "stale mip overwrote edit: {diff}/{checked} px differed from fresh render (max={md})"
    );
}

#[test]
fn r19_patch_base_does_not_clip_adjacent_blur_in_downsample_margin() {
    let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
    let pic_rect = lab.picture_phys_rect();
    let stride = lab.pixmap.width();
    let _ = lab.render(0.0);

    // Place a Gaussian look Blur in the centre (strong edge coupling if clipped by src_r).
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
    let _ = lab.send(Msg::Editor(Message::BlurStyle(annotate::blur::Style::Blur)));
    let _ = lab.send(Msg::Editor(Message::Press(Point::new(1800.0, 1000.0))));
    let _ = lab.send(Msg::Editor(Message::Move(Point::new(2200.0, 1400.0))));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.send(Msg::Editor(Message::Deselect));
    let _ = lab.render(0.0);
    let blur_b = lab.s.editor.as_ref().unwrap().doc.items[0].bounds();

    // Measure exact Step half-width, then place a Step just 2 image px to the right of
    // Blur.bounds(): disjoint from Blur.bounds(), yet strictly inside the ~5-10 px Fit
    // downsampler margin of src_r!
    let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
    let probe = Point::new(1000.0, 500.0);
    let _ = lab.send(Msg::Editor(Message::Press(probe)));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.send(Msg::Editor(Message::Deselect));
    let step_half = lab.s.editor.as_ref().unwrap().doc.items[1].bounds().width * 0.5;
    let _ = lab.send(Msg::Editor(Message::Undo));

    let step_at = Point::new(
        blur_b.x + blur_b.width + step_half + 2.0,
        blur_b.y + blur_b.height * 0.5,
    );
    let _ = lab.send(Msg::Editor(Message::Press(step_at)));
    let _ = lab.send(Msg::Editor(Message::Release));
    let _ = lab.send(Msg::Editor(Message::Deselect));
    let step_b = lab.s.editor.as_ref().unwrap().doc.items[1].bounds();
    let gap = step_b.x - (blur_b.x + blur_b.width);
    assert!(
        !blur_b.intersects(&step_b) && (0.5..=4.0).contains(&gap),
        "Step must be disjoint yet inside Fit downsample margin: gap={gap}"
    );
    let _ = lab.render(0.0);
    let patched = lab.pixels();

    lab.s.editor.as_mut().unwrap().refresh();
    let _ = lab.render(0.0);
    let fresh = lab.pixels();

    let (diff, md, checked) = diff_outside(&fresh, &patched, stride, pic_rect, &[]);
    assert_eq!(
        diff, 0,
        "adjacent Blur clipped in src_r margin: {diff}/{checked} px differed (max={md})"
    );
}

#[test]
fn r19_base_deltas_area_cap_and_17th_edit_fold_cleanly() {
    // Part A: area cap (~25 % of picture): a large Blur (> 25 % of 5K) folds into base
    // rather than storing a huge delta texture.
    {
        let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
        let pic_bytes = 5120usize * 2880 * 4;
        let _ = lab.render(0.0);
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Blur)));
        let _ = lab.send(Msg::Editor(Message::BlurShape(annotate::blur::Kind::Rect)));
        let _ = lab.send(Msg::Editor(Message::Press(Point::new(400.0, 300.0))));
        let _ = lab.send(Msg::Editor(Message::Move(Point::new(3600.0, 2200.0))));
        let _ = lab.send(Msg::Editor(Message::Release));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        let e = lab.s.editor.as_ref().unwrap();
        assert!(
            e.base_delta_bytes() * 4 <= pic_bytes,
            "big Blur delta bytes {} exceeded 25% cap ({})",
            e.base_delta_bytes(),
            pic_bytes / 4
        );
        assert!(e.display.is_some() && e.crisp, "display stays crisp");
        let _ = lab.render(0.0);
        let shown = lab.pixels();
        let pic_rect = lab.picture_phys_rect();
        let stride = lab.pixmap.width();
        lab.s.editor.as_mut().unwrap().refresh();
        let _ = lab.render(0.0);
        let fresh = lab.pixels();
        let (diff, md, checked) = diff_outside(&fresh, &shown, stride, pic_rect, &[]);
        assert_eq!(
            diff, 0,
            "big Blur fold: {diff}/{checked} px differed (max={md})"
        );
    }

    // Part B: 17th edit triggers the count-cap fold slow path, followed by Undo and Redo.
    {
        let mut lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
        let pic_bytes = 5120usize * 2880 * 4;
        let pic_rect = lab.picture_phys_rect();
        let stride = lab.pixmap.width();
        let _ = lab.render(0.0);
        let _ = lab.send(Msg::Editor(Message::Tool(Tool::Step)));
        for i in 0..17usize {
            let col = (i % 6) as f32;
            let row = (i / 6) as f32;
            let at = Point::new(500.0 + col * 720.0, 450.0 + row * 720.0);
            let _ = lab.send(Msg::Editor(Message::Press(at)));
            let _ = lab.send(Msg::Editor(Message::Release));
            let _ = lab.send(Msg::Editor(Message::Deselect));
            let e = lab.s.editor.as_ref().unwrap();
            assert!(
                e.base_delta_bytes() * 4 <= pic_bytes,
                "step {i}: delta bytes exceeded 25% cap"
            );
            assert!(e.display.is_some() && e.crisp);
        }
        assert!(
            lab.s.editor.as_ref().unwrap().base_deltas.is_empty(),
            "17th edit folded base_deltas into base"
        );

        let check_matches_fresh = |lab: &mut StudioLab, phase: &str| {
            let _ = lab.render(0.0);
            let shown = lab.pixels();
            let mut ref_lab = StudioLab::open(5120, 2880, Size::new(1280.0, 800.0), 1.0);
            ref_lab.s.editor.as_mut().unwrap().doc = lab.s.editor.as_ref().unwrap().doc.clone();
            ref_lab.s.editor.as_mut().unwrap().refresh();
            ref_lab.settle_mip();
            let _ = ref_lab.render(0.0);
            let fresh = ref_lab.pixels();
            let (diff, md, checked) = diff_outside(&fresh, &shown, stride, pic_rect, &[]);
            assert_eq!(diff, 0, "{phase}: {diff}/{checked} px differed (max={md})");
        };

        check_matches_fresh(&mut lab, "after 17th commit");
        let _ = lab.send(Msg::Editor(Message::Undo));
        check_matches_fresh(&mut lab, "after undo of 17th");
        let _ = lab.send(Msg::Editor(Message::Redo));
        let _ = lab.send(Msg::Editor(Message::Deselect));
        check_matches_fresh(&mut lab, "after redo of 17th");
    }
}
