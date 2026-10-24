//! Full-screen translate, image side: group OCR lines into blocks (the AI
//! gets whole sentences), erase each block's glyphs from the screenshot and
//! draw the translation in their place, never a box over them.
//!
//! Flow: OCR lines -> [`group_blocks`] -> AI translates each `Block::text`
//! (see [`MARKUP`]) -> [`render`] on the captured RGBA frame.
//! `pipeline::translate_frame` runs it once; `live::Live` runs it per
//! frame, re-reading only what changed.

mod draw;
mod erase;
pub mod lang;
pub mod live;
pub mod pipeline;
#[cfg(test)]
pub(crate) mod tests;

/// Rules for the AI prompt. `render` draws `[[...]]` spans in the block's
/// accent colour (e.g. a gold name) when the original had one, otherwise in
/// its normal colour; the brackets are never drawn. A translation about as
/// long as the original keeps its size (longer ones shrink, then wrap).
pub const MARKUP: &str = "Wrap names of people, places and items (words a game usually shows \
in a highlight colour) in double square brackets, e.g. [[Sunstone]]. Use no other brackets \
or markup. Keep each translation about as long as the original.";

/// One OCR line. `bounds` = x, y, w, h in image pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub bounds: [f32; 4],
    pub text: String,
}

/// Lines that read as one paragraph, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub lines: Vec<Line>,
    /// The lines' texts joined with spaces: what gets translated.
    pub text: String,
    /// Union of the line boxes, x, y, w, h.
    pub bounds: [f32; 4],
}

/// Groups lines (in reading order) into blocks: a wrapped paragraph becomes
/// one block, list items and labels stay separate. A line continues the
/// latest block whose last line sits right above it in the same column, so
/// columns side by side (read interleaved, row by row) still group.
/// ponytail: heuristic (see [`continues`]); the AI step can re-split with
/// context later.
pub fn group_blocks(lines: &[Line]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for l in lines {
        let [x, y, w, h] = l.bounds;
        match out
            .iter()
            .rposition(|b| continues(b.lines.last().expect("never empty"), l))
        {
            Some(i) => {
                let b = &mut out[i];
                b.lines.push(l.clone());
                b.text = format!("{} {}", b.text, l.text);
                let [bx, by, bw, bh] = b.bounds;
                let (x1, y1) = ((bx + bw).max(x + w), (by + bh).max(y + h));
                let (x0, y0) = (bx.min(x), by.min(y));
                b.bounds = [x0, y0, x1 - x0, y1 - y0];
            }
            None => out.push(Block {
                lines: vec![l.clone()],
                text: l.text.clone(),
                bounds: l.bounds,
            }),
        }
    }
    out
}

/// `l` is the next line of the paragraph `p` ends: right below it, same left
/// edge (or same centre: subtitles), about the same size, not wider by more
/// than a few words (a narrow column beside a picture does not join the full
/// width below it), the sentence goes on, and neither is a list of items
/// (`a · b · c`).
fn continues(p: &Line, l: &Line) -> bool {
    let [x, y, w, h] = l.bounds;
    let [px, py, pw, ph] = p.bounds;
    let same_col = (px - x).abs() < 0.6 * h || (px + pw / 2.0 - x - w / 2.0).abs() < 0.6 * h;
    let close = y - (py + ph) < 0.6 * h && y > py;
    // OCR boxes of one font vary with ascenders, descenders and brackets
    let similar = ph.max(h) / ph.min(h) < 1.35;
    let fits = w < pw + 5.0 * h;
    let list = |t: &str| t.contains(['·', '•', '|']);
    let goes_on = l.text.starts_with(char::is_lowercase)
        || (p.text.chars().count() > 30 && !p.text.ends_with(['.', '!', '?', ':']));
    same_col && close && similar && fits && goes_on && !list(&p.text) && !list(&l.text)
}

/// What [`render`] did. Blocks it left as they were: `busy` = the background
/// behind them could not be erased cleanly (a photo, heavy texture), so the
/// original stays rather than a smear; `no_room` = the translation did not
/// fit legibly (>= 70 % of the original size) in the free space around it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drawn {
    pub drawn: usize,
    pub busy: usize,
    pub no_room: usize,
    /// Per block (in order): where its translation may be drawn, x, y, w, h.
    pub rooms: Vec<[f32; 4]>,
}

/// Erase uncertainty (`erase::LineStyle::residual`) above which a line's
/// background counts as busy. Flat, gradient, textured and translucent game
/// panels measure 6-12, an outlined subtitle on jagged trees 22, text on a
/// photo's fine detail 39 (test: `busy_background_threshold`).
const BUSY: f32 = 30.0;

/// Replaces each block's text in `rgba` (width x height, 4 bytes per pixel)
/// with `translations[i]` (may contain [`MARKUP`] spans). Blocks without a
/// translation (or with an empty one) are left untouched, and so are blocks
/// over a busy background or whose translation does not fit ([`Drawn`]).
/// `rtl` = the target language is right-to-left (Arabic): text is
/// right-aligned to the block's right edge (blocks in one left-aligned column
/// share the column's right edge, as far as the space is free); otherwise
/// left-aligned. Centred text stays centred. A short label may grow one em
/// toward its line end, into free space only. The redraw never grows past
/// the original size and never covers another block or a picture.
pub fn render(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    blocks: &[Block],
    translations: &[String],
    rtl: bool,
) -> Drawn {
    let dims = (width as usize, height as usize);
    let mut report = Drawn::default();
    if rgba.len() != dims.0 * dims.1 * 4 {
        return report;
    }
    // the free space around each block, read before anything is erased
    report.rooms = blocks
        .iter()
        .map(|b| room(rgba, dims, b, blocks, rtl))
        .collect();
    // erase every block first, so a long translation is never erased by a neighbour
    let mut todo = Vec::new();
    for (i, (b, t)) in blocks.iter().zip(translations).enumerate() {
        if t.trim().is_empty() {
            continue;
        }
        let mut undo = Vec::new();
        let styles: Vec<erase::LineStyle> = (b.lines.iter())
            .map(|l| {
                let (s, u) = erase::erase_line(rgba, dims.0, dims.1, l.bounds);
                undo.push(u);
                s
            })
            .collect();
        todo.push((i, t, styles, undo));
    }
    let mut fonts = draw::FONTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut fits = Vec::new();
    let mut restore = Vec::new();
    for (i, t, styles, undo) in todo {
        let b = &blocks[i];
        let p = draw::Placement {
            bounds: report.rooms[i],
            line_h: b.lines[0].bounds[3],
            texts: b.lines.iter().map(|l| l.text.clone()).collect(),
            centred: centred(b, dims.0 as f32),
            styles,
            rtl,
        };
        let busy = p.styles.iter().any(|s| s.residual > BUSY);
        match (!busy).then(|| draw::fit(&mut fonts, t, &p)).flatten() {
            Some(fit) => fits.push((p, fit)),
            None => {
                if busy {
                    report.busy += 1;
                } else {
                    report.no_room += 1;
                }
                restore.push(undo);
            }
        }
    }
    // put the skipped blocks back, last erased first
    for (at, old) in restore.iter().rev().flat_map(|b| b.iter().rev()).flatten() {
        rgba[*at..at + 3].copy_from_slice(old);
    }
    report.drawn = fits.len();
    for (p, fit) in fits {
        draw::draw_block(&mut fonts, rgba, dims, &p, fit);
    }
    report
}

/// Where `b`'s translation may go: its box, widened through free space only
/// (plain background like the box's edge; another block's text, a picture or
/// a frame line stops it): for RTL out to its column's right edge, and, for a
/// single line, one em toward the line end (left for RTL, right otherwise; a
/// paragraph keeps its column's edge). Centred text keeps its box.
fn room(rgba: &[u8], dims: (usize, usize), b: &Block, blocks: &[Block], rtl: bool) -> [f32; 4] {
    let [x, y, w, h] = b.bounds;
    if centred(b, dims.0 as f32) {
        return b.bounds;
    }
    // ponytail: the OCR box (the line box without its padding) as one em
    let em = if b.lines.len() == 1 {
        b.lines[0].bounds[3] / 1.3
    } else {
        0.0
    };
    let (mut x0, mut x1) = (x, x + w);
    if rtl {
        // not when another block shares the row on that side (captions side
        // by side, a setting and its value): the text stays under its own
        let right = column_right(b, blocks);
        let beside = blocks.iter().any(|o| {
            let [ox, oy, _, oh] = o.bounds;
            ox >= x1 && ox < right && (oy + oh).min(y + h) - oy.max(y) > 0.5 * h.min(oh)
        });
        x1 = if beside {
            x1
        } else {
            free(rgba, dims, b, x1, right)
        };
        x0 = free(rgba, dims, b, x0, x0 - em);
    } else {
        x1 = free(rgba, dims, b, x1, x1 + em);
    }
    [x0, y, x1 - x0, h]
}

/// From the box edge at `from` toward `to` (either way): how far the pixel
/// columns over `b`'s rows stay like the first one, row by row (90 % of the
/// rows within 16 plus twice the background's grain): a pill or a picture
/// edge, another block's text or a frame line stops it.
fn free(rgba: &[u8], (iw, ih): (usize, usize), b: &Block, from: f32, to: f32) -> f32 {
    let [_, y, _, h] = b.bounds;
    let (y0, y1) = (y.max(0.0) as usize, ((y + h) as usize).min(ih));
    let last = iw as f32 - 1.0;
    let (start, end) = (from.clamp(0.0, last) as usize, to.clamp(0.0, last) as usize);
    if y1 <= y0 + 1 || start == end {
        return from;
    }
    let px = |x: usize, y: usize| &rgba[(y * iw + x) * 4..(y * iw + x) * 4 + 3];
    let diff = |a: &[u8], b: &[u8]| (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
    // grain: the median step between neighbouring pixels of the first column
    let mut steps: Vec<u8> = (y0..y1 - 1)
        .map(|y| diff(px(start, y), px(start, y + 1)))
        .collect();
    steps.sort_unstable();
    let tol = 16 + 2 * steps[steps.len() / 2];
    // the first column must be the block's own background, not past the
    // edge of a pill or a panel it sits in: like the box's padding just in
    let inside = if end > start {
        start.saturating_sub(1)
    } else {
        (start + 1).min(iw - 1)
    };
    let changed = (y0..y1).filter(|&y| diff(px(start, y), px(inside, y)) > tol);
    if changed.count() * 10 > (y1 - y0) * 3 {
        return from;
    }
    let plain = |x: usize| {
        let odd = (y0..y1).filter(|&y| diff(px(x, y), px(start, y)) > tol);
        odd.count() * 10 <= y1 - y0
    };
    let mut x = start;
    while x != end {
        let next = if end > start { x + 1 } else { x - 1 };
        if !plain(next) {
            break;
        }
        x = next;
    }
    if x == start { from } else { x as f32 }
}

/// Right edge of the left-aligned column `b` sits in (a menu, a quest list):
/// mirrored to RTL, the column shares one right edge instead of going ragged.
fn column_right(b: &Block, blocks: &[Block]) -> f32 {
    let [x, y, _, _] = b.bounds;
    let lh = b.lines[0].bounds[3];
    blocks
        .iter()
        .filter(|o| {
            let olh = o.lines[0].bounds[3];
            (o.bounds[0] - x).abs() < lh.max(8.0)
                && (o.bounds[1] - y).abs() < 8.0 * lh
                && (olh - lh).abs() * 3.0 < lh
        })
        .map(|o| o.bounds[0] + o.bounds[2])
        .fold(b.bounds[0] + b.bounds[2], f32::max)
}

/// Centred text: a paragraph whose lines share a centre but not a left edge,
/// or a single line in the middle of the screen.
/// ponytail: a single centred line in an off-centre panel reads as left-aligned.
fn centred(b: &Block, width: f32) -> bool {
    let mid = |l: &Line| l.bounds[0] + l.bounds[2] / 2.0;
    match &b.lines[..] {
        [l] => (mid(l) - width / 2.0).abs() < width * 0.02,
        [first, rest @ ..] => rest.iter().all(|l| {
            (mid(l) - mid(first)).abs() < l.bounds[3] * 0.5
                && (l.bounds[0] - first.bounds[0]).abs() > l.bounds[3]
        }),
        [] => false,
    }
}
