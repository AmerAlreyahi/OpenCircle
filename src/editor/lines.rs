//! Line assist for the blur brush: finds the image's text lines without OCR
//! (instant), so a stroke stays on the line it started on and is as tall as it.
//!
//! Detection: "ink" = a strong horizontal step in brightness (glyph edges; dark
//! on light and light on dark alike; a horizontal rule has none). Columns are
//! split at vertical gutters with no ink at all; in each column, rows with ink
//! form the line bands.
use std::time::{Duration, Instant};

use iced::Point;

use super::document::Image;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

impl Band {
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    pub fn centre(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    /// `p` is on this line (a little slack at its ends).
    pub fn contains(&self, p: Point) -> bool {
        let slack = self.height();
        (self.y0..=self.y1).contains(&p.y) && (self.x0 - slack..=self.x1 + slack).contains(&p.x)
    }

    /// The brush that covers the line (its anti-aliased edges too).
    pub fn brush(&self) -> f32 {
        self.height() + 4.0
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
        // Rows with a pixel or two of ink, joined across 2-px gaps (i dots).
        for (y0, y1) in runs(&rows, 3) {
            if !(5..=MAX_LINE).contains(&(y1 - y0)) {
                continue;
            }
            let cols = (x0..x1).filter(|&x| (y0..y1).any(|y| ink[y * w + x]));
            let (first, last) = cols.fold((x1, x0), |(a, b), x| (a.min(x), b.max(x)));
            bands.push(Band {
                x0: first as f32,
                x1: last as f32 + 1.0,
                y0: y0.saturating_sub(2) as f32,
                y1: (y1 + 2).min(h) as f32,
            });
        }
    }
    bands
}

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

/// The line under `p`.
pub fn under(bands: &[Band], p: Point) -> Option<Band> {
    bands.iter().copied().find(|b| b.contains(p))
}

/// The pointer must stay on another line this long to move the stroke there.
pub const SWITCH_AFTER: Duration = Duration::from_millis(150);

/// A brush stroke held on one line.
pub struct Lock {
    pub band: Band,
    /// Since when the pointer has been over another line, far from this one.
    away: Option<Instant>,
}

pub enum Follow {
    /// The stroke point (on the line's middle).
    Stay(Point),
    /// The pointer has been on another line long enough: go on there.
    Switch(Band),
}

impl Lock {
    pub fn new(band: Band) -> Self {
        Lock { band, away: None }
    }

    /// Where the stroke goes for pointer `p` at `now`.
    pub fn follow(&mut self, bands: &[Band], p: Point, now: Instant) -> Follow {
        let on_line = Point::new(p.x, self.band.centre());
        let far = (p.y - self.band.centre()).abs() >= self.band.height();
        let other = under(bands, p).filter(|b| far && *b != self.band);
        let Some(other) = other else {
            self.away = None;
            return Follow::Stay(on_line);
        };
        let since = *self.away.get_or_insert(now);
        if now.duration_since(since) >= SWITCH_AFTER {
            self.band = other;
            self.away = None;
            return Follow::Switch(other);
        }
        Follow::Stay(on_line)
    }
}
