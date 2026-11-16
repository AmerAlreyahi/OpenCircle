//! What OpenCircle itself lays over other apps that screen captures still show
//! (Live translate's edge glow: Windows can't keep a window with per-pixel alpha
//! out of captures, and X11 has no capture exclusion at all). Every grab of ours
//! (Live's own frames, Circle to Search, Snip) takes it back out, so nothing of
//! ours is read, frozen or snipped: where it was blended the pixels under it are
//! worked out again, where it was solid they are blanked (black).
use std::sync::{Arc, Mutex};

/// A picture drawn over the desktop as parts (rectangles, monitor px from
/// `origin`), each pixel an index into one palette (RGBA, straight alpha).
#[derive(Debug, PartialEq)]
pub struct Drawn {
    /// Its top-left on the desktop (physical px).
    origin: (i32, i32),
    parts: Vec<([u32; 4], Vec<u16>)>,
    table: Vec<[u8; 4]>,
}

/// A pixel this solid (of 255) can't be worked back: it is blanked.
const SOLID: u8 = 240;

impl Drawn {
    pub fn new(origin: (i32, i32), parts: Vec<([u32; 4], Vec<u16>)>, table: Vec<[u8; 4]>) -> Drawn {
        Drawn {
            origin,
            parts,
            table,
        }
    }

    /// The same, drawn with its top-left at `origin`.
    pub fn at(self, origin: (i32, i32)) -> Drawn {
        Drawn { origin, ..self }
    }

    /// Takes it out of `rgba`, a capture of the desktop's `(x, y, w, h)` (physical
    /// px) that shows it: the pixels under a blend worked out, solid ones black.
    /// Returns the parts it blanked (frame px, x, y, w, h). Nothing where the
    /// capture doesn't hold all of it.
    pub fn take_out(&self, rgba: &mut [u8], (x, y, w, h): (i32, i32, u32, u32)) -> Vec<[u32; 4]> {
        let (dx, dy) = (self.origin.0 - x, self.origin.1 - y);
        let inside = |[px, py, pw, ph]: [u32; 4]| {
            dx >= 0 && dy >= 0 && dx as u32 + px + pw <= w && dy as u32 + py + ph <= h
        };
        if rgba.len() < (w * h * 4) as usize || !self.parts.iter().all(|(r, _)| inside(*r)) {
            return Vec::new();
        }
        let mut blanked = Vec::new();
        for ([px, py, pw, _], at) in &self.parts {
            let mut solid = false;
            for (i, &k) in at.iter().enumerate() {
                let Some(&[r, g, b, a]) = self.table.get(usize::from(k)) else {
                    continue;
                };
                if a == 0 {
                    continue;
                }
                let (fx, fy) = (
                    dx as u32 + px + i as u32 % pw,
                    dy as u32 + py + i as u32 / pw,
                );
                let j = ((fy * w + fx) * 4) as usize;
                let pixel = &mut rgba[j..j + 3];
                if a >= SOLID {
                    pixel.fill(0);
                    solid = true;
                    continue;
                }
                // shown = (c a + under (255 - a)) / 255
                let (a, rest) = (u32::from(a), 255 - u32::from(a));
                for (v, c) in pixel.iter_mut().zip([r, g, b]) {
                    let over = u32::from(*v) * 255;
                    let under = (over.saturating_sub(u32::from(c) * a) + rest / 2) / rest;
                    *v = under.min(255) as u8;
                }
            }
            if solid {
                blanked.push([dx as u32 + px, dy as u32 + py, *pw, at.len() as u32 / pw]);
            }
        }
        blanked
    }
}

/// What is drawn now.
static NOW: Mutex<Vec<Arc<Drawn>>> = Mutex::new(Vec::new());

/// `Drawn` on screen: taken out of our captures until this is dropped.
#[derive(Debug)]
pub struct OnScreen(Arc<Drawn>);

/// `drawn` is on screen from now on (until the result is dropped).
pub fn show(drawn: Drawn) -> OnScreen {
    let drawn = Arc::new(drawn);
    if let Ok(mut now) = NOW.lock() {
        now.push(drawn.clone());
    }
    OnScreen(drawn)
}

impl Drop for OnScreen {
    fn drop(&mut self) {
        if let Ok(mut now) = NOW.lock() {
            now.retain(|d| !Arc::ptr_eq(d, &self.0));
        }
    }
}

/// Takes everything of ours that is on screen out of `rgba`, a capture of the
/// desktop's `(x, y, w, h)`; returns what it blanked (frame px, x, y, w, h).
pub fn take_out(rgba: &mut [u8], region: (i32, i32, u32, u32)) -> Vec<[u32; 4]> {
    let now = NOW.lock().map(|n| n.clone()).unwrap_or_default();
    (now.iter())
        .flat_map(|d| d.take_out(rgba, region))
        .collect()
}

/// `take_out` of a grabbed monitor (Circle to Search's, Snip's).
pub fn take_out_of(frame: &mut crate::shared::platform::capture::Frame) {
    let region = (frame.x, frame.y, frame.width, frame.height);
    take_out(&mut frame.rgba, region);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `under` with `drawn` laid over it, as the screen shows it.
    fn shown(under: &[u8], w: u32, drawn: &Drawn) -> Vec<u8> {
        let mut out = under.to_vec();
        for ([px, py, pw, _], at) in &drawn.parts {
            for (i, &k) in at.iter().enumerate() {
                let [r, g, b, a] = drawn.table[usize::from(k)];
                let j = (((py + i as u32 / pw) * w + px + i as u32 % pw) * 4) as usize;
                for (v, c) in out[j..j + 3].iter_mut().zip([r, g, b]) {
                    let (c, a) = (f32::from(c), f32::from(a) / 255.0);
                    *v = (c * a + f32::from(*v) * (1.0 - a)).round() as u8;
                }
            }
        }
        out
    }

    /// A glow over a picture comes back out within a level or two (8 bits each
    /// way), a solid line is blanked and reported, and a capture that doesn't
    /// hold it all is left as it is.
    #[test]
    fn what_we_drew_comes_back_out_of_a_capture() {
        let (w, h) = (40u32, 20u32);
        let under: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        // a 3 px band down the left at three strengths, a solid 1 px line on top
        let band = ([0, 0, 3, h], (0..3 * h).map(|i| (i % 3) as u16).collect());
        let line = ([0, 0, w, 1], vec![3; w as usize]);
        let table = vec![
            [10, 132, 255, 66],
            [94, 92, 230, 40],
            [94, 92, 230, 5],
            [10, 132, 255, 255],
        ];
        let glow = Drawn::new((0, 0), vec![band.clone()], table.clone());
        let mut seen = shown(&under, w, &glow);
        assert_ne!(seen, under);
        assert!(glow.take_out(&mut seen, (0, 0, w, h)).is_empty());
        let off = (seen.iter().zip(&under)).map(|(a, b)| a.abs_diff(*b)).max();
        assert!(off <= Some(2), "{off:?}");
        // solid: blanked, reported
        let both = Drawn::new((100, 50), vec![band, line], table);
        let mut seen = shown(&under, w, &both);
        let blanked = both.take_out(&mut seen, (100, 50, w, h));
        assert_eq!(blanked, vec![[0, 0, w, 1]]);
        assert!(
            seen[..(w * 4) as usize]
                .chunks(4)
                .all(|p| p[..3] == [0, 0, 0])
        );
        // another monitor's capture (it isn't on it): untouched
        let mut other = under.clone();
        assert!(both.take_out(&mut other, (0, 0, w, h)).is_empty());
        assert_eq!(other, under);
        // the registry: only while shown
        let mut seen = shown(&under, w, &both);
        let on = show(both);
        assert_eq!(take_out(&mut seen, (100, 50, w, h)).len(), 1);
        drop(on);
        let mut again = under.clone();
        assert!(take_out(&mut again, (100, 50, w, h)).is_empty());
        assert_eq!(again, under);
    }
}
