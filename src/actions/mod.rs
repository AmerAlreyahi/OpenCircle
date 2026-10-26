//! The action registry: ONE list of what can be done with a selection. The action
//! bar renders it in order; the first entry is the default (Enter).
//! To add an action: see ARCHITECTURE.md ("Add an action").
pub mod ask;
pub mod copy;
pub mod edit;
pub mod entities;
pub mod lens;
pub mod qr;
pub mod save;
pub mod text;
pub mod translate;

use iced::Rectangle;

/// Every action, in action-bar order. The first one runs on Enter.
pub const ACTIONS: &[Action] = &[
    lens::ACTION,
    ask::ACTION,
    text::ACTION,
    translate::ACTION,
    edit::ACTION,
    copy::ACTION,
    save::ACTION,
];

#[derive(Debug)]
pub struct Action {
    /// Stable name (logs, later the config's quick actions).
    pub id: &'static str,
    /// Button text.
    pub label: &'static str,
    pub key: Key,
    /// Glyph in `assets/lucide/lucide-subset.ttf` (must be listed in its `subset.sh`).
    pub icon: char,
    pub run: Run,
}

/// What running an action does.
#[derive(Debug, Clone, Copy)]
pub enum Run {
    /// Close the overlay, then do this with the crop (Copy, Save).
    Finish(fn(Crop) -> Result<(), String>),
    /// Open this result panel beside the selection; the overlay stays.
    Panel(Panel),
    /// Turn the action bar into the editing toolbar over the selection.
    Edit,
    /// Replace the selection's text by its translation, in place.
    Translate,
}

/// The result panels (`ui/panel.rs` draws them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Lens,
    Text,
    Ask,
}

/// The action's key while the overlay is open: a Latin letter (layout-independent),
/// alone or with Ctrl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub ctrl: bool,
    pub letter: char,
}

/// The action bound to `letter` (lower-case Latin) with/without Ctrl; none for an
/// action whose Labs feature is off.
pub fn for_key(labs: &crate::labs::Labs, letter: char, ctrl: bool) -> Option<&'static Action> {
    let key = Key { ctrl, letter };
    (ACTIONS.iter())
        .find(|action| action.key == key)
        .filter(|action| crate::labs::action_on(labs, action.id))
}

/// The action Enter runs: the config's `general.default_action` (the first
/// registry entry if the registry ever lacks it).
pub fn default_for(cfg: &crate::config::Config) -> &'static Action {
    let id = cfg.general.default_action.id();
    ACTIONS.iter().find(|a| a.id == id).unwrap_or(&ACTIONS[0])
}

/// A crop of the frozen frame, in physical pixels.
#[derive(Clone)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Its size only (a log line, not megabytes of pixels).
impl std::fmt::Debug for Crop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Crop({}x{})", self.width, self.height)
    }
}

/// Cuts `area` (logical px) out of an RGBA frame whose pixels are `scale` times the
/// logical size. Edges round outwards; `None` if nothing is left on the frame.
pub fn crop(rgba: &[u8], width: u32, height: u32, scale: f32, area: Rectangle) -> Option<Crop> {
    let [x0, y0, x1, y1] = crop_bounds(width, height, scale, area)?;
    if rgba.len() < width as usize * height as usize * 4 {
        return None;
    }
    let row = |y: u32| {
        let start = ((y * width + x0) * 4) as usize;
        &rgba[start..start + ((x1 - x0) * 4) as usize]
    };
    Some(Crop {
        width: x1 - x0,
        height: y1 - y0,
        rgba: (y0..y1).flat_map(row).copied().collect(),
    })
}

/// The frame pixels `crop` takes for `area`: `[x0, y0, x1, y1]`, ends exclusive.
pub fn crop_bounds(width: u32, height: u32, scale: f32, area: Rectangle) -> Option<[u32; 4]> {
    let edge =
        |v: f32, max: u32, round: fn(f32) -> f32| (round(v * scale).max(0.0) as u32).min(max);
    let (x0, y0) = (
        edge(area.x, width, f32::floor),
        edge(area.y, height, f32::floor),
    );
    let x1 = edge(area.x + area.width, width, f32::ceil);
    let y1 = edge(area.y + area.height, height, f32::ceil);
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

/// Writes an RGBA image as an 8-bit PNG.
pub fn write_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), String> {
    std::fs::write(path, encode_png(width, height, rgba)?).map_err(|e| e.to_string())
}

/// `crop` scaled down (bicubic, so text stays sharp) until its long edge is at
/// most `max_edge` px; as it is when it already fits or `max_edge` is 0.
pub fn fit_long_edge(crop: Crop, max_edge: u32) -> Crop {
    let long = crop.width.max(crop.height);
    if max_edge == 0 || long <= max_edge {
        return crop;
    }
    let shrink =
        |v: u32| ((u64::from(v) * u64::from(max_edge)).div_ceil(u64::from(long)) as u32).max(1);
    let (width, height) = (shrink(crop.width), shrink(crop.height));
    let Some(source) = image::RgbaImage::from_raw(crop.width, crop.height, crop.rgba) else {
        unreachable!("a Crop's buffer is width * height * 4 bytes");
    };
    let scaled = image::imageops::resize(&source, width, height, image::imageops::CatmullRom);
    Crop {
        width,
        height,
        rgba: scaled.into_raw(),
    }
}

/// `crop` as file bytes: PNG, or JPEG at quality 85 (no alpha: opaque screenshots).
pub fn encode(crop: &Crop, format: crate::config::Format) -> Result<Vec<u8>, String> {
    match format {
        crate::config::Format::Png => encode_png(crop.width, crop.height, &crop.rgba),
        crate::config::Format::Jpeg => {
            let (pixels, _) = crop.rgba.as_chunks::<4>();
            let rgb: Vec<u8> = pixels.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
                .encode(
                    &rgb,
                    crop.width,
                    crop.height,
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| e.to_string())?;
            Ok(out)
        }
    }
}

/// An RGBA image as 8-bit PNG bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Size};

    #[test]
    fn registry_keys_and_labels_are_unique() {
        for (i, a) in ACTIONS.iter().enumerate() {
            for b in &ACTIONS[i + 1..] {
                assert_ne!(a.id, b.id);
                assert_ne!(a.key, b.key, "{} and {} share a key", a.id, b.id);
                assert_ne!(a.label, b.label);
            }
            assert!(a.key.letter.is_ascii_lowercase(), "{}", a.id);
            let on = crate::labs::Labs::all_on();
            assert_eq!(
                for_key(&on, a.key.letter, a.key.ctrl).map(|f| f.id),
                Some(a.id)
            );
        }
    }

    #[test]
    fn a_hidden_feature_has_no_key() {
        let (off, on) = (crate::labs::Labs::default(), crate::labs::Labs::all_on());
        let e = edit::ACTION.key;
        assert!(
            for_key(&off, e.letter, e.ctrl).is_none(),
            "E with the editor off"
        );
        assert_eq!(for_key(&on, e.letter, e.ctrl).map(|a| a.id), Some("edit"));
        let l = lens::ACTION.key;
        assert!(for_key(&off, l.letter, l.ctrl).is_some());
    }

    #[test]
    fn enter_runs_the_configured_default_action() {
        use crate::config::{Config, DefaultAction};
        let mut cfg = Config::default();
        assert_eq!(default_for(&cfg).id, "lens", "Lens out of the box");
        for (choice, id) in [
            (DefaultAction::Ai, "ai"),
            (DefaultAction::Text, "text"),
            (DefaultAction::Copy, "copy"),
            (DefaultAction::Lens, "lens"),
        ] {
            cfg.general.default_action = choice;
            assert_eq!(default_for(&cfg).id, id);
        }
        // A saved choice survives the config file.
        let (back, _) = Config::parse("[general]\ndefault_action = \"text\"\n");
        assert_eq!(default_for(&back).id, "text");
    }

    #[test]
    fn every_icon_is_in_the_subset_font() {
        let script = include_str!("../../assets/lucide/subset.sh");
        for a in ACTIONS {
            let code = format!("U+{:04X}", a.icon as u32);
            assert!(script.contains(&code), "{}: add {code} to subset.sh", a.id);
        }
    }

    /// 4x3 frame, pixel (x, y) = [x, y, 0, 255].
    fn frame() -> Vec<u8> {
        (0..3u8)
            .flat_map(|y| (0..4u8).flat_map(move |x| [x, y, 0, 255]))
            .collect()
    }

    #[test]
    fn crop_is_in_physical_pixels() {
        // Scale 2: logical (0.5, 0.5) 1x1 = physical (1, 1) 2x2.
        let area = Rectangle::new(Point::new(0.5, 0.5), Size::new(1.0, 1.0));
        let c = crop(&frame(), 4, 3, 2.0, area).unwrap();
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(
            c.rgba,
            [1, 1, 0, 255, 2, 1, 0, 255, 1, 2, 0, 255, 2, 2, 0, 255]
        );

        // Scale 1.5, fractional edges round outwards: x 1.2..2.1 → 1..3, y 0..1.5 → 0..2.
        let area = Rectangle::new(Point::new(0.8, 0.0), Size::new(0.6, 1.0));
        let c = crop(&frame(), 4, 3, 1.5, area).unwrap();
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(&c.rgba[..4], [1, 0, 0, 255]);
    }

    fn solid(width: u32, height: u32) -> Crop {
        Crop {
            width,
            height,
            rgba: (0..width * height)
                .flat_map(|_| [200, 100, 50, 255])
                .collect(),
        }
    }

    #[test]
    fn export_scales_the_long_edge_down_only() {
        // 4K landscape -> 1568 wide, aspect kept (rounded up, never 0).
        let c = fit_long_edge(solid(3840, 2160), 1568);
        assert_eq!((c.width, c.height), (1568, 882));
        assert_eq!(c.rgba.len(), 1568 * 882 * 4);
        // Flat colour stays that colour (no ringing on a flat area).
        assert!(
            c.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [200, 100, 50, 255])
        );
        // Tall: the height is the long edge.
        let c = fit_long_edge(solid(500, 3000), 1568);
        assert_eq!((c.width, c.height), (262, 1568));
        // Already small, or 0 = original: untouched (never enlarged).
        let c = fit_long_edge(solid(800, 600), 1568);
        assert_eq!((c.width, c.height), (800, 600));
        let c = fit_long_edge(solid(3840, 2160), 0);
        assert_eq!((c.width, c.height), (3840, 2160));
        // A sliver keeps at least one pixel.
        let c = fit_long_edge(solid(5000, 1), 1000);
        assert_eq!((c.width, c.height), (1000, 1));
    }

    #[test]
    fn export_writes_png_or_jpeg() {
        use crate::config::Format;
        let c = solid(40, 30);
        let png = encode(&c, Format::Png).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let jpeg = encode(&c, Format::Jpeg).unwrap();
        assert_eq!(&jpeg[..3], [0xff, 0xd8, 0xff], "JPEG magic");
        let back = image::load_from_memory(&jpeg).unwrap().into_rgb8();
        assert_eq!(back.dimensions(), (40, 30));
        let [r, g, b] = back.get_pixel(20, 15).0;
        let near = |a: u8, b: u8| a.abs_diff(b) <= 4;
        assert!(near(r, 200) && near(g, 100) && near(b, 50), "{r} {g} {b}");
    }

    #[test]
    fn crop_clamps_to_the_frame() {
        let area = Rectangle::new(Point::new(-5.0, 1.0), Size::new(100.0, 100.0));
        let c = crop(&frame(), 4, 3, 1.0, area).unwrap();
        assert_eq!((c.width, c.height), (4, 2));
        assert_eq!(c.rgba.len(), 4 * 2 * 4);
        let off = Rectangle::new(Point::new(10.0, 10.0), Size::new(5.0, 5.0));
        assert!(crop(&frame(), 4, 3, 1.0, off).is_none());
    }
}
