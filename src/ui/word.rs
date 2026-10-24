//! Tap a word: the text lines OCR found around the tap, split into words; the
//! word under the tap is the selection, and a second tap on the same line
//! stretches it to a range. A press-drag from the highlight (or a Shift+click)
//! stretches it across lines in reading order, like selecting text in a
//! document: one highlight per line. All boxes are image px (the overlay at 1x).
use iced::{Point, Rectangle, Size, Vector};

use crate::ocr::TextBlock;

/// What OCR reads around a tap (image px): wide enough for the line around a word.
const REGION: Size = Size::new(720.0, 160.0);
/// The highlight's margin around the words (image px).
const PAD: f32 = 3.0;
/// A tap this far (image px) outside a line's box still hits it.
const SLACK: f32 = 4.0;
/// A drag's read band reaches this far (image px) above and below the range.
const MARGIN: f32 = 40.0;

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

/// More than half of the line's letters are from a right-to-left script
/// (Hebrew, Arabic, Syriac, Thaana, N'Ko, presentation forms).
fn is_rtl(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic());
    let (all, rtl) = letters.fold((0, 0), |(all, rtl), c| {
        let r = matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF);
        (all + 1, rtl + usize::from(r))
    });
    rtl * 2 > all
}

/// A line's words (in text order) and their boxes.
// ponytail: OCR boxes whole lines; a word's box is its share of the line by
// character count (a monospace guess, off by a few px on proportional fonts),
// counted from the right edge on an RTL line (OCR text is in logical order; a
// mixed Latin/Arabic line is approximate). Per-character boxes from the
// recognizer when that shows.
fn words(line: &Line) -> Vec<(Rectangle, &str)> {
    let count = line.text.chars().count().max(1) as f32;
    let per = line.bounds.width / count;
    let right = line.bounds.x + line.bounds.width;
    let rtl = is_rtl(&line.text);
    let mut out = Vec::new();
    let mut start = None;
    let ends = line.text.char_indices().map(|(i, c)| (i, Some(c)));
    for (n, (byte, c)) in ends.chain([(line.text.len(), None)]).enumerate() {
        match (start, c.is_some_and(|c| !c.is_whitespace())) {
            (None, true) => start = Some((n, byte)),
            (Some((first, from)), false) => {
                let width = (n - first) as f32 * per;
                let x = match rtl {
                    true => right - first as f32 * per - width,
                    false => line.bounds.x + first as f32 * per,
                };
                let size = Size::new(width, line.bounds.height);
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

/// Every word's highlight box in `lines` (select-text mode shows them all).
pub fn boxes(lines: &[Line]) -> Vec<Rectangle> {
    (lines.iter())
        .flat_map(|l| words(l).into_iter().map(|(r, _)| r.expand(PAD)))
        .collect()
}

/// The word of `line` nearest to `x` (the one that holds it, else by gap).
fn word_at(line: &Line, x: f32) -> Option<usize> {
    let gap = |r: &Rectangle| (r.x - x).max(x - r.x - r.width).max(0.0);
    let (word, _) = (words(line).iter().enumerate())
        .min_by(|(_, (a, _)), (_, (b, _))| gap(a).total_cmp(&gap(b)))?;
    Some(word)
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
    Some((line, word_at(&lines[line], p.x)?))
}

/// Like `hit`, for a drag: `p` need not be on a line, only within a line's
/// height of one (the line nearest by height, then by distance along it), so a
/// drag past the end of a line or into the gap between two still follows.
fn reach(lines: &[Line], p: Point) -> Option<(usize, usize)> {
    let off = |l: &Line| {
        let b = l.bounds;
        let along = (b.x - p.x).max(p.x - b.x - b.width).max(0.0);
        ((b.y - p.y).max(p.y - b.y - b.height).max(0.0), along)
    };
    let (line, (across, _)) = (lines.iter().enumerate())
        .filter(|(_, l)| !words(l).is_empty())
        .map(|(i, l)| (i, off(l)))
        .min_by(|(_, a), (_, b)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))?;
    (across <= lines[line].bounds.height).then_some((line, word_at(&lines[line], p.x)?))
}

/// The tapped or dragged words: a range in reading order, over one line or more.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    /// In reading order, as OCR returns them.
    lines: Vec<Line>,
    /// Image px that OCR read: `lines` is all the text in it.
    covered: Rectangle,
    /// The first word (line, word) and the latest end of the range; the range
    /// runs from the earlier of the two to the later, in reading order.
    anchor: (usize, usize),
    end: (usize, usize),
    /// Where they were pointed at (image px): the words are found again here
    /// when the lines are read anew.
    from: Point,
    to: Point,
}

impl Picked {
    /// The word under `p` in freshly read `lines` (all the text in `covered`);
    /// `None` if no line is there.
    pub fn new(lines: Vec<Line>, covered: Rectangle, p: Point) -> Option<Picked> {
        let at = hit(&lines, p)?;
        Some(Picked {
            lines,
            covered,
            anchor: at,
            end: at,
            from: p,
            to: p,
        })
    }

    /// Another tap, on the lines read before: on the same line it stretches the
    /// range from the first word to this one; on another line it picks that word.
    /// `false` = not on any known line (read again).
    pub fn tap(&mut self, p: Point) -> bool {
        let Some(at) = hit(&self.lines, p) else {
            return false;
        };
        if at.0 != self.anchor.0 {
            (self.anchor, self.from) = (at, p);
        }
        (self.end, self.to) = (at, p);
        true
    }

    /// A drag (or Shift+click) to `p`: the range now ends at the word nearest
    /// to it, on any line. `false` = no known line there (read the lines anew
    /// if `p` is outside `covers`).
    pub fn extend(&mut self, p: Point) -> bool {
        let Some(at) = reach(&self.lines, p) else {
            return false;
        };
        (self.end, self.to) = (at, p);
        true
    }

    /// One end of the range is held (a handle): the range's start if `start`,
    /// else its end. The other end becomes the anchor, so `extend` moves the
    /// held one.
    pub fn hold(&mut self, start: bool) {
        let forward = self.anchor <= self.end;
        if forward == start {
            (self.anchor, self.end) = (self.end, self.anchor);
            (self.from, self.to) = (self.to, self.from);
        }
    }

    /// OCR has read `lines` over `covered` (a wider band): the range is the same
    /// words, found again. If they are not there any more nothing changes.
    pub fn reread(&mut self, lines: Vec<Line>, covered: Rectangle) {
        if let (Some(anchor), Some(end)) = (reach(&lines, self.from), reach(&lines, self.to)) {
            (self.lines, self.covered, self.anchor, self.end) = (lines, covered, anchor, end);
        }
    }

    /// `p` (image px) is within the lines already read.
    pub fn covers(&self, p: Point) -> bool {
        self.covered.contains(p)
    }

    /// The band to read for a drag to `to`: the whole width of the `screen`,
    /// from above the range or `to` (whichever is higher) to below the other.
    pub fn band(&self, to: Point, screen: Size) -> Rectangle {
        let area = self.area();
        let top = (area.y.min(to.y) - MARGIN).max(0.0);
        let bottom = (area.y + area.height).max(to.y) + MARGIN;
        Rectangle::new(
            Point::new(0.0, top),
            Size::new(screen.width, bottom.min(screen.height) - top),
        )
    }

    /// The selected words per line: (line, first word, last word), in reading
    /// order. A line between the ends is skipped if it is in another column
    /// (it overlaps neither end line sideways).
    // ponytail: columns are guessed from line boxes only; a layout with
    // staggered columns can still pull in a neighbour's line.
    fn spans(&self) -> Vec<(usize, usize, usize)> {
        let (a, b) = (self.anchor.min(self.end), self.anchor.max(self.end));
        let sideways = |l: usize, e: usize| {
            let (x, y) = (self.lines[l].bounds, self.lines[e].bounds);
            x.x < y.x + y.width && y.x < x.x + x.width
        };
        (a.0..=b.0)
            .filter_map(|l| {
                let count = words(&self.lines[l]).len();
                let first = if l == a.0 { a.1 } else { 0 };
                let last = if l == b.0 { b.1 } else { count.checked_sub(1)? };
                let middle = l != a.0 && l != b.0;
                (!middle || sideways(l, a.0) || sideways(l, b.0)).then_some((l, first, last))
            })
            .collect()
    }

    /// The highlight: one box per line around its selected words, with a
    /// margin; where two lines' boxes overlap they meet in the middle.
    pub fn rects(&self) -> Vec<Rectangle> {
        let mut out: Vec<Rectangle> = (self.spans().into_iter())
            .filter_map(|(l, first, last)| {
                let words = words(&self.lines[l]);
                let mut boxes = words[first..=last].iter().map(|(r, _)| *r);
                let one = boxes.next()?;
                Some(boxes.fold(one, |all, r| all.union(&r)).expand(PAD))
            })
            .collect();
        for i in 1..out.len() {
            let (up, down) = (out[i - 1], out[i]);
            let sideways = up.x < down.x + down.width && down.x < up.x + up.width;
            let over = up.y + up.height - down.y;
            if sideways && over > 0.0 && down.y > up.y {
                let mid = down.y + over / 2.0;
                out[i - 1].height = mid - 0.5 - up.y;
                out[i].height = down.y + down.height - (mid + 0.5);
                out[i].y = mid + 0.5;
            }
        }
        out
    }

    /// `p` is on the highlight.
    pub fn on_text(&self, p: Point) -> bool {
        self.rects().iter().any(|r| r.contains(p))
    }

    /// The box around the whole highlight.
    pub fn area(&self) -> Rectangle {
        let rects = self.rects();
        let first = rects.first().copied().unwrap_or_default();
        rects.iter().fold(first, |all, r| all.union(r))
    }

    /// The selected words, a line break between lines.
    pub fn text(&self) -> String {
        let lines = self.spans().into_iter().map(|(l, first, last)| {
            let words = words(&self.lines[l]);
            let words: Vec<&str> = words[first..=last].iter().map(|(_, w)| *w).collect();
            words.join(" ")
        });
        lines.collect::<Vec<_>>().join("\n")
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

    const ALL: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 1000.0,
    };

    fn picked(lines: Vec<Line>, p: Point) -> Option<Picked> {
        Picked::new(lines, ALL, p)
    }

    fn sample() -> Vec<Line> {
        vec![
            line("Press  any key to start", 100.0, 100.0),
            line("Options", 100.0, 130.0),
        ]
    }

    /// A paragraph of three lines, 24 px apart (the boxes are 20 px tall).
    fn paragraph() -> Vec<Line> {
        vec![
            line("the quick brown fox", 100.0, 100.0),
            line("jumps over the lazy", 100.0, 124.0),
            line("dog and runs away", 100.0, 148.0),
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
        let p = picked(sample(), Point::new(185.0, 110.0)).unwrap();
        assert_eq!(p.text(), "any");
        let want = Rectangle::new(Point::new(170.0, 100.0), Size::new(30.0, 20.0));
        assert_eq!(p.area(), want.expand(PAD));
        // in the gap between words: the nearest one
        let p = picked(sample(), Point::new(162.0, 110.0)).unwrap();
        assert_eq!(p.text(), "any");
        // just under a line still hits it; far from any text hits nothing
        let p = picked(sample(), Point::new(105.0, 122.0)).unwrap();
        assert_eq!(p.text(), "Press");
        assert!(picked(sample(), Point::new(600.0, 110.0)).is_none());
        assert!(picked(sample(), Point::new(150.0, 300.0)).is_none());
        assert!(picked(Vec::new(), Point::ORIGIN).is_none());
    }

    #[test]
    fn a_second_tap_on_the_line_makes_a_range() {
        let mut p = picked(sample(), Point::new(185.0, 110.0)).unwrap(); // "any"
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
    fn a_drag_reads_across_lines_in_reading_order() {
        // line 1: the 100-130, quick 140-190, brown 200-250, fox 260-290
        // line 2: jumps 100-150, over 160-200, the 210-240, lazy 250-290
        // line 3: dog 100-130, and 140-170, runs 180-220, away 230-270
        let mut p = picked(paragraph(), Point::new(220.0, 110.0)).unwrap(); // brown
        assert!(p.extend(Point::new(270.0, 134.0))); // lazy
        assert_eq!(p.text(), "brown fox\njumps over the lazy");
        // on to line 3's "and": the middle line is whole
        assert!(p.extend(Point::new(155.0, 158.0)));
        assert_eq!(p.text(), "brown fox\njumps over the lazy\ndog and");
        // back to its own line, before the anchor: reading order kept
        assert!(p.extend(Point::new(105.0, 110.0)));
        assert_eq!(p.text(), "the quick brown");
        // up from the third line to the first
        let mut q = picked(paragraph(), Point::new(155.0, 158.0)).unwrap(); // and
        assert!(q.extend(Point::new(270.0, 110.0))); // fox
        assert_eq!(q.text(), "fox\njumps over the lazy\ndog and");
        // past the end of a line, or in the gap between lines, still follows
        assert!(q.extend(Point::new(800.0, 134.0)));
        assert_eq!(q.text(), "lazy\ndog and");
        assert!(q.extend(Point::new(150.0, 121.5)));
        assert!(
            q.text().starts_with("quick brown fox\njumps"),
            "{}",
            q.text()
        );
        // far from any line: unchanged
        let before = q.text();
        assert!(!q.extend(Point::new(150.0, 400.0)));
        assert_eq!(q.text(), before);
        // the same word twice is just that word
        let mut one = picked(paragraph(), Point::new(155.0, 158.0)).unwrap();
        assert!(one.extend(Point::new(155.0, 158.0)));
        assert_eq!(one.text(), "and");
    }

    #[test]
    fn a_held_end_moves_and_the_other_stays() {
        // "quick" to "lazy", picked either way round
        for (a, b) in [
            ((150.0, 110.0), (270.0, 134.0)),
            ((270.0, 134.0), (150.0, 110.0)),
        ] {
            let mut p = picked(paragraph(), Point::new(a.0, a.1)).unwrap();
            assert!(p.extend(Point::new(b.0, b.1)));
            assert_eq!(p.text(), "quick brown fox\njumps over the lazy");
            p.hold(true); // the start, to "the" on line 1
            assert!(p.extend(Point::new(105.0, 110.0)));
            assert_eq!(p.text(), "the quick brown fox\njumps over the lazy");
            p.hold(false); // the end, to "dog" on line 3
            assert!(p.extend(Point::new(105.0, 158.0)));
            assert_eq!(p.text(), "the quick brown fox\njumps over the lazy\ndog");
        }
    }

    #[test]
    fn the_highlight_is_one_box_per_line_and_never_overlaps() {
        let mut p = picked(paragraph(), Point::new(220.0, 110.0)).unwrap(); // brown
        assert!(p.extend(Point::new(155.0, 158.0))); // and, on line 3
        let rects = p.rects();
        assert_eq!(rects.len(), 3);
        let span = |r: &Rectangle| (r.x, r.x + r.width);
        assert_eq!(
            span(&rects[0]),
            (200.0 - PAD, 290.0 + PAD),
            "brown to the line end"
        );
        assert_eq!(
            span(&rects[1]),
            (100.0 - PAD, 290.0 + PAD),
            "the whole line"
        );
        assert_eq!(
            span(&rects[2]),
            (100.0 - PAD, 170.0 + PAD),
            "the line start to 'and'"
        );
        // 20 px boxes 24 px apart with a 3 px margin would overlap by 2: they meet instead
        for pair in rects.windows(2) {
            assert!(pair[0].y + pair[0].height < pair[1].y, "{pair:?}");
        }
        assert!(rects.iter().all(|r| r.height > 20.0));
        let area = p.area();
        assert_eq!((area.x, area.y), (100.0 - PAD, 100.0 - PAD));
        assert_eq!(area.y + area.height, 148.0 + 20.0 + PAD);
        // a press on a highlighted word is on the text; beside it is not
        assert!(p.on_text(Point::new(200.0, 134.0)));
        assert!(!p.on_text(Point::new(400.0, 134.0)));
        assert!(
            !p.on_text(Point::new(110.0, 110.0)),
            "line 1 starts at 'brown'"
        );
    }

    #[test]
    fn rtl_lines_read_right_to_left() {
        // OCR text is in logical order: the first word is the rightmost.
        let lines = vec![
            line("القائمة الرئيسية", 100.0, 100.0), // 7 + 8 chars: x 100..260
            line("الخيارات والإعدادات", 100.0, 124.0), // 8 + 10 chars: x 100..290
            line("خروج", 100.0, 148.0),             // x 100..140
        ];
        assert!(is_rtl(&lines[0].text) && !is_rtl("Press start خروج"));
        let w = words(&lines[0]);
        assert_eq!(w[0].1, "القائمة", "logical first word");
        assert_eq!(
            (w[0].0.x, w[0].0.x + w[0].0.width),
            (190.0, 260.0),
            "at the right end"
        );
        assert_eq!((w[1].0.x, w[1].0.x + w[1].0.width), (100.0, 180.0));
        // a tap at the right end of line 1 picks the first word
        let mut p = picked(lines.clone(), Point::new(250.0, 110.0)).unwrap();
        assert_eq!(p.text(), "القائمة");
        // down-left to line 2's leftmost (last) word: both lines whole, in logical order
        assert!(p.extend(Point::new(110.0, 134.0)));
        assert_eq!(p.text(), "القائمة الرئيسية\nالخيارات والإعدادات");
        let rects = p.rects();
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].x, 100.0 - PAD);
        // from line 1's left (second) word to line 2's first, which is its right end
        let mut q = picked(lines.clone(), Point::new(120.0, 110.0)).unwrap();
        assert_eq!(q.text(), "الرئيسية");
        assert!(q.extend(Point::new(270.0, 134.0)));
        assert_eq!(q.text(), "الرئيسية\nالخيارات");
        let rects = q.rects();
        assert_eq!(
            rects[0].x + rects[0].width,
            180.0 + PAD,
            "line 1: its second word only"
        );
        assert_eq!(
            rects[1].x,
            210.0 - PAD,
            "line 2: its first word, at the right end"
        );
        // a Latin line then an Arabic one: each line keeps its own direction
        let mixed = vec![line("Press start", 100.0, 100.0), lines[2].clone()];
        let mut m = picked(mixed, Point::new(105.0, 110.0)).unwrap();
        assert!(m.extend(Point::new(120.0, 158.0)));
        assert_eq!(m.text(), "Press start\nخروج");
    }

    #[test]
    fn a_drag_keeps_to_its_column() {
        // two columns, read row by row (left, right, left, right ...)
        let lines = vec![
            line("left one", 100.0, 100.0),
            line("right one", 500.0, 100.0),
            line("left two", 100.0, 124.0),
            line("right two", 500.0, 124.0),
            line("left three", 100.0, 148.0),
        ];
        let mut p = picked(lines, Point::new(110.0, 110.0)).unwrap();
        assert!(p.extend(Point::new(190.0, 158.0)));
        assert_eq!(p.text(), "left one\nleft two\nleft three");
    }

    #[test]
    fn rereading_a_wider_band_keeps_the_words() {
        let wide = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 400.0));
        let small = Rectangle::new(Point::new(0.0, 80.0), Size::new(1000.0, 80.0));
        let mut p = Picked::new(paragraph(), small, Point::new(220.0, 110.0)).unwrap(); // brown
        assert!(p.covers(Point::new(500.0, 120.0)));
        assert!(!p.covers(Point::new(500.0, 300.0)));
        // the band for a drag down to y = 300: whole width, from above the range to below the end
        let band = p.band(Point::new(300.0, 300.0), Size::new(1000.0, 600.0));
        assert_eq!((band.x, band.width), (0.0, 1000.0));
        assert_eq!(band.y, 100.0 - PAD - MARGIN);
        assert_eq!(band.y + band.height, 340.0);
        let top = p.band(Point::new(0.0, 0.0), Size::new(1000.0, 90.0));
        assert_eq!((top.y, top.height), (0.0, 90.0), "kept on screen");
        // read again: one more line at the bottom
        let mut lines = paragraph();
        lines.push(line("down here", 100.0, 300.0));
        p.reread(lines, wide);
        assert_eq!(p.text(), "brown");
        assert!(p.covers(Point::new(500.0, 300.0)));
        assert!(p.extend(Point::new(160.0, 310.0)));
        assert_eq!(
            p.text(),
            "brown fox\njumps over the lazy\ndog and runs away\ndown here"
        );
        // text no longer there: nothing changes
        p.reread(Vec::new(), wide);
        assert_eq!(p.text().lines().count(), 4);
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
