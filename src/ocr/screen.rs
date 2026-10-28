//! Which band of the screen to read next (main mode, #123): pure scheduling,
//! no UI. Rows are image px from the top; a band is always full width, so it
//! is just a top and a height. The app (`ui/app/text.rs`) turns a `Next` into
//! an OCR task and feeds the answer back into `Rows`.
//!
//! The rules: the band under what the user touches first (a press, a drag
//! into unread rows, a tap), at once; then, once text was used (or Ctrl+A
//! waits), the rest, nearest the pointer first, gently; one band at a time
//! (the caller reads only while none is out); Windows OCR reads the whole
//! screen in one go, and its closer second read replaces its quick one.

use super::Source;

/// The height (image px) of the full-width band read where the user touches
/// text: the line under the pointer, with a margin. Small, as the user waits
/// for it: OCR reads every line in it (~0.1 CPU-s each).
const TOUCHED: f32 = 96.0;
/// The height of the bands that read the rest, in the background.
const BAND: f32 = 144.0;
/// Only lines wholly inside a band are kept (one cut by its edge is read
/// again by the next band), so a band knows its rows this far in from a cut
/// edge. Lines up to twice as tall are always whole in one band.
// ponytail: a line over 48 image px tall (a big heading) can be lost
// between two bands; a taller TOUCHED/BAND/EDGE if that shows up.
const EDGE: f32 = 24.0;

/// A full-width stretch of rows: its top and height (image px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub top: f32,
    pub height: f32,
}

/// What to do next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Next {
    /// Nothing to read now (all read, or nobody wants more yet).
    Idle,
    /// Windows OCR: the whole screen at once.
    Whole,
    /// The helper: this band; `gently` = in the background (the user isn't waiting).
    Band { band: Band, gently: bool },
}

/// The rows read so far, as sorted, separate (top, bottom) ranges.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Rows {
    known: Vec<(f32, f32)>,
}

impl Rows {
    /// Row `y` was read.
    pub fn knows(&self, y: f32) -> bool {
        self.known.iter().any(|&(a, b)| a <= y && y < b)
    }

    /// Every row of a screen `height` tall was read.
    pub fn is_whole(&self, height: f32) -> bool {
        matches!(self.known[..], [(a, b)] if a <= 0.0 && b >= height)
    }

    /// The band to read first for row `y` (touched), centred on it.
    pub fn touched(y: f32, height: f32) -> Band {
        let h = TOUCHED.min(height);
        Band {
            top: (y - h / 2.0).clamp(0.0, height - h),
            height: h,
        }
    }

    /// The next band to read, the unread row nearest `y` first; `None` once
    /// all is read. Next to rows already read it overlaps them by `EDGE` (a
    /// line cut there is read whole) and reaches away from them.
    pub fn next_band(&self, y: f32, height: f32) -> Option<Band> {
        let mut gaps = Vec::new();
        let mut from = 0.0;
        for &(a, b) in &self.known {
            if a > from {
                gaps.push((from, a));
            }
            from = b;
        }
        if from < height {
            gaps.push((from, height));
        }
        let off = |&(a, b): &(f32, f32)| (a - y).max(y - b).max(0.0);
        let (a, b) = gaps.into_iter().min_by(|p, q| off(p).total_cmp(&off(q)))?;
        let h = BAND.min(height);
        let top = match () {
            _ if a <= y && y < b => y - h / 2.0,
            _ if a >= y => a - EDGE, // below: down from its top
            _ => b + EDGE - h,       // above: up from its bottom
        };
        Some(Band {
            top: top.clamp(0.0, height - h),
            height: h,
        })
    }

    /// The rows of `band` it knows for sure and that weren't read yet, as
    /// separate (top, bottom) ranges.
    pub fn new_rows(&self, band: Band, height: f32) -> Vec<(f32, f32)> {
        let bottom = band.top + band.height;
        let top = if band.top <= 0.5 {
            0.0
        } else {
            band.top + EDGE
        };
        let bottom = match bottom >= height - 0.5 {
            true => height,
            false => bottom - EDGE,
        };
        let mut new = vec![(top, bottom)];
        for &(a, b) in &self.known {
            new = (new.into_iter())
                .flat_map(|(x, y)| [(x, y.min(a)), (x.max(b), y)])
                .filter(|(x, y)| x < y)
                .collect();
        }
        new.retain(|(x, y)| x < y);
        new
    }

    /// The rows OCR needs to read in `band` (top, bottom): only lines with
    /// their middle in them are kept. `None`: nothing new.
    pub fn unread(&self, band: Band, height: f32) -> Option<(f32, f32)> {
        let new = self.new_rows(band, height);
        Some((new.first()?.0, new.last()?.1))
    }

    /// `band` was read: its new rows (returned) are known now.
    pub fn mark(&mut self, band: Band, height: f32) -> Vec<(f32, f32)> {
        let new = self.new_rows(band, height);
        let (Some(&(top, _)), Some(&(_, bottom))) = (new.first(), new.last()) else {
            return new;
        };
        self.known.push((top, bottom));
        self.known.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut merged: Vec<(f32, f32)> = Vec::new();
        for (a, b) in self.known.drain(..) {
            match merged.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        self.known = merged;
        new
    }

    /// `band` was read by `source`: Windows OCR's closer read replaces its
    /// quick one (everything read before is forgotten). Returns the new rows
    /// and whether this was the first read.
    pub fn mark_read(
        &mut self,
        source: Source,
        band: Band,
        height: f32,
    ) -> (Vec<(f32, f32)>, bool) {
        if source == Source::System {
            self.known.clear();
        }
        let first = self.known.is_empty();
        (self.mark(band, height), first)
    }
}

/// What the user wants read now.
pub struct Wants<'a> {
    /// Who reads (`ocr::route`) and whether its first read already started.
    pub source: Source,
    pub started: bool,
    /// Rows the user touches (a drag's end, a press, a waiting tap).
    pub touches: &'a [f32],
    /// Ctrl+A waits for the rest; text was used (the rest reads gently).
    pub waits: bool,
    pub text_used: bool,
    /// Where the pointer is.
    pub pointer: f32,
}

/// The next read for a `height`-tall screen (the caller reads only while no
/// band is out).
pub fn next(rows: &Rows, height: f32, w: &Wants) -> Next {
    if w.source == Source::System {
        return if w.started { Next::Idle } else { Next::Whole };
    }
    if let Some(&y) = w.touches.iter().find(|&&y| !rows.knows(y)) {
        return Next::Band {
            band: Rows::touched(y, height),
            gently: false,
        };
    }
    if !(w.waits || w.text_used) {
        return Next::Idle;
    }
    match rows.next_band(w.pointer, height) {
        Some(band) => Next::Band {
            band,
            gently: !w.waits, // (Ctrl+A: the user waits)
        },
        None => Next::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f32 = 1080.0;

    fn wants(touches: &[f32]) -> Wants<'_> {
        Wants {
            source: Source::Helper,
            started: true,
            touches,
            waits: false,
            text_used: true,
            pointer: 500.0,
        }
    }

    fn band_of(n: Next) -> Band {
        match n {
            Next::Band { band, .. } => band,
            n => panic!("a band, got {n:?}"),
        }
    }

    #[test]
    fn the_touched_band_comes_first_then_nearest_first() {
        let mut rows = Rows::default();
        // a press at 900 beats a pointer at 500, and is not gentle
        let n = next(&rows, H, &wants(&[900.0]));
        assert!(matches!(n, Next::Band { gently: false, .. }));
        let b = band_of(n);
        assert!(b.top <= 900.0 && 900.0 < b.top + b.height);
        rows.mark(b, H);
        assert!(rows.knows(900.0) && !rows.knows(500.0));
        // no touch left unread: the rest, around the pointer first, gently
        let n = next(&rows, H, &wants(&[900.0]));
        assert!(matches!(n, Next::Band { gently: true, .. }));
        let b = band_of(n);
        assert!(b.top <= 500.0 && 500.0 < b.top + b.height, "{b:?}");
    }

    #[test]
    fn one_band_at_a_time_means_the_same_answer_until_it_is_read() {
        let rows = Rows::default();
        assert_eq!(next(&rows, H, &wants(&[])), next(&rows, H, &wants(&[])));
    }

    #[test]
    fn nothing_reads_until_text_is_used_unless_the_user_waits() {
        let rows = Rows::default();
        let w = Wants {
            text_used: false,
            ..wants(&[])
        };
        assert_eq!(next(&rows, H, &w), Next::Idle);
        let w = Wants { waits: true, ..w };
        assert!(matches!(
            next(&rows, H, &w),
            Next::Band { gently: false, .. }
        ));
    }

    #[test]
    fn it_ends_when_the_whole_screen_is_read() {
        let mut rows = Rows::default();
        let mut reads = 0;
        while let Next::Band { band, .. } = next(&rows, H, &wants(&[])) {
            rows.mark(band, H);
            reads += 1;
            assert!(reads < 30, "it ends");
        }
        assert!(rows.is_whole(H) && reads <= 13, "{reads} bands");
        assert_eq!(next(&rows, H, &wants(&[100.0])), Next::Idle);
    }

    #[test]
    fn windows_ocr_reads_the_whole_screen_once() {
        let rows = Rows::default();
        let w = Wants {
            source: Source::System,
            started: false,
            ..wants(&[])
        };
        assert_eq!(next(&rows, H, &w), Next::Whole);
        assert_eq!(next(&rows, H, &Wants { started: true, ..w }), Next::Idle);
    }

    #[test]
    fn a_closer_windows_read_replaces_the_quick_one() {
        let mut rows = Rows::default();
        let whole = Band {
            top: 0.0,
            height: H,
        };
        let (new, first) = rows.mark_read(Source::System, whole, H);
        assert_eq!((new, first), (vec![(0.0, H)], true));
        // the closer read: the quick one is forgotten, this one counts in full
        let (new, first) = rows.mark_read(Source::System, whole, H);
        assert_eq!((new, first), (vec![(0.0, H)], true));
        assert!(rows.is_whole(H));
        // the helper's second read of the same rows adds nothing
        let (new, first) = rows.mark_read(Source::Helper, whole, H);
        assert_eq!((new, first), (vec![], false));
    }
}
