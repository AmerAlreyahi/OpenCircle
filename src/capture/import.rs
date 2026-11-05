//! Bounded, read-only loading of user-supplied PNGs. No capture or history writes.
use std::io::{Cursor, Read};
use std::path::Path;

use crate::actions::Crop;

const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_RGBA: usize = 128 * 1024 * 1024;
const MAX_EDGE: u32 = 32768;

pub(crate) fn open(path: &Path) -> Result<Crop, String> {
    // Check before opening, then check the opened handle too. O_NONBLOCK prevents
    // a FIFO substituted between those checks from hanging the UI/CLI forever.
    let check = |meta: std::fs::Metadata| {
        if !meta.is_file() || meta.len() > MAX_FILE {
            Err("PNG must be a regular file of at most 64 MiB".to_string())
        } else {
            Ok(())
        }
    };
    check(std::fs::metadata(path).map_err(|e| e.to_string())?)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    check(file.metadata().map_err(|e| e.to_string())?)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("PNG exceeds 64 MiB".into());
    }
    decode(&bytes)
}

fn rgba_size(width: u32, height: u32) -> Result<usize, String> {
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
        return Err("PNG dimensions exceed import limits".into());
    }
    bytes
        .filter(|&n| n <= MAX_RGBA)
        .ok_or_else(|| "PNG exceeds 32 megapixels".into())
}

fn decode(bytes: &[u8]) -> Result<Crop, String> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 32 * 1024 * 1024,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (width, height) = (reader.info().width, reader.info().height);
    let rgba_len = rgba_size(width, height)?;
    if reader.info().animation_control.is_some() {
        return Err("animated PNGs are not supported".into());
    }
    let len = reader
        .output_buffer_size()
        .filter(|&n| n <= MAX_RGBA)
        .ok_or("PNG too large")?;
    let mut buf = vec![0; len];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    // Read through IEND too: a truncated or corrupt trailer must fail before
    // replacing an existing editor, just like a corrupt image-data chunk.
    reader.finish().map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.into_iter().flat_map(|v| [v, v, v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err("PNG palette was not expanded".into()),
    };
    if rgba.len() != rgba_len {
        return Err("invalid PNG pixel count".into());
    }
    Ok(Crop {
        width,
        height,
        rgba,
    })
}

/// Search imports have one opaque representation for display, OCR and uploads.
/// Composite straight-alpha RGB over black, rounded to the nearest byte. The
/// ordinary annotation loader deliberately preserves every original RGBA byte.
pub(crate) fn search_pixels(crop: &mut Crop) {
    for pixel in crop.rgba.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
        pixel[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(pixels)
                .unwrap();
        }
        bytes
    }

    #[test]
    fn imports_rgb_rgba_and_grayscale_without_changing_pixels() {
        for (color, bytes, expected) in [
            (
                png::ColorType::Rgb,
                vec![1, 2, 3, 4, 5, 6],
                vec![1, 2, 3, 255, 4, 5, 6, 255],
            ),
            (
                png::ColorType::Rgba,
                vec![1, 2, 3, 4, 5, 6, 7, 8],
                vec![1, 2, 3, 4, 5, 6, 7, 8],
            ),
            (
                png::ColorType::Grayscale,
                vec![9, 10],
                vec![9, 9, 9, 255, 10, 10, 10, 255],
            ),
            (
                png::ColorType::GrayscaleAlpha,
                vec![9, 2, 10, 3],
                vec![9, 9, 9, 2, 10, 10, 10, 3],
            ),
        ] {
            let crop = decode(&png(color, &bytes)).unwrap();
            assert_eq!((crop.width, crop.height), (2, 1));
            assert_eq!(crop.rgba, expected);
        }
    }

    #[test]
    fn search_erases_different_hidden_rgb_under_fully_transparent_pixels() {
        let bytes = png(png::ColorType::Rgba, &[255, 0, 30, 0, 0, 240, 70, 0]);
        let mut crop = decode(&bytes).unwrap();
        search_pixels(&mut crop);
        assert_eq!(crop.rgba, [0, 0, 0, 255].repeat(2));
        assert_eq!(
            crate::actions::search_in_browser::search_png(&crop),
            Err(crate::actions::search_in_browser::BLANK.into())
        );
        assert_eq!(decode(&bytes).unwrap().rgba, [255, 0, 30, 0, 0, 240, 70, 0]);
    }

    #[test]
    fn rejects_corrupt_truncated_and_oversized_pngs() {
        let good = png(png::ColorType::Rgba, &[1; 8]);
        assert!(decode(b"not a PNG").is_err());
        assert!(decode(&good[..good.len() - 12]).is_err(), "missing IEND");
        let mut bad = good.clone();
        bad[29] ^= 1; // IHDR CRC
        assert!(decode(&bad).is_err());
        let mut oversized = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut oversized, 8192, 8192);
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header().unwrap();
            // Header plus an empty data chunk: rejection must happen before
            // any output allocation or attempt to inflate these image bytes.
            writer.write_chunk(png::chunk::IDAT, &[]).unwrap();
        }
        assert!(matches!(decode(&oversized), Err(e) if e.contains("32 megapixels")));
        assert!(rgba_size(0, 1).is_err());
        assert!(rgba_size(MAX_EDGE + 1, 1).is_err());
        assert!(rgba_size(8192, 8192).is_err());
        assert!(rgba_size(u32::MAX, u32::MAX).is_err());
        assert_eq!(rgba_size(7680, 4320).unwrap(), 7680 * 4320 * 4);
    }

    #[test]
    fn refuses_nonregular_and_large_files_and_preserves_source() {
        let dir = std::env::temp_dir().join(format!("oc-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(open(&dir).is_err());
        let path = dir.join("source.png");
        let bytes = png(png::ColorType::Rgba, &[1; 8]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(open(&path).unwrap().rgba, vec![1; 8]);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let large = dir.join("large.png");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert!(open(&large).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let fifo = dir.join("pipe.png");
            let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            assert!(open(&fifo).is_err(), "FIFO must fail without a writer");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
