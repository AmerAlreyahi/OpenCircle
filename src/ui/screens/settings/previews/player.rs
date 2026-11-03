//! A feature preview's clip: an animated WebP bundled at build time
//! (`assets/previews/<clip>.webp`, `build.rs`), decoded one frame at a time
//! while it plays (never the whole clip at once). It plays once, then rests
//! on its last frame with the decoder dropped; a still one (reduced motion)
//! shows its first frame only. Settings closing drops it with its state.
use std::io::Cursor;
use std::time::Duration;

use iced::widget::image::Handle;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, Frames};

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/previews.rs"));
}

/// The bundled clip called `name` (`None`: not made yet, the placeholder).
pub fn clip(name: &str) -> Option<&'static [u8]> {
    (bundled::CLIPS.iter())
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}

pub struct Player {
    /// The frames still to come; `None` once at rest.
    frames: Option<Frames<'static>>,
    shown: Handle,
    /// When the frame shown ends, from the start of the first clip.
    until: Duration,
    /// The clips in play order and the one playing: with two or more, the
    /// last is followed by the first, for as long as the page is open.
    list: Vec<&'static [u8]>,
    at: usize,
}

fn frames_of(bytes: &'static [u8]) -> Option<Frames<'static>> {
    Some(WebPDecoder::new(Cursor::new(bytes)).ok()?.into_frames())
}

impl Player {
    /// One clip at its first frame (`None`: not a WebP that decodes); `still`
    /// keeps it there.
    #[cfg(test)]
    pub fn new(bytes: &'static [u8], still: bool) -> Option<Self> {
        Self::list(vec![bytes], still)
    }

    /// The clips one after another (a single one plays once and rests).
    pub fn list(list: Vec<&'static [u8]>, still: bool) -> Option<Self> {
        let mut frames = frames_of(list.first()?)?;
        let (shown, until) = decode(frames.next()?.ok()?);
        Some(Player {
            frames: (!still).then_some(frames),
            shown,
            until,
            list,
            at: 0,
        })
    }

    /// Is this the list it was made from?
    pub fn plays(&self, list: &[&'static [u8]]) -> bool {
        self.list.len() == list.len()
            && (self.list.iter().zip(list)).all(|(a, b)| std::ptr::eq(*a, *b))
    }

    /// `at` into the clips: the frames due by then are decoded in turn (only
    /// the last one kept); true while frames are still to come.
    pub fn advance(&mut self, at: Duration) -> bool {
        while at >= self.until {
            let Some(frames) = &mut self.frames else {
                break;
            };
            let next = match frames.next() {
                Some(Ok(frame)) => Some(frame),
                // the end: the next clip of the list, if there is one
                None if self.list.len() > 1 => {
                    self.at = (self.at + 1) % self.list.len();
                    self.frames = frames_of(self.list[self.at]);
                    self.frames.as_mut().and_then(|f| f.next()?.ok())
                }
                // the end (or a frame that doesn't decode): rest here
                _ => None,
            };
            match next {
                Some(frame) => {
                    let (shown, length) = decode(frame);
                    self.shown = shown;
                    self.until += length;
                }
                None => self.frames = None,
            }
        }
        self.playing()
    }

    /// The clip playing now (its place in the list).
    #[cfg(test)]
    pub fn current(&self) -> usize {
        self.at
    }

    pub fn playing(&self) -> bool {
        self.frames.is_some()
    }

    pub fn frame(&self) -> &Handle {
        &self.shown
    }
}

/// A frame as the stage draws it, and how long it shows (at least 10 ms, as
/// browsers play a frame with no delay).
fn decode(frame: image::Frame) -> (Handle, Duration) {
    let (n, d) = frame.delay().numer_denom_ms();
    let length = Duration::from_micros(u64::from(n) * 1000 / u64::from(d.max(1)));
    let buffer = frame.into_buffer();
    let (w, h) = buffer.dimensions();
    (
        Handle::from_rgba(w, h, buffer.into_raw()),
        length.max(Duration::from_millis(10)),
    )
}

/// `tests/fixtures/previews/tiny.webp` (`scripts/make-preview-fixture.py`):
/// 5 frames of 300 ms, 48x27, each one colour.
#[cfg(test)]
pub const TINY: &[u8] = include_bytes!("../../../../../tests/fixtures/previews/tiny.webp");

#[cfg(test)]
mod tests {
    use super::*;

    fn colour(p: &Player) -> [u8; 3] {
        match p.frame() {
            Handle::Rgba { pixels, .. } => [pixels[0], pixels[1], pixels[2]],
            other => panic!("{other:?}"),
        }
    }

    /// Each frame shows for its delay, in turn; the last stays and the
    /// decoder is dropped (nothing left to decode, nothing held but it).
    #[test]
    fn a_clip_plays_frame_by_frame_then_rests_on_its_last() {
        let ms = Duration::from_millis;
        let mut p = Player::new(TINY, false).expect("the fixture decodes");
        let first = colour(&p);
        assert!(p.advance(ms(0)) && p.advance(ms(299)));
        assert_eq!(colour(&p), first, "frame 1 until 300 ms");
        assert!(p.advance(ms(300)));
        let second = colour(&p);
        assert_ne!(second, first, "frame 2 from 300 ms");
        assert!(p.advance(ms(599)));
        assert_eq!(colour(&p), second);
        // a late tick skips to the frame due
        assert!(p.advance(ms(1250)));
        let fifth = colour(&p);
        assert!(!p.advance(ms(1500)), "the end: at rest");
        assert_eq!(colour(&p), fifth, "rests on the last frame");
        assert!(p.frames.is_none(), "the decoder is freed");
        assert!(!p.advance(ms(60_000)));
        assert_eq!(colour(&p), fifth);
    }

    /// Two or more clips play one after another and wrap round; the list is
    /// remembered for a refresh check.
    #[test]
    fn a_list_plays_in_turn_and_loops() {
        let ms = Duration::from_millis;
        let mut p = Player::list(vec![TINY, TINY], false).unwrap();
        assert!(p.plays(&[TINY, TINY]) && !p.plays(&[TINY]));
        assert!(p.advance(ms(1400)) && p.current() == 0);
        assert!(p.advance(ms(1600)) && p.current() == 1, "the second clip");
        assert!(p.advance(ms(3100)) && p.current() == 0, "round again");
        assert!(p.advance(ms(100_000)), "never rests");
        // reduced motion: the first frame of the first clip only
        let mut still = Player::list(vec![TINY, TINY], true).unwrap();
        assert!(!still.advance(ms(5000)) && still.current() == 0);
    }

    /// Reduced motion: the first frame, and nothing plays.
    #[test]
    fn a_still_clip_shows_its_first_frame() {
        let moving = Player::new(TINY, false).unwrap();
        let mut still = Player::new(TINY, true).unwrap();
        assert!(!still.playing() && !still.advance(Duration::from_secs(9)));
        assert_eq!(colour(&still), colour(&moving));
    }

    #[test]
    fn a_broken_clip_is_none() {
        assert!(Player::new(b"RIFF....WEBPnope", false).is_none());
        assert!(Player::new(&TINY[..40], false).is_none());
    }
}
