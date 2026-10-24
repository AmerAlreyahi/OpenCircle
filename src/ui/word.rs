//! Selecting text on the frozen frame like in a document or a PDF viewer: the
//! text lines OCR found, each with a caret position between every two
//! characters (measured on the frame where it can be). A press-drag selects
//! from the caret under the press to the caret under the pointer, in reading
//! order across lines; a double click selects a word, a triple click the line,
//! Ctrl+A everything. The highlight is one flat box per line: from the caret
//! to the line's end on the first line, whole middle lines, the line's start
//! to the caret on the last. All boxes are image px (the overlay at 1x).
use iced::{Point, Rectangle, Size, Vector};

use crate::ocr::TextBlock;

/// What OCR reads around a tap (image px): wide enough for the line around a word.
const REGION: Size = Size::new(720.0, 160.0);
/// The highlight's margin above and below the line (image px); select-text
/// mode's word boxes get it all round.
const PAD: f32 = 3.0;
/// A press this far (image px) outside a line's box still hits it.
const SLACK: f32 = 4.0;
/// A drag's read band reaches this far (image px) above and below the range.
const MARGIN: f32 = 40.0;
/// A pixel this far (largest channel difference) from the background is ink.
const INK: i32 = 56;

/// One OCR line on the overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub bounds: Rectangle,
    /// The x of every caret position (image px): before the first character,
    /// between each two, after the last (`chars + 1` of them, decreasing on an
    /// RTL line). Measured on the frame by `measure`; empty = an even share.
    pub carets: Vec<f32>,
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
                carets: Vec::new(),
            }
        })
        .collect()
}

/// Finds where the characters of `lines` really are on the frame (`rgba`,
/// `width` x `height` frame px, `scale` frame px per image px). In a line's
/// box the columns without ink (pixels unlike its most common colour) split
/// it: the widest `words - 1` gaps are the spaces, and inside a word whose ink
/// falls in exactly one piece per character the gaps are the caret positions
/// (else its width is shared by its characters). A line it can't split that
/// way (fewer gaps than spaces) keeps the even share.
// ponytail: one background colour per line (text over a gradient or a picture
// finds ink everywhere and keeps the even share); characters that touch
// (a tight "rn") share their word's width; RTL words share it right to left.
pub fn measure(lines: &mut [Line], rgba: &[u8], width: u32, height: u32, scale: f32) {
    for line in lines {
        line.carets = on_frame(line, rgba, width, height, scale).unwrap_or_default();
    }
}

/// `measure` for one line.
fn on_frame(line: &Line, rgba: &[u8], width: u32, height: u32, scale: f32) -> Option<Vec<f32>> {
    let b = line.bounds * scale;
    let px = |v: f32, max: u32| (v.max(0.0) as u32).min(max);
    let (x0, x1) = (px(b.x.floor(), width), px((b.x + b.width).ceil(), width));
    let (y0, y1) = (px(b.y.floor(), height), px((b.y + b.height).ceil(), height));
    if x1 <= x0 || y1 <= y0 || rgba.len() < (width * height * 4) as usize {
        return None;
    }
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    };
    // the background: the most common colour (4 bits a channel)
    let mut counts = std::collections::HashMap::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let [r, g, b] = at(x, y);
            *counts.entry([r >> 4, g >> 4, b >> 4]).or_insert(0u32) += 1;
        }
    }
    let (&bg, _) = counts.iter().max_by_key(|(_, n)| **n)?;
    let bg = bg.map(|c| i32::from(c) * 16 + 8);
    let ink: Vec<bool> = (x0..x1)
        .map(|x| {
            (y0..y1).any(|y| {
                let p = at(x, y);
                (0..3).any(|c| (i32::from(p[c]) - bg[c]).abs() > INK)
            })
        })
        .collect();
    let first = ink.iter().position(|&i| i)?;
    let last = ink.iter().rposition(|&i| i)?;
    let gaps_in = |from: usize, to: usize| {
        let mut gaps: Vec<(usize, usize)> = Vec::new();
        let mut run = None;
        for (i, &on) in ink.iter().enumerate().take(to).skip(from) {
            match (on, run) {
                (false, None) => run = Some(i),
                (true, Some(start)) => {
                    gaps.push((start, i - start));
                    run = None;
                }
                _ => {}
            }
        }
        gaps
    };
    let spans = word_spans(&line.text);
    let mut gaps = gaps_in(first, last + 1);
    if spans.is_empty() || gaps.len() + 1 < spans.len() {
        return None;
    }
    gaps.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut spaces: Vec<(usize, usize)> = gaps[..spans.len() - 1].to_vec();
    spaces.sort();
    let mut edges = vec![first];
    for (start, len) in spaces {
        edges.extend([start, start + len]);
    }
    edges.push(last + 1);
    // each word's ink, left to right: (from, to) columns
    let mut inks: Vec<(usize, usize)> = edges.chunks(2).map(|e| (e[0], e[1])).collect();
    let rtl = is_rtl(&line.text);
    if rtl {
        inks.reverse(); // the first word is the rightmost
    }
    let to_image = |col: f32| (x0 as f32 + col) / scale;
    let count = line.text.chars().count();
    let mut carets: Vec<Option<f32>> = vec![None; count + 1];
    for (&(c0, c1, ..), &(from, to)) in spans.iter().zip(&inks) {
        let m = c1 - c0;
        let chars: Vec<char> = line.text.chars().skip(c0).take(m).collect();
        let gaps: Vec<f32> = (gaps_in(from, to).iter())
            .map(|&(s, len)| s as f32 + len as f32 / 2.0)
            .collect();
        let inside: Vec<f32> = if rtl {
            let share = |j: usize| (to - from) as f32 * j as f32 / m as f32;
            (1..m).map(|j| to as f32 - share(j)).collect()
        } else if gaps.len() + 1 == m {
            gaps // one piece of ink per character
        } else {
            split(&chars, from as f32, to as f32, &gaps)
        };
        let (start, end) = if rtl { (to, from) } else { (from, to) };
        carets[c0] = Some(to_image(start as f32));
        carets[c1] = Some(to_image(end as f32));
        for (j, x) in inside.into_iter().enumerate() {
            carets[c0 + 1 + j] = Some(to_image(x));
        }
    }
    // spaces between words share their gap; before the first word and after
    // the last, the carets sit on its edge
    let known: Vec<usize> = (0..=count).filter(|&i| carets[i].is_some()).collect();
    (0..=count)
        .map(|i| {
            let before = known.iter().rev().find(|&&k| k <= i);
            let after = known.iter().find(|&&k| k >= i);
            match (before, after) {
                (Some(&a), Some(&b)) if a != b => {
                    let (xa, xb) = (carets[a]?, carets[b]?);
                    Some(xa + (xb - xa) * (i - a) as f32 / (b - a) as f32)
                }
                (Some(&k), _) | (_, Some(&k)) => carets[k],
                (None, None) => None,
            }
        })
        .collect()
}

/// How wide a character usually is in a proportional Latin font (em).
// ponytail: one table for every font; only a guess where ink doesn't separate
// the characters (kerned or touching letters), snapped to real gaps nearby.
fn width_of(c: char) -> f32 {
    match c {
        'i' | 'j' | 'l' | '!' | '\'' | '.' | ',' | ':' | ';' | '|' | 'I' => 0.27,
        'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '-' => 0.36,
        'm' | 'w' | 'M' | 'W' => 0.88,
        c if c.is_uppercase() => 0.66,
        _ => 0.54,
    }
}

/// The caret positions inside a word whose ink runs `from`..`to` (columns),
/// when its ink doesn't fall in one piece per character: each character gets
/// its usual share of the width (`width_of`), and a caret near the middle of a
/// real gap (`gaps`) moves there.
fn split(chars: &[char], from: f32, to: f32, gaps: &[f32]) -> Vec<f32> {
    let total: f32 = chars.iter().map(|&c| width_of(c)).sum();
    let per = (to - from) / total.max(f32::EPSILON);
    let mut out = Vec::new();
    let mut x = from;
    let mut last = from;
    for &c in &chars[..chars.len().saturating_sub(1)] {
        x += width_of(c) * per;
        let near = (gaps.iter())
            .filter(|&&g| g > last)
            .min_by(|a, b| (*a - x).abs().total_cmp(&(*b - x).abs()))
            .filter(|&&g| (g - x).abs() <= 0.35 * width_of(c) * per);
        let caret = near.copied().unwrap_or(x);
        out.push(caret);
        last = caret;
    }
    out
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

/// The words of `text`: (first char, end char, first byte, end byte).
fn word_spans(text: &str) -> Vec<(usize, usize, usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let ends = text.char_indices().map(|(i, c)| (i, Some(c)));
    for (n, (byte, c)) in ends.chain([(text.len(), None)]).enumerate() {
        match (start, c.is_some_and(|c| !c.is_whitespace())) {
            (None, true) => start = Some((n, byte)),
            (Some((first, from)), false) => {
                out.push((first, n, from, byte));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The x of each caret position of `line` (see `Line::carets`): measured, or
/// an even share of the box per character, from the right on an RTL line.
fn carets(line: &Line) -> Vec<f32> {
    let count = line.text.chars().count();
    if line.carets.len() == count + 1 {
        return line.carets.clone();
    }
    let per = line.bounds.width / count.max(1) as f32;
    let (left, right) = (line.bounds.x, line.bounds.x + line.bounds.width);
    let rtl = is_rtl(&line.text);
    (0..=count)
        .map(|i| {
            if rtl {
                right - per * i as f32
            } else {
                left + per * i as f32
            }
        })
        .collect()
}

/// The box (line height) between carets `a` and `b` of `line`.
fn between(line: &Line, carets: &[f32], a: usize, b: usize) -> Rectangle {
    let (xa, xb) = (carets[a], carets[b]);
    Rectangle::new(
        Point::new(xa.min(xb), line.bounds.y),
        Size::new((xa - xb).abs(), line.bounds.height),
    )
}

/// A line's words (in text order) and their boxes.
fn words(line: &Line) -> Vec<(Rectangle, &str)> {
    let carets = carets(line);
    (word_spans(&line.text).into_iter())
        .map(|(c0, c1, b0, b1)| (between(line, &carets, c0, c1), &line.text[b0..b1]))
        .collect()
}

/// Every word's box in `lines` (select-text mode shows them all).
pub fn boxes(lines: &[Line]) -> Vec<Rectangle> {
    (lines.iter())
        .flat_map(|l| words(l).into_iter().map(|(r, _)| r.expand(PAD)))
        .collect()
}

/// The caret position of `line` nearest to `x`.
fn caret_at(line: &Line, x: f32) -> usize {
    let carets = carets(line);
    (0..carets.len())
        .min_by(|&a, &b| (carets[a] - x).abs().total_cmp(&(carets[b] - x).abs()))
        .unwrap_or(0)
}

/// The line under `p`: its box (with `SLACK`) holds it (the nearest by height
/// if two do).
fn line_at(lines: &[Line], p: Point) -> Option<usize> {
    let near = |l: &&Line| l.bounds.expand(SLACK).contains(p) && !l.text.trim().is_empty();
    let off = |l: &Line| (l.bounds.center_y() - p.y).abs();
    let (line, _) = (lines.iter().enumerate())
        .filter(|(_, l)| near(l))
        .min_by(|(_, a), (_, b)| off(a).total_cmp(&off(b)))?;
    Some(line)
}

/// The line and caret for a drag at `p`: `p` need not be on a line, only within
/// a line's height of one (the line nearest by height, then by distance along
/// it), so a drag past the end of a line or into the gap between two follows.
fn reach(lines: &[Line], p: Point) -> Option<(usize, usize)> {
    let off = |l: &Line| {
        let b = l.bounds;
        let along = (b.x - p.x).max(p.x - b.x - b.width).max(0.0);
        ((b.y - p.y).max(p.y - b.y - b.height).max(0.0), along)
    };
    let (line, (across, _)) = (lines.iter().enumerate())
        .filter(|(_, l)| !l.text.trim().is_empty())
        .map(|(i, l)| (i, off(l)))
        .min_by(|(_, a), (_, b)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))?;
    (across <= lines[line].bounds.height).then(|| (line, caret_at(&lines[line], p.x)))
}

/// What a press selects at first: nothing yet (a caret: one click), the word
/// (a double click) or the line (a triple click).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Caret,
    Word,
    Line,
}

/// The selected text: from one caret position to another (line, caret), in
/// reading order, over one line or more.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    /// In reading order, as OCR returns them.
    lines: Vec<Line>,
    /// Image px that OCR read: `lines` is all the text in it.
    covered: Rectangle,
    /// Where the selection began and where it ends now; it runs from the
    /// earlier of the two to the later, in reading order.
    anchor: (usize, usize),
    end: (usize, usize),
    /// Where they were pointed at (image px): the text is found again here
    /// when the lines are read anew.
    from: Point,
    to: Point,
    /// Everything (Ctrl+A): lines in other columns too.
    all: bool,
}

impl Picked {
    /// A press at `p` on freshly read `lines` (all the text in `covered`): the
    /// caret there, or its word or line. `None` if no line is there.
    pub fn new(lines: Vec<Line>, covered: Rectangle, p: Point, unit: Unit) -> Option<Picked> {
        let line = line_at(&lines, p)?;
        let caret = caret_at(&lines[line], p.x);
        let (a, b) = match unit {
            Unit::Caret => (caret, caret),
            Unit::Line => (0, lines[line].text.chars().count()),
            Unit::Word => {
                let gap = |r: &Rectangle| (r.x - p.x).max(p.x - r.x - r.width).max(0.0);
                let spans = word_spans(&lines[line].text);
                let boxes = words(&lines[line]);
                let (k, _) = (boxes.iter().enumerate())
                    .min_by(|(_, (a, _)), (_, (b, _))| gap(a).total_cmp(&gap(b)))?;
                (spans[k].0, spans[k].1)
            }
        };
        Some(Picked {
            lines,
            covered,
            anchor: (line, a),
            end: (line, b),
            from: p,
            to: p,
            all: false,
        })
    }

    /// All the text in `lines` (Ctrl+A); `None` if there is none.
    pub fn all(lines: Vec<Line>, covered: Rectangle) -> Option<Picked> {
        let last = lines.len().checked_sub(1)?;
        let end = (last, lines[last].text.chars().count());
        let corner = |r: Rectangle| [r.position(), r.position() + Vector::from(r.size())];
        let ([from, _], [_, to]) = (corner(lines[0].bounds), corner(lines[last].bounds));
        Some(Picked {
            lines,
            covered,
            anchor: (0, 0),
            end,
            from,
            to,
            all: true,
        })
    }

    /// A drag (or Shift+click) to `p`: the selection now ends at the caret
    /// nearest to it, on any line. `false` = no known line there (read the
    /// lines anew if `p` is outside `covers`).
    pub fn extend(&mut self, p: Point) -> bool {
        let Some(at) = reach(&self.lines, p) else {
            return false;
        };
        (self.end, self.to, self.all) = (at, p, false);
        true
    }

    /// OCR has read `lines` over `covered` (a wider band): the selection is the
    /// same text, found again. If it is not there any more nothing changes.
    pub fn reread(&mut self, lines: Vec<Line>, covered: Rectangle) {
        let remap = |(_, caret): (usize, usize), p: Point| {
            let (line, _) = reach(&lines, p)?;
            Some((line, caret.min(lines[line].text.chars().count())))
        };
        if let (Some(anchor), Some(end)) = (remap(self.anchor, self.from), remap(self.end, self.to))
        {
            (self.lines, self.covered, self.anchor, self.end) = (lines, covered, anchor, end);
        }
    }

    /// Nothing selected yet (a click, not a drag).
    pub fn is_empty(&self) -> bool {
        self.anchor == self.end
    }

    /// Every text line read around the selection (image px).
    pub fn line_boxes(&self) -> impl Iterator<Item = Rectangle> + '_ {
        self.lines.iter().map(|l| l.bounds)
    }

    /// `p` (image px) is within the lines already read.
    pub fn covers(&self, p: Point) -> bool {
        self.covered.contains(p)
    }

    /// The band to read for a drag to `to`: the whole width of the `screen`,
    /// from above the selection or `to` (whichever is higher) to below the other.
    pub fn band(&self, to: Point, screen: Size) -> Rectangle {
        let area = self.area();
        let area = if self.is_empty() {
            Rectangle::new(self.from, Size::ZERO)
        } else {
            area
        };
        let top = (area.y.min(to.y) - MARGIN).max(0.0);
        let bottom = (area.y + area.height).max(to.y) + MARGIN;
        Rectangle::new(
            Point::new(0.0, top),
            Size::new(screen.width, bottom.min(screen.height) - top),
        )
    }

    /// The selected part of each line: (line, first caret, last caret), in
    /// reading order. A line between the ends is skipped if it is in another
    /// column (it overlaps neither end line sideways), unless all is selected.
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
                let count = self.lines[l].text.chars().count();
                let first = if l == a.0 { a.1 } else { 0 };
                let last = if l == b.0 { b.1 } else { count };
                let middle = l != a.0 && l != b.0;
                let shown = self.all || !middle || sideways(l, a.0) || sideways(l, b.0);
                (shown && first < last).then_some((l, first, last))
            })
            .collect()
    }

    /// The highlight: one flat box per line from the first selected caret to
    /// the last (whole middle lines), the line's height and a little; where
    /// two lines' boxes overlap they meet in the middle.
    pub fn rects(&self) -> Vec<Rectangle> {
        let pad = Vector::new(0.0, PAD);
        let mut out: Vec<Rectangle> = (self.spans().into_iter())
            .map(|(l, first, last)| {
                let line = &self.lines[l];
                let r = between(line, &carets(line), first, last);
                Rectangle::new(r.position() - pad, r.size() + Size::new(0.0, 2.0 * PAD))
            })
            .collect();
        for i in 1..out.len() {
            let (up, down) = (out[i - 1], out[i]);
            let sideways = up.x < down.x + down.width && down.x < up.x + up.width;
            let over = up.y + up.height - down.y;
            if sideways && over > 0.0 && down.y > up.y {
                let mid = down.y + over / 2.0;
                out[i - 1].height = mid - up.y;
                out[i].height = down.y + down.height - mid;
                out[i].y = mid;
            }
        }
        out
    }

    /// The box around the whole highlight.
    pub fn area(&self) -> Rectangle {
        let rects = self.rects();
        let first = rects.first().copied().unwrap_or_default();
        rects.iter().fold(first, |all, r| all.union(r))
    }

    /// The selected text, a line break between lines.
    pub fn text(&self) -> String {
        let lines = self.spans().into_iter().map(|(l, first, last)| {
            let text = &self.lines[l].text;
            let part: String = text.chars().skip(first).take(last - first).collect();
            part.trim().to_string()
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
            carets: Vec::new(),
        }
    }

    const ALL: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 1000.0,
    };

    fn press(lines: Vec<Line>, p: Point, unit: Unit) -> Option<Picked> {
        Picked::new(lines, ALL, p, unit)
    }

    /// A paragraph of three lines, 24 px apart (the boxes are 20 px tall).
    /// Line 1: char i spans x 100 + 10 i .. +10.
    fn paragraph() -> Vec<Line> {
        vec![
            line("the quick brown fox", 100.0, 100.0),
            line("jumps over the lazy", 100.0, 124.0),
            line("dog and runs away", 100.0, 148.0),
        ]
    }

    /// The caret before char `c` of paragraph line `l` (image px, mid-height).
    fn at(l: usize, c: f32) -> Point {
        Point::new(100.0 + 10.0 * c, 110.0 + 24.0 * l as f32)
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
    fn a_drag_selects_from_caret_to_caret() {
        // a click is a caret: nothing selected, no highlight
        let mut p = press(paragraph(), at(0, 5.4), Unit::Caret).unwrap();
        assert!(p.is_empty() && p.rects().is_empty() && p.text().is_empty());
        // to the middle of "brown": "uick br", not whole words
        assert!(p.extend(at(0, 12.3)));
        assert_eq!(p.text(), "uick br");
        let r = p.rects();
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].x, r[0].x + r[0].width), (150.0, 220.0));
        assert_eq!((r[0].y, r[0].height), (100.0 - PAD, 20.0 + 2.0 * PAD));
        // backwards from the press: the press is still one end
        assert!(p.extend(at(0, 1.2)));
        assert_eq!(p.text(), "he q");
        // down to line 3: line 1 to its end, line 2 whole, line 3 to the caret
        assert!(p.extend(at(2, 6.0)));
        assert_eq!(p.text(), "uick brown fox\njumps over the lazy\ndog an");
        let r = p.rects();
        let span = |r: &Rectangle| (r.x, r.x + r.width);
        assert_eq!(span(&r[0]), (150.0, 290.0));
        assert_eq!(span(&r[1]), (100.0, 290.0), "the whole middle line");
        assert_eq!(span(&r[2]), (100.0, 160.0));
        for pair in r.windows(2) {
            assert!(pair[0].y + pair[0].height <= pair[1].y, "{pair:?}");
        }
        // past a line's end, or in the gap between lines, still follows
        assert!(p.extend(Point::new(800.0, 134.0)));
        assert_eq!(p.text(), "uick brown fox\njumps over the lazy");
        // far from any line: unchanged
        assert!(!p.extend(Point::new(150.0, 400.0)));
        // nothing under the press: no selection
        assert!(press(paragraph(), Point::new(600.0, 110.0), Unit::Caret).is_none());
    }

    #[test]
    fn double_and_triple_clicks_and_select_all() {
        let word = press(paragraph(), at(1, 7.5), Unit::Word).unwrap();
        assert_eq!(word.text(), "over");
        let r = word.rects();
        assert_eq!((r[0].x, r[0].width), (160.0, 40.0));
        // a word, then dragged on
        let mut w = word.clone();
        assert!(w.extend(at(2, 2.0)));
        assert_eq!(w.text(), "over the lazy\ndo");
        let line = press(paragraph(), at(2, 9.0), Unit::Line).unwrap();
        assert_eq!(line.text(), "dog and runs away");
        // everything, in reading order, even a line in another column
        let mut lines = paragraph();
        lines.push(line_at_x("a side note", 600.0, 124.0));
        let all = Picked::all(lines, ALL).unwrap();
        assert_eq!(
            all.text(),
            "the quick brown fox\njumps over the lazy\ndog and runs away\na side note"
        );
        assert!(Picked::all(Vec::new(), ALL).is_none());
    }

    fn line_at_x(text: &str, x: f32, y: f32) -> Line {
        line(text, x, y)
    }

    #[test]
    fn rtl_lines_run_right_to_left() {
        // OCR text is in logical order: the first character is the rightmost.
        let lines = vec![
            line("القائمة الرئيسية", 100.0, 100.0), // 16 chars: x 100..260
            line("خروج", 100.0, 124.0),             // x 100..140
        ];
        assert!(is_rtl(&lines[0].text) && !is_rtl("Press start خروج"));
        let w = words(&lines[0]);
        assert_eq!(w[0].1, "القائمة", "logical first word");
        assert_eq!((w[0].0.x, w[0].0.x + w[0].0.width), (190.0, 260.0));
        // a double click at the right end selects the first word
        let p = press(lines.clone(), Point::new(250.0, 110.0), Unit::Word).unwrap();
        assert_eq!(p.text(), "القائمة");
        // from the right end down-left to the next line's left end: both whole
        let mut q = press(lines, Point::new(259.0, 110.0), Unit::Caret).unwrap();
        assert!(q.extend(Point::new(101.0, 134.0)));
        assert_eq!(q.text(), "القائمة الرئيسية\nخروج");
        assert_eq!(q.rects()[0].x, 100.0);
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
        let mut p = press(lines, Point::new(100.0, 110.0), Unit::Caret).unwrap();
        assert!(p.extend(Point::new(200.0, 158.0)));
        assert_eq!(p.text(), "left one\nleft two\nleft three");
    }

    #[test]
    fn rereading_a_wider_band_keeps_the_selection() {
        let wide = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 400.0));
        let small = Rectangle::new(Point::new(0.0, 80.0), Size::new(1000.0, 80.0));
        let mut p = Picked::new(paragraph(), small, at(0, 10.0), Unit::Word).unwrap(); // brown
        assert!(p.covers(Point::new(500.0, 120.0)));
        assert!(!p.covers(Point::new(500.0, 300.0)));
        // the band for a drag down to y = 300: whole width, from above the selection to below the end
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
        assert!(p.covers(Point::new(500.0, 300.0)));
        assert!(p.extend(Point::new(140.0, 310.0)));
        assert_eq!(
            p.text(),
            "brown fox\njumps over the lazy\ndog and runs away\ndown"
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

    const WORDS: [&str; 8] = [
        "Will", "a", "quick", "Wolf,", "jumping", "lightly,", "mimic", "it?",
    ];

    /// A line of `WORDS` in a proportional font (960x60 frame px at `scale`),
    /// with only the characters for which `shown(word, char)` drawn; the
    /// layout is the same whatever is shown.
    fn proportional(shown: impl Fn(usize, usize) -> bool, scale: f32) -> Vec<u8> {
        use iced::widget::{container, rich_text, span};
        let colour = |on: bool| {
            if on {
                iced::Color::WHITE
            } else {
                iced::Color::TRANSPARENT
            }
        };
        let mut spans = Vec::new();
        for (i, w) in WORDS.iter().enumerate() {
            for (j, c) in w.chars().enumerate() {
                spans.push(span::<(), iced::Font>(c.to_string()).color(colour(shown(i, j))));
            }
            spans.push(span(" "));
        }
        let line = rich_text(spans)
            .size(22.0 * scale)
            .font(iced::Font::DEFAULT);
        let page = container(line).padding([12.0 * scale, 30.0 * scale]);
        let view: iced::Element<'_, ()> = page.into();
        crate::ui::widgets::shot::render(view, (960.0 * scale) as u32, (60.0 * scale) as u32)
    }

    /// The columns (frame px) with ink in `rgba` (960 x 60 at `scale`).
    fn ink(rgba: &[u8], scale: f32) -> Option<(f32, f32)> {
        let w = (960.0 * scale) as usize;
        let on = |x: usize| (0..rgba.len() / 4 / w).any(|y| rgba[(y * w + x) * 4] > 128);
        let first = (0..w).find(|&x| on(x))?;
        let last = (0..w).rev().find(|&x| on(x))?;
        Some((first as f32, last as f32 + 1.0))
    }

    #[test]
    fn words_and_characters_are_measured_where_they_really_are() {
        for scale in [1.0, 1.5] {
            let all = proportional(|_, _| true, scale);
            let (x0, x1) = ink(&all, scale).unwrap();
            // OCR's box: a few px of margin round the ink
            let bounds = Rectangle::new(
                Point::new(x0 / scale - 3.0, 8.0),
                Size::new((x1 - x0) / scale + 6.0, 40.0),
            );
            let mut lines = vec![Line {
                text: WORDS.join(" "),
                bounds,
                carets: Vec::new(),
            }];
            let even = words(&lines[0])
                .iter()
                .map(|(r, _)| r.x)
                .collect::<Vec<_>>();
            let (w, h) = ((960.0 * scale) as u32, (60.0 * scale) as u32);
            measure(&mut lines, &all, w, h, scale);
            let got = words(&lines[0]);
            assert_eq!(got.len(), WORDS.len());
            let carets = carets(&lines[0]);
            let mut c = 0; // the first char of word `k` in the line
            for (k, (r, text)) in got.iter().enumerate() {
                assert_eq!(*text, WORDS[k]);
                let (a, b) = ink(&proportional(|i, _| i == k, scale), scale).unwrap();
                let (a, b) = (a / scale, b / scale);
                assert!(
                    (r.x - a).abs() <= 2.0 && (r.x + r.width - b).abs() <= 2.0,
                    "{scale}x {text}: {:?} vs real {a}..{b}",
                    (r.x, r.x + r.width)
                );
                // each caret inside the word sits between its two characters
                for j in 1..text.chars().count() {
                    let left = ink(&proportional(|i, n| i == k && n == j - 1, scale), scale);
                    let right = ink(&proportional(|i, n| i == k && n == j, scale), scale);
                    let (Some((_, end)), Some((start, _))) = (left, right) else {
                        continue;
                    };
                    // touching letters (a kerned "lf") have no gap between them: their
                    // caret is the usual width split, within 3 px
                    let mid = (end + start) / 2.0 / scale;
                    let x = carets[c + j];
                    assert!(
                        (x - mid).abs() <= 3.0,
                        "{scale}x {text} caret {j}: {x} vs real {mid}"
                    );
                }
                c += text.chars().count() + 1;
            }
            // the even share is off by more than that on this line
            let off = (even.iter().zip(&got)).any(|(e, (m, _))| (e - m.x).abs() > 4.0);
            assert!(off, "the fixture must tell the two apart");
        }
        // a busy background (no gaps): the even share stays
        let mut busy = vec![line("Will a quick", 0.0, 0.0)];
        let noise: Vec<u8> = (0..100 * 20)
            .flat_map(|i| [(i * 37 % 256) as u8, 0, 0, 255])
            .collect();
        measure(&mut busy, &noise, 100, 20, 1.0);
        assert!(busy[0].carets.is_empty());
    }
}
