//! Golden images: a view rendered as its window draws it, compared with a PNG
//! under `tests/fixtures/<dir>/`. `OPENCIRCLE_UPDATE_GOLDEN=1` writes them
//! anew (then look at every one). Drawn with the bundled Inter only (CI
//! Linux); where the UI font is Segoe UI (Windows) nothing is compared.
use std::path::{Path, PathBuf};

use iced::{Element, Font, Size};

use super::shot;

pub fn enabled() -> bool {
    super::super::text::ui_font() == Font::with_name("Inter")
}

fn file(dir: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir)
        .join(format!("{name}.png"))
}

/// `view` at each of `scales` (logical `size`) against
/// `<dir>/<name>@<scale>x.png`: what differs, one line each.
pub fn check<'a, Message: 'a>(
    dir: &str,
    name: &str,
    size: Size,
    scales: &[f32],
    view: impl Fn() -> Element<'a, Message>,
) -> Vec<String> {
    if !enabled() {
        return Vec::new();
    }
    let mut failed = Vec::new();
    for &scale in scales {
        let (_, pixels) = shot::redrawn(|i| (i == 0).then(&view), size, scale);
        let (w, h) = ((size.width * scale) as u32, (size.height * scale) as u32);
        if let Err(e) = compare(dir, &format!("{name}@{scale}x"), w, h, &pixels) {
            failed.push(e);
        }
    }
    failed
}

/// RGBA `pixels` (`w` x `h`) against `<dir>/<name>.png`. Tiny differences in
/// anti-aliasing between CPUs are fine: a channel off by more than 24 counts,
/// on more than 0.1 % of the pixels.
pub fn compare(dir: &str, name: &str, w: u32, h: u32, pixels: &[u8]) -> Result<(), String> {
    let file = file(dir, name);
    if std::env::var_os("OPENCIRCLE_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        crate::actions::write_png(&file, w, h, pixels).unwrap();
        return Ok(());
    }
    let Ok(golden) = std::fs::read(&file) else {
        return Err(format!("{name}: no golden image"));
    };
    let (gw, gh, golden) = decode(&golden);
    if (gw, gh) != (w, h) {
        return Err(format!("{name}: {w}x{h}, golden {gw}x{gh}"));
    }
    let off = (pixels.chunks(4).zip(golden.chunks(4)))
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 24))
        .count();
    match off * 1000 > (w * h) as usize {
        true => {
            // CI uploads these on a failure, to see what was drawn instead.
            if let Some(out) = std::env::var_os("OPENCIRCLE_GOLDEN_OUT") {
                let file = std::path::Path::new(&out)
                    .join(dir)
                    .join(format!("{name}.png"));
                let _ = std::fs::create_dir_all(file.parent().unwrap());
                let _ = crate::actions::write_png(&file, w, h, pixels);
            }
            Err(format!("{name}: {off} pixels differ"))
        }
        false => Ok(()),
    }
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut rgba).unwrap();
    rgba.truncate(info.buffer_size());
    (info.width, info.height, rgba)
}
