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
pub mod live;
pub mod pipeline;
#[cfg(test)]
mod tests;

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
/// one block, list items and labels stay separate.
/// ponytail: heuristic (same left edge, tight spacing, similar height, the
/// sentence continues); the AI step can re-split with context later.
pub fn group_blocks(lines: &[Line]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for l in lines {
        let [x, y, _, h] = l.bounds;
        let joins = out.last().is_some_and(|b| {
            let p = b.lines.last().unwrap();
            let [px, py, pw, ph] = p.bounds;
            // same left edge, or same centre (centred subtitles)
            let same_col =
                (px - x).abs() < 0.6 * h || (px + pw / 2.0 - x - l.bounds[2] / 2.0).abs() < 0.6 * h;
            let close = y - (py + ph) < 0.6 * h && y > py;
            let similar = (ph / h - 1.0).abs() < 0.25;
            let continues = l.text.starts_with(char::is_lowercase)
                || (p.text.chars().count() > 30 && !p.text.ends_with(['.', '!', '?', ':']));
            same_col && close && similar && continues
        });
        match out.last_mut() {
            Some(b) if joins => {
                b.lines.push(l.clone());
                b.text = format!("{} {}", b.text, l.text);
                let [bx, by, bw, bh] = b.bounds;
                let (x1, y1) = ((bx + bw).max(x + l.bounds[2]), (by + bh).max(y + h));
                let (x0, y0) = (bx.min(x), by.min(y));
                b.bounds = [x0, y0, x1 - x0, y1 - y0];
            }
            _ => out.push(Block {
                lines: vec![l.clone()],
                text: l.text.clone(),
                bounds: l.bounds,
            }),
        }
    }
    out
}

/// Replaces each block's text in `rgba` (width x height, 4 bytes per pixel)
/// with `translations[i]` (may contain [`MARKUP`] spans). Blocks without a
/// translation (or with an empty one) are left untouched. `rtl` = the target
/// language is right-to-left (Arabic): text is right-aligned to the block's
/// right edge (blocks in one left-aligned column share the column's right
/// edge); otherwise left-aligned. Centred text stays centred. A short label
/// may grow one em toward its line start before it shrinks.
pub fn render(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    blocks: &[Block],
    translations: &[String],
    rtl: bool,
) {
    let dims = (width as usize, height as usize);
    if rgba.len() != dims.0 * dims.1 * 4 {
        return;
    }
    // erase every block first, so a long translation is never erased by a neighbour
    let todo: Vec<(&Block, &String, Vec<erase::LineStyle>)> = blocks
        .iter()
        .zip(translations)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(b, t)| {
            let styles = b
                .lines
                .iter()
                .map(|l| erase::erase_line(rgba, dims.0, dims.1, l.bounds))
                .collect();
            (b, t, styles)
        })
        .collect();
    let mut fonts = draw::FONTS.lock().unwrap_or_else(|e| e.into_inner());
    for (b, t, styles) in todo {
        let mut bounds = b.bounds;
        if rtl {
            bounds[2] = column_right(b, blocks) - bounds[0];
        }
        let p = draw::Placement {
            bounds,
            texts: b.lines.iter().map(|l| l.text.clone()).collect(),
            centred: centred(b, dims.0 as f32),
            styles,
            rtl,
        };
        draw::draw_block(&mut fonts, rgba, dims, t, &p);
    }
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
