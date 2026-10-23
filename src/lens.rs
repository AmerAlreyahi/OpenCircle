//! Google Lens through the user's own browser: no image host, no upload by us.
//! We write a local HTML page that embeds the crop and auto-submits it (multipart
//! POST) to lens.google.com, then open that page in the default browser. The
//! page file lives until the overlay drops its [`LensPage`] (or the next start
//! sweeps it after a crash).
#![allow(dead_code)] // until the action bar wires `lens::open`

use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const PREFIX: &str = "opencircle-lens-";
/// Long edge cap for what we send. Lens does not need more, and the page stays small.
const MAX_EDGE: u32 = 1600;
/// A page younger than this is never swept (it may belong to a live instance).
const SWEEP_AGE: Duration = Duration::from_secs(60);

const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="referrer" content="no-referrer">
<title>OpenCircle</title>
<style>body{font:16px system-ui,sans-serif;margin:3em;color:#333}button{font:inherit;margin-top:1em;padding:.4em 1em}</style>
</head>
<body>
<p>Opening Lens&hellip;</p>
<form id="f" method="POST" enctype="multipart/form-data" action="https://lens.google.com/v3/upload?ep=gsbubb&amp;st={{ST}}&amp;hl={{HL}}">
  <input type="file" id="img" name="encoded_image" accept="image/png" hidden>
  <input type="hidden" name="processed_image_dimensions" value="{{W}},{{H}}">
  <button type="submit">Search with Lens</button>
</form>
<script>
(function () {
  var b = atob("{{B64}}"), a = new Uint8Array(b.length);
  for (var i = 0; i < b.length; i++) a[i] = b.charCodeAt(i);
  var dt = new DataTransfer();
  dt.items.add(new File([a], "crop.png", { type: "image/png" }));
  document.getElementById("img").files = dt.files;
  document.getElementById("f").submit();
})();
</script>
</body>
</html>
"#;

/// A written Lens page. Deletes its file when dropped.
#[derive(Debug)]
pub struct LensPage {
    path: PathBuf,
}

impl Drop for LensPage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Write the page for this PNG crop (`width` x `height`) and open it in the
/// default browser. Hold the result until the overlay closes, then drop it.
pub fn open(png: &[u8], width: u32, height: u32) -> io::Result<LensPage> {
    let (png, w, h) = fit(png, width, height)?;
    let html = page_html(&png, w, h, &locale(), epoch_ms());
    let page = write_page(&std::env::temp_dir(), &html)?;
    launch(&page.path)?; // on error `page` drops and the file goes
    Ok(page)
}

/// Delete leftover pages from a crash. Call once at app start.
pub fn sweep_stale() {
    sweep_dir(&std::env::temp_dir(), SWEEP_AGE);
}

fn sweep_dir(dir: &Path, min_age: Duration) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let ours = e.file_name().to_string_lossy().starts_with(PREFIX);
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age >= min_age);
        // file_type() does not follow symlinks: only plain files of ours go.
        if ours && old && e.file_type().is_ok_and(|t| t.is_file()) {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

fn write_page(dir: &Path, html: &str) -> io::Result<LensPage> {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let path = dir.join(format!("{PREFIX}{}-{nanos}.html", std::process::id()));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true); // O_EXCL: never follows a planted symlink
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&path)?;
    let page = LensPage { path }; // from here a failed write still cleans up
    f.write_all(html.as_bytes())?;
    Ok(page)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn page_html(png: &[u8], w: u32, h: u32, hl: &str, st: u128) -> String {
    TEMPLATE
        .replace("{{ST}}", &st.to_string())
        .replace("{{HL}}", &esc(hl))
        .replace("{{W}}", &w.to_string())
        .replace("{{H}}", &h.to_string())
        .replace("{{B64}}", &base64(png))
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() {
                T[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

// ---- locale -------------------------------------------------------------

/// A BCP-47-ish tag from the OS, or "en".
fn locale() -> String {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(buf: *mut u16, len: i32) -> i32;
        }
        let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
        // SAFETY: buf is valid for 85 u16 and we pass its length.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            return locale_tag(&String::from_utf16_lossy(&buf[..n as usize - 1]));
        }
        "en".into()
    }
    #[cfg(not(windows))]
    {
        let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_default();
        locale_tag(&raw)
    }
}

/// "en_US.UTF-8" -> "en-US". Anything odd (C, POSIX, markup, too long) -> "en".
fn locale_tag(raw: &str) -> String {
    let base = raw.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let ok = base.len() <= 16
        && base.starts_with(|c: char| c.is_ascii_alphabetic())
        && base.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !matches!(base.as_str(), "C" | "POSIX");
    if ok { base } else { "en".into() }
}

// ---- downscale ----------------------------------------------------------

/// Size with the long edge capped at `MAX_EDGE`, aspect ratio kept.
fn scaled_size(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= MAX_EDGE {
        return (w, h);
    }
    let s = |v: u32| ((v as u64 * MAX_EDGE as u64 + long as u64 / 2) / long as u64).max(1) as u32;
    (s(w), s(h))
}

fn bad(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, e.to_string())
}

/// The PNG to send and its size: unchanged when small, else box-downscaled.
fn fit(png: &[u8], w: u32, h: u32) -> io::Result<(Vec<u8>, u32, u32)> {
    if w == 0 || h == 0 {
        return Err(bad("empty image"));
    }
    if w.max(h) <= MAX_EDGE {
        return Ok((png.to_vec(), w, h));
    }
    let mut dec = png::Decoder::new(Cursor::new(png));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(bad)?;
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| bad("image too large"))?
    ];
    let info = reader.next_frame(&mut buf).map_err(bad)?;
    if (info.width, info.height) != (w, h) {
        return Err(bad("PNG size does not match the given size"));
    }
    let ch = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(bad("unexpected indexed PNG")),
    };
    let px = |x: u32, y: u32| -> [u32; 4] {
        let i = (y as usize * w as usize + x as usize) * ch;
        let p = &buf[i..i + ch];
        match ch {
            1 => [p[0] as u32, p[0] as u32, p[0] as u32, 255],
            2 => [p[0] as u32, p[0] as u32, p[0] as u32, p[1] as u32],
            3 => [p[0] as u32, p[1] as u32, p[2] as u32, 255],
            _ => [p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32],
        }
    };
    let (dw, dh) = scaled_size(w, h);
    let mut out = Vec::with_capacity(dw as usize * dh as usize * 4);
    // ponytail: plain (non-premultiplied) box average; screenshots are opaque.
    for dy in 0..dh {
        let (y0, y1) = (dy * h / dh, ((dy + 1) * h / dh).max(dy * h / dh + 1));
        for dx in 0..dw {
            let (x0, x1) = (dx * w / dw, ((dx + 1) * w / dw).max(dx * w / dw + 1));
            let (mut sum, mut n) = ([0u32; 4], 0u32);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = px(x, y);
                    (0..4).for_each(|c| sum[c] += p[c]);
                    n += 1;
                }
            }
            out.extend(sum.iter().map(|s| (s / n) as u8));
        }
    }
    let mut enc_out = Vec::new();
    let mut enc = png::Encoder::new(&mut enc_out, dw, dh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&out))
        .map_err(bad)?;
    Ok((enc_out, dw, dh))
}

// ---- browser ------------------------------------------------------------

/// Open `path` in the default browser. No shell, no URL: the path is one argv
/// entry and must be absolute, so it can never read as an option.
fn launch(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(bad("page path must be absolute"));
    }
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(path);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    };
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap it without blocking: xdg-open can stay alive while the browser runs.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oc-lens-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
        enc.write_header().unwrap().write_image_data(&data).unwrap();
        out
    }

    #[test]
    fn html_has_fields_dimensions_and_neutral_copy() {
        let html = page_html(b"Ma", 3, 2, "en-US", 1234);
        assert!(
            html.contains("https://lens.google.com/v3/upload?ep=gsbubb&amp;st=1234&amp;hl=en-US")
        );
        assert!(html.contains("name=\"encoded_image\""));
        assert!(html.contains("name=\"processed_image_dimensions\" value=\"3,2\""));
        assert!(html.contains("atob(\"TWE=\")"));
        assert!(html.contains("Opening Lens"));
        assert!(html.contains("type=\"submit\""));
        assert!(!html.contains("{{"));
    }

    #[test]
    fn locale_is_sanitised_and_escaped() {
        assert_eq!(locale_tag("en_US.UTF-8"), "en-US");
        assert_eq!(locale_tag("ar_SA@x"), "ar-SA");
        assert_eq!(locale_tag("C"), "en");
        assert_eq!(locale_tag("POSIX"), "en");
        assert_eq!(locale_tag(""), "en");
        assert_eq!(locale_tag("en\"><script>alert(1)</script>"), "en");
        assert_eq!(locale_tag("-x"), "en");
        // even if a bad value reached the template, it is escaped
        let html = page_html(b"", 1, 1, "\"><script>x", 0);
        assert!(!html.contains("<script>x"));
        assert!(html.contains("hl=&quot;&gt;&lt;script&gt;x"));
    }

    #[test]
    fn file_is_private_and_deleted_on_drop() {
        let dir = scratch("drop");
        let page = write_page(&dir, "<html></html>").unwrap();
        let path = page.path.clone();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(PREFIX)
        );
        assert!(path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(page);
        assert!(!path.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sweep_removes_only_old_pages_with_our_prefix() {
        let dir = scratch("sweep");
        let old = SystemTime::now() - Duration::from_secs(3600);
        let mk = |name: &str, aged: bool| {
            let p = dir.join(name);
            let f = fs::File::create(&p).unwrap();
            if aged {
                f.set_modified(old).unwrap();
            }
            p
        };
        let ours_old = mk(&format!("{PREFIX}1-1.html"), true);
        let ours_new = mk(&format!("{PREFIX}2-2.html"), false);
        let other_old = mk("other-file.html", true);
        sweep_dir(&dir, Duration::from_secs(60));
        assert!(!ours_old.exists());
        assert!(ours_new.exists());
        assert!(other_old.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scaled_size_keeps_aspect_and_cap() {
        assert_eq!(scaled_size(800, 600), (800, 600));
        assert_eq!(scaled_size(1600, 900), (1600, 900));
        assert_eq!(scaled_size(3200, 1000), (1600, 500));
        assert_eq!(scaled_size(1000, 4000), (400, 1600));
        assert_eq!(scaled_size(7680, 4320), (1600, 900));
        assert_eq!(scaled_size(10000, 1), (1600, 1)); // never zero
    }

    #[test]
    fn fit_downscales_big_and_passes_small() {
        let small = make_png(40, 30);
        let (out, w, h) = fit(&small, 40, 30).unwrap();
        assert_eq!((out, w, h), (small.clone(), 40, 30));

        let big = make_png(3200, 1000);
        let (out, w, h) = fit(&big, 3200, 1000).unwrap();
        assert_eq!((w, h), (1600, 500));
        let info = png::Decoder::new(Cursor::new(&out))
            .read_info()
            .unwrap()
            .info()
            .clone();
        assert_eq!((info.width, info.height), (1600, 500));

        assert!(fit(&big, 3200, 999).is_err()); // size mismatch
        assert!(fit(b"nope", 4000, 10).is_err()); // not a PNG
        assert!(fit(&small, 0, 30).is_err());
    }

    #[test]
    fn launch_rejects_relative_path() {
        assert!(launch(Path::new("-x.html")).is_err());
    }
}
