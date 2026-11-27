//! Linux: device enumeration and record streams over the PulseAudio native
//! protocol (pure Rust, no libpulse), which PipeWire's pulse server speaks too.
//! The sound server resamples to 48 kHz stereo `f32` for us.

use std::ffi::CString;
use std::io::{BufRead, BufReader, ErrorKind, Read};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use pulseaudio::protocol::{self, Command, ProtocolError, PulseError};

use super::{CHANNELS, Error, Kind, Packet, RATE, RawDevice, frames_duration};

const DEFAULT_MONITOR: &str = "@DEFAULT_MONITOR@";
const DEFAULT_SOURCE: &str = "@DEFAULT_SOURCE@";
/// Bytes per stereo f32 frame (2 channels * 4 bytes).
const FRAME_BYTES: usize = CHANNELS as usize * 4;
/// Target capture packet cadence in milliseconds (~20 ms keeps meter & mixer responsive).
const FRAGMENT_MS: u32 = 20;
/// Bytes per fragment: 48 kHz stereo f32 = 384 B/ms -> 7,680 B for 20 ms.
const FRAGMENT_BYTES: u32 = (RATE / 1000) * (FRAME_BYTES as u32) * FRAGMENT_MS;
/// Timeout once a packet header has started arriving so we never tear a message.
const BODY_TIMEOUT: Duration = Duration::from_secs(2);

#[allow(dead_code)] // called by `audio::devices`
pub(super) fn list_devices(kind: &Kind) -> Result<(Vec<RawDevice>, Option<String>), Error> {
    let (mut sock, version) = connect()?;
    let fail =
        |e: ProtocolError| Error::Unavailable(format!("{e} (is PulseAudio or PipeWire running?)"));

    protocol::write_command_message(sock.get_mut(), 2, &Command::GetServerInfo, version)
        .map_err(fail)?;
    let (_, server) =
        protocol::read_reply_message::<protocol::ServerInfo>(&mut sock, version).map_err(fail)?;

    protocol::write_command_message(sock.get_mut(), 3, &Command::GetSourceInfoList, version)
        .map_err(fail)?;
    let (_, sources) = protocol::read_reply_message::<protocol::SourceInfoList>(&mut sock, version)
        .map_err(fail)?;

    Ok(map_pulse_sources(&server, &sources, kind))
}

/// Maps PulseAudio `ServerInfo` + `SourceInfo` list into raw microphone or
/// system-monitor devices plus the default device id.
#[allow(dead_code)] // used by `list_devices` and unit tests
fn map_pulse_sources(
    server: &protocol::ServerInfo,
    sources: &[protocol::SourceInfo],
    kind: &Kind,
) -> (Vec<RawDevice>, Option<String>) {
    let want_monitor = matches!(kind, Kind::System);
    let default_sink = server
        .default_sink_name
        .as_ref()
        .map(|s| s.to_string_lossy().into_owned());
    let default_source = server
        .default_source_name
        .as_ref()
        .map(|s| s.to_string_lossy().into_owned());

    let mut raw = Vec::new();
    let mut default_id = None;

    for src in sources {
        let is_monitor = src.monitor_of_sink_index.is_some() || src.monitor_of_sink_name.is_some();
        if is_monitor != want_monitor {
            continue;
        }
        let id = src.name.to_string_lossy().into_owned();
        let name = src
            .description
            .as_ref()
            .map(|d| d.to_string_lossy().into_owned())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| id.clone());

        if want_monitor {
            let matches_default_sink = default_sink.as_deref().is_some_and(|sink| {
                src.monitor_of_sink_name
                    .as_ref()
                    .is_some_and(|m| m.to_string_lossy() == sink)
                    || id == format!("{sink}.monitor")
            });
            if matches_default_sink {
                default_id = Some(id.clone());
            }
        } else if default_source.as_deref() == Some(id.as_str()) {
            default_id = Some(id.clone());
        }

        raw.push(RawDevice {
            id,
            name,
            is_default: false,
        });
    }

    (raw, default_id)
}

pub(super) struct PulseInput {
    sock: BufReader<UnixStream>,
    version: u16,
    raw: Vec<u8>,
    rem: Vec<u8>,
}

impl PulseInput {
    pub(super) fn open(kind: Kind) -> Result<Self, Error> {
        let source_str = match &kind {
            Kind::System => DEFAULT_MONITOR,
            Kind::Mic(None) => DEFAULT_SOURCE,
            Kind::Mic(Some(id)) if id.is_empty() => DEFAULT_SOURCE,
            Kind::Mic(Some(id)) => id.as_str(),
        };
        let source =
            CString::new(source_str).map_err(|_| Error::NotFound(source_str.to_string()))?;

        let (mut sock, version) = connect()?;
        let params = record_stream_params(source);
        protocol::write_command_message(
            sock.get_mut(),
            2,
            &Command::CreateRecordStream(params),
            version,
        )
        .map_err(|e| map_proto_err(e, source_str))?;
        protocol::read_reply_message::<protocol::CreateRecordStreamReply>(&mut sock, version)
            .map_err(|e| map_proto_err(e, source_str))?;

        Ok(Self {
            sock,
            version,
            raw: Vec::new(),
            rem: Vec::with_capacity(FRAME_BYTES),
        })
    }

    pub(super) fn read(&mut self, wait: Duration) -> Result<Option<Packet>, Error> {
        let deadline = Instant::now() + wait;
        let mut first = true;
        loop {
            let rem = deadline.saturating_duration_since(Instant::now());
            if !first && rem.is_zero() {
                return Ok(None);
            }
            first = false;
            let _ = self
                .sock
                .get_ref()
                .set_read_timeout(Some(rem.max(Duration::from_millis(1))));

            // Wait for bytes without consuming any header: a timeout here is safe
            // to retry (e.g. a suspended sink sends nothing until audio plays).
            match self.sock.fill_buf().map(|b| b.is_empty()) {
                Ok(false) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Ok(None);
                }
                Ok(true) => {
                    return Err(Error::Unavailable(
                        "sound server closed the audio stream".into(),
                    ));
                }
                Err(e) => return Err(Error::Unavailable(e.to_string())),
            }

            let _ = self.sock.get_ref().set_read_timeout(Some(BODY_TIMEOUT));
            let desc = protocol::read_descriptor(&mut self.sock)
                .map_err(|e| Error::Unavailable(e.to_string()))?;
            if desc.channel == u32::MAX {
                // Server event (moved, suspended, corked...): read past it.
                Command::read_tag_prefixed(&mut self.sock, self.version)
                    .map_err(|e| Error::Unavailable(e.to_string()))?;
                continue;
            }
            let n = desc.length as usize;
            let prefix = self.rem.len();
            self.raw.clear();
            self.raw.extend_from_slice(&self.rem);
            self.rem.clear();
            self.raw.resize(prefix + n, 0);
            self.sock
                .read_exact(&mut self.raw[prefix..])
                .map_err(|e| Error::Unavailable(e.to_string()))?;

            let usable = (self.raw.len() / FRAME_BYTES) * FRAME_BYTES;
            if usable < self.raw.len() {
                self.rem.extend_from_slice(&self.raw[usable..]);
            }
            if usable == 0 {
                continue;
            }
            let samples: Vec<f32> = self.raw[..usable]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect();
            let frames = samples.len() / CHANNELS as usize;
            let now = Instant::now();
            let at = now.checked_sub(frames_duration(frames)).unwrap_or(now);
            return Ok(Some(Packet { samples, at }));
        }
    }
}

/// Builds the PulseAudio record-stream request with ~20 ms fragments and
/// `adjust_latency` enabled so meters and mixers receive timely packets rather
/// than the server's ~2 s default.
fn record_stream_params(source: CString) -> protocol::RecordStreamParams {
    protocol::RecordStreamParams {
        source_name: Some(source),
        sample_spec: protocol::SampleSpec {
            format: protocol::SampleFormat::Float32Le,
            channels: CHANNELS as u8,
            sample_rate: RATE,
        },
        channel_map: protocol::ChannelMap::stereo(),
        buffer_attr: protocol::stream::BufferAttr {
            fragment_size: FRAGMENT_BYTES,
            ..Default::default()
        },
        flags: protocol::stream::StreamFlags {
            adjust_latency: true,
            ..Default::default()
        },
        cvolume: Some(protocol::ChannelVolume::norm(CHANNELS as u8)),
        ..Default::default()
    }
}

/// Connects, authenticates and sets the client name. Returns `(sock, protocol_version)`.
fn connect() -> Result<(BufReader<UnixStream>, u16), Error> {
    let fail =
        |what: &str| Error::Unavailable(format!("{what} (is PulseAudio or PipeWire running?)"));
    let path = pulseaudio::socket_path_from_env().ok_or_else(|| fail("no sound server"))?;
    let stream =
        UnixStream::connect(path).map_err(|_| fail("can't connect to the sound server"))?;
    let _ = stream.set_read_timeout(Some(BODY_TIMEOUT));
    let mut sock = BufReader::new(stream);

    let cookie = pulseaudio::cookie_path_from_env()
        .and_then(|p| std::fs::read(p).ok())
        .unwrap_or_default();
    let auth = protocol::AuthParams {
        version: protocol::MAX_VERSION,
        supports_shm: false,
        supports_memfd: false,
        cookie,
    };
    let max = protocol::MAX_VERSION;
    protocol::write_command_message(sock.get_mut(), 0, &Command::Auth(auth), max)
        .map_err(|e| fail(&e.to_string()))?;
    let (_, reply) = protocol::read_reply_message::<protocol::AuthReply>(&mut sock, max)
        .map_err(|e| fail(&e.to_string()))?;
    let version = max.min(reply.version);

    let mut props = protocol::Props::new();
    props.set(protocol::Prop::ApplicationName, c"OpenCircle");
    protocol::write_command_message(sock.get_mut(), 1, &Command::SetClientName(props), version)
        .map_err(|e| fail(&e.to_string()))?;
    protocol::read_reply_message::<protocol::SetClientNameReply>(&mut sock, version)
        .map_err(|e| fail(&e.to_string()))?;
    Ok((sock, version))
}

fn map_proto_err(err: ProtocolError, source: &str) -> Error {
    match err {
        ProtocolError::ServerError(PulseError::NoEntity) => Error::NotFound(source.to_string()),
        other => Error::Unavailable(format!("{other} (is PulseAudio or PipeWire running?)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::platform::audio::map_devices;

    fn source(
        name: &str,
        desc: Option<&str>,
        monitor_of: Option<(u32, &str)>,
    ) -> protocol::SourceInfo {
        protocol::SourceInfo {
            name: CString::new(name).unwrap(),
            description: desc.map(|d| CString::new(d).unwrap()),
            monitor_of_sink_index: monitor_of.map(|(i, _)| i),
            monitor_of_sink_name: monitor_of.map(|(_, s)| CString::new(s).unwrap()),
            ..Default::default()
        }
    }

    #[test]
    fn pulse_sources_separate_mics_from_monitors_and_pick_defaults() {
        let server = protocol::ServerInfo {
            default_sink_name: Some(CString::new("alsa_output.hdmi").unwrap()),
            default_source_name: Some(CString::new("alsa_input.usb_yeti").unwrap()),
            ..Default::default()
        };
        let sources = vec![
            source("alsa_input.internal", Some("Internal Mic"), None),
            source("alsa_input.usb_yeti", Some("Yeti Stereo"), None),
            source(
                "alsa_output.analog.monitor",
                Some("Monitor of Analog"),
                Some((0, "alsa_output.analog")),
            ),
            source(
                "alsa_output.hdmi.monitor",
                None,
                Some((1, "alsa_output.hdmi")),
            ),
        ];

        let (raw_mics, def_mic) = map_pulse_sources(&server, &sources, &Kind::Mic(None));
        let mics = map_devices(raw_mics, def_mic.as_deref());
        assert_eq!(mics.len(), 2);
        assert_eq!(mics[0].id, "alsa_input.usb_yeti");
        assert_eq!(mics[0].name, "Yeti Stereo");
        assert!(mics[0].is_default);
        assert_eq!(mics[1].id, "alsa_input.internal");
        assert!(!mics[1].is_default);

        let (raw_sys, def_sys) = map_pulse_sources(&server, &sources, &Kind::System);
        let sys = map_devices(raw_sys, def_sys.as_deref());
        // The default-only filter for Kind::System lives in `devices_from`.
        let hdmi = sys
            .iter()
            .find(|d| d.id == "alsa_output.hdmi.monitor")
            .unwrap();
        assert!(hdmi.is_default);
    }

    #[test]
    fn record_stream_params_requests_20ms_fragments_with_adjust_latency() {
        let params = record_stream_params(CString::new(DEFAULT_MONITOR).unwrap());
        let frag = params.buffer_attr.fragment_size;
        assert_ne!(
            frag,
            u32::MAX,
            "fragment_size must not be left at server default (~2 s)"
        );
        assert_eq!(
            frag as usize % FRAME_BYTES,
            0,
            "fragment_size must align to stereo f32 frames"
        );
        let bytes_per_ms = RATE * FRAME_BYTES as u32 / 1000;
        assert!(
            (bytes_per_ms * 10..=bytes_per_ms * 40).contains(&frag),
            "fragment_size={frag} B outside 10..=40 ms ({}..={} B)",
            bytes_per_ms * 10,
            bytes_per_ms * 40
        );
        assert_eq!(frag, 7_680);
        assert!(params.flags.adjust_latency);
    }
}
