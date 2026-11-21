//! Snip Studio's Export (v0.4.2, STUDIO_V2_SPEC §8): the picture as PNG / JPG /
//! WebP / PDF, and the Steps as a Markdown page, an HTML page or a folder of
//! images. Only bytes and files here: the dialogs and pills are the host's.
use std::io::Write;
use std::path::{Path, PathBuf};

use super::transition::{self, Transition};
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

/// The walkthrough's file kinds. MP4 and WebM are AV1 (`movie`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movie {
    Gif,
    Mp4,
    Webm,
}

impl Movie {
    pub const ALL: [Movie; 3] = [Movie::Gif, Movie::Mp4, Movie::Webm];

    pub fn ext(self) -> &'static str {
        match self {
            Movie::Gif => "gif",
            Movie::Mp4 => "mp4",
            Movie::Webm => "webm",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Movie::Gif => "GIF",
            Movie::Mp4 => "MP4",
            Movie::Webm => "WebM",
        }
    }

    /// The config's number for it (`snip.studio.walk_format`).
    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(i: u8) -> Movie {
        Movie::ALL
            .get(usize::from(i))
            .copied()
            .unwrap_or(Movie::Gif)
    }

    /// About how many bytes a frame of `png` bytes adds (a guess, for the estimate line).
    // ponytail: ratios from the lab's flat screenshots (GIF ~ the PNG, AV1 ~ a third); the real size shows in the file.
    pub fn guess(self, png: usize) -> usize {
        match self {
            Movie::Gif => png,
            Movie::Mp4 | Movie::Webm => png / 3,
        }
    }
}

/// The walkthrough's choices (STUDIO_V2_SPEC "Export dialog": Format, Size, Frame time, Loop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Walkthrough {
    pub movie: Movie,
    /// The long edge at most (0 = each frame's own size).
    pub max_edge: u32,
    /// Seconds each frame shows, in milliseconds.
    pub frame_ms: u32,
    /// GIF only (a video file plays once; the player decides).
    pub repeat: bool,
    /// Each frame's place among them, in a badge at its corner.
    pub numbered: bool,
    /// ONE transition between all the steps (`transition`), and its length in ms.
    pub transition: Transition,
    pub transition_ms: u32,
}

/// A video's long edge at most (what a player and an MP4's 16-bit sizes hold).
pub const VIDEO_MAX_EDGE: u32 = 4096;

/// The edits walkthrough's frames at most (a longer history is sampled evenly).
pub const EDIT_FRAMES: usize = 30;

impl Walkthrough {
    /// The frames the transitions add between `steps` steps (what the size estimate adds).
    pub fn mix_frames(self, steps: usize) -> usize {
        steps.saturating_sub(1) * transition::mix_count(self.transition, self.transition_ms)
    }

    /// The long edge the frames are fitted to: the pick, but a video never
    /// above `VIDEO_MAX_EDGE` ("Original" is that, at most).
    pub fn edge(self) -> u32 {
        match (self.movie, self.max_edge) {
            (Movie::Gif, edge) => edge,
            (_, 0) => VIDEO_MAX_EDGE,
            (_, edge) => edge.min(VIDEO_MAX_EDGE),
        }
    }
}

/// The document's history as pictures, the first state to the current one
/// (at most `EDIT_FRAMES`), each fitted to the walkthrough's size as it is
/// drawn: at most `EDIT_FRAMES` frames are held (the transition's frames between
/// them are made one at a time while encoding). The document ends as it was.
pub fn edit_frames(
    doc: &mut crate::features::snip::annotate::Document,
    how: Walkthrough,
) -> Vec<Crop> {
    let total = doc.depth() + 1;
    let want = |i: usize| match total <= EDIT_FRAMES {
        true => true,
        false => (0..EDIT_FRAMES).any(|j| j * (total - 1) / (EDIT_FRAMES - 1) == i),
    };
    let (mut i, mut out) = (0, Vec::new());
    doc.replay(|d| {
        if want(i) {
            out.push(fit_long_edge(
                crate::features::snip::annotate::render(d),
                how.edge(),
            ));
        }
        i += 1;
    });
    out
}

impl Default for Walkthrough {
    fn default() -> Self {
        Walkthrough {
            movie: Movie::Gif,
            max_edge: 1280,
            frame_ms: 2000,
            repeat: true,
            numbered: false,
            transition: Transition::default(),
            transition_ms: transition::DEFAULT_MS,
        }
    }
}

/// The frames (in order) as the file `how.movie` asks for: `None` when
/// `progress(done, total)` said stop. Frames of different sizes sit centred
/// on white in a canvas as big as the biggest.
pub fn encode_walkthrough(
    frames: Vec<Crop>,
    how: Walkthrough,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Vec<u8>>, String> {
    use crate::features::snip::movie;
    let (width, height, frames) = on_canvas(frames, how, how.movie != Movie::Gif)?;
    let cells = transition::timeline(
        frames.len(),
        how.frame_ms,
        how.transition,
        how.transition_ms,
    );
    let shown = pictures(
        &frames,
        &cells,
        how.transition,
        crate::shared::app_language::rtl(),
    );
    if how.movie == Movie::Gif {
        return gif_of(width, height, shown, cells.len(), how, progress);
    }
    // a video sample is a step held for its whole time, or one mixed frame
    let Some((av1c, samples)) = movie::encode_av1(shown, cells.len(), movie::KEY_EVERY, progress)?
    else {
        return Ok(None);
    };
    Ok(Some(match how.movie {
        Movie::Mp4 => movie::mp4(width, height, &av1c, &samples)?,
        _ => movie::webm(width, height, &av1c, &samples)?,
    }))
}

/// What the file shows, one picture at a time with its time (ms): a step is
/// the step itself (borrowed), a mixed frame is made when it is asked for and
/// gone once the encoder has it, so at most a frame or two are ever alive.
fn pictures<'a>(
    frames: &'a [Crop],
    cells: &'a [transition::Cell],
    kind: Transition,
    mirror: bool,
) -> impl Iterator<Item = (std::borrow::Cow<'a, Crop>, u32)> + 'a {
    cells.iter().map(move |c| match *c {
        transition::Cell::Hold(i, ms) => (std::borrow::Cow::Borrowed(&frames[i]), ms),
        transition::Cell::Mix(i, t) => (
            std::borrow::Cow::Owned(transition::mix(&frames[i], &frames[i + 1], kind, t, mirror)),
            transition::TICK,
        ),
    })
}

/// Every frame fitted, numbered, and centred on white in one canvas (`even`: a
/// video wants even sides).
fn on_canvas(
    frames: Vec<Crop>,
    how: Walkthrough,
    even: bool,
) -> Result<(u32, u32, Vec<Crop>), String> {
    let frames: Vec<Crop> = (frames.into_iter().enumerate())
        .map(|(i, f)| {
            let f = fit_long_edge(f, how.edge());
            match how.numbered {
                true => crate::features::snip::annotate::step::badge(
                    f,
                    u32::try_from(i + 1).unwrap_or(u32::MAX),
                ),
                false => f,
            }
        })
        .collect();
    let up = |n: u32| if even { n + n % 2 } else { n };
    let width = up(frames.iter().map(|f| f.width).max().ok_or("no frames")?);
    let height = up(frames.iter().map(|f| f.height).max().ok_or("no frames")?);
    let placed = frames
        .into_iter()
        .map(|f| {
            let mut canvas = vec![255u8; width as usize * height as usize * 4];
            let (dx, dy) = ((width - f.width) / 2, (height - f.height) / 2);
            let row = f.width as usize * 4;
            for y in 0..f.height as usize {
                let from = y * row;
                let to = ((y + dy as usize) * width as usize + dx as usize) * 4;
                canvas[to..to + row].copy_from_slice(&f.rgba[from..from + row]);
            }
            Crop {
                width,
                height,
                rgba: canvas,
            }
        })
        .collect();
    Ok((width, height, placed))
}

/// ponytail: a GIF has 256 colours a frame (NeuQuant, speed 10).
fn gif_of<'a>(
    width: u32,
    height: u32,
    frames: impl Iterator<Item = (std::borrow::Cow<'a, Crop>, u32)>,
    total: usize,
    how: Walkthrough,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Vec<u8>>, String> {
    let (w16, h16) = (
        u16::try_from(width).map_err(|_| "too wide for a GIF")?,
        u16::try_from(height).map_err(|_| "too tall for a GIF")?,
    );
    let mut out = Vec::new();
    let mut enc = gif::Encoder::new(&mut out, w16, h16, &[]).map_err(|e| e.to_string())?;
    if how.repeat {
        enc.set_repeat(gif::Repeat::Infinite)
            .map_err(|e| e.to_string())?;
    }
    for (i, (f, ms)) in frames.enumerate() {
        let mut rgba = f.rgba.clone();
        let mut frame = gif::Frame::from_rgba_speed(w16, h16, &mut rgba, 10);
        frame.delay = u16::try_from(ms / 10).unwrap_or(u16::MAX).max(1);
        enc.write_frame(&frame).map_err(|e| e.to_string())?;
        if !progress(i + 1, total) {
            return Ok(None);
        }
    }
    drop(enc);
    Ok(Some(out))
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

    fn encode_gif(frames: Vec<Crop>, how: Walkthrough) -> Result<Vec<u8>, String> {
        encode_walkthrough(frames, how, &mut |_, _| true).map(Option::unwrap)
    }

    /// W91.3: one transition between all the steps: the file holds each step (its own
    /// time) and the transition's frames between neighbours, one tick each.
    #[test]
    fn the_walkthrough_gif_has_the_transition_frames_between_the_steps() {
        let frames = || vec![crop(40, 20), crop(40, 20), crop(40, 20)];
        let times = |how| {
            let bytes = encode_gif(frames(), how).unwrap();
            let mut d = gif::DecodeOptions::new();
            d.set_color_output(gif::ColorOutput::RGBA);
            let mut r = d.read_info(&bytes[..]).unwrap();
            let mut out = Vec::new();
            while let Some(f) = r.read_next_frame().unwrap() {
                out.push(f.delay);
            }
            out
        };
        let how = |transition, transition_ms| Walkthrough {
            transition,
            transition_ms,
            frame_ms: 1000,
            ..Walkthrough::default()
        };
        assert_eq!(times(how(Transition::Cut, 500)), [100, 100, 100]);
        let fade = times(how(Transition::Crossfade, 300));
        assert_eq!(fade.len(), 3 + 2 * 6, "6 ticks of 50 ms in 0.3 s");
        assert_eq!((fade[0], fade[1], fade[7]), (100, 5, 100));
        let wipe = times(how(Transition::Wipe, 1000));
        assert_eq!(wipe.len(), 3 + 2 * 20);
        assert_eq!(how(Transition::Slide, 500).mix_frames(3), 20);
    }

    /// W91.3: a video holds a step as repeated ticks; frames come out of the encoder
    /// for every tick (the transition's included).
    #[test]
    fn a_video_walkthrough_with_a_transition_encodes_every_tick() {
        let how = Walkthrough {
            movie: Movie::Mp4,
            transition: Transition::Push,
            transition_ms: 300,
            frame_ms: 200,
            ..Walkthrough::default()
        };
        let mut total = 0;
        let bytes = encode_walkthrough(vec![crop(64, 32), crop(64, 32)], how, &mut |_, all| {
            total = all;
            true
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            total,
            1 + 6 + 1,
            "hold, 6 mixes, hold: a hold is one sample"
        );
        assert_eq!(&bytes[4..8], b"ftyp");
    }

    #[test]
    fn walkthrough_gif_has_every_frame_its_time_and_the_loop_flag() {
        let frames = || vec![crop(300, 100), crop(200, 150), crop(300, 100)];
        let cut = Walkthrough {
            transition: Transition::Cut,
            ..Walkthrough::default()
        };
        let bytes = encode_gif(frames(), cut).unwrap();
        let mut d = gif::DecodeOptions::new();
        d.set_color_output(gif::ColorOutput::RGBA);
        let mut r = d.clone().read_info(&bytes[..]).unwrap();
        assert_eq!(
            (r.width(), r.height()),
            (300, 150),
            "canvas = the biggest frame"
        );
        let mut n = 0;
        while let Some(f) = r.read_next_frame().unwrap() {
            assert_eq!(f.delay, 200);
            n += 1;
        }
        assert_eq!(n, 3);
        assert!(bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "loops");
        let once = Walkthrough {
            repeat: false,
            ..cut
        };
        assert!(
            !encode_gif(frames(), once)
                .unwrap()
                .windows(11)
                .any(|w| w == b"NETSCAPE2.0")
        );
        // the size cap scales the frames down
        let small = Walkthrough {
            max_edge: 150,
            ..cut
        };
        let r = d
            .read_info(&encode_gif(frames(), small).unwrap()[..])
            .map(|r| (r.width(), r.height()));
        assert_eq!(r.unwrap(), (150, 113));
        assert!(encode_gif(vec![], Walkthrough::default()).is_err());
        // numbered frames differ from plain ones (the badge) and keep their size
        let plain = |how| {
            let bytes = encode_gif(vec![crop(300, 100)], how).unwrap();
            let mut d = gif::DecodeOptions::new();
            d.set_color_output(gif::ColorOutput::RGBA);
            let mut r = d.read_info(&bytes[..]).unwrap();
            r.read_next_frame().unwrap().unwrap().buffer.to_vec()
        };
        let numbered = Walkthrough {
            numbered: true,
            ..Walkthrough::default()
        };
        let (a, b) = (plain(Walkthrough::default()), plain(numbered));
        assert_eq!(a.len(), b.len());
        assert_ne!(a, b);
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

    /// W91.1: the pictures are made one at a time: a held step is borrowed, a mixed frame
    /// is built only when the iterator reaches it, so a long walkthrough never holds them all.
    #[test]
    fn mixed_frames_are_made_one_at_a_time() {
        let frames = vec![crop(32, 16), crop(32, 16), crop(32, 16)];
        let cells = transition::timeline(3, 1000, Transition::Crossfade, 1000);
        assert_eq!(cells.len(), 3 + 2 * 20);
        let before = transition::MIXES.with(std::cell::Cell::get);
        let mut shown = pictures(&frames, &cells, Transition::Crossfade, false);
        let made = || transition::MIXES.with(std::cell::Cell::get) - before;
        assert_eq!(made(), 0, "nothing is built up front");
        let (first, ms) = shown.next().unwrap();
        assert!(matches!(first, std::borrow::Cow::Borrowed(_)) && ms == 1000);
        let (mix, ms) = shown.next().unwrap();
        assert!(matches!(mix, std::borrow::Cow::Owned(_)) && ms == transition::TICK);
        assert_eq!(made(), 1, "one mix per picture asked for");
        drop(mix);
        let rest = shown.by_ref().take(5).count();
        assert_eq!((rest, made()), (5, 6));
    }
}
