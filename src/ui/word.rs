//! Tap a word: the text lines OCR found around the tap, split into words; the
//! word under the tap is the selection, and a second tap on the same line
//! stretches it to a range. All boxes are image px (the overlay at 1x).
use iced::{Point, Rectangle, Size, Vector};

use crate::ocr::TextBlock;

/// What OCR reads around a tap (image px): wide enough for the line around a word.
const REGION: Size = Size::new(720.0, 160.0);
/// The highlight's margin around the words (image px).
const PAD: f32 = 3.0;
/// A tap this far (image px) outside a line's box still hits it.
const SLACK: f32 = 4.0;

/// One OCR line on the overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub bounds: Rectangle,
}

/// The part of a `screen`-sized overlay that OCR reads for a tap at `at`.
pub fn region(at: Point, screen: Size) -> Rectangle {
    let size = Size::new(
        REGION.width.min(screen.width),
        REGION.height.min(screen.height),
    );
    let x = (at.x - size.width / 2.0).clamp(0.0, screen.width - size.width);
    let y = (at.y - size.height / 2.0).clamp(0.0, screen.height - size.height);
    Rectangle::new(Point::new(x, y), size)
}

/// OCR blocks read from a crop whose top-left is `origin` (frame px), as lines
/// in image px.
pub fn lines(blocks: Vec<TextBlock>, origin: [u32; 2], scale: f32) -> Vec<Line> {
    let offset = Vector::new(origin[0] as f32, origin[1] as f32);
    (blocks.into_iter())
        .map(|b| {
            let [x, y, w, h] = b.bounds;
            let frame = Rectangle::new(Point::new(x, y) + offset, Size::new(w, h));
            Line {
                text: b.text,
                bounds: frame * (1.0 / scale),
            }
        })
        .collect()
}

/// A line's words and their boxes.
// ponytail: OCR boxes whole lines; a word's box is its share of the line by
// character count (a monospace guess, off by a few px on proportional fonts).
// Per-character boxes from the recognizer when that shows.
fn words(line: &Line) -> Vec<(Rectangle, &str)> {
    let count = line.text.chars().count().max(1) as f32;
    let per = line.bounds.width / count;
    let mut out = Vec::new();
    let mut start = None;
    let ends = line.text.char_indices().map(|(i, c)| (i, Some(c)));
    for (n, (byte, c)) in ends.chain([(line.text.len(), None)]).enumerate() {
        match (start, c.is_some_and(|c| !c.is_whitespace())) {
            (None, true) => start = Some((n, byte)),
            (Some((first, from)), false) => {
                let x = line.bounds.x + first as f32 * per;
                let size = Size::new((n - first) as f32 * per, line.bounds.height);
                out.push((
                    Rectangle::new(Point::new(x, line.bounds.y), size),
                    &line.text[from..byte],
                ));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The line and word under `p`: the line whose box (with `SLACK`) holds it
/// (the nearest by height if two do), the word that holds it or else the nearest.
fn hit(lines: &[Line], p: Point) -> Option<(usize, usize)> {
    let near = |l: &&Line| l.bounds.expand(SLACK).contains(p);
    let (line, _) =
        (lines.iter().enumerate())
            .filter(|(_, l)| near(l))
            .min_by(|(_, a), (_, b)| {
                let off = |l: &Line| (l.bounds.center_y() - p.y).abs();
                off(a).total_cmp(&off(b))
            })?;
    let gap = |r: &Rectangle| (r.x - p.x).max(p.x - r.x - r.width).max(0.0);
    let (word, _) = (words(&lines[line]).iter().enumerate())
        .min_by(|(_, (a, _)), (_, (b, _))| gap(a).total_cmp(&gap(b)))?;
    Some((line, word))
}

/// The tapped word(s): a range of words on one line.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    lines: Vec<Line>,
    line: usize,
    /// The first word tapped, and the last (either side of it).
    anchor: usize,
    end: usize,
}

impl Picked {
    /// The word under `p` in freshly read `lines`; `None` if no line is there.
    pub fn new(lines: Vec<Line>, p: Point) -> Option<Picked> {
        let (line, word) = hit(&lines, p)?;
        Some(Picked {
            lines,
            line,
            anchor: word,
            end: word,
        })
    }

    /// Another tap, on the lines read before: on the same line it stretches the
    /// range from the first word to this one; on another line it picks that word.
    /// `false` = not on any known line (read again).
    pub fn tap(&mut self, p: Point) -> bool {
        let Some((line, word)) = hit(&self.lines, p) else {
            return false;
        };
        if line != self.line {
            (self.line, self.anchor) = (line, word);
        }
        self.end = word;
        true
    }

    fn range(&self) -> Vec<(Rectangle, &str)> {
        let (a, b) = (self.anchor.min(self.end), self.anchor.max(self.end));
        let words = words(&self.lines[self.line]);
        words.into_iter().skip(a).take(b - a + 1).collect()
    }

    /// The highlight: the words' box with a margin.
    pub fn area(&self) -> Rectangle {
        let range = self.range();
        let first = range
            .first()
            .map_or(self.lines[self.line].bounds, |(r, _)| *r);
        let all = (range.iter()).fold(first, |all, (r, _)| all.union(r));
        all.expand(PAD)
    }

    pub fn text(&self) -> String {
        let range = self.range();
        range.iter().map(|(_, w)| *w).collect::<Vec<_>>().join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f32, y: f32) -> Line {
        // 10 px per character, 20 px tall
        let w = text.chars().count() as f32 * 10.0;
        Line {
            text: text.into(),
            bounds: Rectangle::new(Point::new(x, y), Size::new(w, 20.0)),
        }
    }

    fn sample() -> Vec<Line> {
        vec![
            line("Press  any key to start", 100.0, 100.0),
            line("Options", 100.0, 130.0),
        ]
    }

    #[test]
    fn words_get_their_share_of_the_line() {
        let l = line("Press  any key", 0.0, 0.0);
        let w = words(&l);
        let texts: Vec<_> = w.iter().map(|(_, t)| *t).collect();
        assert_eq!(texts, ["Press", "any", "key"]);
        assert_eq!(w[0].0, Rectangle::new(Point::ORIGIN, Size::new(50.0, 20.0)));
        assert_eq!(w[1].0.x, 70.0, "two spaces skipped");
        assert_eq!(w[2].0.x + w[2].0.width, l.bounds.width);
        assert!(words(&line("", 0.0, 0.0)).is_empty());
        // no spaces (CJK): the whole line is one word
        assert_eq!(words(&line("日本語", 0.0, 0.0)).len(), 1);
    }

    #[test]
    fn a_tap_picks_the_word_under_it() {
        // "any" spans x 170..200 on the first line
        let p = Picked::new(sample(), Point::new(185.0, 110.0)).unwrap();
        assert_eq!(p.text(), "any");
        let want = Rectangle::new(Point::new(170.0, 100.0), Size::new(30.0, 20.0));
        assert_eq!(p.area(), want.expand(PAD));
        // in the gap between words: the nearest one
        let p = Picked::new(sample(), Point::new(162.0, 110.0)).unwrap();
        assert_eq!(p.text(), "any");
        // just under a line still hits it; far from any text hits nothing
        let p = Picked::new(sample(), Point::new(105.0, 122.0)).unwrap();
        assert_eq!(p.text(), "Press");
        assert!(Picked::new(sample(), Point::new(600.0, 110.0)).is_none());
        assert!(Picked::new(sample(), Point::new(150.0, 300.0)).is_none());
        assert!(Picked::new(Vec::new(), Point::ORIGIN).is_none());
    }

    #[test]
    fn a_second_tap_on_the_line_makes_a_range() {
        let mut p = Picked::new(sample(), Point::new(185.0, 110.0)).unwrap(); // "any"
        assert!(p.tap(Point::new(300.0, 110.0))); // "start"
        assert_eq!(p.text(), "any key to start");
        // backwards from the first word
        assert!(p.tap(Point::new(110.0, 110.0)));
        assert_eq!(p.text(), "Press any");
        assert_eq!(p.area().x, 100.0 - PAD);
        // another line: a new single word there
        assert!(p.tap(Point::new(120.0, 140.0)));
        assert_eq!(p.text(), "Options");
        // off the known lines: read again
        assert!(!p.tap(Point::new(900.0, 900.0)));
        assert_eq!(p.text(), "Options", "unchanged");
    }

    #[test]
    fn ocr_boxes_map_to_image_px() {
        let block = TextBlock {
            text: "Hi".into(),
            bounds: [10.0, 4.0, 40.0, 20.0],
            confidence: 1.0,
        };
        let got = lines(vec![block], [200, 100], 2.0);
        let want = Rectangle::new(Point::new(105.0, 52.0), Size::new(20.0, 10.0));
        assert_eq!(got[0].bounds, want);
    }

    #[test]
    fn the_read_region_stays_on_screen() {
        let screen = Size::new(1920.0, 1080.0);
        let r = region(Point::new(960.0, 540.0), screen);
        assert_eq!(r.center(), Point::new(960.0, 540.0));
        let r = region(Point::new(5.0, 1075.0), screen);
        assert_eq!((r.x, r.y + r.height), (0.0, 1080.0));
        let tiny = region(Point::new(5.0, 5.0), Size::new(300.0, 100.0));
        assert_eq!(tiny, Rectangle::new(Point::ORIGIN, Size::new(300.0, 100.0)));
    }
}
