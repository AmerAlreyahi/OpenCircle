//! The walkthrough's transition (Studio v3.1): ONE transition between all the steps,
//! Cut, Crossfade (default), Slide, Push, Zoom or Wipe, 0.3 / 0.5 / 1 s long. Every
//! function here is pure: `mix(a, b, kind, t, mirror)` is the frame `t` (0..=1) of the way
//! from `a` to `b` (same size; t = 0 is `a`, t = 1 is `b`), and `timeline` is the list of
//! what the file shows. Slide, Push and Wipe come in from the end of the line, so in
//! Arabic (`mirror`) they come in from the left.
use crate::shared::picture::Crop;

/// One tick of a video / of a transition's frames (ms): 20 frames a second.
pub const TICK: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transition {
    Cut,
    #[default]
    Crossfade,
    Slide,
    Push,
    Zoom,
    Wipe,
}

impl Transition {
    pub const ALL: [Transition; 6] = [
        Transition::Cut,
        Transition::Crossfade,
        Transition::Slide,
        Transition::Push,
        Transition::Zoom,
        Transition::Wipe,
    ];
}

/// The lengths on offer (ms), and the default.
pub const LENGTHS: [u32; 3] = [300, 500, 1000];
pub const DEFAULT_MS: u32 = 500;

/// What the file shows, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cell {
    /// Step `i` for `ms`.
    Hold(usize, u32),
    /// `t` of the way from step `from` to the next, for one tick.
    Mix(usize, f32),
}

/// `n` steps each held `hold_ms`, with `kind` (`ms` long) between neighbours.
pub fn timeline(n: usize, hold_ms: u32, kind: Transition, ms: u32) -> Vec<Cell> {
    let mixes = mix_count(kind, ms);
    let mut out = Vec::new();
    for i in 0..n {
        out.push(Cell::Hold(i, hold_ms));
        if i + 1 < n {
            out.extend((1..=mixes).map(|k| Cell::Mix(i, k as f32 / (mixes + 1) as f32)));
        }
    }
    out
}

/// The frames a transition adds between two steps (a Cut adds none).
pub fn mix_count(kind: Transition, ms: u32) -> usize {
    match kind {
        Transition::Cut => 0,
        _ => (ms / TICK).max(1) as usize,
    }
}

/// The frame `t` of the way from `a` to `b` (`a` and `b` the same size).
pub fn mix(a: &Crop, b: &Crop, kind: Transition, t: f32, mirror: bool) -> Crop {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return a.clone();
    }
    if t >= 1.0 || kind == Transition::Cut {
        return b.clone();
    }
    match kind {
        Transition::Crossfade => blend(a, b, t),
        Transition::Zoom => zoom(a, b, t),
        _ if mirror => flip(&slide_like(&flip(a), &flip(b), kind, t)),
        _ => slide_like(a, b, kind, t),
    }
}

/// The columns Slide, Push and Wipe move by: `s` of `width` (rounded).
fn cols(width: u32, t: f32) -> usize {
    (width as f32 * t).round() as usize
}

fn slide_like(a: &Crop, b: &Crop, kind: Transition, t: f32) -> Crop {
    let (w, h) = (a.width as usize, a.height as usize);
    let s = cols(a.width, t).min(w);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = |c: &Crop, x0: usize, n: usize| {
            let i = (y * w + x0) * 4;
            c.rgba[i..i + n * 4].to_vec()
        };
        match kind {
            // b slides in over a, which stays
            Transition::Slide => {
                rgba.extend(row(a, 0, w - s));
                rgba.extend(row(b, 0, s));
            }
            // b pushes a out: both move together
            Transition::Push => {
                rgba.extend(row(a, s, w - s));
                rgba.extend(row(b, 0, s));
            }
            // b is uncovered from the start edge
            _ => {
                rgba.extend(row(b, 0, s));
                rgba.extend(row(a, s, w - s));
            }
        }
    }
    Crop {
        width: a.width,
        height: a.height,
        rgba,
    }
}

/// `t` of the way from `a`'s colours to `b`'s.
fn blend(a: &Crop, b: &Crop, t: f32) -> Crop {
    let rgba = a
        .rgba
        .iter()
        .zip(&b.rgba)
        .map(|(&p, &q)| (f32::from(p) + (f32::from(q) - f32::from(p)) * t).round() as u8)
        .collect();
    Crop {
        width: a.width,
        height: a.height,
        rgba,
    }
}

/// b grows from the middle (half size to full) as it fades in over a.
fn zoom(a: &Crop, b: &Crop, t: f32) -> Crop {
    let (w, h) = (a.width as usize, a.height as usize);
    let scale = 0.5 + 0.5 * t;
    let mut rgba = a.rgba.clone();
    for y in 0..h {
        for x in 0..w {
            // the pixel of b this one shows (nearest), if the zoomed b covers it
            let sx = (x as f32 - w as f32 / 2.0) / scale + w as f32 / 2.0;
            let sy = (y as f32 - h as f32 / 2.0) / scale + h as f32 / 2.0;
            if sx < 0.0 || sy < 0.0 || sx >= w as f32 || sy >= h as f32 {
                continue;
            }
            let (from, to) = ((sy as usize * w + sx as usize) * 4, (y * w + x) * 4);
            for c in 0..4 {
                let (p, q) = (f32::from(rgba[to + c]), f32::from(b.rgba[from + c]));
                rgba[to + c] = (p + (q - p) * t).round() as u8;
            }
        }
    }
    Crop {
        width: a.width,
        height: a.height,
        rgba,
    }
}

/// The picture mirrored left to right.
fn flip(c: &Crop) -> Crop {
    let w = c.width as usize;
    let mut rgba = Vec::with_capacity(c.rgba.len());
    for row in c.rgba.chunks(w * 4) {
        for px in row.chunks(4).rev() {
            rgba.extend_from_slice(px);
        }
    }
    Crop {
        width: c.width,
        height: c.height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two columns: `left`, `right` colours, per row, `h` rows.
    fn pic(w: u32, h: u32, colour: [u8; 4]) -> Crop {
        Crop {
            width: w,
            height: h,
            rgba: colour.repeat((w * h) as usize),
        }
    }

    fn at(c: &Crop, x: u32) -> [u8; 4] {
        let i = x as usize * 4;
        [c.rgba[i], c.rgba[i + 1], c.rgba[i + 2], c.rgba[i + 3]]
    }

    const A: [u8; 4] = [0, 0, 0, 255];
    const B: [u8; 4] = [200, 100, 50, 255];

    #[test]
    fn every_kind_starts_on_a_and_ends_on_b() {
        let (a, b) = (pic(8, 2, A), pic(8, 2, B));
        for kind in Transition::ALL {
            for mirror in [false, true] {
                assert_eq!(
                    mix(&a, &b, kind, 0.0, mirror).rgba,
                    a.rgba,
                    "{kind:?} first"
                );
                assert_eq!(mix(&a, &b, kind, 1.0, mirror).rgba, b.rgba, "{kind:?} last");
            }
        }
    }

    #[test]
    fn crossfade_halfway_is_the_middle_colour() {
        let m = mix(
            &pic(4, 1, A),
            &pic(4, 1, B),
            Transition::Crossfade,
            0.5,
            false,
        );
        assert_eq!(at(&m, 0), [100, 50, 25, 255]);
    }

    #[test]
    fn slide_push_and_wipe_halfway_split_the_picture_and_mirror_in_arabic() {
        // a: left half red, right half blue so Push's movement shows
        let mut a = pic(8, 1, A);
        a.rgba[16..].copy_from_slice(&[9, 9, 9, 255].repeat(4)); // right half grey
        let b = pic(8, 1, B);
        // Slide: a stays, b's head comes in from the end (right; left in Arabic)
        let m = mix(&a, &b, Transition::Slide, 0.5, false);
        assert_eq!((at(&m, 0), at(&m, 3), at(&m, 4), at(&m, 7)), (A, A, B, B));
        let m = mix(&a, &b, Transition::Slide, 0.5, true);
        assert_eq!(
            (at(&m, 0), at(&m, 3), at(&m, 4), at(&m, 7)),
            (B, B, [9, 9, 9, 255], [9, 9, 9, 255])
        );
        // Push: a moves out, b follows: the grey half is now at the left edge
        let m = mix(&a, &b, Transition::Push, 0.5, false);
        assert_eq!(
            (at(&m, 0), at(&m, 3), at(&m, 4)),
            ([9, 9, 9, 255], [9, 9, 9, 255], B)
        );
        // Wipe: b is uncovered from the start edge (left; right in Arabic)
        let m = mix(&a, &b, Transition::Wipe, 0.5, false);
        assert_eq!((at(&m, 0), at(&m, 7)), (B, [9, 9, 9, 255]));
        let m = mix(&a, &b, Transition::Wipe, 0.5, true);
        assert_eq!((at(&m, 0), at(&m, 7)), (A, B));
    }

    #[test]
    fn zoom_halfway_shows_a_at_the_edge_and_b_in_the_middle() {
        let (a, b) = (pic(8, 8, A), pic(8, 8, B));
        let m = mix(&a, &b, Transition::Zoom, 0.5, false);
        assert_eq!(at(&m, 0), A, "b has not reached the edge");
        let mid = (4 * 8 + 4) * 4;
        assert_ne!(&m.rgba[mid..mid + 4], &A, "b shows in the middle");
    }

    #[test]
    fn the_timeline_holds_each_step_with_the_mixes_between() {
        let t = timeline(3, 2000, Transition::Crossfade, 300);
        assert_eq!(t.len(), 3 + 2 * 6);
        assert_eq!(t[0], Cell::Hold(0, 2000));
        assert_eq!(t[1], Cell::Mix(0, 1.0 / 7.0));
        assert_eq!(t[6], Cell::Mix(0, 6.0 / 7.0));
        assert_eq!(t[7], Cell::Hold(1, 2000));
        assert_eq!(
            *t.last().unwrap(),
            Cell::Hold(2, 2000),
            "no mix after the last"
        );
        // a Cut adds nothing; one step has no neighbour
        assert_eq!(timeline(3, 1000, Transition::Cut, 500).len(), 3);
        assert_eq!(timeline(1, 1000, Transition::Wipe, 500).len(), 1);
    }
}
