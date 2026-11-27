//! Audio input capture (microphone and system loopback/monitor): one interface,
//! the OS calls behind it (`wasapi.rs` on Windows via cpal, `pulse.rs` on Linux
//! over the PulseAudio protocol, which PipeWire speaks too).
//!
//! All streams deliver interleaved stereo 48 kHz `f32` PCM in `-1.0..=1.0`,
//! stamped with a monotonic capture `Instant`. Devices are opened on demand and
//! released when dropped so idle cost stays zero.

#[cfg(target_os = "linux")]
mod pulse;
pub mod resample;
#[cfg(windows)]
mod wasapi;

use std::fmt;
use std::time::{Duration, Instant};

/// Standard capture sample rate in Hz (48 kHz: matches WASAPI mix rate, Opus and AAC).
pub const RATE: u32 = 48_000;
/// Standard capture channel count (interleaved stereo `[L, R]`).
pub const CHANNELS: u16 = 2;

/// Which audio source to list or capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A microphone: `None` = OS default input, `Some(id)` = a specific device id.
    Mic(Option<String>),
    /// System sound (Windows WASAPI loopback; Linux PulseAudio/PipeWire monitor source).
    System,
}

/// One chunk of captured audio: interleaved stereo 48 kHz `f32` PCM.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    /// Interleaved `[L0, R0, L1, R1, ...]` samples in `-1.0..=1.0` at [`RATE`].
    pub samples: Vec<f32>,
    /// Monotonic timestamp of the start of this packet.
    #[allow(dead_code)] // read by recording mixer (R-03/R-04)
    pub at: Instant,
}

/// Why opening, listing, or reading audio failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Audio host or sound server is unavailable, or the stream disconnected.
    Unavailable(String),
    /// A requested device id was not found.
    #[allow(dead_code)] // returned when opening a specific device id
    NotFound(String),
    /// The device reported an unsupported stream format.
    #[allow(dead_code)] // returned by WASAPI on exotic sample formats
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unavailable(why) => f.write_str(why),
            Error::NotFound(id) => write!(f, "audio device not found: {id}"),
            Error::Unsupported(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

/// An open audio capture stream yielding stereo 48 kHz `f32` packets.
/// Dropping it closes the underlying OS stream immediately.
pub struct Input {
    #[cfg(target_os = "linux")]
    inner: pulse::PulseInput,
    #[cfg(windows)]
    inner: wasapi::WasapiInput,
}

impl Input {
    /// Opens a live capture stream for `kind`.
    pub fn open(kind: Kind) -> Result<Self, Error> {
        #[cfg(target_os = "linux")]
        let inner = pulse::PulseInput::open(kind)?;
        #[cfg(windows)]
        let inner = wasapi::WasapiInput::open(kind)?;
        Ok(Self { inner })
    }

    /// Waits up to `wait` for the next audio packet.
    ///
    /// Returns `Ok(None)` when `wait` elapses with no audio arriving (for example
    /// while system loopback is silent or a sink is suspended).
    pub fn read(&mut self, wait: Duration) -> Result<Option<Packet>, Error> {
        self.inner.read(wait)
    }
}

/// Duration covered by `frames` stereo frames at [`RATE`].
pub(crate) fn frames_duration(frames: usize) -> Duration {
    Duration::from_nanos((frames as u64).saturating_mul(1_000_000_000) / u64::from(RATE))
}
