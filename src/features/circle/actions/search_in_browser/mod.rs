//! Search in browser: the crop in Google Lens in the user's own browser (see
//! `page`; a text selection searches its text instead: `ui::App::run`).
pub mod page;

use super::{Action, Crop, Key, Run};
use crate::shared::app_language::S;

pub const ACTION: Action = Action {
    id: "lens",
    label: S::act_search,
    key: Key {
        ctrl: false,
        letter: 'l',
    },
    icon: '\u{e154}', // lucide "search"
    run: Run::SearchInBrowser,
};

/// What the user reads when the selection is one flat colour (nothing is sent).
pub const BLANK: crate::shared::app_language::Said =
    crate::shared::app_language::Said(S::pill_blank);

/// `crop` as the PNG Lens gets, every pixel opaque (a screen has no
/// see-through pixels, and a zero alpha from any capture path would reach Lens
/// as blank white). `Err(BLANK)` for a crop of one flat colour: never sent.
pub fn search_png(crop: &Crop) -> Result<Vec<u8>, String> {
    let mut rgba = crop.rgba.clone();
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    pixels.iter_mut().for_each(|p| p[3] = 255);
    if pixels.windows(2).all(|w| w[0] == w[1]) {
        return Err(BLANK.to_string());
    }
    super::encode_png(crop.width, crop.height, &rgba)
}

/// Serves the Lens page for `crop` once and opens it in the default browser.
/// Blocking (PNG encode, process spawn): call off the UI thread.
pub fn open(crop: &Crop) -> Result<(), String> {
    open_with(crop, None)
}

/// [`open`] with the user's `question` added (Ask AI, browser provider): Google
/// answers about the picture. The same crop and page; only the question is new.
pub fn open_asking(crop: &Crop, question: &str) -> Result<(), String> {
    open_with(crop, Some(question))
}

fn open_with(crop: &Crop, question: Option<&str>) -> Result<(), String> {
    let png = search_png(crop)?;
    page::open(&png, crop.width, crop.height, question).map_err(|e| {
        crate::log!("[lens] browser not opened: {e}");
        crate::t!(pill_no_browser).to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_gets_exactly_the_selected_pixels_all_opaque() {
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
        let png = search_png(&crop).unwrap();
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
            assert_eq!(
                search_png(&flat(colour)),
                Err(BLANK.to_string()),
                "{colour:?}"
            );
            assert_eq!(
                open(&flat(colour)).map(drop),
                Err(BLANK.to_string()),
                "never handed off"
            );
        }
        // One pixel of something else is a picture.
        let mut dot = flat([255; 4]);
        dot.rgba[4 * 77..4 * 78].copy_from_slice(&[0, 0, 0, 255]);
        assert!(search_png(&dot).is_ok());
    }
}
