//! Snip Studio's Export (v0.4.2, STUDIO_V2_SPEC §8): the picture as PNG / JPG /
//! WebP / PDF, and the Steps as a Markdown page, an HTML page or a folder of
//! images. Only bytes and files here: the dialogs and pills are the host's.
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::shared::picture::{Crop, encode_png, fit_long_edge};

/// The picture's formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpg,
    /// Lossless (the image crate's WebP encoder has no lossy mode).
    Webp,
    /// One page the picture's size, the picture lossless in it.
    Pdf,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Png, Format::Jpg, Format::Webp, Format::Pdf];

    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Webp => "webp",
            Format::Pdf => "pdf",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpg => "JPG",
            Format::Webp => "WebP",
            Format::Pdf => "PDF",
        }
    }

    /// Its quality can be picked (JPG only: the others are lossless).
    pub fn has_quality(self) -> bool {
        self == Format::Jpg
    }
}

/// The long edges to pick from; 0 = the picture's own size.
pub const SIZES: [u32; 4] = [0, 2048, 1568, 1024];

/// What the picture's export writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    pub format: Format,
    /// JPG quality, 1..=100.
    pub quality: u8,
    /// The long edge at most (`SIZES`; 0 = original).
    pub max_edge: u32,
}

impl Default for Picture {
    fn default() -> Self {
        Picture {
            format: Format::Png,
            quality: 85,
            max_edge: 0,
        }
    }
}

/// WebP's size limit, per edge (the encoder's format limit).
pub const WEBP_MAX: u32 = 16384;
/// A PDF page's limit: 14400 pt at 0.75 pt a pixel.
const PDF_PAGE_PX: u32 = 19200;
/// Rows a PDF page holds: a tall picture goes on several pages.
const PDF_ROWS: u32 = 16384;

/// Why a picture can't be written as asked (the host says it in words).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Over `WEBP_MAX` px on an edge.
    WebpTooBig,
    /// Wider than a PDF page.
    PdfTooWide,
}

/// What stops a `width` x `height` picture being exported as `how` (after its
/// size choice), before anything is asked or encoded.
pub fn problem(width: u32, height: u32, how: Picture) -> Option<Problem> {
    let (w, h) = crate::shared::picture::fitted_size(width, height, how.max_edge);
    match how.format {
        Format::Webp if w.max(h) > WEBP_MAX => Some(Problem::WebpTooBig),
        Format::Pdf if w > PDF_PAGE_PX => Some(Problem::PdfTooWide),
        _ => None,
    }
}

/// `path` with the format's extension when the typed name has none.
pub fn with_ext(path: PathBuf, format: Format) -> PathBuf {
    match path.extension() {
        Some(_) => path,
        None => path.with_extension(format.ext()),
    }
}

/// `crop` encoded as `how` asks (slow for a big picture: off the UI thread).
pub fn encode(crop: Crop, how: Picture) -> Result<Vec<u8>, String> {
    if let Some(p) = problem(crop.width, crop.height, how) {
        return Err(format!("{p:?}"));
    }
    let crop = fit_long_edge(crop, how.max_edge);
    match how.format {
        Format::Png => encode_png(crop.width, crop.height, &crop.rgba),
        Format::Jpg => crate::shared::picture::encode_jpeg(&crop, how.quality.clamp(1, 100)),
        Format::Webp => {
            let mut out = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut out)
                .encode(
                    &crop.rgba,
                    crop.width,
                    crop.height,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
            Ok(out)
        }
        Format::Pdf => pdf(&crop),
    }
}

/// A PDF: the picture (RGB on white, deflated) filling pages as wide as it is
/// at 96 px to the inch, so it prints as big as it showed on a screen; a tall
/// picture goes on pages of at most `PDF_ROWS` rows, so every page stays
/// within a PDF's limits.
fn pdf(crop: &Crop) -> Result<Vec<u8>, String> {
    let rgb = crate::shared::picture::rgb_on_white(crop);
    let row = crop.width as usize * 3;
    let pages: Vec<(u32, Vec<u8>)> = (rgb.chunks(row * PDF_ROWS as usize))
        .map(|rows| {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            z.write_all(rows).map_err(|e| e.to_string())?;
            let packed = z.finish().map_err(|e| e.to_string())?;
            Ok(((rows.len() / row) as u32, packed))
        })
        .collect::<Result<_, String>>()?;
    let w = crop.width;
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut at = Vec::new();
    let mut object = |out: &mut Vec<u8>, head: String, stream: Option<&[u8]>| {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n{head}\n", at.len()).bytes());
        if let Some(bytes) = stream {
            out.extend(b"stream\n");
            out.extend(bytes);
            out.extend(b"\nendstream\n");
        }
        out.extend(b"endobj\n");
    };
    // 1 catalog, 2 pages, then a page, its image and its content in turn
    let kids: String = (0..pages.len())
        .map(|i| format!("{} 0 R ", 3 + 3 * i))
        .collect();
    object(&mut out, "<< /Type /Catalog /Pages 2 0 R >>".into(), None);
    object(
        &mut out,
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()),
        None,
    );
    for (i, (h, image)) in pages.iter().enumerate() {
        let (pw, ph) = (w as f32 * 0.75, *h as f32 * 0.75);
        let draw = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q");
        let first = 3 + 3 * i;
        object(
            &mut out,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] \
                 /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                first + 1,
                first + 2
            ),
            None,
        );
        object(
            &mut out,
            format!(
                "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
                image.len()
            ),
            Some(image),
        );
        object(
            &mut out,
            format!("<< /Length {} >>", draw.len()),
            Some(draw.as_bytes()),
        );
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", at.len() + 1).bytes());
    for offset in &at {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            at.len() + 1
        )
        .bytes(),
    );
    Ok(out)
}

/// The Steps' exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steps {
    /// `steps.md` + the images.
    Markdown,
    /// `index.html` + the images.
    Html,
    /// The images alone.
    Images,
}

/// The words a page needs (in the app's language).
pub struct Words<'a> {
    pub title: &'a str,
    pub step: &'a str,
    pub rtl: bool,
}

/// Writes the steps (`pngs`, in order) into a new folder `name` in `parent`
/// (`name-2`, ... when taken): `step-01.png`, ... and the page `how` asks for.
/// Returns the folder.
pub fn write_steps(
    parent: &Path,
    name: &str,
    pngs: &[&[u8]],
    how: Steps,
    words: &Words<'_>,
) -> std::io::Result<PathBuf> {
    let folder = new_folder(parent, name)?;
    let written = write_into(&folder, pngs, how, words);
    keep_only_if_written(folder, written)
}

/// `folder` when `written` went well; else the half-written folder is removed
/// (it is the one just made for this export: only its files are in it).
fn keep_only_if_written(folder: PathBuf, written: std::io::Result<()>) -> std::io::Result<PathBuf> {
    if written.is_err() {
        let _ = std::fs::remove_dir_all(&folder);
    }
    written.map(|()| folder)
}

fn write_into(folder: &Path, pngs: &[&[u8]], how: Steps, words: &Words<'_>) -> std::io::Result<()> {
    let files: Vec<String> = (1..=pngs.len())
        .map(|n| format!("step-{n:02}.png"))
        .collect();
    for (file, png) in files.iter().zip(pngs) {
        std::fs::write(folder.join(file), png)?;
    }
    let alt = |i: usize| format!("{} {}", words.step, i + 1);
    match how {
        Steps::Markdown => {
            let mut page = format!("# {}\n", words.title);
            for (i, file) in files.iter().enumerate() {
                page += &format!("\n## {}\n\n![{}]({file})\n", i + 1, alt(i));
            }
            std::fs::write(folder.join("steps.md"), page)?;
        }
        Steps::Html => {
            let dir = if words.rtl { "rtl" } else { "ltr" };
            let mut page = format!(
                "<!doctype html>\n<html dir=\"{dir}\">\n<head>\n<meta charset=\"utf-8\">\n\
                 <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
                 <title>{t}</title>\n<style>\n\
                 body {{ font-family: system-ui, sans-serif; max-width: 1000px; margin: 2em auto; padding: 0 16px; }}\n\
                 img {{ max-width: 100%; border: 1px solid #8884; border-radius: 8px; }}\n\
                 li {{ margin: 0 0 2em; }}\n</style>\n</head>\n<body>\n<h1>{t}</h1>\n<ol>\n",
                t = html(words.title)
            );
            for (i, file) in files.iter().enumerate() {
                page += &format!("<li><img src=\"{file}\" alt=\"{}\"></li>\n", html(&alt(i)));
            }
            page += "</ol>\n</body>\n</html>\n";
            std::fs::write(folder.join("index.html"), page)?;
        }
        Steps::Images => {}
    }
    Ok(())
}

fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `parent/name`, or `name-2`, `name-3`, ... if taken: never into an old folder.
fn new_folder(parent: &Path, name: &str) -> std::io::Result<PathBuf> {
    for n in 1.. {
        let folder = match n {
            1 => parent.join(name),
            _ => parent.join(format!("{name}-{n}")),
        };
        match std::fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crop(width: u32, height: u32) -> Crop {
        let rgba = (0..width * height)
            .flat_map(|i| [(i % 251) as u8, 40, 200, 255])
            .collect();
        Crop {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn each_format_writes_its_own_kind_of_file_at_the_asked_size() {
        let how = |format, max_edge| Picture {
            format,
            quality: 70,
            max_edge,
        };
        for format in [Format::Png, Format::Jpg, Format::Webp] {
            let bytes = encode(crop(300, 100), how(format, 0)).unwrap();
            let read = image::load_from_memory(&bytes).unwrap();
            assert_eq!((read.width(), read.height()), (300, 100), "{format:?}");
            let small = encode(crop(300, 100), how(format, 150)).unwrap();
            let read = image::load_from_memory(&small).unwrap();
            assert_eq!((read.width(), read.height()), (150, 50), "{format:?}");
        }
        // lossless: WebP gives the same pixels back
        let webp = encode(crop(30, 20), how(Format::Webp, 0)).unwrap();
        let read = image::load_from_memory(&webp).unwrap().into_rgba8();
        assert_eq!(read.into_raw(), crop(30, 20).rgba);
        // a lower JPG quality is a smaller file
        let q = |quality| {
            let how = Picture {
                quality,
                ..how(Format::Jpg, 0)
            };
            encode(crop(300, 100), how).unwrap().len()
        };
        assert!(q(30) < q(95));
    }

    #[test]
    fn the_pdf_is_one_page_the_pictures_size_with_a_right_cross_reference() {
        use std::io::Read;
        let bytes = encode(
            crop(400, 200),
            Picture {
                format: Format::Pdf,
                ..Picture::default()
            },
        )
        .unwrap();
        let find = |from: usize, what: &str| {
            (bytes[from..].windows(what.len()))
                .position(|w| w == what.as_bytes())
                .map(|i| i + from)
        };
        let at = |i: usize, what: &str| bytes[i..].starts_with(what.as_bytes());
        assert!(at(0, "%PDF-1.4") && bytes.ends_with(b"%%EOF\n"));
        assert!(
            find(0, "/MediaBox [0 0 300.00 150.00]").is_some(),
            "96 px to the inch"
        );
        // every object sits where the cross reference says
        let tail = find(0, "startxref\n").unwrap() + 10;
        let xref: usize = String::from_utf8_lossy(&bytes[tail..])
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(at(xref, "xref\n0 6\n"));
        let table = String::from_utf8_lossy(&bytes[xref..]).into_owned();
        for (n, line) in table.lines().skip(3).take(5).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(at(offset, &format!("{} 0 obj", n + 1)), "{n}");
        }
        // the picture: width x height RGB once inflated
        let start = find(0, "stream\n").unwrap() + 7;
        let end = find(start, "\nendstream").unwrap();
        let mut rgb = Vec::new();
        flate2::read::ZlibDecoder::new(&bytes[start..end])
            .read_to_end(&mut rgb)
            .unwrap();
        assert_eq!(rgb.len(), 400 * 200 * 3);
        assert_eq!(&rgb[..3], &[0, 40, 200]);
    }

    #[test]
    fn steps_go_into_a_new_folder_with_their_page() {
        let parent = std::env::temp_dir().join(format!("oc-steps-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent).unwrap();
        let pngs: [&[u8]; 2] = [b"one", b"two"];
        let words = Words {
            title: "Steps",
            step: "Step",
            rtl: false,
        };
        let md = write_steps(&parent, "s", &pngs, Steps::Markdown, &words).unwrap();
        assert_eq!(md, parent.join("s"));
        assert_eq!(std::fs::read(md.join("step-02.png")).unwrap(), b"two");
        let page = std::fs::read_to_string(md.join("steps.md")).unwrap();
        assert!(page.contains("![Step 1](step-01.png)") && page.contains("![Step 2](step-02.png)"));
        // the name taken: a new folder, the old one untouched
        let html = write_steps(&parent, "s", &pngs, Steps::Html, &words).unwrap();
        assert_eq!(html, parent.join("s-2"));
        let page = std::fs::read_to_string(html.join("index.html")).unwrap();
        assert!(page.contains("<img src=\"step-01.png\" alt=\"Step 1\">"));
        assert!(!md.join("index.html").exists());
        let images = write_steps(&parent, "s", &pngs, Steps::Images, &words).unwrap();
        let mut files: Vec<_> = std::fs::read_dir(&images)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files, ["step-01.png", "step-02.png"]);
        let _ = std::fs::remove_dir_all(&parent);
    }

    fn how(format: Format, max_edge: u32) -> Picture {
        Picture {
            format,
            quality: 70,
            max_edge,
        }
    }

    #[test]
    fn webp_over_its_limit_is_a_problem_unless_the_size_choice_brings_it_under() {
        assert_eq!(
            problem(400, 17000, how(Format::Webp, 0)),
            Some(Problem::WebpTooBig)
        );
        assert_eq!(problem(400, 17000, how(Format::Webp, 2048)), None);
        assert_eq!(problem(400, 17000, how(Format::Png, 0)), None);
        // encode says so too instead of the encoder's own error
        assert!(encode(crop(400, 17000), how(Format::Webp, 0)).is_err());
        assert!(encode(crop(400, 17000), how(Format::Webp, 2048)).is_ok());
    }

    #[test]
    fn a_tall_pdf_is_pages_that_each_stay_within_the_limits() {
        let tall = Crop {
            width: 3440,
            height: 20000,
            rgba: vec![255; 3440 * 20000 * 4],
        };
        let bytes = encode(tall, how(Format::Pdf, 0)).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 2 >>"), "two pages");
        for page in text.split("/MediaBox [0 0 ").skip(1) {
            let h: f32 = page
                .split(' ')
                .nth(1)
                .unwrap()
                .trim_end_matches(']')
                .parse()
                .unwrap();
            assert!(h <= 14400.0, "{h}");
        }
        assert!(text.contains("/MediaBox [0 0 2580.00 12288.00]"));
        assert!(text.contains("/MediaBox [0 0 2580.00 2712.00]"));
        assert_eq!(
            problem(20000, 10, how(Format::Pdf, 0)),
            Some(Problem::PdfTooWide)
        );
    }

    #[test]
    fn see_through_pixels_go_onto_white_in_jpg_and_pdf() {
        let clear = Crop {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 0, 0, 0, 0, 255],
        };
        let jpg = encode(clear.clone(), how(Format::Jpg, 100)).unwrap();
        let read = image::load_from_memory(&jpg).unwrap().into_rgb8();
        assert!(read.get_pixel(0, 0).0.iter().all(|&c| c > 240), "white");
        assert!(
            read.get_pixel(1, 0).0.iter().all(|&c| c < 15),
            "black stays"
        );
        assert_eq!(
            crate::shared::picture::rgb_on_white(&clear),
            [255, 255, 255, 0, 0, 0]
        );
    }

    #[test]
    fn a_typed_name_without_an_extension_gets_the_formats() {
        let p = |s: &str, f| with_ext(PathBuf::from(s), f);
        assert_eq!(p("/a/shot", Format::Webp), PathBuf::from("/a/shot.webp"));
        assert_eq!(p("/a/shot.png", Format::Pdf), PathBuf::from("/a/shot.png"));
    }

    #[test]
    fn a_failed_steps_export_leaves_no_half_written_folder() {
        let parent = std::env::temp_dir().join(format!("oc-steps-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent).unwrap();
        let words = Words {
            title: "t",
            step: "s",
            rtl: false,
        };
        let pngs: [&[u8]; 2] = [b"one", b"two"];
        let folder = new_folder(&parent, "w").unwrap();
        std::fs::create_dir(folder.join("step-02.png")).unwrap(); // a dir where an image goes
        let written = write_into(&folder, &pngs, Steps::Images, &words);
        assert!(written.is_err());
        assert!(keep_only_if_written(folder.clone(), written).is_err());
        assert!(!folder.exists(), "removed");
        let ok = new_folder(&parent, "ok").unwrap();
        assert_eq!(keep_only_if_written(ok.clone(), Ok(())).unwrap(), ok);
        assert!(ok.exists());
        let _ = std::fs::remove_dir_all(&parent);
    }
}
