//! Search in browser: the crop in Google Lens in the user's own browser (see
//! `crate::lens`; a text selection searches its text instead: `ui::App::run`).
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Action, Crop, Key, Panel, Run};
use crate::lens::LensPage;

pub const ACTION: Action = Action {
    id: "lens",
    label: "Search in browser",
    key: Key {
        ctrl: false,
        letter: 'l',
    },
    icon: '\u{e154}', // lucide "search"
    run: Run::Panel(Panel::Lens),
};

/// The browser reads the page a while after we launch it (a cold browser start
/// can take seconds): never delete a page younger than this.
const MIN_LIFE: Duration = Duration::from_secs(10);

/// What the user reads when the selection is one flat colour (nothing is sent).
pub const BLANK: &str =
    "Nothing was sent: this area is one plain colour. Select an area with a picture or text in it.";

/// `crop` as the PNG Lens gets, every pixel opaque (a screen has no
/// see-through pixels, and a zero alpha from any capture path would reach Lens
/// as blank white). `Err(BLANK)` for a crop of one flat colour: never sent.
pub fn lens_png(crop: &Crop) -> Result<Vec<u8>, String> {
    let mut rgba = crop.rgba.clone();
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    pixels.iter_mut().for_each(|p| p[3] = 255);
    if pixels.windows(2).all(|w| w[0] == w[1]) {
        return Err(BLANK.into());
    }
    super::encode_png(crop.width, crop.height, &rgba)
}

/// Writes the Lens page for `crop` and opens it in the default browser.
/// Blocking (PNG encode, file write, process spawn): call off the UI thread.
pub fn open(crop: &Crop) -> Result<Held, String> {
    let png = lens_png(crop)?;
    let page = crate::lens::open(&png, crop.width, crop.height).map_err(|e| {
        eprintln!("[lens] browser not opened: {e}");
        "Couldn't open your browser. Check that a default browser is set, then try again."
            .to_string()
    })?;
    crate::search_history::keep("lens", "", "", Some(crop));
    Ok(Held {
        _page: Arc::new(page),
        opened: Instant::now(),
    })
}

/// An open Lens page; the file goes when the last clone is dropped.
#[derive(Debug, Clone)]
pub struct Held {
    /// Only held: its Drop deletes the file.
    _page: Arc<LensPage>,
    opened: Instant,
}

impl Held {
    /// A held page with no file, for the overlay's tests.
    #[cfg(test)]
    pub fn for_test() -> Held {
        Held {
            _page: Arc::new(crate::lens::LensPage::none()),
            opened: Instant::now(),
        }
    }

    /// The overlay is done with the page: delete it now, or once it is `MIN_LIFE` old.
    pub fn release(self) {
        let Some(wait) = linger(self.opened.elapsed()) else {
            return; // dropped here
        };
        // A failed spawn drops the closure, and the page with it: deleted early
        // rather than leaked (the next start's sweep would get it anyway).
        let _ = std::thread::Builder::new()
            .name("lens page".into())
            .spawn(move || {
                std::thread::sleep(wait);
                drop(self);
            });
    }
}

/// How much longer a page `age` old must stay; `None` = delete it now.
fn linger(age: Duration) -> Option<Duration> {
    Some(MIN_LIFE.saturating_sub(age)).filter(|wait| !wait.is_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens_gets_exactly_the_selected_pixels_all_opaque() {
        // A 16 x 12 frame at scale 2 (8 x 6 logical); every pixel says where it
        // is, and some have alpha 0 (what a bad capture could give).
        let (fw, fh) = (16u32, 12u32);
        let px = |x: u32, y: u32| {
            [
                x as u8 * 10,
                y as u8 * 20,
                7,
                if x.is_multiple_of(3) { 0 } else { 200 },
            ]
        };
        let frame: Vec<u8> = (0..fh)
            .flat_map(|y| (0..fw).flat_map(move |x| px(x, y)))
            .collect();
        let area = iced::Rectangle::new(iced::Point::new(2.0, 1.0), iced::Size::new(3.0, 2.0));
        let crop = super::super::crop(&frame, fw, fh, 2.0, area).unwrap();
        assert_eq!((crop.width, crop.height), (6, 4));
        // What the browser's page sends: the PNG inside the auto-submit form.
        let png = lens_png(&crop).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut sent = vec![0; dec.output_buffer_size().unwrap()];
        let info = dec.next_frame(&mut sent).unwrap();
        assert_eq!(
            (info.width, info.height, info.color_type),
            (6, 4, png::ColorType::Rgba)
        );
        for (i, p) in sent.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (4 + i as u32 % 6, 2 + i as u32 / 6);
            assert_eq!(p[..3], px(x, y)[..3], "pixel ({x}, {y})");
            assert_eq!(p[3], 255, "never see-through");
        }
    }

    #[test]
    fn a_flat_crop_is_refused_and_any_detail_goes() {
        let flat = |rgba: [u8; 4]| Crop {
            width: 30,
            height: 20,
            rgba: rgba.repeat(600),
        };
        for colour in [
            [255; 4],
            [255, 255, 255, 0],
            [0, 0, 0, 255],
            [40, 90, 200, 255],
        ] {
            assert_eq!(lens_png(&flat(colour)), Err(BLANK.into()), "{colour:?}");
            assert_eq!(
                open(&flat(colour)).map(drop),
                Err(BLANK.into()),
                "never handed off"
            );
        }
        // One pixel of something else is a picture.
        let mut dot = flat([255; 4]);
        dot.rgba[4 * 77..4 * 78].copy_from_slice(&[0, 0, 0, 255]);
        assert!(lens_png(&dot).is_ok());
    }

    #[test]
    fn a_page_lives_at_least_min_life() {
        let s = Duration::from_secs;
        assert_eq!(linger(s(0)), Some(MIN_LIFE));
        assert_eq!(linger(s(3)), Some(MIN_LIFE - s(3)));
        assert_eq!(
            linger(MIN_LIFE - Duration::from_millis(1)),
            Some(Duration::from_millis(1))
        );
        assert_eq!(linger(MIN_LIFE), None);
        assert_eq!(linger(s(600)), None);
        assert!(MIN_LIFE >= s(10), "the browser needs time to read the page");
    }
}
