//! What the selection holds besides pictures: a QR code (its text becomes a chip on
//! the action bar) and whether the capture came out black (a protected window).
//! Blocking, so call [`scan`] off the UI thread. Nothing here opens anything.
use super::Crop;

/// Pixels this dark (every channel) count as black.
const BLACK: u8 = 3;
/// A capture with at least this share of black pixels is "protected".
// ponytail: a genuinely pure-black selection (OLED theme, empty terminal) also trips
// it; the pill only says so, nothing is blocked. Tighten here if users report it.
const PROTECTED_SHARE: f64 = 0.98;

/// What a selection holds.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    /// The text of the first QR code found.
    pub qr: Option<String>,
    /// Almost all black: DRM / protected windows are captured as black frames.
    pub protected: bool,
}

/// What the QR chip does when clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Copy,
    Open,
}

pub fn scan(crop: &Crop) -> Scan {
    let protected = is_protected(crop);
    Scan {
        // All black: nothing to decode, and the scan below would be wasted time.
        qr: if protected { None } else { decode(crop) },
        protected,
    }
}

/// Whether (almost) every pixel of `crop` is black.
pub fn is_protected(crop: &Crop) -> bool {
    let pixels = crop.rgba.len() / 4;
    let black = crop
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[..3].iter().all(|&c| c <= BLACK))
        .count();
    pixels > 0 && black as f64 >= pixels as f64 * PROTECTED_SHARE
}

/// The text of the first QR code in `crop` (dark code on a light ground).
// ponytail: no light-on-dark codes and no 1D barcodes; add a second pass on inverted
// luma if users need it (it would double the cost of every code-free selection).
pub fn decode(crop: &Crop) -> Option<String> {
    let (w, h) = (crop.width as usize, crop.height as usize);
    if w < 21 || h < 21 || crop.rgba.len() < w * h * 4 {
        return None; // the smallest QR code is 21 modules
    }
    let mut image = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| {
        let p = &crop.rgba[(y * w + x) * 4..][..3];
        ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8
    });
    image
        .detect_grids()
        .iter()
        .find_map(|grid| grid.decode().ok())
        .map(|(_, text)| text)
        .filter(|text| !text.trim().is_empty())
}

/// `text` as a link to offer "Open" for: one http(s) URL and nothing else.
pub fn web_url(text: &str) -> Option<&str> {
    let text = text.trim();
    crate::lens::is_web_url(text).then_some(text)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A white crop with `text` as a QR code: `module` px per module, a quiet zone
    /// of 4 modules, `pad` extra px of background around it.
    pub(crate) fn qr_crop(text: &str, module: usize, pad: usize) -> Crop {
        let code = qrcode::QrCode::new(text).unwrap();
        let n = code.width();
        let dark = code.to_colors();
        let side = (n + 8) * module + 2 * pad;
        let mut rgba = vec![255u8; side * side * 4];
        for y in 0..n * module {
            for x in 0..n * module {
                if dark[(y / module) * n + x / module] == qrcode::Color::Dark {
                    let at = ((y + (4 * module + pad)) * side + x + 4 * module + pad) * 4;
                    rgba[at..at + 3].fill(0);
                }
            }
        }
        Crop {
            width: side as u32,
            height: side as u32,
            rgba,
        }
    }

    fn solid(colour: [u8; 4], width: u32, height: u32) -> Crop {
        Crop {
            width,
            height,
            rgba: colour.repeat((width * height) as usize),
        }
    }

    #[test]
    fn decodes_a_generated_qr_code() {
        for (text, module, pad) in [
            ("https://example.com/a?b=1", 4, 0),
            ("hello", 3, 10),
            ("WIFI:S:cafe;T:WPA;P:secret;;", 6, 37),
            ("Arabic: \u{645}\u{631}\u{62d}\u{628}\u{627}", 5, 0),
        ] {
            let crop = qr_crop(text, module, pad);
            assert_eq!(decode(&crop).as_deref(), Some(text), "{text:?}");
            assert_eq!(scan(&crop).qr.as_deref(), Some(text));
        }
    }

    #[test]
    fn finds_nothing_in_pictures_without_a_code() {
        assert_eq!(decode(&solid([255, 255, 255, 255], 200, 200)), None);
        let noise: Vec<u8> = (0..120 * 120 * 4u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let crop = Crop {
            width: 120,
            height: 120,
            rgba: noise,
        };
        assert_eq!(decode(&crop), None);
        assert_eq!(
            decode(&solid([255; 4], 10, 10)),
            None,
            "too small for a code"
        );
        // A short pixel buffer never panics.
        let short = Crop {
            width: 100,
            height: 100,
            rgba: vec![0; 40],
        };
        assert_eq!(decode(&short), None);
    }

    #[test]
    fn black_captures_are_protected() {
        assert!(is_protected(&solid([0, 0, 0, 255], 64, 64)));
        assert!(is_protected(&solid([2, 3, 1, 255], 64, 64)), "near black");
        assert!(!is_protected(&solid([0, 0, 0, 255], 0, 0)));
        assert!(
            !is_protected(&solid([30, 30, 30, 255], 64, 64)),
            "dark grey"
        );
        assert!(!is_protected(&solid([255, 255, 255, 255], 64, 64)));
        // The line is 2 % of the pixels not black.
        let mut crop = solid([0, 0, 0, 255], 100, 100);
        for p in crop.rgba.as_chunks_mut::<4>().0.iter_mut().take(200) {
            p[..3].fill(255);
        }
        assert!(is_protected(&crop), "2.00 % lit");
        crop.rgba[200 * 4..200 * 4 + 3].fill(255);
        assert!(!is_protected(&crop), "2.01 % lit");
        // A protected capture is not decoded; a code on white is not protected.
        assert!(scan(&solid([0, 0, 0, 255], 64, 64)).protected);
        let code = scan(&qr_crop("hi", 4, 0));
        assert!(!code.protected && code.qr.is_some());
    }

    #[test]
    fn only_plain_http_urls_get_an_open_button() {
        assert_eq!(web_url(" https://a.io/x \n"), Some("https://a.io/x"));
        assert_eq!(web_url("http://a.io"), Some("http://a.io"));
        for no in [
            "WIFI:S:x;;",
            "mailto:a@b.com",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://a.io/x y",
            "see https://a.io",
            "https://a.io\nhttps://b.io",
            "",
        ] {
            assert_eq!(web_url(no), None, "{no:?}");
        }
    }
}
