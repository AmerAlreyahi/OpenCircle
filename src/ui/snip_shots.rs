//! The snip capture flow's screens (N-73 states 1-7) drawn as the overlay
//! draws them, over a synthetic desktop, against golden images
//! (`tests/fixtures/screens/snip-<state>-<theme>@1x.png`, Sapphire + Daylight;
//! `OPENCIRCLE_UPDATE_GOLDEN=1` writes them anew). `OPENCIRCLE_RENDER_DIR`
//! also saves each one there (for the proof sheets).
use iced::widget::image;
use iced::{Point, Rectangle, Size, Vector};

use super::app::App;
use super::app::message::Message;
use super::app::tests::snipping_on;
use super::frame_times::frames;
use super::kit::testing::golden;
use super::kit::themes::{self, DAYLIGHT, SAPPHIRE};
use super::overlay::Event;
use super::screens::long_capture;
use super::screens::tools::{self, Which, picker, ruler};
use super::snip;
use crate::actions::Crop;
use crate::capture::scroll::Update;
use crate::capture::stitch::MAX_HEIGHT;
use crate::editor::Message as E;

const W: u32 = 1280;
const H: u32 = 720;
/// The light window under the pointer, and the area picked in it.
const WINDOW: Rectangle = Rectangle {
    x: 80.0,
    y: 50.0,
    width: 700.0,
    height: 520.0,
};
const AREA: Rectangle = Rectangle {
    x: 110.0,
    y: 130.0,
    width: 520.0,
    height: 300.0,
};

/// A desktop: a blue-grey wallpaper, a light window (title bar, text lines, a
/// dark picture) and a dark one beside it, a taskbar.
fn desktop() -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    let mut fill = |r: Rectangle, c: [u8; 3]| {
        for y in r.y as u32..(r.y + r.height) as u32 {
            for x in r.x as u32..(r.x + r.width) as u32 {
                let i = ((y * W + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
    };
    let rect = |x, y, w, h| Rectangle::new(Point::new(x, y), Size::new(w, h));
    for y in 0..H {
        let t = y as f32 / H as f32;
        let c = [(70.0 + 40.0 * t) as u8, (88.0 + 30.0 * t) as u8, 128];
        fill(rect(0.0, y as f32, W as f32, 1.0), c);
    }
    fill(WINDOW, [246, 247, 250]);
    fill(rect(80.0, 50.0, 700.0, 34.0), [226, 230, 238]);
    for i in 0..9 {
        let w = [
            520.0, 480.0, 300.0, 500.0, 440.0, 360.0, 510.0, 280.0, 400.0,
        ][i];
        fill(rect(110.0, 110.0 + 26.0 * i as f32, w, 9.0), [70, 78, 92]);
    }
    fill(rect(110.0, 360.0, 330.0, 180.0), [22, 34, 70]);
    fill(rect(470.0, 360.0, 280.0, 60.0), [37, 99, 235]);
    fill(rect(840.0, 90.0, 380.0, 470.0), [24, 28, 38]);
    for i in 0..8 {
        fill(
            rect(870.0, 130.0 + 30.0 * i as f32, 260.0, 8.0),
            [150, 160, 180],
        );
    }
    fill(rect(0.0, 680.0, W as f32, 40.0), [18, 22, 32]);
    px
}

/// A snip over the desktop, the pointer on the light window.
fn snipping() -> App {
    let mut app = snipping_on(W, H, WINDOW);
    app.o.frame = Some(image::Handle::from_rgba(W, H, desktop()));
    app
}

/// After the pick: the live area with its handles and the docked toolbar.
fn picked() -> App {
    let mut app = snipping();
    app.select(AREA);
    let crop = app.crop().unwrap();
    let route = super::snip::route(crate::config::SnipAfter::Editor);
    let _ = app.snip_routed(crop, true, route);
    settle(&mut app);
    app
}

/// Every motion run to its end.
fn settle(app: &mut App) {
    let now = std::time::Instant::now();
    for ms in [0, 400, 800, 1600, 4000] {
        let _ = app.update(super::app::message::Message::Frame(
            now + std::time::Duration::from_millis(ms),
        ));
    }
}

/// The states, each a fresh app.
fn states() -> Vec<(&'static str, App)> {
    let mut out = Vec::new();
    // 1: frozen, the hint at the top, the pointer on the wallpaper
    let mut app = snipping();
    let _ = app.on_overlay(Event::Hover(Point::new(1100.0, 640.0)));
    out.push(("1-freeze", app));
    // 2: a window under the pointer
    let mut app = snipping();
    let _ = app.on_overlay(Event::Hover(Point::new(400.0, 300.0)));
    out.push(("2-hover", app));
    // 3: an area dragged
    let mut app = snipping();
    let from = AREA.position();
    let _ = app.on_overlay(Event::Press {
        at: from,
        shift: false,
    });
    for i in 1..=10 {
        let at = from + Vector::new(AREA.width, AREA.height) * (i as f32 / 10.0);
        let _ = app.on_overlay(Event::Move(at));
    }
    out.push(("3-drag", app));
    // 4: picked
    out.push(("4-picked", picked()));
    // 5: resizing by the bottom-right handle
    let mut app = picked();
    let corner = AREA.position() + Vector::new(AREA.width, AREA.height);
    let _ = app.on_editor(E::Press(corner));
    let _ = app.on_editor(E::Move(corner + Vector::new(60.0, 40.0)));
    settle(&mut app);
    out.push(("5-resizing", app));
    // 6: moved by the keyboard
    let mut app = picked();
    let _ = app.on_editor(E::Nudge((1, 0), (false, true, false)));
    settle(&mut app);
    out.push(("6a-keyboard", app));
    // 6b: Shift+Tab twice: on Save
    let mut app = picked();
    let _ = app.on_editor(E::Focus(true));
    let _ = app.on_editor(E::Focus(true));
    out.push(("6b-tab", app));
    // 7: long capture: started (the hint), going (the preview), at the limit
    out.push(("7a-long-start", long(0, false)));
    out.push(("7b-long", long(3240, false)));
    out.push(("7c-long-limit", long(MAX_HEIGHT, true)));
    // 8: the colour picker: started from the toolbar (its hint), then three
    // colours picked (the swatches, the Copied pill)
    let mut app = picked();
    let _ = app.on_editor(E::ScreenTool(Which::Picker));
    let _ = app.on_overlay(Event::Hover(Point::new(520.0, 390.0)));
    out.push(("8a-picker", app));
    let mut app = tool(Which::Picker);
    for at in [(200.0, 450.0), (900.0, 300.0), (520.0, 390.0)] {
        let at = Point::new(at.0, at.1);
        let _ = app.on_overlay(Event::Press { at, shift: false });
        let _ = app.on_overlay(Event::Release);
    }
    out.push(("8b-picker-copied", app));
    // 9: the ruler: just opened (its hint), a line, across the dark picture, a box
    let mut app = snipping();
    let _ = app.start_tool(Which::Ruler);
    let _ = app.on_overlay(Event::Hover(Point::new(250.0, 450.0)));
    out.push(("9-ruler", app));
    let mut app = tool(Which::Ruler);
    let _ = app.on_overlay(Event::Press {
        at: Point::new(470.0, 300.0),
        shift: false,
    });
    let _ = app.on_overlay(Event::Move(Point::new(760.0, 470.0)));
    out.push(("9a-ruler-line", app));
    let mut app = tool(Which::Ruler);
    let _ = app.on_tool(tools::Msg::Mode(ruler::Mode::Across));
    let _ = app.on_overlay(Event::Hover(Point::new(250.0, 450.0)));
    out.push(("9b-ruler-across", app));
    let mut app = tool(Which::Ruler);
    let _ = app.on_tool(tools::Msg::Mode(ruler::Mode::Box));
    let _ = app.on_overlay(Event::Press {
        at: Point::new(470.0, 360.0),
        shift: false,
    });
    let _ = app.on_overlay(Event::Move(Point::new(750.0, 420.0)));
    let _ = app.on_overlay(Event::Release);
    out.push(("9c-ruler-box", app));
    out
}

/// A screen tool opened by its shortcut, its hint gone.
fn tool(which: Which) -> App {
    let mut app = snipping();
    let _ = app.start_tool(which);
    let opening = app.opening;
    let _ = app.update(Message::Snip(snip::Msg::HintGone(opening)));
    app
}

/// Long capture from the picked area, after `height` px (none: just
/// started); its preview is the area's picture, scaled as the thread does.
fn long(height: u32, full: bool) -> App {
    let mut app = picked();
    let picture = app.crop().unwrap();
    let _ = app.on_editor(E::Scroll);
    if height == 0 {
        return app;
    }
    let card = long_capture::layout(AREA, app.o.screen).card.unwrap();
    let view = long_capture::viewport(card);
    let page = ::image::RgbaImage::from_raw(picture.width, picture.height, picture.rgba).unwrap();
    let small = ::image::imageops::thumbnail(&page, view.width as u32, view.height as u32 / 2);
    let preview = Crop {
        width: small.width(),
        height: small.height(),
        rgba: small.into_raw(),
    };
    let update = Update {
        height,
        full,
        lost: false,
        preview: Some(preview),
    };
    let opening = app.opening;
    let _ = app.update(Message::Snip(snip::Msg::Progress(update)));
    let _ = app.update(Message::Snip(snip::Msg::HintGone(opening)));
    app
}

fn shoot(name: &str, app: &mut App) -> Result<(), String> {
    let drawn = frames(app, 1, true, |_, _, _| {});
    let rgba = &drawn[0].rgba;
    if let Some(dir) = std::env::var_os("OPENCIRCLE_RENDER_DIR") {
        let file = std::path::Path::new(&dir).join(format!("{name}.png"));
        let _ = std::fs::create_dir_all(file.parent().unwrap());
        crate::actions::write_png(&file, W, H, rgba).unwrap();
    }
    if !golden::enabled() {
        return Ok(());
    }
    golden::compare("screens", &format!("{name}@1x"), W, H, rgba)
}

/// Long capture's toolbar, Latin and Arabic (the app has no Arabic UI yet:
/// the screens above are Latin), going and at the limit, both themes.
#[test]
fn the_long_capture_toolbar_mirrors() {
    let mut failed = Vec::new();
    let size = long_capture::BAR;
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        for (name, height, full, rtl) in [
            ("ltr", 3240, false, false),
            ("rtl", 3240, false, true),
            ("limit-rtl", MAX_HEIGHT, true, true),
        ] {
            let view = long_capture::toolbar(height, full, false, rtl);
            let px = super::kit::testing::shot::render(view, size.width as u32, size.height as u32);
            let name = format!("snip-7-toolbar-{name}-{}@1x", theme.id);
            if golden::enabled() {
                let (w, h) = (size.width as u32, size.height as u32);
                failed.extend(golden::compare("screens", &name, w, h, &px).err());
            }
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The screen tools' toolbars, Latin and Arabic, both themes: the picker
/// with no colours yet and with some, the ruler with a measure.
#[test]
fn the_screen_tool_toolbars_mirror() {
    let mut failed = Vec::new();
    let measure = ruler::Measure {
        mode: ruler::Mode::Line,
        a: (10, 10),
        b: (310, 410),
        scale: 1.5,
        live: false,
    };
    let recent = [[30, 144, 255], [239, 68, 68], [255, 255, 255]];
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        for rtl in [false, true] {
            let dir = if rtl { "rtl" } else { "ltr" };
            let bars = [
                ("picker-empty", picker::toolbar(None, &[], rtl)),
                (
                    "picker",
                    picker::toolbar(Some([30, 144, 255]), &recent, rtl),
                ),
                (
                    "ruler",
                    ruler::toolbar(ruler::Mode::Line, Some(&measure), rtl),
                ),
                (
                    "ruler-empty",
                    ruler::toolbar(ruler::Mode::Across, None, rtl),
                ),
            ];
            for (name, bar) in bars {
                let (w, h) = (620, 60);
                let px = super::kit::testing::shot::render(bar, w, h);
                let name = format!("snip-tools-{name}-{dir}-{}@1x", theme.id);
                if golden::enabled() {
                    failed.extend(golden::compare("screens", &name, w, h, &px).err());
                }
            }
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// States 1-9 in Sapphire and Daylight.
#[test]
fn the_snip_screens_match_their_golden_images() {
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        for (state, mut app) in states() {
            let name = format!("snip-{state}-{}", theme.id);
            failed.extend(shoot(&name, &mut app).err());
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}
