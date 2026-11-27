//! Linux: record streams over the PulseAudio native protocol (pure Rust, no
//! libpulse), which PipeWire's pulse server speaks too. The sound server
//! resamples to 48 kHz stereo `f32` for us.

use std::ffi::CString;
use std::io::{BufRead, BufReader, ErrorKind, Read};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use pulseaudio::protocol::{self, Command, ProtocolError, PulseError};

use super::{CHANNELS, Error, Kind, Packet, RATE, frames_duration};

const DEFAULT_MONITOR: &str = "@DEFAULT_MONITOR@";
const DEFAULT_SOURCE: &str = "@DEFAULT_SOURCE@";
/// Bytes per stereo f32 frame (2 channels * 4 bytes).
const FRAME_BYTES: usize = CHANNELS as usize * 4;
/// Timeout once a packet header has started arriving so we never tear a message.
const BODY_TIMEOUT: Duration = Duration::from_secs(2);

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
        let params = protocol::RecordStreamParams {
            source_name: Some(source),
            sample_spec: protocol::SampleSpec {
                format: protocol::SampleFormat::Float32Le,
                channels: CHANNELS as u8,
                sample_rate: RATE,
            },
            channel_map: protocol::ChannelMap::stereo(),
            cvolume: Some(protocol::ChannelVolume::norm(CHANNELS as u8)),
            ..Default::default()
        };
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
