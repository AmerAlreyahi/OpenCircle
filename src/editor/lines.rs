//! The blur brush's Auto size: finds the image's text lines without OCR
//! (instant, any capture), so a stroke is as tall as the line it starts on.
//! Nothing of it is drawn (owner, 2026-10-31: no suggested boxes).
//!
//! Detection: "ink" = a strong horizontal step in brightness (glyph edges; dark
//! on light and light on dark alike; a horizontal rule has none). Columns are
//! split at vertical gutters with no ink at all; in each column, rows with ink
//! form the line bands.
use iced::Point;

use super::document::Image;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    /// Cut from lines set so tight they touch: it already reaches its
    /// neighbours' cores, so it gets only the least margin.
    pub tight: bool,
}

impl Band {
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// `p` is on this line (a little slack at its ends).
    pub fn contains(&self, p: Point) -> bool {
        let slack = self.height();
        (self.y0..self.y1).contains(&p.y) && (self.x0 - slack..=self.x1 + slack).contains(&p.x)
    }

    /// The brush that covers ALL of the line's ink (anti-aliased edges,
    /// accents, tails the edge test misses): a margin of 15 % of its height,
    /// at least 2 image px and 2 display px (`zoom`), above and below; it may
    /// reach into the gap between lines.
    pub fn brush(&self, zoom: f32) -> f32 {
        let least = (2.0f32).max(2.0 / zoom);
        let margin = if self.tight {
            least
        } else {
            least.max(0.15 * self.height())
        };
        self.height() + 2.0 * margin
    }

    pub fn centre(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
}

/// Brightness step between neighbours that counts as a glyph edge.
const EDGE: i32 = 40;
/// Text lines are at most this tall (image px); taller blobs are pictures.
const MAX_LINE: usize = 160;
/// A run of edges down one column longer than this is a rule, not a glyph.
const VERTICAL_RULE: usize = 96;

/// The text lines of `image`, top to bottom per column.
// ponytail: gutters are global (a full-width title over two columns hides the
// gutter); split per block of rows if real layouts need it.
pub fn detect(image: &Image) -> Vec<Band> {
    let (w, h) = (image.width as usize, image.height as usize);
    if w < 2 || h < 2 {
        return Vec::new();
    }
    let luma = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        let p = &image.rgba[i..i + 3];
        (p[0] as i32 * 77 + p[1] as i32 * 150 + p[2] as i32 * 29) >> 8
    };
    let mut ink = vec![false; w * h];
    let mut columns = vec![0u32; w];
    for y in 0..h {
        let mut last = luma(0, y);
        for x in 1..w {
            let l = luma(x, y);
            if (l - last).abs() > EDGE {
                ink[y * w + x] = true;
                columns[x] += 1;
            }
            last = l;
        }
    }
    // Vertical rules (panel borders, a capture's edge on a board) have an edge on
    // every row: taller than any line, they are not ink.
    for (x, count) in columns.iter_mut().enumerate() {
        let mut y = 0;
        while y < h {
            let start = y;
            while y < h && ink[y * w + x] {
                y += 1;
            }
            if y - start > VERTICAL_RULE {
                (start..y).for_each(|y| ink[y * w + x] = false);
                *count -= (y - start) as u32;
            }
            y += 1;
        }
    }
    let gutter = (w / 60).max(12);
    let mut bands = Vec::new();
    for (x0, x1) in runs(&columns, gutter) {
        let rows: Vec<u32> = (0..h)
            .map(|y| ink[y * w + x0..y * w + x1].iter().filter(|&&i| i).count() as u32)
            .collect();
        // Rows with a pixel or two of ink, joined across 2-px gaps (i dots),
        // then split where tight lines touch.
        for (y0, y1, tight) in runs(&rows, 3).into_iter().flat_map(|r| split(&rows, r)) {
            if !(5..=MAX_LINE).contains(&(y1 - y0)) {
                continue;
            }
            let cols = (x0..x1).filter(|&x| (y0..y1).any(|y| ink[y * w + x]));
            let (first, last) = cols.fold((x1, x0), |(a, b), x| (a.min(x), b.max(x)));
            bands.push(Band {
                x0: first as f32,
                x1: last as f32 + 1.0,
                y0: y0 as f32,
                y1: y1 as f32,
                tight,
            });
        }
    }
    bands
}

/// The text lines in the run of ink rows `(y0, y1)`: lines set tight touch
/// (one's descenders, the next one's ascenders), but each has a dense core
/// (its x-height: every glyph has ink there) with only sparse rows between.
/// Each core gets ALL the sparse rows up to its neighbours' cores (they may hold
/// its ascenders, descenders or accents): neighbouring bands overlap in the gap.
fn split(rows: &[u32], (y0, y1): (usize, usize)) -> Vec<(usize, usize, bool)> {
    let max = rows[y0..y1].iter().copied().max().unwrap_or(0);
    let dense: Vec<u32> = (y0..y1)
        .map(|y| u32::from(rows[y] * 100 >= max * CORE))
        .collect();
    // Cores of at least 3 rows; a thinner dense stretch is not a line's core.
    let cores: Vec<_> = (runs(&dense, 1).into_iter())
        .filter(|(a, b)| b - a >= 3)
        .map(|(a, b)| (a + y0, b + y0))
        .collect();
    if cores.len() < 2 {
        return vec![(y0, y1, false)];
    }
    (0..cores.len())
        .map(|i| {
            let top = if i == 0 { y0 } else { cores[i - 1].1 };
            let bottom = match cores.get(i + 1) {
                Some(next) => next.0,
                None => y1,
            };
            (top, bottom, true)
        })
        .collect()
}

/// A row with at least this % of its run's most ink is in a line's core.
const CORE: u32 = 30;

/// Runs of non-zero `counts` (start, end exclusive), joining gaps shorter than `gap`.
fn runs(counts: &[u32], gap: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, _) in counts.iter().enumerate().filter(|(_, c)| **c > 0) {
        match out.last_mut() {
            Some((_, end)) if i - *end < gap => *end = i + 1,
            _ => out.push((i, i + 1)),
        }
    }
    out
}

/// The line under `p` (bands overlap between tight lines: the one whose middle is nearest).
pub fn under(bands: &[Band], p: Point) -> Option<Band> {
    (bands.iter().copied().filter(|b| b.contains(p))).min_by(|a, b| {
        (a.centre() - p.y)
            .abs()
            .total_cmp(&(b.centre() - p.y).abs())
    })
}
