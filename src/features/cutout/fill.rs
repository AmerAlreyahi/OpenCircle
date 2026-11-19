//! The quick cut: when the picture's border is one flat colour, the
//! background is what the border colour floods into (4-connected, from every
//! border pixel). No model, no download, a few ms. Anything else is the
//! model's (`super::model`).

/// A pixel is background when no channel is further than this from the
/// border colour (the lab's test, 14 of 255).
const TOL: i32 = 14;
/// The border is flat when this share of its pixels is within `TOL` of the
/// border's median colour.
const FLAT: f32 = 0.95;
/// Less foreground than this share of the picture is not an object (a flat
/// picture, or a flood that took everything).
const MIN_KEEP: f32 = 0.005;

fn dist(p: &[u8], c: [u8; 3]) -> i32 {
    (0..3)
        .map(|i| (p[i] as i32 - c[i] as i32).abs())
        .max()
        .unwrap_or(0)
}

/// The border's median colour, when the border is flat.
fn flat_border(rgba: &[u8], w: usize, h: usize) -> Option<[u8; 3]> {
    let px = |x: usize, y: usize| &rgba[(y * w + x) * 4..][..3];
    let ring: Vec<&[u8]> = (0..w)
        .flat_map(|x| [px(x, 0), px(x, h - 1)])
        .chain((1..h.saturating_sub(1)).flat_map(|y| [px(0, y), px(w - 1, y)]))
        .collect();
    let median = |c: usize| {
        let mut v: Vec<u8> = ring.iter().map(|p| p[c]).collect();
        v.sort_unstable();
        v[v.len() / 2]
    };
    let bg = [median(0), median(1), median(2)];
    let near = ring.iter().filter(|p| dist(p, bg) <= TOL).count();
    (near as f32 >= FLAT * ring.len() as f32).then_some(bg)
}

/// The alpha (255 keep, 0 background; one pixel of soft edge) of `rgba`
/// (`w` x `h`) when its border is flat and something is left in front of it.
pub(crate) fn quick(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let (w, h) = (w as usize, h as usize);
    if w < 3 || h < 3 || rgba.len() != w * h * 4 {
        return None;
    }
    let bg = flat_border(rgba, w, h)?;
    let is_bg = |i: usize| dist(&rgba[i * 4..], bg) <= TOL;
    let mut gone = vec![false; w * h];
    let mut stack: Vec<usize> = (0..w)
        .flat_map(|x| [x, (h - 1) * w + x])
        .chain((0..h).flat_map(|y| [y * w, y * w + w - 1]))
        .collect();
    while let Some(i) = stack.pop() {
        if gone[i] || !is_bg(i) {
            continue;
        }
        gone[i] = true;
        let (x, y) = (i % w, i / w);
        if x > 0 {
            stack.push(i - 1);
        }
        if x + 1 < w {
            stack.push(i + 1);
        }
        if y > 0 {
            stack.push(i - w);
        }
        if y + 1 < h {
            stack.push(i + w);
        }
    }
    let mut alpha: Vec<u8> = gone.iter().map(|&g| if g { 0 } else { 255 }).collect();
    // the pixels touching the background fade with their distance from it
    // (an anti-aliased edge keeps its own partial cover, no halo)
    for i in 0..w * h {
        let (x, y) = (i % w, i / w);
        let touches = !gone[i]
            && ((x > 0 && gone[i - 1])
                || (x + 1 < w && gone[i + 1])
                || (y > 0 && gone[i - w])
                || (y + 1 < h && gone[i + w]));
        if touches {
            let d = dist(&rgba[i * 4..], bg);
            alpha[i] = (d * 255 / (3 * TOL)).clamp(0, 255) as u8;
        }
    }
    let kept = gone.iter().filter(|&&g| !g).count();
    (kept as f32 >= MIN_KEEP * (w * h) as f32).then_some(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(
        w: usize,
        h: usize,
        bg: [u8; 3],
        draw: impl Fn(usize, usize) -> Option<[u8; 3]>,
    ) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let c = draw(x, y).unwrap_or(bg);
                v.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        v
    }

    /// A flat border: the object stays, the background goes, even a hole of
    /// background colour inside the object stays (it isn't reached).
    #[test]
    fn a_flat_background_is_cut_and_enclosed_background_stays() {
        let img = picture(60, 40, [250, 250, 250], |x, y| {
            let obj = (10..50).contains(&x) && (8..32).contains(&y);
            let hole = (25..35).contains(&x) && (15..25).contains(&y);
            (obj && !hole)
                .then_some([200, 30, 30])
                .or(hole.then_some([250, 250, 250]))
        });
        let a = quick(&img, 60, 40).unwrap();
        assert_eq!(a[0], 0, "corner");
        assert_eq!(a[20 * 60 + 15], 255, "object");
        assert_eq!(a[20 * 60 + 30], 255, "enclosed hole stays");
    }

    /// A textured border (stripes) is not the quick cut's: the model is.
    #[test]
    fn a_busy_border_is_left_to_the_model() {
        let img = picture(60, 40, [0, 0, 0], |x, _| {
            Some(if x % 2 == 0 {
                [0, 0, 0]
            } else {
                [255, 255, 255]
            })
        });
        assert!(quick(&img, 60, 40).is_none());
    }

    /// Nothing in front of the background (a flat picture): no cut.
    #[test]
    fn a_flat_picture_has_nothing_to_keep() {
        let img = picture(30, 30, [10, 20, 30], |_, _| None);
        assert!(quick(&img, 30, 30).is_none());
    }

    /// The object's edge pixel keeps a partial cover (soft edge).
    #[test]
    fn the_edge_is_soft() {
        let img = picture(40, 40, [255, 255, 255], |x, y| {
            if (10..30).contains(&x) && (10..30).contains(&y) {
                Some(if x == 10 { [235, 235, 235] } else { [0, 0, 0] })
            } else {
                None
            }
        });
        let a = quick(&img, 40, 40).unwrap();
        // 20 away from white: outside TOL, but on the edge, only partly kept
        assert!(
            a[20 * 40 + 10] > 0 && a[20 * 40 + 10] < 255,
            "{}",
            a[20 * 40 + 10]
        );
        assert_eq!(a[20 * 40 + 20], 255);
    }
}
