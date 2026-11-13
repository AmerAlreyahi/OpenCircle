//! A sharp downscale for long capture's live preview, fed row by row: every
//! output pixel is the exact area average of the source pixels it covers
//! (fractional edges weighted), so 1 px text strokes stay visible and no row
//! is skipped or doubled, as nearest, plain bilinear or whole-block averages
//! do at ratios like 1.5. Streaming: the stitcher feeds only the rows it adds,
//! so a preview update scales the new band alone.

/// The scaler's state between rows. Cheap to clone (one accumulator row), so
/// rows that may still change (the footer) are scaled on a copy.
#[derive(Clone)]
pub struct Shrink {
    out_w: usize,
    /// Source px per output px (>= 1).
    ratio: f64,
    /// Per output column: its first source column and the weights from there.
    taps: Vec<(usize, Vec<f32>)>,
    /// The output row being gathered (RGBA, weighted sums) and its coverage.
    acc: Vec<f32>,
    covered: f64,
    /// Source rows fed so far and output rows finished.
    fed: u64,
    done: u64,
    /// One source row scaled across (reused).
    across: Vec<f32>,
}

impl Shrink {
    /// From `width` px rows to `out_w` (at most `width`: never enlarges).
    pub fn new(width: usize, out_w: usize) -> Self {
        let out_w = out_w.clamp(1, width.max(1));
        let ratio = width as f64 / out_w as f64;
        let taps = (0..out_w)
            .map(|x| {
                let (lo, hi) = (x as f64 * ratio, (x + 1) as f64 * ratio);
                let first = lo.floor() as usize;
                let last = (hi.ceil() as usize).min(width);
                let weights = (first..last)
                    .map(|s| {
                        let cover = (hi.min(s as f64 + 1.0) - lo.max(s as f64)).max(0.0);
                        (cover / ratio) as f32
                    })
                    .collect();
                (first, weights)
            })
            .collect();
        Shrink {
            out_w,
            ratio,
            taps,
            acc: vec![0.0; out_w * 4],
            covered: 0.0,
            fed: 0,
            done: 0,
            across: vec![0.0; out_w * 4],
        }
    }

    pub fn out_w(&self) -> usize {
        self.out_w
    }

    /// Feeds one source row (RGBA); finished output rows go to `out`.
    pub fn feed(&mut self, row: &[u8], out: &mut Vec<u8>) {
        for (x, (first, weights)) in self.taps.iter().enumerate() {
            let mut px = [0f32; 4];
            for (i, w) in weights.iter().enumerate() {
                let s = &row[(first + i) * 4..(first + i) * 4 + 4];
                for c in 0..4 {
                    px[c] += f32::from(s[c]) * w;
                }
            }
            self.across[x * 4..x * 4 + 4].copy_from_slice(&px);
        }
        let (mut lo, hi) = (self.fed as f64, (self.fed + 1) as f64);
        loop {
            // (from the count, so the edges never drift)
            let edge = (self.done + 1) as f64 * self.ratio;
            if edge > hi + 1e-9 {
                break;
            }
            self.add(edge - lo);
            emit(&self.acc, self.covered, out);
            self.acc.iter_mut().for_each(|a| *a = 0.0);
            (self.covered, self.done, lo) = (0.0, self.done + 1, edge);
        }
        if hi - lo > 1e-9 {
            self.add(hi - lo);
        }
        self.fed += 1;
    }

    fn add(&mut self, weight: f64) {
        let w = weight as f32;
        for (a, v) in self.acc.iter_mut().zip(&self.across) {
            *a += v * w;
        }
        self.covered += weight;
    }

    /// The row still being gathered (the image's last, partly covered), if any.
    pub fn flush(&self, out: &mut Vec<u8>) {
        if self.covered > 1e-6 {
            emit(&self.acc, self.covered, out);
        }
    }
}

fn emit(acc: &[f32], covered: f64, out: &mut Vec<u8>) {
    let k = 1.0 / covered as f32;
    out.extend(acc.iter().map(|v| (v * k).round().clamp(0.0, 255.0) as u8));
}

/// `rgba` (`w` x `h`) scaled to `out_w` wide in one go.
#[cfg(test)]
pub fn shrink(rgba: &[u8], w: usize, out_w: usize) -> (Vec<u8>, usize) {
    let mut s = Shrink::new(w, out_w);
    let mut out = Vec::new();
    for row in rgba.chunks_exact(w * 4) {
        s.feed(row, &mut out);
    }
    s.flush(&mut out);
    let out_w = s.out_w();
    let h = out.len() / (out_w * 4);
    (out, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A text strip: black glyph-like strokes, 1 px wide, every 3 px across
    /// and every 4 px down in 14 px lines, on white.
    fn text_strip(w: usize, h: usize) -> Vec<u8> {
        let mut px = vec![255u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let ink = (y % 14 < 10) && (x % 3 == 0 || (y % 14) % 4 == 0 && x % 7 < 5);
                if ink {
                    px[(y * w + x) * 4..(y * w + x) * 4 + 3].fill(0);
                }
            }
        }
        px
    }

    fn grey(px: &[u8], w: usize, x: usize, y: usize) -> u8 {
        px[(y * w + x) * 4]
    }

    /// The exact area average of output pixel (x, y) at `ratio`, the slow way.
    fn exact(src: &[u8], w: usize, h: usize, ratio: f64, x: usize, y: usize) -> f64 {
        let (x0, x1) = (x as f64 * ratio, ((x + 1) as f64 * ratio).min(w as f64));
        let (y0, y1) = (y as f64 * ratio, ((y + 1) as f64 * ratio).min(h as f64));
        let (mut sum, mut area) = (0.0, 0.0);
        for sy in y0.floor() as usize..(y1.ceil() as usize) {
            for sx in x0.floor() as usize..(x1.ceil() as usize) {
                let a = (x1.min(sx as f64 + 1.0) - x0.max(sx as f64))
                    * (y1.min(sy as f64 + 1.0) - y0.max(sy as f64));
                sum += a * f64::from(grey(src, w, sx, sy));
                area += a;
            }
        }
        sum / area
    }

    #[test]
    fn every_pixel_is_the_exact_area_average() {
        let (w, h) = (300, 140);
        let src = text_strip(w, h);
        for out_w in [200, 150, 113] {
            let (out, oh) = shrink(&src, w, out_w);
            let ratio = w as f64 / out_w as f64;
            assert_eq!(oh, (h as f64 / ratio).ceil() as usize);
            for y in 0..oh {
                for x in 0..out_w {
                    let want = exact(&src, w, h, ratio, x, y);
                    let got = f64::from(grey(&out, out_w, x, y));
                    assert!(
                        (got - want).abs() <= 1.0,
                        "{out_w}: ({x},{y}) {got} vs {want}"
                    );
                }
            }
        }
    }

    /// Sharpness on a text strip at 1/1.5 and 1/2 (preview ratios): every
    /// 1 px stroke column stays visible (darker than 60 % grey); nearest
    /// neighbour (the aliased alternative) drops half of them at 1/2.
    #[test]
    fn text_strokes_stay_visible_and_contrasty() {
        let (w, h) = (300, 14);
        let src = text_strip(w, h);
        for out_w in [200, 150] {
            let (out, _) = shrink(&src, w, out_w);
            let ratio = w as f64 / out_w as f64;
            // source row 1: strokes only (x % 3 == 0)
            let lost = (0..w)
                .step_by(3)
                .filter(|&stroke| {
                    let ox = (stroke as f64 / ratio) as usize;
                    let darkest = (ox.saturating_sub(1)..(ox + 2).min(out_w))
                        .map(|x| grey(&out, out_w, x, 0))
                        .min()
                        .unwrap();
                    darkest > 153
                })
                .count();
            assert_eq!(lost, 0, "every stroke shows at 1/{ratio}");
        }
        // (the metric bites: nearest sampling at 1/2 keeps half the strokes)
        let dark = (0..150)
            .filter(|&x| grey(&src, w, x * 2 + 1, 1) == 0)
            .count();
        assert!(dark <= w / 6, "nearest keeps {dark} of {} strokes", w / 3);
        // a hard edge stays within 2 output px (no wide blur)
        let mut edge = vec![255u8; 30 * 4];
        edge[..15 * 4].fill(0);
        let (e, _) = shrink(&edge, 30, 20);
        let soft = (0..20).filter(|&x| (1..255).contains(&e[x * 4])).count();
        assert!(soft <= 1, "{soft} grey px at the edge");
    }

    #[test]
    fn fed_in_bands_it_equals_fed_at_once() {
        let (w, h) = (300, 97);
        let src = text_strip(w, h);
        let (whole, _) = shrink(&src, w, 113);
        let mut s = Shrink::new(w, 113);
        let mut out = Vec::new();
        for band in src.chunks(w * 4 * 13) {
            for row in band.chunks_exact(w * 4) {
                s.feed(row, &mut out);
            }
            // a peek at the partial row changes nothing
            let peek = s.clone();
            peek.flush(&mut Vec::new());
        }
        s.flush(&mut out);
        assert_eq!(out, whole);
        // never enlarges
        assert_eq!(Shrink::new(50, 80).out_w(), 50);
    }
}
