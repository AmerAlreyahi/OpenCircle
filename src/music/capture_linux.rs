//! Linux: a record stream over the PulseAudio protocol (pure Rust, no libpulse),
//! which PipeWire's pulse server speaks too. "What's playing" = the default
//! sink's monitor; the server converts to mono `RATE` for us.

use std::ffi::CStr;
use std::io::{BufRead, BufReader, ErrorKind, Read};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use pulseaudio::protocol::{self, Command};

use super::{Clip, Error, RATE};
use crate::config::SongSource;

pub fn record(
    source: SongSource,
    want: usize,
    deadline: Instant,
    cancel: &AtomicBool,
    heard: &mut dyn FnMut(u32),
) -> Result<Clip, Error> {
    let name: &CStr = match source {
        SongSource::System => c"@DEFAULT_MONITOR@",
        SongSource::Microphone => c"@DEFAULT_SOURCE@",
    };
    let fail = |what: &str| Error::Capture(format!("{what} (is PulseAudio or PipeWire running?)"));
    let path = pulseaudio::socket_path_from_env().ok_or_else(|| fail("no sound server"))?;
    let stream =
        UnixStream::connect(path).map_err(|_| fail("can't connect to the sound server"))?;
    // Short waits, so Cancel and the deadline are checked often.
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let mut sock = BufReader::new(stream);
    let v = open(&mut sock, name).map_err(|e| fail(&e))?;

    let started = Instant::now();
    let mut bytes = Vec::with_capacity(want * 2);
    let mut buf = Vec::new();
    let mut said = 0;
    while bytes.len() < want * 2 && Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
        let secs = started.elapsed().as_secs() as u32;
        if secs > said {
            said = secs;
            heard(secs);
        }
        // Wait for data without taking any: a timeout here is safe to retry (the
        // stream takes a moment to start; a suspended sink sends nothing).
        match sock.fill_buf().map(|b| b.is_empty()) {
            Ok(false) => {}
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Ok(true) | Err(_) => break, // closed
        }
        // A timeout inside a message would leave it half read: stop with what came in.
        let Ok(desc) = protocol::read_descriptor(&mut sock) else {
            break;
        };
        if desc.channel == u32::MAX {
            // A server message (moved, suspended...): read past it.
            if Command::read_tag_prefixed(&mut sock, v).is_err() {
                break;
            }
            continue;
        }
        buf.resize(desc.length as usize, 0);
        if sock.read_exact(&mut buf).is_err() {
            break;
        }
        bytes.extend_from_slice(&buf);
    }
    bytes.truncate(want * 2);
    let samples = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
        .collect();
    Ok(Clip(samples)) // dropping the socket ends the stream
}

/// Auth, client name, then a mono `RATE` record stream on `source`. Returns
/// the protocol version.
fn open(sock: &mut BufReader<UnixStream>, source: &CStr) -> Result<u16, String> {
    let e = |e: protocol::ProtocolError| e.to_string();
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
    protocol::write_command_message(sock.get_mut(), 0, &Command::Auth(auth), max).map_err(e)?;
    let (_, reply) = protocol::read_reply_message::<protocol::AuthReply>(sock, max).map_err(e)?;
    let v = max.min(reply.version);
    let mut props = protocol::Props::new();
    props.set(protocol::Prop::ApplicationName, c"OpenCircle");
    protocol::write_command_message(sock.get_mut(), 1, &Command::SetClientName(props), v)
        .map_err(e)?;
    protocol::read_reply_message::<protocol::SetClientNameReply>(sock, v).map_err(e)?;
    let params = protocol::RecordStreamParams {
        source_name: Some(source.to_owned()),
        sample_spec: protocol::SampleSpec {
            format: protocol::SampleFormat::S16Le,
            channels: 1,
            sample_rate: RATE,
        },
        channel_map: protocol::ChannelMap::mono(),
        cvolume: Some(protocol::ChannelVolume::norm(1)),
        ..Default::default()
    };
    protocol::write_command_message(sock.get_mut(), 2, &Command::CreateRecordStream(params), v)
        .map_err(e)?;
    protocol::read_reply_message::<protocol::CreateRecordStreamReply>(sock, v).map_err(e)?;
    Ok(v)
}
