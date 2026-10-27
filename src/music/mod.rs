//! Song search (opt-in): listen ~10 s to what this PC plays (or the microphone),
//! send that clip to the user's own recognition service, show the song.
//!
//! Audio is captured only while listening, kept in memory, and sent only when
//! the user pressed the music button (or its quick action). Free fingerprint
//! lookups can't match a clip (spikes/music), so this needs a service key: AudD
//! for now (`audd.rs`); another service = one more file + a `Service` variant.
//! One capture file per platform: `capture_linux.rs` (the default sink's monitor
//! over the PulseAudio protocol, also PipeWire), `capture_windows.rs` (WASAPI
//! loopback through cpal).

mod audd;
#[cfg_attr(target_os = "linux", path = "capture_linux.rs")]
#[cfg_attr(windows, path = "capture_windows.rs")]
mod capture;
pub mod history;

use std::fmt;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crate::config::SongSource;
use crate::secrets::{self, Secret, SecretsError};

/// How long one search listens.
pub const LISTEN_SECS: u32 = 10;
/// What is sent: mono 16-bit PCM at this rate (10 s = 240 KB as WAV). Enough
/// for recognition (tried at 8 and 12 kHz); WAV because we have no encoder.
pub const RATE: u32 = 12_000;
/// Quieter than this (peak, of 32767) all clip long = nothing is playing.
const SILENCE: u16 = 300;

/// A recognition service; the user brings the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Audd,
}

impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Service::Audd => "AudD",
        }
    }

    /// Its keyring entry.
    pub fn key_entry(self) -> &'static str {
        match self {
            Service::Audd => "song-key-audd",
        }
    }
}

/// The one service for now.
pub const SERVICE: Service = Service::Audd;

/// A recorded clip: mono samples at `RATE`.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip(pub Vec<i16>);

impl Clip {
    /// As a WAV file (what the service gets).
    pub fn wav(&self) -> Vec<u8> {
        let data = (self.0.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&RATE.to_le_bytes());
        out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for s in &self.0 {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    fn silent(&self) -> bool {
        self.0.iter().all(|s| s.unsigned_abs() < SILENCE)
    }
}

/// Samples wanted for `secs` of audio (the capture buffer's size).
pub fn wanted(secs: u32) -> usize {
    (secs * RATE) as usize
}

/// Turns any capture format into mono at `RATE`: channels are averaged, and
/// each output sample is the mean of the input samples in its period (a box
/// low-pass, so 48 kHz down to 12 kHz doesn't alias much). Windows' capture
/// uses it; on Linux the sound server converts.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Downmix {
    channels: usize,
    /// Input frames per output sample.
    step: f64,
    /// Input frames since the last output sample, and their sum.
    pos: f64,
    sum: f64,
    n: u32,
    pub out: Vec<i16>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Downmix {
    pub fn new(rate: u32, channels: u16, capacity: usize) -> Downmix {
        Downmix {
            channels: channels.max(1) as usize,
            step: f64::from(rate) / f64::from(RATE),
            pos: 0.0,
            sum: 0.0,
            n: 0,
            out: Vec::with_capacity(capacity),
        }
    }

    /// Interleaved samples in -1..1.
    pub fn push(&mut self, samples: impl IntoIterator<Item = f32>) {
        let mut frame = 0.0;
        let mut i = 0;
        for s in samples {
            frame += s;
            i += 1;
            if i < self.channels {
                continue;
            }
            self.sum += f64::from(frame) / self.channels as f64;
            self.n += 1;
            self.pos += 1.0;
            (frame, i) = (0.0, 0);
            if self.pos >= self.step && self.out.len() < self.out.capacity() {
                let v = (self.sum / f64::from(self.n)).clamp(-1.0, 1.0);
                self.out.push((v * 32767.0) as i16);
                self.pos -= self.step;
                (self.sum, self.n) = (0.0, 0);
            }
        }
    }

    pub fn full(&self) -> bool {
        self.out.len() >= self.out.capacity()
    }
}

/// A found song.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// "2014", or "" when unknown.
    pub year: String,
    /// A PNG cover image to download, if the service has one.
    pub cover: Option<String>,
    /// Where to listen: (label, https URL), opened only on click.
    pub links: Vec<(&'static str, String)>,
}

impl Song {
    /// "Album · 2014", whichever is known.
    pub fn detail(&self) -> String {
        [self.album.as_str(), self.year.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// A YouTube search for the song (a plain search URL: nothing is sent until clicked).
fn youtube(artist: &str, title: &str) -> String {
    let q: String = format!("{artist} {title}")
        .bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b' ' => "+".into(),
            b => format!("%{b:02X}"),
        })
        .collect();
    format!("https://www.youtube.com/results?search_query={q}")
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// No key saved (or the keyring said why).
    NoKey(String),
    /// Nothing audible came in.
    Silent,
    /// The audio system couldn't be opened.
    Capture(String),
    /// Heard, but the service doesn't know the song.
    NotFound,
    /// The service refused the key.
    BadKey,
    /// No connection to the service.
    Offline,
    /// Any other answer from the service (its own words).
    Service(String),
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = SERVICE.name();
        match self {
            Error::NoKey(why) if why.is_empty() => write!(
                f,
                "Add your {name} key in Settings > Song search to identify songs"
            ),
            Error::NoKey(why) => f.write_str(why),
            Error::Silent => f.write_str("No sound is playing. Play the song, then try again"),
            Error::Capture(why) => write!(f, "Can't listen to the audio: {why}"),
            Error::NotFound => f.write_str("No match. Try again while the song plays clearly"),
            Error::BadKey => write!(f, "{name} didn't accept the key. Check it in Settings"),
            Error::Offline => write!(f, "Can't reach {name}. Check your connection"),
            Error::Service(why) => write!(f, "{name}: {why}"),
            Error::Cancelled => f.write_str("Cancelled"),
        }
    }
}

// ---- is a key saved? (the music button shows only then) ----

const UNKNOWN: u8 = 0;
const NO: u8 = 1;
const YES: u8 = 2;
static HAS_KEY: AtomicU8 = AtomicU8::new(UNKNOWN);

/// `None` until `check_key` ran once.
pub fn key_known() -> Option<bool> {
    match HAS_KEY.load(Ordering::Relaxed) {
        UNKNOWN => None,
        v => Some(v == YES),
    }
}

/// Settings saved or removed the key.
pub fn remember_key(saved: bool) {
    HAS_KEY.store(if saved { YES } else { NO }, Ordering::Relaxed);
}

/// Asks the keyring whether a key is saved (blocking, no unlock prompt) and remembers it.
pub fn check_key() -> bool {
    let saved = secrets::by_name::exists(SERVICE.key_entry()).unwrap_or(false);
    remember_key(saved);
    saved
}

fn key() -> Result<Secret, Error> {
    match secrets::by_name::get(SERVICE.key_entry()) {
        Ok(Some(k)) if !k.is_empty() => Ok(k),
        Ok(_) => Err(Error::NoKey(String::new())),
        Err(e @ (SecretsError::Unavailable(_) | SecretsError::Failed(_))) => {
            Err(Error::NoKey(e.to_string()))
        }
    }
}

// ---- the search ----

/// Listens for `LISTEN_SECS` (or until `cancel`), calling `heard(seconds)` about
/// once a second. Blocking: run it on a thread.
pub fn listen(
    source: SongSource,
    cancel: &AtomicBool,
    heard: &mut dyn FnMut(u32),
) -> Result<Clip, Error> {
    let deadline = Instant::now() + Duration::from_secs(u64::from(LISTEN_SECS));
    let clip = capture::record(source, wanted(LISTEN_SECS), deadline, cancel, heard)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    // Under 3 s heard, or all quiet: nothing to recognise.
    if clip.0.len() < wanted(3) || clip.silent() {
        return Err(Error::Silent);
    }
    Ok(clip)
}

/// Sends the clip to the service with the saved key. Blocking.
pub fn identify(clip: &Clip) -> Result<Song, Error> {
    let key = key()?;
    match SERVICE {
        Service::Audd => audd::identify(&key, clip),
    }
}

/// The cover as RGBA (width, height, pixels); `None` on any problem (the card
/// shows a music note instead).
pub fn cover(url: &str) -> Option<(u32, u32, Vec<u8>)> {
    if !url.starts_with("https://") {
        return None;
    }
    let agent = crate::net::agent(crate::net::COVER);
    let bytes = agent
        .get(url)
        .call()
        .ok()?
        .body_mut()
        .with_config()
        .limit(2 << 20)
        .read_to_vec()
        .ok()?;
    let frame = crate::capture::decode_png(Cursor::new(bytes)).ok()?;
    Some((frame.width, frame.height, frame.rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine at `hz`, interleaved over `channels`, at `rate`.
    fn tone(hz: f32, rate: u32, channels: u16, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
                std::iter::repeat_n(v, channels as usize)
            })
            .collect()
    }

    fn crossings(s: &[i16]) -> usize {
        s.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count()
    }

    #[test]
    fn capture_buffer_holds_exactly_the_listening_time() {
        assert_eq!(wanted(LISTEN_SECS), 120_000);
        // 48 kHz stereo (Windows' usual mix format), more than wanted: stops full.
        let mut d = Downmix::new(48_000, 2, wanted(1));
        d.push(tone(440.0, 48_000, 2, 1.5));
        assert!(d.full());
        assert_eq!(d.out.len(), wanted(1));
        assert_eq!(d.out.capacity(), wanted(1), "never grows past the buffer");
    }

    #[test]
    fn downmix_keeps_the_pitch_at_any_rate() {
        for (rate, ch) in [(48_000, 2), (44_100, 2), (12_000, 1), (96_000, 6)] {
            let mut d = Downmix::new(rate, ch, wanted(2));
            d.push(tone(1000.0, rate, ch, 1.0));
            let n = d.out.len() as f32;
            assert!((n - RATE as f32).abs() <= 2.0, "{rate}: {n} samples");
            // 1 kHz = 2000 sign changes a second.
            let c = crossings(&d.out) as f32;
            assert!((c - 2000.0).abs() < 20.0, "{rate} Hz x{ch}: {c}");
            let peak = d.out.iter().map(|s| s.unsigned_abs()).max().unwrap();
            assert!(peak > 12_000, "{rate}: level kept ({peak})");
        }
    }

    #[test]
    fn quiet_clips_count_as_silence() {
        assert!(Clip(vec![0; 100]).silent());
        assert!(Clip(vec![120, -250, 90]).silent());
        assert!(!Clip(vec![0, 4000, -4000]).silent());
    }

    #[test]
    fn wav_header_is_mono_16_bit_at_rate() {
        let wav = Clip(vec![1, -2, 3]).wav();
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes(wav[24..28].try_into().unwrap()),
            RATE,
            "rate"
        );
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16, "bits");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(&wav[44..], &[1, 0, 0xFE, 0xFF, 3, 0]);
        // A full search is ~240 KB.
        assert_eq!(Clip(vec![0; wanted(LISTEN_SECS)]).wav().len(), 240_044);
    }

    #[test]
    fn youtube_link_is_a_plain_search() {
        assert_eq!(
            youtube("Kevin MacLeod", "Wallpaper & Co?"),
            "https://www.youtube.com/results?search_query=Kevin+MacLeod+Wallpaper+%26+Co%3F"
        );
        assert!(crate::lens::web_host(&youtube("Ünïcode", "ü")).is_some());
    }

    #[test]
    fn errors_say_what_to_do() {
        assert!(Error::NoKey(String::new()).to_string().contains("Settings"));
        assert!(Error::Silent.to_string().contains("No sound"));
        assert!(Error::Offline.to_string().contains("connection"));
        assert!(Error::NotFound.to_string().contains("No match"));
        assert!(Error::BadKey.to_string().contains("key"));
    }

    /// Real capture (play something first): `cargo test live_capture -- --ignored`.
    /// `OPENCIRCLE_TEST_CLIP=<out.wav>` keeps the clip to check it.
    #[test]
    #[ignore]
    fn live_capture() {
        let cancel = AtomicBool::new(false);
        let mut ticks = Vec::new();
        let t = Instant::now();
        let clip = listen(SongSource::System, &cancel, &mut |s| ticks.push(s)).unwrap();
        let peak = clip.0.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        crate::log!(
            "heard {:.2} s in {} ms, peak {peak}, ticks {ticks:?}",
            clip.0.len() as f32 / RATE as f32,
            t.elapsed().as_millis()
        );
        if let Some(out) = std::env::var_os("OPENCIRCLE_TEST_CLIP") {
            std::fs::write(out, clip.wav()).unwrap();
        }
        assert!(clip.0.len() >= wanted(LISTEN_SECS - 2));
    }
}
