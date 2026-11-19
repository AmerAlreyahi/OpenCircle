//! Cut out: the picture's background made transparent. A flat border is cut
//! at once, with no model (`fill`); anything else goes through IS-Net in the
//! helper (`model`), downloaded only after the user confirms it. The result
//! is an alpha mask the Studio applies to its picture as one document edit.

mod fill;
pub(crate) mod model;

pub(crate) use fill::quick;
pub(crate) use model::full_size;

use crate::features::text_reading::{self, OcrError};

/// Blocking: fetches what the cut needs (`model::files`; verified files are
/// kept, a partial one continues), `progress` getting each whole percent.
pub(crate) fn download(mut progress: impl FnMut(f32)) -> Result<(), OcrError> {
    let mut shown = None;
    text_reading::ensure(&model::files(), |done, total| {
        let percent = done * 100 / total.max(1);
        // a retry's wait redraws the card too ("Retrying...")
        let now = (percent, text_reading::retrying());
        if shown != Some(now) {
            shown = Some(now);
            progress(percent as f32 / 100.0);
        }
    })
}

/// `rgba` (`w` x `h`) with `alpha` (one byte a pixel) multiplied into its
/// own alpha: kept pixels stay, the background goes transparent.
pub(crate) fn apply(rgba: &mut [u8], alpha: &[u8]) {
    for (p, &a) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
        p[3] = (p[3] as u32 * a as u32 / 255) as u8;
    }
}

/// Blocking: the model's alpha for `rgba` through the helper. The model must
/// be on disk (`model::missing_bytes() == 0`).
pub(crate) fn by_model(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, OcrError> {
    let input = model::pre(rgba, w, h).ok_or_else(|| OcrError::BadImage(format!("{w}x{h}")))?;
    let mask = text_reading::cut_out_in_child(&input)?;
    model::post(mask, w, h)
        .ok_or_else(|| OcrError::Engine("the cut-out mask is the wrong size".into()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn apply_multiplies_alpha() {
        let mut p = vec![10, 20, 30, 255, 10, 20, 30, 200, 1, 2, 3, 255];
        super::apply(&mut p, &[255, 128, 0]);
        assert_eq!([p[3], p[7], p[11]], [255, 100, 0]);
        assert_eq!(&p[..3], &[10, 20, 30], "colour untouched");
    }
}
