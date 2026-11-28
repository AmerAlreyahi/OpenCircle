//! Audio input capture (microphone and system loopback/monitor): one interface,
//! the OS calls behind it (`wasapi.rs` on Windows via cpal, `pulse.rs` on Linux
//! over the PulseAudio protocol, which PipeWire speaks too).
//!
//! All streams deliver interleaved stereo 48 kHz `f32` PCM in `-1.0..=1.0`,
//! stamped with a monotonic capture `Instant`. Devices are opened on demand and
//! released when dropped so idle cost stays zero.

mod channel;
#[cfg(target_os = "linux")]
mod pulse;
pub mod resample;
#[cfg(windows)]
mod wasapi;

use std::collections::HashMap;
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

/// An available audio capture or loopback endpoint for the device picker.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // read by Settings / recording picker (R-04)
pub struct Device {
    /// Stable platform identifier (WASAPI `DeviceId` string; PulseAudio source name).
    pub id: String,
    /// Human-readable label shown to the user.
    pub name: String,
    /// Whether this is the OS's current default device for its [`Kind`].
    pub is_default: bool,
}

/// Unnormalised device entry produced by a platform backend (or a test fake).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // produced by platform backends and unit tests
pub struct RawDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Platform device-enumeration backend (implemented by WASAPI / PulseAudio,
/// and by fakes in unit tests).
#[allow(dead_code)] // used by `devices` and unit tests
pub trait DeviceBackend {
    /// Returns raw devices for `kind` along with the optional default device id.
    fn list(&self, kind: &Kind) -> Result<(Vec<RawDevice>, Option<String>), Error>;
}

/// Lists available microphones (`Kind::Mic(_)`) or the openable default system
/// sound endpoint (`Kind::System`), with the OS default device first and
/// deterministic ordering thereafter.
///
/// For [`Kind::System`], only the OS default output is returned today because
/// [`Input::open`] opens only the default loopback/monitor. Upgrade path:
/// change `Kind::System` to `Kind::System(Option<String>)`, resolve the chosen
/// output id in `wasapi::WasapiInput::open` and `pulse::PulseInput::open`, and
/// remove the default-only filter here and in the backends.
#[allow(dead_code)] // called by Settings / recording device picker (R-04)
pub fn devices(kind: Kind) -> Result<Vec<Device>, Error> {
    struct OsBackend;
    impl DeviceBackend for OsBackend {
        fn list(&self, kind: &Kind) -> Result<(Vec<RawDevice>, Option<String>), Error> {
            #[cfg(target_os = "linux")]
            return pulse::list_devices(kind);
            #[cfg(windows)]
            return wasapi::list_devices(kind);
        }
    }
    devices_from(&OsBackend, kind)
}

/// Lists and normalises devices from `backend`.
///
/// Restricts [`Kind::System`] to the single openable default output until
/// per-output loopback selection (`Kind::System(Option<String>)`) is added.
#[allow(dead_code)] // used by `devices` and unit tests
pub fn devices_from(backend: &dyn DeviceBackend, kind: Kind) -> Result<Vec<Device>, Error> {
    let (raw, default_id) = backend.list(&kind)?;
    let mut devices = map_devices(raw, default_id.as_deref());
    if matches!(kind, Kind::System) {
        // Only the OS default system output can be opened by `Input::open(Kind::System)`.
        // Upgrade path: add `Option<String>` to `Kind::System` and resolve by id in
        // `wasapi::WasapiInput::open` and `pulse::PulseInput::open`, then remove this filter.
        devices.retain(|d| d.is_default);
    }
    Ok(devices)
}

/// Normalises a backend's raw device list:
/// - trims ids and names; drops entries with empty ids; falls back blank names to `id`
/// - collapses duplicate ids (keeping the first non-empty name and preserving `is_default`)
/// - ensures at most one default (matching `default_id` if present, else the first `is_default`)
/// - sorts deterministically: default first, then case-insensitive name, then id
#[allow(dead_code)] // used by `devices_from` and unit tests
pub fn map_devices(raw: Vec<RawDevice>, default_id: Option<&str>) -> Vec<Device> {
    let default_id = default_id.map(str::trim).filter(|s| !s.is_empty());
    let mut index_by_id: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Device> = Vec::new();

    for entry in raw {
        let id = entry.id.trim().to_string();
        if id.is_empty() {
            continue;
        }
        let name = entry.name.trim();
        let name = if name.is_empty() {
            id.clone()
        } else {
            name.to_string()
        };
        if let Some(&idx) = index_by_id.get(&id) {
            if out[idx].name == out[idx].id && name != id {
                out[idx].name = name;
            }
            out[idx].is_default |= entry.is_default;
        } else {
            index_by_id.insert(id.clone(), out.len());
            out.push(Device {
                id,
                name,
                is_default: entry.is_default,
            });
        }
    }

    if let Some(def) = default_id
        && out.iter().any(|d| d.id == def)
    {
        for d in &mut out {
            d.is_default = d.id == def;
        }
    } else {
        let mut saw_default = false;
        for d in &mut out {
            if d.is_default && !saw_default {
                saw_default = true;
            } else {
                d.is_default = false;
            }
        }
    }

    out.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeBackend {
        mics: Vec<RawDevice>,
        default_mic: Option<String>,
        system: Vec<RawDevice>,
        default_system: Option<String>,
        fail: Option<String>,
    }

    impl DeviceBackend for FakeBackend {
        fn list(&self, kind: &Kind) -> Result<(Vec<RawDevice>, Option<String>), Error> {
            if let Some(msg) = &self.fail {
                return Err(Error::Unavailable(msg.clone()));
            }
            match kind {
                Kind::Mic(_) => Ok((self.mics.clone(), self.default_mic.clone())),
                Kind::System => Ok((self.system.clone(), self.default_system.clone())),
            }
        }
    }

    fn raw(id: &str, name: &str, is_default: bool) -> RawDevice {
        RawDevice {
            id: id.into(),
            name: name.into(),
            is_default,
        }
    }

    #[test]
    fn fake_backend_puts_default_first_and_keeps_ids_stable() {
        let backend_a = FakeBackend {
            mics: vec![
                raw("mic-usb-2", "Yeti Nano", false),
                raw("mic-built-in", "Built-in Mic", false),
                raw("mic-headset", "Zebra Headset", false),
            ],
            default_mic: Some("mic-headset".into()),
            system: vec![
                raw("out-hdmi", "HDMI Display", false),
                raw("out-speakers", "Speakers", false),
            ],
            default_system: Some("out-speakers".into()),
            fail: None,
        };
        // Same devices returned in a completely different backend order.
        let backend_b = FakeBackend {
            mics: vec![
                raw("mic-headset", "Zebra Headset", false),
                raw("mic-built-in", "Built-in Mic", false),
                raw("mic-usb-2", "Yeti Nano", false),
            ],
            default_mic: Some("mic-headset".into()),
            system: vec![
                raw("out-speakers", "Speakers", false),
                raw("out-hdmi", "HDMI Display", false),
            ],
            default_system: Some("out-speakers".into()),
            fail: None,
        };

        let mics_a = devices_from(&backend_a, Kind::Mic(None)).unwrap();
        let mics_b = devices_from(&backend_b, Kind::Mic(None)).unwrap();
        assert_eq!(mics_a, mics_b, "device order and ids must be stable");
        assert_eq!(
            mics_a,
            vec![
                Device {
                    id: "mic-headset".into(),
                    name: "Zebra Headset".into(),
                    is_default: true,
                },
                Device {
                    id: "mic-built-in".into(),
                    name: "Built-in Mic".into(),
                    is_default: false,
                },
                Device {
                    id: "mic-usb-2".into(),
                    name: "Yeti Nano".into(),
                    is_default: false,
                },
            ]
        );

        let sys_a = devices_from(&backend_a, Kind::System).unwrap();
        let sys_b = devices_from(&backend_b, Kind::System).unwrap();
        assert_eq!(sys_a, sys_b);
        assert_eq!(
            sys_a,
            vec![Device {
                id: "out-speakers".into(),
                name: "Speakers".into(),
                is_default: true,
            }]
        );
    }

    #[test]
    fn devices_system_returns_only_openable_default_output() {
        let with_default = FakeBackend {
            mics: Vec::new(),
            default_mic: None,
            system: vec![
                raw("out-hdmi", "HDMI Display", false),
                raw("out-speakers", "Speakers", false),
                raw("out-bt", "Bluetooth", false),
            ],
            default_system: Some("out-speakers".into()),
            fail: None,
        };
        let sys = devices_from(&with_default, Kind::System).unwrap();
        assert_eq!(sys.len(), 1);
        assert_eq!(sys[0].id, "out-speakers");
        assert!(sys[0].is_default);

        // Without a default output, nothing can be opened via Kind::System.
        let no_default = FakeBackend {
            mics: Vec::new(),
            default_mic: None,
            system: vec![raw("out-hdmi", "HDMI Display", false)],
            default_system: None,
            fail: None,
        };
        assert!(devices_from(&no_default, Kind::System).unwrap().is_empty());
    }

    #[test]
    fn map_devices_cleans_names_drops_empty_ids_and_deduplicates() {
        let mapped = map_devices(
            vec![
                raw("   ", "Ghost", true),
                raw("id-b", "   ", false),
                raw("id-a", " Alpha ", false),
                raw("id-a", "Alpha Duplicate", true),
                raw("id-c", "beta", true),
            ],
            None,
        );
        // Empty id dropped; id-a kept default from duplicate; only one default allowed.
        assert_eq!(mapped.len(), 3);
        assert_eq!(mapped[0].id, "id-a");
        assert_eq!(mapped[0].name, "Alpha");
        assert!(mapped[0].is_default);
        assert_eq!(mapped.iter().filter(|d| d.is_default).count(), 1);
        // Blank name falls back to its id.
        let b = mapped.iter().find(|d| d.id == "id-b").unwrap();
        assert_eq!(b.name, "id-b");
    }

    #[test]
    fn fake_backend_propagates_errors() {
        let backend = FakeBackend {
            mics: Vec::new(),
            default_mic: None,
            system: Vec::new(),
            default_system: None,
            fail: Some("sound server down".into()),
        };
        let err = devices_from(&backend, Kind::Mic(None)).unwrap_err();
        assert_eq!(err, Error::Unavailable("sound server down".into()));
    }

    /// Real device enumeration (run on a PC / VM with audio):
    /// `cargo test live_devices_lists_mic_and_system -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_devices_lists_mic_and_system() {
        let mics = devices(Kind::Mic(None)).expect("list mics");
        let sys = devices(Kind::System).expect("list system endpoints");
        crate::log!("mics: {mics:?}");
        crate::log!("system: {sys:?}");
        if let Some(first) = mics.first() {
            assert!(mics.iter().filter(|d| d.is_default).count() <= 1);
            assert!(!first.id.is_empty());
        }
        if let Some(first) = sys.first() {
            assert!(sys.iter().filter(|d| d.is_default).count() <= 1);
            assert!(!first.id.is_empty());
        }
    }

    /// Real 48 kHz stereo capture (play audio first):
    /// `cargo test live_input_reads_48k_stereo_pcm -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_input_reads_48k_stereo_pcm() {
        let mut input = Input::open(Kind::System).expect("open system loopback");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut total_samples = 0usize;
        while Instant::now() < deadline && total_samples < (RATE as usize * 2) / 2 {
            if let Some(pkt) = input.read(Duration::from_millis(200)).expect("read") {
                assert_eq!(pkt.samples.len() % CHANNELS as usize, 0);
                total_samples += pkt.samples.len();
            }
        }
        assert!(
            total_samples > 0,
            "expected audio samples from system output"
        );
    }

    /// Real capture of BOTH sources for 5 s (play a tone on the PC and talk or tap the mic):
    /// `cargo test live_both_inputs_report_peaks -- --ignored --nocapture`. Windows R04C: the
    /// loopback peak must be well above -60 dB (0.001) while a tone plays.
    #[test]
    #[ignore]
    fn live_both_inputs_report_peaks() {
        let run = |kind: Kind| {
            std::thread::spawn(move || {
                let label = format!("{kind:?}");
                let mut input = Input::open(kind).expect("open");
                let end = Instant::now() + Duration::from_secs(5);
                let (mut packets, mut frames, mut peak) = (0usize, 0usize, 0.0f32);
                while Instant::now() < end {
                    if let Some(p) = input.read(Duration::from_millis(50)).expect("read") {
                        packets += 1;
                        frames += p.samples.len() / 2;
                        peak = p.samples.iter().fold(peak, |m, s| m.max(s.abs()));
                    }
                }
                println!("{label}: {packets} packets, {frames} frames, peak {peak:.5}");
                (frames, peak)
            })
        };
        let (mic, sys) = (run(Kind::Mic(None)), run(Kind::System));
        let (mic, sys) = (mic.join().unwrap(), sys.join().unwrap());
        assert!(
            mic.0 > 0 && sys.0 > 0,
            "no frames: mic {mic:?} system {sys:?}"
        );
    }

    /// Every listed microphone opened BY ITS ID, 2 s each (the picker's path): name, default and peak.
    /// `cargo test live_each_listed_mic_opens_by_id -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_each_listed_mic_opens_by_id() {
        for d in devices(Kind::Mic(None)).expect("list") {
            let opened = Input::open(Kind::Mic(Some(d.id.clone())));
            let Ok(mut input) = opened else {
                println!(
                    "{} [{}] default {}: OPEN FAILED {:?}",
                    d.name,
                    d.id,
                    d.is_default,
                    opened.err()
                );
                continue;
            };
            let end = Instant::now() + Duration::from_secs(2);
            let (mut frames, mut peak) = (0usize, 0.0f32);
            while Instant::now() < end {
                if let Ok(Some(p)) = input.read(Duration::from_millis(50)) {
                    frames += p.samples.len() / 2;
                    peak = p.samples.iter().fold(peak, |m, s| m.max(s.abs()));
                }
            }
            println!(
                "{} [{}] default {}: {frames} frames, peak {peak:.5}",
                d.name, d.id, d.is_default
            );
        }
    }
}
