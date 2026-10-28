//! The OpenCircle logo in the theme's colour (`core/logo.js`): the design's 16
//! tone ramp, mixed from the theme's accent. The vector is baked once into
//! `assets/logo/tones.png` (`scripts/logo-tones.py`: per pixel where on the ramp
//! it sits, 0..255 = tone 0..15 in 1/17 steps, plus alpha); no SVG at run time.
//! The window, taskbar, tray and exe icons stay the fixed bitmaps (the OS needs them).
use std::cell::RefCell;

use iced::Color;
use iced::widget::image::Handle;

use super::themes;

const TONES: &[u8] = include_bytes!("../../../assets/logo/tones.png");
/// Pixels drawn per logical pixel (the CPU renderer scales down, so it stays sharp at 2x).
const SCALE: u32 = 2;

/// `--logo-t0…t15` from `accent` (`tokens.css`): t0-t7 mix the accent with black at
/// these percentages, t8 is the accent, t9-t15 mix it with white.
pub fn ramp(accent: [u8; 3]) -> [[u8; 3]; 16] {
    const DARK: [u32; 8] = [26, 36, 46, 57, 68, 79, 89, 97];
    const LIGHT: [u32; 7] = [88, 77, 66, 54, 42, 28, 14];
    let mix = |pct: u32, to: u32| {
        accent.map(|c| ((u32::from(c) * pct + to * (100 - pct) + 50) / 100) as u8)
    };
    std::array::from_fn(|i| match i {
        0..=7 => mix(DARK[i], 0),
        8 => accent,
        _ => mix(LIGHT[i - 9], 255),
    })
}

fn rgb8(c: Color) -> [u8; 3] {
    [c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8)
}

/// The 256 colours between the 16 tones (what a baked value stands for).
fn lut(ramp: &[[u8; 3]; 16]) -> [[u8; 3]; 256] {
    std::array::from_fn(|v| {
        let i = (v / 17).min(14);
        let u = (v - i * 17) as f32 / 17.0;
        std::array::from_fn(|k| {
            (f32::from(ramp[i][k]) * (1.0 - u) + f32::from(ramp[i + 1][k]) * u).round() as u8
        })
    })
}

/// The logo as `side` x `side` RGBA in `accent`'s colours (box-filtered).
fn render(accent: [u8; 3], side: u32) -> Vec<u8> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(TONES))
        .read_info()
        .expect("the bundled tones decode");
    let mut grey = vec![0; reader.output_buffer_size().expect("sized")];
    let info = reader
        .next_frame(&mut grey)
        .expect("the bundled tones decode");
    let n = info.width as usize;
    let side = side as usize;
    let lut = lut(&ramp(accent));
    let span = |o: usize| {
        let from = o * n / side;
        (from, ((o + 1) * n / side).max(from + 1).min(n))
    };
    let mut out = Vec::with_capacity(side * side * 4);
    for y in 0..side {
        let (y0, y1) = span(y);
        for x in 0..side {
            let (x0, x1) = span(x);
            let (mut sum, mut a_sum, mut count) = ([0u32; 3], 0u32, 0u32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * n + sx) * 2;
                    let (v, a) = (grey[i] as usize, u32::from(grey[i + 1]));
                    for k in 0..3 {
                        sum[k] += u32::from(lut[v][k]) * a;
                    }
                    a_sum += a;
                    count += 1;
                }
            }
            match a_sum {
                0 => out.extend([0, 0, 0, 0]),
                _ => out.extend([
                    (sum[0] / a_sum) as u8,
                    (sum[1] / a_sum) as u8,
                    (sum[2] / a_sum) as u8,
                    (a_sum / count) as u8,
                ]),
            }
        }
    }
    out
}

type Cached = ([u8; 3], Vec<(u32, Handle)>);

thread_local! {
    /// The handles of the accent in use, by size; a new accent drops them all.
    static CACHE: RefCell<Cached> = const { RefCell::new(([0; 3], Vec::new())) };
}

/// The logo `px` logical pixels square, in the current theme's accent (one handle
/// per size, so the renderer's image cache keeps its pixels between frames).
pub fn logo(px: u32) -> Handle {
    let accent = rgb8(themes::c().accent);
    CACHE.with_borrow_mut(|(of, handles)| {
        if *of != accent {
            *of = accent;
            handles.clear();
        }
        if let Some((_, h)) = handles.iter().find(|(p, _)| *p == px) {
            return h.clone();
        }
        let side = (px * SCALE).clamp(1, 512);
        let h = Handle::from_rgba(side, side, render(accent, side));
        handles.push((px, h.clone()));
        h
    })
}

/// The fixed window icon (taskbar, Alt+Tab), decoded once.
pub struct WindowIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn window_icon() -> &'static WindowIcon {
    static ICON: std::sync::LazyLock<WindowIcon> = std::sync::LazyLock::new(|| {
        let (width, height, rgba) = decode(include_bytes!("../../../assets/icons/64x64.png"));
        WindowIcon {
            width,
            height,
            rgba,
        }
    });
    &ICON
}

/// A bundled RGBA PNG as an image (decode it once: keep the handle).
pub fn png_handle(bytes: &[u8]) -> Handle {
    let (w, h, rgba) = decode(bytes);
    Handle::from_rgba(w, h, rgba)
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("a bundled PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("sized")];
    let info = reader.next_frame(&mut rgba).expect("the PNG decodes");
    rgba.truncate(info.buffer_size());
    (info.width, info.height, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_icon_decodes() {
        let l = window_icon();
        assert_eq!((l.width, l.height), (64, 64));
        assert_eq!(l.rgba.len(), 64 * 64 * 4);
        assert!(l.rgba.chunks(4).any(|p| p[3] > 0), "not blank");
    }

    fn by_id(id: &str) -> &'static themes::Theme {
        themes::THEMES.into_iter().find(|t| t.id == id).unwrap()
    }

    #[test]
    fn the_ramp_follows_the_design() {
        let r = ramp([0x38, 0xbd, 0xf8]);
        assert_eq!(r[8], [0x38, 0xbd, 0xf8]);
        assert_eq!(r[0], [15, 49, 64]); // 26 % over black
        assert_eq!(r[15], [227, 246, 254]); // 14 % over white
    }

    /// Every opaque pixel is a ramp colour of the theme: no bake purple is left.
    #[test]
    fn every_tone_is_replaced_for_every_theme() {
        let bake_purple = [0x73u8, 0x29, 0xe8];
        for t in themes::THEMES {
            let accent = rgb8(t.c.accent);
            let pixels = render(accent, 512);
            let lut = lut(&ramp(accent));
            assert!(pixels.chunks(4).any(|p| p[3] > 250), "{}: drawn", t.id);
            for p in pixels.chunks(4).filter(|p| p[3] > 250) {
                assert!(
                    lut.contains(&[p[0], p[1], p[2]]),
                    "{}: {p:?} is off the ramp",
                    t.id
                );
                assert!(p[..3] != bake_purple || accent == bake_purple, "{}", t.id);
            }
        }
    }

    #[test]
    fn the_colour_follows_the_theme() {
        let brightest = |id: &str| {
            let p = render(rgb8(by_id(id).c.accent), 64);
            let sum = |p: &&[u8]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
            let best = p.chunks(4).filter(|p| p[3] > 250).max_by_key(sum).unwrap();
            [best[0], best[1], best[2]]
        };
        assert_ne!(brightest("sapphire"), brightest("emerald"));
    }

    #[test]
    fn the_cache_is_rebuilt_on_a_theme_change() {
        themes::set(by_id("sapphire"));
        let (a, b) = (logo(20), logo(20));
        assert_eq!(a.id(), b.id(), "one handle per size and accent");
        themes::set(by_id("emerald"));
        assert_ne!(logo(20).id(), a.id(), "a new theme, new pixels");
        CACHE.with_borrow(|(_, h)| assert_eq!(h.len(), 1, "the old handles are dropped"));
        themes::set(by_id("sapphire"));
    }
}
