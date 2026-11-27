//! Blur zones of a recording (R-06): areas, or an app's main window followed while it moves, that
//! are hidden in EVERY frame before the encoder sees it (the file never holds the hidden
//! pixels). The fill is Snip's Hide (`annotate::blur`, the secure Soft style that is built from
//! the background around the zone and never reads what it covers): one implementation, no copy.
//! The other zones are excluded from the ring a fill reads, as Snip's Hides exclude each other.
//!
//! "Hide an app" covers the app's MAIN window only (its rectangle): its dialogs, menus and
//! other windows are not followed.
//!
//! Cost: a zone is painted at a fraction of its size when it is big (the fill is smooth) and
//! its look is kept: every frame the kept copy is pasted, and it is painted again only when the
//! ring round the zone changed AND the kept one is `REPAINT` old, or the zone moved. Anything that
//! goes wrong paints the zone flat, never leaves the raw pixels.
//!
//! Pure of the OS: a window's place comes from a lookup function (`window_rect::Follow` in the
//! real recorder), so tests move rectangles by hand.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::Point;
use tiny_skia::{IntSize, Pixmap};

use crate::features::snip::annotate::blur::{self, Exclude, Footprint, Kind, Style};
use crate::features::snip::annotate::document::Layer;
use crate::shared::platform::capture::window_rect::WinRect;

/// Hide's texture scale (image px): Snip's Medium (half the 16 px reference text height).
const CELL: f32 = 8.0;
/// Pixels around a zone the fill may read and paint: the ring Soft samples, the edge feather and
/// the softening all fit (`blur::reach`, the ring is at most 22 px).
const MARGIN: i32 = 64;
/// How far past a zone painting reaches (the outward feather of `blur::feather`, rounded up).
const FEATHER: i32 = 8;
/// How far out from a zone the fill reads (Soft's ring, at most 22 px, and the softening).
const RING: i32 = 32;
/// A kept look is painted again no sooner than this when only its ring changed.
const REPAINT: Duration = Duration::from_millis(250);
/// The most pixels a zone is painted at: a bigger one is painted smaller (by 2, 4, 8 ...) and
/// scaled up, which costs about 4 ms of paint at most.
const PAINT_PX: u64 = 150_000;
/// A scaled zone of more pixels than this is scaled up cell by cell, not smoothly.
const SMOOTH_PX: i64 = 4_000_000;
/// A followed window's zone is this much bigger on every side (the window moves between a frame
/// and the lookup).
const SLACK: i32 = 8;
/// The grey a zone is painted when the fill cannot be made: never the raw pixels.
const FLAT: u8 = 136;

/// What one zone covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// A fixed area, physical desktop px.
    Area(WinRect),
    /// An app's window, followed while it moves; `last` is where it was last seen (kept while
    /// the window cannot be read for a moment: never un-hidden by a failed read), `prev` where
    /// it was before that: a frame may still show it there, so both are hidden.
    App {
        handle: isize,
        last: WinRect,
        prev: WinRect,
    },
}

impl Zone {
    /// An app's zone, first seen at `at`.
    pub fn app(handle: isize, at: WinRect) -> Zone {
        Zone::App {
            handle,
            last: at,
            prev: at,
        }
    }

    /// Where the zone is (an app's window: where it was last seen).
    pub fn rect(&self) -> WinRect {
        match *self {
            Zone::Area(r) => r,
            Zone::App { last, .. } => last,
        }
    }
}

/// The smallest rectangle holding `a` and `b`, `pad` bigger on every side.
fn union((ax, ay, aw, ah): WinRect, (bx, by, bw, bh): WinRect, pad: i32) -> WinRect {
    let (x0, y0) = (ax.min(bx) - pad, ay.min(by) - pad);
    let x1 = (ax + aw as i32).max(bx + bw as i32) + pad;
    let y1 = (ay + ah as i32).max(by + bh as i32) + pad;
    (x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// The zones of one recording; clones share the list.
#[derive(Debug, Clone, Default)]
pub struct Zones(Arc<Mutex<Vec<Zone>>>);

impl PartialEq for Zones {
    fn eq(&self, other: &Zones) -> bool {
        self.list() == other.list()
    }
}

impl Zones {
    pub fn add(&self, zone: Zone) {
        self.0.lock().unwrap().push(zone);
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    pub fn list(&self) -> Vec<Zone> {
        self.0.lock().unwrap().clone()
    }

    /// What to hide NOW, in the frame of a recording that starts at desktop `origin` and is
    /// `size` big: followed windows are read through `lookup` (and remembered); a window's zone
    /// holds where it was and where it is; what lies outside the frame is cut away, an empty one
    /// is dropped.
    pub fn rects(
        &self,
        origin: (i32, i32),
        size: (u32, u32),
        lookup: &mut dyn FnMut(isize) -> Option<WinRect>,
    ) -> Vec<WinRect> {
        let mut list = self.0.lock().unwrap();
        list.iter_mut()
            .filter_map(|zone| {
                let (x, y, w, h) = match zone {
                    Zone::Area(r) => *r,
                    Zone::App { handle, last, prev } => {
                        *prev = *last;
                        if let Some(now) = lookup(*handle) {
                            *last = now;
                        }
                        union(*prev, *last, SLACK)
                    }
                };
                let (x0, y0) = ((x - origin.0).max(0), (y - origin.1).max(0));
                let x1 = (x - origin.0 + w as i32).min(size.0 as i32);
                let y1 = (y - origin.1 + h as i32).min(size.1 as i32);
                (x1 > x0 && y1 > y0).then_some((x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
            })
            .collect()
    }
}

/// What the capture thread needs to hide the zones in a frame: the list, where the recording sits
/// on the desktop, its secret and how to find a window.
pub struct Hook {
    pub zones: Zones,
    /// The recording's top-left on the desktop, and the frame's size.
    pub origin: (i32, i32),
    pub size: (u32, u32),
    pub seed: u64,
    pub lookup: Box<dyn FnMut(isize) -> Option<WinRect> + Send>,
    /// What each zone was last painted as (`hide`).
    pub cache: Cache,
}

/// The hidden look of each zone (its area and feather), kept while the ring round it is the
/// same: the fill reads ONLY that ring, so an unchanged ring is an unchanged fill (blur once,
/// mask many) and a big zone costs a copy instead of a repaint.
#[derive(Default)]
pub struct Cache {
    looks: HashMap<WinRect, Painted>,
    /// A paint failed: every zone is flat from now on.
    failed: bool,
}

struct Painted {
    ring: u64,
    when: Instant,
    /// The piece of the frame (x0, y0, x1, y1) and its bytes.
    at: (usize, usize, usize, usize),
    bytes: Vec<u8>,
}

impl Hook {
    /// Where the zones are now (frame px).
    pub fn rects(&mut self) -> Vec<WinRect> {
        self.zones.rects(self.origin, self.size, &mut self.lookup)
    }

    /// A copy of `raw` with `rects` hidden.
    pub fn hidden(&mut self, raw: &[u8], rects: &[WinRect]) -> Vec<u8> {
        let mut frame = raw.to_vec();
        hide(
            &mut frame,
            self.size,
            rects,
            self.seed,
            &mut self.cache,
            Instant::now(),
        );
        frame
    }
}

/// Hides `rects` (frame px) in `frame` (`size` RGBA, opaque). `seed` is the recording's secret:
/// the texture is the same in every frame, so a still area stays still. `now` times the cache.
/// A frame of the wrong size is painted flat whole.
pub fn hide(
    frame: &mut [u8],
    size: (u32, u32),
    rects: &[WinRect],
    seed: u64,
    cache: &mut Cache,
    now: Instant,
) {
    let (fw, fh) = (size.0 as usize, size.1 as usize);
    if frame.len() != fw * fh * 4 {
        frame.fill(FLAT);
        return;
    }
    cache.looks.retain(|k, _| rects.contains(k));
    // Three passes, so that no kept copy ever holds another zone's raw pixels: the kept looks are
    // pasted, then the zones that need it are painted, and only then are the new looks taken (a
    // zone's feather band may lie over another zone that was painted after it).
    let mut todo = Vec::new();
    for (i, &rect) in rects.iter().enumerate() {
        let others: Vec<WinRect> = (rects.iter().enumerate())
            .filter(|&(j, r)| j != i && *r != rect)
            .map(|(_, r)| *r)
            .collect();
        let (x, y, w, h) = rect;
        let ring = ring_hash(frame, (fw, fh), rect, &others);
        // the zone and its feather: all that painting changes
        let bound = (
            (x - FEATHER).max(0) as usize,
            (y - FEATHER).max(0) as usize,
            ((x + w as i32 + FEATHER) as usize).min(fw),
            ((y + h as i32 + FEATHER) as usize).min(fh),
        );
        let kept = (!cache.failed)
            .then(|| cache.looks.get(&rect))
            .flatten()
            .filter(|p| {
                p.at == bound && (p.ring == ring || now.saturating_duration_since(p.when) < REPAINT)
            });
        match kept {
            Some(p) => put(frame, fw, bound, &p.bytes),
            None => todo.push((rect, others, ring, bound)),
        }
    }
    for (rect, others, _, _) in &todo {
        // (once a paint has failed the rest of the recording is flat: no panic again and again)
        let ok = !cache.failed
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                paint(frame, (fw, fh), *rect, others, seed)
            }))
            .unwrap_or(None)
            .is_some();
        if !ok {
            if !cache.failed {
                crate::log!("[opencircle] recording: a zone could not be painted, all are flat");
            }
            cache.failed = true;
            flat(frame, fw, *rect);
        }
    }
    for (rect, _, ring, bound) in todo {
        if cache.failed {
            break;
        }
        let (x0, y0, x1, y1) = bound;
        let mut bytes = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
        for row in y0..y1 {
            let at = (row * fw + x0) * 4;
            bytes.extend_from_slice(&frame[at..at + (x1 - x0) * 4]);
        }
        cache.looks.insert(
            rect,
            Painted {
                ring,
                when: now,
                at: bound,
                bytes,
            },
        );
    }
}

/// `bytes` (rows of the piece `(x0, y0, x1, y1)`) into the frame.
fn put(frame: &mut [u8], fw: usize, (x0, y0, x1, y1): (usize, usize, usize, usize), bytes: &[u8]) {
    let n = (x1 - x0) * 4;
    for (i, row) in (y0..y1).enumerate() {
        let at = (row * fw + x0) * 4;
        frame[at..at + n].copy_from_slice(&bytes[i * n..(i + 1) * n]);
    }
}

/// The zone as one grey (the fail-closed look).
fn flat(frame: &mut [u8], fw: usize, (x, y, w, h): WinRect) {
    for row in y.max(0) as usize..(y + h as i32).max(0) as usize {
        let at = (row * fw + x.max(0) as usize) * 4;
        let n = ((x + w as i32) as usize)
            .min(fw)
            .saturating_sub(x.max(0) as usize)
            * 4;
        if let Some(px) = frame.get_mut(at..at + n) {
            px.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| p.copy_from_slice(&[FLAT, FLAT, FLAT, 255]));
        }
    }
}

/// The scale a zone of `w` x `h` is painted at: 1, 2, 4 ... so that it is at most `PAINT_PX`.
fn scale_of(w: u32, h: u32) -> i32 {
    let mut f = 1;
    while u64::from(w) * u64::from(h) / (f * f) as u64 > PAINT_PX {
        f *= 2;
    }
    f
}

/// The cells (painted scale `f`) that cover the zone whole: rounded outward, so a partly covered
/// cell is the zone's, never a ring cell read from the zone's own pixels.
fn cells_of((x, y, w, h): WinRect, f: i32) -> (i32, i32, i32, i32) {
    (
        x.div_euclid(f),
        y.div_euclid(f),
        (x + w as i32 + f - 1).div_euclid(f),
        (y + h as i32 + f - 1).div_euclid(f),
    )
}

/// Paints the zone into `frame`: Snip's Hide on the piece of the frame round it, at `scale_of`
/// its size. `None`: it could not be done.
fn paint(
    frame: &mut [u8],
    (fw, fh): (usize, usize),
    (x, y, w, h): WinRect,
    others: &[WinRect],
    seed: u64,
) -> Option<()> {
    #[cfg(test)]
    if tests::PANIC.get() {
        panic!("a paint that fails (test)");
    }
    let f = scale_of(w, h);
    let (fwi, fhi) = (fw as i32, fh as i32);
    // the zone in the painted scale (covering it whole), the piece worked on, in that scale
    let (sx0, sy0, sx1, sy1) = cells_of((x, y, w, h), f);
    let m = (MARGIN / f).max(24);
    let (cx0, cy0) = (sx0 - m, sy0 - m);
    let (cw, ch) = ((sx1 + m - cx0) as usize, (sy1 + m - cy0) as usize);
    let piece = make_piece(frame, (fw, fh), (cx0, cy0, cw, ch), (sx0, sy0, sx1, sy1), f);
    let mut layer = Layer {
        pm: Pixmap::from_vec(piece, IntSize::from_wh(cw as u32, ch as u32)?)?,
        origin: Point::new(cx0 as f32, cy0 as f32),
    };
    let corners = [
        Point::new(sx0 as f32, sy0 as f32),
        Point::new(sx1 as f32, sy1 as f32),
    ];
    let scaled = |(ox, oy, ow, oh): WinRect| {
        let (a, b) = (
            Point::new(ox as f32 / f as f32, oy as f32 / f as f32),
            Point::new(
                (ox + ow as i32) as f32 / f as f32,
                (oy + oh as i32) as f32 / f as f32,
            ),
        );
        Exclude::Redact { a, b, oval: false }
    };
    let exclude: Vec<Exclude> = others.iter().map(|&r| scaled(r)).collect();
    let fp = Footprint {
        points: &corners,
        size: 0.0,
        kind: Kind::Rect,
        outline: 0.0,
        ramp: &[],
        source: None,
        exclude: &exclude,
    };
    blur::paint(
        &mut layer,
        &fp,
        (CELL / f as f32).max(2.0),
        Style::Soft,
        seed,
    );
    let done = layer.pm.data();
    let piece = Piece {
        done,
        width: cw,
        at: (cx0, cy0),
        zone: (sx0, sy0, sx1, sy1),
    };
    let (x1, y1) = ((x + w as i32).min(fwi), (y + h as i32).min(fhi));
    if f == 1 {
        // the zone and its outward feather, as painted
        let (bx0, by0) = ((x - FEATHER).max(0), (y - FEATHER).max(0));
        let (bx1, by1) = ((x1 + FEATHER).min(fwi), (y1 + FEATHER).min(fhi));
        for fy in by0..by1 {
            let from = ((fy - cy0) as usize * cw + (bx0 - cx0) as usize) * 4;
            let to = (fy as usize * fw + bx0 as usize) * 4;
            let n = (bx1 - bx0) as usize * 4;
            frame[to..to + n].copy_from_slice(&done[from..from + n]);
        }
        return Some(());
    }
    scaled_up(frame, fw, (x, y, x1, y1), f, &piece);
    feather_band(frame, (fw, fh), (x, y, x1, y1), f, &piece);
    Some(())
}

/// The piece of the frame the fill works on, `cw` x `ch` cells from (`cx0`, `cy0`) at scale `f`:
/// every cell is the frame pixel at its middle; the zone's own cells (and what is outside the
/// frame) are left black and are never read.
fn make_piece(
    frame: &[u8],
    (fw, fh): (usize, usize),
    (cx0, cy0, cw, ch): (i32, i32, usize, usize),
    (sx0, sy0, sx1, sy1): (i32, i32, i32, i32),
    f: i32,
) -> Vec<u8> {
    let (fwi, fhi) = (fw as i32, fh as i32);
    let mut piece = vec![0u8; cw * ch * 4];
    for row in 0..ch {
        let fy = (cy0 + row as i32) * f + f / 2;
        if !(0..fhi).contains(&fy) {
            continue;
        }
        for col in 0..cw {
            let sx = cx0 + col as i32;
            let fx = sx * f + f / 2;
            let inside = (sx0..sx1).contains(&sx) && (sy0..sy1).contains(&(cy0 + row as i32));
            let at = (row * cw + col) * 4;
            if inside || !(0..fwi).contains(&fx) {
                piece[at + 3] = 255; // (the zone's own pixels are never read, not even to copy)
                continue;
            }
            let src = (fy as usize * fw + fx as usize) * 4;
            piece[at..at + 3].copy_from_slice(&frame[src..src + 3]);
            piece[at + 3] = 255;
        }
    }
    piece
}

/// The painted piece a scaled zone is put back from: its pixels, width, top-left and zone (all in
/// the painted scale).
struct Piece<'a> {
    done: &'a [u8],
    width: usize,
    at: (i32, i32),
    zone: (i32, i32, i32, i32),
}

impl Piece<'_> {
    /// The painted pixel at cell (`cx`, `cy`) (painted-scale coordinates, clamped to the zone).
    fn cell(&self, cx: i32, cy: i32) -> &[u8] {
        let (sx0, sy0, sx1, sy1) = self.zone;
        let (col, row) = (
            cx.clamp(sx0, sx1 - 1) - self.at.0,
            cy.clamp(sy0, sy1 - 1) - self.at.1,
        );
        let i = (row as usize * self.width + col as usize) * 4;
        &self.done[i..i + 4]
    }
}

/// Where frame coordinate `p` falls between painted cells: the two cells and the weight of the
/// second, 0..256 (bilinear, the cells' centres are at (i + 0.5) * f).
fn between(p: i32, f: i32) -> (i32, i32, u32) {
    let u = (2 * p + 1) * 128 / f - 128;
    (
        u.div_euclid(256),
        u.div_euclid(256) + 1,
        u.rem_euclid(256) as u32,
    )
}

/// The zone `(x0, y0, x1, y1)` of `frame` from the painted cells, smoothly (bilinear: a row of
/// cells is made as wide as the zone once, then rows are mixed).
fn scaled_up(
    frame: &mut [u8],
    fw: usize,
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    f: i32,
    piece: &Piece,
) {
    if (x1 - x0) as i64 * (y1 - y0) as i64 > SMOOTH_PX {
        // (a whole 4K screen: the cell's own colour, bilinear would cost 36 ms)
        let cols: Vec<i32> = (x0..x1).map(|fx| fx.div_euclid(f)).collect();
        for fy in y0..y1 {
            let cy = fy.div_euclid(f);
            let line = &mut frame[(fy as usize * fw + x0 as usize) * 4..][..cols.len() * 4];
            for (px, &cx) in line.as_chunks_mut::<4>().0.iter_mut().zip(&cols) {
                px.copy_from_slice(piece.cell(cx, cy));
            }
        }
        return;
    }
    let cols: Vec<(i32, i32, u32)> = (x0..x1).map(|fx| between(fx, f)).collect();
    let (_, sy0, _, sy1) = piece.zone;
    let wide = |cy: i32| -> Vec<u8> {
        let mut row = Vec::with_capacity(cols.len() * 4);
        for &(a, b, wt) in &cols {
            let (pa, pb) = (piece.cell(a, cy), piece.cell(b, cy));
            for k in 0..4 {
                row.push(((u32::from(pa[k]) * (256 - wt) + u32::from(pb[k]) * wt) >> 8) as u8);
            }
        }
        row
    };
    let mut rows: HashMap<i32, Vec<u8>> = HashMap::new();
    for cy in sy0 - 1..=sy1 {
        rows.insert(cy.clamp(sy0, sy1 - 1), wide(cy.clamp(sy0, sy1 - 1)));
    }
    for fy in y0..y1 {
        let (a, b, wt) = between(fy, f);
        let (ra, rb) = (&rows[&a.clamp(sy0, sy1 - 1)], &rows[&b.clamp(sy0, sy1 - 1)]);
        let line = &mut frame[(fy as usize * fw + x0 as usize) * 4..][..cols.len() * 4];
        for ((o, &p), &q) in line.iter_mut().zip(ra).zip(rb) {
            *o = ((u32::from(p) * (256 - wt) + u32::from(q) * wt) >> 8) as u8;
        }
        for px in line.as_chunks_mut::<4>().0.iter_mut() {
            px[3] = 255;
        }
    }
}

/// The outward feather of a scaled zone, at full size: the band `FEATHER` px outside the zone
/// fades from the zone's fill (its nearest painted cell) into the screen, like Snip's Hide.
fn feather_band(
    frame: &mut [u8],
    (fw, fh): (usize, usize),
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    f: i32,
    piece: &Piece,
) {
    let reach = FEATHER - 2; // (blur::feather of the texture scale, 6 px)
    let (bx0, by0) = ((x0 - reach).max(0), (y0 - reach).max(0));
    let (bx1, by1) = ((x1 + reach).min(fw as i32), (y1 + reach).min(fh as i32));
    for fy in by0..by1 {
        for fx in bx0..bx1 {
            let (dx, dy) = (
                (x0 - fx).max(fx - (x1 - 1)).max(0),
                (y0 - fy).max(fy - (y1 - 1)).max(0),
            );
            if dx == 0 && dy == 0 {
                continue;
            }
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let t = 1.0 - d / (reach + 1) as f32;
            if t <= 0.0 {
                continue;
            }
            let m = t * t * (3.0 - 2.0 * t);
            let fill = piece.cell(fx.div_euclid(f), fy.div_euclid(f));
            let at = (fy as usize * fw + fx as usize) * 4;
            for k in 0..3 {
                frame[at + k] =
                    (f32::from(fill[k]) * m + f32::from(frame[at + k]) * (1.0 - m)).round() as u8;
            }
        }
    }
}

/// A hash of every pixel in the band round `rect` that its fill may read (Soft's ring is at most
/// 22 px wide), left out where another zone lies (those are never read).
fn ring_hash(
    frame: &[u8],
    (fw, fh): (usize, usize),
    (x, y, w, h): WinRect,
    others: &[WinRect],
) -> u64 {
    let (x0, y0) = ((x - RING).max(0) as usize, (y - RING).max(0) as usize);
    let x1 = ((x + w as i32 + RING).min(fw as i32)).max(0) as usize;
    let y1 = ((y + h as i32 + RING).min(fh as i32)).max(0) as usize;
    let (zx0, zy0) = (x.max(0) as usize, y.max(0) as usize);
    let (zx1, zy1) = (
        ((x + w as i32).min(fw as i32)).max(0) as usize,
        ((y + h as i32).min(fh as i32)).max(0) as usize,
    );
    let pad = FEATHER + 1;
    let covered = |col: usize, row: usize| {
        others.iter().any(|&(ox, oy, ow, oh)| {
            (ox - pad..ox + ow as i32 + pad).contains(&(col as i32))
                && (oy - pad..oy + oh as i32 + pad).contains(&(row as i32))
        })
    };
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for row in y0..y1 {
        let inner = (zy0..zy1).contains(&row);
        for col in x0..x1 {
            if (inner && (zx0..zx1).contains(&col)) || (!others.is_empty() && covered(col, row)) {
                continue;
            }
            let i = (row * fw + col) * 4;
            let px = u32::from_le_bytes([frame[i], frame[i + 1], frame[i + 2], 0]);
            hash = (hash ^ u64::from(px)).wrapping_mul(0x0100_0000_01b3);
            hash ^= hash >> 29;
        }
    }
    hash
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    thread_local! {
        /// Makes the next paints on this thread panic (the fail-closed test).
        pub(super) static PANIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    fn differs_in(a: &[u8], b: &[u8], w: u32, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                n += usize::from(a[i..i + 3] != b[i..i + 3]);
            }
        }
        n
    }

    fn hide5(f: &mut [u8], size: (u32, u32), r: &[WinRect], seed: u64, c: &mut Cache) {
        hide(f, size, r, seed, c, Instant::now());
    }

    /// A busy picture: every pixel tells where it is.
    pub(crate) fn busy(w: u32, h: u32) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let ink = (x / 3 + y / 2) % 2 == 0;
                match ink {
                    true => [20, 30, 40, 255],
                    false => [240, 235, 230, 255],
                }
            })
            .collect()
    }

    fn at(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        frame[i..i + 4].try_into().unwrap()
    }

    /// R06-1: inside a zone every pixel is replaced; one pixel outside its edge (past the soft
    /// feather) is identical.
    #[test]
    fn r06_1_pixels_inside_are_replaced_and_outside_are_identical() {
        let (w, h) = (320, 200);
        let before = busy(w, h);
        let mut frame = before.clone();
        hide5(
            &mut frame,
            (w, h),
            &[(100, 60, 120, 80)],
            7,
            &mut Cache::default(),
        );
        let (mut same_in, mut same_out) = (0, 0);
        for y in 0..h {
            for x in 0..w {
                let inside = (100..220).contains(&x) && (60..140).contains(&y);
                let far = !(100 - 20..220 + 20).contains(&x) || !(60 - 20..140 + 20).contains(&y);
                let (a, b) = (at(&before, w, x, y), at(&frame, w, x, y));
                if inside && a == b {
                    same_in += 1;
                }
                if far && a != b {
                    same_out += 1;
                }
            }
        }
        // (a fill may equal a source pixel by chance: a handful at most)
        assert!(
            same_in < 120 * 80 / 50,
            "{same_in} pixels left as they were"
        );
        assert_eq!(same_out, 0, "pixels away from the zone changed");
    }

    /// R06-2: the hidden pixels are the hidden pixels' own: changing what lies UNDER a zone does
    /// not change the frame (the fill never reads the content).
    #[test]
    fn r06_2_the_fill_never_reads_what_it_covers() {
        let (w, h) = (320, 200);
        let rect = [(100, 60, 120, 80)];
        let a = busy(w, h);
        let mut b = a.clone();
        for y in 60..140u32 {
            for x in 100..220u32 {
                let i = ((y * w + x) * 4) as usize;
                b[i..i + 3].copy_from_slice(&[(x * 7) as u8, (y * 5) as u8, 99]);
            }
        }
        let (mut fa, mut fb) = (a, b);
        hide5(&mut fa, (w, h), &rect, 7, &mut Cache::default());
        hide5(&mut fb, (w, h), &rect, 7, &mut Cache::default());
        assert!(fa == fb);
    }

    /// R06-3: a zone at the frame's edge, a zone bigger than the frame and a wrong-sized frame
    /// never panic or touch what they should not.
    #[test]
    fn r06_3_edges_and_bad_sizes_are_safe() {
        let (w, h) = (64, 48);
        let mut f = busy(w, h);
        hide5(
            &mut f,
            (w, h),
            &[(0, 0, 64, 48), (60, 44, 4, 4)],
            1,
            &mut Cache::default(),
        );
        let mut short = vec![9u8; 10];
        hide5(
            &mut short,
            (w, h),
            &[(0, 0, 8, 8)],
            1,
            &mut Cache::default(),
        );
        assert!(
            short.iter().all(|&b| b == FLAT),
            "a wrong-sized frame is flat"
        );
    }

    /// R06-7 (numbers, not a pass/fail: `cargo test --release r06_7 -- --ignored --nocapture`):
    /// what hiding one zone costs per frame, with the copy `Hook::hidden` makes.
    #[test]
    #[ignore = "measures; run in release"]
    fn r06_7_cost_per_frame() {
        use std::time::Instant;
        for (w, h) in [(1920u32, 1080u32), (3840, 2160)] {
            let raw = busy(w, h);
            for (zw, zh) in [(400u32, 200u32), (w / 2, h / 2), (w - 40, h - 40)] {
                let mut hook = Hook {
                    zones: Zones::default(),
                    origin: (0, 0),
                    size: (w, h),
                    seed: 1,
                    lookup: Box::new(|_| None),
                    cache: Cache::default(),
                };
                let rects = [(20, 20, zw, zh)];
                let runs = 20;
                let t = Instant::now();
                for _ in 0..runs {
                    hook.cache = Cache::default(); // the ring changed: a full repaint
                    std::hint::black_box(hook.hidden(&raw, &rects));
                }
                let cold = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
                let t = Instant::now();
                for _ in 0..runs {
                    std::hint::black_box(hook.hidden(&raw, &rects)); // the ring is the same
                }
                let warm = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
                let t = Instant::now();
                for _ in 0..runs {
                    std::hint::black_box(raw.clone());
                }
                let copy = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
                eprintln!(
                    "{w}x{h} zone {zw}x{zh}: repaint {:.2} ms, same ring {:.2} ms (frame copy {copy:.2} ms of both)",
                    cold - copy,
                    warm - copy
                );
            }
        }
    }

    /// R06-11 (needs an X11 display: `xvfb-run -a cargo test r06_11 -- --ignored`): a real window,
    /// followed through `Follow` while it moves, hidden in the real screen frames.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs an X11 display (Xvfb)"]
    fn r06_11_x11_a_real_window_is_followed_and_hidden_in_real_frames() {
        use crate::shared::platform::capture::frames::Frames;
        use crate::shared::platform::capture::window_rect::Follow;
        use std::time::{Duration, Instant};
        use xcb::{Xid, x};

        let (conn, n) = xcb::Connection::connect(None).unwrap();
        let screen = conn.get_setup().roots().nth(n as usize).unwrap();
        let win: x::Window = conn.generate_id();
        conn.send_request(&x::CreateWindow {
            depth: x::COPY_FROM_PARENT as u8,
            wid: win,
            parent: screen.root(),
            x: 50,
            y: 60,
            width: 200,
            height: 100,
            border_width: 0,
            class: x::WindowClass::InputOutput,
            visual: screen.root_visual(),
            value_list: &[x::Cw::BackPixel(0x00ff_0000), x::Cw::OverrideRedirect(true)],
        });
        conn.send_request(&x::MapWindow { window: win });
        conn.flush().unwrap();
        let handle = win.resource_id() as isize;
        let mut follow = Follow::default();
        let frame = |frames: &mut Frames| {
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(f) = frames.next(Duration::from_millis(100)).unwrap() {
                    return f;
                }
                assert!(Instant::now() < end, "no frame");
            }
        };
        let (w, h) = (640u32, 480u32);
        let mut frames = Frames::open((0, 0, w, h));
        let red = |f: &[u8], x: u32, y: u32| f[((y * w + x) * 4) as usize];
        let grabbed = frame(&mut frames);
        assert_eq!(follow.rect(handle), Some((50, 60, 200, 100)));
        assert_eq!(red(&grabbed, 100, 100), 255, "the window shows on screen");
        let zones = Zones::default();
        zones.add(Zone::app(handle, follow.rect(handle).unwrap()));
        let mut hook = Hook {
            zones,
            origin: (0, 0),
            size: (w, h),
            seed: 5,
            lookup: Box::new(move |h| follow.rect(h)),
            cache: Cache::default(),
        };
        let rects = hook.rects();
        assert_eq!(rects, vec![(42, 52, 216, 116)], "the window and the slack");
        let hidden = hook.hidden(&grabbed, &rects);
        assert!(
            red(&hidden, 100, 100) < 200,
            "no red left where the window was"
        );
        // moves: the zone goes with it
        conn.send_request(&x::ConfigureWindow {
            window: win,
            value_list: &[x::ConfigWindow::X(300), x::ConfigWindow::Y(200)],
        });
        conn.flush().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let jump = hook.rects();
        assert_eq!(
            jump,
            vec![(42, 52, 466, 256)],
            "where it was and where it is"
        );
        let rects = hook.rects();
        assert_eq!(rects, vec![(292, 192, 216, 116)]);
        let moved = frame(&mut frames);
        let hidden = hook.hidden(&moved, &rects);
        assert!(red(&moved, 350, 250) == 255 && red(&hidden, 350, 250) < 200);
        assert_eq!(
            red(&hidden, 100, 100),
            red(&moved, 100, 100),
            "the old place is whole"
        );
    }

    /// R06-4: window-follow: an app zone holds where its window was and where it is NOW (a frame
    /// may still show it at the old place), remembered when the window cannot be read, cut to the
    /// frame, and in frame (not desktop) pixels.
    #[test]
    fn r06_4_a_followed_window_moves_the_zone() {
        let zones = Zones::default();
        zones.add(Zone::Area((1000, 100, 50, 50)));
        zones.add(Zone::app(5, (1100, 200, 100, 100)));
        let origin = (1000, 100);
        let size = (1000, 500);
        let at = std::cell::Cell::new(Some((1100, 200, 100, 100)));
        let mut look = |h: isize| {
            assert_eq!(h, 5);
            at.get()
        };
        // still: the window and the slack
        assert_eq!(zones.rects(origin, size, &mut look)[1], (92, 92, 116, 116));
        // it jumps 300 px: the old place and the new one, in one zone
        at.set(Some((1400, 200, 100, 100)));
        assert_eq!(zones.rects(origin, size, &mut look)[1], (92, 92, 416, 116));
        // the next frame: only the new place
        assert_eq!(zones.rects(origin, size, &mut look)[1], (392, 92, 116, 116));
        // unreadable: where it was
        at.set(None);
        assert_eq!(zones.rects(origin, size, &mut look)[1], (392, 92, 116, 116));
        // half out of the frame: cut
        at.set(Some((1900, 200, 400, 100)));
        let r = zones.rects(origin, size, &mut look)[1];
        assert_eq!((r.0 + r.2 as i32, r.1), (1000, 92));
        // entirely out: dropped
        at.set(Some((5000, 0, 10, 10)));
        zones.rects(origin, size, &mut look);
        at.set(Some((5000, 0, 10, 10)));
        assert_eq!(zones.rects(origin, size, &mut look).len(), 1);
    }

    /// R06-12: an unchanged ring gives the very same hidden frame from the cache, a changed ring
    /// is painted again only when the kept look is old (or the zone moved), and a zone that went
    /// is forgotten.
    #[test]
    fn r06_12_the_fill_is_kept_while_its_ring_is_the_same() {
        let (w, h) = (320, 200);
        let rect = [(100, 60, 120, 80)];
        let (t0, later) = (Instant::now(), Duration::from_millis(400));
        let mut cache = Cache::default();
        let raw = busy(w, h);
        let (mut cold, mut warm) = (raw.clone(), raw.clone());
        hide(&mut cold, (w, h), &rect, 7, &mut cache, t0);
        assert_eq!(cache.looks.len(), 1);
        hide(&mut warm, (w, h), &rect, 7, &mut cache, t0);
        assert!(cold == warm, "the kept fill is the painted one");
        let mut under = raw.clone();
        under[((100 * 320 + 150) * 4) as usize] ^= 0xff;
        hide(&mut under, (w, h), &rect, 7, &mut cache, t0);
        assert!(under == cold, "what is under a zone never matters");
        let mut moved = raw.clone();
        for y in 50..60u32 {
            for x in 90..230u32 {
                moved[((y * w + x) * 4) as usize] = 255 - moved[((y * w + x) * 4) as usize];
            }
        }
        let mut fresh = moved.clone();
        hide(&mut fresh, (w, h), &rect, 7, &mut Cache::default(), t0);
        let mut soon = moved.clone();
        hide(&mut soon, (w, h), &rect, 7, &mut cache, t0);
        assert_eq!(
            differs_in(&soon, &cold, w, (100, 60, 220, 140)),
            0,
            "a changed ring waits for the kept look to be old"
        );
        let mut old = moved;
        hide(&mut old, (w, h), &rect, 7, &mut cache, t0 + later);
        assert_eq!(
            differs_in(&old, &fresh, w, (100, 60, 220, 140)),
            0,
            "then it is painted again"
        );
        assert!(
            differs_in(&old, &cold, w, (100, 60, 220, 140)) > 0,
            "and it is new"
        );
        hide(&mut raw.clone(), (w, h), &[], 7, &mut cache, t0);
        assert!(cache.looks.is_empty(), "a zone that went is forgotten");
    }

    /// R06-13: one changed pixel of the ring, on an odd column or row, changes the key.
    #[test]
    fn r06_13_every_pixel_of_the_ring_counts() {
        let (w, h) = (320usize, 200usize);
        let raw = busy(w as u32, h as u32);
        let zone = (100, 60, 120, 80);
        let key = |f: &[u8]| ring_hash(f, (w, h), zone, &[]);
        let base = key(&raw);
        for (x, y) in [
            (99usize, 61usize),
            (101, 59),
            (221, 100),
            (150, 141),
            (80, 60),
        ] {
            let mut f = raw.clone();
            f[(y * w + x) * 4 + 1] ^= 1;
            assert_ne!(key(&f), base, "a pixel at {x},{y}");
        }
        let mut inside = raw.clone();
        inside[(100 * w + 150) * 4] ^= 1;
        assert_eq!(key(&inside), base, "inside the zone does not");
    }

    /// R06-14: the other zones are not read: a fill does not change with what lies in another
    /// zone, whether it is 10 px away or overlaps, and neither does its cache key.
    #[test]
    fn r06_14_a_zone_never_reads_another_zones_pixels() {
        let (w, h) = (320u32, 200u32);
        for second in [(100u32 + 130, 60u32, 60u32, 80u32), (200, 70, 60, 80)] {
            let (x2, y2, w2, h2) = (second.0 as i32, second.1 as i32, second.2, second.3);
            let rects = [(100, 60, 120, 80), (x2, y2, w2, h2)];
            let a = busy(w, h);
            let mut b = a.clone();
            for y in second.1..second.1 + h2 {
                for x in second.0..second.0 + w2 {
                    let i = ((y * w + x) * 4) as usize;
                    b[i..i + 3].copy_from_slice(&[255, 0, (x * 3) as u8]);
                }
            }
            let (mut fa, mut fb) = (a.clone(), b.clone());
            let (mut ca, mut cb) = (Cache::default(), Cache::default());
            hide5(&mut fa, (w, h), &rects, 7, &mut ca);
            hide5(&mut fb, (w, h), &rects, 7, &mut cb);
            assert!(fa == fb, "second zone at {second:?}");
            assert_eq!(
                ca.looks[&rects[0]].ring, cb.looks[&rects[0]].ring,
                "its key too"
            );
        }
    }

    /// R06-15: fail closed: a frame of the wrong size is painted flat whole, and a zone that
    /// cannot be painted is flat, never raw.
    #[test]
    fn r06_15_a_failure_never_leaves_the_raw_pixels() {
        let mut bad = busy(64, 48);
        hide5(
            &mut bad,
            (80, 48),
            &[(0, 0, 8, 8)],
            1,
            &mut Cache::default(),
        );
        assert!(bad.iter().all(|&b| b == FLAT), "wrong size: all flat");
        let (w, h) = (64usize, 48usize);
        let mut f = busy(64, 48);
        flat(&mut f, w, (10, 10, 20, 20));
        assert_eq!(at(&f, 64, 15, 15), [FLAT, FLAT, FLAT, 255]);
        assert_ne!(at(&f, 64, 40, 40), [FLAT, FLAT, FLAT, 255]);
        assert_eq!(f.len(), w * h * 4);
    }

    /// R06-16: a big zone is painted at a smaller scale, never reads its own pixels, and still
    /// hides every pixel of it.
    #[test]
    fn r06_16_a_big_zone_is_painted_small_and_hides_everything() {
        assert_eq!(scale_of(400, 200), 1);
        assert_eq!(scale_of(3800, 2120), 8);
        let (w, h) = (1200u32, 800u32);
        let a = busy(w, h);
        let mut b = a.clone();
        for y in 100..700u32 {
            for x in 100..1100u32 {
                let i = ((y * w + x) * 4) as usize;
                b[i..i + 3].copy_from_slice(&[(x * 7) as u8, (y * 5) as u8, 9]);
            }
        }
        let zone = [(100, 100, 1000, 600)];
        let (mut fa, mut fb) = (a.clone(), b);
        hide5(&mut fa, (w, h), &zone, 3, &mut Cache::default());
        hide5(&mut fb, (w, h), &zone, 3, &mut Cache::default());
        assert!(fa == fb, "what is under a big zone never matters");
        let same = (100..700u32)
            .flat_map(|y| (100..1100u32).map(move |x| (x, y)))
            .filter(|&(x, y)| at(&fa, w, x, y) == at(&a, w, x, y))
            .count();
        assert!(same < 600_000 / 20, "{same} pixels left as they were");
        assert_eq!(at(&fa, w, 50, 50), at(&a, w, 50, 50), "outside: whole");
    }

    /// R06-21 (the reviewer's probe): a zone's kept band never holds another zone's raw pixels: A
    /// stands beside a red app zone B, B moves a little later with the same cache, and no pure red
    /// pixel is in the second frame.
    #[test]
    fn r06_21_a_kept_band_never_holds_another_zones_raw_pixels() {
        let (w, h) = (400u32, 200u32);
        let red = |x0: u32, x1: u32| {
            let mut f = busy(w, h);
            for y in 60..140u32 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    f[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
                }
            }
            f
        };
        let pure = |f: &[u8]| {
            f.as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] == [255, 0, 0])
                .count()
        };
        let (a, t0) = ((100, 60, 120, 80), Instant::now());
        let mut cache = Cache::default();
        let mut one = red(224, 284);
        hide(&mut one, (w, h), &[a, (224, 60, 60, 80)], 7, &mut cache, t0);
        assert_eq!(pure(&one), 0, "frame 1");
        let mut two = red(300, 360);
        hide(
            &mut two,
            (w, h),
            &[a, (300, 60, 60, 80)],
            7,
            &mut cache,
            t0 + Duration::from_millis(30),
        );
        assert_eq!(pure(&two), 0, "frame 2: B moved, A's kept band is pasted");
    }

    /// R06-22: a zone that does not sit on the painted scale (odd position, odd size, scaled by
    /// 4) still never reads a pixel of itself: what is under it never matters.
    #[test]
    fn r06_22_a_zone_off_the_paint_scale_never_reads_its_content() {
        let (w, h) = (1400u32, 900u32);
        let (zx, zy, zw, zh) = (103u32, 57u32, 1004u32, 602u32);
        assert!(scale_of(zw, zh) > 1);
        let a = busy(w, h);
        let mut b = a.clone();
        for y in zy..zy + zh {
            for x in zx..zx + zw {
                let i = ((y * w + x) * 4) as usize;
                b[i..i + 3].copy_from_slice(&[250, 10, 10]);
            }
        }
        let zone = [(zx as i32, zy as i32, zw, zh)];
        let (mut fa, mut fb) = (a.clone(), b);
        hide5(&mut fa, (w, h), &zone, 3, &mut Cache::default());
        hide5(&mut fb, (w, h), &zone, 3, &mut Cache::default());
        assert!(
            fa == fb,
            "what is under the zone never matters, its last row and column too"
        );
        let left = differs_in(&fa, &a, w, (zx, zy, zx + zw, zy + zh));
        assert!(
            left as u32 > zw * zh * 9 / 10,
            "{left} of the zone's pixels changed"
        );
    }

    /// R06-23: a paint that fails leaves the zone flat, never raw, and from then on every zone is
    /// flat (no repeated panics).
    #[test]
    fn r06_23_a_failed_paint_is_flat_and_stays_flat() {
        let (w, h) = (200u32, 120u32);
        let mut cache = Cache::default();
        let raw = busy(w, h);
        let mut f = raw.clone();
        PANIC.set(true);
        hide5(&mut f, (w, h), &[(40, 30, 60, 40)], 1, &mut cache);
        PANIC.set(false);
        assert_eq!(at(&f, w, 60, 50), [FLAT, FLAT, FLAT, 255], "flat, not raw");
        let mut again = raw.clone();
        hide5(
            &mut again,
            (w, h),
            &[(40, 30, 60, 40), (120, 70, 40, 30)],
            1,
            &mut cache,
        );
        assert_eq!(at(&again, w, 60, 50), [FLAT, FLAT, FLAT, 255]);
        assert_eq!(
            at(&again, w, 130, 80),
            [FLAT, FLAT, FLAT, 255],
            "a new zone too"
        );
        assert_eq!(at(&again, w, 5, 5), at(&raw, w, 5, 5), "outside whole");
    }

    /// R06-24: the feather of a scaled zone is back: the band just outside it is a blend, and
    /// far outside is untouched.
    #[test]
    fn r06_24_a_scaled_zone_has_its_soft_edge() {
        let (w, h) = (1400u32, 900u32);
        let a = busy(w, h);
        let mut f = a.clone();
        hide5(
            &mut f,
            (w, h),
            &[(200, 100, 1000, 600)],
            3,
            &mut Cache::default(),
        );
        let moved = differs_in(&f, &a, w, (194, 300, 200, 400));
        assert!(
            moved > 100,
            "{moved} pixels of the band outside the edge are blended"
        );
        assert_eq!(
            differs_in(&f, &a, w, (150, 300, 190, 400)),
            0,
            "far outside: whole"
        );
    }

    /// R06-26: the cells a zone is painted from never come from the zone itself, at any position
    /// and size (the last, partly covered row and column of cells included): a sentinel colour
    /// laid over the zone is nowhere in the piece.
    #[test]
    fn r06_26_the_painted_piece_holds_no_pixel_of_the_zone() {
        let (w, h) = (1400usize, 900usize);
        for (zx, zy, zw, zh) in [
            (103, 57, 1004, 602),
            (100, 60, 1001, 601),
            (97, 51, 1003, 603),
        ] {
            let f = scale_of(zw, zh);
            let mut frame = busy(w as u32, h as u32);
            for y in zy..zy + zh as i32 {
                for x in zx..zx + zw as i32 {
                    let i = (y as usize * w + x as usize) * 4;
                    frame[i..i + 3].copy_from_slice(&[255, 0, 255]);
                }
            }
            let (sx0, sy0, sx1, sy1) = cells_of((zx, zy, zw, zh), f);
            let (cx0, cy0) = (sx0 - 24, sy0 - 24);
            let (cw, ch) = ((sx1 + 24 - cx0) as usize, (sy1 + 24 - cy0) as usize);
            let piece = make_piece(&frame, (w, h), (cx0, cy0, cw, ch), (sx0, sy0, sx1, sy1), f);
            let sentinels = piece
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] == [255, 0, 255])
                .count();
            assert_eq!(sentinels, 0, "zone {zx},{zy} {zw}x{zh} at 1/{f}");
        }
    }
}
