//! What the selection holds besides pictures: a QR code (its text becomes a chip on
//! the action bar) and whether the capture came out black (a protected window).
//! Blocking, so call [`scan`] off the UI thread. Nothing here opens anything.
use super::Crop;

/// Pixels this dark (every channel) count as black.
const BLACK: u8 = 3;
/// A capture with at least this share of black pixels is "protected".
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

/// `scan_around` with a protected window over the whole crop and no hole.
#[cfg(test)]
pub fn scan(crop: &Crop) -> Scan {
    scan_around(crop, None, &[[0, 0, crop.width, crop.height]])
}

/// `is_protected_around` with a window over the whole crop.
#[cfg(test)]
fn is_protected(crop: &Crop) -> bool {
    around(crop, None)
}

#[cfg(test)]
fn around(crop: &Crop, hole: Option<[u32; 4]>) -> bool {
    is_protected_around(crop, hole, &[[0, 0, crop.width, crop.height]])
}

/// What a selection holds. `windows`: the windows the OS keeps out of captures
/// (display affinity), as crop px rects (x0 y0 x1 y1); a black crop is only
/// "protected" where one of them lies. `hole`: not judged, what our own pinned
/// AI chat covered (its window is kept out of captures, so the OS may leave that
/// part black); a crop lying wholly in the hole is judged as usual.
pub fn scan_around(crop: &Crop, hole: Option<[u32; 4]>, windows: &[[u32; 4]]) -> Scan {
    let protected = is_protected_around(crop, hole, windows);
    Scan {
        // All black: nothing to decode, and the scan below would be wasted time.
        qr: if protected { None } else { decode(crop) },
        protected,
    }
}

/// Window rects (physical desktop px: x, y, w, h) in the crop px of a crop whose
/// top-left is at `origin` (desktop px): cut at the crop's top-left, empty ones
/// dropped.
pub fn crop_rects(origin: (i32, i32), rects: &[(i32, i32, u32, u32)]) -> Vec<[u32; 4]> {
    let to = |v: i64, o: i32| (v - i64::from(o)).clamp(0, i64::from(u32::MAX)) as u32;
    rects
        .iter()
        .map(|&(x, y, w, h)| {
            let (x, y) = (i64::from(x), i64::from(y));
            [
                to(x, origin.0),
                to(y, origin.1),
                to(x + i64::from(w), origin.0),
                to(y + i64::from(h), origin.1),
            ]
        })
        .filter(|[x0, y0, x1, y1]| x1 > x0 && y1 > y0)
        .collect()
}

/// Whether (almost) every pixel of `crop` inside `windows` is black. Pixels
/// outside every window are never judged: a black wallpaper is not protected.
fn is_protected_around(crop: &Crop, hole: Option<[u32; 4]>, windows: &[[u32; 4]]) -> bool {
    let w = crop.width.max(1) as usize;
    let within = |r: &[u32; 4], i: usize| {
        let (x, y) = ((i % w) as u32, (i / w) as u32);
        (r[0]..r[2]).contains(&x) && (r[1]..r[3]).contains(&y)
    };
    let in_hole = |i: usize| hole.as_ref().is_some_and(|r| within(r, i));
    let pixels = crop.rgba.as_chunks::<4>().0;
    let count = |judge: &dyn Fn(usize) -> bool| {
        let (mut all, mut black) = (0usize, 0usize);
        for (_, p) in pixels.iter().enumerate().filter(|(i, _)| judge(*i)) {
            all += 1;
            black += usize::from(p[..3].iter().all(|&c| c <= BLACK));
        }
        (all, black)
    };
    let (all, black) = match count(&|i| !in_hole(i) && windows.iter().any(|r| within(r, i))) {
        // only under the chat: as usual
        (0, _) if (0..pixels.len()).all(in_hole) => count(&|_| true),
        counted => counted,
    };
    all > 0 && black as f64 >= all as f64 * PROTECTED_SHARE
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
    crate::sys::open::web_host(text).is_some().then_some(text)
}

/// What the QR chip shows for the code's `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    /// The code holds text-direction controls: they can make it read as something
    /// else (a link spelled backwards), so the chip warns.
    pub warn: bool,
}

/// `text` safe to show in `max` characters. Control, text-direction and
/// zero-width characters are dropped and line breaks shown as spaces, so the
/// chip can't be made to read as something else; when what is shown is not
/// exactly what Copy copies, it ends with "… (copies N chars)".
pub fn shown(text: &str, max: usize) -> Shown {
    let text = text.trim();
    let bidi = |c: char| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}');
    let hidden = |c: char| {
        bidi(c)
            || c.is_control()
            || matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{00AD}' | '\u{180E}')
    };
    // Whitespace first: '\n' is also a control, but it separates words.
    let visible: String = text
        .chars()
        .filter(|&c| c.is_whitespace() || !hidden(c))
        .collect();
    let flat = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = super::entities::shorten(&flat, max);
    if out != text {
        let head = out.strip_suffix('…').unwrap_or(&out).trim_end();
        out = format!("{head}… (copies {} chars)", grouped(text.chars().count()));
    }
    Shown {
        text: out,
        warn: text.chars().any(bidi),
    }
}

/// `n` with thousands separators: 1240 -> "1,240".
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    out
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
    fn the_pinned_chats_hole_is_not_judged() {
        // black everywhere but one grey column: protected, unless the black is
        // where our chat was
        let mut rgba = [0, 0, 0, 255].repeat(64 * 64);
        for y in 0..64 {
            rgba[(y * 64) * 4..][..3].fill(90);
        }
        let crop = Crop {
            width: 64,
            height: 64,
            rgba,
        };
        let hole = Some([1, 0, 64, 64]);
        assert!(around(&crop, None), "98 % black");
        assert!(!around(&crop, hole), "the black is only our chat");
        assert!(!scan_around(&crop, hole, &[[0, 0, 64, 64]]).protected);
        let black = solid([0, 0, 0, 255], 64, 64);
        assert!(
            around(&black, Some([32, 0, 64, 64])),
            "a black app beside it"
        );
        assert!(
            around(&black, Some([0, 0, 64, 64])),
            "wholly under it: as usual"
        );
    }

    #[test]
    fn black_is_protected_only_where_a_protected_window_is() {
        let black = solid([0, 0, 0, 255], 64, 64);
        // a black wallpaper: no window under it
        assert!(!scan_around(&black, None, &[]).protected);
        // a window covering a corner of it
        assert!(scan_around(&black, None, &[[0, 0, 10, 10]]).protected);
        // a protected window beside the black: the black there is not judged
        let mut rgba = [0, 0, 0, 255].repeat(64 * 64);
        for y in 0..64 {
            rgba[(y * 64 + 32) * 4..][..(32 * 4)].fill(90);
        }
        let half = Crop {
            width: 64,
            height: 64,
            rgba,
        };
        assert!(!scan_around(&half, None, &[[32, 0, 64, 64]]).protected);
        assert!(scan_around(&half, None, &[[0, 0, 32, 64]]).protected);
    }

    #[test]
    fn window_rects_land_in_crop_px() {
        // a crop whose top-left is at desktop (100, 50)
        let r = crop_rects(
            (100, 50),
            &[(90, 60, 30, 20), (0, 0, 10, 10), (150, 70, 40, 40)],
        );
        assert_eq!(r, vec![[0, 10, 20, 30], [50, 20, 90, 60]]);
        // a window left of a negative-origin crop
        assert!(crop_rects((-1920, 0), &[(-2000, 0, 50, 50)]).is_empty());
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
            "https://google.com@evil.com/",
            "https://good.com\\@evil.com",
            "https://good.com%2f@evil.com",
            "https://\u{202e}moc.elgoog",
            "",
        ] {
            assert_eq!(web_url(no), None, "{no:?}");
        }
    }

    #[test]
    fn the_chip_never_shows_hidden_or_reordering_characters() {
        // Short and plain: shown as it is.
        assert_eq!(
            shown(" hello ", 36),
            Shown {
                text: "hello".into(),
                warn: false
            }
        );
        // A right-to-left override would show "https://evil.com/moc.elgoog" as
        // ending in "google.com": dropped, and the chip warns.
        let s = shown("https://evil.com/\u{202e}moc.elgoog", 36);
        assert_eq!(s.text, "https://evil.com/moc.elgoog… (copies 28 chars)");
        assert!(s.warn);
        for c in [
            '\u{2066}', '\u{2069}', '\u{200e}', '\u{200f}', '\u{61c}', '\u{202a}',
        ] {
            let s = shown(&format!("a{c}b"), 36);
            assert!(s.warn && !s.text.contains(c), "{c:?}");
        }
        // Zero-width and control characters are dropped, no warning.
        let s = shown("pay\u{200b}pal\u{feff}.com\u{7}", 36);
        assert_eq!(s.text, "paypal.com… (copies 13 chars)");
        assert!(!s.warn);
        // Line breaks: one line on the chip, and it says there is more to copy.
        let s = shown("line one\nline two\r\nthree", 36);
        assert_eq!(s.text, "line one line two three… (copies 24 chars)");
        // Long: cut, with the full length.
        let long = "x".repeat(1240);
        assert_eq!(
            shown(&long, 36).text,
            format!("{}… (copies 1,240 chars)", "x".repeat(36))
        );
        // Arabic is text, not a control.
        let s = shown("\u{645}\u{631}\u{62d}\u{628}\u{627}", 36);
        assert_eq!(s.text, "\u{645}\u{631}\u{62d}\u{628}\u{627}");
        assert!(!s.warn);
        assert_eq!(grouped(7), "7");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
