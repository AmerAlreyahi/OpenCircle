//! A feature preview on the stage: its bundled still
//! (`assets/previews/stills/<clip>.webp`, `build.rs`), then, once the page's
//! clips are on disk (`online`, owner 2026-11-04), all of them one after
//! another in a loop for as long as the page is open. A thread does the work
//! (the still, each clip checked against its pin, AV1 decoded by `av1`) at the
//! stage's size in px (`Width`), at most two frames ahead of the screen;
//! dropping the player ends it, so nothing is decoded once the page or
//! Settings closes. Reduced motion: the still only.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::Duration;

use iced::widget::image::Handle;

use super::{av1, online};
use crate::ocr::models::ModelFile;

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/previews.rs"));
}

/// The bundled still of the clip `name` (`None`: no such clip yet).
pub fn still(name: &str) -> Option<&'static [u8]> {
    (bundled::STILLS.iter())
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}

/// The stage's width in px, as last laid out (0: not yet, full size).
pub type Width = Arc<AtomicU32>;

/// rav1d's threads for one preview: smooth at 1080p, light on the rest.
const THREADS: i32 = 2;

/// A clip to play.
#[derive(Clone)]
pub enum Clip {
    /// Downloaded: used only when it is exactly the pinned file.
    Disk(PathBuf, &'static ModelFile),
    #[cfg(test)]
    Bytes(&'static [u8]),
}

struct Frame {
    image: Handle,
    /// How long it shows (0: the still, until the first clip frame).
    length: Duration,
    /// Which clip it is of.
    clip: usize,
}

pub struct Player {
    names: Vec<&'static str>,
    shown: Option<Handle>,
    /// From the thread; `None` once it is done (a still that was all, or a
    /// clip that failed).
    frames: Option<Receiver<Frame>>,
    /// When the frame shown ends, from the start.
    until: Duration,
    /// The clips are on disk (else the stage offers "Play preview").
    on_disk: bool,
    files: Vec<Clip>,
    /// The clip being shown (an index of `names`).
    clip: usize,
    moving: bool,
    width: Width,
}

impl Player {
    /// The still of `names`' first clip, then, with `play` and every clip on
    /// disk, the clips in a loop (`None`: no still for it).
    pub fn new(
        names: Vec<&'static str>,
        files: Vec<Clip>,
        play: bool,
        width: Width,
    ) -> Option<Self> {
        Self::new_at(names, files, play, width, 0)
    }

    /// The same, beginning at clip `start` (the loop goes on from there).
    pub fn new_at(
        names: Vec<&'static str>,
        files: Vec<Clip>,
        play: bool,
        width: Width,
        start: usize,
    ) -> Option<Self> {
        let start = start % names.len().max(1);
        let still = still(names.get(start)?)?;
        let on_disk = files.iter().all(Clip::present);
        let clips = (play && on_disk).then(|| files.clone());
        let (tx, rx) = mpsc::sync_channel(1);
        let w = width.clone();
        std::thread::Builder::new()
            .name("preview".into())
            .spawn(move || run(still, clips, &w, &tx, start))
            .ok()?;
        Some(Player {
            names,
            shown: None,
            frames: Some(rx),
            until: Duration::ZERO,
            on_disk,
            files,
            clip: start,
            moving: play,
            width,
        })
    }

    /// How many clips, and which one shows.
    pub fn position(&self) -> (usize, usize) {
        (self.clip, self.names.len())
    }

    /// A player that begins at clip `to` (wrapping), its first frame due at
    /// `at` on the page's clock.
    pub fn jump(&self, to: usize, at: Duration) -> Option<Player> {
        let mut p = Self::new_at(
            self.names.clone(),
            self.files.clone(),
            self.moving,
            self.width.clone(),
            to,
        )?;
        p.until = at;
        Some(p)
    }

    /// Is this the list it was made from?
    pub fn plays(&self, names: &[&'static str]) -> bool {
        self.names == names
    }

    /// `at` since it began: the frames due by then, in turn (only the last
    /// kept); true while the thread is still at work.
    pub fn advance(&mut self, at: Duration) -> bool {
        while let Some(frames) = &self.frames {
            if self.shown.is_some() && at < self.until {
                break;
            }
            match frames.try_recv() {
                Ok(f) => self.show(f, Some(at)),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.done(),
            }
        }
        self.playing()
    }

    /// `f` from now on; `at` (the clock): a clip frame that came late (a slow
    /// PC) goes on from then, rather than racing through the ones behind.
    fn show(&mut self, f: Frame, at: Option<Duration>) {
        if let Some(at) =
            at.filter(|at| f.length > Duration::ZERO && *at > self.until + 2 * f.length)
        {
            self.until = at;
        }
        self.until += f.length;
        self.clip = f.clip;
        self.shown = Some(f.image);
    }

    fn done(&mut self) {
        self.frames = None;
        self.on_disk = self.files.iter().all(Clip::present);
    }

    /// Waits for the thread's next frame (tests: the still, then each clip
    /// frame in order); false when it is done.
    #[cfg(test)]
    pub fn next_frame(&mut self) -> bool {
        let Some(f) = self.frames.as_ref().and_then(|r| r.recv().ok()) else {
            self.done();
            return false;
        };
        self.clip = f.clip;
        self.shown = Some(f.image);
        true
    }

    /// `advance`, waiting for each frame due by `at` (tests).
    #[cfg(test)]
    pub fn catch_up(&mut self, at: Duration) {
        while let Some(frames) = &self.frames {
            if self.shown.is_some() && at < self.until {
                break;
            }
            match frames.recv() {
                Ok(f) => self.show(f, None),
                Err(_) => self.done(),
            }
        }
    }

    pub fn playing(&self) -> bool {
        self.frames.is_some()
    }

    pub fn on_disk(&self) -> bool {
        self.on_disk
    }

    /// The frame to show (`None`: the still isn't decoded yet).
    pub fn frame(&self) -> Option<&Handle> {
        self.shown.as_ref()
    }
}

impl Clip {
    fn present(&self) -> bool {
        match self {
            Clip::Disk(path, f) => online::ready(&[(path.clone(), *f)]),
            #[cfg(test)]
            Clip::Bytes(_) => true,
        }
    }

    fn load(&self) -> Option<Vec<u8>> {
        match self {
            Clip::Disk(path, f) => online::load(path, f),
            #[cfg(test)]
            Clip::Bytes(b) => Some(b.to_vec()),
        }
    }
}

/// The thread: the still, then the clips over and over until the player is
/// dropped (a send fails) or a clip doesn't decode.
fn run(
    still: &[u8],
    clips: Option<Vec<Clip>>,
    width: &Width,
    tx: &SyncSender<Frame>,
    start: usize,
) -> Option<()> {
    // the page is laid out right after it opens: its first preview waits for
    // the stage's width (at most 100 ms, then full size; tests: never)
    let tries = if cfg!(test) { 0 } else { 20 };
    for _ in 0..tries {
        if width.load(Ordering::Relaxed) != 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    tx.send(Frame {
        image: still_at(still, target(width))?,
        length: Duration::ZERO,
        clip: start,
    })
    .ok()?;
    let clips: Vec<Vec<u8>> = clips?.iter().map(Clip::load).collect::<Option<_>>()?;
    for i in (0..clips.len()).cycle().skip(start) {
        play(&clips[i], i, width, tx)?;
    }
    None
}

/// One clip's frames to `tx`, each at the stage's width then.
fn play(clip: &[u8], index: usize, width: &Width, tx: &SyncSender<Frame>) -> Option<()> {
    let ivf = av1::ivf(clip)?;
    let length = ivf.frame;
    let mut decoder = av1::Decoder::new(THREADS)?;
    for packet in ivf {
        let mut out = None;
        decoder
            .decode(packet, |pic| {
                let (sw, sh) = pic.size();
                let w = target(width).min(sw as u32).max(1) as usize;
                let h = ((w * sh + sw / 2) / sw).max(1);
                out = pic.rgba(w, h).map(|rgba| Frame {
                    image: Handle::from_rgba(w as u32, h as u32, rgba),
                    length,
                    clip: index,
                });
            })
            .ok()?;
        if let Some(frame) = out {
            tx.send(frame).ok()?;
        }
    }
    Some(())
}

/// The px width to decode at.
fn target(width: &Width) -> u32 {
    match width.load(Ordering::Relaxed) {
        0 => u32::MAX,
        w => w,
    }
}

/// The still, at most `w` px wide (smaller: area-averaged, so it stays sharp).
fn still_at(bytes: &[u8], w: u32) -> Option<Handle> {
    use image::imageops::{self, FilterType};
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP).ok()?;
    let mut img = img.into_rgba8();
    if w < img.width() {
        let (ih, iw) = (u64::from(img.height()), u64::from(img.width()));
        let h = ((ih * u64::from(w) + iw / 2) / iw).max(1) as u32;
        img = imageops::resize(&img, w, h, FilterType::Triangle);
    }
    Some(Handle::from_rgba(img.width(), img.height(), img.into_raw()))
}

/// `tests/fixtures/previews/tiny-a.ivf` (red, green, blue) and `tiny-b.ivf`
/// (white, black): 128x72, 100 ms a frame (`scripts/make-preview-fixture.py`).
#[cfg(test)]
pub const TINY_A: &[u8] = include_bytes!("../../../../tests/fixtures/previews/tiny-a.ivf");
#[cfg(test)]
pub const TINY_B: &[u8] = include_bytes!("../../../../tests/fixtures/previews/tiny-b.ivf");

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn colour(h: &Handle) -> [u8; 3] {
        match h {
            Handle::Rgba { pixels, .. } => [pixels[0], pixels[1], pixels[2]],
            other => panic!("{other:?}"),
        }
    }

    fn player(clips: &[&'static [u8]], play: bool, width: u32) -> Player {
        let clips = clips.iter().map(|b| Clip::Bytes(b)).collect();
        let width = Arc::new(AtomicU32::new(width));
        Player::new(vec!["circle-1"], clips, play, width).expect("circle-1 has a still")
    }

    /// The still first, then a's frames, b's, a's again: in turn, for ever.
    #[test]
    fn the_clips_play_in_turn_and_loop() {
        let mut p = player(&[TINY_A, TINY_B], true, 0);
        assert!(p.next_frame(), "the still");
        let mut seen = Vec::new();
        for _ in 0..12 {
            assert!(p.next_frame());
            let [r, g, b] = colour(p.frame().unwrap());
            seen.push(match (r > 128, g > 128, b > 128) {
                (true, false, false) => 'r',
                (false, true, false) => 'g',
                (false, false, true) => 'b',
                (true, true, true) => 'w',
                (false, false, false) => 'k',
                _ => '?',
            });
        }
        assert_eq!(seen.iter().collect::<String>(), "rgbwkrgbwkrg");
    }

    /// The clock shows each frame for its 100 ms; a late tick catches up.
    #[test]
    fn frames_follow_the_clock() {
        let ms = Duration::from_millis;
        let mut p = player(&[TINY_A], true, 64);
        // the thread is at most two frames ahead: wait for them to be there
        let wait = |p: &mut Player, at| {
            for _ in 0..10_000 {
                p.advance(at);
                if p.until > at {
                    return;
                }
                std::thread::sleep(ms(2));
            }
        };
        wait(&mut p, ms(0));
        let first = colour(p.frame().unwrap());
        assert_eq!(p.until, ms(100), "the still gives way to frame 1 at once");
        wait(&mut p, ms(99));
        assert_eq!(colour(p.frame().unwrap()), first);
        wait(&mut p, ms(100));
        assert_ne!(colour(p.frame().unwrap()), first, "frame 2 from 100 ms");
        assert!(p.playing(), "a clip loops: never done");
    }

    /// Decoded at the stage's width (16:9 kept), never above the clip's own.
    #[test]
    fn frames_are_decoded_at_the_stage_width() {
        let size = |h: &Handle| match h {
            Handle::Rgba { width, height, .. } => (*width, *height),
            other => panic!("{other:?}"),
        };
        let mut p = player(&[TINY_A], true, 64);
        assert!(p.next_frame());
        // the still (1640x922) at 64 px
        assert_eq!(size(p.frame().unwrap()), (64, 36));
        assert!(p.next_frame());
        assert_eq!(size(p.frame().unwrap()), (64, 36));
        let mut big = player(&[TINY_A], true, 4000);
        assert!(big.next_frame() && big.next_frame());
        assert_eq!(size(big.frame().unwrap()), (128, 72), "never upscaled");
    }

    /// Without the clips on disk, or with reduced motion: the still, then done.
    #[test]
    fn a_still_is_all_without_the_clips_or_with_reduced_motion() {
        let mut p = player(&[TINY_A], false, 0);
        assert!(p.next_frame() && !p.next_frame());
        assert!(!p.playing() && p.frame().is_some());
        let missing = std::env::temp_dir().join("oc-no-such-preview.ivf");
        let clips = vec![Clip::Disk(missing, &online::CLIPS[0])];
        let mut p = Player::new(vec!["circle-1"], clips, true, Width::default()).unwrap();
        assert!(!p.on_disk(), "offer Play preview");
        assert!(p.next_frame() && !p.next_frame(), "the still only");
    }

    /// A clip that isn't AV1 ends the thread (the still stays).
    #[test]
    fn a_broken_clip_stops_at_the_still() {
        let mut p = player(&[b"DKIF nope"], true, 0);
        assert!(p.next_frame() && !p.next_frame());
        assert!(p.frame().is_some());
    }
}
