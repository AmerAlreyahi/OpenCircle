//! IS-Net general-use (Apache-2.0, 1024 px, 179 MB) for backgrounds that
//! aren't flat. The helper process runs it (the app never loads ONNX
//! Runtime): the app sends the picture sized to the model's input, the
//! helper builds a session, runs it and drops it (no memory arena, so the
//! ~830 MB of a run go back at once); the reply is the model's mask, which the
//! app sizes back to the picture (a soft edge). Pre/post as rembg does it:
//! pixels / the largest value - 0.5, output min-max scaled.

use std::path::{Path, PathBuf};

use image::imageops::FilterType::Triangle;
use image::{GrayImage, RgbImage};
use ort::value::Tensor;

use crate::features::text_reading::{self, ModelFile, ORT, OcrError, ort_session};

/// The model's side.
pub(crate) const SIDE: usize = 1024;

/// OpenCircle-models release `cutout-1` (the same bytes as rembg's
/// v0.0.0 `isnet-general-use.onnx`; the DIS repository is Apache-2.0).
#[cfg(not(test))]
const URL: &str = "https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/cutout-1/isnet-general-use.onnx";
/// Tests never reach the network.
#[cfg(test)]
const URL: &str = "http://127.0.0.1:9/isnet-general-use.onnx";

pub(crate) const ISNET: ModelFile = ModelFile::plain(
    "isnet-general-use.onnx",
    178_648_008,
    "60920e99c45464f2ba57bee2ad08c919a52bbf852739e96947fbb4358c0d964a",
    URL,
);

/// What the cut needs on disk: ONNX Runtime (if OCR never fetched it) and the model.
pub(crate) fn files() -> Vec<(PathBuf, &'static ModelFile)> {
    let dir = text_reading::models_dir();
    vec![(dir.join(ORT.name), &ORT), (dir.join(ISNET.name), &ISNET)]
}

/// Bytes the cut would still download (0 = ready), by size.
pub(crate) fn missing_bytes() -> u64 {
    text_reading::missing_bytes(&files())
}

/// The whole download, present or not (the card's size).
pub(crate) fn full_size() -> u64 {
    files().iter().map(|(_, f)| f.size).sum()
}

/// The picture as the model's input: RGB, `SIDE` x `SIDE`.
pub(crate) fn pre(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let rgb = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let img = RgbImage::from_raw(w, h, rgb)?;
    Some(image::imageops::resize(&img, SIDE as u32, SIDE as u32, Triangle).into_raw())
}

/// The model's mask (`SIDE` x `SIDE`) sized to the picture: its alpha.
pub(crate) fn post(mask: Vec<u8>, w: u32, h: u32) -> Option<Vec<u8>> {
    let mask = GrayImage::from_raw(SIDE as u32, SIDE as u32, mask)?;
    Some(image::imageops::resize(&mask, w, h, Triangle).into_raw())
}

/// The model's input tensor data from `pre`'s bytes.
fn tensor(rgb: &[u8]) -> Vec<f32> {
    let top = rgb.iter().copied().max().unwrap_or(0).max(1) as f32;
    let plane = SIDE * SIDE;
    let mut out = vec![0.0f32; plane * 3];
    for (i, p) in rgb.as_chunks::<3>().0.iter().enumerate() {
        for c in 0..3 {
            out[c * plane + i] = p[c] as f32 / top - 0.5;
        }
    }
    out
}

/// The output scaled to 0..255 over its own min and max.
fn scale(pred: &[f32]) -> Vec<u8> {
    let (lo, hi) = pred
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    let span = (hi - lo).max(1e-6);
    pred.iter()
        .map(|&v| ((v - lo) / span * 255.0).round() as u8)
        .collect()
}

/// Helper side: runs the model on `rgb` (`pre`'s bytes) with a session that
/// lives only for this call.
pub(crate) fn run(dir: &Path, rgb: &[u8]) -> Result<Vec<u8>, OcrError> {
    if rgb.len() != SIDE * SIDE * 3 {
        return Err(OcrError::BadImage(
            "the cut-out input is the wrong size".into(),
        ));
    }
    let path = dir.join(ISNET.name);
    if !path.is_file() {
        return Err(OcrError::Missing("the cut-out model".into()));
    }
    crate::features::text_reading::init_ort(dir)?;
    let err = |e: ort::Error| OcrError::Engine(e.to_string());
    let mut session = ort_session(&path)?;
    let name = session
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .unwrap_or_else(|| "input".into());
    let input = Tensor::from_array(([1, 3, SIDE, SIDE], tensor(rgb))).map_err(err)?;
    let out = session.run(ort::inputs![name => input]).map_err(err)?;
    let (shape, data) = out[0].try_extract_tensor::<f32>().map_err(err)?;
    if data.len() != SIDE * SIDE || shape.last() != Some(&(SIDE as i64)) {
        return Err(OcrError::Engine(
            "the cut-out model gave an unexpected output".into(),
        ));
    }
    Ok(scale(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_and_post_keep_the_pictures_size() {
        let (w, h) = (50u32, 30u32);
        let rgba = vec![200u8; (w * h * 4) as usize];
        let rgb = pre(&rgba, w, h).unwrap();
        assert_eq!(rgb.len(), SIDE * SIDE * 3);
        let a = post(vec![255; SIDE * SIDE], w, h).unwrap();
        assert_eq!(a.len(), (w * h) as usize);
        assert!(a.iter().all(|&v| v == 255));
    }

    /// Divided by the largest value, centred on 0; the output over its own range.
    #[test]
    fn tensor_and_scale_follow_rembg() {
        let mut rgb = vec![0u8; SIDE * SIDE * 3];
        rgb[0] = 100;
        rgb[3] = 50;
        let t = tensor(&rgb);
        assert!((t[0] - 0.5).abs() < 1e-6 && t[1].abs() < 1e-6);
        assert!((t[SIDE * SIDE] + 0.5).abs() < 1e-6, "next channel plane");
        assert_eq!(scale(&[0.2, 0.4, 0.6]), vec![0, 127, 255]);
        assert_eq!(
            scale(&[0.3, 0.3]),
            vec![0, 0],
            "flat output: nothing kept, no NaN"
        );
    }

    /// The pin: the file is bigger than OCR's models and names a real sha.
    #[test]
    fn the_pin_is_a_whole_download() {
        assert_eq!(ISNET.sha256.len(), 64);
        assert!(full_size() > ISNET.size);
    }

    /// The real model (manual): `OC_ISNET=<isnet-general-use.onnx> OC_ORT_DIR=<dir with ORT>`.
    #[test]
    #[ignore]
    fn the_real_model_cuts_a_disc() {
        let (model, dir) = (
            std::env::var("OC_ISNET").unwrap(),
            std::env::var("OC_ORT_DIR").unwrap(),
        );
        let dir = PathBuf::from(dir);
        std::fs::copy(model, dir.join(ISNET.name)).unwrap();
        let mut rgb = vec![30u8; SIDE * SIDE * 3];
        for y in 0..SIDE {
            for x in 0..SIDE {
                let (dx, dy) = (x as i32 - 512, y as i32 - 512);
                if dx * dx + dy * dy < 250 * 250 {
                    rgb[(y * SIDE + x) * 3..][..3].copy_from_slice(&[230, 60, 40]);
                }
            }
        }
        let mask = run(&dir, &rgb).unwrap();
        assert!(mask[512 * SIDE + 512] > 200 && mask[10 * SIDE + 10] < 50);
    }
}
